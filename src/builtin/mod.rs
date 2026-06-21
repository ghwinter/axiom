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

mod identity;
mod sink;
mod source;
mod tee;
mod latch;
mod collector;
mod entity_root;

pub use identity::Identity;
pub use sink::Sink;
pub use source::Source;
pub use tee::Tee;
pub use latch::Latch;
pub use collector::Collector;
pub use entity_root::EntityRoot;
