//! Backpressure policies — pluggable flow control for link sends.
//!
//! When a machine produces an output faster than its downstream can consume,
//! the link's channel fills up. **Backpressure** is the decision of what to
//! do at that point. axiom defines four algebraic policies, exposed as a
//! trait so runtimes and users can plug in custom behaviour (notably
//! credit-based flow control, which needs state an enum cannot hold).
//!
//! # Algebra vs physics (the axiom layering)
//!
//! This module defines the **algebra** — the four policies and their
//! contract. It does NOT know about `Wire`, channels, or `mpsc`. A runtime
//! adapter (thread, scheduler, tokio) supplies the **physics**: it reads the
//! channel's fill level into a [`BackpressureCtx`], calls
//! [`BackpressurePolicy::decide`] to get an [`BackpressureAction`], and
//! executes the action on its concrete channel type. This keeps core
//! dependency-free and lets the same policy trait govern sends on any
//! physical carrier.
//!
//! # Relationship to [`crate::link::WritePolicy`]
//!
//! [`WritePolicy`](crate::link::WritePolicy) is the **declarative** form:
//! a simple enum (`Blocking` / `Dropping` / `Overwriting`) chosen per-link
//! in the `DeploySpec`. It covers the three stateless policies.
//! [`BackpressurePolicy`] is the **extensible** form: a trait with state,
//! needed for [`CreditPolicy`] (credit-based flow control). A runtime may
//! accept either; [`WritePolicy`] converts to the stateless trait impls via
//! [`WritePolicy::into_policy`].
//!
//! # The four policies
//!
//! | Policy | When full | Stateful | Algebra |
//! |--------|-----------|----------|---------|
//! | [`BlockPolicy`] | Block sender until space | No | Natural backpressure, no loss |
//! | [`DropPolicy`] | Drop the new value | No | Lossy, fire-and-forget |
//! | [`OverwritePolicy`] | Evict oldest, send new | No | Ring semantics, latest-first |
//! | [`CreditPolicy`] | Defer (no credit) | Yes | Credit-based window, lossless, bounded latency |
//!
//! ## Credit-based flow control (the new addition)
//!
//! [`CreditPolicy`] implements classic credit-based flow control (à la TCP
//! receiver window): the sender starts with N credits (the "window"). Each
//! send consumes one credit. When credits hit zero, the policy returns
//! [`BackpressureAction::Defer`] — the runtime yields the machine and
//! retries later, rather than blocking or dropping. Each time the
//! downstream consumes a value, [`BackpressurePolicy::on_consumed`]
//! replenishes one credit. This gives:
//!
//! - **Lossless** delivery (no drops, no overwrites).
//! - **Bounded memory** (at most `window` items in flight).
//! - **No blocking** (the sender defers instead of blocking a worker).
//!
//! This is the ideal policy for scheduler runtimes where blocking a worker
//! thread would stall other machines. `BlockPolicy` blocks the worker;
//! `CreditPolicy` yields it.
//!
//! # How a runtime integrates
//!
//! A runtime's send path consults the policy before each send:
//!
//! ```ignore
//! let ctx = BackpressureCtx { fill, capacity, closed, credits: policy.credits() };
//! match policy.decide(ctx) {
//!     BackpressureAction::Proceed   => { channel.try_send(wire)?; policy.on_sent(); }
//!     BackpressureAction::Block     => { channel.blocking_send(wire)?; policy.on_sent(); }
//!     BackpressureAction::Drop      => { /* drop wire */ }
//!     BackpressureAction::Overwrite => { channel.evict_oldest_and_send(wire); policy.on_sent(); }
//!     BackpressureAction::Defer     => { return Defer; /* retry later */ }
//! }
//! ```
//!
//! When the downstream consumes a value, the runtime calls
//! `policy.on_consumed()` to replenish credits (no-op for stateless
//! policies).

use crate::link::WritePolicy;
use core::sync::atomic::{AtomicU64, Ordering};

// ════════════════════════════════════════════════════════════════════════════
// BackpressureCtx — the channel state snapshot
// ════════════════════════════════════════════════════════════════════════════

