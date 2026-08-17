/// **Maturity: stable** (the stable core, main subject of the current refactor).
///
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

    /// The machine's output type — `SingleOutput` or `MultiOutput`.
    ///
    /// - 1:1 machines set `type ProcessOutput = SingleOutput<Self::Output>`
    /// - fan-out machines set `type ProcessOutput = MultiOutput<Self::Output>`
    ///
    /// This associated type is the type-level basis of `FusedInline` safety:
    /// `FusedInline` requires `ProcessOutput = SingleOutput<Self::Output>`,
    /// and `SingleOutput` has no `YieldMulti` at the type level, so the
    /// compiler completely blocks accidental fan-out misuse.
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

    /// Is the machine ready to be driven? Defaults to `true`.
    ///
    /// Override for machines whose initialization requires an asynchronous
    /// step (e.g. a runtime adapter backend that must connect before it can
    /// process). This mirrors the "async-ready" lifecycle pattern: a driver
    /// polls `is_ready` and only starts the machine once it returns `true`.
    ///
    /// `axiom-runtime` (synchronous) does not wait — it drives machines
    /// immediately; asynchronous runtimes (adapter layer) use this as the
    /// readiness declaration.
    fn is_ready(_ctx: &MachineContext) -> bool
    where
        Self: Sized,
    {
        true
    }

    /// Process one unit of work.
    ///
    /// Returns `Self::ProcessOutput` (`SingleOutput` or `MultiOutput`):
    /// - `Yield(out)` — produce one output value on one port
    /// - `YieldMulti(outs)` — produce multiple output values (`MultiOutput` only)
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
// 1:1 machines return `SingleOutput<O>`; fan-out machines return `MultiOutput<O>`.
// `SingleOutput` has no `YieldMulti` at the type level — this is the type-level
// basis of `FusedInline` safety: the compiler completely prevents fan-out
// machines from entering a fused pipeline, without any `unsafe`.
//
// `ProcessOutput<O>` is kept as the unified type, used by generic runtimes
// after conversion via `MachineOutput::into_process_output()`.

/// The output type of 1:1 machines.
///
/// Contains only `Yield`/`Idle`/`Done` — **no `YieldMulti` at the type level**.
/// This is the type-level basis of `FusedInline` safety: a machine returning
/// `SingleOutput` cannot construct `YieldMulti` at the type level, requiring
/// no `unsafe` promise.
#[derive(Debug, Clone, PartialEq)]
pub enum SingleOutput<O> {
    /// A single output value.
    Yield(O),
    /// No output; the machine waits or is idle.
    Idle,
    /// The machine has finished; it should transition to Stopping.
    Done,
}

/// The output type of 1:N (fan-out) machines.
///
/// Contains `YieldMulti`, used by fan-out machines such as Tee.
/// A machine returning this type **cannot** implement `FusedInline`
/// (rejected at compile time).
#[derive(Debug, Clone, PartialEq)]
pub enum MultiOutput<O> {
    /// A single output value (a fan-out machine may also produce just one).
    Yield(O),
    /// Multiple output values, one per port (fan-out).
    ///
    /// The runtime delivers each value in order. In-vector order is preserved
    /// for deterministic delivery.
    YieldMulti(Vec<O>),
    /// No output; the machine waits or is idle.
    Idle,
    /// The machine has finished; it should transition to Stopping.
    Done,
}

// ── Sealed trait for output types ────────────────────────────────────────────

mod output_private {
    pub trait SealedOutput {}
    impl<O> SealedOutput for super::SingleOutput<O> {}
    impl<O> SealedOutput for super::MultiOutput<O> {}
    impl<O> SealedOutput for super::TupleOutput<O> {}
}

/// The unified trait for machine output (sealed).
///
/// External code cannot introduce new output types — only `SingleOutput` or
/// `MultiOutput` may be used. 1:1 machines set
/// `type ProcessOutput = SingleOutput<Self::Output>`, and fan-out machines set
/// `type ProcessOutput = MultiOutput<Self::Output>`.
///
/// Generic runtimes convert to the unified `ProcessOutput<O>` via
/// `into_process_output()` for processing; FusedInline pipeline consumers
/// receive `SingleOutput` directly, without conversion.
pub trait MachineOutput<O>: output_private::SealedOutput {
    /// Convert to the unified `ProcessOutput<O>` (for generic runtimes).
    ///
    /// This is a variant-tag remap, optimized by LLVM into a noop.
    fn into_process_output(self) -> ProcessOutput<O>;

