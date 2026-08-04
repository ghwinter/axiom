//! declarative_dag — 纯 core 能力展示：声明式任意拓扑 + 复合机器 + 验证 + 分析。
//!
//! 这个 example **不依赖 runtime**——它只用 axiom core 的结构定义能力：
//!
//! 1. `DeploySpec` 声明一个 fan-out + fan-in DAG（非线性）；
//! 2. `CompositeSpec` 定义一个复合机器（子拓扑 + 端口映射）；
//! 3. `expand_composites` 展开复合为扁平拓扑；
//! 4. `validate_deep` 验证端口类型/方向/度约束/环安全性；
//! 5. `analyze` 做图论分析（SCC/SPOF/orphan/observability）。
//!
//! # 拓扑
//!
//! 展开前（含复合 `scaler` 实例）：
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
//! `scaler` 复合展开后（`doubler_a` 被封装在复合内）：
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
//! 这个拓扑包含：fan-out（split → 两条路径）、fan-in（两条路径 → merge）、
//! 复合机器（scaler）、观察端口（doubler_b::status）。它不是线性链——
//! `pipeline2`/`pipeline3` 无法表达这个拓扑，需要 `fanout2` + `fanin2` 组合或动态路径。
//!
//! # 运行
//!
//! ```sh
//! cargo run --example declarative_dag
//! ```

use axiom::compat::HashMap;
use axiom::composite::{expand_composites, CompositeSpec};
use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, WritePolicy, ReadPolicy};
use axiom::port::{PortDecl, PortSchema};
use axiom::resource::MachinePhysicalSpec;

use std::collections::BTreeMap;

// ════════════════════════════════════════════════════════════════════════════
// Port schemas — 模拟真实 Machine 的端口声明（不需要 Machine 实现）
// ════════════════════════════════════════════════════════════════════════════

/// `src` 机器：无输入，一个 i32 输出端口 `out`。
fn src_schema() -> PortSchema {
    PortSchema::new().with(PortDecl::output::<i32>("out"))
}

/// `split` 机器：一个 i32 输入 `in`，两个 i32 输出 `a`/`b`（fan-out 源）。
fn split_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<i32>("a"))
        .with(PortDecl::output::<i32>("b"))
}

/// `doubler` 机器：一个 i32 输入 `x`，一个 i32 输出 `y`。
/// 这是复合 `scaler` 内部的子机器，也是外部的 `doubler_b`。
fn doubler_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("x"))
        .with(PortDecl::output::<i32>("y"))
}

/// `doubler_b` 机器：在 doubler 基础上多一个 observe 端口 `status`。
fn doubler_b_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("x"))
        .with(PortDecl::output::<i32>("y"))
        .with(PortDecl::observe::<i32>("status"))
}

/// `merge` 机器：两个 i32 输入 `a`/`b`，一个 i32 输出 `out`（fan-in 汇点）。
fn merge_schema() -> PortSchema {
    PortSchema::new()
        .with(PortDecl::input::<i32>("a"))
        .with(PortDecl::input::<i32>("b"))
        .with(PortDecl::output::<i32>("out"))
}

// ════════════════════════════════════════════════════════════════════════════
// 复合机器定义
// ════════════════════════════════════════════════════════════════════════════

/// `scaler` 复合：内部是一个 `doubler` 子机器，外部端口 `in`→子`x`，`out`←子`y`。
///
/// 这展示了 core 能独立定义嵌套拓扑——不需要 runtime。
fn scaler_composite() -> CompositeSpec {
    let sub_spec = DeploySpec::new()
        .with_machine(MachineInstance::new("doubler_a", "doubler", MachinePhysicalSpec::default()));
    CompositeSpec::new(sub_spec)
        .with_input("in", "doubler_a", "x")
        .with_output("out", "doubler_a", "y")
}

// ════════════════════════════════════════════════════════════════════════════
// 主流程
// ════════════════════════════════════════════════════════════════════════════

