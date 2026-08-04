//! # 端口模型：语义标注与物理载体分离
//!
//! axiom 的端口分类——`PortDir`（In/Out）与 `FlowKind`（Data/Control/
//! Observe）——是**语义标注**，不是物理属性。物理层上，一次端口传输只是
//! "线程写字节、另一线程读字节"（或在编译器展开后根本没有字节流动）；
//! 标注不改变物理，也没有任何运行时开销用于维持"这条连接存在"。
//!
//! 三个维度各在不同层面起作用：
//!
//! | 标注 | 职责层面 | 作用 |
//! |------|----------|------|
//! | `port_name` | 语义层 | 身份——数据在拓扑中的坐标（拓扑即身份） |
//! | `PortDir` | 代数层 | 有向边的方向（`f: A → B`）；`can_link_to` 要求 out→in |
//! | `FlowKind` | 验证层 | 语义契约：Data↔Data 才可连；Observe 参与可观测性完备性分析（定理 7.2） |
//!
//! 物理差异**完全**由 [`crate::link::LinkKind`] 表达：同一个 `PortDecl`
//! 可以经 `BoundedBuf`（真实缓冲区，写/读有物理过程）、`Latest`（单槽
//! 变量，覆盖写）、`Inline`（函数调用，无缓冲无分配）连接。其中 `Inline`
//! 是"解抽象"的极端形态：拓扑结构在物理层**完全消解**——值经寄存器/栈
//! 传递，无数据流动、无内存分配，抽象过程没有物理过程对应；但语义层
//! 拓扑仍可表达、可验证（`DeploySpec::validate_deep` 仍检查 Inline 无环
//! 与度约束 ≤1）。
//!
//! 因此：**被分类的是标注，不是物理**。物理对称（"端口本质上没有区别"）
//! 与代数不对称（方向、流类型约束验证）并存——这是 axiom 抽象层与物理
//! 层解耦（foundations.md §15）在端口模型上的具体体现。快照、审计、重放
//! 等物理需求不属于端口标注，属于显式契约（`Machine::checkpoint`/`restore`）。

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use alloc::borrow::Cow;
use alloc::sync::Arc;
use core::any::TypeId;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use crate::flow::FlowKind;
use crate::session::SessionType;
use crate::time::TimeTick;

// ── Port direction ────────────────────────────────────────────────────────────

/// The direction of data flow through a port.
///
/// Direction is orthogonal to [`FlowKind`]: an output port can carry data,
/// control, or observation information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum PortDir {
    /// Data flows into the entity.
    In,
    /// Data flows out of the entity.
    Out,
}

// ── Port declaration ─────────────────────────────────────────────────────────

/// A single port declaration in an entity's port schema.
///
/// Four orthogonal dimensions:
/// - **Direction** (`PortDir`): in or out.
/// - **Semantic kind** (`FlowKind`): data, control, or observation.
/// - **Type**: the Rust type of data crossing this port.
/// - **Protocol** (`SessionType`): optional session protocol that constrains
///   the sequence of operations. Two ports can be linked only if their
///   protocols are dual (see `session::is_dual`).
#[derive(Debug, Clone)]
pub struct PortDecl {
    pub name: &'static str,
    pub dir: PortDir,
    pub flow: FlowKind,
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub schema_ver: u32,
    pub description: &'static str,
    /// Optional session protocol for this port.
    /// If `None`, the port has no protocol constraint.
    /// If `Some`, the port's operations must conform to this protocol,
    /// and a peer port's protocol must be dual.
    pub session: Option<SessionType>,
}

impl PortDecl {
    // ── Data ports ─────────────────────────────────────────

