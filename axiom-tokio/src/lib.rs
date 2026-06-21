//! ## axiom-tokio: run axiom Machines on the Tokio runtime.
//!
//! Each Machine is spawned as a `tokio::task::spawn_blocking`, one per OS thread.
//! This keeps `Machine::process()` synchronous inside the task while allowing
//! Tokio's async runtime to handle IO-bound work (networking, timers).
//!
//! # Panic Safety (工程修补 7.5.2)
//!
//! If `process()` panics inside `spawn_blocking`, the state is dropped without
//! calling `cleanup()` — "safe but leaky". This matches `LinearRuntime`'s
//! `CleanupGuard` behavior. We use `catch_unwind` to capture the panic,
//! preventing it from propagating and allowing orderly error return.
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
use std::panic::{catch_unwind, AssertUnwindSafe};
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

/// Result of the process loop inside spawn_blocking.
enum ProcessResult<M: Machine> {
    Ok(M::State, Vec<M::Output>),
    /// process() panicked; state is dropped, cleanup skipped (工程修补 7.5.2).
    Panicked(String),
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
    ///
    /// If `process()` panics, `cleanup()` is skipped (safe but leaky).
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

        // process loop on a blocking thread, with panic capture (工程修补 7.5.2)
        let result = task::spawn_blocking(move || {
            // AssertUnwindSafe: we accept that state may be in an inconsistent
            // state after panic; we will drop it without calling cleanup.
            let mut state = AssertUnwindSafe(state);
            let ctx_ref = AssertUnwindSafe(&*ctx_for_task);
            let inputs = AssertUnwindSafe(inputs);

            let result = catch_unwind(move || {
                let mut outputs = Vec::new();
                for input in inputs.0 {
                    match M::process(&mut state.0, ctx_ref.0, input) {
                        ProcessOutput::Yield(out) => outputs.push(out),
                        ProcessOutput::YieldMulti(outs) => outputs.extend(outs),
                        ProcessOutput::Idle => {}
                        ProcessOutput::Done => break,
                    }
                }
                (state.0, outputs)
            });

            match result {
                Ok((state, outputs)) => ProcessResult::Ok(state, outputs),
                Err(panic_payload) => {
                    let msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    // state is consumed by unwind; dropped without cleanup.
                    ProcessResult::Panicked(msg)
                }
            }
        })
        .await
        .map_err(|e| TokioRunError::TaskPanicked(e.to_string()))?;

        match result {
            ProcessResult::Ok(state, outputs) => {
                M::cleanup(state, &*ctx)
                    .map_err(|e| TokioRunError::CleanupFailed(e.to_string()))?;
                Ok(outputs)
            }
            ProcessResult::Panicked(msg) => {
                // 工程修补 7.5.2：cleanup 被跳过，state 已在 unwind 中 drop。
                Err(TokioRunError::TaskPanicked(msg))
            }
        }
    }
}
