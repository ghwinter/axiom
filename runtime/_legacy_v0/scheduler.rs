//! Scheduler contract — replaceable strategies for the runtime's internal subsystems.
//!
//! # Structural consistency (the runtime is also organized as "module + contract")
//!
//! The runtime is a parent system whose subsystems (scheduling, carrier, lifecycle, IO, replay)
//! are each **necessary but replaceable** modules. The external `RuntimeContract` guarantees the
//! runtime as a whole is replaceable (`docs/architecture.md`); `Scheduler` is the **contractualization
//! of internal subsystems** — scheduling strategies (Sequential / Parallel / future custom) are a
//! limited set of execution forms, chosen at deployment time (the application of `design-principles.md`
//! D1 inside the runtime).
//!
//! # Replaceability
//!
//! `Runtime` selects a scheduler based on `RuntimeConfig::mode` at construction time and holds
//! `Box<dyn Scheduler>`; a custom scheduler = implement [`Scheduler`] and replace it.
//! The scheduler accesses the topology and configuration via `&mut Runtime` (the `drive_*` methods
//! remain Runtime methods — scheduling logic is not duplicated, the contract layer forwards calls).

use crate::config::RuntimeConfig;
use crate::error::RuntimeError;
use crate::erasure::ProcessResult;

/// Scheduler contract — replaceable strategy for the driver loop.
///
/// `tick` injects external inputs, propagates them through the topology, and returns terminal
/// outputs. Implementors hold their own strategies (sequential BFS, multi-threaded, priority-based...),
/// accessing the materialized topology and configuration via `rt`.
pub trait Scheduler {
    /// Drive a single tick.
    fn tick(
        &self,
        rt: &mut crate::runtime::Runtime,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError>;
}

/// Sequential scheduler: single-threaded BFS driving + fairness quota (`Runtime::drive_sequential`).
pub struct SequentialScheduler;

impl Scheduler for SequentialScheduler {
    fn tick(
        &self,
        rt: &mut crate::runtime::Runtime,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        rt.drive_sequential(inputs)
    }
}

/// Parallel scheduler: one OS thread per machine + channel carrier (`Runtime::drive_parallel`).
pub struct ParallelScheduler {
    /// Number of worker threads (declared parameter; the actual driving reads `RuntimeConfig::mode`).
    #[allow(dead_code)]
    pub workers: u32,
}

impl Scheduler for ParallelScheduler {
    fn tick(
        &self,
        rt: &mut crate::runtime::Runtime,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        rt.drive_parallel(inputs)
    }
}

/// Construct the default scheduler (selected by execution mode).
pub(crate) fn default_scheduler(config: &RuntimeConfig) -> Box<dyn Scheduler> {
    match config.mode {
        crate::config::ExecMode::Parallel(n) if n >= 1 => {
            Box::new(ParallelScheduler { workers: n })
        }
        _ => Box::new(SequentialScheduler),
    }
}
