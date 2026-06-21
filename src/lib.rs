pub mod func;
pub mod machine;
pub mod port;
pub mod link;
pub mod resource;
pub mod deploy;
pub mod time;

/// Core prelude for typical use.
pub mod prelude_all {
    pub use crate::func::{Func, FuncWithScratch, FuncScratchPipeline, Scratched, CostEstimate};
    pub use crate::machine::{Machine, ProcessOutput, InitError, CleanupError, RestoreError};
    pub use crate::port::{
        PortDir, PortDecl, PortSchema, PortRegistry, ConfigDecl, ConfigSchema, MachineContext,
        LinkCompat,
    };
    pub use crate::link::{LinkKind, LinkSpec, WritePolicy, ReadPolicy, MemoryRegion};
    pub use crate::deploy::{DeploySpec, DeploySettings, MachineInstance, FuncBinding};
    pub use crate::resource::{MachinePhysicalSpec, ExecutionHint, ResourceClass};
    pub use crate::time::{TimeTick, Clock, RealClock, ReplayClock};
}
