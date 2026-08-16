//! 动态路径基准：`Runtime::tick` 的动态税量化（P0：重新审视"动态税
//! 不可避免"——ID 化路由 + 免装箱 inject 后的实测）。
//!
//! 对比四种执行形态（3 级链 p1 → p2 → p3，100k 消息，release）：
//! - `dynamic (Runtime::tick, Sequential)`：动态路径（类型擦除 + 运行时拓扑）；
//! - `dynamic-fused`：注册 `FusedInline` 机器 → 融合为单 `FusedPipeline`
//!   （链内无路由/队列开销）；
//! - `static`：`Chain::run_all`（线性流式）；
//! - `handwritten`：手写循环（理想手写）。
//!
//! 语义全等（bench 内断言）。绝对数值随环境浮动，排序关系是本基准的意义。
//! 含全局分配计数器——量化每消息堆分配次数（P0：动态税的真实构成）。

#![allow(clippy::new_without_default)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

/// 计数分配器（bench 专用）：统计堆分配次数，定位动态税构成。
struct CountingAlloc;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: CountingAlloc = CountingAlloc;

use axiom::declare_ports;
use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec};
use axiom::machine::{FusedInline, Machine, SingleOutput};
use axiom::port::MachineContext;
use axiom::resource::MachinePhysicalSpec;
use axiom::static_exec::{Chain, StaticChain, StraightId, StraightMachine};
use axiom_runtime::Runtime;

#[path = "bench_harness.rs"]
mod bench_harness;
use bench_harness::BenchGroup;

declare_ports! {
    pub struct StepPorts {
        input type StepInput {
            x[Data] => i32,
        }
        output type StepOutput {
            y[Data] => i32,
        }
    }
}

/// 无状态变换机器：`y = x + 1`。可融合（SingleOutput）。
pub struct Step;
impl Machine for Step {
    type State = ();
    type Input = StepInput;
    type Output = StepOutput;
    type Ports = StepPorts;
    type ProcessOutput = SingleOutput<StepOutput>;

    fn name() -> &'static str { "step" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Self::State, axiom::machine::InitError> {
        Ok(())
    }

    fn process(
        _state: &mut Self::State,
        _ctx: &MachineContext,
        input: StepInput,
    ) -> Self::ProcessOutput {
        let StepInput::x(n) = input;
        SingleOutput::Yield(StepOutput::y(n + 1))
    }

    fn cleanup(_state: Self::State, _ctx: &MachineContext) -> Result<(), axiom::machine::CleanupError> {
        Ok(())
    }
}
impl FusedInline for Step {}

// Straight 裸载荷版（静态路径用）：y = x + 1。
impl StraightMachine for Step {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(_: &mut Self::State, n: i32) -> i32 {
        n + 1
    }
}

// ── 手写等价循环（理想手写：3 级 +1）───────────────────────────────────

fn handwritten(inputs: Vec<i32>) -> Vec<i32> {
    let mut out = Vec::with_capacity(inputs.len());
    for x in inputs {
        out.push(x + 3);
    }
    out
}

// ── 动态路径（Runtime::tick）────────────────────────────────────────────

