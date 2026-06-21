//! ## Generic built-in Machines
//!
//! Each corresponds to a fundamental computation pattern.
//!
//! | Module | Signature | Role |
//! |--------|-----------|------|
//! | Identity | `I → I` | Category identity morphism |
//! | Sink | `I → ∅` | Discards all input |
//! | Source | `∅ → O` | Constant value per tick |
//! | Tee | `I → (I, I)` | Fan-out broadcast |
//! | Latch | `T → T` | Holds last value |
//! | Collector | `I → ∅` | Gathers into observe stream |
//! | EntityRoot | `∅` | System root, no I/O |
//! | FuncMachine | bridges Func → Machine | Wraps any Func |

mod identity;
mod sink;
mod source;
mod tee;
mod latch;
mod collector;
mod entity_root;
mod func_machine;

pub use identity::{Identity, IdentityInput, IdentityOutput};
pub use sink::{Sink, SinkInput, SinkOutput};
pub use source::{Source, SourceInput, SourceOutput};
pub use tee::{Tee, TeeInput, TeeOutput};
pub use latch::{Latch, LatchInput, LatchOutput};
pub use collector::{Collector, CollectorInput, CollectorOutput};
pub use entity_root::EntityRoot;
pub use func_machine::{FuncMachine, FuncMachineInput, FuncMachineOutput};
