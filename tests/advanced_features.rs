use axiom::prelude_all::*;
use axiom::builtin::{IdentityInput, IdentityOutput};
use axiom::session::{is_consistent, project, GlobalOp, GlobalType, LocalOp};
use axiom::topology::{TopologyMutation, TopologyOp};

// ════════════════════════════════════════════════════════════════════════════
// ConfigCell tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_config_cell_initial_value() {
    let cell = ConfigCell::new(42i32);
    let (val, ver) = cell.get();
    assert_eq!(val, 42);
    assert_eq!(ver, 0);
}

#[test]
fn test_config_cell_update_bumps_version() {
    let cell = ConfigCell::new(0u32);
    assert_eq!(cell.version(), 0);

    cell.update(10);
    assert_eq!(cell.version(), 1);
    let (val, _) = cell.get();
    assert_eq!(val, 10);

    cell.update(20);
    assert_eq!(cell.version(), 2);
}

#[test]
fn test_config_cell_check_detects_change() {
    let cell = ConfigCell::new("hello".to_string());

    // No change at version 0.
    let (opt, ver) = cell.check(0);
    assert!(opt.is_none());
    assert_eq!(ver, 0);

    // Update and check.
    cell.update("world".to_string());
    let (opt, ver) = cell.check(0);
    assert!(opt.is_some());
    assert_eq!(opt.unwrap(), "world");
    assert_eq!(ver, 1);

    // Check at current version — no change.
    let (opt, ver) = cell.check(1);
    assert!(opt.is_none());
    assert_eq!(ver, 1);
}

// ════════════════════════════════════════════════════════════════════════════
// Session types tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_session_state_advances_through_protocol() {
    let protocol = SessionType::sequence(&[
        SessionOp::Send { type_name: "Login" },
        SessionOp::Recv { type_name: "Welcome" },
        SessionOp::End,
    ]);

    let mut state = SessionState::new(&protocol);
    assert!(!state.is_complete());

    assert!(state.can_send("Login"));
    assert!(state.advance_send("Login"));

    assert!(state.can_recv("Welcome"));
    assert!(state.advance_recv("Welcome"));

    assert!(state.is_complete());
}

#[test]
fn test_session_state_violation_sets_error() {
    let protocol = SessionType::sequence(&[
        SessionOp::Send { type_name: "Login" },
        SessionOp::End,
    ]);

    let mut state = SessionState::new(&protocol);

    // Try to recv when protocol says send — should fail.
    assert!(!state.advance_recv("Welcome"));
    assert!(state.is_error());
}

#[test]
fn test_session_duality_send_recv() {
    let client = SessionType::sequence(&[
        SessionOp::Send { type_name: "Request" },
        SessionOp::Recv { type_name: "Response" },
        SessionOp::End,
    ]);

    let server = SessionType::sequence(&[
        SessionOp::Recv { type_name: "Request" },
        SessionOp::Send { type_name: "Response" },
        SessionOp::End,
    ]);

    assert!(is_dual(&client, &server));
    assert!(is_dual(&server, &client));
}

#[test]
fn test_session_duality_mismatch() {
    let a = SessionType::sequence(&[
        SessionOp::Send { type_name: "Foo" },
        SessionOp::End,
    ]);

    let b = SessionType::sequence(&[
        SessionOp::Send { type_name: "Foo" },  // Both send — not dual
        SessionOp::End,
    ]);

    assert!(!is_dual(&a, &b));
}

#[test]
fn test_session_duality_type_mismatch() {
    let a = SessionType::sequence(&[
        SessionOp::Send { type_name: "Foo" },
        SessionOp::End,
    ]);

    let b = SessionType::sequence(&[
        SessionOp::Recv { type_name: "Bar" },  // Different type name
        SessionOp::End,
    ]);

    assert!(!is_dual(&a, &b));
}

#[test]
fn test_port_decl_with_session() {
    let protocol = SessionType::sequence(&[
        SessionOp::Send { type_name: "Data" },
        SessionOp::End,
    ]);

    let decl = PortDecl::output::<i32>("out")
        .with_session(protocol.clone());

    assert!(decl.session.is_some());
    assert_eq!(decl.session.as_ref().unwrap().ops().len(), 2);
}

#[test]
fn test_link_compat_with_dual_sessions() {
    let send_protocol = SessionType::sequence(&[
        SessionOp::Send { type_name: "i32" },
        SessionOp::End,
    ]);

    let recv_protocol = SessionType::sequence(&[
        SessionOp::Recv { type_name: "i32" },
        SessionOp::End,
    ]);

    let out = PortDecl::output::<i32>("out")
        .with_session(send_protocol);
    let inp = PortDecl::input::<i32>("in")
        .with_session(recv_protocol);

    assert_eq!(out.can_link_to(&inp), LinkCompat::Compatible);
}