/// A snapshot of the downstream channel's state, passed to
/// [`BackpressurePolicy::decide`].
///
/// The runtime fills this struct from its concrete channel (fill level,
/// capacity, closed flag) and, for credit-based policies, the current
/// credit balance. The policy reads it and returns an action without
/// touching the channel — the separation lets the same trait govern any
/// physical carrier.
#[derive(Debug, Clone, Copy)]
pub struct BackpressureCtx {
    /// Items currently queued in the channel.
    pub fill: usize,
    /// Maximum items the channel holds. `0` means "single-slot" (e.g.
    /// `Latest`) or "unbounded" — the runtime distinguishes; a single-slot
    /// channel reports `fill = 0 or 1` with `capacity = 1`.
    pub capacity: usize,
    /// Whether the channel is closed (receiver gone). If `true`, the
    /// runtime reports `Closed` regardless of the policy's decision.
    pub closed: bool,
    /// Available credits for credit-based policies. `u64::MAX` means
    /// "unlimited" (the policy is not credit-based). Stateful policies
    /// read this to decide between `Proceed` and `Defer`.
    pub credits: u64,
}

impl BackpressureCtx {
    /// Whether the channel is at capacity (would block / need a policy
    /// decision). A channel with `capacity == 0` (unbounded) is never full.
    pub fn is_full(&self) -> bool {
        self.capacity > 0 && self.fill >= self.capacity
    }
}

// ════════════════════════════════════════════════════════════════════════════
// BackpressureAction — what the runtime should do
// ════════════════════════════════════════════════════════════════════════════

/// The action a runtime should take for a send, as decided by a
/// [`BackpressurePolicy`].
///
/// The runtime executes the action on its concrete channel; the policy does
/// not touch the channel directly. This keeps the policy carrier-independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressureAction {
    /// Send normally — there is space in the channel. The runtime does a
    /// non-blocking `try_send` and expects success.
    Proceed,
    /// Block the sender until space is available, then send. The runtime
    /// does a blocking `send`. In a worker-pool runtime this stalls the
    /// worker — prefer [`Defer`](Self::Defer) for scalable backpressure.
    Block,
    /// Drop the new value — do not send. The value is lost. Used by
    /// [`DropPolicy`] for fire-and-forget links.
    Drop,
    /// Evict the oldest queued item and send the new one. The runtime
    /// drains one item then sends. Used by [`OverwritePolicy`] for ring
    /// semantics (latest values prioritised).
    Overwrite,
    /// Defer the send — do not send now, yield the machine, and retry
    /// later. Used by [`CreditPolicy`] when credits are exhausted. The
    /// runtime should re-schedule the machine (e.g. re-push to the ready
    /// queue after a short delay, or when `on_consumed` fires).
    Defer,
}

// ════════════════════════════════════════════════════════════════════════════
// BackpressurePolicy trait
// ════════════════════════════════════════════════════════════════════════════

/// Pluggable backpressure policy — decides what to do when a downstream
/// channel is full (or, for credit-based policies, when credits are
/// exhausted).
///
/// See the [module docs](self) for the algebra/physics split and the four
/// in-tree policies. The trait is `Send + Sync` because policies are
/// shared between worker threads (a single policy instance governs all
/// sends on a link).
///
/// # Lifecycle
///
/// For each send attempt, the runtime:
/// 1. Builds a [`BackpressureCtx`] from the channel's current state.
/// 2. Calls [`decide`](Self::decide) → gets an [`BackpressureAction`].
/// 3. Executes the action on the concrete channel.
/// 4. If the send succeeded, calls [`on_sent`](Self::on_sent) (stateful
///    policies decrement credits here).
///
/// When the downstream consumes a value, the runtime calls
/// [`on_consumed`](Self::on_consumed) (stateful policies replenish credits
/// here). For stateless policies, both `on_sent` and `on_consumed` are
/// no-ops (default impls).
///
/// # Implementing a custom policy
///
/// A custom policy might combine credit + priority: defer low-priority
/// sends when credits are low, but always proceed for high-priority
/// control messages. Implement `BackpressurePolicy` and store the policy
/// in a `Box<dyn BackpressurePolicy>` keyed by link in the runtime.
pub trait BackpressurePolicy: Send + Sync {
    /// Decide what to do with a send, given the current channel state.
    ///
    /// The runtime calls this BEFORE attempting the send. If `ctx.closed`
    /// is `true`, the runtime reports `Closed` regardless of the returned
    /// action (a closed channel accepts nothing).
    fn decide(&self, ctx: BackpressureCtx) -> BackpressureAction;

