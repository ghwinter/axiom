/// Machine — Layer 2: a stateful, ported, computable Entity.
///
/// # Architecture
///
/// ```text
/// IO-Object = (S, I, O, δ)        ← the minimal model
/// Entity    = (S, name)            ← persistent existence
/// Machine   = Entity + ports + δ  ← Entity with typed I/O and process()
/// ```
///
/// A Machine has everything an Entity has, plus:
/// - Typed input/output **interface sets** (`type Input`, `type Output` — each
///   is an enum with one variant per port, implementing `HasPortInfo`)
/// - A `type Ports: PortSet` that connects the enums to a `PortSchema`
///   (auto-derived — no manual `port_schema()` needed)
/// - A computation function `process(state, input) -> output`
/// - Configurable parameters (via `config_schema()`)
///
/// # The type/value unification principle
///
/// Mathematically (foundations.md §2), a Machine's input/output are interface
/// *sets* Γ = {p₁, p₂, …}. The trait encodes this by requiring:
///
/// - `type Input: HasPortInfo`  — the input interface, as an enum
/// - `type Output: HasPortInfo` — the output interface, as an enum
/// - `type Ports: PortSet`      — connects the enums to port declarations
///
/// `port_schema()` is auto-derived from `Self::Ports::port_schema()`.
/// There is no gap between type-space (the enum) and value-space (the schema):
/// they are two views of the same interface set Γ.
///
/// # Sync design
/// All methods are synchronous. The runtime adapter is responsible for
/// wrapping them in async tasks or spawning dedicated threads.

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use crate::port::{PortSchema, ConfigSchema, MachineContext, Lifecycle};
use crate::portset::{HasPortInfo, PortSet};
use crate::resource::{MachinePhysicalSpec, ResourceClass};
use crate::entity::EntityRestoreError;
use core::marker::PhantomData;

