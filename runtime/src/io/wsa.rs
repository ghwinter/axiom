#![allow(non_snake_case, dead_code)]

//! Windows IO multiplexing — based on WSAEventSelect + WSAWaitForMultipleEvents.
//!
//! ## Readiness model
//!
//! `WSAEventSelect` associates a socket's network events with a manual-reset
//! event object; `WSAWaitForMultipleEvents` waits for any event to become
//! ready; `WSAEnumNetworkEvents` enumerates the specific events **and resets
//! the event object** (level-triggered semantics — if data is still readable,
//! it becomes ready again after the reset).
//!
//! ## Limitation
//!
//! `WSAWaitForMultipleEvents` supports at most 64 event objects
//! (`WSA_MAXIMUM_WAIT_EVENTS`); exceeding that returns `CapacityExceeded`.
//! Production-scale IO (thousands of connections) should use IOCP (completion
//! model, a later increment) — this implementation provides a usable
//! readiness multiplexer for zero-dependency scenarios.
//!
//! ## FFI
//!
//! Minimal declaration set — only the 6 functions this reactor needs.
//! `ws2_32.lib` is already linked indirectly through `std::net`, so no
//! additional link directive is needed. Zero external dependencies.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;

use crate::io::{IoError, IoEvent, IoInterest, IoReactor, IoToken, RawIo};

// ── Windows FFI types ─────────────────────────────────────────────────────
type HANDLE = *mut core::ffi::c_void;
type SOCKET = usize;
type BOOL = i32;

// FD_* network event bit masks (winsock2.h)
const FD_READ: i32 = 0x00000001;
const FD_WRITE: i32 = 0x00000002;
const FD_ACCEPT: i32 = 0x00000008;
const FD_CONNECT: i32 = 0x00000010;
const FD_CLOSE: i32 = 0x00000020;

const WSA_WAIT_FAILED: u32 = 0xFFFFFFFF;
const WSA_WAIT_TIMEOUT: u32 = 258;
const WSA_INFINITE: u32 = 0xFFFFFFFF;
const WSA_MAXIMUM_WAIT_EVENTS: usize = 64;

/// winsock2.h: WSANETWORKEVENTS — the output structure of WSAEnumNetworkEvents.
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
    /// socket → registration index (must be updated after swap_remove in deregister).
    socket_index: BTreeMap<SOCKET, usize>,
}

// WSA event handles and sockets are kernel object handles, safe to share
// across threads (not thread-local pointers). `HANDLE = *mut c_void` is not
// Send by default, but here the pointer value is an OS handle, not a memory
// address — mark Send manually to satisfy the `IoReactor: Send` bound.
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
        // If already registered, use the reregister path (avoids creating a duplicate event object).
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
        // swap_remove moves the last element into idx — update the moved element's route index.
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
        // With fWaitAll=FALSE only the first ready index is returned, but several may
        // be ready at once. Enumerate the network events of **all** registered
        // sockets — WSAEnumNetworkEvents resets the event object (level-triggered:
        // if the condition still holds, the next poll reports it again). O(n)
        // (n ≤ 64) — acceptable given WSAEventSelect's scale limit.
        let mut events = Vec::new();
        for reg in &self.registrations {
            let mut ne = WsaNetworkEvents { lNetworkEvents: 0, iErrorCode: [0; 10] };
            let rc = unsafe { WSAEnumNetworkEvents(reg.socket, reg.event, &mut ne) };
            if rc != 0 {
                // The socket may have been closed — skip it (cleaned up on the next deregister).
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