#[test]
fn test_link_compat_with_non_dual_sessions() {
    let send_a = SessionType::sequence(&[
        SessionOp::Send { type_name: "i32" },
        SessionOp::End,
    ]);

    let send_b = SessionType::sequence(&[
        SessionOp::Send { type_name: "i32" },
        SessionOp::End,
    ]);

    let out = PortDecl::output::<i32>("out")
        .with_session(send_a);
    let inp = PortDecl::input::<i32>("in")
        .with_session(send_b);

    // Both send — not dual.
    match out.can_link_to(&inp) {
        LinkCompat::Incompatible { reason } => {
            assert!(reason.contains("not dual"));
        }
        _ => panic!("expected Incompatible"),
    }
}

#[test]
fn test_link_compat_one_session_one_not() {
    let send_protocol = SessionType::sequence(&[
        SessionOp::Send { type_name: "i32" },
        SessionOp::End,
    ]);

    let out = PortDecl::output::<i32>("out").with_session(send_protocol);
    let inp = PortDecl::input::<i32>("in"); // no session

    match out.can_link_to(&inp) {
        LinkCompat::Incompatible { reason } => {
            assert!(reason.contains("one port has a session"));
        }
        _ => panic!("expected Incompatible"),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Schema migration tests
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
struct PayloadV0 { value: i32 }

struct V0ToV1Migrator;
impl SchemaMigrate for V0ToV1Migrator {
    type Value = PayloadV0;
    fn migrate(&self, value: PayloadV0, from_ver: u32, to_ver: u32) -> Option<PayloadV0> {
        match (from_ver, to_ver) {
            (0, 1) => Some(PayloadV0 { value: value.value * 2 }),
            _ => None,
        }
    }
}

#[test]
fn test_migrate_registry_typed() {
    let registry = MigrateRegistry::new();
    registry.register_typed("data_out", V0ToV1Migrator);

    assert!(registry.has_migrator("data_out", std::any::TypeId::of::<PayloadV0>()));
    assert!(!registry.has_migrator("data_out", std::any::TypeId::of::<i32>()));
}

#[test]
fn test_migrate_registry_apply() {
    let registry = MigrateRegistry::new();
    registry.register_typed("data_out", V0ToV1Migrator);

    let original = PayloadV0 { value: 10 };
    let boxed: Box<dyn std::any::Any + Send> = Box::new(original);
    let migrated = registry.migrate(
        "data_out",
        std::any::TypeId::of::<PayloadV0>(),
        boxed,
        0,
        1,
    );

    assert!(migrated.is_some());
    let migrated = migrated.unwrap();
    let result = migrated.downcast_ref::<PayloadV0>().unwrap();
    assert_eq!(result.value, 20);
}

#[test]
fn test_migrate_registry_no_migrator() {
    let registry = MigrateRegistry::new();
    let boxed: Box<dyn std::any::Any + Send> = Box::new(42i32);
    let result = registry.migrate(
        "unknown_port",
        std::any::TypeId::of::<i32>(),
        boxed,
        0,
        1,
    );
    assert!(result.is_none());
}

// ════════════════════════════════════════════════════════════════════════════
// Hybrid system tests
// ════════════════════════════════════════════════════════════════════════════

struct Thermostat;

impl HybridMachine for Thermostat {
    type Continuous = f64;
    type DiscreteState = bool; // heater on/off

    fn flow(c: &f64, dt: f64, d: &bool) -> f64 {
        if *d {
            c + 1.0 * dt // heating
        } else {
            c - 0.5 * dt // cooling
        }
    }

    fn guard(c: &f64, d: &bool) -> Option<Jump<bool>> {
        if *d && *c > 25.0 {
            Some(Jump::Transition(false)) // turn off
        } else if !*d && *c < 20.0 {
            Some(Jump::Transition(true)) // turn on
        } else {
            None
        }
    }
}

#[test]
fn test_hybrid_driver_evolution() {
    // Start at 18°C, heater off.
    let mut driver = HybridDriver::<Thermostat>::new(18.0, false);

    // Step 1 second — cooling (but heater is off, so it cools).
    driver.step(1.0);
    assert!(*driver.continuous() < 18.0); // temperature dropped
    assert!(!*driver.discrete()); // heater still off

    // Guard should fire when temp < 20.
    assert!(driver.has_pending_jumps());
}

#[test]
fn test_hybrid_driver_guard_fires() {
    // Start at 19°C, heater off — guard should fire immediately.
    let mut driver = HybridDriver::<Thermostat>::new(19.0, false);
    driver.step(0.1);

    assert!(driver.has_pending_jumps());
    let jumps = driver.apply_pending_jumps();
    assert_eq!(jumps.len(), 1);
    match &jumps[0] {
        Jump::Transition(new_state) => assert!(*new_state), // heater on
        _ => panic!("expected Transition"),
    }
    assert!(*driver.discrete()); // heater is now on
}

#[test]
fn test_hybrid_driver_no_guard() {
    // Start at 22°C, heater off — no guard fires.
    let mut driver = HybridDriver::<Thermostat>::new(22.0, false);
    driver.step(0.5);

    assert!(!driver.has_pending_jumps());
}

#[test]
fn test_hybrid_driver_step_to_tick() {
    // Verify TimeTick-based stepping computes dt correctly.
    let mut driver = HybridDriver::<Thermostat>::new(22.0, false);

    // First call sets baseline without evolving.
    driver.step_to_tick(TimeTick::from_nanos(0));
    assert!(*driver.continuous() == 22.0); // unchanged

    // Advance 1 second (1_000_000_000 ns) — cooling by 0.5.
    driver.step_to_tick(TimeTick::from_nanos(1_000_000_000));
    assert!((*driver.continuous() - 21.5).abs() < 1e-9);
}

#[test]
fn test_hybrid_driver_reset_jump() {
    // Jump::Reset should invoke reset() and update discrete state.
    struct Bouncer;

    impl HybridMachine for Bouncer {
        type Continuous = f64; // velocity
        type DiscreteState = bool; // moving up / down

        fn flow(c: &f64, _dt: f64, _d: &bool) -> f64 {
            *c // constant (no gravity for test simplicity)
        }

        fn guard(c: &f64, d: &bool) -> Option<Jump<bool>> {
            if *d && *c <= 0.0 {
                Some(Jump::Reset { new_discrete: false })
            } else {
                None
            }
        }

        fn reset(c: &mut f64, _old_d: &bool, new_d: &bool) {
            if !*new_d {
                *c = 10.0; // bounce back to full velocity
            }
        }
    }

    let mut driver = HybridDriver::<Bouncer>::new(0.0, true); // at ground, moving up
    driver.step(0.0); // guard fires: velocity <= 0 and moving up
    assert!(driver.has_pending_jumps());

    let jumps = driver.apply_pending_jumps();
    assert_eq!(jumps.len(), 1);
    match &jumps[0] {
        Jump::Reset { new_discrete } => assert!(!*new_discrete),
        _ => panic!("expected Reset"),
    }
    assert!(!*driver.discrete()); // now moving down
    assert!((*driver.continuous() - 10.0).abs() < 1e-9); // velocity reset to 10
}

// ════════════════════════════════════════════════════════════════════════════
// Dynamic topology tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_topology_spawn() {
    let mut topo = TopologyMutation::new();
    assert_eq!(topo.machine_count(), 0);

    let delta = topo.apply(TopologyOp::Spawn {
        name: "worker_1",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    });

    assert!(delta.is_ok());
    assert_eq!(topo.machine_count(), 1);
    match delta.unwrap().op {
        AppliedOp::Spawned { name } => assert_eq!(name, "worker_1"),
        _ => panic!("expected Spawned"),
    }
}

#[test]
fn test_topology_spawn_duplicate() {
    let mut topo = TopologyMutation::new();
    topo.apply(TopologyOp::Spawn {
        name: "worker_1",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();

    let result = topo.apply(TopologyOp::Spawn {
        name: "worker_1",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    });

    assert!(result.is_err());
    match result.unwrap_err() {
        TopologyError::DuplicateName(n) => assert_eq!(n, "worker_1"),
        _ => panic!("expected DuplicateName"),
    }
}

#[test]
fn test_topology_link_and_unlink() {
    let mut topo = TopologyMutation::new();
    topo.apply(TopologyOp::Spawn {
        name: "a",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Spawn {
        name: "b",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();

    // Link a → b.
    let result = topo.apply(TopologyOp::Link {
        out: ("a", "out"),
        into: ("b", "in"),
        kind: LinkKind::Inline,
    });
    assert!(result.is_ok());
    assert_eq!(topo.link_count(), 1);

    // Duplicate link should fail.
    let dup = topo.apply(TopologyOp::Link {
        out: ("a", "out"),
        into: ("b", "in"),
        kind: LinkKind::Inline,
    });
    assert!(dup.is_err());

    // Unlink.
    let unlinked = topo.apply(TopologyOp::Unlink {
        out: ("a", "out"),
        into: ("b", "in"),
    });
    assert!(unlinked.is_ok());
    assert_eq!(topo.link_count(), 0);
}

#[test]
fn test_topology_retire_with_links_fails() {
    let mut topo = TopologyMutation::new();
    topo.apply(TopologyOp::Spawn {
        name: "a",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Spawn {
        name: "b",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Link {
        out: ("a", "out"),
        into: ("b", "in"),
        kind: LinkKind::Inline,
    })
    .unwrap();

    // Retiring 'a' should fail because it has links.
    let result = topo.apply(TopologyOp::Retire { name: "a" });
    assert!(result.is_err());
    match result.unwrap_err() {
        TopologyError::MachineHasLinks(n) => assert_eq!(n, "a"),
        _ => panic!("expected MachineHasLinks"),
    }
}

#[test]
fn test_topology_retire_without_links_succeeds() {
    let mut topo = TopologyMutation::new();
    topo.apply(TopologyOp::Spawn {
        name: "lonely",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();

    let result = topo.apply(TopologyOp::Retire { name: "lonely" });
    assert!(result.is_ok());
    assert_eq!(topo.machine_count(), 0);
}

#[test]
fn test_topology_snapshot() {
    let mut topo = TopologyMutation::new();
    topo.apply(TopologyOp::Spawn {
        name: "a",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();

    let snapshot = topo.snapshot();
    assert_eq!(snapshot.machines.len(), 1);
    assert_eq!(snapshot.machines[0].name, "a");
}

#[test]
fn test_topology_self_loop_rejected() {
    let mut topo = TopologyMutation::new();
    topo.apply(TopologyOp::Spawn {
        name: "a",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();

    let result = topo.apply(TopologyOp::Link {
        out: ("a", "out"),
        into: ("a", "in"),
        kind: LinkKind::Inline,
    });
    assert!(result.is_err());
    match result.unwrap_err() {
        TopologyError::SelfLoop { machine } => {
            assert_eq!(machine, "a");
        }
        _ => panic!("expected SelfLoop"),
    }
}

#[test]
fn test_topology_cycle_between_machines_allowed() {
    // Cycles between DIFFERENT machines are allowed.
    // TopologyMutation rejects only self-loops, consistent with DynamicTopology::validate().
    let mut topo = TopologyMutation::new();
    topo.apply(TopologyOp::Spawn {
        name: "a",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Spawn {
        name: "b",
        machine_type: "t",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();

    // a → b (forward link)
    topo.apply(TopologyOp::Link {
        out: ("a", "out"),
        into: ("b", "in"),
        kind: LinkKind::Inline,
    })
    .unwrap();

    // b → a (creates a cycle a → b → a) — should SUCCEED now
    let result = topo.apply(TopologyOp::Link {
        out: ("b", "out"),
        into: ("a", "in"),
        kind: LinkKind::Inline,
    });
    assert!(result.is_ok(), "cycle between different machines should be allowed: {:?}", result.err());

    // detect_cycle() should still find the cycle (opt-in strict mode)
    let cycle = topo.detect_cycle();
    assert!(cycle.is_some(), "detect_cycle should find the a↔b cycle");
    let cycle = cycle.unwrap();
    assert!(cycle.iter().any(|s| s == "a"), "cycle should contain 'a'");
    assert!(cycle.iter().any(|s| s == "b"), "cycle should contain 'b'");
}

// ════════════════════════════════════════════════════════════════════════════
// Deep validation tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_validate_deep_port_compatibility() {
    use std::collections::HashMap;

    let mut schemas: HashMap<&str, PortSchema> = HashMap::new();
    schemas.insert("a", PortSchema::new().with(PortDecl::output::<i32>("out")));
    schemas.insert("b", PortSchema::new().with(PortDecl::input::<i32>("in")));

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline));

    let result = spec.validate_deep(&schemas);
    assert!(result.is_ok());
}

#[test]
fn test_validate_deep_type_mismatch() {
    use std::collections::HashMap;

    let mut schemas: HashMap<&str, PortSchema> = HashMap::new();
    schemas.insert("a", PortSchema::new().with(PortDecl::output::<i32>("out")));
    schemas.insert("b", PortSchema::new().with(PortDecl::input::<String>("in")));

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline));

    let result = spec.validate_deep(&schemas);
    assert!(result.is_err());
    match result.unwrap_err() {
        ValidationError::LinkTypeMismatch { reason, .. } => {
            assert!(reason.contains("type mismatch"));
        }
        _ => panic!("expected LinkTypeMismatch"),
    }
}

#[test]
fn test_validate_deep_resource_budget() {
    use std::collections::HashMap;

    let mut schemas: HashMap<&str, PortSchema> = HashMap::new();
    schemas.insert("a", PortSchema::new().with(PortDecl::output::<i32>("out")));

    let spec = DynamicTopology {
        machines: vec![MachineInstance {
            name: "a".into(),
            machine_type: "t".into(),
            physical: MachinePhysicalSpec {
                execution: ExecutionHint::CpuBound,
                ..Default::default()
            },
            config_overrides: vec![],
            is_moore: false,
        }],
        funcs: vec![],
        links: vec![],
        settings: DeploySettings { cpu_threads: 0, io_threads: 1 },
    };

    let result = spec.validate_deep(&schemas);
    assert!(result.is_err());
    match result.unwrap_err() {
        ValidationError::ResourceBudgetExceeded { requested_threads, available_threads } => {
            assert_eq!(requested_threads, 1);
            assert_eq!(available_threads, 0);
        }
        _ => panic!("expected ResourceBudgetExceeded"),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Cycle safety: validate_deep Moore-based algebraic loop detection
// ════════════════════════════════════════════════════════════════════════════

/// Helper: build a schema map for two machines "a" and "b", each with
/// an i32 input port "in" and i32 output port "out".
fn two_machine_schemas() -> std::collections::HashMap<&'static str, PortSchema> {
    let mut schemas: std::collections::HashMap<&str, PortSchema> = std::collections::HashMap::new();
    schemas.insert("a", PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<i32>("out")));
    schemas.insert("b", PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<i32>("out")));
    schemas
}

#[test]
fn test_validate_deep_unsafe_cycle_no_moore() {
    // a → b → a  with neither machine Moore → algebraic loop.
    // Uses BoundedBuf (buffered) — Moore safety applies to buffered links.
    // (Inline cycles are a separate check: see test_validate_deep_inline_cycle.)
    let schemas = two_machine_schemas();
    let buf = LinkKind::BoundedBuf {
        capacity: 16, write_policy: WritePolicy::Blocking, read_policy: ReadPolicy::Blocking,
    };
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), buf.clone()))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), buf));

    let result = spec.validate_deep(&schemas);
    assert!(result.is_err(), "cycle with no Moore machine must be rejected");
    match result.unwrap_err() {
        ValidationError::UnsafeCycle { cycle } => {
            // Cycle should mention both machines.
            assert!(cycle.iter().any(|n| n == "a"), "cycle should include 'a': {:?}", cycle);
            assert!(cycle.iter().any(|n| n == "b"), "cycle should include 'b': {:?}", cycle);
        }
        other => panic!("expected UnsafeCycle, got {:?}", other),
    }
}

#[test]
fn test_validate_deep_safe_cycle_with_moore() {
    // Same cycle, but machine "a" is Moore → loop broken → OK.
    // Uses BoundedBuf — Moore delay applies to buffered links.
    let schemas = two_machine_schemas();
    let buf = LinkKind::BoundedBuf {
        capacity: 16, write_policy: WritePolicy::Blocking, read_policy: ReadPolicy::Blocking,
    };
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()).moore())
        .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), buf.clone()))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), buf));

    let result = spec.validate_deep(&schemas);
    assert!(result.is_ok(), "cycle with one Moore machine should be safe: {:?}", result.err());
}

