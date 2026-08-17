//! # MMO core subgraph — system blueprint
//!
//! A multi-machine collaboration subgraph for a single world shard: connection gateway →
//! input protocol → session lifecycle → world state → per-player view projection → broadcast
//! write-back, plus **event-sourcing logging** (world event stream, replayable to rebuild) and
//! **clock events** (heartbeat / timeout kicking).
//!
//! ## The blueprint (abstraction layer)
//!
//! ```text
//!  IoReactor                     ┌──────────── world layer (single-instance state machine = serial world order) ───────────┐
//!  (epoll/kqueue/WSA)            │                                                                                       │
//!  conn READABLE ──► ConnGateway ──► ProtocolParser ──► SessionManager ──► WorldShard ──► PerPlayerView ──► BroadcastWriter
//!  event          │  (phys read) │  (input proto)  │  (session lifecycle+timeout)  │  (player pos)   (view proj)    │  (phys write)
//!                 │              │                 │        │             │      │                  │
//!                 │              │                 │        └─ tick(clock) │      └─► EventLog (event-sourcing log)
//!                 │              │                 │                       │
//!                 │              │                 └── not-logged-in error hint ─► PerPlayerView.notice
//!                 └──────────────┴─────────────────┴───────────────────────┴──────────────────────────┘
//! ```
//!
//! ## Complexity increment over redis_like
//!
//! 1. **Explicit session lifecycle**: `Login → Playing → Logout` + heartbeat timeout kicking
//!    (driven by clock events) — a connection is no longer an implicit State key but a state machine.
//! 2. **World projection**: world events → a visible view for each of the N players
//!    (`PerPlayerView` filters/formats per player) — the fan-out is projection, not data copying.
//! 3. **Event sourcing**: the log records the **world event stream** (Join/Move/Say/Leave);
//!    on restart the world is rebuilt from the log — more fine-grained than a command log, and a
//!    core requirement for game-server audit/anti-cheat.
//! 4. **Clock**: heartbeat/timeout are periodic events (main injects a timestamp every tick).
//!
//! ## Boundary statements (honest)
//!
//! - Single shard: cross-shard communication (message bus) is Phase 2.
//! - World order: WorldShard is a single serial instance (Sequential direct move);
//!   ordering is guaranteed by the single state machine; multiple shards need an explicit
//!   timestamp design.
//! - Broadcast physical cost: N players = N copies of the view text written back physically;
//!   the performance bill is visible at `BroadcastWriter` (measured by the bench).

use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::resource::MachinePhysicalSpec;

/// Structural blueprint of the MMO core subgraph (DynamicTopology).
pub fn blueprint() -> DynamicTopology {
    DynamicTopology::new()
        // ── machines (7, all single-instance) ─────────────────────────────────
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
        // ── data flow (7 abstract links) ─────────────────────────────────────
        // connection bytes → input message → world event → world update → view → write-back
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
        // session → world (login/move/logout)
        .with_link(LinkSpec::new(
            ("session_mgr", "world"),
            ("world_shard", "evt"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // session → player (not-logged-in error hints)
        .with_link(LinkSpec::new(
            ("session_mgr", "view"),
            ("per_player_view", "notice"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // world → view projection
        .with_link(LinkSpec::new(
            ("world_shard", "world"),
            ("per_player_view", "world"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // view → broadcast write-back
        .with_link(LinkSpec::new(
            ("per_player_view", "view"),
            ("broadcast_writer", "view"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // world event → event-sourcing log
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
