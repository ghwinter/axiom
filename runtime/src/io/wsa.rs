#![allow(non_snake_case, dead_code)]

//! Windows IO 多路复用——基于 WSAEventSelect + WSAWaitForMultipleEvents。
//!
//! ## readiness 模型
//!
//! `WSAEventSelect` 关联 socket 网络事件与手动重置事件对象；
//! `WSAWaitForMultipleEvents` 等待任一事件就绪；
//! `WSAEnumNetworkEvents` 枚举具体事件**并重置事件对象**（level-triggered
//! 语义——若数据仍可读，重置后会再次就绪）。
//!
//! ## 限制
//!
//! `WSAWaitForMultipleEvents` 最多 64 个事件对象
//! （`WSA_MAXIMUM_WAIT_EVENTS`）。超出返回 `CapacityExceeded`。
//! 生产级大规模 IO（数千连接）应使用 IOCP（completion 模型，
//! 后续增量）——本实现为零依赖场景提供可用的 readiness 多路复用。
//!
//! ## FFI
//!
//! 最小声明集——仅本 reactor 需要的 6 个函数。`ws2_32.lib` 已由
//! `std::net` 间接链接，无需额外链接指令。零外部依赖。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;

use crate::io::{IoError, IoEvent, IoInterest, IoReactor, IoToken, RawIo};

// ── Windows FFI 类型 ───────────────────────────────────────────────────────
type HANDLE = *mut core::ffi::c_void;
type SOCKET = usize;
type BOOL = i32;

// FD_* 网络事件位掩码（winsock2.h）
const FD_READ: i32 = 0x00000001;
const FD_WRITE: i32 = 0x00000002;
const FD_ACCEPT: i32 = 0x00000008;
const FD_CONNECT: i32 = 0x00000010;
const FD_CLOSE: i32 = 0x00000020;

const WSA_WAIT_FAILED: u32 = 0xFFFFFFFF;
const WSA_WAIT_TIMEOUT: u32 = 258;
const WSA_INFINITE: u32 = 0xFFFFFFFF;
const WSA_MAXIMUM_WAIT_EVENTS: usize = 64;

/// winsock2.h: WSANETWORKEVENTS——WSAEnumNetworkEvents 的输出结构。
#[repr(C)]
struct WsaNetworkEvents {
    lNetworkEvents: i32,
    iErrorCode: [i32; 10],
}

unsafe extern "system" {
    fn WSACreateEvent() -> HANDLE;
    fn WSACloseEvent(hEvent: HANDLE) -> BOOL;
    fn WSAEventSelect(s: SOCKET, hEventObject: HANDLE, lNetworkEvents: i32) -> i32;
    fn WSAWaitForMultipleEvents(
        cEvents: u32,
        lphEvents: *const HANDLE,
        fWaitAll: BOOL,
        dwTimeout: u32,
        fAlertable: BOOL,
    ) -> u32;
    fn WSAEnumNetworkEvents(
        s: SOCKET,
        hEventObject: HANDLE,
        lpNetworkEvents: *mut WsaNetworkEvents,
    ) -> i32;
    fn WSAGetLastError() -> i32;
}

// ── Reactor ────────────────────────────────────────────────────────────────

struct Registration {
    socket: SOCKET,
    event: HANDLE,
    token: IoToken,
    interest: IoInterest,
}

pub struct WsaReactor {
    registrations: Vec<Registration>,
    /// socket → registration 索引（deregister 用 swap_remove 后需更新）。
    socket_index: BTreeMap<SOCKET, usize>,
}

// WSA 事件句柄与 socket 是内核对象句柄，跨线程共享安全（非线程局部指针）。
// `HANDLE = *mut c_void` 默认不是 Send，但这里的指针值是 OS 句柄而非
// 内存地址——手动标记 Send 让 `IoReactor: Send` 约束满足。
unsafe impl Send for WsaReactor {}

impl WsaReactor {
    pub fn new() -> Result<Self, IoError> {
        Ok(Self {
            registrations: Vec::new(),
            socket_index: BTreeMap::new(),
        })
    }

    fn interest_to_events(interest: IoInterest) -> i32 {
        let mut mask: i32 = 0;
        if interest.is_readable() {
            mask |= FD_READ | FD_ACCEPT | FD_CLOSE;
        }
        if interest.is_writable() {
            mask |= FD_WRITE | FD_CONNECT;
        }
        mask
    }

