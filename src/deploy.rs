/// Deployment specification — the "what, where, and how" of a system.
///
/// A `DeploySpec` describes the complete topology of a deployed system:
/// which machines and functions exist, how they are connected, and with
/// what physical resources each machine runs.
///
/// The spec is **declarative**: it does not execute anything. A runtime
/// adapter interprets the spec to construct and start the system.
///
/// # Example
///
/// A spec can be built in code (using `&'static str` literals via the
/// ergonomic constructors) or loaded from a declarative config file under the
/// `serialize` feature — both paths produce the same `DeploySpec`:
///
/// ```ignore
/// // Code-defined topology
/// let deploy = DeploySpec::new()
///     .with_machine(MachineInstance::new(
///         "ws_reader", "ws_machine", MachinePhysicalSpec::default(),
///     ))
///     .with_machine(MachineInstance::new(
///         "pipeline", "seg_sig_machine", MachinePhysicalSpec::default(),
///     ))
///     .with_link(LinkSpec::new(
///         ("ws_reader", "trade_out"),
///         ("pipeline", "bar_in"),
///         LinkKind::BoundedBuf {
///             capacity: 1024,
///             write_policy: WritePolicy::Blocking,
///             read_policy: ReadPolicy::Blocking,
///         },
///     ));
///
/// // Config-defined topology (requires the `serialize` feature):
/// // let json = std::fs::read_to_string("topology.json")?;
/// // let deploy: DeploySpec = serde_json::from_str(&json)?;
/// ```

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use crate::compat::HashMap;
#[cfg(not(feature = "std"))]
use alloc::format;
use crate::link::LinkSpec;
use crate::port::{PortSchema, PortDir, LinkCompat};
use crate::resource::{MachinePhysicalSpec, ExecutionHint};
use alloc::borrow::Cow;

// ── Machine instance ──────────────────────────────────────────────────────────

/// A single machine instance in the deployment topology.
///
/// Name and type fields use [`Cow<'static, str>`] so an instance can be built
/// from `&'static str` literals in code or from owned `String`s deserialized
/// out of a config file. Under the `serialize` feature the whole `DeploySpec`
/// (and therefore `MachineInstance`) round-trips through Serde.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct MachineInstance {
    /// Unique name within this deployment (used in LinkSpec references).
    pub name: Cow<'static, str>,
    /// Type name registered with the factory.
    pub machine_type: Cow<'static, str>,
    /// Physical resource specification.
    pub physical: MachinePhysicalSpec,
    /// Initial configuration overrides (key → JSON value).
    pub config_overrides: Vec<(Cow<'static, str>, String)>,
    /// Whether this machine implements **Moore semantics** (output depends
    /// only on pre-update state).
    ///
    /// Declared by the deployer. Used by [`DeploySpec::validate_deep`] for
    /// cycle-safety analysis: every cycle in the topology must pass through
    /// at least one Moore machine, otherwise an algebraic loop exists.
    ///
    /// A channel-based runtime introduces a one-tick delay on every link,
    /// which effectively makes every machine Moore from the receiver's
    /// perspective — so for pure channel runtimes this flag is informational.
    /// It becomes load-bearing for fused/inline runtimes where links have no
    /// delay.
    ///
    /// Defaults to `false`. Use [`.moore()`](Self::moore) to set `true`.
    #[cfg_attr(feature = "serialize", serde(default))]
    pub is_moore: bool,
}

impl MachineInstance {
    /// Construct a machine instance with no config overrides.
    ///
    /// `name` and `machine_type` accept `&'static str` or `String` via
    /// `Into<Cow<'static, str>>`. The instance defaults to non-Moore; use
    /// [`.moore()`](Self::moore) to declare Moore semantics.
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        machine_type: impl Into<Cow<'static, str>>,
        physical: MachinePhysicalSpec,
    ) -> Self {
        Self {
            name: name.into(),
            machine_type: machine_type.into(),
            physical,
            config_overrides: Vec::new(),
            is_moore: false,
        }
    }

    /// Declare that this machine implements Moore semantics.
    ///
    /// Builder-style: `MachineInstance::new(...).moore()`. Enables the
    /// deploy-time cycle-safety check in [`DeploySpec::validate_deep`].
    pub fn moore(mut self) -> Self {
        self.is_moore = true;
        self
    }
}

// ── Function binding ──────────────────────────────────────────────────────────

/// A function type referenced in the deployment topology.
///
/// Functions are not instantiated at runtime (they are pure code).
/// This binding exists so the topology is complete and visualizable.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct FuncBinding {
    /// Unique name within this deployment.
    pub name: Cow<'static, str>,
    /// Type name registered with the factory.
    pub func_type: Cow<'static, str>,
}

impl FuncBinding {
    /// Construct a function binding.
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        func_type: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            name: name.into(),
            func_type: func_type.into(),
        }
    }
}

// ── Global settings ───────────────────────────────────────────────────────────

/// Global deployment settings.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
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

// ── Incremental patch ─────────────────────────────────────────────────────────

/// 蓝图的增量补丁——声明式意图（幂等）。
///
/// 用于蓝图的**增量演进**：消费者（如 AI 蓝图接口）读取当前蓝图后，生成
/// 一个 `Patch` 序列而非重写全图。所有操作**幂等**——`remove` 一个不存在
/// 的机器/链接是 no-op，`upsert` 按标识（机器名 / 链接端点）匹配，已存在
/// 则替换、否则添加。
///
/// 应用后配合 [`DeploySpec::validate`] / [`DeploySpec::validate_deep`] 验证
/// 结果——补丁本身不报错（幂等），正确性由验证器把关。这与蓝图的可
/// serialize 特性契合：AI 写 JSON 补丁，得到结构化验证错误，迭代。
///
/// # 标识约定
///
/// - 机器实例以 `name` 为标识；
/// - 链接以 `(out, into)` 端点对为标识（`(machine, port)` 各一）。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum Patch {
    /// Upsert 机器实例：按 `name` 匹配，已存在则整体替换，否则添加。
    UpsertMachine(MachineInstance),
    /// 移除机器（按 `name`）及其所有相关链接；不存在则 no-op。
    RemoveMachine(Cow<'static, str>),
    /// Upsert 链接：按 `(out, into)` 端点匹配，已存在则替换 `kind`，否则添加。
    UpsertLink(LinkSpec),
    /// 移除链接（按 `(out, into)` 端点匹配）；不存在则 no-op。
    RemoveLink {
        out: (Cow<'static, str>, Cow<'static, str>),
        into: (Cow<'static, str>, Cow<'static, str>),
    },
}

