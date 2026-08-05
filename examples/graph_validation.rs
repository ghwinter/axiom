//! # 图分析演示：复杂图的验证与分析
//!
//! 展示 axiom 的**图模型**能力（默认场景是复杂图网络，不是线性管道）：
//!
//! 1. **合法复杂图**（内核风格）：syscall 扇出 + 存储/网络双路径 +
//!    3 个反馈环 + 观测流 —— `validate_deep` 通过 + 结构分析报告
//! 2. **非法变体**：逐项检出（流类型不匹配 / Inline 环 / 全 Moore 环）
//!
//! 运行：`cargo run --example graph_validation`
//!
//! ```text
//!                ┌─────────── 存储路径 ──────────┐
//!  syscall ──to_vfs──► vfs ──bio──► block ──req──► driver.disk
//!    │ fan-out                                      │
//!    ├──to_net──► net ──skb──► driver.net ──────────┘
//!    ├──to_ipc──► ipc
//!    │
//!    ├──sched──► scheduler ◄──wakeup── memory ◄──fault──┐  环① 调度↔内存
//!    │             │  run                                  │
//!    │             └──► process ──done──► wakeup ─────────┘  环② 调度→进程
//!    │                   │  req
//!    └───────────────────┴──► syscall ◄─────────────────────┘  环③ 进程→内核大环
//!    └──events(Observe)──► perf（观测）
//! ```

use std::collections::HashMap;

use axiom::analysis;
use axiom::deploy::{DeploySpec, MachineInstance, ValidationError};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::port::{PortDecl, PortSchema};
use axiom::resource::MachinePhysicalSpec;

fn buf(capacity: usize) -> LinkKind {
    LinkKind::BoundedBuf {
        capacity,
        write_policy: WritePolicy::Blocking,
        read_policy: ReadPolicy::Blocking,
    }
}

