//! Blueprint integration tests — external (consumer) view of the four-constituent core.
//!
//! Six assertions, adapted from the S3-test acceptance plan to the v0.3
//! compile-time model. Each assertion is a hard gate: together they turn the
//! slogan "blueprint-as-type, verification at compile time, budgetable cost"
//! into checkable facts.
//!
//! | # | Assertion | Where |
//! |---|-----------|-------|
//! | 1 | Unified shape language: chain + diamond + loop + repetition + choice + option compose one complex blueprint | `complex_blueprint_composes_and_drives` |
//! | 2 | Type-level contract (`Conforms`) covers composed units, not just leaves | `type_level_contracts_cover_composites` |
//! | 3 | Non-invasion axiom: every definition is a ZST; a frozen blueprint lives in a `const` | `definitions_are_zero_sized_and_const_livable` |
//! | 4 | Static entry is semantically transparent: generic drive == hand-unrolled steps (bit-exact) | `static_entry_matches_handwritten_bit_exact` |
//! | 5 | Definition↔activation split: defined-but-undriven subgraphs have zero runtime presence (∃-side evolution: see runtime/tests/unified.rs) | `defined_without_activation_has_zero_presence` |
//! | 6 | Determinism (R001): identical input sequences through fresh states yield identical outputs | `determinism_rerun_is_identical` |
//!
//! All comments in this file are in English per project convention for new code.

use axiom::cell_core::{
    Blueprint, Chain, Choice, ChoiceIn, ChoiceOut, Diamond, Feedback, Id, Opt, PortCell,
    Rep, Repeat, Static, Wire, assert_wiring, blueprint_is_zero_sized, drive,
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

struct Double;
impl PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_mul(2)
    }
}

/// Sink of a diamond: sums the two branch outputs.
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

/// Lifts a bare value into the optional domain (user-side adapter cell).
struct LiftSome;
impl PortCell for LiftSome {
    type In = i32;
    type Out = Option<i32>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> Option<i32> {
        Some(x)
    }
}

/// Hand-unrolled equivalents used by assertions 4 and 6.
fn inc(x: i32) -> i32 {
    x.wrapping_add(1)
}
fn dbl(x: i32) -> i32 {
    x.wrapping_mul(2)
}

// ── The blueprint under test ───────────────────────────────────────────────
//
//   input ──► Inc ──┬─► Repeat<2, Inc> ──┐
//                   │                  ├─► SumPair ──► LiftSome ──► Opt<Double> ──► Option<i32>
//                   └─► Double ────────┘
//
// Diamond branch order mirrors Broadcast::fire exactly (clone feeds R1, the
// original feeds R2); all cells are pure, so ordering cannot change results.

type BranchA = Repeat<2, Inc>;
type Dia = Diamond<Inc, BranchA, Double, SumPair>;
type Top = Chain<Dia, Chain<LiftSome, Opt<Double>>>;

fn manual_top(x: i32) -> Option<i32> {
    let src = inc(x); // SRC = Inc
    let b1 = inc(inc(src)); // R1 = Repeat<2, Inc>
    let b2 = dbl(src); // R2 = Double
    Some(dbl(b1.wrapping_add(b2))) // SumPair -> LiftSome -> Opt<Double>
}

// ── Assertion 1: unified shape language composes a complex blueprint ──────

#[test]
fn complex_blueprint_composes_and_drives() {
    // The composite drives and agrees with its closed form f(x) = 2*((x+3)+(2*(x+1))).
    let mut st = <Top as PortCell>::State::default();
    let x = 7;
    // src=8; b1=10; b2=16; sum=26; opt-double=52
    assert_eq!(drive::<Top>(&mut st, x), Some(52));

    // Regular operators participate as first-class cells:
    let mut st_c = <Choice<Inc, Double> as PortCell>::State::default();
    assert!(matches!(
        drive::<Choice<Inc, Double>>(&mut st_c, ChoiceIn::A(5)),
        ChoiceOut::A(6)
    ));
    assert!(matches!(
        drive::<Choice<Inc, Double>>(&mut st_c, ChoiceIn::B(5)),
        ChoiceOut::B(10)
    ));
    assert_eq!(drive::<Opt<Inc>>(&mut (), None), None);

    // Exactly-N repetition (Repeat = Rep alias) drives as expected; the opt-in
    let () = Repeat::<1, Inc>::NONEMPTY;
    let mut st_p = <Repeat<3, Inc> as PortCell>::State::default();
    assert_eq!(drive::<Repeat<3, Inc>>(&mut st_p, 0), 3);
    // NONEMPTY forces const-eval at sites requiring N >= 1.

    // A feedback loop closes over a composite body (chain nests loops):
    type Body = Chain<Inc, Double>;
    let (mut sb, mut sf) = (<Body as PortCell>::State::default(), ());
    // tick(1): body 1->2->4; feed Inc 4->5; body 5->6->12
    assert_eq!(Feedback::<Body, Inc>::tick(&mut sb, &mut sf, 1), 12);
}

