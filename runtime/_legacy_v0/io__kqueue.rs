#![allow(dead_code)]

//! macOS/BSD IO multiplexing — based on kqueue (readiness model, level-triggered).
//!
//! ## Model
//!
//! `kqueue()` creates the instance; `kevent(EV_ADD/EV_DELETE)` manages fd
//! interest; `kevent(timeout)` blocks waiting for readiness events.
//! level-triggered (no `EV_CLEAR`).
//!
//! ## FFI
//!
//! Minimal declaration set — `kqueue` / `kevent` / `close` + `__error`
//! (macOS errno). Zero external dependencies. kqueue is a BSD/macOS-specific
//! syscall.

use alloc::vec::Vec;
use core::time::Duration;

use crate::io::{IoError, IoEvent, IoInterest, IoReactor, IoToken, RawIo};

// ── kqueue constants (sys/event.h) ────────────────────────────────────────
const EVFILT_READ: i16 = -1;
const EVFILT_WRITE: i16 = -2;

const EV_ADD: u16 = 0x0001;
const EV_DELETE: u16 = 0x0002;
const EV_ENABLE: u16 = 0x0004;
const EV_EOF: u16 = 0x8000;
const EV_ERROR: u16 = 0x4000;

/// kevent (macOS 64-bit).
#[repr(C)]
#[derive(Clone, Copy)]
struct Kevent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut core::ffi::c_void,
}

/// timespec (kqueue timeout).
#[repr(C)]
#[derive(Clone, Copy)]
struct Timespec {
    tv_sec: isize,
    tv_nsec: isize,
}

unsafe extern "C" {
    fn kqueue() -> i32;
    fn kevent(
        kq: i32,
        changelist: *const Kevent,
        nchanges: i32,
        eventlist: *mut Kevent,
        nevents: i32,
        timeout: *const Timespec,
    ) -> i32;
    fn close(fd: i32) -> i32;
    fn __error() -> *mut i32;
}

fn errno() -> i32 {
    unsafe { *__error() }
}

// ── Reactor ────────────────────────────────────────────────────────────────

pub struct KqueueReactor {
    kq: i32,
    events: Vec<Kevent>,
}

// The kqueue fd and the udata pointers in kevent (which store tokens, not
// thread-local memory) are safe to share across threads.
// `Kevent.udata: *mut c_void` is not Send by default — mark it manually.
unsafe impl Send for KqueueReactor {}

impl KqueueReactor {
    pub fn new() -> Result<Self, IoError> {
        let kq = unsafe { kqueue() };
        if kq < 0 {
            return Err(IoError::RegisterFailed { raw_errno: errno() });
        }
        Ok(Self {
            kq,
            events: (0..64).map(|_| Kevent {
                ident: 0, filter: 0, flags: 0, fflags: 0, data: 0, udata: core::ptr::null_mut(),
            }).collect(),
        })
    }

    fn ctl(&mut self, fd: RawIo, filter: i16, flags: u16, token: IoToken) -> Result<(), IoError> {
        // udata stores the token — kqueue accepts arbitrary pointers; we use it to pass the token (never dereferenced).
        let token_box = Box::new(token.0);
        let kev = Kevent {
            ident: fd as usize,
            filter,
            flags,
            fflags: 0,
            data: 0,
            udata: Box::into_raw(token_box) as *mut core::ffi::c_void,
        };
        let rc = unsafe { kevent(self.kq, &kev, 1, core::ptr::null_mut(), 0, core::ptr::null()) };
        if rc < 0 {
            // Drop the unregistered token_box (on kevent failure, udata was not consumed).
            // Safety: the Box::into_raw allocation has not been taken over by kqueue
            // (the ctl call failed).
            unsafe { drop(Box::from_raw(kev.udata as *mut usize)); }
            return Err(IoError::RegisterFailed { raw_errno: errno() });
        }
        Ok(())
    }
}

impl IoReactor for KqueueReactor {
    fn register(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        // Each kqueue filter is a separate registration — register readable/writable separately.
        if interest.is_readable() {
            self.ctl(raw, EVFILT_READ, EV_ADD | EV_ENABLE, token)?;
        }
        if interest.is_writable() {
            self.ctl(raw, EVFILT_WRITE, EV_ADD | EV_ENABLE, token)?;
        }
        Ok(())
    }

    fn reregister(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        // kqueue's EV_ADD is idempotent — repeated adds are equivalent to a mod. Just use the register path.
        self.register(raw, interest, token)
    }

    fn deregister(&mut self, raw: RawIo) -> Result<(), IoError> {
        // Delete both filters (only one may be registered; EV_DELETE on a missing filter returns ENOENT).
        let mut kevs = [
            Kevent { ident: raw as usize, filter: EVFILT_READ, flags: EV_DELETE, fflags: 0, data: 0, udata: core::ptr::null_mut() },
            Kevent { ident: raw as usize, filter: EVFILT_WRITE, flags: EV_DELETE, fflags: 0, data: 0, udata: core::ptr::null_mut() },
        ];
        let rc = unsafe { kevent(self.kq, kevs.as_ptr(), 2, core::ptr::null_mut(), 0, core::ptr::null()) };
        // Deleting a missing filter reports EV_ERROR + ENOENT in the eventlist, but
        // with nchanges=2 and nevents=0 the error is silently dropped (rc=-1 only
        // when all changes fail). Tolerate ENOENT — we only care that the fd is no
        // longer monitored.
        if rc < 0 {
            let e = errno();
            if e != 2 { // ENOENT = filter not registered, not an error
                return Err(IoError::RegisterFailed { raw_errno: e });
            }
        }
        Ok(())
    }

    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<IoEvent>, IoError> {
        let ts = match timeout {
            None => None,
            Some(d) => Some(Timespec {
                tv_sec: d.as_secs() as isize,
                tv_nsec: d.subsec_nanos() as isize,
            }),
        };
        let ts_ptr = ts.as_ref().map(|t| t as *const Timespec).unwrap_or(core::ptr::null());
        let max = self.events.len() as i32;
        let n = unsafe {
            kevent(self.kq, core::ptr::null(), 0, self.events.as_mut_ptr(), max, ts_ptr)
        };
        if n < 0 {
            let e = errno();
            if e == 4 { return Ok(Vec::new()); } // EINTR
            return Err(IoError::PollFailed { raw_errno: e });
        }
        let mut result = Vec::with_capacity(n as usize);
        for i in 0..n as usize {
            let ev = self.events[i];
            if ev.flags & EV_ERROR != 0 {
                continue; // changelist error report, skip
            }
            let token = IoToken(ev.udata as usize);
            let mut bits: u8 = 0;
            if ev.filter == EVFILT_READ || ev.flags & EV_EOF != 0 {
                bits |= IoInterest::READABLE.bits();
            }
            if ev.filter == EVFILT_WRITE {
                bits |= IoInterest::WRITABLE.bits();
            }
            result.push(IoEvent { token, readiness: IoInterest::from_bits(bits) });
            // Release udata (token_box) — kqueue returns ownership in the eventlist.
            // Safety: the memory from Box::into_raw at register time is no longer
            // referenced by kqueue after kevent returns.
            if !ev.udata.is_null() {
                unsafe { drop(Box::from_raw(ev.udata as *mut usize)); }
            }
        }
        Ok(result)
    }
}

impl Drop for KqueueReactor {
    fn drop(&mut self) {
        if self.kq >= 0 {
            unsafe { close(self.kq); }
        }
    }
}