    /// Called by the runtime AFTER a successful send. Stateful policies
    /// update their internal state here (e.g. [`CreditPolicy`] decrements
    /// its credit balance). Default: no-op (stateless policies).
    fn on_sent(&self) {}

    /// Called by the runtime AFTER the downstream consumes a value.
    /// Stateful policies replenish here (e.g. [`CreditPolicy`] increments
    /// its credit balance). Default: no-op (stateless policies).
    fn on_consumed(&self) {}

    /// Current available credits (for credit-based policies). Default
    /// `u64::MAX` means "unlimited" — the policy is not credit-based and
    /// never returns [`Defer`](BackpressureAction::Defer) on credit grounds.
    /// The runtime reads this when building [`BackpressureCtx`] to pass
    /// the live credit balance into `decide`. [`CreditPolicy`] overrides
    /// this to return its atomic counter; stateless policies inherit the
    /// default.
    fn credits(&self) -> u64 { u64::MAX }

    /// Human-readable policy name (diagnostics). Default: `"backpressure"`.
    fn name(&self) -> &'static str { "backpressure" }
}

// ════════════════════════════════════════════════════════════════════════════
// Stateless policies: Block / Drop / Overwrite
// ════════════════════════════════════════════════════════════════════════════

/// Block the sender when the channel is full — natural backpressure, no loss.
///
/// Equivalent to [`WritePolicy::Blocking`]. The sender stalls until the
/// downstream consumes, which propagates backpressure up the topology. In a
/// worker-pool runtime this stalls the worker thread; prefer
/// [`CreditPolicy`] for scalable lossless backpressure.
#[derive(Debug, Default, Clone, Copy)]
pub struct BlockPolicy;

impl BlockPolicy {
    pub fn new() -> Self { Self }
}

impl BackpressurePolicy for BlockPolicy {
    fn decide(&self, ctx: BackpressureCtx) -> BackpressureAction {
        if ctx.is_full() { BackpressureAction::Block } else { BackpressureAction::Proceed }
    }
    fn name(&self) -> &'static str { "block" }
}

/// Drop the new value when the channel is full — lossy, fire-and-forget.
///
/// Equivalent to [`WritePolicy::Dropping`]. The value is silently lost;
/// the sender never blocks. Suitable for telemetry / status feeds where a
/// stale value is worse than a missing one.
#[derive(Debug, Default, Clone, Copy)]
pub struct DropPolicy;

impl DropPolicy {
    pub fn new() -> Self { Self }
}

impl BackpressurePolicy for DropPolicy {
    fn decide(&self, ctx: BackpressureCtx) -> BackpressureAction {
        if ctx.is_full() { BackpressureAction::Drop } else { BackpressureAction::Proceed }
    }
    fn name(&self) -> &'static str { "drop" }
}

/// Evict the oldest queued item when full — ring-buffer semantics.
///
/// Equivalent to [`WritePolicy::Overwriting`]. The newest value is always
/// delivered; the oldest is sacrificed. Suitable for sliding-window
/// aggregators where recency matters more than completeness.
///
/// # Note
///
/// `std::sync::mpsc` has no sender-side drain, so a runtime realising this
/// on `mpsc` falls back to a blocking send when `try_send` returns `Full`.
/// A true ring-overwrite needs a custom channel (e.g. a `VecDeque` with
/// wrap-around indexing). The policy only says "evict"; the channel
/// decides how.
#[derive(Debug, Default, Clone, Copy)]
pub struct OverwritePolicy;