    pub fn input<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::In, FlowKind::Data)
    }

    pub fn output<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::Out, FlowKind::Data)
    }

    // ── Control ports ──────────────────────────────────────

    pub fn ctrl_in<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::In, FlowKind::Control)
    }

    pub fn ctrl_out<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::Out, FlowKind::Control)
    }

    // ── Observation ports ──────────────────────────────────

    pub fn observe<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::Out, FlowKind::Observe)
    }

    // ── Generic constructor ────────────────────────────────

    pub fn new<T: 'static>(name: &'static str, dir: PortDir, flow: FlowKind) -> Self {
        Self {
            name,
            dir,
            flow,
            type_id: TypeId::of::<T>(),
            type_name: core::any::type_name::<T>(),
            schema_ver: 0,
            description: "",
            session: None,
        }
    }

    pub fn with_schema_ver(mut self, ver: u32) -> Self {
        self.schema_ver = ver;
        self
    }

    pub fn with_description(mut self, desc: &'static str) -> Self {
        self.description = desc;
        self
    }

    /// Attach a session protocol to this port.
    ///
    /// When set, the port's operations must conform to this protocol,
    /// and a peer port's protocol must be dual (checked in `can_link_to`).
    pub fn with_session(mut self, session: SessionType) -> Self {
        self.session = Some(session);
        self
    }
}

// ── Port schema ───────────────────────────────────────────────────────────────

/// The complete set of ports an entity exposes.
#[derive(Debug, Clone)]
pub struct PortSchema {
    ports: Vec<PortDecl>,
    // Cached indices for fast lookup.
    primary_in: Option<usize>,
    primary_out: Option<usize>,
    observe_out: Option<usize>,
}

impl PortSchema {
    pub fn new() -> Self {
        Self {
            ports: Vec::new(),
            primary_in: None,
            primary_out: None,
            observe_out: None,
        }
    }

    pub fn with(mut self, decl: PortDecl) -> Self {
        // O(1) push — duplicate name detection is deferred to `validate()`,
        // which uses a HashSet for O(P) total checking. This makes schema
        // construction O(P) instead of O(P²).
        let idx = self.ports.len();
        match (decl.dir, &decl.flow) {
            (PortDir::In, FlowKind::Data) if self.primary_in.is_none() => {
                self.primary_in = Some(idx);
            }
            (PortDir::Out, FlowKind::Data) if self.primary_out.is_none() => {
                self.primary_out = Some(idx);
            }
            (PortDir::Out, FlowKind::Observe) if self.observe_out.is_none() => {
                self.observe_out = Some(idx);
            }
            _ => {}
        }
        self.ports.push(decl);
        self
    }

    pub fn ports(&self) -> &[PortDecl] { &self.ports }
    pub fn is_empty(&self) -> bool { self.ports.is_empty() }
    pub fn len(&self) -> usize { self.ports.len() }

    pub fn find(&self, name: &str) -> Option<&PortDecl> {
        self.ports.iter().find(|p| p.name == name)
    }

    /// All input ports (any flow kind).
    pub fn inputs(&self) -> impl Iterator<Item = &PortDecl> {
        self.ports.iter().filter(|p| p.dir == PortDir::In)
    }

    /// All output ports (any flow kind).
    pub fn outputs(&self) -> impl Iterator<Item = &PortDecl> {
        self.ports.iter().filter(|p| p.dir == PortDir::Out)
    }

    /// All observe ports (output + Observe flow).
    pub fn observe_ports(&self) -> impl Iterator<Item = &PortDecl> {
        self.ports.iter().filter(|p| p.dir == PortDir::Out && p.flow == FlowKind::Observe)
    }

    /// Validate that this schema satisfies the mathematical definition of an
    /// interface set (Definition 2.2): no duplicate names, each port has a
    /// valid type. Returns `Ok(())` if valid, `Err(reason)` otherwise.
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut seen = crate::compat::HashSet::new();
        for p in &self.ports {
            if !seen.insert(p.name) {
                return Err("duplicate port name in schema");
            }
        }
        Ok(())
    }

    pub fn primary_input(&self) -> Option<&PortDecl> {
        self.primary_in.map(|i| &self.ports[i])
    }

    pub fn primary_output(&self) -> Option<&PortDecl> {
        self.primary_out.map(|i| &self.ports[i])
    }

    pub fn observe_port(&self) -> Option<&PortDecl> {
        self.observe_out.map(|i| &self.ports[i])
    }
}

// ── Link-compatibility check ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkCompat {
    Compatible,
    Migrate { from_ver: u32, to_ver: u32 },
    Incompatible { reason: &'static str },
}

