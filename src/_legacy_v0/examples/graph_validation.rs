//! # Graph-analysis demo: validating and analyzing a complex graph
//!
//! Demonstrates axiom's **graph-model** capabilities (the default scenario is a complex
//! graph network, not a linear pipeline):
//!
//! 1. **Valid complex graph** (kernel-style): syscall fan-out + storage/network dual paths
//!    + 3 feedback loops + an observation stream on a non-blocking carrier — `validate_deep`
//!    passes + structural analysis report
//! 2. **Invalid variants**: each one is detected (flow-type mismatch / Inline loop /
//!    all-Moore loop / Observe-on-blocking-carrier)
//!
//! Run with: `cargo run --example graph_validation`
//!
//! ```text
//!                ┌─────────── Storage path ─────────┐
//!  syscall ──to_vfs──► vfs ──bio──► block ──req──► driver.disk
//!    │ fan-out                                      │
//!    ├──to_net──► net ──skb──► driver.net ──────────┘
//!    ├──to_ipc──► ipc
//!    │
//!    ├──sched──► scheduler ◄──wakeup── memory ◄──fault──┐  loop ① scheduler↔memory
//!    │             │  run                                  │
//!    │             └──► process ──done──► wakeup ─────────┘  loop ② scheduler→process
//!    │                   │  req
//!    └───────────────────┴──► syscall ◄─────────────────────┘  loop ③ process→kernel big loop
//!    └──events(Observe)──► perf (observe)
//! ```

use std::collections::HashMap;

use axiom::analysis;
use axiom::deploy::{DynamicTopology, MachineInstance, ValidationError};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::port::{PortDecl, PortSchema};
use axiom::resource::MachinePhysicalSpec;

fn buf(capacity: usize) -> LinkKind {
    LinkKind::BoundedBuf {
        capacity,
        write_policy: WritePolicy::Blocking,
        read_policy: ReadPolicy::Blocking,
    }
}

/// Non-blocking observation carrier: best-effort, never back-pressures the
/// source (the S3-1 materialization-preference for `Observe` flow).
fn observe_buf(capacity: usize) -> LinkKind {
    LinkKind::Channel {
        capacity,
        drop_when_full: true,
    }
}

