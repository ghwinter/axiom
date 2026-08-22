//! Deploy-time & compile-time seam contracts — with **explicit proof modality**.
//!
//! Every check in this module states honestly *how* it is guaranteed, following
//! the project's four-modality discipline (see `axiom-conventions.md` §13):
//!
//! | Check | Modality | Guarantee source |
//! |---|---|---|
//! | [`Moore`] + [`declare_inline_loop_moore`] | **④ declaration** | deployer axiom: the feed cell's output is claimed state-only. NOT a proof — semantic properties are Rice-undecidable; nothing here can verify the claim |
//! | [`validate_cost`] / [`validate_seam`] | **③ deployment validation** | cost budgets are deployment decisions; checked when a seam is assembled |
//! | [`assert_capacity_nonzero`] | **② compile-time witness** | capacity is a const parameter; sites may force rejection of `CAP = 0` at compile time |
//! | [`validate_capacity`] | **③ deployment validation** | runtime aggregate form of the same fact for assembled seams |
//!
//! The one thing this layer never does is dress ③/④ up as compile-time proofs.
//! A declaration that looks verified is worse than an honest gap.

use crate::carrier::{Carrier, CarrierCost};
use axiom::cell_core::PortCell;

// ── 1. Moore marker (inline feedback loops) — modality ④: declaration ─────

/// Marker: **declaration** that this cell's output depends only on its `State`,
/// never on the same-tick input.
///
/// This is modality ④ (deployer axiom), not a proof. Whether `step`'s output
/// truly ignores its input is a semantic property of arbitrary code and is
/// undecidable in general (Rice) — the compiler cannot check it, and this trait
/// does not pretend to. What the type system *does* enforce: only cells whose
/// author made this declaration can be wired into an unbuffered inline loop via
/// [`declare_inline_loop_moore`] — accidental misuse is rejected; false
/// declarations are the declarer's responsibility, by construction of the
/// trust model.
pub trait Moore {}

/// Record the deployer declaration that `FEED` is Moore, making it legal to run
/// `BODY -> FEED -> BODY` over an unbuffered (inline) carrier.
///
/// Compile time enforcement covers the *declaration*, not the *truth*: wiring a
/// cell without `impl Moore` fails to compile; a wrong declaration compiles and
/// is owned by its author. For buffered loops (`BoundedCarrier`, queues) no
/// declaration is needed — buffering supplies the delay (T3).
pub fn declare_inline_loop_moore<BODY, FEED>()
where
    BODY: PortCell,
    FEED: PortCell<In = BODY::Out, Out = BODY::In> + Moore,
{
}

// ── 2/3. Seam validation — modalities ② and ③ ─────────────────────────────

/// Failure of a deploy-time seam contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractError {
    /// The carrier's declared cost exceeds the seam's budget.
    CostExceeded {
        /// Cost declared by the carrier (`Carrier::cost`).
        declared: CarrierCost,
        /// Maximum cost the seam accepts.
        budget: CarrierCost,
    },
    /// A bounded seam was configured with capacity 0 (rendezvous, not backpressure).
    ZeroCapacity,
}

impl core::fmt::Display for ContractError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ContractError::CostExceeded { declared, budget } => {
                write!(f, "carrier cost {declared:?} exceeds seam budget {budget:?}")
            }
            ContractError::ZeroCapacity => {
                write!(f, "bounded seam requires capacity >= 1; 0 means rendezvous, not backpressure")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ContractError {}

/// Compile-time witness (**modality ②**): reject `CAP = 0` where the seam is named.
///
/// Use at assembly points that must not compile with a degenerate rendezvous
/// channel:
///
/// ```ignore
/// const _: () = assert_capacity_nonzero::<4>();
/// ```
///
/// `CAP = 0` is fully decidable at compile time, so sites that want the earliest
/// possible rejection should prefer this over [`validate_capacity`].
pub const fn assert_capacity_nonzero<const CAP: usize>() {
    assert!(
        CAP > 0,
        "bounded seam requires capacity >= 1; 0 means rendezvous, not backpressure"
    );
}

/// Cost conformance (**modality ③**): the carrier must declare a cost within
/// `budget`.
///
/// Ordering follows the [`CarrierCost`] declaration order:
/// `ZeroAllocInline < PerMessageAlloc < External`. This is a deployment
/// decision (budgets are chosen per seam), hence runtime-checked at assembly;
/// pair it with [`assert_capacity_nonzero`] where a static witness is wanted.
pub fn validate_cost<A, B, C>(budget: CarrierCost) -> Result<(), ContractError>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    let declared = C::cost();
    if declared <= budget {
        Ok(())
    } else {
        Err(ContractError::CostExceeded { declared, budget })
    }
}