/// 构建并物化 3 级链 Runtime。`fused` 控制是否注册 FusedInline（融合）。
fn build_runtime(fused: bool) -> Runtime {
    let mut rt = Runtime::default();
    if fused {
        rt.register_fused::<Step>("step");
    } else {
        rt.register::<Step>("step");
    }
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("p1", "step", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("p2", "step", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("p3", "step", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("p1", "y"), ("p2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("p2", "y"), ("p3", "x"), LinkKind::Inline));
    rt.materialize(&spec).expect("materialize");
    rt
}

/// tick 100k 消息：p1 输入 → 逐级传播 → p3 终端输出。
fn tick_all(rt: &mut Runtime, inputs: &[i32]) -> usize {
    let msgs: Vec<(String, String, Box<dyn core::any::Any + Send>)> = inputs
        .iter()
        .map(|n| ("p1".to_string(), "x".to_string(), Box::new(*n) as Box<dyn core::any::Any + Send>))
        .collect();
    let outputs = rt.tick(msgs).expect("tick");
    outputs.len()
}

// ── 静态路径 ─────────────────────────────────────────────────────────────

// 3 级链：Step → Step → Step（Straight 版——Step 已实现 StraightMachine）。
type StepChain = Chain<Step, Chain<Step, Step, StraightId>, StraightId>;

// ── 语义等价校验 ────────────────────────────────────────────────────────

fn verify_semantic_equivalence() {
    let src: Vec<i32> = (0..64).collect();
    // 手写
    let via_hand = handwritten(src.clone());
    // 动态
    let mut rt = build_runtime(false);
    let n_dyn = tick_all(&mut rt, &src);
    // 动态融合
    let mut rt_f = build_runtime(true);
    let n_fused = tick_all(&mut rt_f, &src);
    // 静态
    let via_static = StepChain::run_all(src).expect("pipeline");
    assert_eq!(via_hand.len(), 64, "handwritten length");
    assert_eq!(n_dyn, 64, "dynamic must yield all terminal outputs");
    assert_eq!(n_fused, 64, "fused dynamic must yield all terminal outputs");
    assert_eq!(via_static, via_hand, "static must match handwritten semantics");
}

// ── Main ─────────────────────────────────────────────────────────────────

fn main() {
    println!("\n═══ Benchmark: dynamic tax (Runtime::tick, P0) ═══════════════════════════\n");

    verify_semantic_equivalence();

    let src: Vec<i32> = (0..100_000).collect();
    let mut rt_dyn = build_runtime(false);
    let mut rt_fused = build_runtime(true);

    // 分配计数（每消息堆分配次数——动态税的真实构成）。
    // inputs 预构建（计数外），只测 tick 内部。
    let prebuilt: Vec<(String, String, Box<dyn core::any::Any + Send>)> = src
        .iter()
        .map(|n| ("p1".to_string(), "x".to_string(), Box::new(*n) as Box<dyn core::any::Any + Send>))
        .collect();
    ALLOCS.store(0, Ordering::Relaxed);
    rt_dyn.tick(prebuilt).expect("tick");
    let allocs_dyn = ALLOCS.load(Ordering::Relaxed);
    println!("  [alloc] dynamic: {} allocs / 100k msgs = {:.1} allocs/msg (3-stage)", allocs_dyn, allocs_dyn as f64 / 100_000.0);

    let prebuilt_f: Vec<(String, String, Box<dyn core::any::Any + Send>)> = src
        .iter()
        .map(|n| ("p1".to_string(), "x".to_string(), Box::new(*n) as Box<dyn core::any::Any + Send>))
        .collect();
    ALLOCS.store(0, Ordering::Relaxed);
    rt_fused.tick(prebuilt_f).expect("tick");
    let allocs_fused = ALLOCS.load(Ordering::Relaxed);
    println!("  [alloc] dynamic-fused: {} allocs / 100k msgs = {:.1} allocs/msg", allocs_fused, allocs_fused as f64 / 100_000.0);

    ALLOCS.store(0, Ordering::Relaxed);
    let _ = StepChain::run_all(src.clone()).expect("pipeline");
    let allocs_static = ALLOCS.load(Ordering::Relaxed);
    println!("  [alloc] static: {} allocs / 100k msgs = {:.3} allocs/msg", allocs_static, allocs_static as f64 / 100_000.0);

    let mut group = BenchGroup::new("dyn_3stage_100k");

    group.bench("dynamic (Runtime::tick)", || {
        let n = tick_all(&mut rt_dyn, &src);
        std::hint::black_box(n);
    });

    group.bench("dynamic-fused (FusedPipeline)", || {
        let n = tick_all(&mut rt_fused, &src);
        std::hint::black_box(n);
    });

    group.bench("static (Chain::run_all, FlowThrough)", || {
        let out = StepChain::run_all(src.clone()).expect("pipeline");
        std::hint::black_box(out);
    });

    group.bench("handwritten loop", || {
        let out = handwritten(src.clone());
        std::hint::black_box(out);
    });

    group.finish();
}
