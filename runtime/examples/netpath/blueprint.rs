//! # 网络收包路径 — 系统蓝图
//!
//! 内核网络栈的收包数据路径（NIC → 以太网 → IP → TCP → 应用交付），
//! 用 pcap 文件重放作为确定性数据源（类似内核 pktgen / 网络测试框架）。
//!
//! ## 蓝图（抽象层）
//!
//! ```text
//!                ┌────────────── 收包数据路径（静态拓扑）──────────────┐
//!                │                                                     │
//!  pcap 文件 ────► PcapReader ──► EthParser ──► IpParser ──► TcpParser ──► AppDeliver ──► report（流统计）
//!  (重放源)      │ (物理读)   │ (以太帧)   │ (IP 头)    │ (TCP 载荷) │ (流聚合)   │
//!                │   │            │             │            │            │              │
//!                │   └─ next 事件（main 注入，逐包驱动）        │            │              │
//!                │                                            │            └─ stats ──► PktStats（观测）
//!                │                                            │           (Observe 流,     (独立线程,
//!                └────────────────────────────────────────────┴────────────  Dropping 载体)  低速统计)
//! ```
//!
//! ## 设计要点
//!
//! 1. **确定性重放**：pcap 是真实抓包格式——同一文件重放两次，处理
//!    结果必须逐包一致（确定性验证的核心手段，与内核网络测试同思路）。
//! 2. **模块 = 协议层**：以太/IP/TCP 各一层一个模块，剥头后只把
//!    payload 传递下游——内核协议栈的分层抽象在蓝图中直接可见。
//! 3. **观测即数据**：`AppDeliver.stats` 是 Observe 流，`PktStats`
//!    低速聚合（Dropping 载体，观测不影响主路径——与 redis_like 同模式）。
//! 4. **只解析不校验**：简化版不做 TCP 重排/校验和（真实内核的
//!    reassembly 是后续增量）；当前路径覆盖"逐包解析 + 流聚合"。
//!
//! ## 边界声明（诚实）
//!
//! - 只处理 IPv4 + TCP（UDP/其他 ethertype 丢弃并计数）。
//! - 不做 TCP 序列号重排（按序到达假设）；不做校验和验证。
//! - 物理过程（NIC 中断、DMA、NAPI 轮询）不在本蓝图——pcap 重放
//!   模拟"包进入协议栈"的边界条件。

use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::resource::MachinePhysicalSpec;

/// 网络收包路径的结构蓝图（DeploySpec）。
pub fn blueprint() -> DeploySpec {
    DeploySpec::new()
        // ── 模块（6 个）──────────────────────────────────────────────
        .with_machine(MachineInstance::new(
            "pcap_reader",
            "pcap_reader",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "eth_parser",
            "eth_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "ip_parser",
            "ip_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "tcp_parser",
            "tcp_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "app_deliver",
            "app_deliver",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "pkt_stats",
            "pkt_stats",
            MachinePhysicalSpec::default(),
        ))
        // ── 数据流（5 条）────────────────────────────────────────────
        // 原始包 → 以太 → IP → TCP → 应用
        .with_link(LinkSpec::new(
            ("pcap_reader", "pkt"),
            ("eth_parser", "pkt"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("eth_parser", "ip"),
            ("ip_parser", "ip"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("ip_parser", "tcp"),
            ("tcp_parser", "tcp"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("tcp_parser", "seg"),
            ("app_deliver", "seg"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 观测流：流统计 → PktStats（低速观测，Dropping 可丢）
        .with_link(LinkSpec::new(
            ("app_deliver", "stats"),
            ("pkt_stats", "log"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::Blocking,
            },
        ))
}
