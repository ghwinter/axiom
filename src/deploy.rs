/// **Maturity: stable** (the stable core, the main subject of the current refactor).
///
/// Deployment specification — the "what, where, and how" of a system.
///
/// A `DynamicTopology` describes the complete topology of a deployed system:
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
/// `serialize` feature — both paths produce the same `DynamicTopology`:
///
/// ```ignore
/// // Code-defined topology
/// let deploy = DynamicTopology::new()
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
/// // let deploy: DynamicTopology = serde_json::from_str(&json)?;
/// ```

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use crate::compat::HashMap;
#[cfg(not(feature = "std"))]
use alloc::format;
use crate::flow::{CarrierCompatibility, CarrierCompatResult, FlowKind};
use crate::link::{LinkKind, LinkSpec, WritePolicy};
use crate::port::{PortSchema, PortDir, LinkCompat};
use crate::resource::{MachinePhysicalSpec, ExecutionHint};
use crate::topology::Topology;
use alloc::borrow::Cow;

// ── Machine instance ──────────────────────────────────────────────────────────

/// A single machine instance in the deployment topology.
///
/// Name and type fields use [`Cow<'static, str>`] so an instance can be built
/// from `&'static str` literals in code or from owned `String`s deserialized
/// out of a config file. Under the `serialize` feature the whole `DynamicTopology`
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
    /// Declared by the deployer. Used by [`DynamicTopology::validate_deep`] for
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
    /// deploy-time cycle-safety check in [`DynamicTopology::validate_deep`].
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

/// Incremental patches for a blueprint — declarative intent (idempotent).
///
/// Used for the blueprint's **incremental evolution**: a consumer (such as an
/// AI blueprint interface) reads the current blueprint and produces a sequence
/// of `Patch`es rather than rewriting the whole graph. Every operation is
/// **idempotent** — `remove` of a non-existent machine/link is a no-op,
/// `upsert` matches by identity (machine name / link endpoint) and replaces if
/// present, otherwise adds.
///
/// After applying, verify the result with
/// [`DynamicTopology::validate`] / [`DynamicTopology::validate_deep`] — the
/// patch itself never errors (idempotent); correctness is enforced by the
/// validator. This fits the blueprint's `serialize` capability: the AI writes
/// a JSON patch, gets structured validation errors, and iterates.
///
/// # Identity conventions
///
/// - a machine instance is identified by `name`;
/// - a link is identified by the `(out, into)` endpoint pair (one `(machine, port)` each).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub enum Patch {
    /// Upsert a machine instance: match by `name`; replace the whole instance
    /// if it exists, otherwise add it.
    UpsertMachine(MachineInstance),
    /// Remove a machine (by `name`) and all of its related links; no-op if absent.
    RemoveMachine(Cow<'static, str>),
    /// Upsert a link: match by the `(out, into)` endpoints; replace `kind`
    /// if it exists, otherwise add it.
    UpsertLink(LinkSpec),
    /// Remove a link (matched by the `(out, into)` endpoints); no-op if absent.
    RemoveLink {
        out: (Cow<'static, str>, Cow<'static, str>),
        into: (Cow<'static, str>, Cow<'static, str>),
    },
}

// ── Full spec ─────────────────────────────────────────────────────────────────

/// Complete deployment specification.
///
/// Under the `serialize` feature, `DynamicTopology` is fully `Serialize`/`Deserialize`
/// — every field (including `MachineInstance::name`, `LinkSpec` endpoints) uses
/// `Cow<'static, str>`, so a topology declared in TOML/JSON can be loaded
/// directly into a `DynamicTopology` and handed to a runtime adapter.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct DynamicTopology {
    pub machines: Vec<MachineInstance>,
    pub funcs: Vec<FuncBinding>,
    pub links: Vec<LinkSpec>,
    pub settings: DeploySettings,
}

// Blueprint concept: `DynamicTopology` is the **runtime projection (value form)**
// of `Topology` — a declared, value-ified, type-erased topology. See [`Topology`]
// (blueprint = unified topology declaration language).
impl Topology for DynamicTopology {}