/// Kernel-style complex graph: syscall fan-out + dual paths + 3 feedback loops + observation.
/// Loop-legality: every cycle needs ≥1 Moore machine (its output depends only on
/// pre-update state, breaking the algebraic loop). scheduler is Moore and sits on
/// every loop, so the single marker makes all loops legal.
fn kernel_graph() -> (DynamicTopology, HashMap<&'static str, PortSchema>) {
    let mut s = HashMap::new();
    s.insert(
        "syscall",
        PortSchema::new()
            .with(PortDecl::input::<u64>("req"))
            .with(PortDecl::output::<u64>("to_vfs"))
            .with(PortDecl::output::<u64>("to_net"))
            .with(PortDecl::output::<u64>("to_ipc"))
            .with(PortDecl::output::<u64>("sched"))
            .with(PortDecl::observe::<u64>("events")),
    );
    s.insert(
        "vfs",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("bio")),
    );
    s.insert(
        "block",
        PortSchema::new()
            .with(PortDecl::input::<u64>("bio"))
            .with(PortDecl::output::<u64>("req")),
    );
    // driver uses two input ports (disk/net), each fed by a single input → degree constraint satisfied
    s.insert(
        "driver",
        PortSchema::new()
            .with(PortDecl::input::<u64>("disk"))
            .with(PortDecl::input::<u64>("net")),
    );
    s.insert(
        "net",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("skb")),
    );
    s.insert(
        "ipc",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("msg")),
    );
    s.insert(
        "scheduler",
        PortSchema::new()
            .with(PortDecl::input::<u64>("wakeup"))
            .with(PortDecl::output::<u64>("run"))
            .with(PortDecl::output::<u64>("fault")),
    );
    s.insert(
        "memory",
        PortSchema::new()
            .with(PortDecl::input::<u64>("fault"))
            .with(PortDecl::output::<u64>("ok")),
    );
    s.insert(
        "process",
        PortSchema::new()
            .with(PortDecl::input::<u64>("run"))
            .with(PortDecl::output::<u64>("done"))
            .with(PortDecl::output::<u64>("req")),
    );
    s.insert(
        "perf",
        PortSchema::new().with(PortDecl::new::<u64>("events", axiom::port::PortDir::In, axiom::flow::FlowKind::Observe)),
    );

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("syscall", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("vfs", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("block", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("driver", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("net", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("ipc", "t", MachinePhysicalSpec::default()))
        .with_machine(
            MachineInstance::new("scheduler", "t", MachinePhysicalSpec::default())
                // Loop-legality condition: every loop needs at least one Moore machine (breaks loop latency).
                // scheduler sits on all three loops → a single Moore marker makes all loops legal.
                .moore(),
        )
        .with_machine(MachineInstance::new("memory", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("process", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("perf", "t", MachinePhysicalSpec::default()))
        // fan-out: syscall → vfs/net/ipc (three output ports)
        .with_link(LinkSpec::new(("syscall", "to_vfs"), ("vfs", "in"), buf(8)))
        .with_link(LinkSpec::new(("syscall", "to_net"), ("net", "in"), buf(8)))
        .with_link(LinkSpec::new(("syscall", "to_ipc"), ("ipc", "in"), buf(8)))
        // storage path: vfs → block → driver.disk
        .with_link(LinkSpec::new(("vfs", "bio"), ("block", "bio"), buf(8)))
        .with_link(LinkSpec::new(("block", "req"), ("driver", "disk"), buf(8)))
        // network path: net → driver.net (both paths merge into driver, but ports are separate → degree-compliant)
        .with_link(LinkSpec::new(("net", "skb"), ("driver", "net"), buf(8)))
        // loop ①: scheduler ↔ memory (page fault → allocate → wakeup)
        .with_link(LinkSpec::new(("scheduler", "fault"), ("memory", "fault"), buf(8)))
        .with_link(LinkSpec::new(("memory", "ok"), ("scheduler", "wakeup"), buf(8)))
        // loop ②: scheduler → process → scheduler
        .with_link(LinkSpec::new(("scheduler", "run"), ("process", "run"), buf(8)))
        .with_link(LinkSpec::new(("process", "done"), ("scheduler", "wakeup"), buf(8)))
        // loop ③: process → syscall → scheduler → process (big loop)
        .with_link(LinkSpec::new(("process", "req"), ("syscall", "req"), buf(8)))
        .with_link(LinkSpec::new(("syscall", "sched"), ("scheduler", "wakeup"), buf(8)))
        // observation: syscall.events (Observe stream) → perf, on a non-blocking
        // carrier (an Observe edge must not back-pressure its source — S3-1)
        .with_link(LinkSpec::new(("syscall", "events"), ("perf", "events"), observe_buf(8)));
    (spec, s)
}

fn check(label: &str, spec: &DynamicTopology, schemas: &HashMap<&'static str, PortSchema>) {
    match spec.validate_deep(schemas) {
        Ok(_) => println!("    {label}: ✓ passed"),
        Err(ValidationError::UnsafeCycle { cycle }) => {
            println!("    {label}: ✗ UnsafeCycle {cycle:?} (cycle with no Moore machine — an algebraic loop)")
        }
        Err(ValidationError::InlineCycle { cycle }) => {
            println!("    {label}: ✗ InlineCycle {cycle:?} (synchronous-call deadlock)")
        }
        Err(ValidationError::CarrierViolatesSemantics { out, flow, carrier, .. }) => {
            println!("    {label}: ✗ CarrierViolatesSemantics {out:?} is {flow} but carrier {carrier} back-pressures the producer")
        }
        Err(e) => println!("    {label}: ✗ {e:?}"),
    }
}

fn main() {
    println!("=== axiom graph-analysis demo: validating and analyzing a complex graph ===\n");

    // ── 1. Legal complex graph: validate_deep + structural analysis ──
    let (spec, schemas) = kernel_graph();
    println!("[1] Legal complex graph (syscall fan-out + storage/network dual paths + 3 feedback loops + observation)");
    check("validate_deep", &spec, &schemas);

    let loops = analysis::feedback_loops(&spec);
    println!("    feedback loops: {} (legal: every loop contains ≥1 Moore machine — scheduler breaks the algebraic loop)", loops.len());
    for l in &loops {
        println!("      - machines={:?}, all_moore={}", l.machines, l.all_moore);
    }
    let spofs = analysis::single_points_of_failure(&spec);
    println!(
        "    SPOFs: {} (fully-connected graph has no source — dominator analysis needs an entry; see [2d] subgraph with an entry)",
        spofs.len()
    );
    let deg = analysis::degree_violations(&spec);
    println!(
        "    degree violations: {} (expected 0: driver uses two ports disk/net, each single-input, no fan-in overload)",
        deg.len()
    );
    let reach = analysis::reachable_from(&spec, "syscall");
    println!("    reachable from syscall: {} machines", reach.len());

    // ── 2. Invalid variants: each one detected ──
    println!("\n[2] Invalid variants → each one detected");

    // 2a. Flow-type mismatch: Observe output → Data input
    let mut s_bad = HashMap::new();
    s_bad.insert("a", PortSchema::new().with(PortDecl::output::<u64>("evt")));
    s_bad.insert(
        "b",
        PortSchema::new().with(PortDecl::input::<u64>("in")), // Data input
    );
    let bad_flow = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "evt"), ("b", "in"), buf(8)));
    print!("    flow-type mismatch (a.evt[Data] → b.in[Data] reverse intent)");
    // a's evt is a Data output; to observe a mismatch, change a's schema — construct directly:
    let mut s_obs = HashMap::new();
    s_obs.insert("a", PortSchema::new().with(PortDecl::observe::<u64>("evt")));
    s_obs.insert("b", PortSchema::new().with(PortDecl::input::<u64>("in")));
    check("Observe output → Data input", &bad_flow, &s_obs);

    // 2b. Inline loop: Inline links on the loop (synchronous-call deadlock, Moore can't save it)
    let mut s2 = HashMap::new();
    s2.insert(
        "a",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("out")),
    );
    s2.insert(
        "b",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("out")),
    );
    let inline_cycle = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()).moore())
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), LinkKind::Inline));
    check("Inline loop (a→b→a via Inline)", &inline_cycle, &s2);

    // 2c. All-non-Moore loop: all state machines on the loop → UnsafeCycle (no Moore to break latency)
    let all_stateful = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), buf(8)))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), buf(8)));
    check("all-non-Moore loop (a→b→a all stateful)", &all_stateful, &s2);

    // 2d. SPOF analysis (subgraph with a source): gateway is the single entry dominator
    let mut s3 = HashMap::new();
    s3.insert("app", PortSchema::new().with(PortDecl::output::<u64>("req")));
    s3.insert(
        "gateway",
        PortSchema::new()
            .with(PortDecl::input::<u64>("req"))
            .with(PortDecl::output::<u64>("to_store"))
            .with(PortDecl::output::<u64>("to_media"))
            .with(PortDecl::output::<u64>("to_logs")),
    );
    s3.insert("storage", PortSchema::new().with(PortDecl::input::<u64>("in")));
    s3.insert("media", PortSchema::new().with(PortDecl::input::<u64>("in")));
    s3.insert("logs", PortSchema::new().with(PortDecl::input::<u64>("in")));
    let spof_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("app", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("gateway", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("storage", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("media", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("logs", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("app", "req"), ("gateway", "req"), buf(8)))
        .with_link(LinkSpec::new(("gateway", "to_store"), ("storage", "in"), buf(8)))
        .with_link(LinkSpec::new(("gateway", "to_media"), ("media", "in"), buf(8)))
        .with_link(LinkSpec::new(("gateway", "to_logs"), ("logs", "in"), buf(8)));
    let spofs = analysis::single_points_of_failure(&spof_spec);
    println!("    SPOF analysis (app→gateway→{{store,media,logs}}):");
    assert!(!spofs.is_empty(), "gateway must be an SPOF");
    for s in &spofs {
        println!("      - {} removed → {} downstream machines unreachable", s.vertex, s.threatens.len());
    }
    assert!(
        spofs.iter().any(|s| s.vertex == "gateway"),
        "gateway is the SPOF dominating all sinks"
    );

    // 2e. Observe flow on a blocking carrier: the S3-1 matrix rejects it (an
    // observation edge must not back-pressure its source). Same schema as the
    // legal graph, but the observation edge uses a blocking BoundedBuf.
    let (_, schemas_obs) = kernel_graph();
    let observe_blocking = DynamicTopology::new()
        .with_machine(MachineInstance::new("syscall", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("perf", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("syscall", "events"), ("perf", "events"), buf(8)));
    check("Observe flow on a blocking carrier", &observe_blocking, &schemas_obs);

    // ── 3. Conclusion ──
    println!("\n[3] Conclusion");
    println!("    axiom's default model is an arbitrary directed graph: legal loops (≥1 Moore machine in each),");
    println!("    fan-out multi-port, fan-in port separation, Observe streams on non-blocking carriers — all pass");
    println!("    validate_deep; the S3-1 matrix rejects an Observe edge on a blocking carrier;");
    println!("    structural analysis (loops/SPOFs/degree/reachability) reports before deploy.");
}
