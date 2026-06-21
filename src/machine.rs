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
/// - Typed input/output ports (in `port_schema()`, annotated with FlowKind)
/// - A computation function `process(state, input) -> output`
/// - Configurable parameters (via `config_schema()`)
///
/// There is no separate `Observe` type. Observation data is just `Output`
/// flowing through ports labelled with `FlowKind::Observe`. This keeps the
/// model at exactly (S, I, O, δ) — nothing more, nothing less.
///
/// # Sync design
/// All methods are synchronous. The runtime adapter is responsible for
/// wrapping them in async tasks or spawning dedicated threads.

use crate::port::{PortSchema, ConfigSchema, MachineContext};
use crate::resource::{MachinePhysicalSpec, ResourceClass};
use crate::entity::EntityRestoreError;

pub trait Machine: Send + Sync + 'static {
    /// Persistent state — heap-allocated, observable.
    type State: Send + 'static;

    /// Human-readable name.
    fn name() -> &'static str
    where
        Self: Sized;

    /// The type of data consumed from the primary input port.
    type Input: Send + 'static;

    /// The type of data produced on the primary output port.
    /// Observation data is just `Output` sent through FlowKind::Observe ports.
    type Output: Send + Sync + 'static;

    /// Declare the machine's port interface.
    fn port_schema() -> PortSchema
    where
        Self: Sized;

    /// Declare the machine's configuration parameters.
    fn config_schema() -> ConfigSchema
    where
        Self: Sized;

    /// Initialize: acquire resources, register ports and configs.
    fn init(ctx: &MachineContext) -> Result<Self::State, InitError>
    where
        Self: Sized;

    /// Process one unit of work.
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

#[derive(Debug)]
pub enum ProcessOutput<O> {
    /// Normal completion with an output value.
    Yield(O),
    /// No output; the machine is waiting or idle.
    Idle,
    /// The machine has finished. Runner should transition to Stopping.
    Done,
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
