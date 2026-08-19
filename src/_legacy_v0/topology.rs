//! **Maturity: stable** (the stable core, main subject of the current refactor).
//!
//! TopologyMutation — runtime mutation of the **instance** graph.
//!
//! # Blueprint projection (T1 positioning)
//!
//! This module is one of the **runtime projections** of the `Topology`
//! blueprint (the unified topology-declaration language): see [`Topology`] /
//! [`StaticTopology`] for the blueprint concept. `DynamicTopology` is the
//! declared value form; `TopologyMutation` (formerly named `DynamicTopology`)
//! is the time-ordered form of instance mutation.
//!
//! # Static-first worldview
//!
//! axiom's default worldview is **static topology + deploy-time validation**:
//! a `DynamicTopology` declares the complete topology before the system starts,
//! `validate_deep()` checks it once, and it never changes afterwards. Static
//! topologies are zero-cost (monomorphized `static_path` functions) and
//! verifiable *before* deployment — the checks in `DynamicTopology::validate_deep`
//! and `analysis` (degree constraints, Inline acyclicity, Moore cycle
//! safety) only have meaning because the topology is fixed.
//!
//! Runtime mutation is an *optional
//! capability* for the few systems that genuinely need it; using it
//! elsewhere is a negative optimization (the dynamic tax, Theorem 15.3).
//!
//! # What "dynamic" means here — and what it does NOT mean
//!
//! One invariant governs everything in this module:
//!
//! > **The instance graph is dynamic; the type space is static.**
//!
//! - **Instance graph** — which machine *instances* exist and how they are
//!   connected: dynamic. `Spawn`/`Link`/`Unlink`/`Retire`/`Replace` mutate
//!   this graph.
//! - **Type space** — which `Machine` implementations exist: **static**.
//!   The compile-time type system (`FusedInline`, `SingleOutput`,
//!   `MachineHandle` typestate) is closed; a `TopologyOp::Spawn` can only
//!   create another instance of an already-registered `machine_type`. axiom
//!   core has no notion of "loading a new type at runtime" — plugin loading
//!   of *new code* (dlopen, wasm, scripts) is a runtime-adapter concern and
//!   lives outside this module's contract.
//!
//! Consequently, the legitimate use cases for runtime mutation are limited
//! to three:
//!
//! 1. **Elastic scaling** — add/remove *replicas* of an existing type
//!    (e.g. workers behind an `ExecutionHint::CpuBoundN(n)` hint); the
//!    topology shape is unchanged.
//! 2. **Hot-swap / self-healing** — `Replace` a failed instance with a new
//!    one. This is an *exchange*, not a deletion of a type.
//! 3. **Session / tenant subgraphs** — per-session private subgraphs spawned
//!    at session start and retired when the session ends.
//!
//! Even these three can usually be expressed with a **static topology +
//! control/state changes**: pre-allocate the maximum replica count at deploy
//! time and start/stop instances via control ports; model hot-swap as
//! standby instances toggled by state. Runtime mutation should be reached
//! for only when the topology itself must be decided *by the running
//! system* — the module exists first for contract completeness, second for
//! that minority of cases.
//!
//! Everything else (bounded systems, fixed pipelines, most production
//! deployments) should stay static: a topology that is compiled/validated
//! once and never mutates is cheaper, safer, and fully analyzable.
//!
//! # What `Retire` is — and is not
//!
//! `Retire` is the **topology projection of the machine lifecycle**
//! (`Init → Running → Stopping → Stopped → cleanup`, see `MachineHandle`
//! typestate): it records the instance's removal from the graph so the
//! runtime can drain, stop, and reclaim it. It is *not* "deleting a
//! module" — the machine type keeps existing for future spawns, and
//! process-exit memory reclamation is the OS's job, not a topology
//! operation.
//!
//! # Design
//!
//! Runtime topology mutations are expressed as `TopologyOp` commands:
//!
//! - `Spawn` — add a new machine instance
//! - `Link` — connect two ports
//! - `Unlink` — disconnect two ports
//! - `Retire` — gracefully stop and remove a machine
//! - `Replace` — atomic hot-swap (retire + spawn, links transferred)
//!
//! The runtime applies these commands atomically (one at a time), after
//! checking that they don't violate the structural invariants:
//!
//! - No duplicate machine names
//! - No duplicate links
//! - No self-loops (a machine linking to itself)
//! - No retiring machines with active links
//!
//! # Cycles are ALLOWED
//!
//! Cycles between **different** machines are allowed, consistent with
//! `DynamicTopology::validate()`. In a channel-based runtime, every
//! link introduces a one-tick delay (the receiver gets the previously-sent
//! value, not the current-tick one). This is the Moore delay in concrete
//! form — the channel IS the delay element. Feedback loops (thermostats,
//! PID controllers, autoregressive models) are first-class supported.
//!
//! Users who want strict acyclic enforcement can call [`crate::topology::TopologyMutation::detect_cycle`]
//! (Kahn's algorithm) manually — it is `pub` for opt-in strict mode.
//!
//! # Batch operations
//!
//! [`crate::topology::TopologyMutation::apply_batch`] applies multiple operations atomically — either all
//! succeed, or none do (rollback on first failure). This is essential for
//! reconfigurations that must be atomic, e.g., "replace machine A with B
//! and rewire 3 links".
//!
//! # Safety
//!
//! Dynamic topology is inherently more dangerous than static deployment:
//! - A newly spawned machine might not have its config set up correctly.
//! - Unlinking a port mid-flight might cause the peer to block forever.
//! - Retiring a machine with pending outputs might lose data.
//!
//! To mitigate these risks, the runtime should:
//! 1. Drain in-flight messages before unlinking (graceful shutdown).
//! 2. Validate each `TopologyOp` against the current topology state.
//! 3. Log all topology changes for audit and replay.
//!
//! # Usage
//!
//! ```ignore
//! use axiom::topology::{TopologyMutation, TopologyOp, TopologyDelta};
//! use axiom::prelude_all::*;
//!
//! let mut topo = TopologyMutation::new();
//!
//! // Single operation
//! topo.apply(TopologyOp::Spawn {
//!     name: "worker_2",
//!     machine_type: "worker",
//!     physical: MachinePhysicalSpec::default(),
//!     config_overrides: vec![],
//! })?;
//!
//! // Batch operation (atomic)
//! topo.apply_batch(vec![
//!     TopologyOp::Spawn {
//!         name: "a", machine_type: "t",
//!         physical: MachinePhysicalSpec::default(),
//!         config_overrides: vec![],
//!     },
//!     TopologyOp::Spawn {
//!         name: "b", machine_type: "t",
//!         physical: MachinePhysicalSpec::default(),
//!         config_overrides: vec![],
//!     },
//!     TopologyOp::Link {
//!         out: ("a", "out"),
//!         into: ("b", "in"),
//!         kind: LinkKind::Inline,
//!     },
//! ])?;
//! ```

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use crate::compat::{HashMap, HashSet, VecDeque};
use crate::deploy::{MachineInstance, DynamicTopology};
use crate::link::{LinkKind, LinkSpec};
use crate::resource::MachinePhysicalSpec;

