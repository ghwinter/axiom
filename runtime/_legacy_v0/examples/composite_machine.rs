//! composite_machine — 复合 Machine 示例：把子拓扑封装为单一 `machine_type`。
//!
//! 展示 `register_composite` 的完整用法：声明子拓扑 + 端口映射 → 注册 →
//! 物化时递归展开。包含单层复合与嵌套复合两个场景。
//!
//! 运行：
//!   cargo run --manifest-path runtime/Cargo.toml --example composite_machine
//!
//! # 场景
//!
//! 定义两个原子机器：
//! - `AddTen`：i32 + 10
//! - `Triple`：i32 × 3
//!
//! 复合 `amp_shift` = AddTen → Triple（先偏置再放大）：
//!   外部端口 "in" → AddTen.x，"out" → Triple.y
//!   语义：x → (x + 10) × 3
//!
//! 嵌套复合 `double_amp` = amp_shift → amp_shift（两轮放大）：
//!   外部端口 "in" → 第一个 amp_shift.in，"out" → 第二个 amp_shift.out
//!   语义：x → ((x + 10) × 3 + 10) × 3
//!
//! 主拓扑：entry(AddTen) → double_amp → sink(Triple)
//!   输入 5：entry → 15 → double_amp → ((15+10)×3+10)×3 = 75×3 = 255 → sink → 765

use axiom::declare_ports;
use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec};
use axiom::machine::Machine;
use axiom::port::MachineContext;
use axiom::resource::MachinePhysicalSpec;

use axiom_runtime::{CompositeSpec, ProcessResult, Runtime, RuntimeConfig};

// ════════════════════════════════════════════════════════════════════════
// 原子机器
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    pub struct AddTenPorts {
        input type AddTenInput { x[Data] => i32 }
        output type AddTenOutput { y[Data] => i32 }
    }
}

pub struct AddTen;
impl Machine for AddTen {
    type State = ();
    type Input = AddTenInput;
    type Output = AddTenOutput;
    type Ports = AddTenPorts;
    type ProcessOutput = axiom::machine::SingleOutput<AddTenOutput>;
    fn name() -> &'static str { "add_ten" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_ctx: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
    fn process(_s: &mut Self::State, _ctx: &MachineContext, input: AddTenInput) -> Self::ProcessOutput {
        match input {
            AddTenInput::x(n) => axiom::machine::SingleOutput::Yield(AddTenOutput::y(n + 10)),
        }
    }
    fn cleanup(_s: Self::State, _ctx: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}
impl axiom::machine::FusedInline for AddTen {}

declare_ports! {
    pub struct TriplePorts {
        input type TripleInput { x[Data] => i32 }
        output type TripleOutput { y[Data] => i32 }
    }
}

pub struct Triple;
impl Machine for Triple {
    type State = ();
    type Input = TripleInput;
    type Output = TripleOutput;
    type Ports = TriplePorts;
    type ProcessOutput = axiom::machine::SingleOutput<TripleOutput>;
    fn name() -> &'static str { "triple" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_ctx: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
    fn process(_s: &mut Self::State, _ctx: &MachineContext, input: TripleInput) -> Self::ProcessOutput {
        match input {
            TripleInput::x(n) => axiom::machine::SingleOutput::Yield(TripleOutput::y(n * 3)),
        }
    }
    fn cleanup(_s: Self::State, _ctx: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}
impl axiom::machine::FusedInline for Triple {}

// ════════════════════════════════════════════════════════════════════════
// 复合定义
// ════════════════════════════════════════════════════════════════════════

/// `amp_shift` 复合 = AddTen → Triple。
/// 语义：x → (x + 10) × 3
fn amp_shift_composite() -> CompositeSpec {
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("add", "add_ten", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("mul", "triple", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("add", "y"), ("mul", "x"), LinkKind::Inline));
    CompositeSpec::new(spec)
        .with_input("in", "add", "x")
        .with_output("out", "mul", "y")
}

/// `double_amp` 嵌套复合 = amp_shift → amp_shift。
/// 语义：x → ((x + 10) × 3 + 10) × 3
fn double_amp_composite() -> CompositeSpec {
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a1", "amp_shift", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("a2", "amp_shift", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a1", "out"), ("a2", "in"), LinkKind::Inline));
    CompositeSpec::new(spec)
        .with_input("in", "a1", "in")
        .with_output("out", "a2", "out")
}

// ════════════════════════════════════════════════════════════════════════
// 辅助：提取 i32 输出
// ════════════════════════════════════════════════════════════════════════

fn extract_i32(results: &[ProcessResult]) -> Vec<i32> {
    results.iter().filter_map(|r| match r {
        ProcessResult::Yield { value, .. } => value.downcast_ref::<i32>().copied(),
        _ => None,
    }).collect()
}

// ════════════════════════════════════════════════════════════════════════
// 场景 1：单层复合
// ════════════════════════════════════════════════════════════════════════