impl PortDecl {
    pub fn can_link_to(&self, other: &PortDecl) -> LinkCompat {
        if self.dir != PortDir::Out {
            return LinkCompat::Incompatible { reason: "source port is not an output" };
        }
        if other.dir != PortDir::In {
            return LinkCompat::Incompatible { reason: "target port is not an input" };
        }
        if self.type_id != other.type_id {
            return LinkCompat::Incompatible { reason: "type mismatch" };
        }
        // FlowKind must match (Data↔Data, Control↔Control, Observe↔In ports don't connect)
        if self.flow != other.flow {
            return LinkCompat::Incompatible { reason: "flow kind mismatch" };
        }
        // Session protocol check: if either port has a protocol, both must
        // have protocols that are dual. If only one has a protocol, the
        // ports are incompatible (protocol-less port can't conform).
        match (&self.session, &other.session) {
            (None, None) => { /* no protocol constraint, OK */ }
            (Some(a), Some(b)) => {
                if !crate::session::is_dual(a, b) {
                    return LinkCompat::Incompatible {
                        reason: "session protocols are not dual",
                    };
                }
            }
            _ => {
                return LinkCompat::Incompatible {
                    reason: "one port has a session protocol, the other does not",
                };
            }
        }
        let ver_diff = if self.schema_ver > other.schema_ver {
            self.schema_ver - other.schema_ver
        } else {
            other.schema_ver - self.schema_ver
        };
        match ver_diff {
            0 => LinkCompat::Compatible,
            1 => LinkCompat::Migrate {
                from_ver: self.schema_ver.min(other.schema_ver),
                to_ver: self.schema_ver.max(other.schema_ver),
            },
            _ => LinkCompat::Incompatible { reason: "schema version drift > 1" },
        }
    }
}

// ── Port registry (runtime) ───────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct PortRegistry {
    entries: Vec<PortEntry>,
}

#[derive(Debug)]
pub struct PortEntry {
    pub name: &'static str,
    pub dir: PortDir,
    pub flow: FlowKind,
    pub type_name: &'static str,
    pub schema_ver: u32,
}

impl PortRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, decl: &PortDecl) {
        self.entries.push(PortEntry {
            name: decl.name,
            dir: decl.dir,
            flow: decl.flow,
            type_name: decl.type_name,
            schema_ver: decl.schema_ver,
        });
    }

    pub fn entries(&self) -> &[PortEntry] { &self.entries }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

// ── Config schema ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ConfigDecl {
    pub key: &'static str,
    pub type_name: &'static str,
    pub description: &'static str,
}

