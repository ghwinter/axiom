#![allow(dead_code)]

//! macOS/BSD IO 多路复用——基于 kqueue（readiness 模型，level-triggered）。
//!
//! ## 模型
//!
//! `kqueue()` 创建实例；`kevent(EV_ADD/EV_DELETE)` 管理 fd 兴趣；
//! `kevent(timeout)` 阻塞等待就绪事件。level-triggered（不设 `EV_CLEAR`）。
//!
//! ## FFI
//!
//! 最小声明集——`kqueue` / `kevent` / `close` + `__error`（macOS errno）。
//! 零外部依赖。kqueue 是 BSD/macOS 专有 syscall。

use alloc::vec::Vec;
use core::time::Duration;

use crate::io::{IoError, IoEvent, IoInterest, IoReactor, IoToken, RawIo};

// ── kqueue 常量（sys/event.h）──────────────────────────────────────────────
const EVFILT_READ: i16 = -1;
const EVFILT_WRITE: i16 = -2;

const EV_ADD: u16 = 0x0001;
const EV_DELETE: u16 = 0x0002;
const EV_ENABLE: u16 = 0x0004;
const EV_EOF: u16 = 0x8000;
const EV_ERROR: u16 = 0x4000;

/// kevent（macOS 64-bit）。
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

/// timespec（kqueue timeout）。
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

// kqueue fd 与 kevent 中的 udata 指针（存 token，非线程局部内存）跨线程安全。
// `Kevent.udata: *mut c_void` 默认不是 Send——手动标记。
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
        // udata 存 token——kqueue 允许任意指针，我们用它传 token（不 deref）。
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
            // 丢弃未注册的 token_box（kevent 失败时 udata 未被消费）。
            // 安全：Box::into_raw 的内存还没被 kqueue 接管（ctl 失败）。
            unsafe { drop(Box::from_raw(kev.udata as *mut usize)); }
            return Err(IoError::RegisterFailed { raw_errno: errno() });
        }
        Ok(())
    }
}

impl IoReactor for KqueueReactor {
    fn register(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        // kqueue 每个 filter 是独立的注册——readable/writable 分别注册。
        if interest.is_readable() {
            self.ctl(raw, EVFILT_READ, EV_ADD | EV_ENABLE, token)?;
        }
        if interest.is_writable() {
            self.ctl(raw, EVFILT_WRITE, EV_ADD | EV_ENABLE, token)?;
        }
        Ok(())
    }

    fn reregister(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        // kqueue 的 EV_ADD 是幂等的——重复 add 等同 mod。直接走 register 路径。
        self.register(raw, interest, token)
    }

    fn deregister(&mut self, raw: RawIo) -> Result<(), IoError> {
        // 删除两个 filter（可能只注册了一个，EV_DELETE 不存在的 filter 返回 ENOENT）。
        let mut kevs = [
            Kevent { ident: raw as usize, filter: EVFILT_READ, flags: EV_DELETE, fflags: 0, data: 0, udata: core::ptr::null_mut() },
            Kevent { ident: raw as usize, filter: EVFILT_WRITE, flags: EV_DELETE, fflags: 0, data: 0, udata: core::ptr::null_mut() },
        ];
        let rc = unsafe { kevent(self.kq, kevs.as_ptr(), 2, core::ptr::null_mut(), 0, core::ptr::null()) };
        // EV_DELETE 不存在的 filter 会在 eventlist 里报 EV_ERROR + ENOENT，
        // 但 nchanges=2 且 nevents=0 时，错误被静默丢弃（rc=-1 仅当全部失败）。
        // 容忍 ENOENT——只关心 fd 不再被监听。
        if rc < 0 {
            let e = errno();
            if e != 2 { // ENOENT = filter 未注册，不是错误
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
                continue; // changelist 错误回报，跳过
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
            // 释放 udata（token_box）——kqueue 在 eventlist 中归还所有权。
            // 安全：register 时 Box::into_raw 的内存，kevent 返回后不再被 kqueue 引用。
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