// ── Full spec ─────────────────────────────────────────────────────────────────

/// Complete deployment specification.
///
/// Under the `serialize` feature, `DeploySpec` is fully `Serialize`/`Deserialize`
/// — every field (including `MachineInstance::name`, `LinkSpec` endpoints) uses
/// `Cow<'static, str>`, so a topology declared in TOML/JSON can be loaded
/// directly into a `DeploySpec` and handed to a runtime adapter.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
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

    /// Apply an incremental patch (idempotent).
    ///
    /// See [`Patch`] for the semantics of each variant. Applying a patch never
    /// fails — `remove` of an absent machine/link is a no-op, `upsert` matches
    /// by identity. Verify the result with [`validate`](Self::validate) or
    /// [`validate_deep`](Self::validate_deep).
    pub fn apply_patch(&mut self, patch: &Patch) {
        match patch {
            Patch::UpsertMachine(m) => {
                if let Some(existing) = self.machines.iter_mut().find(|x| x.name == m.name) {
                    *existing = m.clone();
                } else {
                    self.machines.push(m.clone());
                }
            }
            Patch::RemoveMachine(name) => {
                self.machines
                    .retain(|m| m.name.as_ref() != name.as_ref());
                self.links.retain(|l| {
                    l.out.0.as_ref() != name.as_ref() && l.into.0.as_ref() != name.as_ref()
                });
            }
            Patch::UpsertLink(l) => {
                if let Some(existing) = self
                    .links
                    .iter_mut()
                    .find(|x| x.out == l.out && x.into == l.into)
                {
                    existing.kind = l.kind.clone();
                } else {
                    self.links.push(l.clone());
                }
            }
            Patch::RemoveLink { out, into } => {
                self.links
                    .retain(|l| !(l.out == *out && l.into == *into));
            }
        }
    }

    /// Apply a sequence of patches in order.
    pub fn apply_patches(&mut self, patches: &[Patch]) {
        for p in patches {
            self.apply_patch(p);
        }
    }

    /// Validate the spec (structural):
    /// - All machine/func names referenced in links exist.
    /// - Machine/func names are unique within the deployment.
    /// - No self-loops (a machine linking to itself).
    /// - Cycles between different machines are structurally ALLOWED.
    ///
    /// **Cycle safety** (every cycle must pass through ≥1 Moore machine) is
    /// NOT checked here — it requires the `is_moore` declaration on
    /// [`MachineInstance`], and is performed by [`validate_deep`](Self::validate_deep).
    ///
    /// **Note**: Port name existence and type compatibility require `PortSchema`,
    /// which is not available in the static `DeploySpec`. These checks are
    /// performed at runtime via `LinkCompat::check` or in `validate_deep`.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // 1. 名称唯一性检查
        let mut seen_machines: crate::compat::HashSet<&str> = crate::compat::HashSet::new();
        for m in &self.machines {
            if !seen_machines.insert(m.name.as_ref()) {
                return Err(ValidationError::DuplicateName(m.name.to_string()));
            }
        }
        let mut seen_funcs: crate::compat::HashSet<&str> = crate::compat::HashSet::new();
        for f in &self.funcs {
            if !seen_funcs.insert(f.name.as_ref()) {
                return Err(ValidationError::DuplicateName(f.name.to_string()));
            }
        }

        // 2. 链接引用的机器/函数存在性 + 自环检查
        for link in &self.links {
            let src_name: &str = link.out.0.as_ref();
            let dst_name: &str = link.into.0.as_ref();

            // 自环检查
            if src_name == dst_name {
                return Err(ValidationError::SelfLoop(src_name.to_string()));
            }

            if !self.machines.iter().any(|m| m.name.as_ref() == src_name)
                && !self.funcs.iter().any(|f| f.name.as_ref() == src_name)
            {
                return Err(ValidationError::UnknownMachine(src_name.to_string()));
            }
            if !self.machines.iter().any(|m| m.name.as_ref() == dst_name)
                && !self.funcs.iter().any(|f| f.name.as_ref() == dst_name)
            {
                return Err(ValidationError::UnknownMachine(dst_name.to_string()));
            }
        }

        // 2.5 隐式 fan-out 拒绝：一个输出端口只能链接一个目标。
        // 动态路径的路由是 1 对 1（route_target 取第一个）；fan-out 是
        // **机器的职责**（Split / CloneSplit / Tee 契约）——蓝图必须用
        // 显式 Tee 机器表达广播。此检查把"路由静默丢弃第二个目标"的
        // 未定义行为提前到验证期拒绝。
        let mut fanout_seen: crate::compat::HashSet<(&str, &str)> =
            crate::compat::HashSet::new();
        for link in &self.links {
            let src_name: &str = link.out.0.as_ref();
            let src_port: &str = link.out.1.as_ref();
            if !fanout_seen.insert((src_name, src_port)) {
                return Err(ValidationError::FanOutViaTee {
                    src_machine: src_name.to_string(),
                    src_port: src_port.to_string(),
                });
            }
        }

        // 3. 循环依赖: cycles are structurally ALLOWED here.
        //
        // foundations.md §1.2a defines Moore delay to break algebraic cycles.
        // In a channel-based runtime, every link introduces a one-tick delay
        // (the channel IS the delay element), so cycles are safe regardless
        // of Moore/Mealy. For fused/inline runtimes (no link delay), a cycle
        // is only safe if at least one machine on it is Moore.
        //
        // That semantic check belongs in `validate_deep`, which has access to
        // `MachineInstance::is_moore`. Here we only assert structural validity.

        Ok(())
    }

    /// Deep validation: structural checks + port/type/flow compatibility + resource budget.
    ///
    /// This extends [`validate()`](Self::validate) with checks that require
    /// runtime type information (`PortSchema`). The caller provides a map of
    /// machine_name → PortSchema, typically obtained from `M::port_schema()`.
    ///
    /// # What it checks
    ///
    /// 1. All checks from [`validate()`](Self::validate) (structural integrity).
    /// 2. For each link: source port exists and is an output; target port
    ///    exists and is an input.
    /// 3. Port type compatibility via `LinkCompat::can_link_to()`
    ///    (type match, flow kind match, schema version compatibility).
    /// 4. Resource budget: total CPU-bound threads and thread pools do not
    ///    exceed `DeploySettings::cpu_threads`.
    /// 5. **Cycle safety**: every cycle in the topology must pass through at
    ///    least one Moore machine (`MachineInstance::is_moore == true`).
    ///    A cycle with no Moore machine is an algebraic loop — rejected as
    ///    [`ValidationError::UnsafeCycle`]. (Channel-based runtimes where every
    ///    link has delay can mark all machines Moore, or ignore this check.)
    /// 6. **Edge-degree constraints**: per-port limits for constrained
    ///    `LinkKind` variants (Inline outdeg ≤ 1, Channel indeg ≤ 1,
    ///    CasFreeRing SPSC). Rejected as [`ValidationError::DegreeConstraintViolated`].
    /// 7. **Inline acyclicity**: the Inline-edge subgraph must be a DAG — an
    ///    Inline cycle is a synchronous-call deadlock. Rejected as
    ///    [`ValidationError::InlineCycle`].
    ///
    /// # What it does NOT check
    ///
    /// - Whether a migrator exists for `LinkCompat::Migrate` (that requires
    ///   a `MigrateRegistry`, checked at runtime).
    /// - Actual memory consumption (only declared estimates are summed).
    pub fn validate_deep(&self, schemas: &HashMap<&str, PortSchema>) -> Result<(), ValidationError> {
        // 1. Run structural validation first.
        self.validate()?;

        // 2. Per-link port/type/flow compatibility.
        for link in &self.links {
            let src_machine: &str = link.out.0.as_ref();
            let dst_machine: &str = link.into.0.as_ref();
            let src_port_name: &str = link.out.1.as_ref();
            let dst_port_name: &str = link.into.1.as_ref();

            let src_schema = schemas.get(src_machine).ok_or_else(|| {
                ValidationError::UnknownMachine(src_machine.to_string())
            })?;
            let dst_schema = schemas.get(dst_machine).ok_or_else(|| {
                ValidationError::UnknownMachine(dst_machine.to_string())
            })?;

            let src_port = src_schema.find(src_port_name).ok_or_else(|| {
                ValidationError::UnknownPort {
                    machine: src_machine.to_string(),
                    port: src_port_name.to_string(),
                }
            })?;
            let dst_port = dst_schema.find(dst_port_name).ok_or_else(|| {
                ValidationError::UnknownPort {
                    machine: dst_machine.to_string(),
                    port: dst_port_name.to_string(),
                }
            })?;

            // Direction check: source must be Out, target must be In.
            if src_port.dir != PortDir::Out {
                return Err(ValidationError::LinkTypeMismatch {
                    out: (src_machine.to_string(), src_port_name.to_string()),
                    into: (dst_machine.to_string(), dst_port_name.to_string()),
                    reason: "source port is not an output".to_string(),
                });
            }
            if dst_port.dir != PortDir::In {
                return Err(ValidationError::LinkTypeMismatch {
                    out: (src_machine.to_string(), src_port_name.to_string()),
                    into: (dst_machine.to_string(), dst_port_name.to_string()),
                    reason: "target port is not an input".to_string(),
                });
            }

            // Type/flow/version compatibility.
            let compat = src_port.can_link_to(dst_port);
            match compat {
                LinkCompat::Compatible => {}
                LinkCompat::Migrate { from_ver, to_ver } => {
                    // Migration is allowed but requires a runtime migrator.
                    // Report as a warning via DeepValidationWarning if needed;
                    // here we accept it (runtime will fail if no migrator).
                    let _ = (from_ver, to_ver);
                }
                LinkCompat::Incompatible { reason } => {
                    return Err(ValidationError::LinkTypeMismatch {
                        out: (src_machine.to_string(), src_port_name.to_string()),
                        into: (dst_machine.to_string(), dst_port_name.to_string()),
                        reason: reason.to_string(),
                    });
                }
            }
        }

        // 3. Resource budget check: total dedicated threads vs. cpu_threads.
        let mut total_dedicated_threads: usize = 0;
        for m in &self.machines {
            match &m.physical.execution {
                ExecutionHint::CpuBound => {
                    total_dedicated_threads += 1;
                }
                ExecutionHint::CpuBoundN(n) => {
                    total_dedicated_threads += n;
                }
                ExecutionHint::ThreadPool(spec) => {
                    total_dedicated_threads += spec.max_threads;
                }
                ExecutionHint::Async | ExecutionHint::Subprocess(_) => {}
            }
        }
        if total_dedicated_threads > self.settings.cpu_threads {
            return Err(ValidationError::ResourceBudgetExceeded {
                requested_threads: total_dedicated_threads,
                available_threads: self.settings.cpu_threads,
            });
        }

        // 4. Cycle safety: every cycle must pass through ≥1 Moore machine.
        //
        // A cycle with no Moore machine is an algebraic loop (each machine's
        // output depends on its current input, which depends on another's
        // current output). Moore machines break the loop because their output
        // depends only on pre-update state.
        //
        // Algorithm: remove all Moore machines from the graph, then check if
        // the remaining (non-Moore) subgraph has a cycle. If it does, that
        // cycle has no Moore machine → unsafe.
        //
        // Self-loops are already rejected by `validate()`. Func bindings are
        // not graph nodes (they have no output edges).
        if let Some(cycle) = self.find_non_moore_cycle() {
            return Err(ValidationError::UnsafeCycle { cycle });
        }

        // 5. Edge-degree constraints (per-port limits for Inline/Channel/CasFreeRing).
        //
        // These are physical invariants of the link kinds: Inline is a single
        // function call (one callee), Channel is MPSC (one consumer), CasFreeRing
        // is SPSC (one producer, one consumer). See docs/architecture.md §2.
        if let Some(v) = crate::analysis::degree_violations(self).into_iter().next() {
            return Err(ValidationError::DegreeConstraintViolated {
                machine: v.machine,
                port: v.port,
                link_kind: v.link_kind,
                direction: v.direction,
                limit: v.limit,
                actual: v.actual,
            });
        }

        // 6. Inline acyclicity: the Inline-edge subgraph must be a DAG.
        //
        // Inline links are synchronous function calls with no delay. A cycle of
        // Inline edges is a recursive call that never returns — a deadlock.
        // Unlike the non-Moore cycle check (which Moore delay can break), an
        // Inline cycle is unsafe regardless of Moore semantics, because Inline
        // has no buffering.
        if let Some(cycle) = crate::analysis::inline_cycle(self) {
            return Err(ValidationError::InlineCycle { cycle });
        }

        Ok(())
    }

    /// Advisory topology analysis: feedback loops, SPOF, orphans, observability.
    ///
    /// Returns a [`TopologyReport`](crate::analysis::TopologyReport) with
    /// advisory warnings. This does **not** validate — call `validate_deep`
    /// first for correctness checks.
    ///
    /// `schemas` is needed for observability completeness (Theorem 7.2).
    /// Pass `None` to skip that check.
    pub fn analyze(
        &self,
        schemas: Option<&HashMap<&str, PortSchema>>,
    ) -> crate::analysis::TopologyReport {
        crate::analysis::analyze(self, schemas)
    }

    /// Collect **all** structural and deep violations (not fail-fast).
    ///
    /// Each violation is a structured [`RuleViolation`] with a stable
    /// `rule_id`, a JSON-path-like `path`, and `expected`/`actual` —
    /// machine-readable feedback for AI-driven blueprint iteration. The checks
    /// mirror [`validate_deep`](Self::validate_deep) but **append** instead of
    /// returning on the first error.
    ///
    /// `Migrate` compatibility is reported as a **warning** (a runtime
    /// migrator is still required), matching the advisory note in
    /// `validate_deep`.
    pub fn validate_report(
        &self,
        schemas: &HashMap<&str, PortSchema>,
    ) -> ValidationReport {
        let mut report = ValidationReport::default();

        // 1. Name uniqueness.
        let mut seen_machines: crate::compat::HashSet<&str> = crate::compat::HashSet::new();
        for (i, m) in self.machines.iter().enumerate() {
            if !seen_machines.insert(m.name.as_ref()) {
                report.push(RuleViolation::new(
                    "name-unique",
                    format!("machines[{i}].name"),
                    "machine names unique within deployment",
                    format!("duplicate '{}'", m.name),
                ));
            }
        }
        let mut seen_funcs: crate::compat::HashSet<&str> = crate::compat::HashSet::new();
        for (i, f) in self.funcs.iter().enumerate() {
            if !seen_funcs.insert(f.name.as_ref()) {
                report.push(RuleViolation::new(
                    "name-unique",
                    format!("funcs[{i}].name"),
                    "func names unique within deployment",
                    format!("duplicate '{}'", f.name),
                ));
            }
        }

        // 2. Link endpoint resolution + self-loop (every link, not fail-fast).
        let known = |name: &str| {
            self.machines.iter().any(|m| m.name.as_ref() == name)
                || self.funcs.iter().any(|f| f.name.as_ref() == name)
        };
        for (i, link) in self.links.iter().enumerate() {
            let src: &str = link.out.0.as_ref();
            let dst: &str = link.into.0.as_ref();
            if src == dst {
                report.push(RuleViolation::new(
                    "link-self-loop",
                    format!("links[{i}]"),
                    "source and target machines distinct",
                    format!("'{}' links to itself", src),
                ));
            }
            if !known(src) {
                report.push(RuleViolation::new(
                    "link-resolve-machine",
                    format!("links[{i}].out[0]"),
                    "source machine declared in machines or funcs",
                    format!("'{}' not found", src),
                ));
            }
            if !known(dst) {
                report.push(RuleViolation::new(
                    "link-resolve-machine",
                    format!("links[{i}].into[0]"),
                    "target machine declared in machines or funcs",
                    format!("'{}' not found", dst),
                ));
            }
        }

        // 3. Per-link port existence / direction / type compatibility.
        for (i, link) in self.links.iter().enumerate() {
            let src_machine: &str = link.out.0.as_ref();
            let dst_machine: &str = link.into.0.as_ref();
            let src_port_name: &str = link.out.1.as_ref();
            let dst_port_name: &str = link.into.1.as_ref();

            let src_schema = match schemas.get(src_machine) {
                Some(s) => s,
                None => continue, // already reported by link-resolve-machine
            };
            let dst_schema = match schemas.get(dst_machine) {
                Some(s) => s,
                None => continue,
            };

            let src_port = match src_schema.find(src_port_name) {
                Some(p) => p,
                None => {
                    report.push(RuleViolation::new(
                        "link-resolve-port",
                        format!("links[{i}].out[1]"),
                        format!("'{src_machine}' declares port '{src_port_name}'"),
                        "port not found",
                    ));
                    continue;
                }
            };
            let dst_port = match dst_schema.find(dst_port_name) {
                Some(p) => p,
                None => {
                    report.push(RuleViolation::new(
                        "link-resolve-port",
                        format!("links[{i}].into[1]"),
                        format!("'{dst_machine}' declares port '{dst_port_name}'"),
                        "port not found",
                    ));
                    continue;
                }
            };

            if src_port.dir != PortDir::Out {
                report.push(RuleViolation::new(
                    "link-direction",
                    format!("links[{i}].out[1]"),
                    "source port is an output",
                    format!("'{src_port_name}' is {:?}", src_port.dir),
                ));
            }
            if dst_port.dir != PortDir::In {
                report.push(RuleViolation::new(
                    "link-direction",
                    format!("links[{i}].into[1]"),
                    "target port is an input",
                    format!("'{dst_port_name}' is {:?}", dst_port.dir),
                ));
            }

            match src_port.can_link_to(dst_port) {
                LinkCompat::Compatible => {}
                LinkCompat::Migrate { from_ver, to_ver } => {
                    report.warn(RuleViolation::new(
                        "link-migrate",
                        format!("links[{i}]"),
                        "compatible via schema migration",
                        format!("version {from_ver} → {to_ver} (runtime migrator required)"),
                    ));
                }
                LinkCompat::Incompatible { reason } => {
                    report.push(RuleViolation::new(
                        "link-type",
                        format!("links[{i}]"),
                        "source port can link to target port",
                        reason.to_string(),
                    ));
                }
            }
        }

        // 4. Resource budget.
        let mut total: usize = 0;
        for m in &self.machines {
            match &m.physical.execution {
                ExecutionHint::CpuBound => total += 1,
                ExecutionHint::CpuBoundN(n) => total += n,
                ExecutionHint::ThreadPool(spec) => total += spec.max_threads,
                ExecutionHint::Async | ExecutionHint::Subprocess(_) => {}
            }
        }
        if total > self.settings.cpu_threads {
            report.push(RuleViolation::new(
                "resource-budget",
                "settings.cpu_threads",
                format!("at least {total} threads available"),
                format!("cpu_threads = {}", self.settings.cpu_threads),
            ));
        }

        // 5. Non-Moore cycle safety (first unsafe cycle).
        if let Some(cycle) = self.find_non_moore_cycle() {
            report.push(RuleViolation::new(
                "cycle-no-moore",
                "links",
                "every cycle passes through ≥1 Moore machine",
                format!("cycle without Moore machine: {}", cycle.join(" → ")),
            ));
        }

        // 6. Degree constraints (all violations).
        for v in crate::analysis::degree_violations(self) {
            report.push(RuleViolation::new(
                "degree-limit",
                format!("{}.{}", v.machine, v.port),
                format!("{} degree ≤ {}", v.direction, v.limit),
                format!("actual {}", v.actual),
            ));
        }

        // 7. Inline acyclicity (first Inline cycle).
        if let Some(cycle) = crate::analysis::inline_cycle(self) {
            report.push(RuleViolation::new(
                "inline-cycle",
                "links",
                "Inline-edge subgraph is a DAG (no synchronous-call deadlock)",
                format!("Inline cycle: {}", cycle.join(" → ")),
            ));
        }

        report
    }

    /// Find a cycle in the subgraph induced by non-Moore machines.
    ///
    /// Returns the machine names along the cycle (in traversal order) if one
    /// exists, or `None` if the non-Moore subgraph is acyclic.
    ///
    // Implementation: iterative DFS with color marking. White = unvisited,
    // Gray = on current stack, Black = finished. A Gray→Gray edge is a back
    // edge (cycle). We reconstruct the cycle from the stack.
    fn find_non_moore_cycle(&self) -> Option<Vec<String>> {
        use crate::compat::HashMap as Map;
        use crate::compat::HashSet as Set;

        // Index machines by name; record which are Moore.
        let mut moore_names: Set<&str> = Set::new();
        for m in &self.machines {
            if m.is_moore {
                moore_names.insert(m.name.as_ref());
            }
        }

        // Build adjacency list over non-Moore machines only.
        // Edges where either endpoint is Moore are dropped (Moore breaks cycles).
        let mut adj: Map<&str, Vec<&str>> = Map::new();
        for link in &self.links {
            let src: &str = link.out.0.as_ref();
            let dst: &str = link.into.0.as_ref();
            if moore_names.contains(src) || moore_names.contains(dst) {
                continue;
            }
            // Only machine→machine edges (skip func endpoints — they have no
            // output edges and cannot participate in a cycle).
            let src_is_machine = self.machines.iter().any(|m| m.name.as_ref() == src);
            let dst_is_machine = self.machines.iter().any(|m| m.name.as_ref() == dst);
            if !src_is_machine || !dst_is_machine {
                continue;
            }
            adj.entry(src).or_default().push(dst);
            adj.entry(dst).or_default(); // ensure dst is a known node
        }

        // Iterative DFS with a (node, child-iterator-state) stack.
        // Sort adjacency lists for deterministic cycle reporting.
        for neighbors in adj.values_mut() {
            neighbors.sort();
        }

        let mut color: Map<&str, u8> = Map::new(); // 0=white, 1=gray, 2=black
        let mut nodes: Vec<&str> = adj.keys().copied().collect();
        nodes.sort(); // deterministic DFS start order

        for &start in &nodes {
            if color.get(start).copied().unwrap_or(0) != 0 {
                continue;
            }
            // Stack entries: (node, index-into-its-adjacency)
            let mut stack: Vec<(&str, usize)> = Vec::new();
            color.insert(start, 1);
            stack.push((start, 0));

            while let Some(&(node, idx)) = stack.last() {
                let neighbors = adj.get(node).map(|v| v.as_slice()).unwrap_or(&[]);
                if idx < neighbors.len() {
                    // Advance the child pointer for this node.
                    stack.last_mut().unwrap().1 = idx + 1;
                    let next = neighbors[idx];
                    match color.get(next).copied().unwrap_or(0) {
                        0 => {
                            color.insert(next, 1);
                            stack.push((next, 0));
                        }
                        1 => {
                            // Back edge: cycle found. Reconstruct from stack.
                            let cycle_start = stack.iter().position(|&(n, _)| n == next);
                            if let Some(pos) = cycle_start {
                                let cycle: Vec<String> = stack[pos..]
                                    .iter()
                                    .map(|&(n, _)| n.to_string())
                                    .chain(core::iter::once(next.to_string()))
                                    .collect();
                                return Some(cycle);
                            }
                            // If `next` isn't on the stack, it's a cross/forward
                            // edge to a finished-but-recolor'd node — not a cycle.
                        }
                        _ => {} // black: already finished, skip
                    }
                } else {
                    // Done with this node's children.
                    color.insert(node, 2);
                    stack.pop();
                }
            }
        }

        None
    }
}