impl ConfigDecl {
    pub fn new<T: 'static>(key: &'static str, description: &'static str) -> Self {
        Self {
            key,
            type_name: core::any::type_name::<T>(),
            description,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConfigSchema {
    decls: Vec<ConfigDecl>,
}

impl ConfigSchema {
    pub fn new() -> Self { Self::default() }
    pub fn with(mut self, decl: ConfigDecl) -> Self {
        self.decls.push(decl);
        self
    }
    pub fn decls(&self) -> &[ConfigDecl] { &self.decls }
}

// ── MachineContext ────────────────────────────────────────────────────────────

/// Context provided to a Machine during its lifecycle.
///
/// Carries snapshot capabilities, lifecycle state, signal polling, time
/// access, and initial value injection.
///
/// # 0-cost design
///
/// `MachineContext` contains **no `Arc` allocations** in its hot path.
/// The old `observe_count` / `output_count` `Arc<AtomicUsize>` counters
/// were removed because:
/// 1. They were never wired by any runtime deploy path —
///    `observe_is_connected()` always returned `false` (a silent contract
///    violation, not a real optimization).
/// 2. They imposed an allocation tax on every `MachineContext::new()`,
///    contradicting the 0-cost abstraction goal.
/// 3. The same semantic ("skip work when nobody is listening") is better
///    expressed at the **deploy layer** (don't connect the port) or via
///    **type constraints** (`PortDecl::observe` already declares
///    `FlowKind::Observe` at compile time).
///
/// Observation-aware short-circuiting is now the runtime's responsibility,
/// not the Machine's — a Machine should always execute its full computation;
/// the runtime decides whether to route observation outputs.
pub struct MachineContext {
    /// Machine 实例名（`Cow` 使 runtime 可用 owned name 构造上下文，
    /// 与 `DeploySpec` 的 `Cow<'static, str>` 名称一致——消除了
    /// 从 `String` 反序列化名到 `&'static str` 的 `leak`）。
    name: Cow<'static, str>,
    /// Snapshot function (wired by runtime, optional).
    pub(crate) snapshot_fn: Option<Arc<dyn Fn() -> Option<Vec<u8>> + Send + Sync>>,
    /// Initial value injection for Source-like machines.
    /// Stored as type-erased `Arc<dyn Any + Send + Sync>`; machines downcast in `init()`.
    pub(crate) initial_value: Option<Arc<dyn core::any::Any + Send + Sync>>,
    /// Current lifecycle phase (set by runtime).
    lifecycle: AtomicU8,
    /// Pending system signals count (inc by runtime, polled by machine).
    signal_flag: AtomicU8,
    /// Current time in nanoseconds since epoch (set by runtime each tick).
    /// Full nanosecond precision — accessed via `time_tick()` / `time_ns()`.
    time_ns: AtomicU64,
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

/// The phase a Machine is currently in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum Lifecycle {
    Init     = 0,
    Running  = 1,
    Stopping = 2,
    Stopped  = 3,
}

impl Lifecycle {
    /// Progress to the next phase (monotonic forward only).
    pub fn next(self) -> Option<Self> {
        match self {
            Lifecycle::Init => Some(Lifecycle::Running),
            Lifecycle::Running => Some(Lifecycle::Stopping),
            Lifecycle::Stopping => Some(Lifecycle::Stopped),
            Lifecycle::Stopped => None,
        }
    }
    pub fn is_active(self) -> bool { self == Lifecycle::Running }
    pub fn is_terminal(self) -> bool { self == Lifecycle::Stopped }
}

// ── SystemSignal ──────────────────────────────────────────────────────────────

/// A signal sent from the runtime to a Machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemSignal {
    /// Request graceful shutdown after current process() completes.
    Shutdown,
    /// Request a state checkpoint (if supported).
    Checkpoint,
}

impl core::fmt::Debug for MachineContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MachineContext")
            .field("name", &self.name())
            .field("has_snapshot_fn", &self.snapshot_fn.is_some())
            .field("lifecycle", &self.lifecycle())
            .field("time_ns", &self.time_ns.load(Ordering::Relaxed))
            .finish()
    }
}