#[test]
fn test_validate_deep_inline_cycle_rejected() {
    // Inline cycle = synchronous call deadlock, regardless of Moore.
    // Even with a Moore machine, an Inline cycle is rejected.
    let schemas = two_machine_schemas();
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()).moore())
        .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), LinkKind::Inline));

    let result = spec.validate_deep(&schemas);
    assert!(result.is_err(), "Inline cycle must be rejected even with Moore");
    match result.unwrap_err() {
        ValidationError::InlineCycle { cycle } => {
            assert!(cycle.iter().any(|n| n == "a"), "cycle should include 'a': {:?}", cycle);
            assert!(cycle.iter().any(|n| n == "b"), "cycle should include 'b': {:?}", cycle);
        }
        other => panic!("expected InlineCycle, got {:?}", other),
    }
}

#[test]
fn test_validate_deep_dag_no_cycle_ok() {
    // Linear chain a → b → c (no cycle) → always OK regardless of Moore.
    let mut schemas: std::collections::HashMap<&str, PortSchema> = std::collections::HashMap::new();
    schemas.insert("a", PortSchema::new().with(PortDecl::output::<i32>("out")));
    schemas.insert("b", PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<i32>("out")));
    schemas.insert("c", PortSchema::new().with(PortDecl::input::<i32>("in")));

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("c", "C", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("b", "out"), ("c", "in"), LinkKind::Inline));

    let result = spec.validate_deep(&schemas);
    assert!(result.is_ok(), "DAG with no cycle should pass: {:?}", result.err());
}

