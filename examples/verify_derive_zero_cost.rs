// E90-derive 验证：derive feature 下 builtin 机器零开销
//
// 这是 E90-builtin-machines 探针的 derive 模式版本。
// 用 `cargo run --example verify-derive-zero-cost --features derive --release` 运行。

use axiom::prelude_all::*;
use axiom::builtin::{
    Identity, IdentityInput, IdentityOutput,
    Tee, TeeInput, TeeOutput,
    Latch, LatchInput, LatchOutput,
    Collector, CollectorInput, CollectorOutput,
};
use axiom::flow::FlowKind;

// ── 分配计数器 ─────────────────────────────────────────────────────────────

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
    println!("=== E90-derive: derive feature 下 builtin 机器零开销验证 ===\n");
    let mut all_ok = true;

    // ── Identity（宏生成端口） ──
    println!("Identity（#[ports] 宏生成）");
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
    all_ok &= check(&format!("Identity process 零堆分配（实际 {}）", delta), delta == 0);
    println!();

    // ── Tee（宏生成端口，双输出） ──
    println!("Tee（#[ports] 宏生成，双输出）");
    let mut state = <Tee<u32> as Machine>::init(&ctx).unwrap();

    let out = <Tee<u32> as Machine>::process(&mut state, &ctx, TeeInput::Input(100));
    match out {
        MultiOutput::YieldMulti(v) => {
            all_ok &= check("Tee YieldMulti 包含 2 个输出", v.len() == 2);
            all_ok &= check("Tee 第一项是 OutputA(100)",
                matches!(v[0], TeeOutput::OutputA(x) if x == 100));
            all_ok &= check("Tee 第二项是 OutputB(100)",
                matches!(v[1], TeeOutput::OutputB(x) if x == 100));
        }
        _ => all_ok &= check("Tee 应返回 YieldMulti", false),
    }
    println!();

    // ── Latch（宏生成端口，有状态） ──
    println!("Latch（#[ports] 宏生成，有状态）");
    let mut state = <Latch<u32> as Machine>::init(&ctx).unwrap();

    let out1 = <Latch<u32> as Machine>::process(&mut state, &ctx, LatchInput::Input(10));
    all_ok &= check("Latch 首次输入 → Idle", matches!(out1, SingleOutput::Idle));

    let out2 = <Latch<u32> as Machine>::process(&mut state, &ctx, LatchInput::Input(20));
    all_ok &= check("Latch 第二次输入 → Yield(Output(10))",
        matches!(out2, SingleOutput::Yield(LatchOutput::Output(v)) if v == 10));
    println!();

    // ── Collector（宏生成端口，Observe 流） ──
    println!("Collector（#[ports] 宏生成，Observe 流）");
    let mut state = <Collector<u32> as Machine>::init(&ctx).unwrap();

    let out1 = <Collector<u32> as Machine>::process(&mut state, &ctx, CollectorInput::Input(10));
    match &out1 {
        SingleOutput::Yield(CollectorOutput::Snapshots(snap)) => {
            all_ok &= check("Collector 第一次输入后快照长度 == 1", snap.len() == 1);
            all_ok &= check("Collector 快照内容 == [10]", snap == &vec![10u32]);
        }
        _ => all_ok &= check("Collector 应 Yield Snapshots", false),
    }

    let observe_kind = CollectorOutput::<u32>::Snapshots(vec![]).flow_kind();
    all_ok &= check("Collector Snapshots FlowKind == Observe",
        observe_kind == FlowKind::Observe);
    println!();

    // ── 汇总 ──
    println!("=== 汇总 ===");
    if all_ok {
        println!("derive feature 下 builtin 机器零开销验证全部通过 ✓");
        std::process::exit(0);
    } else {
        println!("存在失败子命题 ✗");
        std::process::exit(1);
    }
}
