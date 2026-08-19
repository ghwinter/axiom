//! declarative_dag — pure core capability demo: declarative arbitrary topologies
//! + composite machines + validation + analysis.
//!
//! This example **does not depend on the runtime** — it uses only axiom core's
//! structural-definition capabilities:
//!
//! 1. `DynamicTopology` declares a fan-out + fan-in DAG (non-linear);
//! 2. `CompositeSpec` defines a composite machine (sub-topology + port mapping);
//! 3. `expand_composites` expands composites into a flat topology;
//! 4. `validate_deep` validates port types/directions/degree constraints/cycle safety;
//! 5. `analyze` performs graph-theoretic analysis (SCC/SPOF/orphan/observability).
//!
//! # Topology
//!
//! Before expansion (with the composite `scaler` instance):
//!
//! ```text
//!                  ┌─► scaler (composite) ─┐
//! src ──► split ───┤                       ├─► merge
//!                  └─► doubler_b ──────────┘
//!                          │
//!                          ▼
//!                      observer (observe port, no consumer)
//! ```
//!
//! After the `scaler` composite is expanded (`doubler_a` is wrapped inside it):
//!
//! ```text
//!                          ┌─► scaler.doubler_a ─┐
//! src ──► split ───────────┤                     ├─► merge
//!                          └─► doubler_b ────────┘
//!                                  │
//!                                  ▼
//!                              observer
//! ```
//!
//! This topology contains: fan-out (split → two paths), fan-in (two paths → merge),
//! a composite machine (scaler), and an observe port (doubler_b::status). It is not a
//! linear chain — `pipeline2`/`pipeline3` cannot express it; it needs a `fanout2` +
//! `fanin2` combination or the dynamic path.
//!
//! # Running
//!
//! ```sh
//! cargo run --example declarative_dag
//! ```

use axiom::compat::HashMap;
use axiom::composite::{expand_composites, CompositeSpec};
use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, WritePolicy, ReadPolicy};
use axiom::port::{PortDecl, PortSchema};
use axiom::resource::MachinePhysicalSpec;

use std::collections::BTreeMap;

// ════════════════════════════════════════════════════════════════════════════
// Port schemas — simulate the port declarations of real Machines (no Machine impl needed)
// ════════════════════════════════════════════════════════════════════════════

/// `src` machine: no input, one i32 output port `out`.
fn src_schema() -> PortSchema {
    PortSchema::new().with(PortDecl::output::<i32>("out"))
}

/// `split` machine: one i32 input `in`, two i32 outputs `a`/`b` (fan-out source).
fn split_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<i32>("a"))
        .with(PortDecl::output::<i32>("b"))
}

/// `doubler` machine: one i32 input `x`, one i32 output `y`.
/// This is the sub-machine inside the composite `scaler`, and also the external `doubler_b`.
fn doubler_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("x"))
        .with(PortDecl::output::<i32>("y"))
}

/// `doubler_b` machine: like doubler, plus an observe port `status`.
fn doubler_b_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("x"))
        .with(PortDecl::output::<i32>("y"))
        .with(PortDecl::observe::<i32>("status"))
}

/// `merge` machine: two i32 inputs `a`/`b`, one i32 output `out` (fan-in sink).
fn merge_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("a"))
        .with(PortDecl::input::<i32>("b"))
        .with(PortDecl::output::<i32>("out"))
}

// ════════════════════════════════════════════════════════════════════════════
// Composite machine definition
// ════════════════════════════════════════════════════════════════════════════

/// The `scaler` composite: inside is a `doubler` sub-machine, with external port `in`→sub `x`
/// and `out`←sub `y`.
///
/// This shows that core can define nested topologies on its own — no runtime needed.
fn scaler_composite() -> CompositeSpec {
    let sub_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("doubler_a", "doubler", MachinePhysicalSpec::default()));
    CompositeSpec::new(sub_spec)
        .with_input("in", "doubler_a", "x")
        .with_output("out", "doubler_a", "y")
}

// ════════════════════════════════════════════════════════════════════════════
// Main flow
// ════════════════════════════════════════════════════════════════════════════

