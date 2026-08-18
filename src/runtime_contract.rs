//! **Maturity: tool** (development-time tool / runtime-adapter constraints, reinforced per the unified convention).
//!
//! Runtime capability contracts — the guardrail for runtime adapters.
//!
//! axiom is an abstraction layer: a `DynamicTopology` is pure structure, and any
//! runtime adapter (the built-in `axiom-runtime`, a future `axiom_tokio`,
//! `axiom_io_uring`, `axiom_wasi`, an embedded bare-metal executor, …)
//! interprets it with its own physics. The **constraint side** of "provide a
//! runtime for extension **or constraint**" has been empty: an adapter could
//! silently not support a `LinkKind` or an `ExecutionHint` and fail at
//! runtime.
//!
//! This module closes that gap:
//!
//! - [`RuntimeContract`] — what a runtime adapter **declares** it can do:
//!   which [`LinkKind`]s it supports (with per-kind capability details),
//!   which execution modes, its memory-order guarantee, its IO capabilities,
//!   and its physical budget (threads / memory / alignment).
//! - [`Guarantees`] — the declarative capability struct every adapter
//!   returns from [`RuntimeContract::guarantees`].
//! - [`RuntimeContract::check_spec`] — default implementation: verifies a
//!   `DynamicTopology` against the declared guarantees and returns a structured
//!   [`ValidationReport`] (reusing `RuleViolation`). A blueprint that needs a
//!   carrier or execution mode the runtime does not provide is rejected
//!   *before* deployment, with `rule_id`-tagged violations the AI loop can
//!   fix.
//!
//! ## Contract position
//!
//! The guarantees are **pure declarations** — they add no physics themselves.
//! They are the physical side of the abstraction made explicit at the
//! interface: the blueprint says *what structure*, the runtime says *what it
//! can physically honor*, and `check_spec` is the pure function that decides
//! whether they agree.

use crate::backpressure::BackpressureAction;
use crate::compat::HashMap;
use crate::deploy::{DynamicTopology, RuleViolation, ValidationReport};
use crate::link::{
    LinkKind, LinkSpec, MemoryRegion, ReadPolicy, WritePolicy,
};
use crate::port::PortSchema;
use crate::resource::{CpuAffinity, ExecutionHint, HugePages};

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::vec;
use alloc::borrow::Cow;
use alloc::vec::Vec;

/// Per-kind capability for `BoundedBuf` links.
#[derive(Debug, Clone)]
pub struct BoundedBufSupport {
    /// Which write policies the runtime implements.
    pub write_policies: Vec<WritePolicy>,
    /// Which read policies the runtime implements.
    pub read_policies: Vec<ReadPolicy>,
    /// Maximum `capacity` accepted (0 = no limit).
    pub max_capacity: usize,
}

/// Per-kind capability for `Channel` links.
#[derive(Debug, Clone)]
pub struct ChannelSupport {
    /// Whether `drop_when_full = true` is honored (fire-and-forget).
    pub drop_when_full: bool,
    /// Maximum `capacity` accepted (0 = no limit).
    pub max_capacity: usize,
}

/// Per-kind capability for `CasFreeRing` links.
#[derive(Debug, Clone)]
pub struct CasFreeRingSupport {
    /// Supports `MemoryRegion::Heap` storage.
    pub heap: bool,
    /// Supports `MemoryRegion::Static` storage (fixed address).
    pub static_region: bool,
}

/// Which backpressure actions the runtime can execute on its carriers
/// (S3-3 — policy↔carrier correspondence).
///
/// Each runtime declares which [`BackpressureAction`]s its carriers
/// natively support. A `BackpressurePolicy` that `required_action`s
/// `Block` may only be wired onto a link whose runtime declares
/// `block: true`; otherwise the policy would silently fail to deliver
/// its declared semantics.
#[derive(Debug, Clone, Default)]
pub struct BackpressureActionSupport {
    /// Runtime can block the sender thread (e.g. `SyncSender::send`).
    pub block: bool,
    /// Runtime can drop the message when full (e.g. `try_send` + abandon).
    pub drop: bool,
    /// Runtime can evict the oldest and deliver the newest (overwrite
    /// carrier, e.g. ring-buffer with wrap-around).
    pub overwrite: bool,
    /// Runtime can defer the send and re-schedule the machine when
    /// credits replenish (credit-based flow control).
    pub defer: bool,
}

/// Which `LinkKind` variants the runtime implements, with capability details.
#[derive(Debug, Clone, Default)]
pub struct LinkKindSupport {
    pub inline: bool,
    pub bounded_buf: Option<BoundedBufSupport>,
    pub channel: Option<ChannelSupport>,
    pub latest: bool,
    pub cas_free_ring: Option<CasFreeRingSupport>,
    pub shared_state: bool,
}

/// Which execution modes the runtime implements.
#[derive(Debug, Clone, Default)]
pub struct ExecModeSupport {
    /// Single-threaded sequential drive (the reference mode).
    pub sequential: bool,
    /// One OS thread per machine (parallel drive).
    pub thread_per_machine: bool,
    /// Async cooperative multitasking (Tokio/Embassy-style).
    pub async_cooperative: bool,
    /// Subprocess isolation.
    pub subprocess: bool,
    /// Maximum usable parallelism (0 = unlimited).
    pub max_parallelism: usize,
}

/// Memory-order guarantee the runtime provides across links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryOrder {
    /// Every link transfer is a full sequential-consistency synchronization
    /// point (the strongest guarantee; typical of channel runtimes).
    #[default]
    SeqCst,
    /// Synchronization exists but is weak/relaxed (lock-free carriers).
    Relaxed,
    /// No cross-link memory-order guarantee (single-threaded runtimes only).
    None,
}