    fn events_to_interest(events: i32) -> IoInterest {
        let mut bits: u8 = 0;
        if events & (FD_READ | FD_ACCEPT | FD_CLOSE) != 0 {
            bits |= IoInterest::READABLE.bits();
        }
        if events & (FD_WRITE | FD_CONNECT) != 0 {
            bits |= IoInterest::WRITABLE.bits();
        }
        IoInterest::from_bits(bits)
    }
}

impl IoReactor for WsaReactor {
    fn register(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        if self.registrations.len() >= WSA_MAXIMUM_WAIT_EVENTS {
            return Err(IoError::CapacityExceeded);
        }
        let socket = raw as SOCKET;
        // 已注册则走 reregister 路径（避免重复创建事件对象）。
        if self.socket_index.contains_key(&socket) {
            return self.reregister(raw, interest, token);
        }
        let event = unsafe { WSACreateEvent() };
        if event.is_null() {
            return Err(IoError::RegisterFailed { raw_errno: unsafe { WSAGetLastError() } });
        }
        let mask = Self::interest_to_events(interest);
        let rc = unsafe { WSAEventSelect(socket, event, mask) };
        if rc != 0 {
            let errno = unsafe { WSAGetLastError() };
            unsafe { WSACloseEvent(event) };
            return Err(IoError::RegisterFailed { raw_errno: errno });
        }
        self.socket_index.insert(socket, self.registrations.len());
        self.registrations.push(Registration { socket, event, token, interest });
        Ok(())
    }

    fn reregister(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        let socket = raw as SOCKET;
        let idx = match self.socket_index.get(&socket) {
            Some(&i) => i,
            None => return self.register(raw, interest, token),
        };
        let event = self.registrations[idx].event;
        let mask = Self::interest_to_events(interest);
        let rc = unsafe { WSAEventSelect(socket, event, mask) };
        if rc != 0 {
            return Err(IoError::RegisterFailed { raw_errno: unsafe { WSAGetLastError() } });
        }
        self.registrations[idx].interest = interest;
        self.registrations[idx].token = token;
        Ok(())
    }

    fn deregister(&mut self, raw: RawIo) -> Result<(), IoError> {
        let socket = raw as SOCKET;
        let idx = match self.socket_index.remove(&socket) {
            Some(i) => i,
            None => return Ok(()),
        };
        let reg = self.registrations.swap_remove(idx);
        // swap_remove 把最后一个元素移到 idx——更新被移动元素的路由索引。
        if idx < self.registrations.len() {
            let moved_socket = self.registrations[idx].socket;
            self.socket_index.insert(moved_socket, idx);
        }
        unsafe { WSACloseEvent(reg.event) };
        Ok(())
    }

    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<IoEvent>, IoError> {
        if self.registrations.is_empty() {
            return Ok(Vec::new());
        }
        let handles: Vec<HANDLE> = self.registrations.iter().map(|r| r.event).collect();
        let timeout_ms = match timeout {
            None => WSA_INFINITE,
            Some(d) => {
                let ms = d.as_millis();
                if ms > u32::MAX as u128 { WSA_INFINITE } else { ms as u32 }
            }
        };
        let count = handles.len() as u32;
        let wait_result = unsafe {
            WSAWaitForMultipleEvents(count, handles.as_ptr(), 0, timeout_ms, 0)
        };
        if wait_result == WSA_WAIT_FAILED {
            return Err(IoError::PollFailed { raw_errno: unsafe { WSAGetLastError() } });
        }
        if wait_result == WSA_WAIT_TIMEOUT {
            return Ok(Vec::new());
        }
        // fWaitAll=FALSE 时只返回第一个就绪索引，但多个可能同时就绪。
        // 枚举**所有**注册 socket 的网络事件——WSAEnumNetworkEvents 会重置
        // 事件对象（level-triggered：若条件仍满足，下次 poll 再报）。
        // O(n)（n ≤ 64）——对 WSAEventSelect 的规模上限是可接受的。
        let mut events = Vec::new();
        for reg in &self.registrations {
            let mut ne = WsaNetworkEvents { lNetworkEvents: 0, iErrorCode: [0; 10] };
            let rc = unsafe { WSAEnumNetworkEvents(reg.socket, reg.event, &mut ne) };
            if rc != 0 {
                // socket 可能已关闭——跳过（下次 deregister 清理）。
                continue;
            }
            if ne.lNetworkEvents != 0 {
                let interest = Self::events_to_interest(ne.lNetworkEvents);
                events.push(IoEvent { token: reg.token, readiness: interest });
            }
        }
        Ok(events)
    }
}

impl Drop for WsaReactor {
    fn drop(&mut self) {
        for reg in &self.registrations {
            unsafe { WSACloseEvent(reg.event) };
        }
    }
}
