pub mod builtin;
pub mod deploy;
pub mod entity;
pub mod flow;
pub mod func;
pub mod link;
pub mod machine;
pub mod port;
pub mod resource;
pub mod runtime;
pub mod time;

/// Core prelude for typical use.
pub mod prelude_all {
    pub use crate::builtin::{
        Identity, Sink, Tee, Latch, Collector, EntityRoot, FuncMachine,
    };
    pub use crate::deploy::{DeploySpec, DeploySettings, MachineInstance, FuncBinding};
    pub use crate::entity::{Entity, EntityRestoreError};
    pub use crate::flow::FlowKind;
    pub use crate::func::{Func, FuncWithScratch, FuncScratchPipeline, Scratched, CostEstimate};
    pub use crate::link::{LinkKind, LinkSpec, WritePolicy, ReadPolicy, MemoryRegion};
    pub use crate::machine::{Machine, ProcessOutput, InitError, CleanupError};
    pub use crate::port::{
        PortDir, PortDecl, PortSchema, PortRegistry, ConfigDecl, ConfigSchema, MachineContext,
        LinkCompat, Lifecycle, SystemSignal,
    };
    pub use crate::resource::{MachinePhysicalSpec, ExecutionHint, ResourceClass, ThreadPoolSpec};
    pub use crate::time::{TimeTick, Clock, RealClock, ReplayClock};
}
