//! Deploy-time & compile-time seam contracts — with **explicit proof modality**.
//!
//! Every check in this module states honestly *how* it is guaranteed, following
//! the project's four-modality discipline (② compile-time witness / ③ deployment
//! validation / ④ declaration):
//!
//! | Check | Modality | Guarantee source |
//! |---|---|---|
//! | [`Moore`] + [`declare_inline_loop_moore`] | **④ declaration** | deployer axiom: the feed cell's output is claimed state-only. NOT a proof — semantic properties are Rice-undecidable; nothing here can verify the claim |
//! | [`validate_cost`] / [`validate_seam`] | **③ deployment validation** | cost budgets are deployment decisions; wired into the assembly entries [`assemble_link`](crate::drive::flow::assemble_link) / [`assemble_seam`](crate::drive::flow::assemble_seam) — deployment-time, once, before the zero-cost drive path; rejection = assembly failure |
//! | [`assert_capacity_nonzero`] | **② compile-time witness** | capacity is a const parameter; sites may force rejection of `CAP = 0` at compile time |
//! | [`validate_capacity`] | **③ deployment validation** | runtime aggregate form of the same fact for assembled seams |
//! | [`validate_saturation`] | **③ deployment validation** | the carrier's saturation policy must meet the profile's saturation floor (A1; wired into the profile assembly gate) |
//!
//! The one thing this layer never does is dress ③/④ up as compile-time proofs.
//! A declaration that looks verified is worse than an honest gap.

use crate::movers::carrier::{Carrier, CarrierCost, SaturationPolicy};
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

/// Marker: **declaration** (modality ④) that this cell's `step` does not panic.
///
/// "cell 内禁 panic"是**声明纪律，非类型证明**——panic 语义不可判定（A5 诚实）：
/// 码 `NoPanic` 或误声明都为作者责任。机械落点（已存在，不重造）：
/// [`drive_catch`](crate::drive::flow::drive_catch) 是失败边界载体——未声明
/// `NoPanic` 的 cell 可能 panic，须经它（`catch_unwind` 截为值）跨信任边界驱动，
/// 不得直接落零成本快速路径（[`drive_link`](crate::drive::flow::drive_link)）。
/// 同 [`Moore`]：声明被类型承接，其真理性由声明者背书。
pub trait NoPanic {}

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
    /// The carrier's obligation class falls short of the profile's obligation
    /// minimum on some axis (modality ③; C10 step 2).
    ObligationUnderMet {
        /// The violated axis ("resource" or "delivery").
        axis: &'static str,
        /// Obligation declared by the carrier (`Carrier::obligation`).
        declared: crate::checks::obligation::ObligationClass,
        /// Minimum required by the profile (`Profile::obligation_min`).
        minimum: crate::checks::obligation::ObligationClass,
    },
    /// The carrier's saturation policy is weaker than the profile's saturation
    /// floor (modality ③; A1). A carrier that drops (with receipt) under-mets a
    /// `Block` floor — e.g. a Service delivery seam must not silently shed load.
    SaturationUnderMet {
        /// Saturation declared by the carrier (`Carrier::saturation`).
        declared: SaturationPolicy,
        /// Floor required by the profile (`Profile::saturation_floor`).
        floor: SaturationPolicy,
    },
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
            ContractError::ObligationUnderMet {
                axis,
                declared,
                minimum,
            } => write!(
                f,
                "carrier obligation {declared:?} under-mets profile minimum {minimum:?} on axis {axis}"
            ),
            ContractError::SaturationUnderMet { declared, floor } => write!(
                f,
                "carrier saturation {declared:?} under-mets profile floor {floor:?}"
            ),
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
/// ```
/// const _: () = axiom_semantics::checks::contract::assert_capacity_nonzero::<4>();
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

/// Obligation-minimum conformance (**modality ③**; C10 step 2): the carrier's
/// declared obligation class must be no weaker than the profile's obligation
/// minimum, axis by axis (resource declared ≤ minimum; delivery declared
/// ≥ minimum per the `DeliveryKind` strength order `NotApplicable <
/// MechanizedFullClosed`; reference/lifecycle axes not yet judged — they are
/// kept as declarations without fabrication).
///
/// This turns `Profile::obligation_min` from a placeholder into an enforceable
/// assembly gate: the same topology assembled under a different profile changes
/// not only the cost budget but the obligation floor (T6, obligations included).
pub fn validate_obligation_min(
    declared: crate::checks::obligation::ObligationClass,
    minimum: crate::checks::obligation::ObligationClass,
) -> Result<(), ContractError> {
    match declared.meets_min(&minimum) {
        Ok(()) => Ok(()),
        Err(axis) => Err(ContractError::ObligationUnderMet {
            axis,
            declared,
            minimum,
        }),
    }
}

/// Saturation-conformance (**modality ③**; A1): the carrier's saturation policy
/// must be no weaker than the profile's saturation floor.
///
/// The compatibility is [`SaturationPolicy::meets_saturation_floor`]'s partial
/// order: a carrier that drops (with receipt) under-mets a `Block` floor. Wired
/// into the profile assembly gate alongside cost and obligation minimums, since
/// the saturation floor is a property of the deployment profile, not of any one
/// carrier. Does not disturb the cost/capacity checks in [`validate_seam`].
pub fn validate_saturation<A, B, C>(
    floor: SaturationPolicy,
) -> Result<(), ContractError>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    let declared = C::saturation();
    if declared.meets_saturation_floor(floor) {
        Ok(())
    } else {
        Err(ContractError::SaturationUnderMet { declared, floor })
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::movers::carrier::{InlineCarrier, QueueCarrier};

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
    fn inline_loop_drive_requires_moore_declaration() {
        // 门禁落位（S7）：runtime 驱动器 drive_feedback_inline 要求 FEED: Moore。
        let (mut sb, mut sf) = ((), ());
        let out = crate::drive::flow::drive_feedback_inline::<Pass, IdentityFeed>(&mut sb, &mut sf, 5);
        // Pass(5)=5 -> feed Identity(5)=5 -> Pass(5)=5
        assert_eq!(out, 5);
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
    fn saturation_floor_gates_dropping_carriers() {
        use crate::movers::carrier::SaturationPolicy as S;
        // 直通（N/A）对无饱和义务下限：通过；对 Block 下限：拒绝（无背压点）。
        assert_eq!(
            validate_saturation::<Inc, Double, InlineCarrier>(S::NotApplicable),
            Ok(())
        );
        assert!(matches!(
            validate_saturation::<Inc, Double, InlineCarrier>(S::Block),
            Err(ContractError::SaturationUnderMet { declared: S::NotApplicable, floor: S::Block })
        ));
        // 队列（Block，保守默认）满足 Block 下限。
        assert_eq!(
            validate_saturation::<Inc, Double, QueueCarrier>(S::Block),
            Ok(())
        );
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