#[test]
fn test_validate_deep_moore_breaks_only_its_own_cycle() {
    // Two cycles sharing no nodes: a↔b and c↔d.
    // Marking "a" Moore breaks a↔b but c↔d is still unsafe.
    // Uses BoundedBuf — Moore safety applies to buffered links.
    let schemas = {
        let mut m: std::collections::HashMap<&str, PortSchema> = std::collections::HashMap::new();
        for name in &["a", "b", "c", "d"] {
            m.insert(*name, PortSchema::new()
                .with(PortDecl::input::<i32>("in"))
                .with(PortDecl::output::<i32>("out")));
        }
        m
    };
    let buf = LinkKind::BoundedBuf {
        capacity: 16, write_policy: WritePolicy::Blocking, read_policy: ReadPolicy::Blocking,
    };
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "A", MachinePhysicalSpec::default()).moore())
        .with_machine(MachineInstance::new("b", "B", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("c", "C", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d", "D", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), buf.clone()))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), buf.clone()))
        .with_link(LinkSpec::new(("c", "out"), ("d", "in"), buf.clone()))
        .with_link(LinkSpec::new(("d", "out"), ("c", "in"), buf));

    let result = spec.validate_deep(&schemas);
    assert!(result.is_err(), "c↔d cycle has no Moore machine");
    match result.unwrap_err() {
        ValidationError::UnsafeCycle { cycle } => {
            // The reported cycle should be c↔d, not a↔b (a is Moore).
            assert!(!cycle.iter().any(|n| n == "a"), "Moore machine 'a' should not appear in unsafe cycle: {:?}", cycle);
            assert!(cycle.iter().any(|n| n == "c"), "cycle should include 'c': {:?}", cycle);
            assert!(cycle.iter().any(|n| n == "d"), "cycle should include 'd': {:?}", cycle);
        }
        other => panic!("expected UnsafeCycle, got {:?}", other),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Lifecycle typestate tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_lifecycle_typestate_full_flow() {
    // Verify the full lifecycle: Init → Running → Stopping → Stopped → cleanup.
    let ctx = MachineContext::new("identity");
    let handle = MachineHandle::<Identity<i32>, Init>::new(ctx).unwrap();

    // Init state: lifecycle flag should be Init.
    assert_eq!(handle.context().lifecycle(), Lifecycle::Init);

    // Transition to Running.
    let mut running = handle.start();
    assert_eq!(running.context().lifecycle(), Lifecycle::Running);

    // Process an input — only available in Running.
    let out = running.process(IdentityInput::Input(42));
    match out {
        SingleOutput::Yield(IdentityOutput::Output(v)) => assert_eq!(v, 42),
        _ => panic!("expected Yield"),
    }

    // Transition to Stopping.
    let mut stopping = running.stop();
    assert_eq!(stopping.context().lifecycle(), Lifecycle::Stopping);

    // Process is still available in Stopping (for draining).
    let out = stopping.process(IdentityInput::Input(99));
    match out {
        SingleOutput::Yield(IdentityOutput::Output(v)) => assert_eq!(v, 99),
        _ => panic!("expected Yield"),
    }

    // Transition to Stopped.
    let stopped = stopping.finish();
    assert_eq!(stopped.context().lifecycle(), Lifecycle::Stopped);

    // Cleanup — only available in Stopped.
    let result = stopped.cleanup();
    assert!(result.is_ok());
}

#[test]
fn test_lifecycle_typestate_state_inspection() {
    // Verify state() accessor works in all lifecycle states.
    let ctx = MachineContext::new("identity");
    let handle = MachineHandle::<Identity<i32>, Init>::new(ctx).unwrap();

    // In Init, state is accessible for inspection.
    let _state_ref = handle.state();

    let running = handle.start();
    let _state_ref = running.state();

    let stopping = running.stop();
    let _state_ref = stopping.state();

    let stopped = stopping.finish();
    let _state_ref = stopped.state();

    stopped.cleanup().unwrap();
}

// ════════════════════════════════════════════════════════════════════════════
// Session: is_consistent tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_is_consistent_valid_protocol() {
    // A valid 3-party protocol: Buyer → Seller (order), Seller → Shipper (dispatch).
    let global = GlobalType::sequence(&[
        GlobalOp::Message { from: "Buyer", to: "Seller", label: "order" },
        GlobalOp::Message { from: "Seller", to: "Shipper", label: "dispatch" },
        GlobalOp::End,
    ]);
    assert!(is_consistent(&global));
}

#[test]
fn test_is_consistent_empty() {
    // Empty global type is trivially consistent.
    assert!(is_consistent(&GlobalType::empty()));
}

#[test]
fn test_is_consistent_end_only() {
    // A global type with only End is consistent.
    assert!(is_consistent(&GlobalType::end()));
}

#[test]
fn test_is_consistent_single_message() {
    let global = GlobalType::sequence(&[
        GlobalOp::Message { from: "A", to: "B", label: "msg" },
        GlobalOp::End,
    ]);
    assert!(is_consistent(&global));
}

#[test]
fn test_is_consistent_large_protocol() {
    // A large valid protocol with 100 messages among 3 roles.
    let roles = ["A", "B", "C"];
    let mut ops = Vec::new();
    for i in 0..100 {
        let from = roles[i % 3];
        let to = roles[(i + 1) % 3];
        let label = Box::leak(format!("msg{}", i).into_boxed_str());
        ops.push(GlobalOp::Message { from, to, label });
    }
    ops.push(GlobalOp::End);
    let global = GlobalType::sequence(&ops);
    assert!(is_consistent(&global));
}

// ════════════════════════════════════════════════════════════════════════════
// Session: project with branching (Choice)
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_project_choice_first_branch() {
    // Global type with a choice: selector "A" picks between two branches.
    let branch1 = GlobalType::sequence(&[
        GlobalOp::Message { from: "A", to: "B", label: "left" },
        GlobalOp::End,
    ]);
    let branch2 = GlobalType::sequence(&[
        GlobalOp::Message { from: "A", to: "C", label: "right" },
        GlobalOp::End,
    ]);
    let global = GlobalType::sequence(&[
        GlobalOp::Choice {
            selector: "A",
            branches: vec![branch1, branch2],
        },
        GlobalOp::End,
    ]);

    // A is the selector → it projects to an internal Select; both branches are
    // fully preserved (no longer a degenerate "keep only the first branch").
    let local_a = project(&global, "A");
    let select = local_a.ops().iter().find_map(|op| match op {
        LocalOp::Select { branches } => Some(branches),
        _ => None,
    }).expect("A (selector) should project to an internal Select");
    assert_eq!(select.len(), 2);
    assert!(matches!(&select[0].ops()[0], LocalOp::Send { to, label } if *to == "B" && *label == "left"));
    assert!(matches!(&select[1].ops()[0], LocalOp::Send { to, label } if *to == "C" && *label == "right"));

    // B is not the selector → it projects to an external Choose (B accepts the peer's choice).
    let local_b = project(&global, "B");
    let choose = local_b.ops().iter().find_map(|op| match op {
        LocalOp::Choose { branches } => Some(branches),
        _ => None,
    }).expect("B (non-selector) should project to an external Choose");
    assert_eq!(choose.len(), 2);
    assert!(matches!(&choose[0].ops()[0], LocalOp::Recv { from, label } if *from == "A" && *label == "left"));
}

#[test]
fn test_project_recurse_preserves_recursion() {
    // Recursive global protocol: Server loops receiving requests and sending responses.
    let rec_body = GlobalType::sequence(&[
        GlobalOp::Message { from: "Client", to: "Server", label: "req" },
        GlobalOp::Message { from: "Server", to: "Client", label: "resp" },
        GlobalOp::Var { var: "loop" },
    ]);
    let global = GlobalType::sequence(&[
        GlobalOp::Recurse { var: "loop", body: Box::new(rec_body) },
        GlobalOp::End,
    ]);

    // Client projection: the Recurse structure is preserved (no longer "take only the first op of the body").
    let client = project(&global, "Client");
    let rec = client.ops().iter().find_map(|op| match op {
        LocalOp::Recurse { var, body } => Some((*var, body)),
        _ => None,
    }).expect("Client should project to a Recurse");
    assert_eq!(rec.0, "loop");
    let body = rec.1;
    assert!(matches!(&body.ops()[0], LocalOp::Send { to, label } if *to == "Server" && *label == "req"));
    assert!(matches!(&body.ops()[1], LocalOp::Recv { from, label } if *from == "Server" && *label == "resp"));
    assert!(matches!(&body.ops()[2], LocalOp::Var { var } if *var == "loop"));
}

#[test]
fn test_is_dual_choice_and_recursion() {
    // Select (internal choice) and Choose (external choice) are dual (branches dualize pairwise).
    let s = SessionType::sequence(&[
        SessionOp::Select {
            branches: vec![
                SessionType::single(SessionOp::Send { type_name: "a" }),
                SessionType::single(SessionOp::Send { type_name: "b" }),
            ],
        },
    ]);
    let c = SessionType::sequence(&[
        SessionOp::Choose {
            branches: vec![
                SessionType::single(SessionOp::Recv { type_name: "a" }),
                SessionType::single(SessionOp::Recv { type_name: "b" }),
            ],
        },
    ]);
    assert!(is_dual(&s, &c));
    assert!(is_dual(&c, &s));
    // Mismatched branches → not dual
    let c_bad = SessionType::sequence(&[
        SessionOp::Choose {
            branches: vec![
                SessionType::single(SessionOp::Recv { type_name: "a" }),
                SessionType::single(SessionOp::Recv { type_name: "X" }),
            ],
        },
    ]);
    assert!(!is_dual(&s, &c_bad));

    // Recursive duality: same var name + dual bodies.
    let r1 = SessionType::sequence(&[
        SessionOp::Recurse {
            var: "loop",
            body: Box::new(SessionType::single(SessionOp::Send { type_name: "m" })),
        },
        SessionOp::Var { var: "loop" },
    ]);
    let r2 = SessionType::sequence(&[
        SessionOp::Recurse {
            var: "loop",
            body: Box::new(SessionType::single(SessionOp::Recv { type_name: "m" })),
        },
        SessionOp::Var { var: "loop" },
    ]);
    assert!(is_dual(&r1, &r2));
    // Different var names → not dual
    let r3 = SessionType::sequence(&[
        SessionOp::Recurse {
            var: "other",
            body: Box::new(SessionType::single(SessionOp::Recv { type_name: "m" })),
        },
        SessionOp::Var { var: "other" },
    ]);
    assert!(!is_dual(&r1, &r3));
}

#[test]
fn test_is_consistent_recursive() {
    // Messages inside Choice branches must also participate in the consistency check.
    let branch1 = GlobalType::sequence(&[
        GlobalOp::Message { from: "A", to: "B", label: "ok" },
        GlobalOp::End,
    ]);
    let branch2 = GlobalType::sequence(&[
        GlobalOp::Message { from: "A", to: "C", label: "alt" },
        GlobalOp::End,
    ]);
    let global = GlobalType::sequence(&[
        GlobalOp::Choice { selector: "A", branches: vec![branch1, branch2] },
        GlobalOp::End,
    ]);
    // The labels A sends to B/C match in each branch's projection → consistent.
    assert!(is_consistent(&global));
}

#[test]
fn test_project_skip_for_uninvolved_role() {
    // Message A→B, project onto C (uninvolved) → should contain Skip.
    let global = GlobalType::sequence(&[
        GlobalOp::Message { from: "A", to: "B", label: "msg" },
        GlobalOp::End,
    ]);
    let local_c = project(&global, "C");
    // C is not involved in the A→B message, so it gets Skip.
    assert!(local_c.ops().iter().any(|op| matches!(op, LocalOp::Skip)));
}

// ════════════════════════════════════════════════════════════════════════════
// Topology: apply_batch rollback tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_apply_batch_rollback_on_self_loop() {
    // Cycles between different machines are ALLOWED, so we use a self-loop
    // (still rejected) to trigger batch rollback.
    let mut topo = TopologyMutation::new();

    // Pre-spawn two nodes.
    topo.apply(TopologyOp::Spawn {
        name: "pre_a",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Spawn {
        name: "pre_b",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Link {
        out: ("pre_a", "out"),
        into: ("pre_b", "in"),
        kind: LinkKind::Inline,
    })
    .unwrap();

    let pre_count = topo.machine_count();
    let pre_links = topo.link_count();

    // Batch that fails: spawn + self-loop on pre_a (rejected).
    let result = topo.apply_batch(vec![
        TopologyOp::Spawn {
            name: "batch_node",
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        },
        // This link is a self-loop: pre_a → pre_a (always rejected).
        TopologyOp::Link {
            out: ("pre_a", "out"),
            into: ("pre_a", "in"),
            kind: LinkKind::Inline,
        },
    ]);

    // Batch should fail (self-loop rejected).
    assert!(result.is_err());

    // Verify rollback: state should be unchanged.
    assert_eq!(topo.machine_count(), pre_count, "machine count should be restored after rollback");
    assert_eq!(topo.link_count(), pre_links, "link count should be restored after rollback");
}

#[test]
fn test_apply_batch_partial_failure_rollback() {
    let mut topo = TopologyMutation::new();

    // Batch: spawn 3 nodes (OK), then duplicate spawn (fails).
    let result = topo.apply_batch(vec![
        TopologyOp::Spawn {
            name: "node1",
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        },
        TopologyOp::Spawn {
            name: "node2",
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        },
        TopologyOp::Spawn {
            name: "node3",
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        },
        // Duplicate name — should fail.
        TopologyOp::Spawn {
            name: "node1",
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        },
    ]);

    assert!(result.is_err());
    // All 3 spawns should be rolled back.
    assert_eq!(topo.machine_count(), 0, "all spawns should be rolled back");
}

// ════════════════════════════════════════════════════════════════════════════
// Topology: Replace operation tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn test_topology_replace_transfers_links() {
    let mut topo = TopologyMutation::new();

    // Spawn A → B chain.
    topo.apply(TopologyOp::Spawn {
        name: "old_a",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Spawn {
        name: "b",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    topo.apply(TopologyOp::Link {
        out: ("old_a", "out"),
        into: ("b", "in"),
        kind: LinkKind::Inline,
    })
    .unwrap();

    assert_eq!(topo.machine_count(), 2);
    assert_eq!(topo.link_count(), 1);

    // Replace old_a with new_a — links should transfer.
    let result = topo.apply(TopologyOp::Replace {
        old_name: "old_a",
        new_name: "new_a",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    });

    assert!(result.is_ok());
    // old_a should be gone, new_a should exist.
    assert_eq!(topo.machine_count(), 2);
    // The link should have been transferred to new_a.
    assert_eq!(topo.link_count(), 1);
}

#[test]
fn test_topology_replace_nonexistent_fails() {
    let mut topo = TopologyMutation::new();

    let result = topo.apply(TopologyOp::Replace {
        old_name: "ghost",
        new_name: "replacement",
        machine_type: "worker",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    });

    assert!(result.is_err());
}
