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
use axiom::port::{MachineContext, Lifecycle};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::{self, JoinHandle};

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
        Self::run_with_ctx::<M>(ctx, inputs).await
    }

    /// Run a Machine with a pre-configured MachineContext.
    /// Use this when machines need access to injected values (e.g., Config).
    pub async fn run_with_ctx<M: Machine>(
        ctx: MachineContext,
        inputs: Vec<M::Input>,
    ) -> Result<Vec<M::Output>, TokioRunError> {
        // init on the async context
        let state = M::init(&ctx).map_err(|e| TokioRunError::InitFailed(e.to_string()))?;

        // share the context between the blocking task and cleanup
        let ctx = Arc::new(ctx);
        let ctx_for_task = Arc::clone(&ctx);

        // process loop on a blocking thread, with panic capture (工程修补 7.5.2)
        let result = task::spawn_blocking(move || {
            // AssertUnwindSafe: we accept that state may be in an inconsistent
            // state after panic; we will drop it without calling cleanup.
            let result = catch_unwind(AssertUnwindSafe(move || {
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
            }));

            match result {
                Ok((state, outputs)) => ProcessResult::Ok::<M>(state, outputs),
                Err(panic_payload) => {
                    let msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    // state is consumed by unwind; dropped without cleanup.
                    ProcessResult::Panicked::<M>(msg)
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

    /// Spawn a Machine as a long-running tokio task.
    ///
    /// Unlike `run`/`run_with_ctx` (which accept a pre-built `Vec<Input>` and
    /// run to completion), `spawn` lets a Machine run **continuously**: it reads
    /// inputs from `input_rx` as they arrive, processes them, and sends outputs
    /// to `output_tx`. The task completes when:
    ///
    /// - `input_rx` closes (all upstream senders dropped), or
    /// - `process()` returns `Done`, or
    /// - `output_tx` closes (downstream dropped).
    ///
    /// This is the concurrent counterpart to `run`, enabling multi-Machine
    /// pipelines where each Machine runs on its own task and communicates via
    /// tokio channels.
    ///
    /// # Panic Safety (工程修补 7.5.2)
    ///
    /// If `process()` panics, `cleanup()` is skipped — "safe but leaky".
    /// The panic is captured and returned as `Err(TaskPanicked)` in the
    /// `JoinHandle` result; the task does not propagate the panic.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (in_tx, in_rx) = mpsc::channel::<MyInput>(64);
    /// let (out_tx, mut out_rx) = mpsc::channel::<MyOutput>(64);
    /// let handle = TokioRuntime::spawn::<MyMachine>(ctx, in_rx, out_tx);
    /// // feed inputs...
    /// drop(in_tx); // close input → Machine finishes
    /// handle.await??;
    /// ```
    pub fn spawn<M: Machine>(
        ctx: MachineContext,
        mut input_rx: mpsc::Receiver<M::Input>,
        output_tx: mpsc::Sender<M::Output>,
    ) -> JoinHandle<Result<(), TokioRunError>> {
        tokio::spawn(async move {
            // Phase 1: init
            let ctx = Arc::new(ctx);
            let mut state =
                M::init(&ctx).map_err(|e| TokioRunError::InitFailed(e.to_string()))?;
            ctx.set_lifecycle(Lifecycle::Running);

            // Phase 2: process loop
            loop {
                let input = match input_rx.recv().await {
                    Some(input) => input,
                    None => break, // upstream closed → done
                };

                // Panic-safe process (工程修补 7.5.2)
                let result = catch_unwind(AssertUnwindSafe(|| {
                    M::process(&mut state, &ctx, input)
                }));

                match result {
                    Ok(process_output) => {
                        let is_done = matches!(process_output, ProcessOutput::Done);
                        let outputs: Vec<M::Output> = match process_output {
                            ProcessOutput::Yield(o) => vec![o],
                            ProcessOutput::YieldMulti(os) => os,
                            ProcessOutput::Idle => vec![],
                            ProcessOutput::Done => vec![],
                        };
                        for out in outputs {
                            if output_tx.send(out).await.is_err() {
                                // downstream closed → stop
                                break;
                            }
                        }
                        if is_done {
                            break;
                        }
                    }
                    Err(panic_payload) => {
                        // state in inconsistent state; cleanup skipped (safe but leaky)
                        let msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                            s.to_string()
                        } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                            s.clone()
                        } else {
                            "unknown panic".to_string()
                        };
                        ctx.set_lifecycle(Lifecycle::Stopped);
                        return Err(TokioRunError::TaskPanicked(msg));
                    }
                }
            }

            // Phase 3: cleanup
            ctx.set_lifecycle(Lifecycle::Stopping);
            M::cleanup(state, &ctx)
                .map_err(|e| TokioRunError::CleanupFailed(e.to_string()))?;
            ctx.set_lifecycle(Lifecycle::Stopped);
            Ok(())
        })
    }
}
