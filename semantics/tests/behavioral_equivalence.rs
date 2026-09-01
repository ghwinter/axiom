//! C8-2: behavioral counterexample search (T5) — minimal, zero-dependency.
//!
//! T5 states that substitutability is behavioral equivalence (bisimulation),
//! NOT structural isomorphism. The syntax-level approximation (structural
//! equivalence, the C8 e-graph domain) must never be presented as a behavioral
//! verdict. This module makes that negative claim mechanically visible:
//!
//! - `structural_isomorphism_does_not_imply_behavior`: two shape-identical
//!   cells (same In/Out types) with different behavior — a small-domain
//!   exhaustive search finds a separating input. Structural isomorphism ⊬
//!   behavioral equivalence, witnessed by a concrete counterexample.
//! - `behavioral_equivalence_holds_within_depth`: two composites expected to be
//!   behaviorally equivalent agree on the same small domain — the
//!   verifiable fragment of T6 (sampling verification, modality ③), never
//!   a proof.
//!
//! Honest scope note (A5): exhaustive search over a bounded domain is
//! sampling, not a universal judgment; it falsifies, it does not prove.

use axiom::cell_core::{Chain, PortCell};

// ── Shape-identical cells with different behavior ──────────────────────────

struct Inc; // x -> x+1
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_add(1)
    }
}

struct Inc2; // x -> x+2 (same shape: i32 -> i32, different behavior)
impl PortCell for Inc2 {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_add(2)
    }
}

struct Double; // x -> 2x
impl PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_mul(2)
    }
}

/// Small-domain counterexample search: the first input in `domain` where the
/// two step functions produce different outputs, or `None` (no separation on
/// the sampled domain).
fn separator<F1, F2, I>(f1: &mut F1, f2: &mut F2, domain: I) -> Option<i32>
where
    F1: FnMut(i32) -> i32,
    F2: FnMut(i32) -> i32,
    I: IntoIterator<Item = i32>,
{
    domain.into_iter().find(|&x| f1(x) != f2(x))
}

#[test]
fn structural_isomorphism_does_not_imply_behavior() {
    // Same shape (i32 -> i32), different behavior; the separating input is
    // found immediately. T5: substitutability is behavioral, not structural.
    let mut inc = |x: i32| Inc::step(&mut (), x);
    let mut inc2 = |x: i32| Inc2::step(&mut (), x);
    let sep = separator(&mut inc, &mut inc2, -8..=8);
    assert!(
        sep.is_some(),
        "shape-identical cells with different behavior must be separated by some input"
    );
    assert_eq!(sep, Some(-8), "域首点 x=-8: Inc=-7, Inc2=-6 即分离");
}

#[test]
fn behavioral_equivalence_holds_within_depth() {
    // Chain<Inc, Double> ((x+1)*2) vs the hand-unrolled equivalent: agree on
    // the sampled domain. This is the verifiable fragment of T6 (modality ③
    // sampling), not a universal proof.
    type Comp = Chain<Inc, Double>;
    let mut st = <Comp as PortCell>::State::default();
    for x in -64..=64i32 {
        let via_chain = <Comp as PortCell>::step(&mut st, x);
        let by_hand = (x.wrapping_add(1)).wrapping_mul(2);
        assert_eq!(via_chain, by_hand, "sampled domain: chain ≡ hand-unrolled at {x}");
    }
}

#[test]
fn different_composites_are_separated_where_behavior_differs() {
    // Chain<Inc, Inc> (x+2) vs Chain<Inc2, Id-like>? Use Inc vs Inc2 again at
    // composite level: Chain<Inc, Inc> = x+2 == Inc2 on every input (they
    // ARE behaviorally equal), so the separator must NOT find a gap; whereas
    // Chain<Inc, Inc> vs Inc (x+1 vs x+2) IS separated. This shows the search
    // distinguishes real equivalence from near-miss.
    type TwoInc = Chain<Inc, Inc>;
    let mut st = <TwoInc as PortCell>::State::default();
    let mut two_inc = |x: i32| <TwoInc as PortCell>::step(&mut st, x);
    let mut inc2 = |x: i32| Inc2::step(&mut (), x);
    // Behaviorally equal structures: no counterexample on the sampled domain.
    assert_eq!(
        separator(&mut two_inc, &mut inc2, -16..=16),
        None,
        "Chain<Inc,Inc> ≡ Inc2 on every sampled input"
    );
    // Near-miss (x+2 vs x+1): separated immediately.
    let mut inc = |x: i32| Inc::step(&mut (), x);
    assert!(separator(&mut inc, &mut inc2, -16..=16).is_some(), "x+1 vs x+2 separated");
}