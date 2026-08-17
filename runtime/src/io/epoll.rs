#![allow(dead_code)]

//! Linux IO multiplexing — based on epoll (readiness model, level-triggered).
//!
//! ## Model
//!
//! `epoll_create1` creates the instance; `epoll_ctl(ADD/MOD/DEL)` manages fd
//! interest; `epoll_wait` blocks waiting for readiness events. level-triggered
//! (no `EPOLLET`) — if data is still readable, the next `epoll_wait` will
//! report it again.
//!
//! ## FFI
//!
//! Minimal declaration set — `epoll_create1` / `epoll_ctl` / `epoll_wait` +
//! `__errno_location`. Zero external dependencies (the libc crate is not
//! linked; the symbols are declared directly). epoll is a Linux-specific
//! syscall and can be reached via FFI directly through VDSO / syscall
//! wrappers.

use alloc::vec::Vec;
use core::time::Duration;

use crate::io::{IoError, IoEvent, IoInterest, IoReactor, IoToken, RawIo};

// ── epoll constants (sys/epoll.h) ──────────────────────────────────────────
const EPOLL_CLOEXEC: i32 = 0x80000;
const EPOLL_CTL_ADD: i32 = 1;
const EPOLL_CTL_DEL: i32 = 2;
const EPOLL_CTL_MOD: i32 = 3;

const EPOLLIN: u32 = 0x001;
const EPOLLOUT: u32 = 0x004;
const EPOLLERR: u32 = 0x008;
const EPOLLHUP: u32 = 0x010;
const EPOLLRDHUP: u32 = 0x2000;

/// epoll_event (Linux 64-bit): events(u32) + data(u64).
///
/// **Must use `#[repr(C)]`, not `#[repr(C, packed)]`** — the Linux kernel's
/// `struct epoll_event` aligns `data` to 8 bytes on 64-bit (offset 8, with 4
/// bytes of padding before it). `packed` would place `data` at offset 4,
/// causing the kernel-written token to be read from the wrong offset. On
/// x86_64 this happens to work under relaxed alignment; on aarch64 strict
/// alignment would read garbage (misaligned token → test failure).
#[repr(C)]
#[derive(Clone, Copy)]
struct EpollEvent {
    events: u32,
    data: u64,
}

unsafe extern "C" {
    fn epoll_create1(flags: i32) -> i32;
    fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32;
    fn epoll_wait(epfd: i32, events: *mut EpollEvent, maxevents: i32, timeout: i32) -> i32;
    fn __errno_location() -> *mut i32;
}

fn errno() -> i32 {
    unsafe { *__errno_location() }
}

// ── Reactor ────────────────────────────────────────────────────────────────

pub struct EpollReactor {
    epfd: i32,
    /// Reused event buffer (avoids allocating on every poll).
    events: Vec<EpollEvent>,
}

impl EpollReactor {
    pub fn new() -> Result<Self, IoError> {
        let epfd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
        if epfd < 0 {
            return Err(IoError::RegisterFailed { raw_errno: errno() });
        }
        Ok(Self {
            epfd,
            events: (0..64).map(|_| EpollEvent { events: 0, data: 0 }).collect(),
        })
    }

    fn interest_to_events(interest: IoInterest) -> u32 {
        let mut mask: u32 = EPOLLERR | EPOLLHUP;
        if interest.is_readable() {
            mask |= EPOLLIN | EPOLLRDHUP;
        }
        if interest.is_writable() {
            mask |= EPOLLOUT;
        }
        mask
    }

    fn events_to_interest(events: u32) -> IoInterest {
        let mut bits: u8 = 0;
        if events & (EPOLLIN | EPOLLERR | EPOLLHUP | EPOLLRDHUP) != 0 {
            bits |= IoInterest::READABLE.bits();
        }
        if events & EPOLLOUT != 0 {
            bits |= IoInterest::WRITABLE.bits();
        }
        IoInterest::from_bits(bits)
    }
}

impl IoReactor for EpollReactor {
    fn register(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        let mut ev = EpollEvent {
            events: Self::interest_to_events(interest),
            data: token.0 as u64,
        };
        let rc = unsafe { epoll_ctl(self.epfd, EPOLL_CTL_ADD, raw, &mut ev) };
        if rc < 0 {
            return Err(IoError::RegisterFailed { raw_errno: errno() });
        }
        Ok(())
    }

    fn reregister(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        let mut ev = EpollEvent {
            events: Self::interest_to_events(interest),
            data: token.0 as u64,
        };
        let rc = unsafe { epoll_ctl(self.epfd, EPOLL_CTL_MOD, raw, &mut ev) };
        if rc < 0 {
            return Err(IoError::RegisterFailed { raw_errno: errno() });
        }
        Ok(())
    }

    fn deregister(&mut self, raw: RawIo) -> Result<(), IoError> {
        let rc = unsafe { epoll_ctl(self.epfd, EPOLL_CTL_DEL, raw, core::ptr::null_mut()) };
        if rc < 0 {
            return Err(IoError::RegisterFailed { raw_errno: errno() });
        }
        Ok(())
    }

    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<IoEvent>, IoError> {
        let timeout_ms = match timeout {
            None => -1,
            Some(d) => {
                let ms = d.as_millis();
                if ms > i32::MAX as u128 { i32::MAX } else { ms as i32 }
            }
        };
        let max = self.events.len() as i32;
        let n = unsafe {
            epoll_wait(self.epfd, self.events.as_mut_ptr(), max, timeout_ms)
        };
        if n < 0 {
            // EINTR (interrupted by a signal) is not a real error — return empty (the caller may retry).
            let e = errno();
            if e == 4 { return Ok(Vec::new()); }
            return Err(IoError::PollFailed { raw_errno: e });
        }
        let mut result = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            let ev = self.events[i];
            let token = IoToken(ev.data as usize);
            let readiness = Self::events_to_interest(ev.events);
            result.push(IoEvent { token, readiness });
        }
        Ok(result)
    }
}

impl Drop for EpollReactor {
    fn drop(&mut self) {
        // close(epfd) — close via a raw fd. The standard library's
        // std::os::unix::io does not expose close(), but libc close is a C ABI
        // and can be declared directly.
        unsafe extern "C" { fn close(fd: i32) -> i32; }
        if self.epfd >= 0 {
            unsafe { close(self.epfd); }
        }
    }
}