impl MachineContext {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            snapshot_fn: None,
            initial_value: None,
            lifecycle: AtomicU8::new(Lifecycle::Init as u8),
            signal_flag: AtomicU8::new(0),
            time_ns: AtomicU64::new(0),
        }
    }

    /// The machine instance name.
    pub fn name(&self) -> &str {
        &self.name
    }

    // ── Snapshot ─────────────────────────────────────────

    /// Returns a byte-serialized snapshot of state, if available.
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        self.snapshot_fn.as_ref().and_then(|f| f())
    }

    // ── Initial value injection ──────────────────────────

    /// Inject an initial value for Source-like machines.
    /// Called by the runtime/deploy layer before `init()`.
    pub fn set_initial_value<V: core::any::Any + Send + Sync + 'static>(&mut self, value: V) {
        self.initial_value = Some(Arc::new(value));
    }

    /// Retrieve the injected initial value, downcasting to `V`.
    /// Returns `None` if no value was injected or type mismatch.
    pub fn initial_value<V: core::any::Any + Send + Sync + 'static>(&self) -> Option<&V> {
        self.initial_value.as_ref()?.downcast_ref::<V>()
    }

    // ── Lifecycle ────────────────────────────────────────

    /// Current lifecycle phase of the machine.
    /// Set by the runtime. Machines can query this to adjust behaviour
    /// during shutdown (e.g. skip non-essential work during Stopping).
    pub fn lifecycle(&self) -> Lifecycle {
        match self.lifecycle.load(Ordering::Acquire) {
            0 => Lifecycle::Init,
            1 => Lifecycle::Running,
            2 => Lifecycle::Stopping,
            _ => Lifecycle::Stopped,
        }
    }

    // ── Time ─────────────────────────────────────────────

    /// Current time as a full-precision `TimeTick` (nanoseconds).
    /// Set by the runtime before each process() call.
    /// Returns `TimeTick::from_nanos(0)` if the runtime does not provide time.
    pub fn time_tick(&self) -> TimeTick {
        TimeTick::from_nanos(self.time_ns.load(Ordering::Relaxed))
    }

    /// Current time in nanoseconds (full precision).
    pub fn time_ns(&self) -> u64 {
        self.time_ns.load(Ordering::Relaxed)
    }

    // ── Runtime adapter API ──────────────────────────────

    /// Set the current lifecycle phase (called by runtime).
    pub fn set_lifecycle(&self, lc: Lifecycle) {
        self.lifecycle.store(lc as u8, Ordering::Release);
    }

    /// Set the current time from a `TimeTick` (called by runtime before process()).
    /// This is the only time-setting method — nanosecond precision preserved.
    pub fn set_time_tick(&self, tick: TimeTick) {
        self.time_ns.store(tick.ns, Ordering::Relaxed);
    }

    /// Send a signal to this machine (called by runtime).
    pub fn send_signal(&self, signal: SystemSignal) {
        let code = match signal {
            SystemSignal::Shutdown => 1,
            SystemSignal::Checkpoint => 2,
        };
        self.signal_flag.store(code, Ordering::Release);
    }

    /// Poll for a pending **advisory** signal (`Checkpoint`). Returns and
    /// clears `Checkpoint` if pending; returns `None` for `Shutdown`.
    ///
    /// `Shutdown` is **runtime-enforced** — the runtime peeks
    /// [`has_shutdown_signal`](Self::has_shutdown_signal) every iteration and
    /// breaks before calling `process()`. This method deliberately does NOT
    /// consume `Shutdown`, so a machine calling `poll_signal()` inside
    /// `process()` cannot accidentally clear a pending `Shutdown` before the
    /// runtime observes it. This eliminates the race between the machine's
    /// `poll_signal()` and the runtime's `has_shutdown_signal()` peek.
    ///
    /// # Algebraic basis
    ///
    /// ```text
    /// SystemSignal = Shutdown + Checkpoint
    /// Shutdown  = *must act*  → runtime-enforced (peek + break, never consumed here)
    /// Checkpoint = *may act*  → machine-handled (poll + save, consumed here)
    /// ```
    ///
    /// Should be called once at the top of `process()` to observe advisory
    /// checkpoint requests. A machine that never calls this method simply
    /// ignores checkpoints — but still shuts down when the runtime enforces
    /// `Shutdown`.
    pub fn poll_signal(&self) -> Option<SystemSignal> {
        let flag = self.signal_flag.load(Ordering::Acquire);
        if flag == 2 {
            // Checkpoint: consume and return.
            self.signal_flag.store(0, Ordering::Release);
            Some(SystemSignal::Checkpoint)
        } else {
            // 0 (nothing) or 1 (Shutdown): do NOT consume Shutdown.
            // The runtime will observe it via has_shutdown_signal().
            None
        }
    }

    /// Peek whether a `Shutdown` signal is pending, **without consuming it**.
    ///
    /// This is the runtime-side counterpart of [`poll_signal`](Self::poll_signal):
    /// the runtime peeks for `Shutdown` every loop iteration to enforce
    /// shutdown, while leaving `Checkpoint` signals for the machine to
    /// consume via `poll_signal()` inside `process()`. The split reflects the
    /// algebraic distinction:
    ///
    /// - `Shutdown`  = *must act*  → runtime-enforced (peek + break)
    /// - `Checkpoint` = *may act*  → machine-handled (poll + save)
    ///
    /// This preserves machine autonomy for advisory signals while making
    /// shutdown non-negotiable.
    #[inline]
    pub fn has_shutdown_signal(&self) -> bool {
        self.signal_flag.load(Ordering::Acquire) == 1
    }

    pub fn set_snapshot_fn(&mut self, f: Arc<dyn Fn() -> Option<Vec<u8>> + Send + Sync>) {
        self.snapshot_fn = Some(f);
    }
}

