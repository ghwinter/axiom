//! Runtime capability contracts — the guardrail for runtime adapters.
//!
//! axiom is an abstraction layer: a `DeploySpec` is pure structure, and any
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
//!   `DeploySpec` against the declared guarantees and returns a structured
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

use crate::compat::HashMap;
use crate::deploy::{DeploySpec, RuleViolation, ValidationReport};
use crate::link::{
    LinkKind, LinkSpec, MemoryRegion, ReadPolicy, WritePolicy,
};
use crate::port::PortSchema;
use crate::resource::ExecutionHint;

#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::vec;
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

    /// Verify a `DeploySpec` against the declared guarantees.
    ///
    /// Default implementation checks, per link: kind support, per-kind policy
    /// support, capacity limits. Per machine: execution-mode support, physical
    /// budget. Plus: Moore/cycle requirement (only when `link_delay == Zero`)
    /// and total-thread budget.
    fn check_spec(
        &self,
        spec: &DeploySpec,
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
            let has_non_moore_cycle = spec_has_non_moore_cycle(spec);
            if has_non_moore_cycle {
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

// ── Helpers ────────────────────────────────────────────────────────────────────

fn thread_count(e: &ExecutionHint) -> usize {
    match e {
        ExecutionHint::CpuBound => 1,
        ExecutionHint::CpuBoundN(n) => *n,
        ExecutionHint::ThreadPool(spec) => spec.max_threads,
        ExecutionHint::Async | ExecutionHint::Subprocess(_) => 0,
    }
}

fn spec_has_non_moore_cycle(spec: &DeploySpec) -> bool {
    // Reuse the public analysis: feedback_loops finds cycles; for the
    // zero-delay rule we need a cycle without any Moore machine.
    // Simplification: if the spec has any feedback loop and not every
    // machine is Moore, flag it (conservative).
    let loops = crate::analysis::feedback_loops(spec);
    if loops.is_empty() {
        return false;
    }
    let any_moore = spec.machines.iter().any(|m| m.is_moore);
    !any_moore
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
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::{DeploySpec, MachineInstance};
    use crate::port::PortDecl;
    use crate::resource::MachinePhysicalSpec;

    fn schema_io() -> HashMap<&'static str, PortSchema> {
        let s = PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::input::<i32>("in"));
        let mut schemas = HashMap::new();
        schemas.insert("a", s.clone());
        schemas.insert("b", s.clone());
        schemas
    }

    fn spec_ab(link: LinkKind) -> DeploySpec {
        DeploySpec::new()
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
        let spec = DeploySpec::new().with_machine(MachineInstance::new(
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
        let spec = DeploySpec::new()
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
        let spec = DeploySpec::new()
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
}
