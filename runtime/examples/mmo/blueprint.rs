//! # MMO 核心子图 — 系统蓝图
//!
//! 单世界分区（shard）的多线程协同子图：连接网关 → 输入协议 →
//! 会话生命周期 → 世界状态 → 玩家视野投影 → 广播写回，外加
//! **事件溯源日志**（世界事件流，可回放重建）与 **时钟事件**
//! （心跳/超时踢人）。
//!
//! ## 蓝图（抽象层）
//!
//! ```text
//!  IoReactor                     ┌──────────── 世界层（单实例状态机 = 串行世界序）───────────┐
//!  (epoll/kqueue/WSA)            │                                                           │
//!  连接 READABLE ──► ConnGateway ──► ProtocolParser ──► SessionManager ──► WorldShard ──► PerPlayerView ──► BroadcastWriter
//!  事件          │  (物理读)   │  (输入协议)  │  (会话生命周期+超时)  │  (玩家位置)   (视野投影)    │  (物理写回)
//!                │             │              │        │             │      │                  │
//!                │             │              │        └─ tick(时钟) │      └─► EventLog（事件溯源日志）
//!                │             │              │                       │
//!                │             │              └── 未登录错误提示 ─────► PerPlayerView.notice
//!                └─────────────┴──────────────┴───────────────────────┴──────────────────────────┘
//! ```
//!
//! ## 与 redis_like 的复杂度增量
//!
//! 1. **显式会话生命周期**：`Login → Playing → Logout` + 心跳超时踢人
//!    （时钟事件驱动）——连接不再是隐式的 State 键，而是状态机。
//! 2. **世界投影**：世界事件 → N 玩家可见视图（`PerPlayerView` 按
//!    玩家过滤/格式化）——fan-out 是投影而非数据复制。
//! 3. **事件溯源**：日志记录**世界事件流**（Join/Move/Say/Leave），
//!    重启后从日志重建世界——比命令日志更细粒度，是游戏服务器
//!    审计/防作弊的核心需求。
//! 4. **时钟**：心跳/超时是周期事件（main 每 tick 注入时间戳）。
//!
//! ## 边界声明（诚实）
//!
//! - 单分区：跨 shard 通信（消息总线）是 Phase 2。
//! - 世界序：WorldShard 单实例串行（Sequential 直接 move），
//!   顺序由单状态机保证；多 shard 需显式时间戳设计。
//! - 广播物理成本：N 玩家 = N 份视图文本物理写回，性能账单在
//!   `BroadcastWriter` 可见（bench 测量）。

use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::resource::MachinePhysicalSpec;

/// MMO 核心子图的结构蓝图（DeploySpec）。
pub fn blueprint() -> DeploySpec {
    DeploySpec::new()
        // ── 模块（7 个，全单实例）─────────────────────────────────────
        .with_machine(MachineInstance::new(
            "conn_gateway",
            "conn_gateway",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "protocol_parser",
            "protocol_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "session_mgr",
            "session_mgr",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "world_shard",
            "world_shard",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "per_player_view",
            "per_player_view",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "broadcast_writer",
            "broadcast_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "event_log",
            "event_log",
            MachinePhysicalSpec::default(),
        ))
        // ── 数据流（7 条抽象线）───────────────────────────────────────
        // 连接字节 → 输入消息 → 世界事件 → 世界更新 → 视野 → 写回
        .with_link(LinkSpec::new(
            ("conn_gateway", "raw"),
            ("protocol_parser", "raw"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("protocol_parser", "msg"),
            ("session_mgr", "msg"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 会话 → 世界（登录/移动/登出）
        .with_link(LinkSpec::new(
            ("session_mgr", "world"),
            ("world_shard", "evt"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 会话 → 玩家（未登录错误提示）
        .with_link(LinkSpec::new(
            ("session_mgr", "view"),
            ("per_player_view", "notice"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 世界 → 视野投影
        .with_link(LinkSpec::new(
            ("world_shard", "world"),
            ("per_player_view", "world"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 视野 → 广播写回
        .with_link(LinkSpec::new(
            ("per_player_view", "view"),
            ("broadcast_writer", "view"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // 世界事件 → 事件溯源日志
        .with_link(LinkSpec::new(
            ("world_shard", "log"),
            ("event_log", "log"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
}