impl OverwritePolicy {
    pub fn new() -> Self { Self }
}

impl BackpressurePolicy for OverwritePolicy {
    fn decide(&self, ctx: BackpressureCtx) -> BackpressureAction {
        if ctx.is_full() { BackpressureAction::Overwrite } else { BackpressureAction::Proceed }
    }
    fn name(&self) -> &'static str { "overwrite" }
}

// ════════════════════════════════════════════════════════════════════════════
// CreditPolicy — stateful, credit-based flow control
// ════════════════════════════════════════════════════════════════════════════

/// Credit-based flow control — lossless, bounded-memory, non-blocking.
///
/// The sender starts with `window` credits. Each send consumes one; when
/// credits reach zero, the policy returns [`BackpressureAction::Defer`] —
/// the runtime yields the machine (re-schedules it) instead of blocking or
/// dropping. Each downstream consumption replenishes one credit via
/// [`on_consumed`](BackpressurePolicy::on_consumed).
///
/// This is the ideal backpressure for worker-pool runtimes: it bounds
/// in-flight memory to `window` items WITHOUT blocking a worker thread
/// (unlike [`BlockPolicy`]) and WITHOUT losing data (unlike [`DropPolicy`] /
/// [`OverwritePolicy`]).
///
/// # The credit invariant
///
/// `credits` ranges over `[0, window]`. `on_sent` decrements (saturating
/// at 0); `on_consumed` increments (saturating at `window`). The atomic
/// operations are `Relaxed` — credit accounting is best-effort; a slight
/// over/under-send is harmless because the runtime re-checks `ctx.credits`
/// on each `decide` call.
///
/// # Construction
///
/// `window` is the maximum in-flight items. A larger window gives higher
/// throughput (more pipeline parallelism) at the cost of more memory. A
/// window of 1 gives strict lockstep (send → consume → send).
pub struct CreditPolicy {
    credits: AtomicU64,
    window: u64,
}

impl CreditPolicy {
    /// Create a credit policy with `window` initial credits.
    pub fn new(window: u64) -> Self {
        Self { credits: AtomicU64::new(window), window }
    }

    /// The configured window (max in-flight items).
    pub fn window(&self) -> u64 { self.window }

    /// Current available credits (diagnostic; may be racy).
    pub fn credits(&self) -> u64 { self.credits.load(Ordering::Relaxed) }
}

impl Default for CreditPolicy {
    fn default() -> Self { Self::new(64) }
}

impl BackpressurePolicy for CreditPolicy {
    fn decide(&self, ctx: BackpressureCtx) -> BackpressureAction {
        // Use the live credit balance from the ctx (the runtime reads it
        // from `self.credits()` when building ctx). If credits are 0,
        // defer — don't block the worker, don't drop the value.
        if ctx.credits == 0 {
            BackpressureAction::Defer
        } else {
            BackpressureAction::Proceed
        }
    }

    fn credits(&self) -> u64 {
        self.credits.load(Ordering::Relaxed)
    }

    fn on_sent(&self) {
        // Decrement credits (saturating at 0). Relaxed is fine: a rare
        // over-decrement just means one extra Defer, self-correcting.
        let _ = self.credits.fetch_update(
            Ordering::Relaxed, Ordering::Relaxed,
            |c| if c > 0 { Some(c - 1) } else { None },
        );
    }

    fn on_consumed(&self) {
        // Replenish one credit (saturating at window). Called by the
        // runtime when the downstream consumes a value.
        let _ = self.credits.fetch_update(
            Ordering::Relaxed, Ordering::Relaxed,
            |c| if c < self.window { Some(c + 1) } else { None },
        );
    }

    fn name(&self) -> &'static str { "credit" }
}

impl core::fmt::Debug for CreditPolicy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CreditPolicy")
            .field("window", &self.window)
            .field("credits", &self.credits.load(Ordering::Relaxed))
            .finish()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// WritePolicy interop
// ════════════════════════════════════════════════════════════════════════════