// ── Validation errors ─────────────────────────────────────────────────────────

/// Errors raised by [`DeploySpec::validate`] / [`DeploySpec::validate_deep`].
///
/// Variants own their diagnostic strings (`String`, not `&'static str`) so they
/// can describe names that originate from a deserialized config file as well as
/// from compile-time literals. This also makes `ValidationError` self-contained
/// and freely storable across `Result` boundaries.
#[derive(Debug)]
pub enum ValidationError {
    UnknownMachine(String),
    /// 机器名或函数名在部署中重复。
    DuplicateName(String),
    /// 机器链接到自身。
    SelfLoop(String),
    /// 隐式 fan-out：一个输出端口链接了多个目标。
    ///
    /// 动态路径路由是 1 对 1；fan-out 必须是机器的职责（`Split`/
    /// `CloneSplit`/Tee）。修复：插入显式 Tee 机器，每端口一条链接。
    FanOutViaTee { src_machine: String, src_port: String },
    UnknownPort {
        machine: String,
        port: String,
    },
    LinkTypeMismatch {
        out: (String, String),
        into: (String, String),
        reason: String,
    },
    /// 资源预算超出限制（validate_deep 专用）。
    ResourceBudgetExceeded {
        requested_threads: usize,
        available_threads: usize,
    },
    /// 拓扑中存在一个环，且环上没有任何 Moore 机器——构成代数环路。
    ///
    /// `cycle` 列出环上的机器名（顺序为环的遍历顺序）。
    /// 修复方式：将环上至少一台机器标记为 Moore（[`.moore()`](crate::deploy::MachineInstance::moore)），
    /// 或确保运行时为每条链路提供单拍延迟（channel-based runtime）。
    UnsafeCycle { cycle: Vec<String> },
    /// 端口边度约束被违反。
    ///
    /// 每种 `LinkKind` 对每端口连接数有上限（见 `docs/architecture.md` §2）：
    /// - `Inline`：每个输出端口出度 ≤ 1
    /// - `Channel`：每个输入端口入度 ≤ 1（单消费者）
    /// - `CasFreeRing`：出度 ≤ 1、入度 ≤ 1（SPSC）
    DegreeConstraintViolated {
        machine: String,
        port: String,
        link_kind: String,
        direction: String,
        limit: usize,
        actual: usize,
    },
    /// Inline 边构成环——同步调用死锁。
    ///
    /// `Inline` 连接是直接函数调用，无延迟。Inline 边的环是递归调用
    /// 永不返回的死锁。即使环上有 Moore 机器也不安全（Moore 延迟只在
    /// 有缓冲/通道的链路中有效）。
    InlineCycle { cycle: Vec<String> },
}

