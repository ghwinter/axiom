//! Minimal zero-dependency measurement harness shared by axiom benches.
//!
//! Methodology (honest-measurement rules, fixed after observing 2.7–6.1%
//! single-shot variance):
//!
//! 1. **Warmup** — every variant runs unmeasured passes first, so cold caches
//!    and lazy page faults cannot tax whichever variant happens to go first.
//! 2. **Interleaving** — measured rounds rotate the starting variant, so any
//!    residual position/drift effect spreads evenly instead of piling onto one
//!    variant.
//! 3. **Min-of-N** — the per-variant statistic is the MINIMUM pass time. Noise
//!    only ever adds time, so the minimum converges to true steady-state cost;
//!    means/medians would re-absorb the artifacts the method intends to exclude.
//! 4. **Self-noise floor** — the baseline variant gets a second independent
//!    min-of-N set; the |delta| between the two baselines is printed as the
//!    measurement's own uncertainty. A headline delta is credible only if it
//!    clearly exceeds this floor.

use std::time::Instant;

/// Messages per measured pass.
pub const ITERS: usize = 1_000_000;
/// Unmeasured warmup passes per variant.
pub const WARMUP_PASSES: usize = 2;
/// Measured (interleaved) rounds per variant.
pub const ROUNDS: usize = 8;

/// Time one full pass of `f` in nanoseconds.
pub fn time_pass(mut f: impl FnMut()) -> u128 {
    let t = Instant::now();
    f();
    t.elapsed().as_nanos()
}

/// Interleaved measurement of three variants (A/B/C).
///
/// Runs `WARMUP_PASSES` warmup rounds, then `ROUNDS` measured rounds rotating
/// the start order (`A→B→C`, `B→C→A`, `C→A→B`, …). Returns
/// `[min_ns_a, min_ns_b, min_ns_c]`.
pub fn measure3(
    mut a: impl FnMut(),
    mut b: impl FnMut(),
    mut c: impl FnMut(),
) -> [u128; 3] {
    for _ in 0..WARMUP_PASSES {
        a();
        b();
        c();
    }
    let (mut ma, mut mb, mut mc) = (u128::MAX, u128::MAX, u128::MAX);
    for r in 0..ROUNDS {
        match r % 3 {
            0 => {
                ma = ma.min(time_pass(&mut a));
                mb = mb.min(time_pass(&mut b));
                mc = mc.min(time_pass(&mut c));
            }
            1 => {
                mb = mb.min(time_pass(&mut b));
                mc = mc.min(time_pass(&mut c));
                ma = ma.min(time_pass(&mut a));
            }
            _ => {
                mc = mc.min(time_pass(&mut c));
                ma = ma.min(time_pass(&mut a));
                mb = mb.min(time_pass(&mut b));
            }
        }
    }
    [ma, mb, mc]
}

/// Noise-floor estimate (modality ③ of measurement honesty): rerun the
/// baseline variant in a second independent min-of-N set and report the
/// relative drift of the two baselines.
pub fn noise_floor_pct(baseline_first: u128, mut baseline_again: impl FnMut()) -> f64 {
    let mut second = u128::MAX;
    for _ in 0..ROUNDS {
        second = second.min(time_pass(&mut baseline_again));
    }
    (second as f64 - baseline_first as f64).abs() / baseline_first as f64 * 100.0
}

/// Percentage delta of `x` over reference `ref_`.
pub fn pct_over(x: u128, ref_: u128) -> f64 {
    (x as f64 - ref_ as f64) / ref_ as f64 * 100.0
}