// ════════════════════════════════════════════════════════════════════════════
// Section 0: Topology blueprint concept (T1 — the unified topology-declaration language)
// ════════════════════════════════════════════════════════════════════════════
//
// axiom's "one topology-declaration language" is expressed by a single
// blueprint concept, materialized as two paths:
//
//   Topology (blueprint) ──┬─ StaticTopology  compile-time projection (typed, monomorphized, zero cost)
//                          │                   Chain/Diamond/Composite + StraightMachine
//                          └─ runtime projection  valued, type-erased
//                                                  DynamicTopology (the declared value form)
//                                                  TopologyMutation (the time-ordered form of instance mutation)
//                                                  CompositeSpec (subgraph reuse)
//
// Structural validation (acyclicity, degree constraints, SPOF, observability
// completeness) is defined at the language layer (DynamicTopology's
// validate/analysis), and the static and dynamic forms share the same
// semantics. The static form introduces no runtime topology object —
// `StaticTopology` is a zero-sized type marker, serving only as a
// compile-time anchor.

/// Blueprint — the type-level expression of "one topology".
///
/// In axiom, every topology form is an implementation of the same blueprint
/// concept [`Topology`]:
///
/// - **Compile-time projection** ([`StaticTopology`]): the `Chain`/`Diamond`/
///   `Composite` combinators and `StraightMachine` machines — the shape is
///   fully known at compile time, zero cost.
/// - **Runtime projection**: [`DynamicTopology`](crate::deploy::DynamicTopology)
///   (the declared value form) and [`TopologyMutation`] (the time-ordered form
///   of instance mutation).
/// - **Composite**: `CompositeSpec` (the subgraph-reuse mechanism, merged into
///   the blueprint concept).
///
/// This is a **structural-layer contract**: it unifies "one
/// topology-declaration language" and specifies no execution behavior. How the
/// physical layer places it (single-threaded pass-through ↔ multi-threaded
/// cross-core) is decided by deploy-time physical decisions
/// (`MachinePhysicalSpec`), not part of the blueprint concept.
pub trait Topology {}

/// Compile-time projection — the blueprint's monomorphized materialization in
/// the type system.
///
/// Implementors: the `Chain`/`Diamond`/`Composite` combinators, and any
/// `StraightMachine` machine (via blanket impl, see `static_exec`).
/// Implementing `StaticTopology` means "this topology's shape is fully known at
/// compile time; the execution form can be monomorphized by the type system,
/// with no runtime topology object".
///
/// `StaticTopology` is a **zero-sized marker** — it carries no runtime state;
/// it only lifts the fact of "compile-time materialization" into a constrainable
/// contract at the type level (e.g. a generic function can require `T:
/// StaticTopology`).
pub trait StaticTopology: Topology {}

// Blanket: any `StaticTopology` is a `Topology` (the compile-time projection
// is one form of the blueprint).
impl<T: StaticTopology> Topology for T {}

