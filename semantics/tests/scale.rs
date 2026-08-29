//! C12 scale evidence: evidence side of the scale-neutrality claim
//! (meta-foundations Remark 7.1 / note 9.9: a subsystem is a same-scale cell).
//!
//! Four assertions turn the claim into checkable facts:
//! - 64 cells compose and drive (scheduler-row + deterministic-island hybrid
//!   form, Prop. 7.3);
//! - scale recursion is semantic identity: `Rep<8, Rep<8, Inc>>` == `Rep<64, Inc>`
//!   (64 cells = 8 subsystems of 8 cells each; nested repetition is exactly the
//!   recursive same-scale structure);
//! - the blueprint stays zero-sized at scale (no runtime presence);
//! - wiring legality (T1) holds across the composite's seams.
//!
//! Comments in English per project convention for new code.

use axiom::cell_core::{
    Chain, Conforms, Diamond, PortCell, Rep, Wire, assert_conforms, assert_wiring,
    blueprint_is_zero_sized, drive,
};

// ── Test cells ─────────────────────────────────────────────────────────────

struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_add(1)
    }
}

/// Sink of a diamond: sums the two branch outputs (deterministic island).
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

// ── The 64-cell blueprints under test ──────────────────────────────────────
//
// (a) Flat chain:   Rep<64, Inc>                                   — 64 cells.
// (b) Nested 8×8:   Rep<8, Rep<8, Inc>>                            — 64 cells, recursively.
// (c) Hybrid form:  Chain<Rep<15, Inc>, Chain<Diamond<Inc,
//                      Rep<16, Inc>, Rep<16, Inc>, SumPair>, Rep<15, Inc>>>
//   scheduler row (15) + deterministic island (src 1 + branches 32 + sink 1)
//   + tail row (15) = 64 cells. Closed form: x -> 15 adds -> island:
//   2*(x+16) -> 15 adds = 2x + 79.

type Flat64 = Rep<64, Inc>;
type Nested8x8 = Rep<8, Rep<8, Inc>>;
type Island = Diamond<Inc, Rep<16, Inc>, Rep<16, Inc>, SumPair>;
type Hybrid64 = Chain<Rep<15, Inc>, Chain<Island, Rep<15, Inc>>>;

#[test]
fn flat_64_chain_drives_with_expected_value() {
    let mut st = <Flat64 as PortCell>::State::default();
    assert_eq!(drive::<Flat64>(&mut st, 5), 69, "64 adds on top of 5");
    assert_eq!(drive::<Flat64>(&mut st, 0), 64);
}

#[test]
fn nested_8x8_is_scale_recursion_identity() {
    // Scale neutrality: 64 cells = 8 subsystems of 8 cells; the nested
    // repetition is semantically identical to the flat form (same-level cell).
    let mut st_a = <Flat64 as PortCell>::State::default();
    let mut st_b = <Nested8x8 as PortCell>::State::default();
    for x in [0i32, 1, 13, -4] {
        assert_eq!(
            drive::<Flat64>(&mut st_a, x),
            drive::<Nested8x8>(&mut st_b, x),
            "nested Rep equals flat Rep at scale 64 (subsystem = same-scale cell)"
        );
    }
    assert_eq!(drive::<Nested8x8>(&mut st_b, 0), 64);
}

#[test]
fn hybrid_scheduler_row_plus_island_64_nodes() {
    // Mixed form (Prop. 7.3): scheduler row + deterministic island + tail row.
    let mut st = <Hybrid64 as PortCell>::State::default();
    assert_eq!(drive::<Hybrid64>(&mut st, 0), 79, "closed form 2x+79 at x=0");
    assert_eq!(drive::<Hybrid64>(&mut st, 10), 99, "closed form 2x+79 at x=10");
}

#[test]
fn scale_blueprints_stay_zero_sized_and_wired() {
    // Zero runtime presence at scale (assertion 3 family): blueprints are ZSTs.
    assert!(blueprint_is_zero_sized::<Flat64>());
    assert!(blueprint_is_zero_sized::<Nested8x8>());
    assert!(blueprint_is_zero_sized::<Hybrid64>());

    // T1 wiring legality holds across the composite's seams.
    assert_wiring::<Rep<15, Inc>, Island>();
    assert_wiring::<Island, Rep<15, Inc>>();
    let _: bool = <() as Conforms<Wire<Rep<15, Inc>, Island>>>::OK;

    // Slot conformance at scale: the 64-node hybrid is an admissible inhabitant
    // of the typed hole (compile-∀, existential at runtime).
    assert_conforms::<axiom::cell_core::Slot<i32, i32>, Hybrid64>();
}