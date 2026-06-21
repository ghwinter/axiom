/// Time abstraction — a replaceable time source.
///
/// In production, `RealClock` provides wall-clock time.
/// In backtest/replay, `ReplayClock` reads time from recorded data.
///
/// Both implement the same `Clock` trait. The deployer chooses which one
/// to connect to each machine's implicit `time_in` port.

// ── Time tick ─────────────────────────────────────────────────────────────────

/// A moment in time, represented as nanoseconds since an arbitrary epoch.
///
/// The epoch is context-dependent:
/// - `RealClock`: nanoseconds since Unix epoch.
/// - `ReplayClock`: nanoseconds since the start of the recorded data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeTick {
    pub ns: u64,
}

impl TimeTick {
    pub const fn from_nanos(ns: u64) -> Self {
        Self { ns }
    }

    pub fn from_millis(ms: u64) -> Self {
        Self { ns: ms * 1_000_000 }
    }

    pub fn as_millis(&self) -> u64 {
        self.ns / 1_000_000
    }

    pub fn as_secs_f64(&self) -> f64 {
        self.ns as f64 / 1_000_000_000.0
    }

    pub fn duration_since(&self, earlier: TimeTick) -> core::time::Duration {
        let diff = self.ns.saturating_sub(earlier.ns);
        core::time::Duration::from_nanos(diff)
    }
}

// ── Clock trait ───────────────────────────────────────────────────────────────

/// A source of `TimeTick`s.
///
/// Implementations:
/// - `RealClock` — wall-clock time.
/// - `ReplayClock` — from recorded data.
/// - `SimulatedClock` — controlled step-by-step for testing.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> TimeTick;

    /// Advance the clock by `step` (no-op for real clocks, meaningful for
    /// simulated/replay clocks).
    fn advance(&mut self, step: core::time::Duration) {
        let _ = step;
    }
}

// ── Built-in clock implementations ────────────────────────────────────────────

/// Wall-clock time source.
///
/// Wraps `std::time::SystemTime` or a platform-specific monotonic clock.
#[derive(Debug, Default)]
pub struct RealClock;

impl RealClock {
    pub fn new() -> Self {
        Self
    }
}

impl Clock for RealClock {
    fn now(&self) -> TimeTick {
        #[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            TimeTick::from_nanos(now.as_nanos() as u64)
        }
        #[cfg(any(target_arch = "wasm32", not(feature = "std")))]
        {
            // Fallback for no_std / wasm: tick counter.
            TimeTick::from_nanos(0)
        }
    }
}

// ── Replay clock ──────────────────────────────────────────────────────────────

/// A replayable time source that yields pre-recorded timestamps.
///
/// Used in backtesting: the time advances only when `advance` is called,
/// and it jumps to the next recorded timestamp rather than flowing continuously.
#[derive(Debug)]
pub struct ReplayClock {
    ticks: Vec<TimeTick>,
    index: usize,
}

impl ReplayClock {
    pub fn new(ticks: Vec<TimeTick>) -> Self {
        Self { ticks, index: 0 }
    }

    pub fn current_index(&self) -> usize {
        self.index
    }

    pub fn total_ticks(&self) -> usize {
        self.ticks.len()
    }

    pub fn is_exhausted(&self) -> bool {
        self.index >= self.ticks.len()
    }
}

impl Clock for ReplayClock {
    fn now(&self) -> TimeTick {
        self.ticks.get(self.index).copied().unwrap_or(TimeTick::from_nanos(0))
    }

    fn advance(&mut self, _step: core::time::Duration) {
        if self.index < self.ticks.len() {
            self.index = self.index.saturating_add(1);
        }
    }
}