pub trait Machine: Send + Sync + 'static {
    /// Persistent state — heap-allocated, observable.
    type State: Send + 'static;

    /// The input interface set Γ_in: an enum with one variant per input port.
    ///
    /// Each variant carries the payload type of that port. The enum itself
    /// implements `HasPortInfo`, which provides runtime port introspection
    /// (port name, flow kind, TypeId) for dynamic dispatch.
    type Input: HasPortInfo;

    /// The output interface set Γ_out: an enum with one variant per output port.
    ///
    /// Observation data is just an output variant whose port is labelled
    /// `FlowKind::Observe`. There is no separate `Obs` type — the model
    /// stays at exactly (S, I, O, δ, ρ).
    type Output: HasPortInfo;

    /// The PortSet that connects `Input` and `Output` to a `PortSchema`.
    ///
    /// `port_schema()` is auto-derived from this. Use `declare_ports!` to
    /// generate a PortSet, or use `SinglePorts<T>` for single-port machines.
    type Ports: PortSet<Input = Self::Input, Output = Self::Output>;

    /// 机器的输出类型——`SingleOutput` 或 `MultiOutput`。
    ///
    /// - 1:1 机器设 `type ProcessOutput = SingleOutput<Self::Output>`
    /// - fan-out 机器设 `type ProcessOutput = MultiOutput<Self::Output>`
    ///
    /// 此关联类型是 `FusedInline` 安全性的类型基础：
    /// `FusedInline` 要求 `ProcessOutput = SingleOutput<Self::Output>`，
    /// 而 `SingleOutput` 类型层不含 `YieldMulti`，编译器完备阻止 fan-out 误用。
    type ProcessOutput: MachineOutput<Self::Output>;

    /// Human-readable name.
    fn name() -> &'static str
    where
        Self: Sized;

    /// Declare the machine's port interface.
    ///
    /// Default: auto-derived from `Self::Ports::port_schema()`.
    /// Override only if you need a custom schema (rare).
    fn port_schema() -> PortSchema
    where
        Self: Sized,
    {
        Self::Ports::port_schema()
    }

    /// Declare the machine's configuration parameters.
    fn config_schema() -> ConfigSchema
    where
        Self: Sized;

    /// Initialize: acquire resources, register ports and configs.
    ///
    /// **0-cost note**: implementations should mark this `#[inline]` (or
    /// `#[inline(always)]`) to enable cross-crate inlining. Without it, the
    /// fused pipeline cannot inline `init` and pays a setup tax.
    fn init(ctx: &MachineContext) -> Result<Self::State, InitError>
    where
        Self: Sized;

    /// Process one unit of work.
    ///
    /// Returns `Self::ProcessOutput`（`SingleOutput` 或 `MultiOutput`）:
    /// - `Yield(out)` — produce one output value on one port
    /// - `YieldMulti(outs)` — produce multiple output values (仅 `MultiOutput`)
    /// - `Idle` — no output this tick
    /// - `Done` — machine finished, transition to Stopping
    ///
    /// **0-cost cornerstone**: implementations MUST mark this `#[inline]`
    /// (or `#[inline(always)]`) to enable cross-crate inlining. A future fused
    /// pipeline mechanism will inline this method into a single loop; without
    /// cross-crate inlining the loop cannot fuse, stage boundaries remain
    /// function-call barriers, and the "data flow" metaphor fails to dissolve
    /// into pure computation.
    fn process(
        state: &mut Self::State,
        ctx: &MachineContext,
        input: Self::Input,
    ) -> Self::ProcessOutput;

    /// Clean up resources before destruction.
    ///
    /// **0-cost note**: implementations should mark this `#[inline]`.
    fn cleanup(state: Self::State, ctx: &MachineContext) -> Result<(), CleanupError>;

    // ── Physical resource specification ────────────────────

    /// Physical resource declaration. Used by the deployer to allocate
    /// threads, budget memory, and schedule the machine.
    fn physical_spec() -> MachinePhysicalSpec
    where
        Self: Sized,
    {
        MachinePhysicalSpec::default()
    }

    /// Resource classification for lifecycle-aware resource tracking.
    fn resource_classes() -> &'static [ResourceClass]
    where
        Self: Sized,
    {
        &[]
    }

    // ── Optional ──────────────────────────────────────────

    /// Whether this machine is deterministic (replay-safe).
    fn deterministic() -> bool
    where
        Self: Sized,
    {
        false
    }

    /// Serialize state for checkpoint/restore.
    ///
    /// # Positioning
    ///
    /// This is the **explicit home of snapshot complexity** — the counterpart
    /// of `HasPortInfo` carrying no `Clone` bound: state that needs to survive
    /// a restart or be audited declares serialization *here*, rather than
    /// forcing every port payload to be clonable. Defaults to `None`
    /// (no checkpoint support); a machine opts in by implementing it.
    fn checkpoint(_state: &Self::State) -> Option<Vec<u8>> {
        None
    }

    /// Deserialize and restore state from a checkpoint.
    fn restore(
        _state: &mut Self::State,
        _data: &[u8],
    ) -> Result<(), EntityRestoreError> {
        Err(EntityRestoreError::NotSupported)
    }
}

// ── Moore marker trait ───────────────────────────────────────────────────────

/// Marker trait for machines that implement **Moore semantics**.
///
/// A Moore machine's output depends ONLY on its **pre-update** state:
/// ```text
/// o  = λ(s_old)           ← output from pre-update state
/// s' = δ(s_old, i)        ← state transition
/// ```
/// This is in contrast to a Mealy machine, where output depends on both
/// state and input: `o = λ(s, i)`.
///
/// # Why this matters: feedback loops
///
/// The Moore property is what makes feedback loops algebraically safe.
/// In a cycle `M₁ → M₂ → M₁`, if both machines are Mealy, the output of
/// M₁ at tick `t` depends on its input at tick `t`, which depends on M₂'s
/// output at tick `t`, which depends on M₁'s output at tick `t` — an
/// algebraic loop with no solution.
///
/// If at least one machine is Moore, its output at tick `t` depends on its
/// state from tick `t-1` (not on its input at tick `t`). This breaks the
/// loop: `o₁(t) = λ₁(s₁(t-1))`, `o₂(t) = λ₂(s₂(t-1), o₁(t))`, and
/// `s₁(t) = δ₁(s₁(t-1), o₂(t))` — all defined.
///
/// # Usage
///
/// Implement both `Machine` and `Moore`:
///
/// ```ignore
/// impl Machine for Actuator {
///     fn process(state: &mut State, ctx: &MachineContext, input: Input) -> ProcessOutput<Output> {
///         let yielded = Output::state_out(state.current); // λ(s_old)
///         state.current = ...;                              // δ(s, i)
///         ProcessOutput::Yield(yielded)
///     }
/// }
/// impl Moore for Actuator {}
/// ```
///
/// This trait has no methods — it is a pure marker for compile-time
/// documentation and static analysis. The deploy layer can declare Moore
/// machines in `MachineInstance::is_moore` for cycle-safety checks.
pub trait Moore: Machine {}