// ════════════════════════════════════════════════════════════════════════════
// TESTS
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionOp;
    use alloc::string::String;

    // ── can_link_to: five-dimension compatibility ─────────────────────────
    // Order of checks (short-circuit): dir → type → flow → session → version.

    #[test]
    fn can_link_to_compatible() {
        let src = PortDecl::output::<i32>("y");
        let dst = PortDecl::input::<i32>("x");
        let compat = src.can_link_to(&dst);
        assert!(matches!(compat, LinkCompat::Compatible));
    }

    #[test]
    fn can_link_to_source_not_output() {
        let src = PortDecl::input::<i32>("a");
        let dst = PortDecl::input::<i32>("b");
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Incompatible { reason: "source port is not an output" }
        ));
    }

    #[test]
    fn can_link_to_target_not_input() {
        let src = PortDecl::output::<i32>("a");
        let dst = PortDecl::output::<i32>("b");
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Incompatible { reason: "target port is not an input" }
        ));
    }

    #[test]
    fn can_link_to_type_mismatch() {
        let src = PortDecl::output::<i32>("a");
        let dst = PortDecl::input::<String>("b");
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Incompatible { reason: "type mismatch" }
        ));
    }

    #[test]
    fn can_link_to_flow_mismatch() {
        // src is Control, dst is Data → flow kind mismatch.
        let src = PortDecl::ctrl_out::<i32>("a");
        let dst = PortDecl::input::<i32>("b");
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Incompatible { reason: "flow kind mismatch" }
        ));
    }

    #[test]
    fn can_link_to_version_compatible() {
        let src = PortDecl::output::<i32>("a").with_schema_ver(0);
        let dst = PortDecl::input::<i32>("b").with_schema_ver(0);
        let compat = src.can_link_to(&dst);
        assert!(matches!(compat, LinkCompat::Compatible));
    }

    #[test]
    fn can_link_to_version_migrate() {
        let src = PortDecl::output::<i32>("a").with_schema_ver(0);
        let dst = PortDecl::input::<i32>("b").with_schema_ver(1);
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Migrate { from_ver: 0, to_ver: 1 }
        ));
    }

    #[test]
    fn can_link_to_version_incompatible() {
        let src = PortDecl::output::<i32>("a").with_schema_ver(0);
        let dst = PortDecl::input::<i32>("b").with_schema_ver(2);
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Incompatible { reason: "schema version drift > 1" }
        ));
    }

    #[test]
    fn can_link_to_version_migrate_reverse() {
        // from_ver/to_ver use min/max so from <= to regardless of direction.
        let src = PortDecl::output::<i32>("a").with_schema_ver(1);
        let dst = PortDecl::input::<i32>("b").with_schema_ver(0);
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Migrate { from_ver: 0, to_ver: 1 }
        ));
    }

    #[test]
    fn can_link_to_session_both_none() {
        let src = PortDecl::output::<i32>("a");
        let dst = PortDecl::input::<i32>("b");
        let compat = src.can_link_to(&dst);
        assert!(matches!(compat, LinkCompat::Compatible));
    }

    #[test]
    fn can_link_to_session_one_has() {
        let src = PortDecl::output::<i32>("a")
            .with_session(SessionType::single(SessionOp::End));
        let dst = PortDecl::input::<i32>("b");
        let compat = src.can_link_to(&dst);
        assert!(matches!(
            compat,
            LinkCompat::Incompatible {
                reason: "one port has a session protocol, the other does not"
            }
        ));
    }

    // ── PortSchema ────────────────────────────────────────────────────────

    #[test]
    fn port_schema_find() {
        let schema = PortSchema::new()
            .with(PortDecl::output::<i32>("y"))
            .with(PortDecl::input::<i32>("x"));
        assert!(schema.find("y").is_some());
        assert!(schema.find("x").is_some());
        assert!(schema.find("z").is_none());
    }

    #[test]
    fn port_schema_primary_indices() {
        // First In+Data port becomes primary_input.
        let schema = PortSchema::new()
            .with(PortDecl::input::<i32>("x"))
            .with(PortDecl::input::<i32>("x2"));
        assert_eq!(schema.primary_input().map(|p| p.name), Some("x"));
    }

    #[test]
    fn port_schema_validate_rejects_duplicate() {
        let schema = PortSchema::new()
            .with(PortDecl::input::<i32>("x"))
            .with(PortDecl::output::<i32>("x"));
        assert!(schema.validate().is_err());
    }

    #[test]
    fn port_schema_inputs_outputs() {
        let schema = PortSchema::new()
            .with(PortDecl::input::<i32>("in1"))
            .with(PortDecl::output::<i32>("out1"))
            .with(PortDecl::ctrl_in::<i32>("ctrl_in"))
            .with(PortDecl::ctrl_out::<i32>("ctrl_out"));
        assert_eq!(schema.inputs().count(), 2);
        assert_eq!(schema.outputs().count(), 2);
    }

    // ── MachineContext signal semantics ───────────────────────────────────
    // poll_signal consumes Checkpoint (flag==2) but NOT Shutdown (flag==1);
    // has_shutdown_signal peeks flag==1 without consuming.

    #[test]
    fn context_poll_signal_none_initially() {
        let ctx = MachineContext::new("test");
        assert!(ctx.poll_signal().is_none());
    }

    #[test]
    fn context_poll_signal_checkpoint_consumed() {
        let ctx = MachineContext::new("test");
        ctx.send_signal(SystemSignal::Checkpoint);
        assert_eq!(ctx.poll_signal(), Some(SystemSignal::Checkpoint));
        // Already consumed — second poll returns None.
        assert!(ctx.poll_signal().is_none());
    }

    #[test]
    fn context_poll_signal_shutdown_not_consumed() {
        let ctx = MachineContext::new("test");
        ctx.send_signal(SystemSignal::Shutdown);
        // Shutdown is not returned by poll_signal (runtime-enforced, not
        // machine-consumed).
        assert!(ctx.poll_signal().is_none());
        // ... and it remains peekable.
        assert!(ctx.has_shutdown_signal());
    }

    #[test]
    fn context_has_shutdown_signal() {
        let ctx = MachineContext::new("test");
        assert!(!ctx.has_shutdown_signal());
        ctx.send_signal(SystemSignal::Shutdown);
        assert!(ctx.has_shutdown_signal());
    }

    #[test]
    fn context_send_signal_overwrites() {
        // send_signal is a covering store (not fetch_or): a later Checkpoint
        // overwrites the earlier Shutdown, flipping the flag from 1 to 2.
        let ctx = MachineContext::new("test");
        ctx.send_signal(SystemSignal::Shutdown);
        ctx.send_signal(SystemSignal::Checkpoint);
        assert!(!ctx.has_shutdown_signal());
    }

    #[test]
    fn context_lifecycle_roundtrip() {
        let ctx = MachineContext::new("test");
        ctx.set_lifecycle(Lifecycle::Running);
        assert_eq!(ctx.lifecycle(), Lifecycle::Running);
        ctx.set_lifecycle(Lifecycle::Stopped);
        assert_eq!(ctx.lifecycle(), Lifecycle::Stopped);
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────

    #[test]
    fn lifecycle_next_chain() {
        assert_eq!(Lifecycle::Init.next(), Some(Lifecycle::Running));
        assert_eq!(Lifecycle::Running.next(), Some(Lifecycle::Stopping));
        assert_eq!(Lifecycle::Stopping.next(), Some(Lifecycle::Stopped));
        assert_eq!(Lifecycle::Stopped.next(), None);
    }

    #[test]
    fn lifecycle_is_active() {
        assert!(!Lifecycle::Init.is_active());
        assert!(Lifecycle::Running.is_active());
        assert!(!Lifecycle::Stopping.is_active());
        assert!(!Lifecycle::Stopped.is_active());
    }

    #[test]
    fn lifecycle_is_terminal() {
        assert!(!Lifecycle::Init.is_terminal());
        assert!(!Lifecycle::Running.is_terminal());
        assert!(!Lifecycle::Stopping.is_terminal());
        assert!(Lifecycle::Stopped.is_terminal());
    }

    // ── initial_value injection ───────────────────────────────────────────

    #[test]
    fn context_initial_value() {
        let mut ctx = MachineContext::new("test");
        ctx.set_initial_value(42i32);
        assert_eq!(ctx.initial_value::<i32>(), Some(&42));
        // Type mismatch → None.
        assert_eq!(ctx.initial_value::<String>(), None);
    }
}
