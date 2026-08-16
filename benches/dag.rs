//! DAG 融合基准：Diamond 静态路径 vs 手写批循环。
//!
//! 验证 **DAG 融合的语义等价与零成本**：`Diamond` 在编译期把"分叉 →
//! 两路 → 汇合"摊平为单一驱动循环，单态化后无 `Box<dyn Any>`、无 trait
//! dispatch、**无端口枚举标签**（P0：StraightMachine 裸载荷直传）。
//!
//! # 验收（P0 修复后）
//!
//! - **语义等价**：`Diamond::run_all` 与手写循环逐阶段等价。
//! - **每输入成本**：静态路径应 ≈ 手写（`ε < 5%`，non-invasion axiom）。
//!   对比 P0 修复前的 ~13×（端口枚举标签税）。
//!
//! Run with: `cargo bench --bench dag`（release 模式）。

#[path = "bench_harness.rs"]
mod bench_harness;

use bench_harness::BenchGroup;
use axiom::declare_ports;
use axiom::machine::{CleanupError, InitError, Machine, SingleOutput};
use axiom::port::MachineContext;
use axiom::static_exec::{
    Diamond, StraightClone, StraightId, StraightMachine, StraightMerge,
};
use axiom::static_exec::StaticChain;

// ── 测试机器（Machine 枚举 + StraightMachine 裸载荷）────────────────────

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DoublerPorts {
        input type DoublerInput {
            x[Data] => i32,
        }
        output type DoublerOutput {
            y[Data] => i32,
        }
    }
}

pub struct Doubler;
impl Machine for Doubler {
    type State = ();
    type Input = DoublerInput;
    type Output = DoublerOutput;
    type Ports = DoublerPorts;
    type ProcessOutput = SingleOutput<DoublerOutput>;
    fn name() -> &'static str { "doubler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_: &mut (), _: &MachineContext, input: DoublerInput) -> SingleOutput<DoublerOutput> {
        match input {
            DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
        }
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}
impl StraightMachine for Doubler {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(_: &mut (), n: i32) -> i32 { n * 2 }
}

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct AdderPorts {
        input type AdderInput {
            x[Data] => i32,
        }
        output type AdderOutput {
            y[Data] => i32,
        }
    }
}

pub struct Adder;
impl Machine for Adder {
    type State = i32;
    type Input = AdderInput;
    type Output = AdderOutput;
    type Ports = AdderPorts;
    type ProcessOutput = SingleOutput<AdderOutput>;
    fn name() -> &'static str { "adder" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i32, InitError> { Ok(0) }
    fn process(state: &mut i32, _: &MachineContext, input: AdderInput) -> SingleOutput<AdderOutput> {
        match input {
            AdderInput::x(n) => {
                *state += n;
                SingleOutput::Yield(AdderOutput::y(*state))
            }
        }
    }
    fn cleanup(_: i32, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}
impl StraightMachine for Adder {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(state: &mut i32, n: i32) -> i32 {
        *state += n;
        *state
    }
}

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct TriplerPorts {
        input type TriplerInput {
            x[Data] => i32,
        }
        output type TriplerOutput {
            y[Data] => i32,
        }
    }
}

pub struct Tripler;
impl Machine for Tripler {
    type State = ();
    type Input = TriplerInput;
    type Output = TriplerOutput;
    type Ports = TriplerPorts;
    type ProcessOutput = SingleOutput<TriplerOutput>;
    fn name() -> &'static str { "tripler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_: &mut (), _: &MachineContext, input: TriplerInput) -> SingleOutput<TriplerOutput> {
        match input {
            TriplerInput::x(n) => SingleOutput::Yield(TriplerOutput::y(n * 3)),
        }
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}
impl StraightMachine for Tripler {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(_: &mut (), n: i32) -> i32 { n * 3 }
}

// ── 裸汇合（StraightMerge）──────────────────────────────────────────────

struct Sum;
impl StraightMerge<i32, i32> for Sum {
    type Output = i32;
    #[inline]
    fn merge(a: i32, b: i32) -> i32 { a + b }
}

/// 菱形：Doubler → StraightClone → (Adder, Tripler) → Sum → Adder。
type DiamondShape = Diamond<
    Doubler,
    Adder,
    Tripler,
    Adder,
    StraightClone,
    StraightId,
    StraightId,
    Sum,
>;

// ── 手写等价循环 ─────────────────────────────────────────────────────────

/// 手写等价循环：批量模型（1 个 out Vec，值直接流过，无中间中转）。
///
/// 每个输入 `x`：Doubler `d = 2x` → StraightClone `(d,d)` → 左臂 Adder
/// 累加 → 右臂 Tripler `3d` → Sum 求和 → 下游 Adder 累加。
fn handwritten(inputs: Vec<i32>) -> Vec<i32> {
    let mut acc_left = 0i32;
    let mut acc_down = 0i32;
    let mut out = Vec::with_capacity(inputs.len());
    for x in inputs {
        let d = x * 2;
        acc_left += d;
        let merged = acc_left + d * 3;
        acc_down += merged;
        out.push(acc_down);
    }
    out
}

/// 流式 Diamond（范式验证）：每台机器 State 一次初始化，单 for 循环
/// 嵌套调用——执行形态与手写同构（无中间 Vec 中转）。
///
/// 这是"流式直通"范式的可行性证明：若它 ≈ 手写，则 StaticChain 从
/// "Vec 中转批量递归"革新为"线性流式"后可达 ε→0。
fn diamond_stream(inputs: Vec<i32>) -> Vec<i32> {
    let mut sa: () = ();
    let mut sl: i32 = 0;
    let mut sr: () = ();
    let mut sd: i32 = 0;
    let mut out = Vec::with_capacity(inputs.len());
    for x in inputs {
        // 机器 A（Doubler）
        let _ = &mut sa;
        let a = x * 2;
        // StraightClone：split
        let (l, r) = (a, a);
        // 左臂（Adder）
        sl += l;
        let lo = sl;
        // 右臂（Tripler）
        let _ = &mut sr;
        let ro = r * 3;
        // Sum：merge
        let m = lo + ro;
        // 下游（Adder）
        sd += m;
        out.push(sd);
    }
    out
}

// ── 语义等价校验（bench 前的正确性门）───────────────────────────────────

fn verify_semantic_equivalence() {
    let src: Vec<i32> = (0..50).collect();
    let via_diamond = DiamondShape::run_all(src.clone()).expect("diamond");
    let via_hand = handwritten(src.clone());
    let via_stream = diamond_stream(src);
    assert_eq!(via_diamond, via_hand, "Diamond must match handwritten semantics");
    assert_eq!(
        via_diamond, via_stream,
        "streaming Diamond must match batch Diamond semantics"
    );
}

// ── Main ─────────────────────────────────────────────────────────────────

fn main() {
    println!("\n═══ Benchmark: dag fusion (Straight, P0) ════════════════════════════════\n");

    verify_semantic_equivalence();

    // 大 batch 摊销固定开销（init/cleanup）——"每输入成本"的对比。
    let src: Vec<i32> = (0..100_000).collect();

    let mut group = BenchGroup::new("diamond_100k");

    group.bench("static_path (Diamond, straight)", || {
        let out = DiamondShape::run_all(src.clone()).expect("diamond");
        std::hint::black_box(out);
    });

    group.bench("handwritten loop", || {
        let out = handwritten(src.clone());
        std::hint::black_box(out);
    });

    group.bench("streaming (flow-through, paradigm)", || {
        let out = diamond_stream(src.clone());
        std::hint::black_box(out);
    });

    group.finish();
}
