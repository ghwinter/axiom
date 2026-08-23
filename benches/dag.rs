//! DAG (diamond) zero-cost benchmark — generic composite vs handwritten vs type erasure.
//!
//! Companion to `benches/chain.rs`: the same three-way comparison on the
//! serial-parallel shape (`Diamond` = split + two branches + merge), proving
//! that the zero-cost promise extends beyond linear chains.
//!
//! Methodology (see `bench_common.rs`): warmup → interleaved rounds → min-of-N,
//! plus a handwritten self-noise floor so the headline delta carries its own
//! uncertainty. Single-shot timings were retired after they showed 2.7–6.1%
//! order-dependent variance (measurement artifact, not abstraction cost).
//!
//! Run: `cargo bench --bench dag` (harness = false; built-in minimal timing).
//!
//! Bench is a measurement script: under debug builds (`--all-targets`) the
//! measurement body is cfg'd out, so its types/fns look "unused" to debug —
//! expected, hence file-level allows.

#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]

#[path = "bench_common.rs"]
mod bench_common;

use std::hint::black_box;

use axiom::cell_core::{Diamond, PortCell, Repeat};

struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x + 1
    }
}

struct Scaler;
impl PortCell for Scaler {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x * 2
    }
}

/// Sink: sums both branch outputs into one value.
struct SumPair;
impl PortCell for SumPair {
    type In = (i32, i32);
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), (a, b): (i32, i32)) -> i32 {
        a.wrapping_add(b)
    }
}

type Dia = Diamond<Inc, Repeat<2, Inc>, Scaler, SumPair>;

/// A. Static composite path: one monomorphized `step` per message.
fn body_composite(st: &mut <Dia as PortCell>::State, inputs: &[i32]) -> i64 {
    let mut acc: i64 = 0;
    for &x in inputs {
        acc ^= black_box(<Dia as PortCell>::step(st, black_box(x))) as i64;
    }
    acc
}

/// B. Handwritten baseline: identical dataflow as nested direct calls.
fn body_hand(
    ss: &mut (),
    s1: &mut (),
    s2: &mut (),
    sd: &mut (),
    inputs: &[i32],
) -> i64 {
    let mut acc: i64 = 0;
    for &x in inputs {
        let mid = Inc::step(ss, black_box(x));
        let b1 = Inc::step(s1, mid);
        let b1 = Inc::step(s1, b1); // Repeat<2, Inc> unrolled
        let b2 = Scaler::step(s2, mid);
        let out = SumPair::step(sd, (b1, b2));
        acc ^= black_box(out) as i64;
    }
    acc
}

/// C. Type-erased simulation: `Box<dyn Any>` at each hop (dynamic tax contrast).
fn body_erased(
    es: &mut (),
    e1: &mut (),
    e2: &mut (),
    ed: &mut (),
    inputs: &[i32],
) -> i64 {
    let mut acc: i64 = 0;
    for &x in inputs {
        let boxed: Box<dyn core::any::Any + Send> = Box::new(Inc::step(es, black_box(x)));
        let mid = boxed.downcast::<i32>().unwrap();
        let b1 = Inc::step(e1, *mid);
        let b1 = Inc::step(e1, b1); // Repeat<2, Inc> unrolled
        let b2 = Scaler::step(e2, *mid);
        let pair: Box<dyn core::any::Any + Send> = Box::new((b1, b2));
        let out = SumPair::step(ed, *pair.downcast::<(i32, i32)>().unwrap());
        acc ^= black_box(out) as i64;
    }
    acc
}

fn main() {
    // Evidence hygiene (see file header): unoptimized numbers are meaningless,
    // so the whole measurement body is cfg'd out under --all-targets/debug.
    #[cfg(debug_assertions)]
    println!("dag: debug build — benchmark skipped; run `cargo bench --bench dag`");
    #[cfg(not(debug_assertions))]
    run();
}

fn run() {
    use bench_common::{ITERS, ROUNDS, WARMUP_PASSES, measure3, noise_floor_pct, pct_over};

    let inputs: Vec<i32> = (0..ITERS as i64).map(|i| (i % 4096) as i32).collect();
    let mut st_c = <Dia as PortCell>::State::default();
    let (mut hs, mut h1, mut h2, mut hd) = ((), (), (), ());
    let (mut es, mut e1, mut e2, mut ed) = ((), (), (), ());

    // Correctness gate (and an extra warmup pass): all variants must agree bit-exactly.
    let (rc, rh, re) = (
        body_composite(&mut st_c, &inputs),
        body_hand(&mut hs, &mut h1, &mut h2, &mut hd, &inputs),
        body_erased(&mut es, &mut e1, &mut e2, &mut ed, &inputs),
    );
    assert_eq!(rc, rh, "composite and handwritten must agree bit-exactly");
    assert_eq!(rc, re, "erased path must stay semantically equivalent");

    let [ns_c, ns_h, ns_e] = measure3(
        || {
            body_composite(&mut st_c, &inputs);
        },
        || {
            body_hand(&mut hs, &mut h1, &mut h2, &mut hd, &inputs);
        },
        || {
            body_erased(&mut es, &mut e1, &mut e2, &mut ed, &inputs);
        },
    );

    // Self-noise floor: rerun the handwritten baseline in an independent set.
    let floor = noise_floor_pct(ns_h, || {
        body_hand(&mut hs, &mut h1, &mut h2, &mut hd, &inputs);
    });

    let delta = pct_over(ns_c, ns_h);
    let tax_x = ns_e as f64 / ns_h as f64;
    println!(
        "dag methodology: warmup {WARMUP_PASSES}×3, {ROUNDS} interleaved rounds, {ITERS} msgs/pass, statistic = min"
    );
    println!(
        "dag[composite]   {:>7.3} ns/msg  (min {} µs/pass)",
        ns_c as f64 / ITERS as f64,
        ns_c / 1000
    );
    println!(
        "dag[handwritten] {:>7.3} ns/msg  (min {} µs/pass)",
        ns_h as f64 / ITERS as f64,
        ns_h / 1000
    );
    println!(
        "dag[type-erase]  {:>7.3} ns/msg  ({tax_x:.1}× dynamic-tax contrast)",
        ns_e as f64 / ITERS as f64
    );
    println!(
        "dag verdict: Δ(composite−handwritten) = {delta:+.2}% | self-noise ±{floor:.2}% | gate <5% => {}",
        if delta.abs() <= 5.0 { "PASS" } else { "FAIL" }
    );
}
