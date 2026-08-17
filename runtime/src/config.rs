//! Runtime configuration — execution mode and physical parameters centralized here.

/// The runtime's execution mode — single-threaded vs multi-threaded is a configuration parameter, not two separate types.
///
/// - `Inline`: executed directly on the caller's thread, zero threading overhead
/// - `Sequential`: single-threaded sequential loop
/// - `Parallel(n)`: scheduled in parallel across N worker threads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    Inline,
    Sequential,
    Parallel(u32),
}

impl Default for ExecMode {
    fn default() -> Self {
        ExecMode::Sequential
    }
}

/// Runtime configuration — all physical execution parameters centralized here.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Execution mode (threading strategy).
    pub mode: ExecMode,
    /// Maximum number of process calls per tick in the driver loop (prevents
    /// infinite loops). `None` = unlimited.
    pub max_ticks: Option<u64>,
    /// Fairness quota: the maximum number of inputs each machine may process per
    /// **round**.
    ///
    /// `None` = unlimited (FIFO propagation level by level, the default).
    /// `Some(n)` = once a machine reaches the quota, its remaining messages are
    /// deferred to the next round (other machines take priority) — prevents a
    /// single flooding source from starving others (tick fairness, derived
    /// from the fairness cap mode of mailbox polling). `0` = invalid
    /// (equivalent to `None`).
    pub max_messages_per_machine: Option<u64>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            mode: ExecMode::default(),
            max_ticks: Some(1_000_000),
            max_messages_per_machine: None,
        }
    }
}

impl RuntimeConfig {
    pub fn inline() -> Self { Self { mode: ExecMode::Inline, max_ticks: None, max_messages_per_machine: None } }
    pub fn sequential() -> Self { Self { mode: ExecMode::Sequential, max_ticks: Some(1_000_000), max_messages_per_machine: None } }
    pub fn parallel(n: u32) -> Self { Self { mode: ExecMode::Parallel(n), max_ticks: Some(10_000_000), max_messages_per_machine: None } }
}
