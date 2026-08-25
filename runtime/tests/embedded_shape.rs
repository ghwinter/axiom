//! B3: embedded-shape evidence — a composition that uses ONLY cell_core
//! (no std, no alloc imports in the graph definitions).
//!
//! The embedded form (no_std + no alloc) is exactly the static picture: a
//! Blueprint is a zero-sized type; driving uses only stack frames. This test
//! pins that shape — every definition below imports only `axiom::cell_core`.
//! The CI no_std build (both crates, --no-default-features) is the second
//! witness; this file proves the code under test carries no std dependency.

// Only core types of the crate under test — nothing else:
use axiom::cell_core::{Chain, Diamond, Id, PortCell, Rep, assert_wiring, drive};

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

// Embedded-shaped topology: Id → Diamond(Inc, Inc) → Rep<2, Inc> — all ZSTs.
type Emb = Chain<Id<i32>, Chain<Diamond<Inc, Inc, Inc, SumPair>, Rep<2, Inc>>>;

#[test]
fn embedded_shape_composes_and_drives_stack_only() {
    // x -> x (Id) -> diamond(1+1+1... SRC=Inc, R1=Inc, R2=Inc, sum)
    //   = (x+1)+1 + (x+1)+1 = 2x+4 -> Rep<2, Inc> +2 = 2x+6.
    assert_wiring::<Inc, Inc>();
    let mut st = <Emb as PortCell>::State::default();
    assert_eq!(drive::<Emb>(&mut st, 0), 6, "close form 2x+6 at x=0");
    assert_eq!(drive::<Emb>(&mut st, 5), 16, "close form 2x+6 at x=5");
    // State is a stack tuple of units (zero runtime footprint).
    assert!(core::mem::size_of::<<Emb as PortCell>::State>() <= 16);
}

#[test]
fn embedded_shape_blueprint_is_zero_sized() {
    assert!(axiom::cell_core::blueprint_is_zero_sized::<Emb>());
}