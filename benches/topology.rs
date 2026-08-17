//! Benchmarks for dynamic topology operations.
//!
//! Measures:
//! - `link_on_graph`: Link operation on an existing graph (no Kahn)
//! - `link_cycle_allow`: Link that creates a cycle (now O(1), was O(V+E) Kahn)
//! - `apply_batch`: atomic batch with snapshot + rollback
//! - `apply Spawn`: single spawn operation overhead
//!
//! Run with: cargo bench --bench topology

#[path = "bench_harness.rs"]
mod bench_harness;

use bench_harness::BenchGroup;
use axiom::prelude_all::*;
use axiom::topology::{TopologyMutation, TopologyOp};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn gen_names(count: usize) -> Vec<&'static str> {
    (0..count).map(|i| leak_str(format!("m{}", i))).collect()
}

fn build_linear_chain(topo: &mut TopologyMutation, n: usize) {
    let names = gen_names(n);
    for &name in &names {
        topo.apply(TopologyOp::Spawn {
            name,
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        })
        .unwrap();
    }
    for i in 0..n.saturating_sub(1) {
        topo.apply(TopologyOp::Link {
            out: (names[i], "out"),
            into: (names[i + 1], "in"),
            kind: LinkKind::Inline,
        })
        .unwrap();
    }
}

fn build_star(topo: &mut TopologyMutation, n: usize) {
    let names = gen_names(n);
    for &name in &names {
        topo.apply(TopologyOp::Spawn {
            name,
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        })
        .unwrap();
    }
    for i in 1..n {
        topo.apply(TopologyOp::Link {
            out: (names[0], "out"),
            into: (names[i], "in"),
            kind: LinkKind::Inline,
        })
        .unwrap();
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    println!("\n═══ Benchmark: topology ═════════════════════════════════════════════════\n");

    // ── spawn ──────────────────────────────────────────────────────────────
    let mut group = BenchGroup::new("spawn_n");

    for n in [50, 100, 200] {
        let names = gen_names(n);
        group.bench(&format!("n={}", n), || {
            let mut topo = TopologyMutation::new();
            for &name in &names {
                topo.apply(TopologyOp::Spawn {
                    name,
                    machine_type: "worker",
                    physical: MachinePhysicalSpec::default(),
                    config_overrides: vec![],
                })
                .unwrap();
            }
            std::hint::black_box(topo);
        });
    }
    group.finish();

    // ── link on existing graph (no Kahn) ───────
    let mut group = BenchGroup::new("link_on_graph");

    for n in [50, 200, 500] {
        // Pre-build topology outside the measured region.
        let mut topo = TopologyMutation::new();
        build_linear_chain(&mut topo, n);
        let extra_a = leak_str(format!("extra_a_{}", n));
        let extra_b = leak_str(format!("extra_b_{}", n));
        topo.apply(TopologyOp::Spawn {
            name: extra_a,
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        })
        .unwrap();
        topo.apply(TopologyOp::Spawn {
            name: extra_b,
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        })
        .unwrap();

        group.bench(&format!("V={}", n), || {
            // Clone the topology so each iteration starts fresh.
            let mut t = topo.clone();
            t.apply(TopologyOp::Link {
                out: (extra_a, "out"),
                into: (extra_b, "in"),
                kind: LinkKind::Inline,
            })
            .unwrap();
            std::hint::black_box(t);
        });
    }
    group.finish();

    // ── link cycle allow (cycles between machines are allowed) ─
    let mut group = BenchGroup::new("link_cycle_allow");

    for n in [50, 200, 500] {
        let mut topo = TopologyMutation::new();
        build_linear_chain(&mut topo, n);
        let first = "m0";
        let last = leak_str(format!("m{}", n - 1));

        group.bench(&format!("V={}", n), || {
            let mut t = topo.clone();
            // This creates a cycle (m_last → m0) — ALLOWED.
            // Previously this ran Kahn's algorithm and rejected; now it's
            // just a link insertion (O(1) amortized, no graph traversal).
            let _ = t.apply(TopologyOp::Link {
                out: (last, "out"),
                into: (first, "in"),
                kind: LinkKind::Inline,
            });
            std::hint::black_box(t);
        });
    }
    group.finish();

    // ── apply_batch (spawn + link chain) ──────────────────────────────────
    let mut group = BenchGroup::new("apply_batch");

    for batch_size in [10, 50, 100] {
        let names = gen_names(batch_size);
        let mut ops = Vec::with_capacity(batch_size * 2);
        for &name in &names {
            ops.push(TopologyOp::Spawn {
                name,
                machine_type: "worker",
                physical: MachinePhysicalSpec::default(),
                config_overrides: vec![],
            });
        }
        for i in 0..batch_size.saturating_sub(1) {
            ops.push(TopologyOp::Link {
                out: (names[i], "out"),
                into: (names[i + 1], "in"),
                kind: LinkKind::Inline,
            });
        }

        group.bench(&format!("batch={}", batch_size), || {
            let mut topo = TopologyMutation::new();
            topo.apply_batch(ops.clone()).unwrap();
            std::hint::black_box(topo);
        });
    }
    group.finish();

    // ── apply_batch rollback (via duplicate name, since cycles are allowed) ─
    let mut group = BenchGroup::new("apply_batch_rollback");

    for batch_size in [10, 50, 100] {
        let names = gen_names(batch_size);
        let mut ops = Vec::with_capacity(batch_size * 2);
        for &name in &names {
            ops.push(TopologyOp::Spawn {
                name,
                machine_type: "worker",
                physical: MachinePhysicalSpec::default(),
                config_overrides: vec![],
            });
        }
        for i in 0..batch_size.saturating_sub(1) {
            ops.push(TopologyOp::Link {
                out: (names[i], "out"),
                into: (names[i + 1], "in"),
                kind: LinkKind::Inline,
            });
        }
        // Final op: Spawn with a duplicate name (fails, triggers rollback).
        // Previously this used a cycle to trigger rollback, but cycles are
        // now allowed. Duplicate name is a reliable failure case.
        ops.push(TopologyOp::Spawn {
            name: names[0], // duplicate
            machine_type: "worker",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        });

        group.bench(&format!("batch={}", batch_size), || {
            let mut topo = TopologyMutation::new();
            // This should fail and trigger rollback.
            let _ = topo.apply_batch(ops.clone());
            std::hint::black_box(topo);
        });
    }
    group.finish();

    // ── snapshot (clone) ──────────────────────────────────────────────────
    let mut group = BenchGroup::new("snapshot");

    for n in [50, 200, 500] {
        let mut topo = TopologyMutation::new();
        build_star(&mut topo, n);

        group.bench(&format!("V={}", n), || {
            let snap = topo.snapshot();
            std::hint::black_box(snap);
        });
    }
    group.finish();
}
