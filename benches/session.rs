//! Benchmarks for session type operations.
//!
//! Measures:
//! - `is_dual`: binary session type duality check (linear scan)
//! - `project`: MPST global→local projection
//! - `is_consistent`: global type consistency check (calls project twice per message)
//!
//! Run with: cargo bench --bench session

#[path = "bench_harness.rs"]
mod bench_harness;

use bench_harness::BenchGroup;
use axiom::session::{
    is_consistent, is_dual, project, GlobalOp, GlobalType, SessionOp, SessionType,
};

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a linear dual pair: alternating Send/Recv of increasing length.
fn build_dual_pair(n: usize) -> (SessionType, SessionType) {
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
    (SessionType::sequence(&a_ops), SessionType::sequence(&b_ops))
}

/// Build a linear global type with n messages among 3 roles.
fn build_linear_global(n: usize) -> GlobalType {
    let roles = ["role_a", "role_b", "role_c"];
    let mut ops = Vec::with_capacity(n + 1);
    for i in 0..n {
        let from = roles[i % 3];
        let to = roles[(i + 1) % 3];
        let label = Box::leak(format!("msg{}", i).into_boxed_str());
        ops.push(GlobalOp::Message { from, to, label });
    }
    ops.push(GlobalOp::End);
    GlobalType::sequence(&ops)
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    println!("\n═══ Benchmark: session ═════════════════════════════════════════════════\n");

    // ── is_dual ─────────────────────────────────────────────────────────────
    let mut group = BenchGroup::new("is_dual");

    for n in [10, 100, 1000, 5000] {
        let (a, b) = build_dual_pair(n);
        group.bench(&format!("n={}", n), || {
            let result = is_dual(&a, &b);
            std::hint::black_box(result);
        });
    }
    group.finish();

    // ── is_dual mismatch (early exit) ──────────────────────────────────────
    let mut group = BenchGroup::new("is_dual_mismatch");

    for n in [100, 1000, 5000] {
        let (_, b) = build_dual_pair(n);
        // Build a broken session: first op is Recv where b expects Send.
        let mut a_ops = Vec::with_capacity(n + 1);
        for i in 0..n {
            let label = Box::leak(format!("t{}", i).into_boxed_str());
            // All Recv — mismatch on first op.
            a_ops.push(SessionOp::Recv { type_name: label });
        }
        a_ops.push(SessionOp::End);
        let a_broken = SessionType::sequence(&a_ops);

        group.bench(&format!("n={}", n), || {
            let result = is_dual(&a_broken, &b);
            std::hint::black_box(result);
        });
    }
    group.finish();

    // ── project ─────────────────────────────────────────────────────────────
    let mut group = BenchGroup::new("project");

    for n in [10, 100, 1000, 5000] {
        let global = build_linear_global(n);
        group.bench(&format!("n={}", n), || {
            let local = project(&global, "role_a");
            std::hint::black_box(local);
        });
    }
    group.finish();

    // ── is_consistent ──────────────────────────────────────────────────────
    let mut group = BenchGroup::new("is_consistent");

    for n in [10, 50, 100, 500] {
        let global = build_linear_global(n);
        group.bench(&format!("n={}", n), || {
            let result = is_consistent(&global);
            std::hint::black_box(result);
        });
    }
    group.finish();
}
