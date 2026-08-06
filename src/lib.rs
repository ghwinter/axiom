//! # axiom
//!
//! **Func + Machine: typed ports, explicit topology, deploy-time physics.**
//!
//! Zero-dependency computation primitives for observable, controllable systems.
//! `Func` (stack, stateless) and `Machine` (heap, stateful) are organized around
//! six core primitives — [`Port`], [`Flow`], [`Session`], [`Topology`],
//! [`Lifecycle`], [`Machine`] — with typed I/O, explicit link topology,
//! deployment specs, and resource classification.
//!
//! axiom is a **pure contract layer**: it defines typed ports, flow semantics,
//! session protocols, and deployment specs — the vocabulary a runtime adapter
//! interprets. The core crate carries no runtime, no executor, no async, and
//! (outside the optional `serialize` feature) no dependencies.
//!
//! ## Features
//!
//! | Feature | Default | What it enables |
//! |---------|---------|-----------------|
//! | `std` | yes | `std::error::Error` impls, `ConfigCell`/`MigrateRegistry` (RwLock-backed), `RealClock` wall-clock |
//! | `serialize` | no | `serde::Serialize`/`Deserialize` on pure-data enums & structs |
//!
//! Build with `--no-default-features` for a `no_std + alloc` configuration
//! (embedded/WASM): the pure-data + `alloc` subset compiles without std.
//! Collection types come from `crate::compat` so the crate stays
//! zero-dependency in both configurations. See `docs/foundations.md` §14.2.
//!
//! [`Port`]: crate::port
//! [`Flow`]: crate::flow
//! [`Session`]: crate::session
//! [`Topology`]: crate::topology
//! [`Lifecycle`]: crate::machine
//! [`Machine`]: crate::machine::Machine

// Enable `#[doc(cfg)]` rendering on docs.rs (nightly-only, gated so stable
// builds are unaffected).
#![cfg_attr(docsrs, feature(doc_cfg))]

// axiom supports a `no_std + alloc` build. With the default `std` feature the
// crate uses `std::collections`/`std::sync::RwLock`/`std::time`; without it,
// the pure-data + `alloc` subset compiles on embedded/WASM targets. The
// `std::error::Error` impls, the RwLock-backed `ConfigCell`/`MigrateRegistry`,
// and `RealClock` are gated behind `#[cfg(feature = "std")]`. Collection types
// come from `crate::compat` so they map to `BTreeMap`/`BTreeSet` under `no_std`.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

// 过程宏生成的代码用 `::axiom::` 绝对路径引用 core 类型。
// 在 axiom crate 自身内部，需要 `extern crate self` 让这个路径可用。
// 这只在 `derive` feature 启用时需要（过程宏生成的代码才存在）。
#[cfg(feature = "derive")]
extern crate self as axiom;

// 过程宏入口——当 `derive` feature 启用时，`#[axiom::ports]` 可用。
// 过程宏在编译期运行，生成的代码引用 `::axiom::` 路径，因此与 no_std 兼容。
#[cfg(feature = "derive")]
pub use axiom_derive::ports;

// axiom core targets `std` by default. A `no_std` + `alloc` configuration is
// available for embedded/edge use — the error types below gate their
// `std::error::Error` impls behind `#[cfg(feature = "std")]`. See
// docs/foundations.md §14.2 (future work).
pub mod analysis;
pub mod backpressure;
#[cfg(feature = "serialize")]
pub mod blueprint;
pub mod builtin;
pub mod compat;
pub mod composite;
#[cfg(feature = "std")]
pub mod config;
pub mod deploy;
pub mod entity;
pub mod flow;
pub mod func;
pub mod hybrid;
pub mod link;
pub mod lint;
pub mod machine;
#[cfg(feature = "std")]
pub mod migrate;
pub mod port;
pub mod portset;
pub mod resource;
pub mod runtime_contract;
pub mod session;
pub mod static_exec;
pub mod stream;
pub mod time;
pub mod topology;

/// Core prelude for typical use.
pub mod prelude_all {
    pub use crate::backpressure::{
        BackpressurePolicy, BackpressureAction, BackpressureCtx,
        BlockPolicy, DropPolicy, OverwritePolicy, CreditPolicy,
    };
    pub use crate::builtin::{
        Identity, Sink, Tee, Latch, Collector, EntityRoot, FuncMachine,
    };
    pub use crate::composite::{CompositeSpec, CompositeError, expand_composites};
    pub use crate::deploy::{DeploySpec, DeploySettings, MachineInstance, FuncBinding, ValidationError};
    #[cfg(feature = "std")]
    pub use crate::config::{ConfigCell, ConfigError};
    pub use crate::entity::{Entity, EntityRestoreError};
    pub use crate::flow::FlowKind;
    pub use crate::func::{Func, FuncRef, FuncWithScratch, FuncScratchPipeline, Scratched, CostEstimate};
    pub use crate::hybrid::{HybridMachine, HybridDriver, HybridState, ContinuousState, Jump};
    pub use crate::link::{LinkKind, LinkSpec, WritePolicy, ReadPolicy, MemoryRegion};
    pub use crate::machine::{
        Machine, Moore, ProcessOutput, InitError, CleanupError,
        MachineHandle, LifecycleState,
        Init, Running, Stopping, Stopped,
        SingleOutput, MultiOutput, TupleOutput, MachineOutput, FusedInline, FusedCompatible,
    };
    #[cfg(feature = "std")]
    pub use crate::migrate::{SchemaMigrate, MigrateFn, MigrateRegistry};
    pub use crate::port::{
        PortDir, PortDecl, PortSchema, PortRegistry, ConfigDecl, ConfigSchema, MachineContext,
        LinkCompat, Lifecycle, SystemSignal,
    };
    pub use crate::portset::{
        PortSet, HasPortInfo,
        In, Out, SinglePorts,        // single-port convenience
        NoInput, NoOutput,           // empty-port convenience
    };
    pub use crate::resource::{MachinePhysicalSpec, ExecutionHint, ResourceClass, ThreadPoolSpec};
    pub use crate::session::{
        SessionType, SessionOp, SessionState, SessionProtocol, SessionError, is_dual,
        GlobalType, GlobalOp, LocalType, LocalOp, Role, project, is_consistent,
    };
    pub use crate::stream::StreamingMachine;
    pub use crate::static_exec::{Link, IdLink, Split, CloneSplit, Merge, StaticExecError};
    pub use crate::time::{TimeTick, Clock, RealClock, ReplayClock};
    pub use crate::topology::{DynamicTopology, TopologyOp, TopologyDelta, AppliedOp, TopologyError};
    pub use crate::analysis::{
        TopologyWarning, FeedbackLoop, SinglePointOfFailure, TopologyReport,
    };

    /// The port declaration macro for multi-port Machines.
    pub use crate::declare_ports;
}