/// IO capability of the runtime.
#[derive(Debug, Clone, Default)]
pub struct IoCapability {
    /// Event loop / readiness reactor (epoll, kqueue, WSAEventSelect, …).
    pub event_loop: bool,
    /// io_uring completion-based IO.
    pub io_uring: bool,
    /// Non-blocking IO support at all.
    pub non_blocking: bool,
}

/// Whether a link introduces a one-tick delay.
///
/// This is load-bearing for cycle safety: a channel-based runtime delays
/// every link by one tick (the channel **is** the delay element), so a cycle
/// is safe without Moore machines. An Inline/fused runtime has zero delay, so
/// every cycle must pass through ≥1 Moore machine. `check_spec` uses this to
/// decide whether the Moore requirement applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkDelay {
    /// Inline/fused links have no buffering — Moore machines required on cycles.
    Zero,
    /// Every link carries a one-tick delay — cycles are safe regardless.
    #[default]
    OneTick,
}

/// Physical budget the runtime can honor (B2 — physical budget contract).
///
/// Zero values mean "no limit" (or "not supported" for boolean fields).
#[derive(Debug, Clone, Default)]
pub struct PhysicalBudget {
    /// Maximum total dedicated threads across all machines (0 = unlimited).
    pub max_threads: usize,
    /// Maximum `state_heap_bytes` per machine (0 = unlimited).
    pub max_state_bytes: usize,
    /// Runtime can honor `cache_line_align` requests.
    pub cache_line_align: bool,
    /// Maximum `max_cleanup_latency_us` accepted (0 = unlimited).
    pub max_cleanup_latency_us: u64,
    /// Runtime can pin a machine to specific cores (`CpuAffinity::Allowed`).
    pub cpu_affinity: bool,
    /// Runtime can guarantee exclusive core ownership (`CpuAffinity::Exclusive`).
    pub cpu_exclusive: bool,
    /// Runtime can place machine memory on a requested NUMA node.
    pub numa: bool,
    /// Runtime can allocate huge pages for machine working memory.
    pub huge_pages: bool,
    /// SIMD instruction sets the runtime's build target provides
    /// (LLVM `target_feature` names, e.g. `"avx2"`, `"sse4.2"`, `"neon"`).
    pub simd_features: Vec<Cow<'static, str>>,
}

/// Everything a runtime adapter declares it guarantees.
#[derive(Debug, Clone, Default)]
pub struct Guarantees {
    /// Supported link kinds with per-kind capability details.
    pub link_kinds: LinkKindSupport,
    /// Supported execution modes.
    pub exec_modes: ExecModeSupport,
    /// Memory-order guarantee across links.
    pub memory_order: MemoryOrder,
    /// IO capability.
    pub io: IoCapability,
    /// Whether links introduce delay (drives the Moore requirement).
    pub link_delay: LinkDelay,
    /// Runtime supports deterministic replay (event sourcing).
    pub deterministic_replay: bool,
    /// Physical budget the runtime can honor.
    pub physical: PhysicalBudget,
    /// Which backpressure actions the runtime's carriers can execute.
    pub backpressure: BackpressureActionSupport,
}

impl Guarantees {
    /// Whether a concrete backpressure policy can be wired onto this
    /// runtime's carriers — i.e. the action [`BackpressurePolicy::required_action`]
    /// demands is executable (S3-3). `true` if the runtime declares the
    /// action, `false` if it would silently violate the policy.
    pub fn backpressure_supported(
        &self,
        policy: &dyn crate::backpressure::BackpressurePolicy,
    ) -> bool {
        self.backpressure.supports(policy.required_action())
    }
}

impl BackpressureActionSupport {
    /// Whether an action can be executed on the runtime's carriers.
    ///
    /// `Proceed` (a plain successful send) is always executable — every
    /// carrier can do a non-blocking send; the other actions require the
    /// corresponding declared capability.
    pub fn supports(&self, action: BackpressureAction) -> bool {
        match action {
            BackpressureAction::Proceed => true,
            BackpressureAction::Block => self.block,
            BackpressureAction::Drop => self.drop,
            BackpressureAction::Overwrite => self.overwrite,
            BackpressureAction::Defer => self.defer,
        }
    }
}

/// A runtime adapter's capability contract.
///
/// Implementors declare what they support via [`guarantees`](Self::guarantees);
/// the framework derives [`check_spec`](Self::check_spec) — a pure
/// blueprint-vs-capability audit. An adapter that lies about its guarantees
/// is a bug in the adapter, not in axiom.
pub trait RuntimeContract {
    /// Stable identifier of the runtime (e.g. `"axiom-runtime/sequential"`).
    fn id(&self) -> &'static str;

    /// The declared capabilities.
    fn guarantees(&self) -> Guarantees;

