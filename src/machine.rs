/// Stateful computation primitive.
///
/// # Physics
/// - **Memory**: heap. `State` is allocated once (`init`) and lives across
///   repeated `process` calls until `cleanup` drops it.
/// - **Lifetime**: observable. A snapshotter can read `PortRegistry` stats,
///   `ConfigCell` values, and `Observe`-port data at any point between calls.
/// - **Controllable**: via `ConfigCell<T>` entries registered during `init`.
/// - **Connection**: via typed ports declared in `port_schema()`.
///   The deployer connects these ports to other machines or functions.
///
/// # Sync design
/// All `Machine` methods are synchronous. This is intentional:
/// the core library makes zero assumptions about the runtime.
/// An async runtime adapter (e.g., `axiom_tokio`) wraps synchronous
/// `process()` calls in async tasks.
///
/// # Determinism
/// A `Machine` is nondeterministic by default. A `Machine` whose output
/// depends only on its input and internal state can override
/// `deterministic()` to return `true`, enabling replay guarantees.

use crate::port::{PortSchema, ConfigSchema, MachineContext};

pub trait Machine: Send + Sync + 'static {
    /// The internal state, allocated on the heap by `init` and passed to every
    /// subsequent `process` call.
    type State: Send + 'static;

    /// The type of data consumed from the primary input port.
    type Input: Send + 'static;

    /// The type of data produced on the primary output port.
    type Output: Send + Sync + 'static;

    /// The type of structured observation data pushed to the `observe_out` port.
    type Observe: Send + Sync + 'static;

    /// Human-readable name for diagnostics, topology displays, and factory registration.
    fn name() -> &'static str
    where
        Self: Sized;

    /// Declare the machine's port interface.
    ///
    /// The returned schema includes:
    /// - Primary input port (type = `Self::Input`, direction = `In`)
    /// - Primary output port (type = `Self::Output`, direction = `Out`)
    /// - Observation port (type = `Self::Observe`, direction = `Observe`)
    /// - Any additional named ports.
    fn port_schema() -> PortSchema
    where
        Self: Sized;

    /// Declare the machine's configuration parameters.
    fn config_schema() -> ConfigSchema
    where
        Self: Sized;

    /// Initialize the machine: acquire resources, register ports, register configs.
    ///
    /// Called once before any `process` call.
    fn init(ctx: &MachineContext) -> Result<Self::State, InitError>
    where
        Self: Sized;

    /// Process one unit of work.
    ///
    /// Called repeatedly by the runner. Returns `ProcessOutput` synchronously.
    fn process(
        state: &mut Self::State,
        ctx: &MachineContext,
        input: Self::Input,
    ) -> ProcessOutput<Self::Output>;

    /// Clean up resources before the machine is destroyed.
    ///
    /// Called once after the last `process` call. The `State` is consumed.
    fn cleanup(state: Self::State, ctx: &MachineContext) -> Result<(), CleanupError>;

    // ── Optional ──────────────────────────────────────────────────────────

    /// Whether this machine is deterministic (replay-safe).
    fn deterministic() -> bool
    where
        Self: Sized,
    {
        false
    }

    /// Serialize the current state into a byte vector for checkpoint/restore.
    fn checkpoint(_state: &Self::State) -> Option<Vec<u8>> {
        None
    }

    /// Restore the state from a previously saved checkpoint.
    fn restore(
        _state: &mut Self::State,
        _data: &[u8],
    ) -> Result<(), RestoreError> {
        Err(RestoreError::NotSupported)
    }
}

// ── Process output ────────────────────────────────────────────────────────────

/// The result of a single `process` call.
#[derive(Debug)]
pub enum ProcessOutput<O> {
    /// Normal completion with an output value.
    Yield(O),
    /// No output; the machine is waiting for more input or is idle.
    Idle,
    /// The machine has finished its work and should not be called again.
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
            Self::ResourceAcquisitionFailed(s) => write!(f, "resource acquisition failed: {}", s),
            Self::ConfigurationInvalid(s) => write!(f, "configuration invalid: {}", s),
            Self::PortRegistrationFailed(s) => write!(f, "port registration failed: {}", s),
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
            Self::ResourceReleaseFailed(s) => write!(f, "resource release failed: {}", s),
            Self::Timeout => write!(f, "cleanup timeout"),
            Self::Other(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug)]
pub enum RestoreError {
    NotSupported,
    ChecksumMismatch,
    VersionMismatch { expected: u32, actual: u32 },
    DeserializationFailed(String),
}

impl core::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotSupported => write!(f, "checkpoint/restore not supported"),
            Self::ChecksumMismatch => write!(f, "checkpoint checksum mismatch"),
            Self::VersionMismatch { expected, actual } => {
                write!(f, "version mismatch: expected {}, got {}", expected, actual)
            }
            Self::DeserializationFailed(s) => write!(f, "deserialization failed: {}", s),
        }
    }
}