impl core::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownMachine(n) => write!(f, "unknown machine: {}", n),
            Self::DuplicateName(n) => write!(f, "duplicate name: {}", n),
            Self::SelfLoop(n) => write!(f, "machine '{}' links to itself", n),
            Self::FanOutViaTee { src_machine, src_port } => write!(
                f,
                "output port '{src_port}' on machine '{src_machine}' links to multiple targets — fan-out must be explicit (Split/CloneSplit/Tee machine)"
            ),
            Self::UnknownPort { machine, port } => {
                write!(f, "unknown port '{}' on machine '{}'", port, machine)
            }
            Self::LinkTypeMismatch { out, into, reason } => {
                write!(f, "link type mismatch {:?} → {:?}: {}", out, into, reason)
            }
            Self::ResourceBudgetExceeded { requested_threads, available_threads } => {
                write!(
                    f,
                    "resource budget exceeded: requested {} threads, {} available",
                    requested_threads, available_threads
                )
            }
            Self::UnsafeCycle { cycle } => {
                write!(
                    f,
                    "unsafe cycle with no Moore machine: {}",
                    cycle.join(" → ")
                )
            }
            Self::DegreeConstraintViolated {
                machine, port, link_kind, direction, limit, actual,
            } => {
                write!(
                    f,
                    "degree constraint violated: {}::{} {} {} links (limit {}, actual {})",
                    machine, port, link_kind, direction, limit, actual,
                )
            }
            Self::InlineCycle { cycle } => {
                write!(
                    f,
                    "Inline edge cycle (deadlock): {}",
                    cycle.join(" → ")
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ValidationError {}

// ════════════════════════════════════════════════════════════════════════════
// Section: Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
    use crate::port::{PortDecl, PortSchema};
    use crate::resource::{ExecutionHint, MachinePhysicalSpec};

    // ── Helpers ───────────────────────────────────────────────────────

    /// Default-physical machine (Async execution — does not count toward the
    /// CPU thread budget).
    fn machine(name: &'static str) -> MachineInstance {
        MachineInstance::new(name, "test", MachinePhysicalSpec::default())
    }

    /// Default-physical machine declared Moore.
    fn machine_moore(name: &'static str) -> MachineInstance {
        machine(name).moore()
    }

    /// Machine that occupies 1 dedicated CPU thread (`CpuBound`).
    fn machine_cpu(name: &'static str) -> MachineInstance {
        let mut physical = MachinePhysicalSpec::default();
        physical.execution = ExecutionHint::CpuBound;
        MachineInstance::new(name, "test", physical)
    }

    /// BoundedBuf link between two `(machine, port)` endpoints.
    fn bounded(
        a: &'static str, pa: &'static str,
        b: &'static str, pb: &'static str,
    ) -> LinkSpec {
        LinkSpec::new(
            (a, pa),
            (b, pb),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        )
    }

    /// Inline link between two `(machine, port)` endpoints.
    fn inline(
        a: &'static str, pa: &'static str,
        b: &'static str, pb: &'static str,
    ) -> LinkSpec {
        LinkSpec::new((a, pa), (b, pb), LinkKind::Inline)
    }

    /// Schema with an i32 Data output `"out"` and an i32 Data input `"in"`
    /// (both `schema_ver = 0`).
    fn schema_io_i32() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::input::<i32>("in"))
    }

    // ══════════════════════════════════════════════════════════════════
    // apply_patch() — incremental patch (idempotent)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn apply_patch_upsert_machine() {
        let mut spec = DeploySpec::new().with_machine(machine("a"));
        // upsert 已存在的机器：整体替换。
        let updated = MachineInstance::new("a", "new_type", MachinePhysicalSpec::default());
        spec.apply_patch(&Patch::UpsertMachine(updated));
        assert_eq!(spec.machines.len(), 1);
        assert_eq!(spec.machines[0].machine_type.as_ref(), "new_type");
        // upsert 新机器：添加。
        spec.apply_patch(&Patch::UpsertMachine(machine("b")));
        assert_eq!(spec.machines.len(), 2);
    }

    #[test]
    fn apply_patch_remove_machine_cascades_links() {
        let mut spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"));
        spec.apply_patch(&Patch::RemoveMachine("a".into()));
        assert_eq!(spec.machines.len(), 1);
        assert!(spec.links.is_empty(), "removing a machine cascades its links");
        // 幂等：再删一次仍是 no-op。
        spec.apply_patch(&Patch::RemoveMachine("a".into()));
        assert_eq!(spec.machines.len(), 1);
    }

    #[test]
    fn apply_patch_upsert_link_replaces_kind() {
        let mut spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"));
        // upsert 相同端点：替换 kind。
        let channel = LinkSpec::new(
            ("a", "out"),
            ("b", "in"),
            LinkKind::Channel {
                capacity: 8,
                drop_when_full: true,
            },
        );
        spec.apply_patch(&Patch::UpsertLink(channel));
        assert_eq!(spec.links.len(), 1);
        assert!(matches!(spec.links[0].kind, LinkKind::Channel { .. }));
    }

    #[test]
    fn apply_patch_remove_link_idempotent() {
        let mut spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"));
        let out = ("a".into(), "out".into());
        let into = ("b".into(), "in".into());
        spec.apply_patch(&Patch::RemoveLink {
            out: out.clone(),
            into: into.clone(),
        });
        assert!(spec.links.is_empty());
        // 幂等：再删一次仍是 no-op。
        spec.apply_patch(&Patch::RemoveLink { out, into });
        assert!(spec.links.is_empty());
    }

    #[test]
    fn apply_patches_sequence() {
        // 一次应用多个补丁：从空蓝图重建一个拓扑。
        let mut spec = DeploySpec::new();
        spec.apply_patches(&[
            Patch::UpsertMachine(machine("a")),
            Patch::UpsertMachine(machine("b")),
            Patch::UpsertLink(inline("a", "out", "b", "in")),
        ]);
        assert_eq!(spec.machines.len(), 2);
        assert_eq!(spec.links.len(), 1);
        assert!(spec.validate().is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // validate() — structural checks
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_ok_single_machine() {
        let spec = DeploySpec::new().with_machine(machine("a"));
        assert!(spec.validate().is_ok(), "single machine should validate");
    }

    #[test]
    fn validate_ok_two_machines_linked() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        assert!(spec.validate().is_ok(), "two linked machines should validate");
    }

    #[test]
    fn validate_rejects_duplicate_machine_name() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("a"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::DuplicateName(ref n) if n == "a"));
    }

    #[test]
    fn validate_rejects_duplicate_func_name() {
        let spec = DeploySpec::new()
            .with_func(FuncBinding::new("f", "ftype"))
            .with_func(FuncBinding::new("f", "ftype"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::DuplicateName(ref n) if n == "f"));
    }

    #[test]
    fn validate_rejects_self_loop() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_link(bounded("a", "out", "a", "in"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::SelfLoop(ref n) if n == "a"));
    }

    #[test]
    fn validate_rejects_unknown_machine_in_link_src() {
        // "a" (src) does not exist; only "b" is declared.
        let spec = DeploySpec::new()
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::UnknownMachine(ref n) if n == "a"));
    }

    #[test]
    fn validate_rejects_unknown_machine_in_link_dst() {
        // "b" (dst) does not exist; only "a" is declared.
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_link(bounded("a", "out", "b", "in"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::UnknownMachine(ref n) if n == "b"));
    }

    // ══════════════════════════════════════════════════════════════════
    // validate_deep() — port / type / flow / version compatibility
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_deep_ok_compatible_types() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", schema_io_i32());
        schemas.insert("b", schema_io_i32());
        let result = spec.validate_deep(&schemas);
        assert!(result.is_ok(), "i32 Out → i32 In should be compatible, got: {:?}", result.err());
    }

    #[test]
    fn validate_deep_rejects_source_not_output() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "x", "b", "in"));
        let mut schemas = HashMap::new();
        // a's port "x" is an INPUT, not an output.
        schemas.insert("a", PortSchema::new().with(PortDecl::input::<i32>("x")));
        schemas.insert("b", schema_io_i32());
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::LinkTypeMismatch { reason, .. } => {
                assert_eq!(reason, "source port is not an output");
            }
            other => panic!("expected LinkTypeMismatch, got: {:?}", other),
        }
    }

    #[test]
    fn validate_deep_rejects_target_not_input() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "y"));
        let mut schemas = HashMap::new();
        schemas.insert("a", schema_io_i32());
        // b's port "y" is an OUTPUT, not an input.
        schemas.insert("b", PortSchema::new().with(PortDecl::output::<i32>("y")));
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::LinkTypeMismatch { reason, .. } => {
                assert_eq!(reason, "target port is not an input");
            }
            other => panic!("expected LinkTypeMismatch, got: {:?}", other),
        }
    }

    #[test]
    fn validate_deep_rejects_type_mismatch() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", PortSchema::new().with(PortDecl::output::<i32>("out")));
        schemas.insert("b", PortSchema::new().with(PortDecl::input::<String>("in")));
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::LinkTypeMismatch { reason, .. } => {
                assert_eq!(reason, "type mismatch");
            }
            other => panic!("expected LinkTypeMismatch, got: {:?}", other),
        }
    }

    #[test]
    fn validate_deep_rejects_flow_mismatch() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let mut schemas = HashMap::new();
        // a's "out" is Control Out, b's "in" is Data In — flow mismatch.
        schemas.insert("a", PortSchema::new().with(PortDecl::ctrl_out::<i32>("out")));
        schemas.insert("b", PortSchema::new().with(PortDecl::input::<i32>("in")));
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::LinkTypeMismatch { reason, .. } => {
                assert_eq!(reason, "flow kind mismatch");
            }
            other => panic!("expected LinkTypeMismatch, got: {:?}", other),
        }
    }

    #[test]
    fn validate_deep_accepts_version_drift_1() {
        // schema_ver 0 → 1: can_link_to returns Migrate, which validate_deep accepts.
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let mut schemas = HashMap::new();
        schemas.insert(
            "a",
            PortSchema::new().with(PortDecl::output::<i32>("out").with_schema_ver(0)),
        );
        schemas.insert(
            "b",
            PortSchema::new().with(PortDecl::input::<i32>("in").with_schema_ver(1)),
        );
        let result = spec.validate_deep(&schemas);
        assert!(result.is_ok(), "schema_ver drift of 1 should be accepted as Migrate, got: {:?}", result.err());
    }

    #[test]
    fn validate_deep_rejects_version_drift_2() {
        // schema_ver 0 → 2: can_link_to returns Incompatible ("schema version drift > 1").
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let mut schemas = HashMap::new();
        schemas.insert(
            "a",
            PortSchema::new().with(PortDecl::output::<i32>("out").with_schema_ver(0)),
        );
        schemas.insert(
            "b",
            PortSchema::new().with(PortDecl::input::<i32>("in").with_schema_ver(2)),
        );
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::LinkTypeMismatch { reason, .. } => {
                assert_eq!(reason, "schema version drift > 1");
            }
            other => panic!("expected LinkTypeMismatch, got: {:?}", other),
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // validate_deep() — resource budget
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_deep_rejects_resource_budget_exceeded() {
        // 3 CpuBound machines (3 dedicated threads) vs cpu_threads = 2.
        let mut spec = DeploySpec::new()
            .with_machine(machine_cpu("a"))
            .with_machine(machine_cpu("b"))
            .with_machine(machine_cpu("c"));
        spec.settings = DeploySettings { cpu_threads: 2, io_threads: 2 };
        let schemas = HashMap::new(); // no links → schemas unused
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::ResourceBudgetExceeded {
                requested_threads,
                available_threads,
            } => {
                assert_eq!(requested_threads, 3);
                assert_eq!(available_threads, 2);
            }
            other => panic!("expected ResourceBudgetExceeded, got: {:?}", other),
        }
    }

    #[test]
    fn validate_deep_ok_resource_budget_within_limit() {
        // 2 CpuBound machines (2 dedicated threads) vs cpu_threads = 2 — within limit.
        let mut spec = DeploySpec::new()
            .with_machine(machine_cpu("a"))
            .with_machine(machine_cpu("b"));
        spec.settings = DeploySettings { cpu_threads: 2, io_threads: 2 };
        let schemas = HashMap::new();
        let result = spec.validate_deep(&schemas);
        assert!(result.is_ok(), "2 CpuBound with cpu_threads=2 should be within budget, got: {:?}", result.err());
    }

    // ══════════════════════════════════════════════════════════════════
    // validate_deep() — cycle safety (Moore analysis)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_deep_rejects_unsafe_non_moore_cycle() {
        // a → b → a via BoundedBuf, neither machine Moore → algebraic loop.
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "a", "in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", schema_io_i32());
        schemas.insert("b", schema_io_i32());
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::UnsafeCycle { cycle } => {
                assert!(cycle.contains(&"a".to_string()), "cycle should mention a: {:?}", cycle);
                assert!(cycle.contains(&"b".to_string()), "cycle should mention b: {:?}", cycle);
            }
            other => panic!("expected UnsafeCycle, got: {:?}", other),
        }
    }

    #[test]
    fn validate_deep_ok_moore_breaks_cycle() {
        // a → b → a via BoundedBuf, a is Moore → cycle is algebraically safe.
        let spec = DeploySpec::new()
            .with_machine(machine_moore("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "a", "in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", schema_io_i32());
        schemas.insert("b", schema_io_i32());
        let result = spec.validate_deep(&schemas);
        assert!(result.is_ok(), "Moore machine on the cycle should make it safe, got: {:?}", result.err());
    }

    // ══════════════════════════════════════════════════════════════════
    // validate_deep() — Inline acyclicity
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_deep_rejects_inline_cycle() {
        // a → b → a via Inline links. a is Moore so the non-Moore cycle check
        // (step 4) does not fire; the Inline-cycle check (step 6) catches the
        // synchronous-call deadlock.
        let spec = DeploySpec::new()
            .with_machine(machine_moore("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"))
            .with_link(inline("b", "out", "a", "in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", schema_io_i32());
        schemas.insert("b", schema_io_i32());
        let err = spec.validate_deep(&schemas).unwrap_err();
        match err {
            ValidationError::InlineCycle { cycle } => {
                assert!(cycle.contains(&"a".to_string()), "cycle should mention a: {:?}", cycle);
                assert!(cycle.contains(&"b".to_string()), "cycle should mention b: {:?}", cycle);
            }
            other => panic!("expected InlineCycle, got: {:?}", other),
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // validate_deep() — degree constraints
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_deep_rejects_degree_violation_inline_fanout() {
        // Two Inline links from a::out → outdeg 2, limit 1.
        // 结构层现在**提前**拦截：FanOutViaTee（fan-out 必须显式 Tee）。
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(inline("a", "out", "b", "in"))
            .with_link(inline("a", "out", "c", "in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", PortSchema::new().with(PortDecl::output::<i32>("out")));
        schemas.insert("b", PortSchema::new().with(PortDecl::input::<i32>("in")));
        schemas.insert("c", PortSchema::new().with(PortDecl::input::<i32>("in")));
        let err = spec.validate_deep(&schemas).unwrap_err();
        // 验证错误信息引导用户用 Tee
        assert!(
            err.to_string().contains("fan-out must be explicit"),
            "error should guide the user to an explicit Tee: {err}"
        );
        match err {
            ValidationError::FanOutViaTee { src_machine, src_port } => {
                assert_eq!(src_machine, "a");
                assert_eq!(src_port, "out");
            }
            other => panic!(
                "expected FanOutViaTee (fan-out must be explicit), got: {:?}",
                other
            ),
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // validate_report() — structured, collect-all violations
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_report_collects_all_violations() {
        // Three links with distinct problems: unknown machine, unknown port,
        // wrong direction (target is an Out port).
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(("ghost", "out"), ("b", "in"), LinkKind::Inline))
            .with_link(LinkSpec::new(("a", "nope"), ("b", "in"), LinkKind::Inline))
            .with_link(LinkSpec::new(("a", "out"), ("b", "out"), LinkKind::Inline));
        let mut schemas = HashMap::new();
        schemas.insert("a", schema_io_i32());
        schemas.insert("b", schema_io_i32());
        let report = spec.validate_report(&schemas);
        assert!(!report.is_ok());
        let ids: Vec<&str> = report.violations.iter().map(|v| v.rule_id).collect();
        assert!(
            ids.contains(&"link-resolve-machine"),
            "unknown machine expected: {ids:?}"
        );
        assert!(
            ids.contains(&"link-resolve-port"),
            "unknown port expected: {ids:?}"
        );
        assert!(
            ids.contains(&"link-direction"),
            "direction violation expected: {ids:?}"
        );
        // fail-fast validate_deep stops at the first error; the report sees all.
        assert!(ids.len() >= 3, "expected ≥3 violations, got {ids:?}");
    }

    #[test]
    fn validate_report_ok_on_clean_spec() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline));
        let mut schemas = HashMap::new();
        schemas.insert("a", schema_io_i32());
        schemas.insert("b", schema_io_i32());
        let report = spec.validate_report(&schemas);
        assert!(report.is_ok(), "{:?}", report.violations);
    }

    #[test]
    fn validate_report_rule_violation_display() {
        let v = RuleViolation::new(
            "link-type",
            "links[0]",
            "source port can link to target port",
            "i32 vs String",
        );
        let s = v.to_string();
        assert!(s.contains("[link-type]"), "{s}");
        assert!(s.contains("links[0]"), "{s}");
    }
}

// ── Structured validation report ────────────────────────────────────────────────

/// A single rule violation, structured for programmatic (AI) consumption.
///
/// Unlike a bare error string, each violation carries enough structure to be
/// located and fixed automatically: a stable `rule_id` (the rule that fired),
/// a JSON-path-like `path` (where in the blueprint), and `expected`/`actual`
/// (what the blueprint should look like vs. what it contains).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleViolation {
    /// Stable identifier of the violated rule (e.g. `"link-type"`).
    pub rule_id: &'static str,
    /// JSON-path-like location of the offending element (e.g. `links[3].into[1]`).
    pub path: String,
    /// What the deployment should look like.
    pub expected: String,
    /// What the deployment actually contains.
    pub actual: String,
}

impl RuleViolation {
    /// Construct a violation.
    pub fn new(
        rule_id: &'static str,
        path: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            rule_id,
            path: path.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }
}

impl core::fmt::Display for RuleViolation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "[{}] {}: expected {}, got {}",
            self.rule_id, self.path, self.expected, self.actual
        )
    }
}

/// All violations (and advisory warnings) found by [`DeploySpec::validate_report`].
///
/// - `violations` — hard failures: the blueprint is invalid.
/// - `warnings` — advisory: valid, but may require runtime support
///   (e.g. schema migration).
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// Hard violations — the blueprint is invalid.
    pub violations: Vec<RuleViolation>,
    /// Advisory warnings — valid, but may need runtime support.
    pub warnings: Vec<RuleViolation>,
}

impl ValidationReport {
    /// True when there are no hard violations (warnings are allowed).
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn push(&mut self, v: RuleViolation) {
        self.violations.push(v);
    }

    pub fn warn(&mut self, v: RuleViolation) {
        self.warnings.push(v);
    }
}

