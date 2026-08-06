//! # Redis 风格服务器 — 系统蓝图
//!
//! 用 axiom 刻画一个真实生产级系统的**结构蓝图**（与物理过程解耦），
//! 展示：静态拓扑 + 共享状态机（fan-in）+ 多模块边界 + 确定性。
//!
//! ## 蓝图（抽象层：模块 = 边界 + 功能；链接 = 数据流）
//!
//! ```text
//!                    ┌────────────── 静态拓扑（连接动态性 = State 内容）──────────────┐
//!                    │                                                                   │
//!  IoReactor         │  ┌───────────┐   ┌───────────┐   ┌───────────┐   ┌───────────┐   │
//!  (epoll/kqueue/    │  │ConnReader │──▶│RespParser │──▶│ DataStore │──▶│RespEncoder│──▶│ ConnWriter
//!   WSAEventSelect)  │  │  (共享)   │raw│  (共享)   │cmd│  (共享)   │rsp│  (共享)   │out│ （物理写回 socket）
//!  READABLE 事件 ────┼─▶│ 按conn分状态│  │ 按conn分状态│  │ KV+List+Hash│  │ 无状态    │   │
//!                    │  └───────────┘   └───────────┘   └─────┬─────┘   └───────────┘   │
//!                    │                                        │log                        │
//!                    │                                        ▼                           │
//!                    │                                 ┌───────────┐                    │
//!                    │                                 │ AofWriter │──▶ AOF 文件（追加） │
//!                    │                                 │  (共享)   │                    │
//!                    │                                 └───────────┘                    │
//!                    └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 设计要点（axiom 哲学）
//!
//! 1. **拓扑静态，连接是数据**：连接会话的增删不是拓扑变化，而是
//!    `State` 的内容变化（`HashMap<conn_id, ...>`）。这与
//!    "static-first worldview"（静态优先、实例图动态）一致——蓝图不随
//!    连接数改变。
//! 2. **共享状态机 = Redis 单线程模型**：所有连接 fan-in 到单个
//!    `DataStore` 实例（Sequential 模式直接 move 投递，天然无锁）；
//!    这是 Redis "单线程数据层" 的结构化表达。
//! 3. **模块边界 = 关注点分离**：读（物理 IO）、解析（协议）、存储
//!    （数据语义）、编码（回复格式）、写回（物理 IO）、持久化（日志）
//!    各自独立——任一模块可替换/测试/复用。
//! 4. **持久化与主路径解耦**：`AofWriter` 是独立下游，写命令在主路径
//!    之外追加日志——展示"异步物理过程"在蓝图中的位置。
//! 5. **确定性**：同命令序列 → 同最终状态；AOF 重放可恢复（验证见
//!    main.rs 的 replay 测试）。
//!
//! ## 物理过程（蓝图之下，见 main.rs）
//!
//! - `IoReactor`（默认后端：Linux=epoll / macOS=kqueue / Windows=WSA）
//!   监听 listener + 已连接 socket 的 READABLE 事件，事件按 token
//!   路由注入 `ConnReader` 的 `io` 端口。
//! - 共享连接表 `Arc<Mutex<ConnTable>>`：连接 socket 由 main 的
//!   accept 循环管理，`ConnReader`/`ConnWriter` 按 conn_id 查表执行
//!   实际读写（这是 OS 资源层的物理共享，蓝图不表达）。
//! - `ConnWriter` 非阻塞写：WouldBlock 时暂存待写缓冲，注册 WRITABLE
//!   事件（简化版：本 showcase 直接尝试写，失败重试于下轮）。

use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::resource::MachinePhysicalSpec;

/// Redis 风格服务器的最小结构蓝图（DeploySpec）。
///
/// 默认含观测/调试模块（monitor + debugger）：`observe → monitor` 用
/// `Dropping` 载体（观测来不及就丢，主路径零阻塞）；`debugger → ctrl`
/// 为 Control 流反向注入。
pub fn blueprint() -> DeploySpec {
    blueprint_with_monitor(WritePolicy::Dropping)
}