/// Whether the Moore-on-every-cycle rule applies during deep validation.
///
/// A cycle of machines whose outputs depend on current inputs is an algebraic
/// loop. Two kinds of runtime can break it:
///
/// - a **Moore machine** on the cycle (output depends only on pre-update
///   state) — the blueprint-level guarantee that [`validate_deep`](crate::deploy::DynamicTopology::validate_deep)
///   enforces by default;
/// - a **per-link delay** — a channel-based runtime where every link is a
///   one-tick buffer *is* the delay element, so a cycle is safe without Moore
///   machines. That runtime passes [`CycleRule::AnyDelay`].
///
/// A runtime adapter chooses the rule from its declared physics (its
/// [`LinkDelay`](crate::runtime_contract::LinkDelay)) and calls
/// [`DynamicTopology::validate_deep_for`] at materialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleRule {
    /// Every cycle must pass through ≥1 Moore machine (strict, runtime-agnostic).
    RequireMoore,
    /// Per-link delay breaks cycles — no Moore requirement (channel-based runtime).
    AnyDelay,
}

impl CycleRule {
    /// Map a runtime's declared link-delay fact onto the cycle rule: zero delay
    /// requires Moore machines; a one-tick (or greater) delay allows any cycle.
    pub fn from_link_delay(delay: crate::runtime_contract::LinkDelay) -> Self {
        match delay {
            crate::runtime_contract::LinkDelay::Zero => CycleRule::RequireMoore,
            crate::runtime_contract::LinkDelay::OneTick => CycleRule::AnyDelay,
        }
    }
}

impl DynamicTopology {
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
    /// which is not available in the static `DynamicTopology`. These checks are
    /// performed at runtime via `LinkCompat::check` or in `validate_deep`.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // 1. Name uniqueness check
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

