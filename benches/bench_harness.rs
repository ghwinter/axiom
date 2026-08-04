//! Minimal benchmark harness — zero external dependencies.
//!
//! Uses std::time::Instant for timing. Runs each benchmark with automatic
//! iteration count detection, collects samples, and reports statistics.
//!
//! This is intentionally simple: no HTML reports, no statistical analysis
//! beyond mean/median/p99/min/max. For rigorous benchmarking, use criterion.
//! But for axiom's purposes (verifying O(n) vs O(n²) scaling, detecting
//! regressions), this is sufficient and keeps the zero-dependency principle.

use std::time::{Duration, Instant};
use std::io::{self, Write};

/// Result of a single benchmark run.
#[derive(Debug, Clone)]
pub struct BenchResult {
    pub name: String,
    pub iterations: u64,
    /// Total wall-clock time across all measurement samples.
    // Kept for API completeness / future reporting; not used by Display.
    #[allow(dead_code)]
    pub total_time: Duration,
    pub mean: Duration,
    pub median: Duration,
    pub p99: Duration,
    /// Minimum per-iteration time observed.
    #[allow(dead_code)]
    pub min: Duration,
    /// Maximum per-iteration time observed.
    #[allow(dead_code)]
    pub max: Duration,
}

impl BenchResult {
    fn ops_per_sec(&self) -> f64 {
        if self.mean.is_zero() {
            return f64::INFINITY;
        }
        1_000_000_000.0 / self.mean.as_nanos() as f64
    }

    fn mean_ns(&self) -> f64 {
        self.mean.as_nanos() as f64
    }
}

impl std::fmt::Display for BenchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:<45} {:>8} iters | {:>10.1} ns/iter | med {:>10.1} ns | p99 {:>10.1} ns | {:>12.0} ops/s",
            self.name,
            self.iterations,
            self.mean_ns(),
            self.median.as_nanos(),
            self.p99.as_nanos(),
            self.ops_per_sec(),
        )
    }
}

/// A benchmark group with a shared label prefix.
pub struct BenchGroup {
    label: String,
    results: Vec<BenchResult>,
}

impl BenchGroup {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            results: Vec::new(),
        }
    }

    /// Benchmark a closure with automatic iteration count detection.
    ///
    /// The closure receives no arguments — pre-build any data before calling
    /// this method and capture it by reference in the closure.
    pub fn bench<F: Fn()>(&mut self, name: &str, f: F) {
        let full_name = format!("{}/{}", self.label, name);
        let result = run_bench(&full_name, f);
        println!("{}", result);
        let _ = io::stdout().flush();
        self.results.push(result);
    }

    /// Print a summary table of all results in this group.
    pub fn finish(&self) {
        println!("\n── Summary: {} ──────────────────────────────────────────", self.label);
        for r in &self.results {
            println!("{}", r);
        }
        println!();
    }
}

/// Run a benchmark with automatic iteration count detection.
fn run_bench<F: Fn()>(name: &str, f: F) -> BenchResult {
    // ── Warmup: find a suitable iteration count ────────────────────────────
    let warmup_target = Duration::from_millis(100);
    let mut iter_count = 1u64;

    loop {
        let start = Instant::now();
        for _ in 0..iter_count {
            f();
        }
        let elapsed = start.elapsed();
        if elapsed >= warmup_target || iter_count >= 1_000_000 {
            break;
        }
        if elapsed > Duration::from_micros(10) {
            iter_count = ((warmup_target.as_nanos() / elapsed.as_nanos().max(1))
                * iter_count as u128) as u64;
            iter_count = iter_count.clamp(1, 1_000_000);
        } else {
            iter_count *= 10;
        }
    }

    // ── Measurement phase ──────────────────────────────────────────────────
    let measure_target = Duration::from_millis(500);
    let mut samples: Vec<Duration> = Vec::new();
    let mut total_time = Duration::ZERO;
    let mut total_iters = 0u64;

    while total_time < measure_target {
        let start = Instant::now();
        for _ in 0..iter_count {
            f();
        }
        let elapsed = start.elapsed();
        let per_iter = elapsed / iter_count as u32;
        samples.push(per_iter);
        total_time += elapsed;
        total_iters += iter_count;
    }

    // ── Statistics ─────────────────────────────────────────────────────────
    samples.sort();
    let n = samples.len();
    let mean_ns = samples.iter().map(|d| d.as_nanos() as f64).sum::<f64>() / n as f64;
    let median = samples[n / 2];
    let p99_idx = ((n as f64) * 0.99) as usize;

    BenchResult {
        name: name.to_string(),
        iterations: total_iters,
        total_time,
        mean: Duration::from_nanos(mean_ns as u64),
        median,
        p99: samples[p99_idx.min(n - 1)],
        min: samples[0],
        max: samples[n - 1],
    }
}