impl WritePolicy {
    /// Convert a declarative [`WritePolicy`] into a boxed
    /// [`BackpressurePolicy`] trait object. This lets a runtime accept
    /// either form: the declarative enum from a `LinkSpec`, or a custom
    /// stateful policy constructed at deploy time.
    ///
    /// `CreditPolicy` has no `WritePolicy` counterpart (it needs state),
    /// so this conversion covers only the three stateless policies.
    ///
    /// Always available: axiom always links `extern crate alloc`, so
    /// `alloc::boxed::Box` is always defined (under both `std` and
    /// `no_std + alloc` builds). No feature gate needed.
    pub fn into_policy(self) -> alloc::boxed::Box<dyn BackpressurePolicy> {
        match self {
            Self::Blocking => alloc::boxed::Box::new(BlockPolicy),
            Self::Dropping => alloc::boxed::Box::new(DropPolicy),
            Self::Overwriting => alloc::boxed::Box::new(OverwritePolicy),
        }
    }
}

impl From<WritePolicy> for alloc::boxed::Box<dyn BackpressurePolicy> {
    fn from(p: WritePolicy) -> Self { p.into_policy() }
}

// ════════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::WritePolicy;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn not_full() -> BackpressureCtx {
        BackpressureCtx { fill: 1, capacity: 4, closed: false, credits: u64::MAX }
    }

    fn full() -> BackpressureCtx {
        BackpressureCtx { fill: 4, capacity: 4, closed: false, credits: u64::MAX }
    }

    fn unbounded() -> BackpressureCtx {
        BackpressureCtx { fill: 100, capacity: 0, closed: false, credits: u64::MAX }
    }

    // ── BackpressureCtx::is_full ─────────────────────────────────────────────

    #[test]
    fn ctx_is_full_when_fill_ge_capacity() {
        assert!(full().is_full());
    }

    #[test]
    fn ctx_not_full_when_fill_lt_capacity() {
        assert!(!not_full().is_full());
    }

    #[test]
    fn ctx_unbounded_never_full() {
        assert!(!unbounded().is_full());
    }

    // ── BlockPolicy ──────────────────────────────────────────────────────────

    #[test]
    fn block_policy_proceeds_when_not_full() {
        let p = BlockPolicy::new();
        assert_eq!(p.decide(not_full()), BackpressureAction::Proceed);
    }

    #[test]
    fn block_policy_blocks_when_full() {
        let p = BlockPolicy::new();
        assert_eq!(p.decide(full()), BackpressureAction::Block);
    }

    #[test]
    fn block_policy_name() {
        assert_eq!(BlockPolicy::new().name(), "block");
    }

    // ── DropPolicy ───────────────────────────────────────────────────────────

    #[test]
    fn drop_policy_proceeds_when_not_full() {
        let p = DropPolicy::new();
        assert_eq!(p.decide(not_full()), BackpressureAction::Proceed);
    }

    #[test]
    fn drop_policy_drops_when_full() {
        let p = DropPolicy::new();
        assert_eq!(p.decide(full()), BackpressureAction::Drop);
    }

    // ── OverwritePolicy ──────────────────────────────────────────────────────

    #[test]
    fn overwrite_policy_proceeds_when_not_full() {
        let p = OverwritePolicy::new();
        assert_eq!(p.decide(not_full()), BackpressureAction::Proceed);
    }

    #[test]
    fn overwrite_policy_overwrites_when_full() {
        let p = OverwritePolicy::new();
        assert_eq!(p.decide(full()), BackpressureAction::Overwrite);
    }

    // ── CreditPolicy ─────────────────────────────────────────────────────────

    #[test]
    fn credit_policy_initial_credits_equals_window() {
        let p = CreditPolicy::new(10);
        assert_eq!(p.credits(), 10);
        assert_eq!(p.window(), 10);
    }

    #[test]
    fn credit_policy_default_window_64() {
        let p = CreditPolicy::default();
        assert_eq!(p.window(), 64);
        assert_eq!(p.credits(), 64);
    }

    #[test]
    fn credit_policy_proceeds_with_credits() {
        let p = CreditPolicy::new(5);
        let ctx = BackpressureCtx { fill: 0, capacity: 4, closed: false, credits: 5 };
        assert_eq!(p.decide(ctx), BackpressureAction::Proceed);
    }

    #[test]
    fn credit_policy_defers_when_zero_credits() {
        let p = CreditPolicy::new(5);
        let ctx = BackpressureCtx { fill: 0, capacity: 4, closed: false, credits: 0 };
        assert_eq!(p.decide(ctx), BackpressureAction::Defer);
    }

    #[test]
    fn credit_policy_on_sent_decrements() {
        let p = CreditPolicy::new(3);
        p.on_sent();
        p.on_sent();
        assert_eq!(p.credits(), 1);
    }

    #[test]
    fn credit_policy_on_sent_saturates_at_zero() {
        let p = CreditPolicy::new(1);
        p.on_sent();
        p.on_sent();
        p.on_sent();
        assert_eq!(p.credits(), 0);
    }

    #[test]
    fn credit_policy_on_consumed_replenishes() {
        // window=0 is degenerate (on_consumed saturates at window=0, so it
        // can never lift credits off 0). Use window>0, spend a credit, then
        // replenish.
        let p = CreditPolicy::new(2);
        p.on_sent();
        assert_eq!(p.credits(), 1);
        p.on_consumed();
        assert_eq!(p.credits(), 2);
    }

    #[test]
    fn credit_policy_on_consumed_saturates_at_window() {
        let p = CreditPolicy::new(2);
        for _ in 0..5 {
            p.on_consumed();
        }
        assert_eq!(p.credits(), 2);
    }

    #[test]
    fn credit_policy_name() {
        assert_eq!(CreditPolicy::default().name(), "credit");
    }

    #[test]
    fn credit_policy_send_consume_cycle() {
        let p = CreditPolicy::new(2);

        // send → 1
        p.on_sent();
        assert_eq!(p.credits(), 1);

        // send → 0
        p.on_sent();
        assert_eq!(p.credits(), 0);

        // out of credits: defer
        let ctx_empty =
            BackpressureCtx { fill: 0, capacity: 4, closed: false, credits: 0 };
        assert_eq!(p.decide(ctx_empty), BackpressureAction::Defer);

        // downstream consumes → 1 credit
        p.on_consumed();
        assert_eq!(p.credits(), 1);

        // now we can proceed
        let ctx_one =
            BackpressureCtx { fill: 0, capacity: 4, closed: false, credits: 1 };
        assert_eq!(p.decide(ctx_one), BackpressureAction::Proceed);
    }

    // ── WritePolicy::into_policy ─────────────────────────────────────────────

    #[test]
    fn write_policy_blocking_into_block() {
        let p = WritePolicy::Blocking.into_policy();
        assert_eq!(p.decide(full()), BackpressureAction::Block);
    }

    #[test]
    fn write_policy_dropping_into_drop() {
        let p = WritePolicy::Dropping.into_policy();
        assert_eq!(p.decide(full()), BackpressureAction::Drop);
    }

    #[test]
    fn write_policy_overwriting_into_overwrite() {
        let p = WritePolicy::Overwriting.into_policy();
        assert_eq!(p.decide(full()), BackpressureAction::Overwrite);
    }

    // ── stateless policy 默认方法 ───────────────────────────────────────────

    #[test]
    fn stateless_policy_credits_unlimited() {
        assert_eq!(BlockPolicy::new().credits(), u64::MAX);
        assert_eq!(DropPolicy::new().credits(), u64::MAX);
        assert_eq!(OverwritePolicy::new().credits(), u64::MAX);
    }

    #[test]
    fn stateless_policy_on_sent_noop() {
        let p = BlockPolicy::new();
        p.on_sent();
        p.on_consumed();
        // 无 panic 即通过；decide 仍按 is_full 行为不变
        assert_eq!(p.decide(not_full()), BackpressureAction::Proceed);
        assert_eq!(p.decide(full()), BackpressureAction::Block);
    }
}
