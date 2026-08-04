//! IO 多路复用——平台抽象 + runtime 集成。
//!
//! ## 设计定位
//!
//! axiom core 的 `Machine::process` 保持同步签名不变。IO 多路复用是
//! runtime 的职责（见 `lib.rs` 设计原则）。本模块提供：
//!
//! - **`IoReactor` trait**：readiness 模型的平台抽象（register / poll）。
//!   Linux → epoll，macOS/BSD → kqueue，Windows → WSAEventSelect。
//! - **`IoEvent`**：就绪事件，作为 `Any + Send` payload 注入 machine 的
//!   输入端口——machine 在 `process` 中收到后执行实际 IO（read/write）。
//! - **`ManualReactor`**：预装载事件的内存 reactor，用于无 OS 依赖的
//!   单元测试（验证 runtime 集成，不依赖真实 socket）。
//!
//! ## 集成模型（外部注册）
//!
//! 现有 side-channel 全是 runtime→machine 单向（signal/time/lifecycle），
//! machine 无法在 `process` 内主动注册 FD。故采用**外部注册**：
//!
//! 1. 调用方创建 IO source（如 `TcpListener`），取 raw fd/socket；
//! 2. `rt.register_io(token, machine_name, port, raw, interest)` 注册——
//!    runtime 维护 token→(machine, port) 映射，reactor 维护 token→fd；
//! 3. `rt.run_io(external_inputs, timeout)` ——poll reactor → 就绪事件
//!    转为 inputs → 合并外部 inputs → 驱动现有 tick 循环 → 返回输出。
//!
//! 这保持了 `process` 签名不变，无需扩展 `MachineContext`。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;

#[cfg(unix)]
pub type RawIo = std::os::unix::io::RawFd;

#[cfg(windows)]
pub type RawIo = std::os::windows::io::RawSocket;

#[cfg(not(any(unix, windows)))]
pub type RawIo = i32;

/// IO 就绪兴趣标志（readiness 模型）。
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

    /// 位向量（平台实现内部用——epoll/kqueue/WSA 的掩码构造）。
    pub(crate) fn bits(self) -> u8 { self.0 }

    /// 从位向量构造（平台实现内部用——就绪事件归一化）。
    pub(crate) fn from_bits(b: u8) -> Self { Self(b) }
}

/// IO source 的注册令牌——调用方用它关联就绪事件与 machine 输入端口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IoToken(pub usize);

/// 从 reactor 返回的就绪事件。
///
/// 作为 `Box<dyn Any + Send>` 注入 machine 的输入端口。machine 在
/// `process` 中 downcast 收到它，按 `readiness` 执行实际 IO。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoEvent {
    pub token: IoToken,
    pub readiness: IoInterest,
}

/// reactor 错误——OS 多路复用调用的失败归一化。
#[derive(Debug)]
pub enum IoError {
    /// register / deregister 失败（OS errno 归一化）。
    RegisterFailed { raw_errno: i32 },
    /// poll 失败（OS errno 归一化）。
    PollFailed { raw_errno: i32 },
    /// 超出平台限制（如 WSAEventSelect 最多 64 个事件对象）。
    CapacityExceeded,
    /// 不支持的平台（无 epoll/kqueue/WSA 可用）。
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

/// readiness 模型的 IO 多路复用抽象。
///
/// 平台实现：
/// - Linux：`epoll`（`EpollReactor`）
/// - macOS/BSD：`kqueue`（`KqueueReactor`）
/// - Windows：`WSAEventSelect`（`WsaReactor`）
///
/// 所有方法接收 `RawIo`（Unix = `RawFd`，Windows = `RawSocket`）。
/// `poll` 返回就绪事件列表（可能为空——timeout 到期且无就绪）。
pub trait IoReactor: Send {
    /// 注册一个 IO source 的就绪兴趣。`token` 用于在 `poll` 结果中
    /// 关联就绪事件与调用方上下文。
    fn register(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError>;

    /// 更新已注册 source 的兴趣（readiness 模型下 rearm）。
    fn reregister(&mut self, raw: RawIo, interest: IoInterest, token: IoToken) -> Result<(), IoError>;

    /// 注销一个 IO source。
    fn deregister(&mut self, raw: RawIo) -> Result<(), IoError>;

    /// 阻塞等待就绪事件，最多 `timeout`。`None` = 阻塞直到有事件；
    /// `Some(0)` = 立即返回（非阻塞轮询）。
    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<IoEvent>, IoError>;
}

// ── 平台选择 ──────────────────────────────────────────────────────────────
//
// 当前平台可用的最佳 reactor。`default_reactor()` 返回一个新实例。
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

/// 构造当前平台的默认 reactor。
#[allow(dead_code)]
pub fn default_reactor() -> Result<DefaultReactor, IoError> {
    DefaultReactor::new()
}

// ── ManualReactor：测试用内存 reactor ─────────────────────────────────────
//
// 预装载事件队列——`poll` 弹出预置事件。不依赖 OS socket，用于验证
// runtime 集成（register_io → run_io → IoEvent 注入）的正确性。
use alloc::collections::VecDeque;

/// 内存 reactor——预装载就绪事件，`poll` 按序弹出。
///
/// `register`/`deregister` 是 no-op（仅记录调用）。`poll` 从预置队列
/// 弹出事件；队列空时按 `timeout` 行为：`None` → 返回空（不阻塞），
/// `Some(0)` → 返回空，`Some(_)` → 返回空（测试场景不真睡）。
pub struct ManualReactor {
    pending: VecDeque<IoEvent>,
    registered: BTreeMap<IoToken, (RawIo, IoInterest)>,
}

impl ManualReactor {
    pub fn new() -> Self {
        Self { pending: VecDeque::new(), registered: BTreeMap::new() }
    }

    /// 预注入一个就绪事件，下次 `poll` 会返回它。
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
