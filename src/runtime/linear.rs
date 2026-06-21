/// Single-threaded, sequential runtime for axiom Machines.
///
/// # Physics
/// - Single OS thread, current thread.
/// - Zero allocation for port connections (inline passing).
/// - No locking, no channels, no async.
/// - init() and cleanup() are called exactly once each.
///
/// # Usage
///
/// ```ignore
/// use axiom::runtime::LinearRuntime;
///
/// let outputs = LinearRuntime::run::<MyMachine>("instance", inputs)?;
/// ```
///
/// For pipelining multiple machines, pass the output of one
/// as the input to the next:
///
/// ```ignore
/// let mid = LinearRuntime::run::<First>("a", inputs)?;
/// let out = LinearRuntime::run::<Second>("b", mid)?;
/// ```

use crate::machine::{Machine, ProcessOutput};
use crate::port::MachineContext;

/// Errors that can occur during linear execution.
#[derive(Debug)]
pub enum LinearError {
    InitFailed(String),
    ProcessPanicked(String),
    CleanupFailed(String),
}

impl core::fmt::Display for LinearError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InitFailed(s) => write!(f, "init failed: {}", s),
            Self::ProcessPanicked(s) => write!(f, "process panicked: {}", s),
            Self::CleanupFailed(s) => write!(f, "cleanup failed: {}", s),
        }
    }
}

/// Single-threaded linear runtime driver.
pub struct LinearRuntime;

impl LinearRuntime {
    /// Run a single Machine instance with the given inputs.
    ///
    /// 1. `M::init()` — creates the initial State.
    /// 2. `M::process()` — called once per input.
    /// 3. `M::cleanup()` — releases resources.
    ///
    /// Returns the collected outputs.
    pub fn run<M: Machine>(
        name: &'static str,
        inputs: Vec<M::Input>,
    ) -> Result<Vec<M::Output>, LinearError> {
        Self::run_with_ctx::<M>(name, &MachineContext::new(name), inputs)
    }

    /// Like `run`, but uses an externally-created MachineContext.
    /// This allows the caller to set up observation or snapshot
    /// hooks before execution begins.
    pub fn run_with_ctx<M: Machine>(
        _name: &'static str,
        ctx: &MachineContext,
        inputs: Vec<M::Input>,
    ) -> Result<Vec<M::Output>, LinearError> {
        let mut state =
            M::init(ctx).map_err(|e| LinearError::InitFailed(e.to_string()))?;

        let mut outputs = Vec::new();

        for input in inputs {
            match M::process(&mut state, ctx, input) {
                ProcessOutput::Yield(out) => outputs.push(out),
                ProcessOutput::Idle => {}
                ProcessOutput::Done => break,
            }
        }

        M::cleanup(state, ctx)
            .map_err(|e| LinearError::CleanupFailed(e.to_string()))?;

        Ok(outputs)
    }
}