// ── Output types: SingleOutput / MultiOutput / MachineOutput ─────────────────
//
// 1:1 机器返回 `SingleOutput<O>`，fan-out 机器返回 `MultiOutput<O>`。
// `SingleOutput` 类型层不含 `YieldMulti`——这是 `FusedInline` 安全性的
// 类型基础：编译器完备地阻止 fan-out 机器进入融合流水线，无需 unsafe。
//
// `ProcessOutput<O>` 保留为统一类型，供通用 runtime 通过
// `MachineOutput::into_process_output()` 转换后使用。

/// 1:1 机器的输出类型。
///
/// 仅含 `Yield`/`Idle`/`Done`——**类型层不含 `YieldMulti`**。
/// 这是 `FusedInline` 安全性的类型基础：一个返回 `SingleOutput` 的机器
/// 在类型层就不可能构造 `YieldMulti`，无需 `unsafe` 承诺。
#[derive(Debug, Clone, PartialEq)]
pub enum SingleOutput<O> {
    /// 单个输出值。
    Yield(O),
    /// 无输出；机器等待或空闲。
    Idle,
    /// 机器已完成，应转换到 Stopping。
    Done,
}

/// 1:N 机器的输出类型（fan-out）。
///
/// 含 `YieldMulti`，用于 Tee 等 fan-out 机器。
/// 返回此类型的机器**不能**实现 `FusedInline`（编译期拒绝）。
#[derive(Debug, Clone, PartialEq)]
pub enum MultiOutput<O> {
    /// 单个输出值（fan-out 机器也可能仅产出一个）。
    Yield(O),
    /// 多个输出值，各在其端口上（fan-out）。
    ///
    /// runtime 按顺序投递每个值。向量内顺序保留用于确定性投递。
    YieldMulti(Vec<O>),
    /// 无输出；机器等待或空闲。
    Idle,
    /// 机器已完成，应转换到 Stopping。
    Done,
}

// ── Sealed trait for output types ────────────────────────────────────────────

mod output_private {
    pub trait SealedOutput {}
    impl<O> SealedOutput for super::SingleOutput<O> {}
    impl<O> SealedOutput for super::MultiOutput<O> {}
    impl<O> SealedOutput for super::TupleOutput<O> {}
}

/// 机器输出的统一 trait（sealed）。
///
/// 外部无法引入新的输出类型——只能使用 `SingleOutput` 或 `MultiOutput`。
/// 1:1 机器设 `type ProcessOutput = SingleOutput<Self::Output>`，
/// fan-out 机器设 `type ProcessOutput = MultiOutput<Self::Output>`。
///
/// 通用 runtime 通过 `into_process_output()` 转换为统一的 `ProcessOutput<O>`
/// 后处理；FusedInline pipeline 消费者直接接收 `SingleOutput`，不转换。
pub trait MachineOutput<O>: output_private::SealedOutput {
    /// 转换为统一的 `ProcessOutput<O>`（供通用 runtime 使用）。
    ///
    /// 这是一次 variant tag 重映射，LLVM 优化为 noop。
    fn into_process_output(self) -> ProcessOutput<O>;

    /// 收集所有输出到向量。`Idle`/`Done` 产生空向量；
    /// `Done` 通过第二个元素标记终止。
    ///
    /// **注意**：此便捷方法每次分配 `Vec`——是**非热路径** API。runtime
    /// 适配器应直接 match `ProcessOutput`（或 `Self::ProcessOutput`）逐项
    /// 投递，避免在每条消息上付 Vec 分配税（E12 已验证 `YieldMulti` 路径
    /// 的 Vec 仅在扇出时分配；此方法把同样的分配带到每个 `Yield`）。
    fn into_outputs(self) -> (Vec<O>, bool);
}

