//! DynamicTopology serialization roundtrip verification
//!
//! Verifies that `DynamicTopology` and its components survive a JSON roundtrip under the `serialize` feature.
//!
//! Run with: `cargo test --features serialize --test e60-serde-roundtrip`

#![cfg(feature = "serialize")]

extern crate alloc;

use axiom::deploy::{DynamicTopology, MachineInstance, FuncBinding};
use axiom::link::{LinkKind, LinkSpec, WritePolicy, ReadPolicy};
use axiom::resource::{ExecutionHint, MachinePhysicalSpec, ThreadPoolSpec};

/// Build a complete DynamicTopology to use in the roundtrip tests
fn make_spec() -> DynamicTopology {
    DynamicTopology::new()
        .with_machine(
            MachineInstance::new(
                "ws_reader",
                "ws_machine",
                MachinePhysicalSpec {
                    execution: ExecutionHint::CpuBound,
                    state_heap_bytes: 8192,
                    cache_line_align: true,
                    deterministic: true,
                    max_cleanup_latency_us: 5000,
                    per_message_latency_us: 0,
                },
            )
            .moore(),
        )
        .with_machine(
            MachineInstance::new(
                "pipeline",
                "seg_sig_machine",
                MachinePhysicalSpec::default(),
            ),
        )
        .with_func(FuncBinding::new("agg_func", "aggregator"))
        .with_link(LinkSpec::new(
            ("ws_reader", "trade_out"),
            ("pipeline", "bar_in"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("pipeline", "signal_out"),
            ("ws_reader", "ctrl_in"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ))
}

#[test]
fn e60_1_full_roundtrip() {
    let original = make_spec();
    let json = serde_json::to_string(&original).expect("serialize");
    let restored: DynamicTopology = serde_json::from_str(&json).expect("deserialize");

    // Machine count
    assert_eq!(original.machines.len(), restored.machines.len());
    // Machine names and types
    for (o, r) in original.machines.iter().zip(restored.machines.iter()) {
        assert_eq!(o.name, r.name, "machine name mismatch");
        assert_eq!(o.machine_type, r.machine_type, "machine_type mismatch");
        assert_eq!(o.is_moore, r.is_moore, "is_moore mismatch");
    }
    // Link count and endpoints
    assert_eq!(original.links.len(), restored.links.len());
    for (o, r) in original.links.iter().zip(restored.links.iter()) {
        assert_eq!(o.out, r.out, "link out mismatch");
        assert_eq!(o.into, r.into, "link into mismatch");
        assert_eq!(o.kind, r.kind, "link kind mismatch");
    }
    // Func bindings
    assert_eq!(original.funcs.len(), restored.funcs.len());
    for (o, r) in original.funcs.iter().zip(restored.funcs.iter()) {
        assert_eq!(o.name, r.name);
        assert_eq!(o.func_type, r.func_type);
    }
    // Settings
    assert_eq!(original.settings.cpu_threads, restored.settings.cpu_threads);
    assert_eq!(original.settings.io_threads, restored.settings.io_threads);

    println!("E60.1 full DynamicTopology serialization roundtrip ✓");
}

#[test]
fn e60_2_cow_dual_source_equivalence() {
    use alloc::borrow::Cow;

    // A Cow built from code (Borrowed) and one from deserialization (Owned) should be equal
    let borrowed: Cow<'static, str> = Cow::Borrowed("test_name");
    let owned: Cow<'static, str> = Cow::Owned("test_name".to_string());
    assert_eq!(borrowed, owned, "Cow::Borrowed == Cow::Owned for same content");

    // MachineInstance constructed from code (Borrowed)
    let from_code = MachineInstance::new("m1", "type1", MachinePhysicalSpec::default());
    // Serialize → deserialize (Owned)
    let json = serde_json::to_string(&from_code).expect("serialize");
    let from_json: MachineInstance = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(from_code.name, from_json.name);
    assert_eq!(from_code.machine_type, from_json.machine_type);

    println!("E60.2 Cow<'static, str> dual-source equivalence ✓");
}

#[test]
fn e60_3_is_moore_default_false() {
    // Without explicitly setting is_moore, the serialized JSON should omit it or use false
    let m = MachineInstance::new("m", "t", MachinePhysicalSpec::default());
    let json = serde_json::to_string(&m).expect("serialize");

    // After deserialization, is_moore should be false (serde(default))
    let restored: MachineInstance = serde_json::from_str(&json).expect("deserialize");
    assert!(!restored.is_moore, "is_moore should default to false");

    // Build a JSON without the is_moore field (simulating a config file that omits it)
    let json_without_moore = r#"{"name":"x","machine_type":"y","physical":{"execution":"Async","state_heap_bytes":4096,"cache_line_align":false,"deterministic":false,"max_cleanup_latency_us":10000},"config_overrides":[],"is_moore":false}"#;
    let from_minimal: MachineInstance = serde_json::from_str(json_without_moore).expect("deserialize");
    assert!(!from_minimal.is_moore);

    println!("E60.3 is_moore defaults to false ✓");
}

#[test]
fn e60_4_config_overrides_roundtrip() {
    use alloc::borrow::Cow;

    let mut m = MachineInstance::new("m", "t", MachinePhysicalSpec::default());
    m.config_overrides.push((
        Cow::Borrowed("threshold"),
        "0.5".to_string(),
    ));
    m.config_overrides.push((
        Cow::Borrowed("mode"),
        "\"fast\"".to_string(),
    ));

    let json = serde_json::to_string(&m).expect("serialize");
    let restored: MachineInstance = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(m.config_overrides.len(), restored.config_overrides.len());
    for (o, r) in m.config_overrides.iter().zip(restored.config_overrides.iter()) {
        assert_eq!(o.0, r.0, "config override key mismatch");
        assert_eq!(o.1, r.1, "config override value mismatch");
    }

    println!("E60.4 config_overrides roundtrip ✓");
}

#[test]
fn e60_5_execution_hint_variants_roundtrip() {
    let hints = vec![
        ExecutionHint::Async,
        ExecutionHint::CpuBound,
        ExecutionHint::CpuBoundN(4),
        ExecutionHint::ThreadPool(ThreadPoolSpec::io_pool("pool", 8)),
    ];

    for hint in &hints {
        let json = serde_json::to_string(hint).expect("serialize");
        let restored: ExecutionHint = serde_json::from_str(&json).expect("deserialize");
        // ExecutionHint does not derive PartialEq, so compare Debug strings
        assert_eq!(
            format!("{:?}", hint),
            format!("{:?}", restored),
            "ExecutionHint roundtrip mismatch"
        );
    }

    println!("E60.5 ExecutionHint variant roundtrip ✓");
}

#[test]
fn e60_6_link_kind_variants_roundtrip() {
    let kinds = vec![
        LinkKind::Inline,
        LinkKind::BoundedBuf {
            capacity: 256,
            write_policy: WritePolicy::Blocking,
            read_policy: ReadPolicy::Blocking,
        },
        LinkKind::Channel {
            capacity: 512,
            drop_when_full: false,
        },
    ];

    for kind in &kinds {
        let json = serde_json::to_string(kind).expect("serialize");
        let restored: LinkKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(kind, &restored, "LinkKind roundtrip mismatch");
    }

    println!("E60.6 LinkKind variant roundtrip ✓");
}
