//! # axiom-runtime
//!
//! The unified runtime of axiom: materializes a `DynamicTopology` into live `MachineHandle`s,
//! drives the `process` loop, and manages the lifecycle.
//!
//! ## Design principles
//!
//! - **Unified runtime, modes configured, not separate types**: single-threaded and multi-threaded
//!   are not two independent types but different values of the same `Runtime` on
//!   `RuntimeConfig::mode`.
//!   `Inline` → inlined execution on the caller's thread;
//!   `Sequential` → single-threaded sequential loop;
//!   `Parallel(n)` → N worker threads scheduled in parallel.
//! - **The native loop cannot bootstrap itself**: the runtime's driver loop is not itself a
//!   Machine; it is a C-style `loop { pull; process; route }`.
//! - **process stays synchronous**: the synchronous signature of `Machine::process` is unchanged.
//!   IO multiplexing and thread-pool management are the runtime's responsibility and do not pollute
//!   core's pure contract layer.
//! - **Static topology first**: once the runtime materializes a `DynamicTopology`, the topology is
//!   fixed in memory; machines cannot be added or removed at runtime. Behavior that needs to "look
//!   dynamic" (elasticity, routing) is expressed with a static topology + Machine-internal State
//!   changes.
//!
//! ## Scope
//!
//! - Single-threaded sequential driver loop (`Sequential` mode, direct move delivery)
//! - Multi-threaded driving (`Parallel(n)` mode: one OS thread per machine; links materialize per
//!   `LinkKind` as `mpsc::channel` / `mpsc::sync_channel` / custom bounded overwrite / single-slot
//!   overwrite carriers; channel disconnection cascades shutdown)
//! - `RegisterFn` registry + type-erased `RunningMachine`
//! - `materialize` / `tick` / `shutdown` lifecycle
//! - **output → input routing** (tick delivers outputs downstream per `LinkSpec`, propagating
//!   level by level in BFS order, including Tee fan-out)
//! - **shutdown propagation** (`Done` = stop signal: the machine stops, backlog is dropped, and the
//!   stop cascades to every downstream whose in-edge sources are all stopped; a Parallel thread
//!   exits immediately upon receiving `Done`)
//! - **fan-in support** (in Parallel mode, multiple in-edges are merged and consumed via a forward
//!   thread, injected in arrival order)
//! - **Tier-B carriers**: `Overwriting` bounded overwrite (overwrites the oldest when full),
//!   `Latest`/`SharedState` single-slot overwrite, `ReadPolicy::NonBlocking` polling
//! - **pipelineN fusion**: `materialize` automatically recognizes Inline chains of adjacent
//!   `FusedInline` machines and replaces them with a `FusedPipeline` — eliminating the per-hop
//!   route lookup (2 String clones), bringing each hop from +4 down to +2 allocs (R003)
//! - **Composite Machine**: `register_composite` wraps a sub-topology + port mapping as a single
//!   `machine_type`; `materialize` expands it recursively (namespaced sub-machines + redirected
//!   external links), and expansion happens before fusion — so `FusedPipeline` can fuse across
//!   original composite boundaries
//!
//! Not covered (later increments):
//! - Multi-reader semantics of `SharedState` (currently a single-consumer approximation)
//! - IOCP completion model for large-scale Windows IO (the current WSAEventSelect readiness model
//!   supports ≤64 sources; production-scale thousands of connections need IOCP)
//! - Bulk-injection shapes that rebuild the thread scope per tick (parallel gains depend on a
//!   sustained stream of commands arriving, not a one-off bulk injection)
//!
//! ## Module structure
//!
//! - [`config`] — `ExecMode` / `RuntimeConfig`
//! - [`erasure`] — `RunningMachine` trait + `ProcessResult` + `MachineWrapper`
//! - [`registry`] — `RegisterFn` + `Registry`
//! - [`topology`] — `LiveTopology` + `PhysicalLink`
//! - [`carrier`] — Parallel link carriers (`ChanSender`/`ChanReceiver` + overwrite/single-slot implementations)
//! - [`routing`] — routing + shutdown propagation + endpoint validation + cycle detection
//! - [`fusion`] — pipelineN fusion (`FusedPipeline` + chain recognition + `apply_fusion`)
//! - [`io`] — IO multiplexing (`IoReactor` trait + epoll/kqueue/WSAEventSelect platform implementations)
//! - [`runtime`] — the `Runtime` core (`materialize`/`tick`/`shutdown`/`run_io`)

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
compile_error!("axiom-runtime currently requires the `std` feature (std::sync::mpsc, std::thread)");

extern crate alloc;

mod carrier;
mod config;
mod erasure;
mod error;
mod fusion;
mod io;
mod registry;
mod replay;
mod routing;
mod scheduler;
mod runtime;
mod static_path;
mod topology;
mod typed_slot;

#[cfg(test)]
mod tests;

pub use axiom::composite::{CompositeSpec, CompositeError, expand_composites};
pub use axiom::static_exec::{
    Chain, Composite, Diamond, FlowThrough, run_parallel, StaticChain, StaticExecError,
    StraightClone, StraightId, StraightLink, StraightMachine, StraightMerge, StraightSplit,
};
pub use axiom::topology::{Topology, StaticTopology};
pub use replay::{ReplayJournal, Replayer};
pub use config::{ExecMode, RuntimeConfig};
pub use erasure::{ProcessResult, RunningMachine};
pub use error::RuntimeError;
pub use io::{
    IoError, IoEvent, IoInterest, IoReactor, IoToken, ManualReactor, RawIo, DefaultReactor,
    default_reactor,
};
pub use registry::{RegisterFn, Registry};
pub use runtime::Runtime;
pub use static_path::{diamond, feedback, pipeline_chain};
pub use topology::{LiveTopology, PhysicalLink};
