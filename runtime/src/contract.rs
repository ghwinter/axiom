//! Runtime capability contract — the `Runtime`'s honest declaration of what
//! it can physically honor.
//!
//! [`RuntimeContract`] turns the "constraint side" of axiom into a deployment
//! guard: the runtime declares its capabilities once ([`RuntimeContract::guarantees`]),
//! and [`RuntimeContract::check_spec`] audits every `DynamicTopology` against
//! them *before* materialization — a link kind the carrier factory cannot
//! build, a backpressure action the carriers cannot execute, or an execution
//! mode the driver does not implement is rejected up front instead of failing
//! mid-stream.
//!
//! The declaration below is kept in lockstep with the physical truth of this
//! crate:
//!
//! - [`crate::carrier::channel_for`] materializes all six `LinkKind` variants;
//! - the driver (`Sequential` / `Inline` / `Parallel(n)`) runs machines
//!   cooperatively and gives each machine a thread in parallel mode — it does
//!   not spawn subprocesses;
//! - the carriers execute block / drop / overwrite, but **credit/defer**
//!   (rescheduling on credit replenishment) is not yet wired in, so `defer`
//!   is honestly declared unsupported.

use axiom::link::{ReadPolicy, WritePolicy};
use axiom::runtime_contract::{
    BackpressureActionSupport, BoundedBufSupport, CasFreeRingSupport, ChannelSupport,
    ExecModeSupport, Guarantees, IoCapability, LinkDelay, LinkKindSupport, MemoryOrder,
    PhysicalBudget, RuntimeContract,
};

use crate::config::ExecMode;
use crate::runtime::Runtime;

impl RuntimeContract for Runtime {
    fn id(&self) -> &'static str {
        "axiom-runtime"
    }

    fn guarantees(&self) -> Guarantees {
        Guarantees {
            // The carrier factory handles every LinkKind (see carrier.rs):
            // BoundedBuf (Blocking/Dropping/Overwriting), Channel (with or
            // without drop), Latest/SharedState (single slot), CasFreeRing
            // (heap ring), Inline (migrated to an unbounded channel when the
            // edge crosses threads).
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
            // The driver runs machines in its loop regardless of the machine's
            // `ExecutionHint` (a uniform-drive model — threading is configured
            // globally via `RuntimeConfig::mode`). `sequential` and
            // `thread_per_machine` mirror `Sequential` / `Parallel(n)` modes;
            // `async_cooperative` covers machines declared `ExecutionHint::Async`,
            // which the loop drives synchronously. Subprocesses are not
            // implemented, so that hint is rejected.
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
            // Link delay is a property of the *selected driver*, not a
            // runtime-wide constant — the declaration must match the physics
            // of the configured mode (design principle §0.4: decoupling
            // without a verified correspondence is a lie):
            // - `Parallel(n)` — links are real channels; each hop is a
            //   one-tick temporal boundary, so cycles are safe without Moore
            //   machines (`CycleRule::AnyDelay`).
            // - `Sequential` / `Inline` — direct move delivery on the caller's
            //   thread, zero per-link delay; a cycle is an unbounded recursion
            //   within a tick, so it must contain a Moore machine
            //   (`CycleRule::RequireMoore`).
            link_delay: match self.config().mode {
                ExecMode::Parallel(_) => LinkDelay::OneTick,
                ExecMode::Sequential | ExecMode::Inline => LinkDelay::Zero,
            },
            deterministic_replay: true,
            physical: PhysicalBudget::default(),
            // BoundedBlocking blocks, BoundedDropping / Channel(drop) drops,
            // BoundedOverwriting / Latest overwrites. Credit/defer is not
            // wired into the carriers yet — declared unsupported so a
            // `CreditPolicy` cannot be silently mis-deployed.
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
    use axiom::backpressure::{BackpressurePolicy, BlockPolicy, CreditPolicy};
    use axiom::compat::HashMap;
    use axiom::deploy::{DynamicTopology, MachineInstance};
    use axiom::link::LinkKind;
    use axiom::resource::{
        ExecutionHint, MachinePhysicalSpec, RestartPolicy, SubprocessSpec,
    };

    fn empty_schemas() -> HashMap<&'static str, axiom::port::PortSchema> {
        HashMap::new()
    }

    fn ab(link: LinkKind) -> DynamicTopology {
        DynamicTopology::new()
            .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()))
            .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
            .with_link(axiom::link::LinkSpec::new(("a", "out"), ("b", "in"), link))
    }

    #[test]
    fn runtime_accepts_its_own_carriers() {
        let rt = Runtime::default();
        // Every carrier the runtime materializes must pass its own declaration.
        for link in [
            LinkKind::Inline,
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Dropping,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Overwriting,
                read_policy: axiom::link::ReadPolicy::NonBlocking,
            },
            LinkKind::Channel { capacity: 4, drop_when_full: true },
            LinkKind::Channel { capacity: 4, drop_when_full: false },
            LinkKind::Latest { capacity: 0 },
            LinkKind::CasFreeRing {
                capacity: 4,
                storage: axiom::link::MemoryRegion::Heap { size: 64 },
            },
            LinkKind::SharedState,
        ] {
            let report = rt.check_spec(&ab(link.clone()), &empty_schemas());
            assert!(report.is_ok(), "runtime rejected its own carrier {link:?}: {:?}", report.violations);
        }
    }

    #[test]
    fn runtime_rejects_subprocess_execution() {
        let rt = Runtime::default();
        let spec = DynamicTopology::new().with_machine(MachineInstance::new(
            "worker",
            "W",
            MachinePhysicalSpec {
                execution: ExecutionHint::Subprocess(SubprocessSpec {
                    executable: "isolated".into(),
                    args: vec![],
                    restart: RestartPolicy::Never,
                }),
                ..MachinePhysicalSpec::default()
            },
        ));
        let report = rt.check_spec(&spec, &empty_schemas());
        assert!(
            report.violations.iter().any(|v| v.rule_id == "runtime-exec-mode"),
            "subprocess hint must be rejected, got {:?}",
            report.violations,
        );
    }

    #[test]
    fn runtime_does_not_support_credit_defer() {
        // The carriers implement block/drop/overwrite but not credit-based
        // defer. A `CreditPolicy` must therefore be reported as unsupported —
        // wiring it would silently violate its no-loss flow-control semantics.
        let rt = Runtime::default();
        let g: Guarantees = rt.guarantees();
        assert!(g.backpressure_supported(&BlockPolicy::new()));
        assert!(!g.backpressure_supported(&CreditPolicy::default()));
        assert_eq!(CreditPolicy::default().required_action(), axiom::backpressure::BackpressureAction::Defer);
    }
}
