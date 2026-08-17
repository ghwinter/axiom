//! **Maturity: experimental** (an extension; advanced as part of the core, not dropped).
//!
//! Rebuildability contract — rebuilding state from an event stream.
//!
//! # Positioning
//!
//! `Projection` is the contractual form of "observable ⟺ rebuildable": any
//! state that implements [`Projection`] can be rebuilt from an event stream
//! without side effects. This is the basis of debugging, auditing, forking,
//! time travel, and deterministic replay — axiom's `Observe` stream guarantees
//! "no reverse influence on the source", and `Projection` further guarantees
//! "rebuildable from the event stream", upgrading observability from a
//! philosophical promise to a type contract.
//!
//! Complementary to [`Entity`](crate::entity::Entity)'s `checkpoint`/`restore`:
//!
//! - `Projection`: **incremental event projection** — `apply(state, event)`
//!   rebuilds step by step;
//! - `checkpoint`: **full snapshot** — `restore` directly recovers a point in
//!   time.
//!
//! # Why it lives in core
//!
//! Rebuildability is a structural-layer contract — it declares "how state is
//! derived from an event stream", independent of execution. The runtime's
//! recording/replay/forking (`ReplayJournal`) all build on this contract.
//!
//! # Replay concept convergence (S2 naming unification)
//!
//! This module and the runtime's `ReplayJournal` / `Replayer` belong to the
//! same **Replay concept**: projection = **contract**, journal = **carrier**.
//!
//! - **Contract** (this module): the [`Projection`] trait + [`Projection::replay`]
//!   (the operation primitive that rebuilds state from an event stream) —
//!   declares "how state can be rebuilt", independent of execution.
//! - **Carrier** (runtime): `ReplayJournal` (records the input event stream) +
//!   `Replayer` (a runtime that rebuilds any point in time from the journal) —
//!   the actual recording/replay mechanism, built on the contract.
//!
//! The relationship: the contract is "what to do" (a pure fold); the carrier
//! is "what to store / how to feed" (record + tick in order). They share the
//! naming family (`Replay`), with responsibilities separated by layer — no
//! more semantic drift between "a floating `replay` function in core" and "a
//! `ReplayJournal` in the runtime".
//!
//! # Pure-function constraint
//!
//! [`Projection::apply`] must be a **pure function**: same state + same event
//! → same new state, with no external side effects. This is the precondition
//! for rebuildability — otherwise replay is non-deterministic.

/// Rebuildability contract: state `S` as a projection of an event stream.
///
/// The implementor declares the event type [`Event`](Projection::Event) and
/// the pure function [`apply`](Projection::apply) — applying one event updates
/// the state without side effects. Given an initial state and a sequence of
/// events, [`Projection::replay`] can rebuild the state at any point.
pub trait Projection<S> {
    /// The event type — an element of the append-only event stream.
    type Event;

    /// Apply one event, updating the state (pure function, no side effects).
    fn apply(state: &mut S, event: &Self::Event);

    /// Rebuild state from an event stream (the **operational primitive of the
    /// Replay contract**).
    ///
    /// Given an initial state and a sequence of events, applies each event in
    /// order and returns the rebuilt state. This is the core primitive of
    /// deterministic replay, forking, and time travel — the same event stream
    /// plus the same initial state always yields the same final state
    /// (`apply` is a pure function).
    ///
    /// Belongs to the same **Replay concept** (S2 naming unification) as the
    /// runtime's `ReplayJournal` (carrier) / `Replayer` (executor): this method
    /// is the contract-side operation, declaring "how state can be rebuilt";
    /// the carrier side is responsible for "what to record, how to feed it
    /// back in order".
    fn replay(initial: S, events: &[Self::Event]) -> S {
        let mut state = initial;
        for event in events {
            Self::apply(&mut state, event);
        }
        state
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Unit tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec;

    /// Counter projection: events are deltas, state is the running sum.
    struct Counter;
    impl Projection<i32> for Counter {
        type Event = i32;
        fn apply(state: &mut i32, event: &i32) {
            *state += event;
        }
    }

    #[test]
    fn replay_folds_events() {
        let s = Counter::replay(0, &[1, 2, 3]);
        assert_eq!(s, 6);
    }

    #[test]
    fn replay_empty_is_initial() {
        let s = Counter::replay(42, &[]);
        assert_eq!(s, 42);
    }

    /// String-concatenation projection: events are string fragments, state is
    /// the accumulated text.
    struct Concat;
    impl Projection<String> for Concat {
        type Event = String;
        fn apply(state: &mut String, event: &String) {
            state.push_str(event);
        }
    }

    #[test]
    fn replay_accumulates_string() {
        let events: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let s = Concat::replay(String::new(), &events);
        assert_eq!(s, "abc");
    }

    #[test]
    fn replay_is_deterministic() {
        // The same event stream plus the same initial state replays to the
        // same result twice (the pure-function precondition).
        let events = [10, -3, 5];
        let a = Counter::replay(0, &events);
        let b = Counter::replay(0, &events);
        assert_eq!(a, b);
    }
}