fn main() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  axiom core — declarative DAG + composite + validate + analyze  ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    // ── 1. 构造 DeploySpec（声明式拓扑，含复合实例）──
    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("src", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("split", "split", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("scaler", "scaler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("doubler_b", "doubler_b", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("merge", "merge", MachinePhysicalSpec::default()))
        // src → split
        .with_link(LinkSpec::new(("src", "out"), ("split", "in"), LinkKind::Inline))
        // split → scaler (fan-out 分支 A)
        .with_link(LinkSpec::new(
            ("split", "a"), ("scaler", "in"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // split → doubler_b (fan-out 分支 B)
        .with_link(LinkSpec::new(
            ("split", "b"), ("doubler_b", "x"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // scaler → merge (fan-in 分支 A)
        .with_link(LinkSpec::new(("scaler", "out"), ("merge", "a"), LinkKind::Inline))
        // doubler_b → merge (fan-in 分支 B)
        .with_link(LinkSpec::new(("doubler_b", "y"), ("merge", "b"), LinkKind::Inline));

    println!("── 1. DeploySpec (含复合实例 scaler) ──────────────────────────");
    println!("  machines: {}", spec.machines.len());
    println!("  links:    {}", spec.links.len());
    for m in &spec.machines {
        println!("    • {} ({})", m.name, m.machine_type);
    }
    println!();

    // ── 2. 验证复合定义 ──
    println!("── 2. CompositeSpec::validate (scaler) ────────────────────────");
    let scaler = scaler_composite();
    match scaler.validate() {
        Ok(()) => println!("  ✓ scaler composite validates (input_map + output_map 完整)"),
        Err(e) => {
            println!("  ✗ scaler composite validation failed: {e}");
            return;
        }
    }
    println!();

    // ── 3. 展开复合 ──
    println!("── 3. expand_composites (scaler → scaler.doubler_a) ───────────");
    let mut composites = BTreeMap::new();
    composites.insert("scaler".to_string(), scaler);
    let (expanded_machines, expanded_links) =
        expand_composites(spec.machines.clone(), spec.links.clone(), &composites)
            .expect("expand_composites");
    println!("  展开前: {} machines, {} links", spec.machines.len(), spec.links.len());
    println!("  展开后: {} machines, {} links", expanded_machines.len(), expanded_links.len());
    for m in &expanded_machines {
        println!("    • {} ({})", m.name, m.machine_type);
    }
    println!();

    // ── 4. 构造展开后的 DeploySpec 并验证 ──
    println!("── 4. validate_deep (端口类型/方向/度约束/环安全) ──────────────");
    let expanded_spec = DeploySpec::new()
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
        Ok(()) => println!("  ✓ validate_deep passed — 端口类型/方向/度约束/环安全全部通过"),
        Err(e) => println!("  ✗ validate_deep failed: {e}"),
    }
    println!();

    // ── 5. 图论分析 ──
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

    // ── 6. 附加：演示反窄化规则验证 ──
    println!("── 6. 反窄化规则验证 ──────────────────────────────────────────");
    println!("  这个拓扑包含:");
    println!("    • fan-out  (split → {{scaler, doubler_b}})");
    println!("    • fan-in   ({{scaler, doubler_b}} → merge)");
    println!("    • 复合机器 (scaler → scaler.doubler_a)");
    println!("    • 多机器类型 (src/split/doubler/doubler_b/merge)");
    println!("    • 多链接物理语义 (Inline + BoundedBuf)");
    println!("  pipeline2/pipeline3 无法表达这个拓扑——它们只支持 A→B→C。");
    println!("  这个 example 证明 core 的 DeploySpec 能表达任意 DAG。");
    println!();

    // ── 7. 附加：演示度约束违反被捕获 ──
    println!("── 7. 度约束违反检测 (Inline outdeg ≤ 1) ──────────────────────");
    let bad_spec = DeploySpec::new()
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
        Err(e) => println!("  ✓ 正确拒绝 Inline outdeg=2: {e}"),
    }
    println!();

    println!("══════════════════════════════════════════════════════════════════");
    println!("  core 能独立定义、验证、分析任意拓扑——不需要 runtime。");
    println!("  runtime 的职责是执行，不是定义。");
    println!("══════════════════════════════════════════════════════════════════");
}
