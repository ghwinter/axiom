//! Streaming contract — pull-model output for `Machine`s (A3).
//!
//! `Machine::process` is **push-shaped**: one input in, a fixed batch of
//! outputs out (`ProcessOutput`). For large results (a million-row `SELECT`,
//! CSV export, log streams) this forces full materialisation before
//! `process()` returns. `StreamingMachine` adds the complementary
//! **pull-shaped** contract: one input, a *lazy* iterator of outputs that
//! borrows the state, which the runtime drains at its own pace (e.g. one
//! batch per N items).
//!
//! # Design
//!
//! ```text
//! Machine::process        : S × I → ProcessOutput<O>      (push, eager)
//! StreamingMachine::process_stream : S × I → Result<impl Iterator<O>, E>
//!                                                        (pull, lazy)
//! ```
//!
//! - The returned iterator borrows `&'a mut State` — the machine's state
//!   *is* the cursor. While the stream is being drained, no other
//!   `process`/`process_stream` call can happen (Rust's `&mut` borrowing
//!   enforces this; per-instance serialisation is identical to `Machine`).
//! - All fallible checks (schema lookup, projection validation, arity) must
//!   happen **before** the iterator is returned: `Result<_, Self::StreamError>`.
//!   The iterator itself is infallible — stream-time errors would otherwise
//!   force a `Result<O>` item type and complicate every consumer.
//! - One input, zero or many outputs: the count is runtime-lazy, so a
//!   streaming machine is **not** `FusedInline` (fixed-output-count is the
//!   fused pipeline's static requirement). The two are orthogonal: a
//!   machine implements either, never both.
//!
//! # Why this exists now
//!
//! This contract is a **runtime prerequisite**: the `DeploySpec` → runtime
//! materialisation and the fused-pipeline consumer both need to know whether
//! a machine is push-only or can be drained lazily. Defining the signature
//! here — before any runtime adapter is built — prevents adapters from
//! being written against the push-only shape and then reworked.
//!
//! # Example
//!
//! ```ignore
//! use axiom::stream::StreamingMachine;
//!
//! impl StreamingMachine for RowSource {
//!     type StreamError = String;
//!     fn process_stream<'a>(
//!         state: &'a mut i64,
//!         _ctx: &MachineContext,
//!         input: In<i64>,
//!     ) -> Result<impl Iterator<Item = Out<i64>> + 'a, String> {
//!         let In(n) = input;
//!         if n < 0 {
//!             return Err("negative row count".into());
//!         }
//!         // State is the cursor; the FIRST next() resets it (strict
//!         // laziness: construction has no side effects), each next()
//!         // advances it.
//!         let mut started = false;
//!         Ok(std::iter::from_fn(move || {
//!             if !started {
//!                 *state = 0;
//!                 started = true;
//!             }
//!             if *state < n {
//!                 let v = *state;
//!                 *state += 1;
//!                 Some(Out(v))
//!             } else {
//!                 None
//!             }
//!         }))
//!     }
//! }
//! ```

use crate::machine::Machine;
use crate::port::MachineContext;

/// Streaming contract for a `Machine` whose output can be drained lazily.
///
/// # Contract
///
/// 1. `process_stream` returns a **lazy** iterator borrowing `&'a mut State`:
///    nothing is produced (and no side effect happens) until the iterator is
///    drained; the state is the cursor and advances per `next()`.
/// 2. All fallible work happens before the iterator is returned; the
///    iterator itself is infallible (`Iterator<Item = Self::Output>`).
/// 3. While the iterator is alive, the machine must not be driven by
///    `Machine::process` (the `&mut` borrow makes this a compile-time
///    impossibility for the same instance).
/// 4. A streaming machine does **not** implement [`crate::machine::FusedInline`]:
///    its output count is runtime-lazy, which the fused pipeline's
///    fixed-output-count requirement cannot accommodate.
///
/// The runtime adapter may drain the iterator in batches (e.g. every
/// `PAGE_SIZE` items) to bound memory while preserving lazy semantics.
pub trait StreamingMachine: Machine {
    /// Error type for the fallible **construction** of the stream.
    /// (`Display` so adapters can surface it on the machine's error path.)
    type StreamError: core::fmt::Display;

    /// Process one input lazily, producing zero or more outputs.
    ///
    /// Returns an iterator borrowing `state`; `Err` is returned only for
    /// errors detectable before streaming begins.
    fn process_stream<'a>(
        state: &'a mut Self::State,
        ctx: &MachineContext,
        input: Self::Input,
    ) -> Result<impl Iterator<Item = Self::Output> + 'a, Self::StreamError>;
}