/// 内核风格复杂图：syscall 扇出 + 双路径 + 3 反馈环 + 观测。
/// 所有机器非 Moore（有状态，可打破环延迟）→ 环合法。
fn kernel_graph() -> (DeploySpec, HashMap<&'static str, PortSchema>) {
    let mut s = HashMap::new();
    s.insert(
        "syscall",
        PortSchema::new()
            .with(PortDecl::input::<u64>("req"))
            .with(PortDecl::output::<u64>("to_vfs"))
            .with(PortDecl::output::<u64>("to_net"))
            .with(PortDecl::output::<u64>("to_ipc"))
            .with(PortDecl::output::<u64>("sched"))
            .with(PortDecl::observe::<u64>("events")),
    );
    s.insert(
        "vfs",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("bio")),
    );
    s.insert(
        "block",
        PortSchema::new()
            .with(PortDecl::input::<u64>("bio"))
            .with(PortDecl::output::<u64>("req")),
    );
    // driver 用两个输入端口（disk/net）各单入 → 度约束合规
    s.insert(
        "driver",
        PortSchema::new()
            .with(PortDecl::input::<u64>("disk"))
            .with(PortDecl::input::<u64>("net")),
    );
    s.insert(
        "net",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("skb")),
    );
    s.insert(
        "ipc",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("msg")),
    );
    s.insert(
        "scheduler",
        PortSchema::new()
            .with(PortDecl::input::<u64>("wakeup"))
            .with(PortDecl::output::<u64>("run"))
            .with(PortDecl::output::<u64>("fault")),
    );
    s.insert(
        "memory",
        PortSchema::new()
            .with(PortDecl::input::<u64>("fault"))
            .with(PortDecl::output::<u64>("ok")),
    );
    s.insert(
        "process",
        PortSchema::new()
            .with(PortDecl::input::<u64>("run"))
            .with(PortDecl::output::<u64>("done"))
            .with(PortDecl::output::<u64>("req")),
    );
    s.insert(
        "perf",
        PortSchema::new().with(PortDecl::new::<u64>("events", axiom::port::PortDir::In, axiom::flow::FlowKind::Observe)),
    );

    let spec = DeploySpec::new()
        .with_machine(MachineInstance::new("syscall", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("vfs", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("block", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("driver", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("net", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("ipc", "t", MachinePhysicalSpec::default()))
        .with_machine(
            MachineInstance::new("scheduler", "t", MachinePhysicalSpec::default())
                // 环合法条件：每环至少一个 Moore 机器（打破环延迟）。
                // scheduler 同时位于三个环上 → 一个 Moore 标记使全部环合法。
                .moore(),
        )
        .with_machine(MachineInstance::new("memory", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("process", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("perf", "t", MachinePhysicalSpec::default()))
        // fan-out：syscall → vfs/net/ipc（三个输出端口）
        .with_link(LinkSpec::new(("syscall", "to_vfs"), ("vfs", "in"), buf(8)))
        .with_link(LinkSpec::new(("syscall", "to_net"), ("net", "in"), buf(8)))
        .with_link(LinkSpec::new(("syscall", "to_ipc"), ("ipc", "in"), buf(8)))
        // 存储路径：vfs → block → driver.disk
        .with_link(LinkSpec::new(("vfs", "bio"), ("block", "bio"), buf(8)))
        .with_link(LinkSpec::new(("block", "req"), ("driver", "disk"), buf(8)))
        // 网络路径：net → driver.net（双路径汇入 driver，但端口分离 → 度合规）
        .with_link(LinkSpec::new(("net", "skb"), ("driver", "net"), buf(8)))
        // 环①：scheduler ↔ memory（缺页 → 分配 → 唤醒）
        .with_link(LinkSpec::new(("scheduler", "fault"), ("memory", "fault"), buf(8)))
        .with_link(LinkSpec::new(("memory", "ok"), ("scheduler", "wakeup"), buf(8)))
        // 环②：scheduler → process → scheduler
        .with_link(LinkSpec::new(("scheduler", "run"), ("process", "run"), buf(8)))
        .with_link(LinkSpec::new(("process", "done"), ("scheduler", "wakeup"), buf(8)))
        // 环③：process → syscall → scheduler → process（大环）
        .with_link(LinkSpec::new(("process", "req"), ("syscall", "req"), buf(8)))
        .with_link(LinkSpec::new(("syscall", "sched"), ("scheduler", "wakeup"), buf(8)))
        // 观测：syscall.events（Observe 流）→ perf
        .with_link(LinkSpec::new(("syscall", "events"), ("perf", "events"), buf(8)));
    (spec, s)
}

fn check(label: &str, spec: &DeploySpec, schemas: &HashMap<&'static str, PortSchema>) {
    match spec.validate_deep(schemas) {
        Ok(_) => println!("    {label}: ✓ 通过"),
        Err(ValidationError::UnsafeCycle { cycle }) => {
            println!("    {label}: ✗ UnsafeCycle {cycle:?}（环中无状态机器，无法打破延迟）")
        }
        Err(ValidationError::InlineCycle { cycle }) => {
            println!("    {label}: ✗ InlineCycle {cycle:?}（同步调用死锁）")
        }
        Err(e) => println!("    {label}: ✗ {e:?}"),
    }
}

fn main() {
    println!("=== axiom 图分析演示：复杂图的验证与分析 ===\n");

    // ── 1. 合法复杂图：validate_deep + 结构分析 ──
    let (spec, schemas) = kernel_graph();
    println!("[1] 合法复杂图（syscall 扇出 + 存储/网络双路径 + 3 反馈环 + 观测）");
    check("validate_deep", &spec, &schemas);

    let loops = analysis::feedback_loops(&spec);
    println!("    反馈环: {} 个（合法：每环含非 Moore 状态机）", loops.len());
    for l in &loops {
        println!("      - machines={:?}, all_moore={}", l.machines, l.all_moore);
    }
    let spofs = analysis::single_points_of_failure(&spec);
    println!(
        "    SPOF: {} 个（本图全连通无 source——dominator 分析需入口；见 [2d] 带入口子图）",
        spofs.len()
    );
    let deg = analysis::degree_violations(&spec);
    println!(
        "    度违规: {} 个（预期 0：driver 用双端口 disk/net 各单入，无 fan-in 超载）",
        deg.len()
    );
    let reach = analysis::reachable_from(&spec, "syscall");
    println!("    syscall 可达: {} 个机器", reach.len());

    // ── 2. 非法变体：逐项检出 ──
    println!("\n[2] 非法变体 → 逐项检出");

    // 2a. 流类型不匹配：Observe 输出 → Data 输入
    let mut s_bad = HashMap::new();
    s_bad.insert("a", PortSchema::new().with(PortDecl::output::<u64>("evt")));
    s_bad.insert(
        "b",
        PortSchema::new().with(PortDecl::input::<u64>("in")), // Data 输入
    );
    let bad_flow = DeploySpec::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "evt"), ("b", "in"), buf(8)));
    print!("    流类型不匹配（a.evt[Data] → b.in[Data] 反向意图）");
    // a 的 evt 是 Data 输出；若要 Observe 不匹配需改 a 的 schema —— 直接构造：
    let mut s_obs = HashMap::new();
    s_obs.insert("a", PortSchema::new().with(PortDecl::observe::<u64>("evt")));
    s_obs.insert("b", PortSchema::new().with(PortDecl::input::<u64>("in")));
    check("Observe 输出 → Data 输入", &bad_flow, &s_obs);

    // 2b. Inline 环：环上 Inline 链接（同步调用死锁，Moore 也救不了）
    let mut s2 = HashMap::new();
    s2.insert(
        "a",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("out")),
    );
    s2.insert(
        "b",
        PortSchema::new()
            .with(PortDecl::input::<u64>("in"))
            .with(PortDecl::output::<u64>("out")),
    );
    let inline_cycle = DeploySpec::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()).moore())
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), LinkKind::Inline));
    check("Inline 环（a→b→a 用 Inline）", &inline_cycle, &s2);

    // 2c. 全非 Moore 环：环上全是状态机 → UnsafeCycle（无 Moore 打破延迟）
    let all_stateful = DeploySpec::new()
        .with_machine(MachineInstance::new("a", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), buf(8)))
        .with_link(LinkSpec::new(("b", "out"), ("a", "in"), buf(8)));
    check("全非 Moore 环（a→b→a 全状态机）", &all_stateful, &s2);

    // 2d. SPOF 分析（带 source 的子图）：gateway 是唯一入口支配者
    let mut s3 = HashMap::new();
    s3.insert("app", PortSchema::new().with(PortDecl::output::<u64>("req")));
    s3.insert(
        "gateway",
        PortSchema::new()
            .with(PortDecl::input::<u64>("req"))
            .with(PortDecl::output::<u64>("to_store"))
            .with(PortDecl::output::<u64>("to_media"))
            .with(PortDecl::output::<u64>("to_logs")),
    );
    s3.insert("storage", PortSchema::new().with(PortDecl::input::<u64>("in")));
    s3.insert("media", PortSchema::new().with(PortDecl::input::<u64>("in")));
    s3.insert("logs", PortSchema::new().with(PortDecl::input::<u64>("in")));
    let spof_spec = DeploySpec::new()
        .with_machine(MachineInstance::new("app", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("gateway", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("storage", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("media", "t", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("logs", "t", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("app", "req"), ("gateway", "req"), buf(8)))
        .with_link(LinkSpec::new(("gateway", "to_store"), ("storage", "in"), buf(8)))
        .with_link(LinkSpec::new(("gateway", "to_media"), ("media", "in"), buf(8)))
        .with_link(LinkSpec::new(("gateway", "to_logs"), ("logs", "in"), buf(8)));
    let spofs = analysis::single_points_of_failure(&spof_spec);
    println!("    SPOF 分析（app→gateway→{{store,media,logs}}）:");
    assert!(!spofs.is_empty(), "gateway 必须是 SPOF");
    for s in &spofs {
        println!("      - {} 断开 → {} 个下游不可达", s.vertex, s.threatens.len());
    }
    assert!(
        spofs.iter().any(|s| s.vertex == "gateway"),
        "gateway 是支配所有 sink 的 SPOF"
    );

    // ── 3. 结论 ──
    println!("\n[3] 结论");
    println!("    axiom 的默认模型是任意有向图：环合法（非 Moore 打破延迟）、");
    println!("    fan-out 多端口、fan-in 端口分离、Observe 观测流——全部通过");
    println!("    validate_deep 验证；结构分析（环/SPOF/度/可达）在部署前给出报告。");
}