// ── Assertion 2: type-level contracts cover composites ────────────────────

#[test]
fn type_level_contracts_cover_composites() {
    use axiom::cell_core::{Conforms, Slot, assert_conforms};

    // Wiring legality (T1) holds across every seam of the composite:
    assert_wiring::<Inc, Dia>();
    assert_wiring::<Dia, LiftSome>();
    assert_wiring::<Chain<Dia, LiftSome>, Opt<Double>>();
    let _: bool = <() as Conforms<Wire<Dia, LiftSome>>>::OK;

    // Slot conformance accepts any inhabitant with the matching dual pair:
    assert_conforms::<Slot<i32, i32>, Inc>();
    assert_conforms::<Slot<Option<i32>, Option<i32>>, Opt<Double>>();
}

// ── Assertion 3: non-invasion — definitions are zero-sized ────────────────

#[test]
fn definitions_are_zero_sized_and_const_livable() {
    assert_eq!(core::mem::size_of::<Top>(), 0);
    assert_eq!(core::mem::size_of::<Blueprint<Top>>(), 0);
    assert!(blueprint_is_zero_sized::<Top>());
    assert_eq!(core::mem::size_of::<Diamond<Inc, BranchA, Double, SumPair>>(), 0);
    assert_eq!(core::mem::size_of::<Id<i32>>(), 0);

    // A frozen blueprint can live in a `const`: definition never touches the heap.
    const FROZEN: Blueprint<Top> = Blueprint::define();
    let _ = &FROZEN;

    // States are the ONLY runtime data (here: none — pure pipeline):
    assert_eq!(core::mem::size_of::<<Top as PortCell>::State>(), 0);
}

// ── Assertion 4: static entry matches handwritten steps bit-exactly ───────

#[test]
fn static_entry_matches_handwritten_bit_exact() {
    let mut st = <Top as PortCell>::State::default();
    for x in -64..=64i32 {
        let got = drive::<Top>(&mut st, x);
        assert_eq!(got, manual_top(x), "mismatch at x = {x}");
    }
}

// ── Assertion 5: defined-but-undriven has zero runtime presence ───────────

#[test]
fn defined_without_activation_has_zero_presence() {
    // A subgraph that is declared, verified, frozen — and never activated —
    // occupies no memory beyond its (zero-sized) definition:
    type Unused = Chain<Rep<9, Inc>, Double>;
    let _declared = Static::<Unused>::declare();
    const FROZEN_UNUSED: Blueprint<Unused> = Blueprint::define();
    let _ = &FROZEN_UNUSED;
    let _: bool = <() as axiom::cell_core::Conforms<Wire<Rep<9, Inc>, Double>>>::OK;

    assert_eq!(core::mem::size_of::<Static<Unused>>(), 0);
    assert_eq!(core::mem::size_of::<Blueprint<Unused>>(), 0);
    // Its state would exist only upon activation; the definition itself is void.
    assert_eq!(core::mem::size_of::<Blueprint<<Unused as PortCell>::State>>(), 0);

    // Runtime-side evolution (∃ install / swap / drive on an interface-fixed
    // position) is exercised in axiom-runtime: runtime/tests/unified.rs.
}

// ── Assertion 6: determinism (R001) ───────────────────────────────────────

#[test]
fn determinism_rerun_is_identical() {
    let inputs: Vec<i32> = (-128..128).collect();

    let run = || {
        let mut st = <Top as PortCell>::State::default();
        inputs.iter().map(|&x| drive::<Top>(&mut st, x)).collect::<Vec<_>>()
    };

    let first = run();
    let second = run();
    assert_eq!(first, second, "same input sequence must give identical output");
    // And both agree with the hand-written reference:
    let expected: Vec<Option<i32>> = inputs.iter().map(|&x| manual_top(x)).collect();
    assert_eq!(first, expected);
}