/// 带观测/调试模块的蓝图：`obs_policy` 控制 observe→monitor 载体的
/// 写策略（bench 用它对比 Blocking vs Dropping 对主路径的影响）。
pub fn blueprint_with_monitor(obs_policy: WritePolicy) -> DeploySpec {
    base_blueprint()
        .with_machine(MachineInstance::new(
            "monitor",
            "monitor",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "debugger",
            "debugger",
            MachinePhysicalSpec::default(),
        ))
        // 观测流：data_store.observe → monitor.log（低速观测，Dropping 可丢）
        .with_link(LinkSpec::new(
            ("data_store", "observe"),
            ("monitor", "log"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: obs_policy,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 控制流：debugger.out → data_store.ctrl（调试反向注入）
        .with_link(LinkSpec::new(
            ("debugger", "out"),
            ("data_store", "ctrl"),
            LinkKind::BoundedBuf {
                capacity: 64,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
}


/// 基蓝图：6 机器 + 5 链接（不含观测/调试模块）。
fn base_blueprint() -> DeploySpec {
    DeploySpec::new()
        // ── 模块（6 个，全单实例：连接动态性在 State 里）─────────────
        .with_machine(MachineInstance::new(
            "conn_reader",
            "conn_reader",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "resp_parser",
            "resp_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "data_store",
            "data_store",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "resp_encoder",
            "resp_encoder",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "conn_writer",
            "conn_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "aof_writer",
            "aof_writer",
            MachinePhysicalSpec::default(),
        ))
        // ── 数据流（5 条抽象线）─────────────────────────────────────
        // 连接字节 → 命令 → 逻辑回复 → RESP 编码 → 写回 socket
        .with_link(LinkSpec::new(
            ("conn_reader", "raw"),
            ("resp_parser", "raw"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("resp_parser", "cmd"),
            ("data_store", "cmd"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 逻辑回复 → RESP 编码（无状态纯变换，可 FusedInline）
        .with_link(LinkSpec::new(
            ("data_store", "reply"),
            ("resp_encoder", "reply"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("resp_encoder", "out"),
            ("conn_writer", "resp"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 写命令日志 → AOF 追加（持久化，与主路径解耦）
        .with_link(LinkSpec::new(
            ("data_store", "log"),
            ("aof_writer", "log"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
}

/// 分片集群蓝图（复杂拓扑验证）：
///
/// ```text
/// conn_reader → resp_parser → sharder ─┬─► data_store_0 ─┬─► resp_encoder → conn_writer
///                                      └─► data_store_1 ─┘        │
///                                           │   │                ├─► monitor(观测)
///                                           └───┴──► aof_writer_0 / aof_writer_1
/// debugger ──(Control 广播)──► data_store_0.ctrl / data_store_1.ctrl
/// ```
///
/// 与单 DataStore 版不同的结构特性：
/// - **fan-out**：`sharder` 按 key 哈希把命令路由到 2 个分片（确定性）；
///   `FLUSHALL` 广播两分片。
/// - **fan-in**：两分片的 `reply` 汇聚回 `resp_encoder`（按 conn_id 写回）；
///   两分片的 `observe` 汇聚回 `monitor`。
/// - **并行分片**：`Parallel(n)` 下两个 DataStore 可各占一线程（真实多核）。
/// - **双 AOF**：每分片独立日志（写命令无跨分片依赖 → 重放顺序安全）。
pub fn blueprint_sharded() -> DeploySpec {
    DeploySpec::new()
        .with_machine(MachineInstance::new(
            "conn_reader",
            "conn_reader",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "resp_parser",
            "resp_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "sharder",
            "sharder",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "data_store_0",
            "data_store",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "data_store_1",
            "data_store",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "resp_encoder",
            "resp_encoder",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "conn_writer",
            "conn_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "aof_writer_0",
            "aof_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "aof_writer_1",
            "aof_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "monitor",
            "monitor",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "debugger",
            "debugger",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "broadcast_tee",
            "broadcast_tee",
            MachinePhysicalSpec::default(),
        ))
        // ── 数据流 ────────────────────────────────────────────────────
        .with_link(link_buf("conn_reader", "raw", "resp_parser", "raw"))
        .with_link(link_buf("resp_parser", "cmd", "sharder", "cmd"))
        // fan-out：分片路由
        .with_link(link_buf("sharder", "shard0", "data_store_0", "cmd"))
        .with_link(link_buf("sharder", "shard1", "data_store_1", "cmd"))
        // fan-in：两分片回复汇聚
        .with_link(link_buf("data_store_0", "reply", "resp_encoder", "reply"))
        .with_link(link_buf("data_store_1", "reply", "resp_encoder", "reply"))
        .with_link(link_buf("resp_encoder", "out", "conn_writer", "resp"))
        // 双 AOF：每分片独立日志
        .with_link(link_buf("data_store_0", "log", "aof_writer_0", "log"))
        .with_link(link_buf("data_store_1", "log", "aof_writer_1", "log"))
        // 观测：两分片汇聚（低速观测，可丢）
        .with_link(LinkSpec::new(
            ("data_store_0", "observe"),
            ("monitor", "log"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("data_store_1", "observe"),
            ("monitor", "log"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 控制流：debugger → broadcast_tee（显式 fan-out）→ 两分片 ctrl
        .with_link(link_buf("debugger", "out", "broadcast_tee", "cmd"))
        .with_link(link_buf("broadcast_tee", "out0", "data_store_0", "ctrl"))
        .with_link(link_buf("broadcast_tee", "out1", "data_store_1", "ctrl"))
}

/// BoundedBuf（Blocking/Blocking）链接的简写。
fn link_buf(
    src_m: &'static str,
    src_p: &'static str,
    dst_m: &'static str,
    dst_p: &'static str,
) -> LinkSpec {
    LinkSpec::new(
        (src_m, src_p),
        (dst_m, dst_p),
        LinkKind::BoundedBuf {
            capacity: 1024,
            write_policy: WritePolicy::Blocking,
            read_policy: ReadPolicy::Blocking,
        },
    )
}