impl<O> MachineOutput<O> for SingleOutput<O> {
    #[inline]
    fn into_process_output(self) -> ProcessOutput<O> {
        match self {
            SingleOutput::Yield(o) => ProcessOutput::Yield(o),
            SingleOutput::Idle => ProcessOutput::Idle,
            SingleOutput::Done => ProcessOutput::Done,
        }
    }
    #[inline]
    fn into_outputs(self) -> (Vec<O>, bool) {
        match self {
            SingleOutput::Yield(o) => (vec![o], false),
            SingleOutput::Idle => (vec![], false),
            SingleOutput::Done => (vec![], true),
        }
    }
}

impl<O> MachineOutput<O> for MultiOutput<O> {
    #[inline]
    fn into_process_output(self) -> ProcessOutput<O> {
        match self {
            MultiOutput::Yield(o) => ProcessOutput::Yield(o),
            MultiOutput::YieldMulti(os) => ProcessOutput::YieldMulti(os),
            MultiOutput::Idle => ProcessOutput::Idle,
            MultiOutput::Done => ProcessOutput::Done,
        }
    }
    #[inline]
    fn into_outputs(self) -> (Vec<O>, bool) {
        match self {
            MultiOutput::Yield(o) => (vec![o], false),
            MultiOutput::YieldMulti(os) => (os, false),
            MultiOutput::Idle => (vec![], false),
            MultiOutput::Done => (vec![], true),
        }
    }
}

// ── TupleOutput: fixed multi-port output (1:1:1, no data fan-out) ────────────

/// 固定双端口输出类型：每次 `process` 恰好产出**两个**值，各在一个端口上。
///
/// 这是“多端口 1:1:1”机器（如数据端口 + 观察端口各产出一个）的输出类型。
/// 与 `MultiOutput`（含 `YieldMulti`，真 fan-out 1:N）不同，`TupleOutput`
/// 在类型层**不含数量不确定性**——恰好两个值，无数据扇出，因此可以安全
/// 进入 `FusedInline` 融合流水线（融合器无需运行时决策即可完整处理）。
///
/// # 与 `MultiOutput` 的代数区别
///
/// - `MultiOutput::YieldMulti(Vec<O>)`：输出数量是运行时的 → 数据扇出，
///   进融合流水线会丢数据 → 被 `FusedInline` 拒绝（编译期）。
/// - `TupleOutput::Yield(O, O)`：输出数量固定为 2，每个值走其 variant
///   声明的端口 → 无数据扇出 → 可安全融合。
#[derive(Debug, Clone, PartialEq)]
pub enum TupleOutput<O> {
    /// 恰好两个输出值（各走其 variant 对应的端口）。
    Yield(O, O),
    /// 无输出；机器等待或空闲。
    Idle,
    /// 机器已完成，应转换到 Stopping。
    Done,
}

impl<O> MachineOutput<O> for TupleOutput<O> {
    #[inline]
    fn into_process_output(self) -> ProcessOutput<O> {
        match self {
            TupleOutput::Yield(a, b) => ProcessOutput::YieldMulti(vec![a, b]),
            TupleOutput::Idle => ProcessOutput::Idle,
            TupleOutput::Done => ProcessOutput::Done,
        }
    }
    #[inline]
    fn into_outputs(self) -> (Vec<O>, bool) {
        match self {
            TupleOutput::Yield(a, b) => (vec![a, b], false),
            TupleOutput::Idle => (vec![], false),
            TupleOutput::Done => (vec![], true),
        }
    }
}

// ── ProcessOutput (unified, for runtime) ─────────────────────────────────────

/// 统一输出类型（runtime 内部使用）。
///
/// 机器不再直接返回此类型——它们返回 `SingleOutput` 或 `MultiOutput`。
/// runtime 通过 `MachineOutput::into_process_output()` 转换。
///
/// 保留 `YieldMulti` variant 以便统一 match 处理。
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessOutput<O> {
    /// 单个输出值。
    Yield(O),
    /// 多个输出值（fan-out）。
    YieldMulti(Vec<O>),
    /// 无输出。
    Idle,
    /// 机器已完成。
    Done,
}

impl<O> ProcessOutput<O> {
    /// 收集所有输出到向量。
    pub fn into_outputs(self) -> (Vec<O>, bool) {
        match self {
            ProcessOutput::Yield(o) => (vec![o], false),
            ProcessOutput::YieldMulti(os) => (os, false),
            ProcessOutput::Idle => (vec![], false),
            ProcessOutput::Done => (vec![], true),
        }
    }
}