    /// Collect all outputs into a vector. `Idle`/`Done` produce an empty
    /// vector; `Done` marks termination via the second element.
    ///
    /// **Note**: this convenience method allocates a `Vec` each call — it is a
    /// **non-hot-path** API. Runtime adapters should directly match
    /// `ProcessOutput` (or `Self::ProcessOutput`) item by item and deliver
    /// each value, avoiding the Vec allocation tax on every message (E12 has
    /// verified that on the `YieldMulti` path the Vec is allocated only on
    /// fan-out; this method brings the same allocation to every `Yield`).
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

/// A fixed dual-port output type: each `process` produces exactly **two**
/// values, one on each port.
///
/// This is the output type of "multi-port 1:1:1" machines (e.g. a data port
/// plus an observe port, each producing one value). Unlike `MultiOutput`
/// (which contains `YieldMulti`, true 1:N fan-out), `TupleOutput` has **no
/// count uncertainty at the type level** — exactly two values, no data
/// fan-out, so it can safely enter a `FusedInline` fusion pipeline (the
/// fuser can process it completely without any runtime decision).
///
/// # Algebraic difference from `MultiOutput`
///
/// - `MultiOutput::YieldMulti(Vec<O>)`: output count is runtime-determined →
///   data fan-out → entering a fusion pipeline would lose data → rejected by
///   `FusedInline` (at compile time).
/// - `TupleOutput::Yield(O, O)`: output count is fixed at 2, each value goes
///   to the port declared by its variant → no data fan-out → can fuse safely.
#[derive(Debug, Clone, PartialEq)]
pub enum TupleOutput<O> {
    /// Exactly two output values (each going to its variant's port).
    Yield(O, O),
    /// No output; the machine waits or is idle.
    Idle,
    /// The machine has finished; it should transition to Stopping.
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

/// Unified output type (used internally by the runtime).
///
/// Machines no longer return this type directly — they return `SingleOutput`
/// or `MultiOutput`. The runtime converts via
/// `MachineOutput::into_process_output()`.
///
/// The `YieldMulti` variant is kept for unified match handling.
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessOutput<O> {
    /// A single output value.
    Yield(O),
    /// Multiple output values (fan-out).
    YieldMulti(Vec<O>),
    /// No output.
    Idle,
    /// The machine has finished.
    Done,
}

impl<O> ProcessOutput<O> {
    /// Collect all outputs into a vector.
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
/// `FusedInline` requires `Machine::ProcessOutput: FusedCompatible`, i.e. the
/// output type is `SingleOutput` (exactly one output) or `TupleOutput`
/// (exactly two outputs). What both share is that **the output count is fixed
/// at the type level and contains no `YieldMulti`** — the compiler completely
/// prevents fan-out (1:N) machines from implementing this trait, without
/// `unsafe`, no escape hatch, no runtime checks.
///
/// ```ignore
/// // Tee returns MultiOutput, so it cannot implement FusedInline:
/// impl FusedInline for Tee {}  // E0277: trait bound not satisfied
/// ```
///
/// # Contract
///
/// Implementing this trait is a promise:
/// 1. `process` returns `SingleOutput` or `TupleOutput` (type-guaranteed —
///    the output count is fixed and contains no `YieldMulti`)
/// 2. `process` is marked `#[inline]` or `#[inline(always)]`
///    (still a manual guarantee)
///
/// Item 1 is fully verified by the type system; item 2 is a documented
/// convention (`#[inline]` is a suggestion, not a guarantee — it cannot be
/// expressed in types, but cross-crate inlining usually happens under `-O`).
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
/// # Positioning: type-contract layer
///
/// This trait is the **type-contract layer** of axiom's 0-cost abstraction:
/// the compiler enforces that only fixed-output-count Machines enter the fused
/// pipeline path. Its concrete consumers live in the runtime static path
/// (`static_path::run_machine` / `Chain` / `Diamond`), which is monomorphized
/// per concrete `FusedInline` machine:
pub trait FusedInline: Machine
where
    Self::ProcessOutput: FusedCompatible,
{
}

/// Output types that can safely enter a `FusedInline` fusion pipeline (sealed).
///
/// Implemented only by `SingleOutput` (exactly one output) and `TupleOutput`
/// (exactly two outputs) — both have **no `YieldMulti` at the type level**, the
/// output count is fixed, and the fuser can process every output completely
/// without runtime decisions. `MultiOutput` (which contains `YieldMulti`, a
/// runtime output count) is excluded.
///
/// External code cannot bypass this — `SealedFused` is a private trait, with
/// no `unsafe`; the compiler handles it completely.
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
