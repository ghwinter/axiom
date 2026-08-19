//! IO multiplexing — platform abstraction + runtime integration.
//!
//! ## Design role
//!
//! axiom core's `Machine::process` keeps its synchronous signature. IO
//! multiplexing is the runtime's responsibility (see the design principles
//! in `lib.rs`). This module provides:
//!
//! - **`IoReactor` trait**: the platform abstraction for the readiness model
//!   (register / poll). Linux → epoll, macOS/BSD → kqueue, Windows →
//!   WSAEventSelect.
//! - **`IoEvent`**: a readiness event injected as an `Any + Send` payload
//!   into a machine's input port — the machine performs the actual IO
//!   (read/write) in `process` after receiving it.
//! - **`ManualReactor`**: an in-memory reactor with preloaded events, used
//!   for OS-independent unit tests (verifying runtime integration without
//!   relying on real sockets).
//!
//! ## Integration model (external registration)
//!
//! The existing side channels are all one-way runtime→machine
//! (signal/time/lifecycle), so a machine cannot proactively register an FD
//! inside `process`. Hence **external registration**:
//!
//! 1. The caller creates an IO source (e.g. `TcpListener`) and takes its raw
//!    fd/socket;
//! 2. `rt.register_io(token, machine_name, port, raw, interest)` registers
//!    it — the runtime keeps a token→(machine, port) mapping and the reactor
//!    keeps a token→fd mapping;
//! 3. `rt.run_io(external_inputs, timeout)` — polls the reactor, turns ready
//!    events into inputs, merges external inputs, drives the existing tick
//!    loop, and returns the outputs.
//!
//! This keeps the `process` signature unchanged and avoids extending
//! `MachineContext`.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;

#[cfg(unix)]
pub type RawIo = std::os::unix::io::RawFd;

#[cfg(windows)]
pub type RawIo = std::os::windows::io::RawSocket;

#[cfg(not(any(unix, windows)))]
pub type RawIo = i32;

/// IO readiness interest flags (readiness model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoInterest(u8);

impl IoInterest {
    pub const READABLE: Self = Self(0b01);
    pub const WRITABLE: Self = Self(0b10);
    pub const READ_WRITE: Self = Self(0b11);

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn is_readable(self) -> bool {
        self.0 & Self::READABLE.0 != 0
    }

    pub fn is_writable(self) -> bool {
        self.0 & Self::WRITABLE.0 != 0
    }

    /// The bit vector (for internal use by platform implementations — building epoll/kqueue/WSA masks).
    pub(crate) fn bits(self) -> u8 { self.0 }

    /// Construct from a bit vector (for internal use by platform implementations — normalizing readiness events).
    pub(crate) fn from_bits(b: u8) -> Self { Self(b) }
}

/// Registration token for an IO source — the caller uses it to associate readiness events with a machine's input port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IoToken(pub usize);

/// A readiness event returned from the reactor.
///
/// Injected into a machine's input port as a `Box<dyn Any + Send>`. The
/// machine receives it via downcast in `process` and performs the actual IO
/// according to `readiness`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoEvent {
    pub token: IoToken,
    pub readiness: IoInterest,
}

/// Reactor error — normalization of OS multiplexing call failures.
#[derive(Debug)]
pub enum IoError {
    /// register / deregister failure (normalized OS errno).
    RegisterFailed { raw_errno: i32 },
    /// poll failure (normalized OS errno).
    PollFailed { raw_errno: i32 },
    /// Exceeds a platform limit (e.g. WSAEventSelect supports at most 64 event objects).
    CapacityExceeded,
    /// Unsupported platform (no epoll/kqueue/WSA available).
    Unsupported,
}

impl core::fmt::Display for IoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RegisterFailed { raw_errno } => write!(f, "io register failed (errno={raw_errno})"),
            Self::PollFailed { raw_errno } => write!(f, "io poll failed (errno={raw_errno})"),
            Self::CapacityExceeded => write!(f, "io reactor capacity exceeded"),
            Self::Unsupported => write!(f, "io reactor unsupported on this platform"),
        }
    }
}

/// IO multiplexing abstraction for the readiness model.
///
/// Platform implementations:
/// - Linux: `epoll` (`EpollReactor`)
/// - macOS/BSD: `kqueue` (`KqueueReactor`)
/// - Windows: `WSAEventSelect` (`WsaReactor`)
///
/// All methods take a `RawIo` (Unix = `RawFd`, Windows = `RawSocket`).
/// `poll` returns the list of readiness events (possibly empty — the timeout
/// expired with nothing ready).
pub trait IoReactor: Send {
    /// Register readiness interest for an IO source. `token` is used to
    /// associate readiness events with the caller's context in `poll` results.
    fn register(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError>;

    /// Update the interest of a registered source (rearm under the readiness model).
    fn reregister(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError>;

    /// Deregister an IO source.
    fn deregister(&mut self, raw: RawIo) -> Result<(), IoError>;

    /// Block waiting for readiness events, at most `timeout`. `None` = block
    /// until an event arrives; `Some(0)` = return immediately (non-blocking poll).
    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<IoEvent>, IoError>;
}

// ── Platform selection ────────────────────────────────────────────────────
//
// The best reactor available on the current platform. `default_reactor()`
// returns a new instance.
#[cfg(target_os = "linux")]
pub mod epoll;
#[cfg(target_os = "linux")]
pub use epoll::EpollReactor as DefaultReactor;

#[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
pub mod kqueue;
#[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
pub use kqueue::KqueueReactor as DefaultReactor;

#[cfg(target_os = "windows")]
pub mod wsa;
#[cfg(target_os = "windows")]
pub use wsa::WsaReactor as DefaultReactor;

/// Construct the default reactor for the current platform.
#[allow(dead_code)]
pub fn default_reactor() -> Result<DefaultReactor, IoError> {
    DefaultReactor::new()
}

// ── ManualReactor: in-memory reactor for tests ────────────────────────────
//
// A preloaded event queue — `poll` pops the preset events. It does not rely
// on OS sockets; it is used to verify the runtime integration
// (register_io → run_io → IoEvent injection).
use alloc::collections::VecDeque;

/// In-memory reactor — preloads readiness events; `poll` pops them in order.
///
/// `register`/`deregister` are no-ops (only recording the calls). `poll` pops
/// events from the preloaded queue; when the queue is empty, `timeout`
/// behavior: `None` → return empty (no blocking), `Some(0)` → return empty,
/// `Some(_)` → return empty (tests do not actually sleep).
pub struct ManualReactor {
    pending: VecDeque<IoEvent>,
    registered: BTreeMap<IoToken, (RawIo, IoInterest)>,
}

impl ManualReactor {
    pub fn new() -> Self {
        Self { pending: VecDeque::new(), registered: BTreeMap::new() }
    }

    /// Pre-inject a readiness event; the next `poll` will return it.
    pub fn push_event(&mut self, event: IoEvent) {
        self.pending.push_back(event);
    }
}

impl Default for ManualReactor {
    fn default() -> Self {
        Self::new()
    }
}

impl IoReactor for ManualReactor {
    fn register(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        self.registered.insert(token, (raw, interest));
        Ok(())
    }

    fn reregister(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError> {
        self.registered.insert(token, (raw, interest));
        Ok(())
    }

    fn deregister(&mut self, _raw: RawIo) -> Result<(), IoError> {
        Ok(())
    }

    fn poll(&mut self, _timeout: Option<Duration>) -> Result<Vec<IoEvent>, IoError> {
        let mut events = Vec::new();
        while let Some(e) = self.pending.pop_front() {
            events.push(e);
        }
        Ok(events)
    }
}