/// Backpressure readiness (**modality ③**): a bounded seam requires capacity ≥ 1.
///
/// Capacity 0 would still typecheck (rendezvous channels are legal), but it
/// provides neither buffering nor backpressure headroom. Runtime aggregate form;
/// see [`assert_capacity_nonzero`] for the compile-time witness.
pub fn validate_capacity<const CAP: usize>() -> Result<(), ContractError> {
    if CAP == 0 {
        Err(ContractError::ZeroCapacity)
    } else {
        Ok(())
    }
}

/// Combined seam check (**modality ③**): cost budget **and** capacity in one call.
///
/// One entry point a deployment driver runs before wiring a bounded seam.
pub fn validate_seam<A, B, C, const CAP: usize>(
    budget: CarrierCost,
) -> Result<(), ContractError>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    validate_cost::<A, B, C>(budget)?;
    validate_capacity::<CAP>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::carrier::{InlineCarrier, QueueCarrier};

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

    struct Double;
    impl PortCell for Double {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x * 2
        }
    }

    struct Pass;
    impl PortCell for Pass {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x
        }
    }

    // Declared (not proven) as a state-only relay: modality ④ in action.
    struct IdentityFeed;
    impl PortCell for IdentityFeed {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x
        }
    }
    impl Moore for IdentityFeed {}

    #[test]
    fn moore_declaration_gates_inline_loops() {
        // The loop compiles because IdentityFeed carries the deployer declaration.
        declare_inline_loop_moore::<Pass, IdentityFeed>();
    }

    #[test]
    fn cost_conformance_accepts_and_rejects() {
        // Inline within a zero-alloc budget: OK.
        assert_eq!(
            validate_cost::<Inc, Double, InlineCarrier>(CarrierCost::ZeroAllocInline),
            Ok(())
        );
        // Queue carrier against a zero-alloc budget: rejected with both values reported.
        assert_eq!(
            validate_cost::<Inc, Double, QueueCarrier>(CarrierCost::ZeroAllocInline),
            Err(ContractError::CostExceeded {
                declared: CarrierCost::PerMessageAlloc,
                budget: CarrierCost::ZeroAllocInline
            })
        );
        // Queue carrier under a per-message budget: acceptable.
        assert_eq!(
            validate_cost::<Inc, Double, QueueCarrier>(CarrierCost::PerMessageAlloc),
            Ok(())
        );
    }

    #[test]
    fn capacity_zero_is_rejected_both_ways() {
        // Deployment-time aggregate check:
        assert_eq!(validate_capacity::<0>(), Err(ContractError::ZeroCapacity));
        assert_eq!(validate_capacity::<1>(), Ok(()));
        // Compile-time witness: const-evaluated at monomorphization.
        const _: () = assert_capacity_nonzero::<64>();
    }

    #[test]
    fn combined_seam_check_runs_both_contracts() {
        assert_eq!(
            validate_seam::<Inc, Double, InlineCarrier, 4>(CarrierCost::ZeroAllocInline),
            Ok(())
        );
        assert_eq!(
            validate_seam::<Inc, Double, QueueCarrier, 0>(CarrierCost::PerMessageAlloc),
            Err(ContractError::ZeroCapacity)
        );
    }
}