        // 2. Existence of machines/funcs referenced by links + self-loop check
        for link in &self.links {
            let src_name: &str = link.out.0.as_ref();
            let dst_name: &str = link.into.0.as_ref();

            // Self-loop check
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

        // 2.5 Implicit fan-out rejection: an output port may link to only one target.
        // Dynamic-path routing is 1-to-1 (route_target takes the first target);
        // fan-out is the **machine's responsibility** (Split / CloneSplit / Tee
        // contract) — a blueprint must express broadcast through an explicit Tee
        // machine. This check rejects the undefined behavior of "routing silently
        // dropping the second target" at validation time.
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

        // 3. Cycles: cycles are structurally ALLOWED here.
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
    /// This is the **strict, runtime-agnostic** variant: it requires a Moore
    /// machine on every cycle (the safe default for an unknown runtime). A
    /// runtime that knows its own physics — e.g. a channel-based runtime whose
    /// every link carries a one-tick delay and can therefore break algebraic
    /// cycles without Moore machines — should call
    /// [`validate_deep_for`](Self::validate_deep_for) with its declared
    /// [`CycleRule`].
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
    ///    [`ValidationError::UnsafeCycle`].
    /// 6. **Edge-degree constraints**: per-port limits for constrained
    ///    `LinkKind` variants (Inline outdeg ≤ 1, Channel indeg ≤ 1,
    ///    CasFreeRing SPSC). Rejected as [`ValidationError::DegreeConstraintViolated`].
    /// 7. **Inline acyclicity**: the Inline-edge subgraph must be a DAG — an
    ///    Inline cycle is a synchronous-call deadlock. Rejected as
    ///    [`ValidationError::InlineCycle`].
    /// 8. **Materialization compatibility** (S3-1): for each **annotated**
    ///    edge (`Observe`/`Control` flow), the carrier must not violate the
    ///    flow semantics — an `Observe` edge (which must not back-pressure
    ///    its source) on a blocking carrier (`BoundedBuf(Blocking)` /
    ///    `Channel(drop=false)`) is rejected as
    ///    [`ValidationError::CarrierViolatesSemantics`]. `Data` (the default /
    ///    un-annotated) carries no constraint. See [`carrier_compatible`].
    ///
    /// # What it does NOT check
    ///
    /// - Whether a migrator exists for `LinkCompat::Migrate` (that requires
    ///   a `MigrateRegistry`, checked at runtime).
    /// - Actual memory consumption (only declared estimates are summed).
    /// - Whether `MachineInstance::is_moore` matches the machine's type
    ///   implementing [`crate::machine::Moore`] — that needs type information
    ///   and is checked at deploy/materialize time by the runtime registry.
    pub fn validate_deep(&self, schemas: &HashMap<&str, PortSchema>) -> Result<(), ValidationError> {
        self.validate_deep_for(schemas, CycleRule::RequireMoore)
    }

    /// Deep validation with a runtime-aware cycle rule.
    ///
    /// Identical to [`validate_deep`](Self::validate_deep) except that the
    /// Moore-on-every-cycle requirement is applied only when `rule` is
    /// [`CycleRule::RequireMoore`]. A runtime whose links carry delay (a
    /// channel-based runtime: each link is a one-tick buffer) can break
    /// algebraic cycles without Moore machines, and passes
    /// [`CycleRule::AnyDelay`].
    pub fn validate_deep_for(
        &self,
        schemas: &HashMap<&str, PortSchema>,
        rule: CycleRule,
    ) -> Result<(), ValidationError> {
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

            // 2b. Materialization compatibility matrix (S3-1): the flow kind
            // is a semantic annotation that implies an optional carrier
            // preference. `Violates` is a hard contract violation (an
            // annotated edge whose carrier actively contradicts its
            // semantics); `Permitted` is suboptimal but never wrong.
            match carrier_compatible(src_port.flow, &link.kind) {
                CarrierCompatResult::Violates => {
                    return Err(ValidationError::CarrierViolatesSemantics {
                        out: (src_machine.to_string(), src_port_name.to_string()),
                        into: (dst_machine.to_string(), dst_port_name.to_string()),
                        flow: src_port.flow.as_str(),
                        carrier: link.kind.name().to_string(),
                        reason: format!(
                            "{} flow must not be back-pressured, but carrier blocks the producer",
                            src_port.flow.as_str()
                        ),
                    });
                }
                CarrierCompatResult::Recommended | CarrierCompatResult::Permitted => {}
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

        // 4. Cycle safety: every cycle must pass through ≥1 Moore machine —
        //    but only when the runtime cannot break cycles itself (its links
        //    have no delay). A channel-based runtime (each link a one-tick
        //    buffer) declares `CycleRule::AnyDelay` and is exempt.
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
        if rule == CycleRule::RequireMoore {
            if let Some(cycle) = self.find_non_moore_cycle() {
                return Err(ValidationError::UnsafeCycle { cycle });
            }
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

            // 2b. Materialization compatibility matrix (S3-1): a `Violates`
            // carrier is a hard contract violation. `Permitted`/`Recommended`
            // produce no finding (lossless carriers are suboptimal, never
            // wrong).
            match carrier_compatible(src_port.flow, &link.kind) {
                CarrierCompatResult::Violates => {
                    report.push(RuleViolation::new(
                        "flow-carrier",
                        format!("links[{i}].kind"),
                        format!(
                            "{} flow on a non-back-pressuring carrier",
                            src_port.flow.as_str()
                        ),
                        format!(
                            "carrier {} can block the producer ({} → {})",
                            link.kind.name(),
                            src_machine,
                            dst_machine,
                        ),
                    ));
                }
                CarrierCompatResult::Recommended | CarrierCompatResult::Permitted => {}
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

// ── Materialization compatibility matrix (S3-1) ───────────────────────────────

/// The `(FlowKind, LinkKind)` materialization compatibility matrix.
///
/// `FlowKind` is a *semantic* annotation (unified value-flow principle — the
/// physical layer does not distinguish Data/Control/Observe). From the
/// receiver-side semantics it implies an **optional physical preference**
/// (materialization preference), and the matrix classifies each carrier
/// against that preference:
///
/// | | `Recommended` | `Permitted` (suboptimal, no finding) | `Violates` |
/// |---|--------------|--------------------------------------|------------|
/// | `Data` (default / un-annotated) | every carrier | — | — |
/// | `Observe` ("must not back-pressure the source") | `Latest`, `BoundedBuf(Dropping\|Overwriting)`, `Channel(drop=true)`, `CasFreeRing`, `SharedState` | `Inline` (same-thread coupling) | `BoundedBuf(Blocking)`, `Channel(drop=false)` |
/// | `Control` ("droppable, latest wins") | `Latest`, `BoundedBuf(Dropping\|Overwriting)`, `Channel(drop=true)` | `Inline`, `BoundedBuf(Blocking)`, `Channel(drop=false)`, `SharedState`, `CasFreeRing` | — |
///
/// Only **annotated** edges enter the matrix: `Data` (the default) carries no
/// constraint, so un-annotated edges are always `Recommended` — a blueprint
/// without annotations is not slower or less valid, it simply runs under pure
/// structural analysis.
///
/// `Violates` is a hard contract violation (an Observe edge that can block
/// its producer contradicts "must not back-pressure the source") and is
/// rejected by [`DynamicTopology::validate_deep`]. `Permitted` is accepted
/// silently — lossless carriers are *more* conservative than the preference,
/// which is never wrong (a Control edge on a blocking channel still delivers
/// the latest value; it just may block).
pub fn carrier_compatible(flow: FlowKind, kind: &LinkKind) -> CarrierCompatResult {
    match flow.carrier_compatibility() {
        CarrierCompatibility::Any => CarrierCompatResult::Recommended,
        CarrierCompatibility::NonBlocking => match kind {
            // Blocking senders back-pressure the source → violate Observe.
            LinkKind::BoundedBuf { write_policy, .. } => match write_policy {
                WritePolicy::Blocking => CarrierCompatResult::Violates,
                WritePolicy::Dropping | WritePolicy::Overwriting => {
                    CarrierCompatResult::Recommended
                }
            },
            LinkKind::Channel { drop_when_full, .. } => {
                if *drop_when_full {
                    CarrierCompatResult::Recommended
                } else {
                    CarrierCompatResult::Violates
                }
            }
            // Same-thread observation couples observer latency to the source;
            // acceptable when a tight coupling is intended, but not the
            // "independent thread" recommendation.
            LinkKind::Inline => CarrierCompatResult::Permitted,
            // Latest / CasFreeRing / SharedState never block the producer.
            LinkKind::Latest { .. }
            | LinkKind::CasFreeRing { .. }
            | LinkKind::SharedState => CarrierCompatResult::Recommended,
        },
        CarrierCompatibility::LatestWins => match kind {
            LinkKind::Latest { .. }
            | LinkKind::BoundedBuf {
                write_policy: WritePolicy::Dropping | WritePolicy::Overwriting,
                ..
            }
            | LinkKind::Channel {
                drop_when_full: true,
                ..
            } => CarrierCompatResult::Recommended,
            // Lossless / same-thread carriers deliver at least the latest
            // value (they deliver more) — suboptimal, never wrong.
            LinkKind::Inline
            | LinkKind::BoundedBuf {
                write_policy: WritePolicy::Blocking,
                ..
            }
            | LinkKind::Channel {
                drop_when_full: false,
                ..
            }
            | LinkKind::CasFreeRing { .. }
            | LinkKind::SharedState => CarrierCompatResult::Permitted,
        },
    }
}

// ── Validation errors ─────────────────────────────────────────────────────────

/// Errors raised by [`DynamicTopology::validate`] / [`DynamicTopology::validate_deep`].
///
/// Variants own their diagnostic strings (`String`, not `&'static str`) so they
/// can describe names that originate from a deserialized config file as well as
/// from compile-time literals. This also makes `ValidationError` self-contained
/// and freely storable across `Result` boundaries.
#[derive(Debug)]
pub enum ValidationError {
    UnknownMachine(String),
    /// A machine name or function name is duplicated within the deployment.
    DuplicateName(String),
    /// A machine links to itself.
    SelfLoop(String),
    /// Implicit fan-out: an output port links to multiple targets.
    ///
    /// Dynamic-path routing is 1-to-1; fan-out must be the machine's
    /// responsibility (a `Split`/`CloneSplit`/`Tee`). Fix: insert an explicit
    /// Tee machine, one link per port.
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
    /// The resource budget was exceeded (used by `validate_deep` only).
    ResourceBudgetExceeded {
        requested_threads: usize,
        available_threads: usize,
    },
    /// The topology contains a cycle with no Moore machine on it — an algebraic loop.
    ///
    /// `cycle` lists the machine names on the cycle (in cycle traversal order).
    /// To fix: mark at least one machine on the cycle as Moore
    /// ([`.moore()`](crate::deploy::MachineInstance::moore)), or ensure the
    /// runtime provides a one-tick delay on every link (a channel-based runtime).
    UnsafeCycle { cycle: Vec<String> },
    /// A per-port edge-degree constraint was violated.
    ///
    /// Each `LinkKind` has an upper bound on connections per port (see `docs/architecture.md` §2):
    /// - `Inline`: each output port has out-degree ≤ 1
    /// - `Channel`: each input port has in-degree ≤ 1 (single consumer)
    /// - `CasFreeRing`: out-degree ≤ 1 and in-degree ≤ 1 (SPSC)
    DegreeConstraintViolated {
        machine: String,
        port: String,
        link_kind: String,
        direction: String,
        limit: usize,
        actual: usize,
    },
    /// Inline edges form a cycle — a synchronous-call deadlock.
    ///
    /// `Inline` connections are direct function calls with no delay. A cycle of
    /// Inline edges is a recursive call that never returns — a deadlock. Even a
    /// Moore machine on the cycle does not help (Moore delay is only effective
    /// on buffered/channel links).
    InlineCycle { cycle: Vec<String> },
    /// The materialization annotation (`FlowKind`) conflicts with the carrier
    /// (`LinkKind`) semantic contract (S3-1).
    ///
    /// Applies only to **annotated** edges (`Observe`/`Control`):
    /// - `Observe` ("must not back-pressure the source") on a blocking carrier
    ///   (`BoundedBuf(Blocking)` / `Channel(drop_when_full=false)`) — the
    ///   source gets back-pressured, violating the semantics.
    ///
    /// `Data` (default / un-annotated) carries no constraint and does not enter
    /// the matrix; no carrier of `Control` constitutes a hard conflict (a
    /// lossless carrier is merely more conservative than "droppable, latest wins").
    CarrierViolatesSemantics {
        out: (String, String),
        into: (String, String),
        flow: &'static str,
        carrier: String,
        reason: String,
    },
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
            Self::CarrierViolatesSemantics { out, into, flow, carrier, reason } => {
                write!(
                    f,
                    "flow-kind {:?} → carrier {:?} violates semantics on {:?} → {:?}: {}",
                    flow, carrier, out, into, reason
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
        let mut spec = DynamicTopology::new().with_machine(machine("a"));
        // Upsert an existing machine: replace it entirely.
        let updated = MachineInstance::new("a", "new_type", MachinePhysicalSpec::default());
        spec.apply_patch(&Patch::UpsertMachine(updated));
        assert_eq!(spec.machines.len(), 1);
        assert_eq!(spec.machines[0].machine_type.as_ref(), "new_type");
        // Upsert a new machine: add it.
        spec.apply_patch(&Patch::UpsertMachine(machine("b")));
        assert_eq!(spec.machines.len(), 2);
    }

    #[test]
    fn apply_patch_remove_machine_cascades_links() {
        let mut spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"));
        spec.apply_patch(&Patch::RemoveMachine("a".into()));
        assert_eq!(spec.machines.len(), 1);
        assert!(spec.links.is_empty(), "removing a machine cascades its links");
        // Idempotent: removing again is still a no-op.
        spec.apply_patch(&Patch::RemoveMachine("a".into()));
        assert_eq!(spec.machines.len(), 1);
    }

    #[test]
    fn apply_patch_upsert_link_replaces_kind() {
        let mut spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"));
        // Upsert with the same endpoints: replace the kind.
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
        let mut spec = DynamicTopology::new()
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
        // Idempotent: removing again is still a no-op.
        spec.apply_patch(&Patch::RemoveLink { out, into });
        assert!(spec.links.is_empty());
    }

    #[test]
    fn apply_patches_sequence() {
        // Apply multiple patches at once: rebuild a topology from an empty blueprint.
        let mut spec = DynamicTopology::new();
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
        let spec = DynamicTopology::new().with_machine(machine("a"));
        assert!(spec.validate().is_ok(), "single machine should validate");
    }

    #[test]
    fn validate_ok_two_machines_linked() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        assert!(spec.validate().is_ok(), "two linked machines should validate");
    }

    #[test]
    fn validate_rejects_duplicate_machine_name() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("a"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::DuplicateName(ref n) if n == "a"));
    }

    #[test]
    fn validate_rejects_duplicate_func_name() {
        let spec = DynamicTopology::new()
            .with_func(FuncBinding::new("f", "ftype"))
            .with_func(FuncBinding::new("f", "ftype"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::DuplicateName(ref n) if n == "f"));
    }

    #[test]
    fn validate_rejects_self_loop() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_link(bounded("a", "out", "a", "in"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::SelfLoop(ref n) if n == "a"));
    }

    #[test]
    fn validate_rejects_unknown_machine_in_link_src() {
        // "a" (src) does not exist; only "b" is declared.
        let spec = DynamicTopology::new()
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, ValidationError::UnknownMachine(ref n) if n == "a"));
    }

    #[test]
    fn validate_rejects_unknown_machine_in_link_dst() {
        // "b" (dst) does not exist; only "a" is declared.
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let mut spec = DynamicTopology::new()
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
        let mut spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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
        // The structural layer now intercepts **early**: FanOutViaTee
        // (fan-out must be an explicit Tee).
        let spec = DynamicTopology::new()
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
        // The validation error message guides the user toward a Tee
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
        let spec = DynamicTopology::new()
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
        let spec = DynamicTopology::new()
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

    // ══════════════════════════════════════════════════════════════════
    // S3-1 — FlowKind × LinkKind materialization compatibility matrix
    // ══════════════════════════════════════════════════════════════════

    /// Schema: source has an Observe output `obs` + Data output `out`;
    /// target has an Observe input `obs_in` + Data input `in`.
    fn schema_observe_pair() -> (PortSchema, PortSchema) {
        let src = PortSchema::new()
            .with(PortDecl::observe::<i32>("obs"))
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::ctrl_out::<i32>("ctrl"));
        let dst = PortSchema::new()
            .with(PortDecl::new::<i32>("obs_in", PortDir::In, FlowKind::Observe))
            .with(PortDecl::input::<i32>("in"))
            .with(PortDecl::ctrl_in::<i32>("ctrl_in"));
        (src, dst)
    }

    #[test]
    fn carrier_compatible_matrix_observe() {
        // Observe ("must not back-pressure the source"):
        // blocking carriers violate; dropping/latest/casfree/shared never do.
        assert_eq!(
            carrier_compatible(
                FlowKind::Observe,
                &LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ),
            CarrierCompatResult::Violates,
        );
        assert_eq!(
            carrier_compatible(
                FlowKind::Observe,
                &LinkKind::Channel {
                    capacity: 4,
                    drop_when_full: false,
                },
            ),
            CarrierCompatResult::Violates,
        );
        assert_eq!(
            carrier_compatible(
                FlowKind::Observe,
                &LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Dropping,
                    read_policy: ReadPolicy::NonBlocking,
                },
            ),
            CarrierCompatResult::Recommended,
        );
        assert_eq!(
            carrier_compatible(FlowKind::Observe, &LinkKind::Latest { capacity: 1 }),
            CarrierCompatResult::Recommended,
        );
        // Inline couples observer latency to the source — suboptimal, not a violation.
        assert_eq!(
            carrier_compatible(FlowKind::Observe, &LinkKind::Inline),
            CarrierCompatResult::Permitted,
        );
    }

