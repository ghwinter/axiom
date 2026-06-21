/// Entity — Layer 0: a persistent thing with state and physical requirements.
///
/// # What this is
///
/// An `Entity` is the simplest possible declaration of a persistent computation
/// unit. It says:
///
/// - "I have persistent state on the heap." (`type State`)
/// - "I have a name." (`fn name()`)
/// - "I have physical resource requirements." (`fn physical_spec()`)
/// - "My state can be checkpointed." (optional `fn checkpoint`, `fn restore`)
///
/// # What this is NOT
///
/// - No ports. An Entity has no declared communication boundaries.
/// - No process. An Entity has no computation behaviour.
///
/// # Physical mapping
///
/// | Abstract | Code | Memory | Observable |
/// |----------|------|--------|-----------|
/// | State `S` | `type State` | Heap | Yes |
/// | Name | `fn name()` | Code segment | Yes |
/// | Physical spec | `fn physical_spec()` | Code segment | Yes |
///
/// # When to implement
///
/// Implement `Entity` when you need a named, persistent state container.
/// If you also need ports, implement `Portalled`. If you also need computation,
/// implement `Machine`.

use crate::resource::{MachinePhysicalSpec, ResourceClass};

pub trait Entity: Send + Sync + 'static {
    /// Persistent state type — heap-allocated, observable.
    type State: Send + 'static;

    /// Human-readable name for diagnostics and topology.
    fn name() -> &'static str
    where
        Self: Sized;

    /// Physical resource declaration. The deployer uses this to allocate
    /// threads, budget memory, and schedule the entity.
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

    // ── Optional lifecycle extensions ─────────────────────

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

/// Errors that can occur when restoring an Entity from a checkpoint.
#[derive(Debug)]
pub enum EntityRestoreError {
    NotSupported,
    ChecksumMismatch,
    VersionMismatch { expected: u32, actual: u32 },
    DeserializationFailed(String),
}

impl core::fmt::Display for EntityRestoreError {
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
