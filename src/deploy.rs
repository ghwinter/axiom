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

    /// Validate the spec (工程修补 7.5.5):
    /// - All machine/func names referenced in links exist.
    /// - Machine/func names are unique within the deployment.
    /// - No self-loops (a machine linking to itself).
    /// - No cyclic dependencies (topological sort).
    ///
    /// **Note**: Port name existence and type compatibility require `PortSchema`,
    /// which is not available in the static `DeploySpec`. These checks are
    /// performed at runtime via `LinkCompat::check`.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // 1. 名称唯一性检查
        let mut seen_machines = std::collections::HashSet::new();
        for m in &self.machines {
            if !seen_machines.insert(m.name) {
                return Err(ValidationError::DuplicateName(m.name));
            }
        }
        let mut seen_funcs = std::collections::HashSet::new();
        for f in &self.funcs {
            if !seen_funcs.insert(f.name) {
                return Err(ValidationError::DuplicateName(f.name));
            }
        }

        // 2. 链接引用的机器/函数存在性 + 自环检查
        for link in &self.links {
            let src_name = link.out.0;
            let dst_name = link.into.0;

            // 自环检查
            if src_name == dst_name {
                return Err(ValidationError::SelfLoop(src_name));
            }

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

        // 3. 循环依赖检查（拓扑排序，Kahn 算法）
        // 构建邻接表：src → [dst, ...]
        let mut adj: std::collections::HashMap<&'static str, Vec<&'static str>> =
            std::collections::HashMap::new();
        let mut in_degree: std::collections::HashMap<&'static str, usize> =
            std::collections::HashMap::new();

        // 初始化所有节点
        for m in &self.machines {
            adj.entry(m.name).or_default();
            in_degree.entry(m.name).or_insert(0);
        }
        for f in &self.funcs {
            adj.entry(f.name).or_default();
            in_degree.entry(f.name).or_insert(0);
        }

        // 构建边
        for link in &self.links {
            adj.entry(link.out.0).or_default().push(link.into.0);
            *in_degree.entry(link.into.0).or_insert(0) += 1;
        }

        // Kahn 算法
        let mut queue: Vec<&'static str> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(&name, _)| name)
            .collect();
        let mut visited = 0usize;
        while let Some(node) = queue.pop() {
            visited += 1;
            if let Some(neighbors) = adj.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(&neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(neighbor);
                        }
                    }
                }
            }
        }

        if visited != in_degree.len() {
            // 存在环——找出环中的节点
            let cycle_node = in_degree
                .iter()
                .find(|&(_, &deg)| deg > 0)
                .map(|(&name, _)| name)
                .unwrap_or("unknown");
            return Err(ValidationError::CyclicDependency(cycle_node));
        }

        Ok(())
    }
}

// ── Validation errors ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ValidationError {
    UnknownMachine(&'static str),
    /// 机器名或函数名在部署中重复。
    DuplicateName(&'static str),
    /// 机器链接到自身。
    SelfLoop(&'static str),
    /// 存在循环依赖（拓扑排序未完成）。
    CyclicDependency(&'static str),
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
