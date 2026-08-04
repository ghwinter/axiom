//! Benchmarks for port schema operations and lifecycle typestate.
//!
//! Measures:
//! - `PortSchema::find`: linear port lookup by name (O(P))
//! - `PortSchema::primary_input/primary_output`: cached O(1) lookup
//! - `PortDecl::can_link_to`: compatibility check (O(1) without session)
//! - `PortDecl::can_link_to` with session: includes `is_dual` (O(n))
//! - `MachineHandle` typestate transitions: verify zero-cost abstraction
//!
//! Run with: cargo bench --bench port

#[path = "bench_harness.rs"]
mod bench_harness;

use bench_harness::BenchGroup;
use axiom::builtin::{Identity, IdentityInput};
use axiom::machine::{Init, MachineHandle};
use axiom::prelude_all::*;

// ── Helpers ─────────────────────────────────────────────────────────────────

fn build_schema(count: usize) -> PortSchema {
    let mut schema = PortSchema::new();
    for i in 0..count - 2 {
        let name = Box::leak(format!("port_{}", i).into_boxed_str());
        schema = schema.with(PortDecl::input::<f64>(name));
    }
    let last_in = Box::leak(format!("target_in").into_boxed_str());
    schema = schema.with(PortDecl::input::<f64>(last_in));
    schema = schema.with(PortDecl::output::<f64>("target_out"));
    schema
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    println!("\n═══ Benchmark: port ══════════════════════════════════════════════════════\n");

    // ── PortSchema::find (worst case — last port) ─────────────────────────
    let mut group = BenchGroup::new("port_find_worst");

    for p in [4, 16, 64, 256] {
        let schema = build_schema(p);
        group.bench(&format!("P={}", p), || {
            let result = schema.find("target_in");
            std::hint::black_box(result);
        });
    }
    group.finish();

    // ── PortSchema::find (best case — first port) ─────────────────────────
    let mut group = BenchGroup::new("port_find_best");

    for p in [16, 64, 256] {
        let schema = build_schema(p);
        group.bench(&format!("P={}", p), || {
            let result = schema.find("port_0");
            std::hint::black_box(result);
        });
    }
    group.finish();

    // ── primary_input (cached O(1)) ───────────────────────────────────────
    let mut group = BenchGroup::new("primary_input_cached");

    for p in [4, 16, 64, 256] {
        let schema = build_schema(p);
        group.bench(&format!("P={}", p), || {
            let result = schema.primary_input();
            std::hint::black_box(result);
        });
    }
    group.finish();

    // ── primary_output (cached O(1)) ──────────────────────────────────────
    let mut group = BenchGroup::new("primary_output_cached");

    for p in [4, 16, 64, 256] {
        let schema = build_schema(p);
        group.bench(&format!("P={}", p), || {
            let result = schema.primary_output();
            std::hint::black_box(result);
        });
    }
    group.finish();

    // ── can_link_to without session (O(1)) ────────────────────────────────
    let mut group = BenchGroup::new("can_link_no_session");

    let out_port = PortDecl::output::<f64>("out");
    let in_port = PortDecl::input::<f64>("in");

    group.bench("O(1)", || {
        let result = out_port.can_link_to(&in_port);
        std::hint::black_box(result);
    });
    group.finish();

    // ── can_link_to with session (includes is_dual, O(n)) ─────────────────
    let mut group = BenchGroup::new("can_link_with_session");

    for n in [10, 100, 1000] {
        let mut a_ops = Vec::with_capacity(n + 1);
        let mut b_ops = Vec::with_capacity(n + 1);
        for i in 0..n {
            let label = Box::leak(format!("t{}", i).into_boxed_str());
            if i % 2 == 0 {
                a_ops.push(SessionOp::Send { type_name: label });
                b_ops.push(SessionOp::Recv { type_name: label });
            } else {
                a_ops.push(SessionOp::Recv { type_name: label });
                b_ops.push(SessionOp::Send { type_name: label });
            }
        }
        a_ops.push(SessionOp::End);
        b_ops.push(SessionOp::End);
        let session_a = SessionType::sequence(&a_ops);
        let session_b = SessionType::sequence(&b_ops);

        let out_port = PortDecl::output::<f64>("out").with_session(session_a);
        let in_port = PortDecl::input::<f64>("in").with_session(session_b);

        group.bench(&format!("n={}", n), || {
            let result = out_port.can_link_to(&in_port);
            std::hint::black_box(result);
        });
    }
    group.finish();

    // ── PortSchema construction (O(P) with deferred duplicate check) ─────
    let mut group = BenchGroup::new("port_schema_build");

    for p in [4, 16, 64, 256] {
        // Pre-allocate port names outside the measured region to avoid
        // leaking memory inside the hot loop (Box::leak per iteration
        // would accumulate hundreds of MB and cause OOM).
        let names: Vec<&'static str> = (0..p - 1)
            .map(|i| Box::leak(format!("p{}", i).into_boxed_str()) as &'static str)
            .collect();
        group.bench(&format!("P={}", p), || {
            let mut schema = PortSchema::new();
            for &name in &names {
                schema = schema.with(PortDecl::input::<f64>(name));
            }
            schema = schema.with(PortDecl::output::<f64>("out"));
            std::hint::black_box(schema);
        });
    }
    group.finish();

    // ── Lifecycle typestate: full cycle ───────────────────────────────────
    let mut group = BenchGroup::new("lifecycle_full");

    group.bench("new→start→process×10→stop→finish→cleanup", || {
        let ctx = MachineContext::new("bench");
        let handle = MachineHandle::<Identity<i32>, Init>::new(ctx).unwrap();
        let mut running = handle.start();
        for _ in 0..10 {
            let _ = running.process(IdentityInput::Input(42));
        }
        let stopped = running.stop().finish();
        stopped.cleanup().unwrap();
    });
    group.finish();

    // ── Lifecycle typestate: transitions only (start+stop+finish) ────────
    let mut group = BenchGroup::new("lifecycle_transitions");

    group.bench("start→stop→finish", || {
        let ctx = MachineContext::new("bench");
        let handle = MachineHandle::<Identity<i32>, Init>::new(ctx).unwrap();
        let running = handle.start();
        let stopping = running.stop();
        let stopped = stopping.finish();
        // cleanup is expensive (drop), but we want to measure transitions.
        // We can't skip it without leaking, so include it.
        let _ = stopped.cleanup();
    });
    group.finish();

    // ── Lifecycle typestate: single process() ────────────────────────────
    let mut group = BenchGroup::new("lifecycle_process");

    group.bench("process(42)", || {
        let ctx = MachineContext::new("bench");
        let handle = MachineHandle::<Identity<i32>, Init>::new(ctx).unwrap();
        let mut running = handle.start();
        let out = running.process(IdentityInput::Input(42));
        std::hint::black_box(out);
        let _ = running.stop().finish().cleanup();
    });
    group.finish();
}