fn main() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  axiom core — declarative DAG + composite + validate + analyze  ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    // ── 1. Build a DynamicTopology (declarative topology, with a composite instance) ──
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("src", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("split", "split", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("scaler", "scaler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("doubler_b", "doubler_b", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("merge", "merge", MachinePhysicalSpec::default()))
        // src → split
        .with_link(LinkSpec::new(("src", "out"), ("split", "in"), LinkKind::Inline))
        // split → scaler (fan-out branch A)
        .with_link(LinkSpec::new(
            ("split", "a"), ("scaler", "in"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // split → doubler_b (fan-out branch B)
        .with_link(LinkSpec::new(
            ("split", "b"), ("doubler_b", "x"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // scaler → merge (fan-in branch A)
        .with_link(LinkSpec::new(("scaler", "out"), ("merge", "a"), LinkKind::Inline))
        // doubler_b → merge (fan-in branch B)
        .with_link(LinkSpec::new(("doubler_b", "y"), ("merge", "b"), LinkKind::Inline));

    println!("── 1. DynamicTopology (with composite instance scaler) ──────────────");
    println!("  machines: {}", spec.machines.len());
    println!("  links:    {}", spec.links.len());
    for m in &spec.machines {
        println!("    • {} ({})", m.name, m.machine_type);
    }
    println!();

    // ── 2. Validate the composite definition ──
    println!("── 2. CompositeSpec::validate (scaler) ────────────────────────");
    let scaler = scaler_composite();
    match scaler.validate() {
        Ok(()) => println!("  ✓ scaler composite validates (input_map + output_map complete)"),
        Err(e) => {
            println!("  ✗ scaler composite validation failed: {e}");
            return;
        }
    }
    println!();

    // ── 3. Expand composites ──
    println!("── 3. expand_composites (scaler → scaler.doubler_a) ───────────");
    let mut composites = BTreeMap::new();
    composites.insert("scaler".to_string(), scaler);
    let (expanded_machines, expanded_links) =
        expand_composites(spec.machines.clone(), spec.links.clone(), &composites)
            .expect("expand_composites");
    println!("  before expansion: {} machines, {} links", spec.machines.len(), spec.links.len());
    println!("  after expansion:  {} machines, {} links", expanded_machines.len(), expanded_links.len());
    for m in &expanded_machines {
        println!("    • {} ({})", m.name, m.machine_type);
    }
    println!();

    // ── 4. Build the expanded DynamicTopology and validate it ──
    println!("── 4. validate_deep (port types/directions/degree constraints/cycle safety) ──");
    let expanded_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("src", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("split", "split", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("scaler.doubler_a", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("doubler_b", "doubler_b", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("merge", "merge", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("src", "out"), ("split", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("split", "a"), ("scaler.doubler_a", "x"), LinkKind::BoundedBuf {
            capacity: 16, write_policy: WritePolicy::Blocking, read_policy: ReadPolicy::Blocking,
        }))
        .with_link(LinkSpec::new(("split", "b"), ("doubler_b", "x"), LinkKind::BoundedBuf {
            capacity: 16, write_policy: WritePolicy::Blocking, read_policy: ReadPolicy::Blocking,
        }))
        .with_link(LinkSpec::new(("scaler.doubler_a", "y"), ("merge", "a"), LinkKind::Inline))
        .with_link(LinkSpec::new(("doubler_b", "y"), ("merge", "b"), LinkKind::Inline));

    let mut schemas = HashMap::new();
    schemas.insert("src", src_schema());
    schemas.insert("split", split_schema());
    schemas.insert("scaler.doubler_a", doubler_schema());
    schemas.insert("doubler_b", doubler_b_schema());
    schemas.insert("merge", merge_schema());

    match expanded_spec.validate_deep(&schemas) {
        Ok(()) => println!("  ✓ validate_deep passed — port types/directions/degree constraints/cycle safety all pass"),
        Err(e) => println!("  ✗ validate_deep failed: {e}"),
    }
    println!();

    // ── 5. Graph-theoretic analysis ──
    println!("── 5. analyze (SCC/SPOF/orphan/observability) ─────────────────");
    let report = expanded_spec.analyze(Some(&schemas));
    if report.is_clean() {
        println!("  ✓ topology is clean — no advisory warnings");
    } else {
        println!("  {} advisory warning(s):", report.len());
        for warning in report.iter() {
            println!("    ⚠ {warning}");
        }
    }
    println!();

    // ── 6. Bonus: demonstrate the anti-narrowing rule validation ──
    println!("── 6. Anti-narrowing rule validation ───────────────────────────");
    println!("  This topology contains:");
    println!("    • fan-out  (split → {{scaler, doubler_b}})");
    println!("    • fan-in   ({{scaler, doubler_b}} → merge)");
    println!("    • a composite machine (scaler → scaler.doubler_a)");
    println!("    • multiple machine types (src/split/doubler/doubler_b/merge)");
    println!("    • multiple link physical semantics (Inline + BoundedBuf)");
    println!("  pipeline2/pipeline3 cannot express this topology — they only support A→B→C.");
    println!("  This example proves that core's DynamicTopology can express arbitrary DAGs.");
    println!();

    // ── 7. Bonus: demonstrate degree-constraint violations being caught ──
    println!("── 7. Degree-constraint violation detection (Inline outdeg ≤ 1) ─");
    let bad_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "split", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("c", "split", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("a", "out"), ("c", "in"), LinkKind::Inline));
    let mut bad_schemas = HashMap::new();
    bad_schemas.insert("a", src_schema());
    bad_schemas.insert("b", split_schema());
    bad_schemas.insert("c", split_schema());
    match bad_spec.validate_deep(&bad_schemas) {
        Ok(()) => println!("  ✗ should have rejected Inline fan-out (bug!)"),
        Err(e) => println!("  ✓ correctly rejected Inline outdeg=2: {e}"),
    }
    println!();

    println!("══════════════════════════════════════════════════════════════════");
    println!("  core can define, validate, and analyze arbitrary topologies on its own — no runtime needed.");
    println!("  The runtime's job is execution, not definition.");
    println!("══════════════════════════════════════════════════════════════════");
}
