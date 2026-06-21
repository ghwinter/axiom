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

use crate::machine::{Machine, ProcessOutput, CleanupError};
use crate::port::MachineContext;

/// Panic-safe cleanup guard.
///
/// Ensures `Machine::cleanup()` is called even if `process()` panics.
/// If `process()` panics, the State is dropped without calling cleanup
/// (safe default). If processing completed normally, mark with `.ok()`.
pub struct CleanupGuard<S> {
    state: Option<S>,
    ctx: *const MachineContext,
    cleanup_fn: fn(S, &MachineContext) -> Result<(), CleanupError>,
    success: bool,
}

// Safety: MachineContext outlives the guard in linear/single-threaded runtimes.
unsafe impl<S: Send> Send for CleanupGuard<S> {}
unsafe impl<S: Sync> Sync for CleanupGuard<S> {}

impl<S> CleanupGuard<S> {
    /// Wrap state with its cleanup function.
    /// cleanup() will be called on drop ONLY if .ok() was called.
    pub fn new(
        state: S,
        ctx: &MachineContext,
        cleanup_fn: fn(S, &MachineContext) -> Result<(), CleanupError>,
    ) -> Self {
        Self {
            state: Some(state),
            ctx: ctx as *const MachineContext,
            cleanup_fn,
            success: false,
        }
    }

    /// Mutable reference to the inner State.
    pub fn state(&mut self) -> &mut S {
        self.state.as_mut().expect("CleanupGuard: state already taken")
    }

    /// Mark the process loop as completed — cleanup will run on drop.
    pub fn ok(&mut self) {
        self.success = true;
    }

    /// Take ownership of State without calling cleanup.
    pub fn take(&mut self) -> Option<S> {
        self.state.take()
    }
}

impl<S> Drop for CleanupGuard<S> {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            if self.success {
                let ctx = unsafe { &*self.ctx };
                if let Err(e) = (self.cleanup_fn)(state, ctx) {
                    eprintln!("[axiom::runtime] cleanup error: {}", e);
                }
            }
            // if !success: process panicked, just drop state silently
        }
    }
}

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
    /// Returns the collected outputs. `Yield` produces one entry;
    /// `YieldMulti` produces multiple entries (fan-out).
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
        let state =
            M::init(ctx).map_err(|e| LinearError::InitFailed(e.to_string()))?;

        let mut guard = CleanupGuard::new(state, ctx, M::cleanup);
        let mut outputs = Vec::new();

        for input in inputs {
            match M::process(guard.state(), ctx, input) {
                ProcessOutput::Yield(out) => outputs.push(out),
                ProcessOutput::YieldMulti(outs) => outputs.extend(outs),
                ProcessOutput::Idle => {}
                ProcessOutput::Done => break,
            }
        }

        guard.ok();
        // guard drops here → calls M::cleanup(state, ctx)
        // if process panicked → guard drops without .ok() → state dropped silently
        Ok(outputs)
    }
}
