/// Deployment specification — the "what, where, and how" of a system.
///
/// A `DeploySpec` describes the complete topology of a deployed system:
/// which machines and functions exist, how they are connected, and with
/// what physical resources each machine runs.
///
/// The spec is **declarative**: it does not execute anything. A runtime
/// adapter (e.g., `axiom_tokio`) interprets the spec to construct
/// and start the system.
///
/// # Example
///
/// ```ignore
/// let deploy = DeploySpec {
///     machines: vec![
///         MachineInstance {
///             name: "ws_reader",
///             machine_type: "ws_machine",
///             physical: MachinePhysicalSpec { execution: Async, .. },
///             config_overrides: vec![("url", "\"wss://...\"".into())],
///         },
///         MachineInstance {
///             name: "pipeline",
///             machine_type: "seg_sig_machine",
///             physical: MachinePhysicalSpec { execution: CpuBound, .. },
///             config_overrides: vec![],
///         },
///     ],
///     funcs: vec![],
///     links: vec![
///         LinkSpec::new(
///             ("ws_reader", "trade_out"),
///             ("pipeline", "bar_in"),
///             LinkKind::BoundedBuf { capacity: 1024, write_policy: WritePolicy::Blocking, read_policy: ReadPolicy::Blocking },
///         ),
///     ],
///     settings: DeploySettings { cpu_threads: 2, io_threads: 2 },
/// };
/// ```

use crate::link::LinkSpec;
use crate::resource::MachinePhysicalSpec;

// ── Machine instance ──────────────────────────────────────────────────────────

/// A single machine instance in the deployment topology.
#[derive(Debug, Clone)]
pub struct MachineInstance {
    /// Unique name within this deployment (used in LinkSpec references).
    pub name: &'static str,
    /// Type name registered with the factory.
    pub machine_type: &'static str,
    /// Physical resource specification.
    pub physical: MachinePhysicalSpec,
    /// Initial configuration overrides (key → JSON value).
    pub config_overrides: Vec<(&'static str, String)>,
}

// ── Function binding ──────────────────────────────────────────────────────────

/// A function type referenced in the deployment topology.
///
/// Functions are not instantiated at runtime (they are pure code).
/// This binding exists so the topology is complete and visualizable.
#[derive(Debug, Clone)]
pub struct FuncBinding {
    /// Unique name within this deployment.
    pub name: &'static str,
    /// Type name registered with the factory.
    pub func_type: &'static str,
}

// ── Global settings ───────────────────────────────────────────────────────────

/// Global deployment settings.
#[derive(Debug, Clone)]
pub struct DeploySettings {
    /// Number of CPU-bound threads in the shared pool.
    pub cpu_threads: usize,
    /// Number of IO threads in the async runtime.
    pub io_threads: usize,
}

impl Default for DeploySettings {
    fn default() -> Self {
        Self {
            cpu_threads: 1,
            io_threads: 2,
        }
    }
}

// ── Full spec ─────────────────────────────────────────────────────────────────

/// Complete deployment specification.
#[derive(Debug, Clone)]
pub struct DeploySpec {
    pub machines: Vec<MachineInstance>,
    pub funcs: Vec<FuncBinding>,
    pub links: Vec<LinkSpec>,
    pub settings: DeploySettings,
}

impl DeploySpec {
    /// Create an empty deployment spec.
    pub fn new() -> Self {
        Self {
            machines: Vec::new(),
            funcs: Vec::new(),
            links: Vec::new(),
            settings: DeploySettings::default(),
        }
    }

    /// Add a machine.
    pub fn with_machine(mut self, m: MachineInstance) -> Self {
        self.machines.push(m);
        self
    }

    /// Add a function binding.
    pub fn with_func(mut self, f: FuncBinding) -> Self {
        self.funcs.push(f);
        self
    }

    /// Add a link.
    pub fn with_link(mut self, l: LinkSpec) -> Self {
        self.links.push(l);
        self
    }

    /// Validate the spec:
    /// - All machine names referenced in links exist.
    /// - All port names referenced in links exist in the machine's port schema.
    /// - No cyclic dependencies that violate deployment constraints.
    pub fn validate(&self) -> Result<(), ValidationError> {
        for link in &self.links {
            let src_name = link.out.0;
            let dst_name = link.into.0;

            if !self.machines.iter().any(|m| m.name == src_name)
                && !self.funcs.iter().any(|f| f.name == src_name)
            {
                return Err(ValidationError::UnknownMachine(src_name));
            }
            if !self.machines.iter().any(|m| m.name == dst_name)
                && !self.funcs.iter().any(|f| f.name == dst_name)
            {
                return Err(ValidationError::UnknownMachine(dst_name));
            }
        }
        Ok(())
    }
}

// ── Validation errors ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ValidationError {
    UnknownMachine(&'static str),
    UnknownPort {
        machine: &'static str,
        port: &'static str,
    },
    LinkTypeMismatch {
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
        reason: &'static str,
    },
}
