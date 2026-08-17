//! # Redis-style server — system blueprint
//!
//! Uses axiom to describe the **structural blueprint** of a real production-grade system
//! (decoupled from the physical process), demonstrating: static topology + shared state machine
//! (fan-in) + multi-module boundaries + determinism.
//!
//! ## The blueprint (abstraction layer: modules = boundary + function; links = data flow)
//!
//! ```text
//!                    ┌────────────── static topology (connection dynamism = State content) ──────────────┐
//!                    │                                                                   │
//!  IoReactor         │  ┌───────────┐   ┌───────────┐   ┌───────────┐   ┌───────────┐   │
//!  (epoll/kqueue/    │  │ConnReader │──▶│RespParser │──▶│ DataStore │──▶│RespEncoder│──▶│ ConnWriter
//!   WSAEventSelect)  │  │  (shared) │raw│  (shared) │cmd│  (shared) │rsp│  (shared) │out│ (physical socket write-back)
//!  READABLE event ────┼─▶│ per-conn state│ │ per-conn state│ │ KV+List+Hash│ │ stateless │   │
//!                    │  └───────────┘   └───────────┘   └─────┬─────┘   └───────────┘   │
//!                    │                                        │log                        │
//!                    │                                        ▼                           │
//!                    │                                 ┌───────────┐                    │
//!                    │                                 │ AofWriter │──▶ AOF file (append)│
//!                    │                                 │  (shared) │                    │
//!                    │                                 └───────────┘                    │
//!                    └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Design highlights (axiom philosophy)
//!
//! 1. **Topology static, connections are data**: the add/remove of connection sessions is not a
//!    topology change but a change in `State` content (`HashMap<conn_id, ...>`). This is consistent
//!    with the "static-first worldview" (static-first, instance graph dynamic) — the blueprint does
//!    not change with the number of connections.
//! 2. **Shared state machine = the Redis single-threaded model**: all connections fan in to a single
//!    `DataStore` instance (in Sequential mode, direct move delivery — naturally lock-free);
//!    this is the structural expression of Redis's "single-threaded data layer".
//! 3. **Module boundary = separation of concerns**: read (physical IO), parse (protocol), storage
//!    (data semantics), encode (reply format), write-back (physical IO), persistence (logging) are
//!    each independent — any module can be replaced/tested/reused.
//! 4. **Persistence decoupled from the main path**: `AofWriter` is an independent downstream; write
//!    commands append to the log outside the main path — showing where an "asynchronous physical
//!    process" sits in the blueprint.
//! 5. **Determinism**: same command sequence → same final state; AOF replay can restore it
//!    (see the replay test in main.rs).
//!
//! ## Physical process (below the blueprint, see main.rs)
//!
//! - `IoReactor` (default backend: Linux=epoll / macOS=kqueue / Windows=WSA) watches the listener and
//!   connected sockets for READABLE events; events are routed by token into the `ConnReader` `io` port.
//! - Shared connection table `Arc<Mutex<ConnTable>>`: connection sockets are managed by main's
//!   accept loop; `ConnReader`/`ConnWriter` look up the table by conn_id to perform the actual
//!   reads/writes (this is physical sharing at the OS-resource layer, not expressed in the blueprint).
//! - `ConnWriter` non-blocking write: on WouldBlock it buffers pending writes and registers a WRITABLE
//!   event (simplified here: this showcase writes directly and retries on the next round).

use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::resource::MachinePhysicalSpec;

/// Minimal structural blueprint for a Redis-style server (DynamicTopology).
///
/// Includes the observer/debug modules by default (monitor + debugger): `observe → monitor` uses
/// the `Dropping` carrier (drops observations when the observer lags; zero blocking on the main
/// path); `debugger → ctrl` is reverse Control-flow injection.
pub fn blueprint() -> DynamicTopology {
    blueprint_with_monitor(WritePolicy::Dropping)
}

/// Blueprint with observer/debug modules: `obs_policy` controls the write policy of the
/// observe→monitor carrier (bench uses it to compare how Blocking vs Dropping affect the main path).
pub fn blueprint_with_monitor(obs_policy: WritePolicy) -> DynamicTopology {
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
        // Observation stream: data_store.observe → monitor.log (slow observer; Dropping may drop)
        .with_link(LinkSpec::new(
            ("data_store", "observe"),
            ("monitor", "log"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: obs_policy,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        // Control flow: debugger.out → data_store.ctrl (reverse debug injection)
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


/// Base blueprint: 6 machines + 5 links (no observer/debug modules).
fn base_blueprint() -> DynamicTopology {
    DynamicTopology::new()
        // ── modules (6, all single instance: connection dynamism lives in State) ──
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
        // ── data flow (5 abstract lines) ─────────────────────────────────
        // connection bytes → command → logical reply → RESP encoding → write back to socket
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
        // logical reply → RESP encoding (stateless pure transform, fusable via FusedInline)
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
        // write-command log → AOF append (persistence, decoupled from the main path)
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

/// Sharded-cluster blueprint (complex-topology validation):
///
/// ```text
/// conn_reader → resp_parser → sharder ─┬─► data_store_0 ─┬─► resp_encoder → conn_writer
///                                      └─► data_store_1 ─┘        │
///                                           │   │                ├─► monitor (observer)
///                                           └───┴──► aof_writer_0 / aof_writer_1
/// debugger ──(Control broadcast)──► data_store_0.ctrl / data_store_1.ctrl
/// ```
///
/// Structural features that differ from the single-DataStore variant:
/// - **fan-out**: `sharder` routes commands to 2 shards by key hash (deterministic);
///   `FLUSHALL` broadcasts to both shards.
/// - **fan-in**: both shards' `reply` converge back to `resp_encoder` (written back by conn_id);
///   both shards' `observe` converge back to `monitor`.
/// - **Parallel shards**: under `Parallel(n)` the two DataStores can each occupy a thread (real multicore).
/// - **Dual AOF**: each shard has an independent log (write commands have no cross-shard
///   dependencies → replay order is safe).
pub fn blueprint_sharded() -> DynamicTopology {
    DynamicTopology::new()
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
        // ── data flow ────────────────────────────────────────────────────
        .with_link(link_buf("conn_reader", "raw", "resp_parser", "raw"))
        .with_link(link_buf("resp_parser", "cmd", "sharder", "cmd"))
        // fan-out: shard routing
        .with_link(link_buf("sharder", "shard0", "data_store_0", "cmd"))
        .with_link(link_buf("sharder", "shard1", "data_store_1", "cmd"))
        // fan-in: both shards' replies converge
        .with_link(link_buf("data_store_0", "reply", "resp_encoder", "reply"))
        .with_link(link_buf("data_store_1", "reply", "resp_encoder", "reply"))
        .with_link(link_buf("resp_encoder", "out", "conn_writer", "resp"))
        // dual AOF: one independent log per shard
        .with_link(link_buf("data_store_0", "log", "aof_writer_0", "log"))
        .with_link(link_buf("data_store_1", "log", "aof_writer_1", "log"))
        // observer: both shards converge (slow observer, may drop)
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
        // Control flow: debugger → broadcast_tee (explicit fan-out) → both shards' ctrl
        .with_link(link_buf("debugger", "out", "broadcast_tee", "cmd"))
        .with_link(link_buf("broadcast_tee", "out0", "data_store_0", "ctrl"))
        .with_link(link_buf("broadcast_tee", "out1", "data_store_1", "ctrl"))
}

/// Shorthand for a BoundedBuf (Blocking/Blocking) link.
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




