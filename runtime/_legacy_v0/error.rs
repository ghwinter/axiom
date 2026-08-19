//! Error types for runtime materialization or driving.

/// An error occurring during runtime materialization or driving.
#[derive(Debug)]
pub enum RuntimeError {
    /// `MachineInstance::name` holds non-`'static` data (an owned String),
    /// while `MachineContext::new` requires `&'static str`.
    NonStaticName { instance: String },

    /// The given link kind is not supported by the current runtime implementation.
    UnsupportedLinkKind { kind: String, hint: String },

    /// The topology references a machine or port that does not exist.
    DanglingRef { machine: String, port: String },

    /// The topology cannot be materialized in Parallel mode (e.g. Source-like machines with no input ports).
    UnsupportedTopology { machine: String, reason: String },

    /// Machine init failed.
    InitFailed { machine: String, error: axiom::machine::InitError },

    /// The driver loop reached max_ticks without completing.
    TickLimitExceeded { ticks: u64 },

    /// cleanup failed.
    CleanupFailed { machine: String },

    /// An IO multiplexing operation failed (reactor register / poll error).
    IoFailed { error: crate::io::IoError },

    /// The composite machine nesting depth exceeded the limit (possibly an unbounded expansion caused by a composite self-reference).
    CompositeTooDeep { depth: usize, hint: String },

    /// The deployment declares Moore semantics (`MachineInstance::is_moore`) that
    /// do not match the machine type's actual implementation: `is_moore: true` is
    /// declared, but that `machine_type` was not registered via `register_moore`
    /// (a type-level `M: Moore` guarantee). A declaration inconsistent with the
    /// implementation would mislead `validate_deep`'s cycle-safety analysis
    /// (mistakenly assuming feedback cycles can be broken) — rejected at deploy
    /// time.
    MooreMismatch { machine: String, machine_type: String },

    /// The blueprint failed one of the deployment contracts — deep validation
    /// (`DynamicTopology::validate_deep_for`: port existence, type/flow
    /// compatibility, the FlowKind×carrier matrix, edge-degree constraints,
    /// Inline acyclicity, cycle safety) or the runtime capability audit
    /// (`RuntimeContract::check_spec`: link kinds, backpressure actions,
    /// execution modes, physical budget). `report` carries the structured
    /// violations (`rule_id`-tagged) for machine-readable feedback. Rejected
    /// before any physics is created.
    ContractViolation {
        /// The contract that failed: `"validate_deep"` or `"check_spec"`.
        contract: &'static str,
        report: axiom::deploy::ValidationReport,
    },
}

impl core::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NonStaticName { instance } => write!(
                f, "machine instance `{instance}` has non-'static name"
            ),
            Self::UnsupportedLinkKind { kind, hint } => write!(
                f, "link kind `{kind}` not supported: {hint}"
            ),
            Self::DanglingRef { machine, port } => write!(
                f, "topology references non-existent endpoint ({machine}, {port})"
            ),
            Self::UnsupportedTopology { machine, reason } => write!(
                f, "topology `{machine}` not supported in this mode: {reason}"
            ),
            Self::InitFailed { machine, error } => write!(
                f, "machine `{machine}` init failed: {error:?}"
            ),
            Self::TickLimitExceeded { ticks } => write!(
                f, "driver loop exceeded {ticks} ticks"
            ),
            Self::CleanupFailed { machine } => write!(
                f, "machine `{machine}` cleanup failed"
            ),
            Self::IoFailed { error } => write!(f, "io reactor error: {error}"),
            Self::CompositeTooDeep { depth, hint } => write!(
                f, "composite expansion exceeded depth {depth}: {hint}"
            ),
            Self::MooreMismatch { machine, machine_type } => write!(
                f,
                "machine `{machine}` declares Moore semantics but type `{machine_type}` is not registered as Moore \
                 (register the type with `register_moore` to make the declaration contract-valid)"
            ),
            Self::ContractViolation { contract, report } => {
                write!(f, "{contract} rejected the blueprint")?;
                for v in &report.violations {
                    write!(f, "\n  {v}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for RuntimeError {}
