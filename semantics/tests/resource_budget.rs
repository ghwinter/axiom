//! C4: topology-level resource budget — the feasible subset.
//!
//! Honest scoping (documents over derivation):
//! - **Thread count is countable**: `spawned_flow` instantiates exactly one
//!   thread per flow; assembly makes it explicit.
//! - **Allocation is summable**: the chain's per-message cost class = the max
//!   over segments (the `CarrierCost` order dominates; `validate_cost` already
//!   enforces per-seam budgets).
//! - **Stack depth is generally undecidable**: compile-time stack-depth
//!   derivation is NOT promised (honest boundary; no fake derivation).
//!
//! This module pins the mechanical subset; the scoping statement lives in
//! runtime.md §8.

use axiom_semantics::prelude_all::CarrierCost;

#[test]
fn chain_cost_is_dominated_by_most_expensive_segment() {
    // Allocation algebra: chain per-message cost = max over segments
    // (the most expensive class dominates; CarrierCost order).
    let inline = CarrierCost::ZeroAllocInline;
    let per_msg = CarrierCost::PerMessageAlloc;
    let external = CarrierCost::External;
    assert!(inline < per_msg && per_msg < external, "声明序");
    // Inline → Queue chain: whole-chain per-message = PerMessageAlloc.
    assert_eq!(per_msg, inline.max(per_msg));
    // Queue → External chain: External dominates.
    assert_eq!(external, per_msg.max(external));
}

#[test]
fn zero_alloc_chain_stays_zero_alloc() {
    // All-zero-alloc segments: chain class stays ZeroAllocInline (declared
    // budget keeps holding at every seam — this is what KernelProfile gates).
    let inline = CarrierCost::ZeroAllocInline;
    assert_eq!(inline, inline.max(inline));
    // A non-zero segment flips the class (dominance).
    assert_ne!(inline.max(CarrierCost::PerMessageAlloc), inline);
}

#[test]
fn thread_count_policy_is_documented_per_spawned_flow() {
    // Thread-count budget is a documentary/assembly claim: one explicit
    // thread per spawned flow (acquisition is countable at assembly time —
    // activation obligation, C15-T1). The mechanical subset here covers the
    // allocation algebra above; thread counting is deployment arithmetic.
    let _ = <axiom::cell_core::Id<i32> as axiom::cell_core::PortCell>::step as fn(
        &mut <axiom::cell_core::Id<i32> as axiom::cell_core::PortCell>::State,
        i32,
    ) -> i32; // symbol-level witness that cell_core resolves in this context
}