// ── Topology operation ──────────────────────────────────────────────────────

/// A single mutation to the runtime topology.
///
/// Each variant corresponds to one atomic change. The runtime applies
/// these via [`TopologyMutation::apply`], which validates and records
/// each operation.
#[derive(Debug, Clone)]
pub enum TopologyOp {
    /// Spawn a new machine instance.
    Spawn {
        name: &'static str,
        machine_type: &'static str,
        physical: MachinePhysicalSpec,
        config_overrides: Vec<(&'static str, String)>,
    },

    /// Link two ports (connect source output to target input).
    Link {
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
        kind: LinkKind,
    },

    /// Unlink two ports (disconnect source from target).
    Unlink {
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
    },

    /// Retire a machine: graceful shutdown and removal from the instance graph.
    ///
    /// `Retire` is the **topology projection of the machine lifecycle**
    /// (`Stopping → Stopped → cleanup`): it records the instance's removal
    /// so the runtime can drain, stop, and reclaim it. The machine *type*
    /// keeps existing — future `Spawn`s of the same type are unaffected.
    /// The runtime should drain in-flight messages before removing.
    /// Fails if the machine has active links (drain or unlink first).
    Retire {
        name: &'static str,
    },

    /// Replace a machine with a new instance (hot-swap).
    /// Atomic from the topology's perspective — the new machine takes
    /// over the old one's links. No drain required.
    Replace {
        old_name: &'static str,
        new_name: &'static str,
        machine_type: &'static str,
        physical: MachinePhysicalSpec,
        config_overrides: Vec<(&'static str, String)>,
    },
}

// ── Topology delta ──────────────────────────────────────────────────────────

/// The result of applying a `TopologyOp` — what changed.
#[derive(Debug, Clone, PartialEq)]
pub struct TopologyDelta {
    /// The operation that was applied.
    pub op: AppliedOp,
    /// Sequence number (monotonically increasing).
    pub seq: u64,
    /// Timestamp when the operation was applied (nanoseconds).
    pub timestamp_ns: u64,
}

/// A record of an applied topology operation.
#[derive(Debug, Clone, PartialEq)]
pub enum AppliedOp {
    /// A machine was spawned.
    Spawned { name: &'static str },
    /// A link was created.
    Linked { out: (&'static str, &'static str), into: (&'static str, &'static str) },
    /// A link was removed.
    Unlinked { out: (&'static str, &'static str), into: (&'static str, &'static str) },
    /// A machine was retired.
    Retired { name: &'static str },
    /// A machine was replaced.
    Replaced { old_name: &'static str, new_name: &'static str },
}

// ── Topology error ──────────────────────────────────────────────────────────

/// Errors that can occur during dynamic topology operations.
#[derive(Debug)]
pub enum TopologyError {
    /// A machine with this name already exists.
    DuplicateName(&'static str),
    /// No machine with this name exists.
    UnknownMachine(&'static str),
    /// The specified port doesn't exist on the machine.
    UnknownPort { machine: &'static str, port: &'static str },
    /// A link between these ports already exists.
    LinkExists { out: (&'static str, &'static str), into: (&'static str, &'static str) },
    /// No link between these ports exists (can't unlink).
    LinkNotFound { out: (&'static str, &'static str), into: (&'static str, &'static str) },
    /// The operation would create a cyclic dependency.
    /// Contains the cycle path (machine names) for diagnostics.
    ///
    /// Owned `String`s because cycle nodes are discovered by inspecting the
    /// runtime topology, whose names may come from a deserialized `DynamicTopology`
    /// rather than `&'static str` literals.
    ///
    /// **Note**: `apply_link()` no longer returns this error — cycles between
    /// different machines are allowed. This variant is kept
    /// for `detect_cycle()` (opt-in strict mode) and future strict validation.
    CyclicDependency { cycle: Vec<String> },
    /// The operation would create a self-loop (a machine linking to itself).
    /// Self-loops are rejected even with Moore delay, because they are
    /// degenerate (the machine would need to hold both its own sender and
    /// receiver) and almost always a configuration bug.
    SelfLoop { machine: &'static str },
    /// The machine being retired has active links (drain first).
    MachineHasLinks(&'static str),
    /// A batch operation failed. Contains the index of the failing op
    /// and the error. All prior ops in the batch were rolled back.
    BatchFailed {
        failed_op_index: usize,
        error: Box<TopologyError>,
    },
}

impl core::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateName(n) => write!(f, "duplicate machine name: {}", n),
            Self::UnknownMachine(n) => write!(f, "unknown machine: {}", n),
            Self::UnknownPort { machine, port } => {
                write!(f, "unknown port '{}' on machine '{}'", port, machine)
            }
            Self::LinkExists { out, into } => {
                write!(f, "link already exists: {:?} → {:?}", out, into)
            }
            Self::LinkNotFound { out, into } => {
                write!(f, "link not found: {:?} → {:?}", out, into)
            }
            Self::CyclicDependency { cycle } => {
                write!(f, "cyclic dependency: {}", cycle.join(" → "))
            }
            Self::SelfLoop { machine } => {
                write!(f, "self-loop not allowed: machine '{}' linking to itself", machine)
            }
            Self::MachineHasLinks(n) => write!(f, "machine '{}' has active links", n),
            Self::BatchFailed { failed_op_index, error } => {
                write!(f, "batch failed at op {}: {}", failed_op_index, error)
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for TopologyError {}

// ── Dynamic topology ────────────────────────────────────────────────────────

/// A mutable runtime topology that tracks all spawn/link/unlink/retire
/// operations.
///
/// Wraps a `DynamicTopology`-like structure but allows in-place mutation.
/// Each mutation is recorded as a `TopologyDelta` for audit and replay.
///
/// # Cycle policy
///
/// Cycles between **different** machines are ALLOWED, consistent with
/// `DynamicTopology::validate()`. In a channel-based runtime, every link
/// introduces a one-tick delay (Moore delay), making feedback loops safe.
/// Self-loops (a machine linking to itself) are rejected.
///
/// For opt-in strict acyclic enforcement, call [`crate::topology::TopologyMutation::detect_cycle`] manually
/// after `apply_link()`.
///
/// # Thread safety
///
/// `TopologyMutation` is **not** internally synchronized: it has no `RwLock`
/// and must be owned by a single thread (or guarded externally). Operations
/// take `&mut self` and mutate the machine/link maps in place; `apply`/
/// `apply_batch` are atomic from the caller's perspective (snapshot +
/// rollback on failure). A runtime adapter that shares topology state across
/// threads must wrap it in its own `Mutex`/`RwLock` — the type deliberately
/// stays free of synchronization primitives for `no_std` compatibility.
#[derive(Clone)]
pub struct TopologyMutation {
    /// Current set of machine instances.
    ///
    /// Keys are owned `String`s because a topology may be seeded from a
    /// deserialized `DynamicTopology` (whose names are `Cow<'static, str>`, possibly
    /// owned) and then mutated at runtime. Owned keys keep lookups and removals
    /// simple regardless of where a name originated.
    machines: HashMap<String, MachineInstance>,
    /// Current set of links.
    links: Vec<LinkSpec>,
    /// Applied operations (audit log).
    history: Vec<TopologyDelta>,
    /// Next sequence number.
    next_seq: u64,
}

impl TopologyMutation {
    /// Create an empty topology.
    pub fn new() -> Self {
        Self {
            machines: HashMap::new(),
            links: Vec::new(),
            history: Vec::new(),
            next_seq: 0,
        }
    }

    /// Create a topology from a static `DynamicTopology`.
    /// This is the typical starting point: deploy a static spec, then
    /// mutate it dynamically as needed.
    pub fn from_spec(spec: &DynamicTopology) -> Self {
        let mut topo = Self::new();
        for m in &spec.machines {
            topo.machines.insert(m.name.to_string(), m.clone());
        }
        topo.links = spec.links.clone();
        topo
    }

    /// Apply a single topology operation.
    ///
    /// Validates the operation against the current topology state.
    /// Returns the delta on success, or an error if the operation
    /// would violate an invariant.
    pub fn apply(&mut self, op: TopologyOp) -> Result<TopologyDelta, TopologyError> {
        match op {
            TopologyOp::Spawn { name, machine_type, physical, config_overrides } => {
                self.apply_spawn(name, machine_type, physical, config_overrides)
            }
            TopologyOp::Link { out, into, kind } => {
                self.apply_link(out, into, kind)
            }
            TopologyOp::Unlink { out, into } => {
                self.apply_unlink(out, into)
            }
            TopologyOp::Retire { name } => {
                self.apply_retire(name)
            }
            TopologyOp::Replace { old_name, new_name, machine_type, physical, config_overrides } => {
                self.apply_replace(old_name, new_name, machine_type, physical, config_overrides)
            }
        }
    }

    /// Apply multiple operations atomically.
    ///
    /// Either all operations succeed, or none do (rollback on first failure).
    /// The audit log records all operations from a successful batch as
    /// individual deltas with consecutive sequence numbers.
    ///
    /// # Rollback
    ///
    /// If operation N fails, all operations 0..N are rolled back by
    /// restoring the topology to its pre-batch state. This is implemented
    /// by snapshotting the machines and links before applying the batch.
    pub fn apply_batch(&mut self, ops: Vec<TopologyOp>) -> Result<Vec<TopologyDelta>, TopologyError> {
        // Snapshot for rollback.
        let machines_backup = self.machines.clone();
        let links_backup = self.links.clone();
        let next_seq_backup = self.next_seq;
        let history_len_backup = self.history.len();

        let mut deltas = Vec::with_capacity(ops.len());
        for (i, op) in ops.into_iter().enumerate() {
            match self.apply(op) {
                Ok(delta) => deltas.push(delta),
                Err(error) => {
                    // Rollback.
                    self.machines = machines_backup;
                    self.links = links_backup;
                    self.next_seq = next_seq_backup;
                    self.history.truncate(history_len_backup);
                    return Err(TopologyError::BatchFailed {
                        failed_op_index: i,
                        error: Box::new(error),
                    });
                }
            }
        }
        Ok(deltas)
    }

    /// Current number of machines.
    pub fn machine_count(&self) -> usize {
        self.machines.len()
    }

    /// Current number of links.
    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    /// All applied operations (audit log).
    pub fn history(&self) -> &[TopologyDelta] {
        &self.history
    }

    /// Read-only access to the current link set.
    ///
    /// Used by the LiveRuntime to discover which links involve a machine
    /// being `Replace`d — the topology record has already repointed those
    /// links to the new machine by the time `apply_op` runs, so the
    /// LiveRuntime queries the *post-apply* link set to know which
    /// upstream/downstream channels to re-wire on the running side.
    pub fn links(&self) -> &[LinkSpec] {
        &self.links
    }

    /// Snapshot the current topology as a `DynamicTopology`.
    /// Useful for checkpointing or migrating to a static deployment.
    pub fn snapshot(&self) -> DynamicTopology {
        DynamicTopology {
            machines: self.machines.values().cloned().collect(),
            funcs: Vec::new(),
            links: self.links.clone(),
            settings: crate::deploy::DeploySettings::default(),
        }
    }

    // ── Internal apply methods ──────────────────────────

    fn apply_spawn(
        &mut self,
        name: &'static str,
        machine_type: &'static str,
        physical: MachinePhysicalSpec,
        config_overrides: Vec<(&'static str, String)>,
    ) -> Result<TopologyDelta, TopologyError> {
        if self.machines.contains_key(name) {
            return Err(TopologyError::DuplicateName(name));
        }
        self.machines.insert(name.to_string(), MachineInstance {
            name: name.into(),
            machine_type: machine_type.into(),
            physical,
            config_overrides: config_overrides
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
            is_moore: false,
        });
        self.record(AppliedOp::Spawned { name })
    }

    fn apply_link(
        &mut self,
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
        kind: LinkKind,
    ) -> Result<TopologyDelta, TopologyError> {
        // Check machines exist.
        if !self.machines.contains_key(out.0) {
            return Err(TopologyError::UnknownMachine(out.0));
        }
        if !self.machines.contains_key(into.0) {
            return Err(TopologyError::UnknownMachine(into.0));
        }
        // Self-loop check: a machine linking to itself is always rejected.
        // Even with Moore delay (channel provides one-tick delay), a self-loop
        // is degenerate (the machine holds both its own sender and receiver)
        // and almost always a configuration bug. This aligns with
        // DynamicTopology::validate() which also rejects self-loops.
        if out.0 == into.0 {
            return Err(TopologyError::SelfLoop { machine: out.0 });
        }
        // Check link doesn't already exist.
        if self.links.iter().any(|l| {
            l.out.0.as_ref() == out.0 && l.out.1.as_ref() == out.1
                && l.into.0.as_ref() == into.0 && l.into.1.as_ref() == into.1
        }) {
            return Err(TopologyError::LinkExists { out, into });
        }

        // cycles between DIFFERENT machines are ALLOWED.
        //
        // Previously, apply_link() ran Kahn's algorithm here to reject any
        // cycle. This was inconsistent with DynamicTopology::validate() (which was
        // later changed to allow cycles). In a channel-based runtime, every
        // link introduces a one-tick delay — the Moore delay that breaks
        // algebraic cycles. Feedback loops are first-class supported.
        //
        // Users who need strict acyclic enforcement can call detect_cycle()
        // (pub, below) manually after apply_link().
        self.links.push(LinkSpec::new(out, into, kind));

        self.record(AppliedOp::Linked { out, into })
    }

    fn apply_unlink(
        &mut self,
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
    ) -> Result<TopologyDelta, TopologyError> {
        let idx = self.links.iter().position(|l| {
            l.out.0.as_ref() == out.0 && l.out.1.as_ref() == out.1
                && l.into.0.as_ref() == into.0 && l.into.1.as_ref() == into.1
        });
        match idx {
            Some(i) => {
                self.links.remove(i);
                self.record(AppliedOp::Unlinked { out, into })
            }
            None => Err(TopologyError::LinkNotFound { out, into }),
        }
    }

    fn apply_retire(&mut self, name: &'static str) -> Result<TopologyDelta, TopologyError> {
        if !self.machines.contains_key(name) {
            return Err(TopologyError::UnknownMachine(name));
        }
        // Check no active links.
        let has_links = self.links.iter().any(|l| l.out.0.as_ref() == name || l.into.0.as_ref() == name);
        if has_links {
            return Err(TopologyError::MachineHasLinks(name));
        }
        self.machines.remove(name);
        self.record(AppliedOp::Retired { name })
    }

    fn apply_replace(
        &mut self,
        old_name: &'static str,
        new_name: &'static str,
        machine_type: &'static str,
        physical: MachinePhysicalSpec,
        config_overrides: Vec<(&'static str, String)>,
    ) -> Result<TopologyDelta, TopologyError> {
        if !self.machines.contains_key(old_name) {
            return Err(TopologyError::UnknownMachine(old_name));
        }
        if self.machines.contains_key(new_name) && old_name != new_name {
            return Err(TopologyError::DuplicateName(new_name));
        }
        // Remove old machine, add new one.
        self.machines.remove(old_name);
        // Update links to point to the new machine.
        for link in &mut self.links {
            if link.out.0.as_ref() == old_name {
                link.out.0 = new_name.into();
            }
            if link.into.0.as_ref() == old_name {
                link.into.0 = new_name.into();
            }
        }
        self.machines.insert(new_name.to_string(), MachineInstance {
            name: new_name.into(),
            machine_type: machine_type.into(),
            physical,
            config_overrides: config_overrides
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
            is_moore: false,
        });
        self.record(AppliedOp::Replaced { old_name, new_name })
    }

    fn record(&mut self, op: AppliedOp) -> Result<TopologyDelta, TopologyError> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let delta = TopologyDelta {
            op,
            seq,
            timestamp_ns: 0, // Set by runtime; 0 here for library-only use.
        };
        self.history.push(delta.clone());
        Ok(delta)
    }

    // ── Cycle detection (Kahn's algorithm) — opt-in strict mode ─────────

    /// Detect if the current link graph contains a cycle.
    ///
    /// Uses Kahn's algorithm: compute in-degrees, repeatedly remove
    /// nodes with in-degree 0. If any nodes remain, they form a cycle.
    ///
    /// Returns `Some(cycle_path)` if a cycle exists, `None` otherwise.
    /// The cycle path is a list of machine names (owned, since they may
    /// originate from a deserialized `DynamicTopology` rather than `&'static str`).
    ///
    /// **Note**: This is NOT called by `apply_link()` — cycles between
    /// different machines are allowed. This method is
    /// `pub` for opt-in strict mode: users who need acyclic enforcement
    /// can call it manually after `apply_link()` to detect and reject cycles.
    pub fn detect_cycle(&self) -> Option<Vec<String>> {
        // Build adjacency list and in-degree map (borrows from self).
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut in_degree: HashMap<&str, usize> = HashMap::new();

        for name in self.machines.keys() {
            let name: &str = name.as_str();
            adj.entry(name).or_default();
            in_degree.entry(name).or_insert(0);
        }

        for link in &self.links {
            adj.entry(link.out.0.as_ref()).or_default().push(link.into.0.as_ref());
            *in_degree.entry(link.into.0.as_ref()).or_insert(0) += 1;
        }

        // Kahn's algorithm.
        let mut queue: VecDeque<&str> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(name, _)| *name)
            .collect();

        let mut visited = 0;
        let mut removed: HashSet<&str> = HashSet::new();

        while let Some(node) = queue.pop_front() {
            removed.insert(node);
            visited += 1;
            if let Some(neighbors) = adj.get(node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 && !removed.contains(neighbor) {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        // If visited < total nodes, there's a cycle.
        if visited < self.machines.len() {
            // Collect the cycle nodes (those not removed).
            let cycle_nodes: Vec<String> = self.machines.keys()
                .filter(|name| !removed.contains(name.as_str()))
                .cloned()
                .collect();
            Some(cycle_nodes)
        } else {
            None
        }
    }
}

impl Default for TopologyMutation {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for TopologyMutation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TopologyMutation")
            .field("machine_count", &self.machines.len())
            .field("link_count", &self.links.len())
            .field("history_len", &self.history.len())
            .finish()
    }
}

impl Topology for TopologyMutation {}

// ════════════════════════════════════════════════════════════════════════════
// Note: S1 kept a `pub type DynamicTopology = TopologyMutation;` migration
// alias; after the S2-2 naming convergence, `DynamicTopology` uniformly refers
// to the "runtime projection (declared value form)" — i.e.
// `crate::deploy::DynamicTopology` (formerly `DeploySpec`). The old alias
// collided with the new type's name and was removed; the original "instance
// mutation" concept is always expressed via [`TopologyMutation`].
// ════════════════════════════════════════════════════════════════════════════

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::{DynamicTopology, MachineInstance};
    use crate::link::{LinkKind, LinkSpec};
    use crate::resource::MachinePhysicalSpec;

    // ── helpers ────────────────────────────────────────────────────────

    fn spawn_op(name: &'static str) -> TopologyOp {
        TopologyOp::Spawn {
            name,
            machine_type: "test",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        }
    }

    fn link_op(
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
    ) -> TopologyOp {
        TopologyOp::Link {
            out,
            into,
            kind: LinkKind::Inline,
        }
    }

    fn unlink_op(
        out: (&'static str, &'static str),
        into: (&'static str, &'static str),
    ) -> TopologyOp {
        TopologyOp::Unlink { out, into }
    }

    fn retire_op(name: &'static str) -> TopologyOp {
        TopologyOp::Retire { name }
    }

    fn replace_op(old_name: &'static str, new_name: &'static str) -> TopologyOp {
        TopologyOp::Replace {
            old_name,
            new_name,
            machine_type: "test",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        }
    }

    /// Build a topology with two spawned machines "a" and "b".
    fn topo_with_ab() -> TopologyMutation {
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("a")).unwrap();
        topo.apply(spawn_op("b")).unwrap();
        topo
    }

    // ── apply_spawn ────────────────────────────────────────────────────

    #[test]
    fn apply_spawn_ok() {
        let mut topo = TopologyMutation::new();
        let delta = topo.apply(spawn_op("m1")).unwrap();
        assert_eq!(topo.machine_count(), 1);
        assert_eq!(topo.history().len(), 1);
        assert_eq!(delta.seq, 0);
        assert!(matches!(delta.op, AppliedOp::Spawned { name: "m1" }));
    }

    #[test]
    fn apply_spawn_duplicate() {
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("m1")).unwrap();
        let err = topo.apply(spawn_op("m1")).unwrap_err();
        assert!(matches!(err, TopologyError::DuplicateName("m1")));
        // Failed op must not be recorded.
        assert_eq!(topo.history().len(), 1);
        assert_eq!(topo.machine_count(), 1);
    }

    // ── apply_link ─────────────────────────────────────────────────────

    #[test]
    fn apply_link_ok() {
        let mut topo = topo_with_ab();
        let delta = topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap();
        assert_eq!(topo.link_count(), 1);
        assert!(matches!(delta.op, AppliedOp::Linked { .. }));
    }

    #[test]
    fn apply_link_unknown_src() {
        // Only "b" exists; "ghost" is the unknown source.
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("b")).unwrap();
        let err = topo.apply(link_op(("ghost", "out"), ("b", "in"))).unwrap_err();
        assert!(matches!(err, TopologyError::UnknownMachine("ghost")));
    }

    #[test]
    fn apply_link_self_loop() {
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("a")).unwrap();
        let err = topo.apply(link_op(("a", "x"), ("a", "y"))).unwrap_err();
        assert!(matches!(err, TopologyError::SelfLoop { machine: "a" }));
    }

    #[test]
    fn apply_link_duplicate() {
        let mut topo = topo_with_ab();
        topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap();
        let err = topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap_err();
        assert!(matches!(err, TopologyError::LinkExists { .. }));
        assert_eq!(topo.link_count(), 1);
    }

    // ── apply_unlink ───────────────────────────────────────────────────

    #[test]
    fn apply_unlink_ok() {
        let mut topo = topo_with_ab();
        topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap();
        assert_eq!(topo.link_count(), 1);
        let delta = topo
            .apply(unlink_op(("a", "out"), ("b", "in")))
            .unwrap();
        assert_eq!(topo.link_count(), 0);
        assert!(matches!(delta.op, AppliedOp::Unlinked { .. }));
    }

    #[test]
    fn apply_unlink_not_found() {
        let mut topo = topo_with_ab();
        let err = topo
            .apply(unlink_op(("a", "out"), ("b", "in")))
            .unwrap_err();
        assert!(matches!(err, TopologyError::LinkNotFound { .. }));
    }

    // ── apply_retire ───────────────────────────────────────────────────

    #[test]
    fn apply_retire_ok() {
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("a")).unwrap();
        assert_eq!(topo.machine_count(), 1);
        let delta = topo.apply(retire_op("a")).unwrap();
        assert_eq!(topo.machine_count(), 0);
        assert!(matches!(delta.op, AppliedOp::Retired { name: "a" }));
    }

    #[test]
    fn apply_retire_unknown() {
        let mut topo = TopologyMutation::new();
        let err = topo.apply(retire_op("ghost")).unwrap_err();
        assert!(matches!(err, TopologyError::UnknownMachine("ghost")));
    }

    #[test]
    fn apply_retire_has_links() {
        let mut topo = topo_with_ab();
        topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap();
        let err = topo.apply(retire_op("a")).unwrap_err();
        assert!(matches!(err, TopologyError::MachineHasLinks("a")));
        // Machine must still be present (operation rejected).
        assert_eq!(topo.machine_count(), 2);
    }

    // ── apply_replace ──────────────────────────────────────────────────

    #[test]
    fn apply_replace_ok() {
        let mut topo = topo_with_ab();
        topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap();
        let delta = topo.apply(replace_op("a", "c")).unwrap();
        assert!(matches!(
            delta.op,
            AppliedOp::Replaced { old_name: "a", new_name: "c" }
        ));
        // "a" removed, "c" inserted → count unchanged.
        assert_eq!(topo.machine_count(), 2);
        // Link endpoints rewritten a → c (out side); b stays (into side).
        assert_eq!(topo.links().len(), 1);
        assert_eq!(topo.links()[0].out.0.as_ref(), "c");
        assert_eq!(topo.links()[0].into.0.as_ref(), "b");
    }

    #[test]
    fn apply_replace_unknown_old() {
        let mut topo = TopologyMutation::new();
        let err = topo.apply(replace_op("ghost", "c")).unwrap_err();
        assert!(matches!(err, TopologyError::UnknownMachine("ghost")));
    }

    #[test]
    fn apply_replace_duplicate_new() {
        let mut topo = topo_with_ab();
        let err = topo.apply(replace_op("a", "b")).unwrap_err();
        assert!(matches!(err, TopologyError::DuplicateName("b")));
    }

    #[test]
    fn apply_replace_same_name() {
        // new_name == old_name is allowed (in-place hot-swap).
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("a")).unwrap();
        let delta = topo.apply(replace_op("a", "a")).unwrap();
        assert!(matches!(
            delta.op,
            AppliedOp::Replaced { old_name: "a", new_name: "a" }
        ));
        assert_eq!(topo.machine_count(), 1);
    }

    // ── apply_batch ────────────────────────────────────────────────────

    #[test]
    fn apply_batch_all_ok() {
        let mut topo = TopologyMutation::new();
        let deltas = topo
            .apply_batch(vec![
                spawn_op("a"),
                spawn_op("b"),
                link_op(("a", "out"), ("b", "in")),
            ])
            .unwrap();
        assert_eq!(deltas.len(), 3);
        // Consecutive sequence numbers.
        assert_eq!(deltas[0].seq, 0);
        assert_eq!(deltas[1].seq, 1);
        assert_eq!(deltas[2].seq, 2);
        assert_eq!(topo.machine_count(), 2);
        assert_eq!(topo.link_count(), 1);
        assert_eq!(topo.history().len(), 3);
    }

    #[test]
    fn apply_batch_rollback_on_failure() {
        let mut topo = TopologyMutation::new();
        let err = topo
            .apply_batch(vec![spawn_op("a"), spawn_op("a")])
            .unwrap_err();
        match err {
            TopologyError::BatchFailed { failed_op_index, error } => {
                assert_eq!(failed_op_index, 1);
                assert!(matches!(*error, TopologyError::DuplicateName("a")));
            }
            other => panic!("expected BatchFailed, got {:?}", other),
        }
        // First successful Spawn must also be rolled back.
        assert_eq!(topo.machine_count(), 0);
        assert_eq!(topo.link_count(), 0);
        assert_eq!(topo.history().len(), 0);
    }

    #[test]
    fn apply_batch_partial_then_fail() {
        let mut topo = TopologyMutation::new();
        let err = topo
            .apply_batch(vec![
                spawn_op("a"),
                spawn_op("b"),
                spawn_op("b"), // duplicate → fails
            ])
            .unwrap_err();
        match err {
            TopologyError::BatchFailed { failed_op_index, error } => {
                assert_eq!(failed_op_index, 2);
                assert!(matches!(*error, TopologyError::DuplicateName("b")));
            }
            other => panic!("expected BatchFailed, got {:?}", other),
        }
        // Both prior Spawns rolled back.
        assert_eq!(topo.machine_count(), 0);
        assert_eq!(topo.link_count(), 0);
        assert_eq!(topo.history().len(), 0);
    }

    // ── detect_cycle ───────────────────────────────────────────────────

    #[test]
    fn detect_cycle_acyclic() {
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("a")).unwrap();
        topo.apply(spawn_op("b")).unwrap();
        topo.apply(spawn_op("c")).unwrap();
        topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap();
        topo.apply(link_op(("b", "out"), ("c", "in"))).unwrap();
        assert!(topo.detect_cycle().is_none());
    }

    #[test]
    fn detect_cycle_cyclic() {
        let mut topo = TopologyMutation::new();
        topo.apply(spawn_op("a")).unwrap();
        topo.apply(spawn_op("b")).unwrap();
        topo.apply(link_op(("a", "out"), ("b", "in"))).unwrap();
        topo.apply(link_op(("b", "out"), ("a", "in"))).unwrap();
        let cycle = topo.detect_cycle().expect("cycle should be detected");
        // Order is unspecified by Kahn's algorithm — check membership.
        assert!(cycle.contains(&"a".to_string()));
        assert!(cycle.contains(&"b".to_string()));
    }

    #[test]
    fn detect_cycle_empty() {
        let topo = TopologyMutation::new();
        assert!(topo.detect_cycle().is_none());
    }

    // ── from_spec / snapshot roundtrip ─────────────────────────────────

    #[test]
    fn from_spec_snapshot_roundtrip() {
        let spec = DynamicTopology::new()
            .with_machine(MachineInstance::new(
                "a",
                "test",
                MachinePhysicalSpec::default(),
            ))
            .with_machine(MachineInstance::new(
                "b",
                "test",
                MachinePhysicalSpec::default(),
            ))
            .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline));
        let topo = TopologyMutation::from_spec(&spec);
        assert_eq!(topo.machine_count(), 2);
        assert_eq!(topo.link_count(), 1);
        let snap = topo.snapshot();
        assert_eq!(snap.machines.len(), 2);
        assert_eq!(snap.links.len(), 1);
        // Link round-trips exactly (LinkSpec: PartialEq).
        assert_eq!(
            snap.links[0],
            LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline)
        );
        // Machine names round-trip (MachineInstance has no PartialEq).
        let names: Vec<String> =
            snap.machines.iter().map(|m| m.name.to_string()).collect();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }
}