// ── FusedInline marker trait (0-cost pipeline contract, compile-time safe) ───

/// Marker trait for Machines whose `process` is safe for fused inline
/// pipelines.
///
/// # Compile-time safety (no `unsafe`)
///
/// `FusedInline` 要求 `Machine::ProcessOutput: FusedCompatible`，即输出类型
/// 为 `SingleOutput`（恰好一个输出）或 `TupleOutput`（恰好两个输出）。
/// 两者的共同点是**输出数量在类型层确定，不含 `YieldMulti`**——编译器
/// 在类型层完备地阻止 fan-out（1:N）机器实现此 trait，无需 `unsafe`，
/// 无逃生口，无运行时检查。
///
/// ```ignore
/// // Tee 返回 MultiOutput，因此无法实现 FusedInline：
/// impl FusedInline for Tee {}  // E0277: trait bound not satisfied
/// ```
///
/// # Contract
///
/// 实现此 trait 是一项承诺：
/// 1. `process` 返回 `SingleOutput` 或 `TupleOutput`（类型保证——输出数量
///    固定，不含 `YieldMulti`）
/// 2. `process` 标记 `#[inline]` 或 `#[inline(always)]`（仍需人工保证）
///
/// 第 1 项由类型系统完备验证；第 2 项是文档约定（`#[inline]` 是建议非保证，
/// 无法用类型表达，但跨 crate 内联在 `-O` 下通常发生）。
///
/// # Why this exists
///
/// A fused pipeline mechanism fuses N stages into a single loop. This is only
/// correct when each stage's output count is known at compile time: a stage
/// producing `YieldMulti` (runtime count) would force the fused loop to either
/// drop the extra outputs — a data-loss bug — or allocate dynamically.
/// `SingleOutput`/`TupleOutput` keep the count static, so the fused loop can
/// handle every output without runtime decisions. The `FusedInline` constraint
/// makes misuse a compile-time error instead.
///
/// This is the **type-contract layer** of axiom's 0-cost abstraction: the
/// compiler enforces that only fixed-output-count Machines enter the fused
/// pipeline path.
///
/// # Positioning: forward-declared contract
///
/// This trait has **no consumer in axiom core** today. It is a forward
/// declaration of the type-contract that a future fused-pipeline mechanism
/// will require:
///
/// ```ignore
/// fn pipeline3<A: FusedInline, B: FusedInline, C: FusedInline>(...) { ... }
/// ```
pub trait FusedInline: Machine
where
    Self::ProcessOutput: FusedCompatible,
{
}

/// 可安全进入 `FusedInline` 融合流水线的输出类型（sealed）。
///
/// 仅由 `SingleOutput`（恰好一个输出）与 `TupleOutput`（恰好两个输出）
/// 实现——两者在类型层都**不含 `YieldMulti`**，输出数量固定，融合器
/// 无需运行时决策即可完整处理每个输出。`MultiOutput`（含 `YieldMulti`，
/// 运行时的输出数量）被排除。
///
/// 外部无法绕过——`SealedFused` 是 private trait，无 `unsafe`，编译器完备。
pub trait FusedCompatible: fused_compat_private::SealedFused {}

mod fused_compat_private {
    pub trait SealedFused {}
    impl<O> SealedFused for super::SingleOutput<O> {}
    impl<O> SealedFused for super::TupleOutput<O> {}
}

impl<O> FusedCompatible for SingleOutput<O> {}
impl<O> FusedCompatible for TupleOutput<O> {}


// ── Error types ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum InitError {
    ResourceAcquisitionFailed(String),
    ConfigurationInvalid(String),
    PortRegistrationFailed(String),
    Other(String),
}

impl core::fmt::Display for InitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ResourceAcquisitionFailed(s) => write!(f, "resource: {}", s),
            Self::ConfigurationInvalid(s) => write!(f, "config: {}", s),
            Self::PortRegistrationFailed(s) => write!(f, "port: {}", s),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for InitError {}

#[derive(Debug)]
pub enum CleanupError {
    ResourceReleaseFailed(String),
    Timeout,
    Other(String),
}

impl core::fmt::Display for CleanupError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ResourceReleaseFailed(s) => write!(f, "resource release: {}", s),
            Self::Timeout => write!(f, "timeout"),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CleanupError {}