fn scenario_single_layer() {
    println!("── 场景 1：单层复合 amp_shift ─────────────────────────────────");
    // 拓扑：entry(AddTen) → comp(amp_shift)
    // 展开后：entry → comp.add → comp.mul
    // 输入 5：5 → entry(+10)=15 → comp.add(+10)=25 → comp.mul(×3)=75
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "add_ten", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "amp_shift", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<AddTen>("add_ten");
    rt.register::<Triple>("triple");
    rt.register_composite("amp_shift", amp_shift_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    println!("  展开后机器数: {} (entry + comp.add + comp.mul)", topo.machines.len());
    assert_eq!(topo.machines.len(), 3);
    println!("  展开后链接数: {} (entry→comp.add + comp.add→comp.mul)", topo.links.len());
    assert_eq!(topo.links.len(), 2);

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(5i32))])
        .expect("tick");
    let vals = extract_i32(&out);
    println!("  输入 5 → 输出 {:?}（期望 [75]：5+10=15 → 15+10=25 → 25×3=75）", vals);
    assert_eq!(vals, vec![75]);
}

// ════════════════════════════════════════════════════════════════════════
// 场景 2：嵌套复合（double_amp = amp_shift → amp_shift）
// ════════════════════════════════════════════════════════════════════════

fn scenario_nested() {
    println!("\n── 场景 2：嵌套复合 double_amp ────────────────────────────────");
    // 拓扑：entry(AddTen) → quad(double_amp) → sink(Triple)
    // double_amp 展开后：quad.a1(amp_shift) → quad.a2(amp_shift)
    // 每个 amp_shift 再展开：a1.add → a1.mul → a2.add → a2.mul
    // 完整链：entry → quad.a1.add → quad.a1.mul → quad.a2.add → quad.a2.mul → sink
    // 6 台机器：entry + 4 个子机器 + sink
    //
    // 输入 5：
    //   entry: 5+10 = 15
    //   a1.add: 15+10 = 25
    //   a1.mul: 25×3 = 75
    //   a2.add: 75+10 = 85
    //   a2.mul: 85×3 = 255
    //   sink: 255×3 = 765
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "add_ten", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("quad", "double_amp", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("sink", "triple", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("quad", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("quad", "out"), ("sink", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<AddTen>("add_ten");
    rt.register::<Triple>("triple");
    rt.register_composite("amp_shift", amp_shift_composite());
    rt.register_composite("double_amp", double_amp_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    println!("  展开后机器数: {} (entry + 4 子机器 + sink)", topo.machines.len());
    assert_eq!(topo.machines.len(), 6);
    println!("  展开后链接数: {} (entry→a1.add + 3 内部 + a2.mul→sink)", topo.links.len());
    assert_eq!(topo.links.len(), 5);

    // 打印展开后的机器名，展示名字空间化
    let mut names: Vec<&str> = topo.machines.keys().map(|s| s.as_str()).collect();
    names.sort();
    println!("  展开后机器名: {names:?}");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(5i32))])
        .expect("tick");
    let vals = extract_i32(&out);
    println!("  输入 5 → 输出 {:?}（期望 [765]）", vals);
    assert_eq!(vals, vec![765]);
}

// ════════════════════════════════════════════════════════════════════════
// 场景 3：跨复合边界融合
// ════════════════════════════════════════════════════════════════════════

fn scenario_fusion_across_boundary() {
    println!("\n── 场景 3：跨复合边界融合 ────────────────────────────────────");
    // register_fused 注册 AddTen/Triple，复合内部的 Inline 链与外部链
    // 都是 FusedInline + Inline → 展开后融合为单个 FusedPipeline。
    // 场景 1 的拓扑（entry → comp(amp_shift)）展开后 3 台机器，
    // 融合后应降为 1 个 FusedPipeline。
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "add_ten", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "amp_shift", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<AddTen>("add_ten");
    rt.register_fused::<Triple>("triple");
    rt.register_composite("amp_shift", amp_shift_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    println!("  融合后机器数: {} (3 台 → 1 个 FusedPipeline)", topo.machines.len());
    assert_eq!(topo.machines.len(), 1, "复合边界对融合透明");
    println!("  融合后链接数: {} (链内链接被 FusedPipeline 吸收)", topo.links.len());
    assert_eq!(topo.links.len(), 0);

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(5i32))])
        .expect("tick");
    let vals = extract_i32(&out);
    println!("  输入 5 → 输出 {:?}（与场景 1 一致：[75]）", vals);
    assert_eq!(vals, vec![75]);
}

// ════════════════════════════════════════════════════════════════════════
// main
// ════════════════════════════════════════════════════════════════════════

fn main() {
    println!("axiom-runtime 复合 Machine 示例");
    println!("═══════════════════════════════════════════════════════════════");

    scenario_single_layer();
    scenario_nested();
    scenario_fusion_across_boundary();

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("完成。复合 Machine 把子拓扑封装为单一 machine_type，物化时递归");
    println!("展开（名字空间化 + 链接重定向），展开在融合之前——FusedPipeline");
    println!("可跨原复合边界融合。");
}