    /// Verify a `DynamicTopology` against the declared guarantees.
    ///
    /// Default implementation checks, per link: kind support, per-kind policy
    /// support, capacity limits, and **backpressure-action support** (the
    /// carrier must be able to execute the action the link's declared policy
    /// demands). Per machine: execution-mode support, physical budget
    /// (memory / alignment / cleanup latency / threads, plus the deep budget —
    /// CPU affinity, NUMA placement, huge pages, SIMD features). Plus:
    /// Moore/cycle requirement (only when `link_delay == Zero`) and
    /// total-thread budget.
    fn check_spec(
        &self,
        spec: &DynamicTopology,
        schemas: &HashMap<&str, PortSchema>,
    ) -> ValidationReport {
        let g = self.guarantees();
        let mut report = ValidationReport::default();
        // `schemas` is reserved for port-level capability checks in adapters
        // that override `check_spec`; the default implementation keeps it as
        // a stable extension point.
        let _ = schemas;

        // 1. Per-link carrier support.
        for (i, link) in spec.links.iter().enumerate() {
            check_link_kind(&mut report, i, link, &g);
        }

        // 1b. Backpressure policy ↔ carrier correspondence (S3-3): each
        // link's declared policy (or its carrier's built-in semantics) demands
        // an action when full; the runtime must declare it can execute that
        // action on its carriers.
        for (i, link) in spec.links.iter().enumerate() {
            if let Some(action) = link_required_backpressure_action(&link.kind) {
                if !g.backpressure.supports(action) {
                    report.push(RuleViolation::new(
                        "runtime-backpressure-action",
                        format!("links[{i}].kind"),
                        format!("backpressure action {action:?} supported by runtime"),
                        format!(
                            "link kind {} demands {action:?}, runtime unsupported",
                            link.kind.name()
                        ),
                    ));
                }
            }
        }

        // 2. Per-machine execution-mode support + physical budget.
        let mut total_threads: usize = 0;
        for (i, m) in spec.machines.iter().enumerate() {
            let threads = thread_count(&m.physical.execution);
            total_threads += threads;

            let supported = match &m.physical.execution {
                ExecutionHint::Async => g.exec_modes.async_cooperative,
                ExecutionHint::CpuBound
                | ExecutionHint::CpuBoundN(_)
                | ExecutionHint::ThreadPool(_) => g.exec_modes.thread_per_machine,
                ExecutionHint::Subprocess(_) => g.exec_modes.subprocess,
            };
            if !supported {
                report.push(RuleViolation::new(
                    "runtime-exec-mode",
                    format!("machines[{i}].physical.execution"),
                    format!("runtime '{}' supports {:?}", self.id(), m.physical.execution),
                    "declared unsupported",
                ));
            }

            // Physical budget (B2).
            let b = &g.physical;
            if b.max_state_bytes > 0 && m.physical.state_heap_bytes > b.max_state_bytes {
                report.push(RuleViolation::new(
                    "runtime-resource-memory",
                    format!("machines[{i}].physical.state_heap_bytes"),
                    format!("state ≤ {} bytes (runtime budget)", b.max_state_bytes),
                    format!("{} bytes declared", m.physical.state_heap_bytes),
                ));
            }
            if !b.cache_line_align && m.physical.cache_line_align {
                report.push(RuleViolation::new(
                    "runtime-resource-align",
                    format!("machines[{i}].physical.cache_line_align"),
                    "cache-line alignment requested",
                    "runtime does not honor cache_line_align",
                ));
            }
            if b.max_cleanup_latency_us > 0
                && m.physical.max_cleanup_latency_us > b.max_cleanup_latency_us
            {
                report.push(RuleViolation::new(
                    "runtime-resource-cleanup",
                    format!("machines[{i}].physical.max_cleanup_latency_us"),
                    format!("cleanup ≤ {}µs (runtime budget)", b.max_cleanup_latency_us),
                    format!("{}µs declared", m.physical.max_cleanup_latency_us),
                ));
            }

            // Deep physical budget (B2): CPU affinity / NUMA / huge pages /
            // SIMD. A blueprint that declares a physical requirement the
            // runtime cannot honor is rejected *before* deployment — the
            // "exclusive core + huge pages" deployment scenario must fail
            // loudly here rather than degrade silently at runtime.
            if !b.cpu_affinity && !matches!(m.physical.cpu_affinity, CpuAffinity::None) {
                report.push(RuleViolation::new(
                    "runtime-resource-affinity",
                    format!("machines[{i}].physical.cpu_affinity"),
                    "CPU core affinity supported by runtime",
                    format!(
                        "{:?} requested, runtime does not pin cores",
                        m.physical.cpu_affinity
                    ),
                ));
            }
            if !b.cpu_exclusive && matches!(m.physical.cpu_affinity, CpuAffinity::Exclusive(_)) {
                report.push(RuleViolation::new(
                    "runtime-resource-affinity-exclusive",
                    format!("machines[{i}].physical.cpu_affinity"),
                    "exclusive core ownership supported by runtime",
                    "CpuAffinity::Exclusive requested, runtime cannot guarantee exclusive cores",
                ));
            }
            if !b.numa && m.physical.numa_node.is_some() {
                report.push(RuleViolation::new(
                    "runtime-resource-numa",
                    format!("machines[{i}].physical.numa_node"),
                    "NUMA placement supported by runtime",
                    format!(
                        "node {:?} requested, runtime does not place by NUMA",
                        m.physical.numa_node
                    ),
                ));
            }
            if !b.huge_pages && m.physical.huge_pages != HugePages::None {
                report.push(RuleViolation::new(
                    "runtime-resource-hugepages",
                    format!("machines[{i}].physical.huge_pages"),
                    "huge-page allocation supported by runtime",
                    format!(
                        "{:?} requested, runtime cannot allocate huge pages",
                        m.physical.huge_pages
                    ),
                ));
            }
            if let Some(simd) = &m.physical.simd {
                for feat in &simd.features {
                    if !b.simd_features.iter().any(|s| s == feat) {
                        report.push(RuleViolation::new(
                            "runtime-resource-simd",
                            format!("machines[{i}].physical.simd"),
                            format!("SIMD feature '{feat}' supported by runtime"),
                            "declared unsupported",
                        ));
                    }
                }
            }
        }

        // 3. Total-thread budget.
        if g.physical.max_threads > 0 && total_threads > g.physical.max_threads {
            report.push(RuleViolation::new(
                "runtime-resource-threads",
                "machines[].physical.execution",
                format!("≤ {} dedicated threads (runtime budget)", g.physical.max_threads),
                format!("{total_threads} declared"),
            ));
        }
        if g.exec_modes.max_parallelism > 0 && total_threads > g.exec_modes.max_parallelism {
            report.push(RuleViolation::new(
                "runtime-resource-threads",
                "machines[].physical.execution",
                format!("≤ {} threads (runtime max parallelism)", g.exec_modes.max_parallelism),
                format!("{total_threads} declared"),
            ));
        }

        // 4. Moore/cycle requirement — only when links have no delay.
        if g.link_delay == LinkDelay::Zero && !spec.machines.is_empty() {
            // Validate the Moore invariant directly: a cycle with no Moore
            // machine is an algebraic loop in a zero-delay runtime.
            // Reuses deploy's `find_non_moore_cycle`: a cycle in the subgraph
            // induced by non-Moore machines. This is exact (not the previous
            // conservative "any cycle ∧ no Moore at all" approximation, which
            // missed two-cycle specs where one cycle had a Moore machine).
            if spec.find_non_moore_cycle().is_some() {
                report.push(RuleViolation::new(
                    "runtime-cycle-moore",
                    "links",
                    format!(
                        "runtime '{}' links have zero delay — every cycle needs ≥1 Moore machine",
                        self.id()
                    ),
                    "cycle without Moore machine in zero-delay runtime",
                ));
            }
        }

        report
    }
}

