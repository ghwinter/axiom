#![allow(dead_code)]

//! Linux IO 多路复用——基于 epoll（readiness 模型，level-triggered）。
//!
//! ## 模型
//!
//! `epoll_create1` 创建实例；`epoll_ctl(ADD/MOD/DEL)` 管理 fd 兴趣；
//! `epoll_wait` 阻塞等待就绪事件。level-triggered（不设 `EPOLLET`）——
//! 若数据仍可读，下次 `epoll_wait` 会再次报告。
//!
//! ## FFI
//!
//! 最小声明集——`epoll_create1` / `epoll_ctl` / `epoll_wait` +
//! `__errno_location`。零外部依赖（不链接 libc crate，直接声明）。
//! epoll 是 Linux 专有 syscall，通过 VDSO / syscall wrapper 可直接 FFI。

use alloc::vec::Vec;
use core::time::Duration;

use crate::io::{IoError, IoEvent, IoInterest, IoReactor, IoToken, RawIo};

// ── epoll 常量（sys/epoll.h）───────────────────────────────────────────────
const EPOLL_CLOEXEC: i32 = 0x80000;
const EPOLL_CTL_ADD: i32 = 1;
const EPOLL_CTL_DEL: i32 = 2;
const EPOLL_CTL_MOD: i32 = 3;

const EPOLLIN: u32 = 0x001;
const EPOLLOUT: u32 = 0x004;
const EPOLLERR: u32 = 0x008;
const EPOLLHUP: u32 = 0x010;
const EPOLLRDHUP: u32 = 0x2000;

/// epoll_event（Linux 64-bit）：events(u32) + data(u64)。
///
/// **必须用 `#[repr(C)]` 而非 `#[repr(C, packed)]`**——Linux 内核的
/// `struct epoll_event` 在 64-bit 上 data 字段对齐 8（offset 8，前面有
/// 4 字节 padding）。packed 会把 data 放到 offset 4，导致内核写入的
/// token 被从错误偏移读取。x86_64 宽松对齐下碰巧工作，aarch64 严格
/// 对齐会读到垃圾值（token 错位 → 测试失败）。
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
    /// 复用的 event 缓冲区（避免每次 poll 分配）。
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
            // EINTR（信号中断）不是真错误——返回空（调用方可重试）。
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
        // close(epfd)——用 std 的 raw fd close（Rust 的 std::os::unix::io
        // 不暴露 close()，但 libc close 是 C ABI，可直接声明）。
        unsafe extern "C" { fn close(fd: i32) -> i32; }
        if self.epfd >= 0 {
            unsafe { close(self.epfd); }
        }
    }
}
