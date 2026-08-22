//! DAG (diamond) zero-cost benchmark — generic composite vs handwritten vs type erasure.
//!
//! Companion to `benches/chain.rs`: the same three-way comparison on the
//! serial-parallel shape (`Diamond` = split + two branches + merge), proving
//! that the zero-cost promise extends beyond linear chains:
//!
//! - A. Static composite: `drive::<Diamond<..>>` — fully type-parameterized,
//!   monomorphized, zero allocation. The claim under test: t(composite) ≈
//!   t(handwritten) within noise (< 5%).
//! - B. Handwritten baseline: the same dataflow written as plain nested calls.
//! - C. Type-erased simulation: `Box<dyn Any>` per message hop — the dynamic
//!   tax lower bound, for contrast.
//!
//! Run: `cargo bench --bench dag` (harness = false; built-in minimal timing).
//!
//! Bench is a measurement script: under debug builds (`--all-targets`) the
//! measurement body is cfg'd out, so its types/fns look "unused" to debug —
//! expected, hence file-level allows.

#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]

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

fn main() {
    // Evidence hygiene (see file header): unoptimized numbers are meaningless,
    // so the whole measurement body is cfg'd out under --all-targets/debug.
    #[cfg(debug_assertions)]
    println!("dag: debug build — benchmark skipped; run `cargo bench --bench dag`");
    #[cfg(not(debug_assertions))]
    run();
}

#[cfg(not(debug_assertions))]
fn run() {
    const N: usize = 1_000_000;
    let inputs: Vec<i32> = (0..N as i64).map(|i| (i % 4096) as i32).collect();

    // A. Static composite path: one monomorphized `step` per message.
    let mut st = <Dia as PortCell>::State::default();
    let t0 = now();
    let mut acc: i64 = 0;
    for &x in &inputs {
        acc ^= black_box(<Dia as PortCell>::step(&mut st, black_box(x))) as i64;
    }
    let t_static = now() - t0;
    println!("dag: static composite (zero alloc)  {N} = {t_static:?} (acc {acc:#x})");

    // B. Handwritten baseline: identical dataflow as nested direct calls.
    let (mut ss, mut s1, mut s2, mut sd) = ((), (), (), ());
    let t1 = now();
    let mut acc2: i64 = 0;
    for &x in &inputs {
        let mid = Inc::step(&mut ss, black_box(x));
        let b1 = Inc::step(&mut s1, mid);
        let b1 = Inc::step(&mut s1, b1); // Repeat<2, Inc> unrolled
        let b2 = Scaler::step(&mut s2, mid);
        let out = SumPair::step(&mut sd, (b1, b2));
        acc2 ^= black_box(out) as i64;
    }
    let t_hand = now() - t1;
    println!("dag: handwritten baseline           {N} = {t_hand:?} (acc {acc2:#x})");
    assert_eq!(acc, acc2, "composite and handwritten must agree bit-exactly");

    // C. Type-erased simulation: Box<dyn Any> at each hop (dynamic tax contrast).
    let (mut es, mut e1, mut e2, mut ed) = ((), (), (), ());
    let t2 = now();
    let mut acc3: i64 = 0;
    for &x in &inputs {
        let boxed: Box<dyn core::any::Any + Send> = Box::new(Inc::step(&mut es, black_box(x)));
        let mid = boxed.downcast::<i32>().unwrap();
        let b1 = Inc::step(&mut e1, *mid);
        let b1 = Inc::step(&mut e1, b1); // Repeat<2, Inc> unrolled
        let b2 = Scaler::step(&mut e2, *mid);
        let pair: Box<dyn core::any::Any + Send> = Box::new((b1, b2));
        let out = SumPair::step(&mut ed, *pair.downcast::<(i32, i32)>().unwrap());
        acc3 ^= black_box(out) as i64;
    }
    let t_erase = now() - t2;
    println!("dag: type erased (Box<dyn Any>/hop) {N} = {t_erase:?} (acc {acc3:#x})");
    assert_eq!(acc, acc3, "erased path must stay semantically equivalent");

    // Verdict lines: overhead of abstraction, and the tax it avoids.
    let overhead_ns = t_static.saturating_sub(t_hand).as_nanos();
    let ratio = (overhead_ns as f64) / (t_hand.as_nanos().max(1) as f64) * 100.0;
    println!(
        "dag: composite vs handwritten delta = {overhead_ns}ns ({ratio:.2}%) | erased tax delta = {:?}",
        t_erase.saturating_sub(t_static)
    );
}

fn now() -> std::time::Instant {
    std::time::Instant::now()
}