// ── Per-link kind checks ───────────────────────────────────────────────────────

fn check_link_kind(report: &mut ValidationReport, i: usize, link: &LinkSpec, g: &Guarantees) {
    let kinds = &g.link_kinds;
    match &link.kind {
        LinkKind::Inline => {
            if !kinds.inline {
                push_unsupported(report, i, "Inline");
            }
        }
        LinkKind::BoundedBuf {
            capacity,
            write_policy,
            read_policy,
        } => match &kinds.bounded_buf {
            Some(sup) => {
                if sup.max_capacity > 0 && *capacity > sup.max_capacity {
                    report.push(RuleViolation::new(
                        "runtime-link-capacity",
                        format!("links[{i}].kind"),
                        format!("capacity ≤ {} (runtime limit)", sup.max_capacity),
                        format!("capacity {capacity}"),
                    ));
                }
                if !sup.write_policies.contains(write_policy) {
                    report.push(RuleViolation::new(
                        "runtime-link-policy",
                        format!("links[{i}].kind"),
                        "BoundedBuf write policy supported by runtime",
                        format!("write policy {:?} unsupported", write_policy),
                    ));
                }
                if !sup.read_policies.contains(read_policy) {
                    report.push(RuleViolation::new(
                        "runtime-link-policy",
                        format!("links[{i}].kind"),
                        "BoundedBuf read policy supported by runtime",
                        format!("read policy {:?} unsupported", read_policy),
                    ));
                }
            }
            None => push_unsupported(report, i, "BoundedBuf"),
        },
        LinkKind::Channel {
            capacity,
            drop_when_full,
        } => match &kinds.channel {
            Some(sup) => {
                if sup.max_capacity > 0 && *capacity > sup.max_capacity {
                    report.push(RuleViolation::new(
                        "runtime-link-capacity",
                        format!("links[{i}].kind"),
                        format!("capacity ≤ {} (runtime limit)", sup.max_capacity),
                        format!("capacity {capacity}"),
                    ));
                }
                if *drop_when_full && !sup.drop_when_full {
                    report.push(RuleViolation::new(
                        "runtime-link-policy",
                        format!("links[{i}].kind"),
                        "Channel drop_when_full supported by runtime",
                        "drop_when_full=true unsupported",
                    ));
                }
            }
            None => push_unsupported(report, i, "Channel"),
        },
        LinkKind::Latest { .. } => {
            if !kinds.latest {
                push_unsupported(report, i, "Latest");
            }
        }
        LinkKind::CasFreeRing { storage, .. } => match &kinds.cas_free_ring {
            Some(sup) => match storage {
                MemoryRegion::Heap { .. } => {
                    if !sup.heap {
                        push_unsupported(report, i, "CasFreeRing/Heap");
                    }
                }
                MemoryRegion::Static { .. } => {
                    if !sup.static_region {
                        push_unsupported(report, i, "CasFreeRing/Static");
                    }
                }
            },
            None => push_unsupported(report, i, "CasFreeRing"),
        },
        LinkKind::SharedState => {
            if !kinds.shared_state {
                push_unsupported(report, i, "SharedState");
            }
        }
    }
}

fn push_unsupported(report: &mut ValidationReport, i: usize, kind: &str) {
    report.push(RuleViolation::new(
        "runtime-link-kind",
        format!("links[{i}].kind"),
        format!("link kind {kind} supported by runtime"),
        "declared unsupported",
    ));
}

