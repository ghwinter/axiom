// E90-derive verification: builtin machines are zero-overhead under the derive feature
//
// This is the derive-mode version of the E90-builtin-machines probe.
// Run with `cargo run --example verify-derive-zero-cost --features derive --release`.

use axiom::prelude_all::*;
use axiom::builtin::{
    Identity, IdentityInput, IdentityOutput,
    Tee, TeeInput, TeeOutput,
    Latch, LatchInput, LatchOutput,
    Collector, CollectorInput, CollectorOutput,
};
use axiom::flow::FlowKind;

// ── Allocation counter ────────────────────────────────────────────────────

use std::alloc::{GlobalAlloc, Layout, System};

struct CountingAlloc;

static ALLOC_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn alloc_count() -> usize {
    ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

fn alloc_delta<F: FnOnce() -> R, R>(f: F) -> (R, usize) {
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    let before = alloc_count();
    let r = f();
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    let after = alloc_count();
    (r, after.saturating_sub(before))
}

fn check(label: &str, ok: bool) -> bool {
    println!("  {:<55} {}", label, if ok { "✓" } else { "✗" });
    ok
}

fn main() {
    println!("=== E90-derive: zero-overhead verification of builtin machines under the derive feature ===\n");
    let mut all_ok = true;

    // ── Identity (macro-generated ports) ──
    println!("Identity (#[ports] macro-generated)");
    let ctx = MachineContext::new("test");
    let mut state = <Identity<u32> as Machine>::init(&ctx).unwrap();

    let input = IdentityInput::Input(42u32);
    let out = <Identity<u32> as Machine>::process(&mut state, &ctx, input);
    all_ok &= check("Identity Input(42) → Yield(Output(42))",
        matches!(out, SingleOutput::Yield(IdentityOutput::Output(v)) if v == 42));

    let input = IdentityInput::Input(7u32);
    let (_, delta) = alloc_delta(|| {
        <Identity<u32> as Machine>::process(&mut state, &ctx, input)
    });
    all_ok &= check(&format!("Identity process zero heap allocations (actual {})", delta), delta == 0);
    println!();

    // ── Tee (macro-generated ports, dual output) ──
    println!("Tee (#[ports] macro-generated, dual output)");
    let mut state = <Tee<u32> as Machine>::init(&ctx).unwrap();

    let out = <Tee<u32> as Machine>::process(&mut state, &ctx, TeeInput::Input(100));
    match out {
        MultiOutput::YieldMulti(v) => {
            all_ok &= check("Tee YieldMulti contains 2 outputs", v.len() == 2);
            all_ok &= check("Tee first item is OutputA(100)",
                matches!(v[0], TeeOutput::OutputA(x) if x == 100));
            all_ok &= check("Tee second item is OutputB(100)",
                matches!(v[1], TeeOutput::OutputB(x) if x == 100));
        }
        _ => all_ok &= check("Tee should return YieldMulti", false),
    }
    println!();

    // ── Latch (macro-generated ports, stateful) ──
    println!("Latch (#[ports] macro-generated, stateful)");
    let mut state = <Latch<u32> as Machine>::init(&ctx).unwrap();

    let out1 = <Latch<u32> as Machine>::process(&mut state, &ctx, LatchInput::Input(10));
    all_ok &= check("Latch first input → Idle", matches!(out1, SingleOutput::Idle));

    let out2 = <Latch<u32> as Machine>::process(&mut state, &ctx, LatchInput::Input(20));
    all_ok &= check("Latch second input → Yield(Output(10))",
        matches!(out2, SingleOutput::Yield(LatchOutput::Output(v)) if v == 10));
    println!();

    // ── Collector (macro-generated ports, Observe stream) ──
    println!("Collector (#[ports] macro-generated, Observe stream)");
    let mut state = <Collector<u32> as Machine>::init(&ctx).unwrap();

    let out1 = <Collector<u32> as Machine>::process(&mut state, &ctx, CollectorInput::Input(10));
    match &out1 {
        SingleOutput::Yield(CollectorOutput::Snapshots(snap)) => {
            all_ok &= check("Collector snapshot length == 1 after first input", snap.len() == 1);
            all_ok &= check("Collector snapshot content == [10]", snap == &vec![10u32]);
        }
        _ => all_ok &= check("Collector should Yield Snapshots", false),
    }

    let observe_kind = CollectorOutput::<u32>::Snapshots(vec![]).flow_kind();
    all_ok &= check("Collector Snapshots FlowKind == Observe",
        observe_kind == FlowKind::Observe);
    println!();

    // ── Summary ──
    println!("=== Summary ===");
    if all_ok {
        println!("zero-overhead verification of builtin machines under the derive feature: all passed ✓");
        std::process::exit(0);
    } else {
        println!("some sub-assertions failed ✗");
        std::process::exit(1);
    }
}
