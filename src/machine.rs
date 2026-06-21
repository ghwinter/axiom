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

use crate::port::{PortSchema, ConfigSchema, MachineContext};
use crate::portset::{HasPortInfo, PortSet};
use crate::resource::{MachinePhysicalSpec, ResourceClass};
use crate::entity::EntityRestoreError;

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
    fn init(ctx: &MachineContext) -> Result<Self::State, InitError>
    where
        Self: Sized;

    /// Process one unit of work.
    ///
    /// Returns:
    /// - `Yield(out)` — produce one output value on one port
    /// - `YieldMulti(outs)` — produce multiple output values (e.g. Tee fan-out)
    /// - `Idle` — no output this tick
    /// - `Done` — machine finished, transition to Stopping
    fn process(
        state: &mut Self::State,
        ctx: &MachineContext,
        input: Self::Input,
    ) -> ProcessOutput<Self::Output>;

    /// Clean up resources before destruction.
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

// ── Process output ────────────────────────────────────────────────────────────

/// The result of a single `process()` call.
///
/// A machine may produce zero, one, or multiple output values per input.
/// `YieldMulti` supports multi-port fan-out (e.g. `Tee` yields to both
/// `output_a` and `output_b` in a single step).
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessOutput<O> {
    /// Normal completion with a single output value on one port.
    Yield(O),
    /// Multiple output values, each on its own port (fan-out).
    ///
    /// The runtime delivers each value to its target port. Order within
    /// the vector is preserved for deterministic delivery.
    YieldMulti(Vec<O>),
    /// No output; the machine is waiting or idle.
    Idle,
    /// The machine has finished. Runner should transition to Stopping.
    Done,
}

impl<O> ProcessOutput<O> {
    /// Collect all yielded outputs into a vector. `Idle` and `Done` produce
    /// empty vectors; `Done` also signals termination via the second element.
    ///
    /// Returns `(outputs, is_done)`.
    pub fn into_outputs(self) -> (Vec<O>, bool) {
        match self {
            ProcessOutput::Yield(o) => (vec![o], false),
            ProcessOutput::YieldMulti(os) => (os, false),
            ProcessOutput::Idle => (vec![], false),
            ProcessOutput::Done => (vec![], true),
        }
    }
}

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