// ════════════════════════════════════════════════════════════════════════════
// Lifecycle typestate — compile-time enforcement of init → process → cleanup
// ════════════════════════════════════════════════════════════════════════════
//
// The runtime `Lifecycle` enum (in `port.rs`) is stored as an `AtomicU8` so
// that the runtime can signal shutdown across threads. But it cannot prevent
// a programming error like calling `cleanup()` before `process()` has
// finished, or calling `process()` after `cleanup()`.
//
// The typestate pattern encodes the lifecycle phase as a **type parameter**,
// making invalid state transitions unrepresentable at compile time:
//
// ```text
// MachineHandle<M, Init>     ──start()──►  MachineHandle<M, Running>
// MachineHandle<M, Running>  ──stop()───►  MachineHandle<M, Stopping>
// MachineHandle<M, Stopping> ──finish()──► MachineHandle<M, Stopped>
// MachineHandle<M, Stopped>  ──cleanup()──► ()
// ```
//
// Each state exposes only the methods valid in that state:
// - `Init`     → `start()`
// - `Running`  → `process()`, `stop()`
// - `Stopping` → `process()` (for draining), `finish()`
// - `Stopped`  → `cleanup()`
//
// It is impossible to call `process()` on a `Stopped` machine — the method
// does not exist on that type. The compiler rejects it.

/// Typestate marker: the machine has been initialised but not yet started.
///
/// In this state, the machine's state exists but `process()` cannot be
/// called. The only valid transition is [`MachineHandle::start()`].
pub struct Init;

/// Typestate marker: the machine is actively processing inputs.
///
/// `process()` is available. The machine transitions to `Stopping` via
/// [`MachineHandle::stop()`] when a shutdown signal is received or the
/// machine returns `ProcessOutput::Done`.
pub struct Running;

/// Typestate marker: the machine is draining in-flight work before shutdown.
///
/// `process()` is still available so the machine can flush pending outputs,
/// but `ctx.lifecycle()` returns `Lifecycle::Stopping`, allowing the machine
/// to skip non-essential work. The only forward transition is
/// [`MachineHandle::finish()`].
pub struct Stopping;

/// Typestate marker: the machine has stopped and is ready for cleanup.
///
/// `process()` is no longer available. The only valid operation is
/// [`MachineHandle::cleanup()`], which consumes the handle and releases
/// all resources.
pub struct Stopped;

/// A typed handle to a machine that carries its lifecycle phase as a type
/// parameter.
///
/// The type parameter `S` is one of [`Init`], [`Running`], [`Stopping`],
/// or [`Stopped`]. Each state exposes a different set of methods, making
/// invalid lifecycle transitions a compile-time error.
///
/// # Example
///
/// ```ignore
/// use axiom::machine::{MachineHandle, Init, Running, Stopped};
/// use axiom::prelude_all::*;
///
/// let ctx = MachineContext::new("acc");
/// let handle = MachineHandle::<Accumulator, Init>::new(ctx)?;
/// let mut running = handle.start();
///
/// let out = running.process(AccumulatorInput::in_(42.0));
///
/// let stopped = running.stop().finish();
/// stopped.cleanup()?;
/// ```
///
/// # Compile-time guarantees
///
/// The following are rejected by the compiler:
///
/// ```ignore
/// let handle = MachineHandle::<M, Init>::new(ctx)?;
/// handle.process(input);  // ERROR: no method `process` on Init
///
/// let stopped = handle.start().stop().finish();
/// stopped.process(input); // ERROR: no method `process` on Stopped
/// stopped.cleanup();
/// stopped.cleanup();      // ERROR: use of moved value
/// ```
pub struct MachineHandle<M: Machine, S> {
    state: M::State,
    ctx: MachineContext,
    _marker: PhantomData<S>,
}

// ── Init state ──────────────────────────────────────────────────────────────

impl<M: Machine> MachineHandle<M, Init> {
    /// Initialise a new machine handle.
    ///
    /// Calls `M::init()` to acquire resources and create the initial state.
    /// The returned handle is in the `Init` state — `process()` is not yet
    /// available; call [`start()`](Self::start) to transition to `Running`.
    pub fn new(ctx: MachineContext) -> Result<Self, InitError> {
        let state = M::init(&ctx)?;
        Ok(Self {
            state,
            ctx,
            _marker: PhantomData,
        })
    }

