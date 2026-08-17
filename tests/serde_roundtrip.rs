//! Proves that a `DynamicTopology` declared in a config file (JSON here; TOML would
//! work the same way with the `toml` crate) round-trips through Serde under the
//! `serialize` feature.
//!
//! This is the payoff of the `Cow<'static, str>` migration: every deploy-time
//! type — `MachineInstance`, `FuncBinding`, `LinkSpec`, `DynamicTopology`,
//! `MachinePhysicalSpec`, `ExecutionHint`, `ThreadPoolSpec` — accepts both
//! `&'static str` literals (code-defined topologies) and owned `String`s
//! (config-defined topologies), so a declarative topology can be loaded
//! straight into a `DynamicTopology` and handed to a runtime adapter.

#![cfg(feature = "serialize")]

use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::resource::{ExecutionHint, MachinePhysicalSpec};

/// A realistic topology: two machines connected by a bounded buffer, declared
/// as JSON (the way a config file would express it), deserialized into a
/// `DynamicTopology`, validated, and checked field-by-field.
#[test]
fn deploy_spec_roundtrip_from_json() {
    let json = r#"{
        "machines": [
            {
                "name": "source",
                "machine_type": "ws_reader",
                "physical": {
                    "execution": "Async",
                    "state_heap_bytes": 8192,
                    "cache_line_align": false,
                    "deterministic": false,
                    "max_cleanup_latency_us": 5000
                },
                "config_overrides": [
                    ["url", "\"wss://feed.example.com\""]
                ]
            },
            {
                "name": "sink",
                "machine_type": "printer",
                "physical": {
                    "execution": { "CpuBoundN": 2 },
                    "state_heap_bytes": 4096,
                    "cache_line_align": true,
                    "deterministic": true,
                    "max_cleanup_latency_us": 1000
                },
                "config_overrides": []
            }
        ],
        "funcs": [],
        "links": [
            {
                "out": ["source", "trade_out"],
                "into": ["sink", "bar_in"],
                "kind": {
                    "BoundedBuf": {
                        "capacity": 1024,
                        "write_policy": "Blocking",
                        "read_policy": "Blocking"
                    }
                }
            }
        ],
        "settings": { "cpu_threads": 2, "io_threads": 2 }
    }"#;

    let spec: DynamicTopology = serde_json::from_str(json).expect("deserialize DynamicTopology");

    // Structural validation passes (names unique, endpoints exist, no cycles).
    spec.validate().expect("validate");

    // Field-by-field checks: the deserialized spec matches intent.
    assert_eq!(spec.machines.len(), 2);
    assert_eq!(spec.machines[0].name, "source");
    assert_eq!(spec.machines[0].machine_type, "ws_reader");
    assert_eq!(spec.machines[0].config_overrides.len(), 1);
    assert_eq!(spec.machines[0].config_overrides[0].0, "url");
    assert!(matches!(
        spec.machines[0].physical.execution,
        ExecutionHint::Async
    ));

    assert_eq!(spec.machines[1].name, "sink");
    assert!(matches!(
        spec.machines[1].physical.execution,
        ExecutionHint::CpuBoundN(2)
    ));
    assert!(spec.machines[1].physical.cache_line_align);
    assert!(spec.machines[1].physical.deterministic);

    assert_eq!(spec.links.len(), 1);
    assert_eq!(spec.links[0].out.0, "source");
    assert_eq!(spec.links[0].out.1, "trade_out");
    assert_eq!(spec.links[0].into.0, "sink");
    assert!(matches!(
        spec.links[0].kind,
        LinkKind::BoundedBuf {
            capacity: 1024,
            write_policy: WritePolicy::Blocking,
            read_policy: ReadPolicy::Blocking,
        }
    ));

    assert_eq!(spec.settings.cpu_threads, 2);
    assert_eq!(spec.settings.io_threads, 2);

    // Round-trip back to JSON and reparse — the spec is stable across cycles.
    let reserialized = serde_json::to_string(&spec).expect("serialize DynamicTopology");
    let spec2: DynamicTopology =
        serde_json::from_str(&reserialized).expect("re-deserialize DynamicTopology");
    assert_eq!(spec.machines.len(), spec2.machines.len());
    assert_eq!(spec2.machines[0].name, "source");
}

/// A code-built spec (using `&'static str` literals via the ergonomic
/// constructors) serializes to JSON and deserializes back identically.
/// This confirms the two construction paths interoperate.
#[test]
fn deploy_spec_code_built_serializes() {
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new(
            "a",
            "t",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "b",
            "t",
            MachinePhysicalSpec::default(),
        ))
        .with_link(LinkSpec::new(
            ("a", "out"),
            ("b", "in"),
            LinkKind::Inline,
        ));

    let json = serde_json::to_string(&spec).expect("serialize");
    let parsed: DynamicTopology = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.machines.len(), 2);
    assert_eq!(parsed.links.len(), 1);
    assert_eq!(parsed.machines[0].name, "a");
    assert_eq!(parsed.links[0].out.0, "a");
    // validate() must still pass after a round-trip.
    parsed.validate().expect("validate after round-trip");
}