/// The backpressure action a link's declared policy demands when its
/// carrier is full — the carrier-side contract (S3-3).
///
/// Links with no queued backpressure semantics (`Inline` direct call /
/// `SharedState` shared memory) return `None` — nothing to validate.
fn link_required_backpressure_action(kind: &LinkKind) -> Option<BackpressureAction> {
    match kind {
        LinkKind::Inline | LinkKind::SharedState => None,
        LinkKind::BoundedBuf { write_policy, .. } => Some(match write_policy {
            WritePolicy::Blocking => BackpressureAction::Block,
            WritePolicy::Dropping => BackpressureAction::Drop,
            WritePolicy::Overwriting => BackpressureAction::Overwrite,
        }),
        LinkKind::Channel { drop_when_full, .. } => Some(if *drop_when_full {
            // Fire-and-forget: drop the message when full.
            BackpressureAction::Drop
        } else {
            // Natural backpressure: block the sender.
            BackpressureAction::Block
        }),
        // A single overwrite slot = latest-wins = evict-oldest semantics.
        LinkKind::Latest { .. } => Some(BackpressureAction::Overwrite),
        // SPSC ring: bounded capacity, sender blocks when full.
        LinkKind::CasFreeRing { .. } => Some(BackpressureAction::Block),
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn thread_count(e: &ExecutionHint) -> usize {
    match e {
        ExecutionHint::CpuBound => 1,
        ExecutionHint::CpuBoundN(n) => *n,
        ExecutionHint::ThreadPool(spec) => spec.max_threads,
        ExecutionHint::Async | ExecutionHint::Subprocess(_) => 0,
    }
}

// ── Reference runtime ──────────────────────────────────────────────────────────

/// Reference runtime declaration matching the built-in `axiom-runtime`
/// (channel-based, sequential or thread-per-machine, one-tick link delay).
///
/// Use as a model for third-party adapters (`axiom_tokio`, `axiom_io_uring`,
/// …). Core cannot depend on the runtime crate, so this is a **declaration**,
/// not the runtime itself.
pub struct ReferenceRuntime;

impl RuntimeContract for ReferenceRuntime {
    fn id(&self) -> &'static str {
        "axiom-runtime/reference"
    }

    fn guarantees(&self) -> Guarantees {
        Guarantees {
            link_kinds: LinkKindSupport {
                inline: true,
                bounded_buf: Some(BoundedBufSupport {
                    write_policies: vec![
                        WritePolicy::Blocking,
                        WritePolicy::Dropping,
                        WritePolicy::Overwriting,
                    ],
                    read_policies: vec![ReadPolicy::Blocking, ReadPolicy::NonBlocking],
                    max_capacity: 0,
                }),
                channel: Some(ChannelSupport {
                    drop_when_full: true,
                    max_capacity: 0,
                }),
                latest: true,
                cas_free_ring: Some(CasFreeRingSupport {
                    heap: true,
                    static_region: false,
                }),
                shared_state: true,
            },
            exec_modes: ExecModeSupport {
                sequential: true,
                thread_per_machine: true,
                async_cooperative: true,
                subprocess: false,
                max_parallelism: 0,
            },
            memory_order: MemoryOrder::SeqCst,
            io: IoCapability {
                event_loop: true,
                io_uring: false,
                non_blocking: true,
            },
            link_delay: LinkDelay::OneTick,
            deterministic_replay: true,
            physical: PhysicalBudget {
                max_threads: 0,
                max_state_bytes: 0,
                cache_line_align: false,
                max_cleanup_latency_us: 0,
                // The reference runtime drives machines cooperatively without
                // core pinning, NUMA placement, huge-page allocation, or
                // target-feature dispatch — honestly declared unsupported so
                // a blueprint demanding them is rejected before deployment.
                cpu_affinity: false,
                cpu_exclusive: false,
                numa: false,
                huge_pages: false,
                simd_features: vec![],
            },
            // Built-in carrier validation (see runtime/carrier.rs): BoundedBlocking
            // supports Block, BoundedDropping/Channel(drop) supports Drop,
            // BoundedOverwriting supports Overwrite. CreditPolicy's Defer
            // (rescheduling + on_consumed credit replenishment) is not yet wired
            // in — honestly declared as unsupported.
            backpressure: BackpressureActionSupport {
                block: true,
                drop: true,
                overwrite: true,
                defer: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backpressure::{
        BackpressurePolicy, BlockPolicy, CreditPolicy, DropPolicy, OverwritePolicy,
    };
    use crate::deploy::{DynamicTopology, MachineInstance};
    use crate::port::PortDecl;
    use crate::resource::{CpuAffinity, HugePages, MachinePhysicalSpec, SimdRequirement};

    fn schema_io() -> HashMap<&'static str, PortSchema> {
        let s = PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::input::<i32>("in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", s.clone());
        schemas.insert("b", s.clone());
        schemas
    }

    fn spec_ab(link: LinkKind) -> DynamicTopology {
        DynamicTopology::new()
            .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()))
            .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
            .with_link(crate::link::LinkSpec::new(("a", "out"), ("b", "in"), link))
    }

    #[test]
    fn reference_runtime_accepts_full_spec() {
        let spec = spec_ab(LinkKind::BoundedBuf {
            capacity: 16,
            write_policy: WritePolicy::Blocking,
            read_policy: ReadPolicy::NonBlocking,
        });
        let rt = ReferenceRuntime;
        let report = rt.check_spec(&spec, &schema_io());
        assert!(report.is_ok(), "{:?}", report.violations);
    }

    #[test]
    fn rejects_unsupported_link_kind() {
        // A runtime that only supports Channel + Inline must reject BoundedBuf.
        struct ChannelOnly;
        impl RuntimeContract for ChannelOnly {
            fn id(&self) -> &'static str {
                "test/channel-only"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        channel: Some(ChannelSupport {
                            drop_when_full: true,
                            max_capacity: 0,
                        }),
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    ..Guarantees::default()
                }
            }
        }
        let spec = spec_ab(LinkKind::BoundedBuf {
            capacity: 4,
            write_policy: WritePolicy::Blocking,
            read_policy: ReadPolicy::Blocking,
        });
        let report = ChannelOnly.check_spec(&spec, &schema_io());
        assert!(!report.is_ok());
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-link-kind"));
    }

    #[test]
    fn rejects_unsupported_write_policy() {
        struct BlockingOnly;
        impl RuntimeContract for BlockingOnly {
            fn id(&self) -> &'static str {
                "test/blocking-only"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        bounded_buf: Some(BoundedBufSupport {
                            write_policies: vec![WritePolicy::Blocking],
                            read_policies: vec![ReadPolicy::Blocking],
                            max_capacity: 0,
                        }),
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    ..Guarantees::default()
                }
            }
        }
        let spec = spec_ab(LinkKind::BoundedBuf {
            capacity: 4,
            write_policy: WritePolicy::Dropping,
            read_policy: ReadPolicy::Blocking,
        });
        let report = BlockingOnly.check_spec(&spec, &schema_io());
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-link-policy"));
    }

    #[test]
    fn rejects_unsupported_exec_mode() {
        struct SequentialOnly;
        impl RuntimeContract for SequentialOnly {
            fn id(&self) -> &'static str {
                "test/sequential-only"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::Zero,
                    ..Guarantees::default()
                }
            }
        }
        let spec = DynamicTopology::new().with_machine(MachineInstance::new(
            "a",
            "A",
            MachinePhysicalSpec {
                execution: ExecutionHint::CpuBoundN(4),
                ..MachinePhysicalSpec::default()
            },
        ));
        let report = SequentialOnly.check_spec(&spec, &schema_io());
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-exec-mode"));
    }

    #[test]
    fn rejects_over_budget_threads() {
        struct Budget1;
        impl RuntimeContract for Budget1 {
            fn id(&self) -> &'static str {
                "test/budget-1"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        thread_per_machine: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    physical: PhysicalBudget {
                        max_threads: 1,
                        ..PhysicalBudget::default()
                    },
                    ..Guarantees::default()
                }
            }
        }
        let spec = DynamicTopology::new()
            .with_machine(MachineInstance::new(
                "a",
                "A",
                MachinePhysicalSpec {
                    execution: ExecutionHint::CpuBound,
                    ..MachinePhysicalSpec::default()
                },
            ))
            .with_machine(MachineInstance::new(
                "b",
                "B",
                MachinePhysicalSpec {
                    execution: ExecutionHint::CpuBound,
                    ..MachinePhysicalSpec::default()
                },
            ));
        let report = Budget1.check_spec(&spec, &schema_io());
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-resource-threads"));
    }

    #[test]
    fn zero_delay_runtime_requires_moore_on_cycle() {
        struct ZeroDelay;
        impl RuntimeContract for ZeroDelay {
            fn id(&self) -> &'static str {
                "test/zero-delay"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::Zero,
                    ..Guarantees::default()
                }
            }
        }
        // a → b → a cycle, neither Moore.
        let spec = DynamicTopology::new()
            .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()))
            .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
            .with_link(crate::link::LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline))
            .with_link(crate::link::LinkSpec::new(("b", "out"), ("a", "in"), LinkKind::Inline));
        let report = ZeroDelay.check_spec(&spec, &schema_io());
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-cycle-moore"));
    }

    #[test]
    fn zero_delay_runtime_flags_cycle_among_mixed_moore_spec() {
        // Regression: the old `spec_has_non_moore_cycle` only flagged a cycle
        // when NO machine was Moore. With two cycles — one breaking through a
        // Moore machine, one not — the unsafe cycle was silently accepted.
        // The exact `find_non_moore_cycle` (cycle in the non-Moore subgraph)
        // must flag it.
        struct ZeroDelay;
        impl RuntimeContract for ZeroDelay {
            fn id(&self) -> &'static str {
                "test/zero-delay-2"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::Zero,
                    ..Guarantees::default()
                }
            }
        }
        // Safe cycle: m(moore) → m → m(moore).
        // Unsafe cycle: a → b → a, neither Moore.
        let spec = DynamicTopology::new()
            .with_machine(MachineInstance::new("m1", "M1", MachinePhysicalSpec::default()).moore())
            .with_machine(MachineInstance::new("m2", "M2", MachinePhysicalSpec::default()))
            .with_machine(MachineInstance::new("m3", "M3", MachinePhysicalSpec::default()).moore())
            .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()))
            .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
            .with_link(crate::link::LinkSpec::new(("m1", "out"), ("m2", "in"), LinkKind::Inline))
            .with_link(crate::link::LinkSpec::new(("m2", "out"), ("m3", "in"), LinkKind::Inline))
            .with_link(crate::link::LinkSpec::new(("m3", "out"), ("m1", "in"), LinkKind::Inline))
            .with_link(crate::link::LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline))
            .with_link(crate::link::LinkSpec::new(("b", "out"), ("a", "in"), LinkKind::Inline));
        let report = ZeroDelay.check_spec(&spec, &schema_io());
        assert!(
            report.violations.iter().any(|v| v.rule_id == "runtime-cycle-moore"),
            "unsafe non-Moore cycle missed among mixed-Moore spec: {:?}",
            report.violations,
        );
    }

    // ── S3-3: backpressure policy ↔ carrier correspondence ─────────────────────

    /// A runtime whose carriers can only block (no drop / overwrite / defer).
    fn block_only_runtime() -> impl RuntimeContract {
        struct BlockOnly;
        impl RuntimeContract for BlockOnly {
            fn id(&self) -> &'static str {
                "test/block-only"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        bounded_buf: Some(BoundedBufSupport {
                            write_policies: vec![WritePolicy::Blocking],
                            read_policies: vec![ReadPolicy::Blocking],
                            max_capacity: 0,
                        }),
                        channel: Some(ChannelSupport {
                            drop_when_full: false,
                            max_capacity: 0,
                        }),
                        latest: true,
                        cas_free_ring: Some(CasFreeRingSupport {
                            heap: true,
                            static_region: false,
                        }),
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    backpressure: BackpressureActionSupport {
                        block: true,
                        ..BackpressureActionSupport::default()
                    },
                    ..Guarantees::default()
                }
            }
        }
        BlockOnly
    }

    #[test]
    fn rejects_unsupported_backpressure_action() {
        // Overwrite-demanding link (BoundedBuf{Overwriting}) on a runtime
        // that can only block → runtime-backpressure-action violation.
        let rt = block_only_runtime();
        let spec = spec_ab(LinkKind::BoundedBuf {
            capacity: 4,
            write_policy: WritePolicy::Overwriting,
            read_policy: ReadPolicy::Blocking,
        });
        let report = rt.check_spec(&spec, &schema_io());
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-backpressure-action"),
            "{:?}", report.violations);

        // Latest (single overwrite slot) demands Overwrite too.
        let report = rt.check_spec(&spec_ab(LinkKind::Latest { capacity: 0 }), &schema_io());
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-backpressure-action"),
            "{:?}", report.violations);

        // Channel{drop_when_full:true} demands Drop.
        let report = rt.check_spec(
            &spec_ab(LinkKind::Channel { capacity: 4, drop_when_full: true }),
            &schema_io(),
        );
        assert!(report
            .violations
            .iter()
            .any(|v| v.rule_id == "runtime-backpressure-action"),
            "{:?}", report.violations);
    }

    #[test]
    fn accepts_backpressure_actions_when_declared() {
        // ReferenceRuntime declares block/drop/overwrite → all stateless
        // carrier semantics pass.
        let rt = ReferenceRuntime;
        let specs = [
            spec_ab(LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            }),
            spec_ab(LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::Blocking,
            }),
            spec_ab(LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: WritePolicy::Overwriting,
                read_policy: ReadPolicy::Blocking,
            }),
            spec_ab(LinkKind::Channel { capacity: 4, drop_when_full: false }),
            spec_ab(LinkKind::Channel { capacity: 4, drop_when_full: true }),
            spec_ab(LinkKind::Latest { capacity: 0 }),
            spec_ab(LinkKind::CasFreeRing {
                capacity: 4,
                storage: crate::link::MemoryRegion::Heap { size: 64 },
            }),
        ];
        for spec in &specs {
            let report = rt.check_spec(spec, &schema_io());
            assert!(report.is_ok(), "{:?}", report.violations);
        }
    }

    #[test]
    fn policy_required_action_matches_semantics() {
        // Each policy declares the action it demands when full/out-of-credit.
        assert_eq!(BlockPolicy::new().required_action(), BackpressureAction::Block);
        assert_eq!(DropPolicy::new().required_action(), BackpressureAction::Drop);
        assert_eq!(OverwritePolicy::new().required_action(), BackpressureAction::Overwrite);
        assert_eq!(CreditPolicy::default().required_action(), BackpressureAction::Defer);
    }

    #[test]
    fn credit_policy_requires_defer_support() {
        // CreditPolicy demands Defer; a runtime without defer (e.g. the
        // reference runtime — credit not yet wired into carriers) must reject
        // wiring it; a runtime declaring defer accepts it.
        let g = ReferenceRuntime.guarantees();
        assert!(!g.backpressure_supported(&CreditPolicy::default()),
            "reference runtime does not implement credit/defer");
        assert!(g.backpressure_supported(&BlockPolicy::new()));
        assert!(g.backpressure_supported(&DropPolicy::new()));
        assert!(g.backpressure_supported(&OverwritePolicy::new()));

        let with_defer = Guarantees {
            backpressure: BackpressureActionSupport {
                block: true,
                drop: true,
                overwrite: true,
                defer: true,
            },
            ..Guarantees::default()
        };
        assert!(with_defer.backpressure_supported(&CreditPolicy::default()));
    }

    /// A runtime that declares *no* backpressure actions rejects every link
    /// whose carrier has queued semantics — even a plain blocking send. Only
    /// Inline / SharedState (no queued backpressure) and `Proceed` (a
    /// successful send) remain admissible.
    #[test]
    fn no_backpressure_runtime_rejects_queued_links() {
        struct NoBp;
        impl RuntimeContract for NoBp {
            fn id(&self) -> &'static str {
                "test/no-bp"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        bounded_buf: Some(BoundedBufSupport {
                            write_policies: vec![
                                WritePolicy::Blocking,
                                WritePolicy::Dropping,
                                WritePolicy::Overwriting,
                            ],
                            read_policies: vec![ReadPolicy::Blocking],
                            max_capacity: 0,
                        }),
                        channel: Some(ChannelSupport {
                            drop_when_full: true,
                            max_capacity: 0,
                        }),
                        latest: true,
                        shared_state: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    // backpressure: all-false (default) — carriers can only
                    // `Proceed`.
                    ..Guarantees::default()
                }
            }
        }
        let rt = NoBp;
        // Every queued carrier demands an action the runtime cannot execute.
        for link in [
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::Blocking,
            },
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: WritePolicy::Overwriting,
                read_policy: ReadPolicy::Blocking,
            },
            LinkKind::Channel { capacity: 4, drop_when_full: false },
            LinkKind::Channel { capacity: 4, drop_when_full: true },
            LinkKind::Latest { capacity: 0 },
        ] {
            let report = rt.check_spec(&spec_ab(link.clone()), &schema_io());
            assert!(
                report.violations.iter().any(|v| v.rule_id == "runtime-backpressure-action"),
                "expected backpressure violation for {:?}: {:?}",
                link,
                report.violations,
            );
        }
        // Inline / SharedState have no queued semantics → no backpressure
        // violation even on a no-bp runtime.
        for link in [LinkKind::Inline, LinkKind::SharedState] {
            let report = rt.check_spec(&spec_ab(link.clone()), &schema_io());
            assert!(
                report.violations.iter().all(|v| v.rule_id != "runtime-backpressure-action"),
                "unexpected backpressure violation for {:?}: {:?}",
                link,
                report.violations,
            );
        }
    }

    // ── B2: deep physical budget (CPU affinity / NUMA / huge pages / SIMD) ────

    /// A machine that declares the full hard-real-time stack: exclusive cores,
    /// NUMA node 0, 2 MiB huge pages, and an AVX2 hot path.
    fn hard_real_time_spec() -> DynamicTopology {
        DynamicTopology::new().with_machine(MachineInstance::new(
            "rt",
            "RT",
            MachinePhysicalSpec {
                execution: ExecutionHint::CpuBound,
                cpu_affinity: CpuAffinity::Exclusive(vec![2, 3]),
                numa_node: Some(0),
                huge_pages: HugePages::Size2MiB,
                simd: Some(SimdRequirement {
                    features: vec![alloc::borrow::Cow::Borrowed("avx2")],
                }),
                ..MachinePhysicalSpec::default()
            },
        ))
    }

    /// A runtime that can honor none of the deep physical budget.
    fn no_deep_budget_runtime() -> impl RuntimeContract {
        struct NoDeep;
        impl RuntimeContract for NoDeep {
            fn id(&self) -> &'static str {
                "test/no-deep-budget"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        thread_per_machine: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    physical: PhysicalBudget::default(),
                    ..Guarantees::default()
                }
            }
        }
        NoDeep
    }

    /// A runtime that can honor the full deep physical budget.
    fn full_deep_budget_runtime() -> impl RuntimeContract {
        struct FullDeep;
        impl RuntimeContract for FullDeep {
            fn id(&self) -> &'static str {
                "test/full-deep-budget"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        thread_per_machine: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    physical: PhysicalBudget {
                        cpu_affinity: true,
                        cpu_exclusive: true,
                        numa: true,
                        huge_pages: true,
                        simd_features: vec![alloc::borrow::Cow::Borrowed("avx2")],
                        ..PhysicalBudget::default()
                    },
                    ..Guarantees::default()
                }
            }
        }
        FullDeep
    }

    #[test]
    fn rejects_deep_physical_budget_when_unsupported() {
        let rt = no_deep_budget_runtime();
        let report = rt.check_spec(&hard_real_time_spec(), &schema_io());
        for rule in [
            "runtime-resource-affinity",
            "runtime-resource-affinity-exclusive",
            "runtime-resource-numa",
            "runtime-resource-hugepages",
            "runtime-resource-simd",
        ] {
            assert!(
                report.violations.iter().any(|v| v.rule_id == rule),
                "expected {rule} violation, got {:?}",
                report.violations,
            );
        }
    }

    #[test]
    fn accepts_deep_physical_budget_when_declared() {
        let rt = full_deep_budget_runtime();
        let report = rt.check_spec(&hard_real_time_spec(), &schema_io());
        assert!(report.is_ok(), "{:?}", report.violations);
    }

    #[test]
    fn allowed_affinity_needs_only_pinning_not_exclusivity() {
        // `CpuAffinity::Allowed` must be satisfied by a runtime that pins
        // cores but does not guarantee exclusive ownership.
        struct PinOnly;
        impl RuntimeContract for PinOnly {
            fn id(&self) -> &'static str {
                "test/pin-only"
            }
            fn guarantees(&self) -> Guarantees {
                Guarantees {
                    link_kinds: LinkKindSupport {
                        inline: true,
                        ..LinkKindSupport::default()
                    },
                    exec_modes: ExecModeSupport {
                        sequential: true,
                        thread_per_machine: true,
                        ..ExecModeSupport::default()
                    },
                    link_delay: LinkDelay::OneTick,
                    physical: PhysicalBudget {
                        cpu_affinity: true,
                        ..PhysicalBudget::default()
                    },
                    ..Guarantees::default()
                }
            }
        }
        let spec = DynamicTopology::new().with_machine(MachineInstance::new(
            "rt",
            "RT",
            MachinePhysicalSpec {
                execution: ExecutionHint::CpuBound,
                cpu_affinity: CpuAffinity::Allowed(vec![4]),
                ..MachinePhysicalSpec::default()
            },
        ));
        let report = PinOnly.check_spec(&spec, &schema_io());
        assert!(report.is_ok(), "{:?}", report.violations);

        // The same runtime must reject the exclusive form.
        let spec = DynamicTopology::new().with_machine(MachineInstance::new(
            "rt",
            "RT",
            MachinePhysicalSpec {
                execution: ExecutionHint::CpuBound,
                cpu_affinity: CpuAffinity::Exclusive(vec![4]),
                ..MachinePhysicalSpec::default()
            },
        ));
        let report = PinOnly.check_spec(&spec, &schema_io());
        assert!(
            report.violations.iter().any(|v| v.rule_id == "runtime-resource-affinity-exclusive"),
            "{:?}",
            report.violations,
        );
    }
}