    /// Transition from `Init` to `Running`.
    ///
    /// Sets the runtime lifecycle flag to `Lifecycle::Running` and returns
    /// a handle in the `Running` state, which has `process()` available.
    ///
    /// This consumes `self` — the `Init` handle cannot be reused.
    pub fn start(self) -> MachineHandle<M, Running> {
        self.ctx.set_lifecycle(Lifecycle::Running);
        MachineHandle {
            state: self.state,
            ctx: self.ctx,
            _marker: PhantomData,
        }
    }
}

// ── Running state ───────────────────────────────────────────────────────────

impl<M: Machine> MachineHandle<M, Running> {
    /// Process one unit of work.
    ///
    /// Only available in the `Running` state. Delegates to
    /// `M::process()` with the current state and context.
    pub fn process(&mut self, input: M::Input) -> M::ProcessOutput {
        M::process(&mut self.state, &self.ctx, input)
    }

    /// Transition from `Running` to `Stopping`.
    ///
    /// Sets the runtime lifecycle flag to `Lifecycle::Stopping`. The
    /// returned handle still allows `process()` (for draining), but the
    /// machine can query `ctx.lifecycle()` to detect that shutdown is
    /// requested.
    pub fn stop(self) -> MachineHandle<M, Stopping> {
        self.ctx.set_lifecycle(Lifecycle::Stopping);
        MachineHandle {
            state: self.state,
            ctx: self.ctx,
            _marker: PhantomData,
        }
    }
}

// ── Stopping state ──────────────────────────────────────────────────────────

impl<M: Machine> MachineHandle<M, Stopping> {
    /// Process one unit of work during shutdown (draining).
    ///
    /// Available in `Stopping` so the machine can flush pending outputs.
    /// The machine should check `ctx.lifecycle()` (which returns
    /// `Lifecycle::Stopping`) to skip non-essential work.
    pub fn process(&mut self, input: M::Input) -> M::ProcessOutput {
        M::process(&mut self.state, &self.ctx, input)
    }

    /// Transition from `Stopping` to `Stopped`.
    ///
    /// Sets the runtime lifecycle flag to `Lifecycle::Stopped`. The
    /// returned handle no longer allows `process()` — only `cleanup()`.
    pub fn finish(self) -> MachineHandle<M, Stopped> {
        self.ctx.set_lifecycle(Lifecycle::Stopped);
        MachineHandle {
            state: self.state,
            ctx: self.ctx,
            _marker: PhantomData,
        }
    }
}

// ── Stopped state ───────────────────────────────────────────────────────────

impl<M: Machine> MachineHandle<M, Stopped> {
    /// Clean up resources and consume the handle.
    ///
    /// Only available in the `Stopped` state. Calls `M::cleanup()` to
    /// release resources. The handle is consumed and cannot be reused.
    pub fn cleanup(self) -> Result<(), CleanupError> {
        M::cleanup(self.state, &self.ctx)
    }
}

// ── Common accessors for all states ─────────────────────────────────────────

/// A sealed trait implemented by all lifecycle state markers.
///
/// This trait cannot be implemented outside this module, ensuring that
/// no external code can introduce new lifecycle states.
pub trait LifecycleState: private::Sealed {}

impl LifecycleState for Init {}
impl LifecycleState for Running {}
impl LifecycleState for Stopping {}
impl LifecycleState for Stopped {}

mod private {
    pub trait Sealed {}
    impl Sealed for super::Init {}
    impl Sealed for super::Running {}
    impl Sealed for super::Stopping {}
    impl Sealed for super::Stopped {}
}

impl<M: Machine, S: LifecycleState> MachineHandle<M, S> {
    /// Borrow the machine state (read-only).
    ///
    /// Available in all lifecycle states for inspection and checkpointing.
    pub fn state(&self) -> &M::State {
        &self.state
    }

    /// Borrow the machine context.
    ///
    /// Available in all lifecycle states.
    pub fn context(&self) -> &MachineContext {
        &self.ctx
    }
}

impl<M: Machine, S> core::fmt::Debug for MachineHandle<M, S>
where
    M::State: core::fmt::Debug,
    S: LifecycleState,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MachineHandle")
            .field("state", &self.state)
            .field("lifecycle", &self.ctx.lifecycle())
            .finish()
    }
}
