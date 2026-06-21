//! ## axiom-tokio: run axiom Machines on the Tokio runtime.
//!
//! Each Machine is spawned as a `tokio::task::spawn_blocking`, one per OS thread.
//! This keeps `Machine::process()` synchronous inside the task while allowing
//! Tokio's async runtime to handle IO-bound work (networking, timers).
//!
//! # Example
//!
//! ```ignore
//! use axiom_tokio::TokioRuntime;
//! use axiom::prelude_all::*;
//!
//! let outputs = TokioRuntime::run::<MyMachine>("instance", inputs).await;
//! ```

use axiom::machine::{Machine, ProcessOutput};
use axiom::port::MachineContext;
use std::sync::Arc;
use tokio::task;

/// Errors that can occur during Tokio-based execution.
#[derive(Debug)]
pub enum TokioRunError {
    InitFailed(String),
    TaskPanicked(String),
    CleanupFailed(String),
}

impl core::fmt::Display for TokioRunError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InitFailed(s) => write!(f, "init: {}", s),
            Self::TaskPanicked(s) => write!(f, "task panic: {}", s),
            Self::CleanupFailed(s) => write!(f, "cleanup: {}", s),
        }
    }
}

/// Multi-threaded Tokio runtime adapter.
pub struct TokioRuntime;

impl TokioRuntime {
    /// Run a Machine on the Tokio blocking thread pool.
    ///
    /// 1. `init()` on the current async context.
    /// 2. `process()` loop on a blocking thread (spawn_blocking).
    /// 3. `cleanup()` after join.
    ///
    /// Returns the collected outputs. `Yield` produces one entry;
    /// `YieldMulti` produces multiple entries (fan-out).
    pub async fn run<M: Machine>(
        name: &'static str,
        inputs: Vec<M::Input>,
    ) -> Result<Vec<M::Output>, TokioRunError> {
        let ctx = MachineContext::new(name);

        // init on the async context
        let state = M::init(&ctx).map_err(|e| TokioRunError::InitFailed(e.to_string()))?;

        // share the context between the blocking task and cleanup
        let ctx = Arc::new(ctx);
        let ctx_for_task = Arc::clone(&ctx);

        // process loop on a blocking thread
        let outputs = task::spawn_blocking(move || {
            let mut state = state;
            let mut outputs = Vec::new();
            for input in inputs {
                match M::process(&mut state, &*ctx_for_task, input) {
                    ProcessOutput::Yield(out) => outputs.push(out),
                    ProcessOutput::YieldMulti(outs) => outputs.extend(outs),
                    ProcessOutput::Idle => {}
                    ProcessOutput::Done => break,
                }
            }
            (state, outputs)
        })
        .await
        .map_err(|e| TokioRunError::TaskPanicked(e.to_string()))?;

        let (state, outputs) = outputs;

        M::cleanup(state, &*ctx).map_err(|e| TokioRunError::CleanupFailed(e.to_string()))?;

        Ok(outputs)
    }
}