    #[test]
    fn carrier_compatible_matrix_control_and_data() {
        // Control ("droppable, latest wins"): droppable carriers recommended;
        // lossless carriers are permitted (more conservative, never wrong).
        assert_eq!(
            carrier_compatible(
                FlowKind::Control,
                &LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Overwriting,
                    read_policy: ReadPolicy::NonBlocking,
                },
            ),
            CarrierCompatResult::Recommended,
        );
        assert_eq!(
            carrier_compatible(
                FlowKind::Control,
                &LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ),
            CarrierCompatResult::Permitted,
        );
        // Data (default / un-annotated): every carrier is fine.
        assert_eq!(
            carrier_compatible(
                FlowKind::Data,
                &LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ),
            CarrierCompatResult::Recommended,
        );
    }

    #[test]
    fn validate_deep_rejects_observe_on_blocking_carrier() {
        // An Observe edge on a blocking carrier back-pressures its source —
        // contradicts "must not back-pressure the source".
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(
                ("a", "obs"),
                ("b", "obs_in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ));
        let (sa, sb) = schema_observe_pair();
        let mut schemas = HashMap::new();
        schemas.insert("a", sa);
        schemas.insert("b", sb);
        match spec.validate_deep(&schemas) {
            Err(ValidationError::CarrierViolatesSemantics { out, into, flow, carrier, .. }) => {
                assert_eq!(out, ("a".to_string(), "obs".to_string()));
                assert_eq!(into, ("b".to_string(), "obs_in".to_string()));
                assert_eq!(flow, "observe");
                assert_eq!(carrier, "BoundedBuf");
            }
            other => panic!("expected CarrierViolatesSemantics, got: {other:?}"),
        }
    }

    #[test]
    fn validate_deep_ok_observe_on_nonblocking_carrier() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(
                ("a", "obs"),
                ("b", "obs_in"),
                LinkKind::Latest { capacity: 1 },
            ));
        let (sa, sb) = schema_observe_pair();
        let mut schemas = HashMap::new();
        schemas.insert("a", sa);
        schemas.insert("b", sb);
        assert!(spec.validate_deep(&schemas).is_ok());
    }

    #[test]
    fn validate_deep_ok_control_on_blocking_carrier() {
        // Control on a lossless blocking carrier: permitted (not a violation).
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(
                ("a", "ctrl"),
                ("b", "ctrl_in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ));
        let (sa, sb) = schema_observe_pair();
        let mut schemas = HashMap::new();
        schemas.insert("a", sa);
        schemas.insert("b", sb);
        assert!(spec.validate_deep(&schemas).is_ok());
    }

    #[test]
    fn validate_report_flags_flow_carrier() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(
                ("a", "obs"),
                ("b", "obs_in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ));
        let (sa, sb) = schema_observe_pair();
        let mut schemas = HashMap::new();
        schemas.insert("a", sa);
        schemas.insert("b", sb);
        let report = spec.validate_report(&schemas);
        assert!(
            report.violations.iter().any(|v| v.rule_id == "flow-carrier"),
            "{:?}",
            report.violations.iter().map(|v| v.rule_id).collect::<Vec<_>>()
        );
    }

    /// Matrix completion: the remaining carrier×flow cells. Observe on a
    /// dropping channel, a lock-free ring and shared memory never blocks the
    /// producer → Recommended. Control on a single-slot carrier is the
    /// canonical "latest wins" → Recommended.
    #[test]
    fn carrier_compatible_matrix_completion() {
        assert_eq!(
            carrier_compatible(
                FlowKind::Observe,
                &LinkKind::Channel {
                    capacity: 8,
                    drop_when_full: true,
                },
            ),
            CarrierCompatResult::Recommended,
        );
        assert_eq!(
            carrier_compatible(
                FlowKind::Observe,
                &LinkKind::CasFreeRing {
                    capacity: 8,
                    storage: crate::link::MemoryRegion::Heap { size: 64 },
                },
            ),
            CarrierCompatResult::Recommended,
        );
        assert_eq!(
            carrier_compatible(FlowKind::Observe, &LinkKind::SharedState),
            CarrierCompatResult::Recommended,
        );
        assert_eq!(
            carrier_compatible(FlowKind::Control, &LinkKind::Latest { capacity: 0 }),
            CarrierCompatResult::Recommended,
        );
        // Control on a lock-free ring: lossless → suboptimal, never wrong.
        assert_eq!(
            carrier_compatible(
                FlowKind::Control,
                &LinkKind::CasFreeRing {
                    capacity: 8,
                    storage: crate::link::MemoryRegion::Heap { size: 64 },
                },
            ),
            CarrierCompatResult::Permitted,
        );
    }

    /// An un-annotated (`Data`) edge carries no materialization constraint:
    /// a blocking carrier is *fine* for it — the compatibility matrix only
    /// constrains edges whose flow kind is explicitly annotated.
    #[test]
    fn validate_deep_ok_unannotated_data_on_blocking_carrier() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(
                ("a", "out"),
                ("b", "in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ));
        let (sa, sb) = schema_observe_pair();
        let mut schemas = HashMap::new();
        schemas.insert("a", sa);
        schemas.insert("b", sb);
        assert!(spec.validate_deep(&schemas).is_ok());
    }

    /// Mixed annotation topology: one annotated (`Observe` on a blocking
    /// carrier → violates) plus one un-annotated (`Data` on a blocking
    /// carrier → fine). The matrix must flag only the annotated edge.
    #[test]
    fn validate_deep_mixed_flow_flags_only_annotated_edge() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(
                ("a", "obs"),
                ("b", "obs_in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ))
            .with_link(LinkSpec::new(
                ("a", "out"),
                ("b", "in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ));
        let (sa, sb) = schema_observe_pair();
        let mut schemas = HashMap::new();
        schemas.insert("a", sa);
        schemas.insert("b", sb);
        match spec.validate_deep(&schemas) {
            Err(ValidationError::CarrierViolatesSemantics { out, flow, .. }) => {
                // The violation must name the annotated Observe edge, not the
                // un-annotated Data edge.
                assert_eq!(out, ("a".to_string(), "obs".to_string()));
                assert_eq!(flow, "observe");
            }
            other => panic!("expected CarrierViolatesSemantics, got: {other:?}"),
        }
    }

    /// The report form reports exactly one `flow-carrier` violation for the
    /// mixed topology — the Data edge contributes none.
    #[test]
    fn validate_report_mixed_flow_single_violation() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(LinkSpec::new(
                ("a", "obs"),
                ("b", "obs_in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ))
            .with_link(LinkSpec::new(
                ("a", "out"),
                ("b", "in"),
                LinkKind::BoundedBuf {
                    capacity: 4,
                    write_policy: WritePolicy::Blocking,
                    read_policy: ReadPolicy::Blocking,
                },
            ));
        let (sa, sb) = schema_observe_pair();
        let mut schemas = HashMap::new();
        schemas.insert("a", sa);
        schemas.insert("b", sb);
        let report = spec.validate_report(&schemas);
        let flow_violations: Vec<_> = report
            .violations
            .iter()
            .filter(|v| v.rule_id == "flow-carrier")
            .collect();
        assert_eq!(flow_violations.len(), 1, "{:?}", report.violations);
        // The single violation is the annotated Observe edge, not the
        // un-annotated Data edge (which the matrix does not constrain).
        assert!(flow_violations[0].expected.contains("observe"));
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

/// All violations (and advisory warnings) found by [`DynamicTopology::validate_report`].
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
