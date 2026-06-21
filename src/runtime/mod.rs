//! ## Minimal runtimes for axiom machines.
//!
//! axiom's core defines what a Machine is, not how it runs.
//! This module provides the simplest possible "how":
//!
//! - `LinearRuntime`: single-threaded, sequential, zero allocation channels.
//!   Each Machine is `init` → `process` loop → `cleanup` in the current thread.
//!   Port connections use `inline` passing (no heap channels).
//!
//! For multi-threaded, async, or distributed execution, see:
//! - `axiom_tokio` (separate crate)

mod linear;

pub use linear::{LinearRuntime, CleanupGuard};
