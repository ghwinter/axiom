//! **Maturity: stable** (the stable core, main subject of the current refactor).
//!
//! Type contracts for the static execution path — compile-time-known topology,
//! zero cost.
//!
//! # Positioning (homed under the anti-narrowing rule)
//!
//! This module is a **structural-layer + type-layer contract**: it defines the
//! contracts needed by the static execution path — [`StraightMachine`]
//! (single-port raw-payload pass-through) and the [`Chain`]/[`Diamond`]
//! combinators (recursively expressing series-parallel DAGs) — and serves as
//! the **compile-time projection** of the blueprint `Topology`,
//! [`StaticTopology`], as its only implementation surface (via blanket impl).
//!
//! # The single static entry point (S1 unification)
//!
//! The **only entry point** of the static execution path is: the
//! `Chain`/`Diamond` combinators + the `Straight` contract
//! (`StraightMachine`/`StraightLink`/`StraightSplit`/`StraightMerge`).
//! Combinators compose recursively to express series-parallel DAGs and
//! feedback loops; `run_parallel` runs multiple independent streams in
//! parallel. The old enum-port contracts (`Link`/`Split`/`Merge` and the
//! fixed-N functions `pipelineN`/`fanoutN`) have been removed in a breaking
//! refactor — new code always uses the combinators + Straight contract:
//!
//! | Topology | Contract | Combinator |
//! |------|-------|---------------------|
//! | Linear A→B→C | `StraightLink` | `Chain` + `pipeline_chain` |
//! | Arbitrary-depth linear chain | `StraightLink` | `Chain` + `pipeline_chain` |
//! | Fan-out A→(B,C) | `StraightSplit` | `Diamond` (arms may be any chain) |
//! | Fan-in (A,B)→C | `StraightMerge` | `Diamond` |
//! | Diamond A→(B,C)→D | `StraightSplit` + `StraightMerge` | `Diamond` |
//! | Series-parallel DAG | Straight contract recursion | `Chain` + `Diamond` nesting |
//!
//! # Zero cost (P0: eliminating the port-label tax)
//!
//! The static path executes with raw payloads: `process_straight(state, i) -> o`
//! has no port enum, no `match`, no `MachineContext`, no `ProcessOutput`
//! dispatch. Sources/destinations are fixed by the type system at compile
//! time — zero validation at physical execution (a source/destination error is
//! a business-logic error, not a justification for performance overhead).
//! Compare with the dynamic path (`Box<dyn Any>` type erasure, heap
//! allocation + downcast per hop, ~5x).
//!
//! `StraightIn`/`StraightOut` are **pure data payloads** (P3: no
//! `HasPortInfo`/runtime introspection required) — the port labels of
//! single-port machines are stripped from the physical layer, while the
//! abstraction layer (ports/topology/validation/observability) stays in the
//! `Machine` contract.
//!
//! # Expressiveness boundary (series-parallel DAGs, not arbitrary DAGs)
//!
//! `Chain` (serial) and `Diamond` (fork-join) form a recursive algebra whose
//! generated language is exactly the **series-parallel graphs** — serial
//! composition and parallel composition are recursively closed. Any
//! series-parallel topology (pipelines, map-reduce, diamond networks,
//! multi-level fork-join trees) can be expressed as a nesting of the two,
//! fully monomorphized.
//!
//! A true **arbitrary DAG** (with non-series-parallel crossing edges, such as
//! the transitive reduction of K4) cannot be expressed in this algebra: stable
//! Rust cannot describe an "arbitrary edge table" with const generics while
//! keeping port type safety — an edge table `(usize, usize)` is value-level
//! information, while endpoint port types are type-level information; the
//! mapping between them needs GAT / `generic_const_exprs`. This is a boundary
//! of the type system, not an implementation flaw; non-series-parallel
//! topologies go through the dynamic path (`Runtime`), for the same reason as
//! the dynamic tax (mathematically unavoidable).
//!
//! # Safety
//!
//! The static path only accepts `FusedInline` machines (`SingleOutput` or
//! `TupleOutput`). `MultiOutput` (which contains `YieldMulti`, a runtime output
//! count) is rejected at the type level — the static path handles
//! compile-time-known output counts and cannot handle runtime-decided fan-out.

use crate::machine::Machine;
use crate::topology::StaticTopology;

use alloc::string::{String, ToString};

// ════════════════════════════════════════════════════════════════════════════
// Section 4: Error types
// ════════════════════════════════════════════════════════════════════════════

/// An error from the static execution path.
#[derive(Debug)]
pub enum StaticExecError {
    /// A machine's `init()` failed.
    InitFailed { machine: &'static str, reason: String },
    /// A machine's `cleanup()` failed.
    CleanupFailed { machine: &'static str, reason: String },
    /// A machine returned `Done` mid-execution, terminating early.
    ///
    /// This is not an error — but for batch execution that expects to process
    /// every input, `Done` means the machine cannot keep consuming the
    /// remaining inputs. The caller may choose to ignore or handle it.
    MachineDone { machine: &'static str, processed: usize },
}

impl core::fmt::Display for StaticExecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InitFailed { machine, reason } => {
                write!(f, "init failed for '{}': {}", machine, reason)
            }
            Self::CleanupFailed { machine, reason } => {
                write!(f, "cleanup failed for '{}': {}", machine, reason)
            }
            Self::MachineDone { machine, processed } => {
                write!(
                    f,
                    "machine '{}' returned Done after processing {} inputs",
                    machine, processed
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StaticExecError {}

// ════════════════════════════════════════════════════════════════════════════
// Section 4.4: Straight — raw-payload pass-through contract (eliminating the port-label tax)
// ════════════════════════════════════════════════════════════════════════════
//
// The static path's zero-cost fix (P0): the compile-time types already fix
// "where the data comes from and where it goes" — source/destination
// validation is a business-logic error (the developer's responsibility), not
// a physical-execution cost. This contract lets single-port machines pass raw
// payloads directly: no port enum, no match, no label check. Multi-port
// machines and the dynamic path keep enums/introspection (topology is only
// known at runtime there; labels are necessary).

/// The label-free pass-through contract for single-port machines.
///
/// The static path (`Chain`/`Diamond`/`feedback`) requires machines to
/// implement this contract and execute with raw payloads:
/// `process_straight(state, input) -> output` — no enum wrapping/unwrapping,
/// no `MachineContext`, no `ProcessOutput` match. The payload types
/// [`StraightIn`]/[`StraightOut`] are pure data (no `HasPortInfo` required) —
/// the data's destination is fixed by the type system at compile time, with
/// zero validation at runtime.
///
/// Relationship to `Machine`: `Machine`'s port/topology/validation/
/// observability contracts stay at the abstraction layer (outside
/// `process_straight`); `process_straight` is the physical-layer pass-through
/// channel. Multi-port machines (fan-out/multi-input) go through the dynamic
/// path (`Runtime`), where labels are necessary.
pub trait StraightMachine: Machine {
    /// The payload type of the single input port (de-labeled, pure data).
    type StraightIn: Send + 'static;
    /// The payload type of the single output port (de-labeled, pure data).
    type StraightOut: Send + 'static;

    /// Raw-payload process: no enum wrapping/unwrapping, no ctx, no label
    /// check.
    ///
    /// Implementations MUST be `#[inline]` — this is the precondition for
    /// cross-crate fusion (`StaticChain` monomorphization).
    fn process_straight(state: &mut Self::State, input: Self::StraightIn) -> Self::StraightOut;
}

/// Raw-payload link: `fn(StraightOut) -> StraightIn`.
///
/// The compile-time types already fix "S's output must go to D's input" — no
/// enum match, no `Option` check (the old `Link::extract -> Option` was
/// removed in a breaking refactor).
pub trait StraightLink<S: StraightMachine, D: StraightMachine> {
    /// Convert `S::StraightOut` into `D::StraightIn`.
    fn convert(out: S::StraightOut) -> D::StraightIn;
}

/// Identity raw link — used when `S::StraightOut: Into<D::StraightIn>`
/// (usually when the two machines have the same payload type).
pub struct StraightId;

impl<S: StraightMachine, D: StraightMachine> StraightLink<S, D> for StraightId
where
    S::StraightOut: Into<D::StraightIn>,
{
    #[inline]
    fn convert(out: S::StraightOut) -> D::StraightIn {
        out.into()
    }
}

/// Raw-payload split: `fn(T) -> (Left, Right)`.
///
/// No enum labels — routing/copying by content is business logic (dispatch),
/// not validation.
pub trait StraightSplit<T> {
    /// The left payload type (sent to the first downstream).
    type Left;
    /// The right payload type (sent to the second downstream).
    type Right;

    /// Split `input` into `(Left, Right)`.
    fn split(input: T) -> (Self::Left, Self::Right);
}

/// Copy split (Tee semantics): duplicates the same payload into two copies.
pub struct StraightClone;

impl<T: Clone> StraightSplit<T> for StraightClone {
    type Left = T;
    type Right = T;

    #[inline]
    fn split(input: T) -> (T, T) {
        (input.clone(), input.clone())
    }
}

/// Raw-payload merge: `fn(A, B) -> Output`.
pub trait StraightMerge<A, B> {
    /// The merged payload type.
    type Output;

    /// Merge `a` and `b` into a single payload.
    fn merge(a: A, b: B) -> Self::Output;
}

// ════════════════════════════════════════════════════════════════════════════
// Section 4.5: Chain — compile-time linear chain of arbitrary depth
// ════════════════════════════════════════════════════════════════════════════

use crate::port::MachineContext;
use alloc::vec::Vec;
use core::marker::PhantomData;

/// Compile-time linear-chain combinator: `Chain<A, B>` expresses `A → B` and
/// can be nested to arbitrary depth.
///
/// `Chain<A, Chain<B, C>>` is a 3-stage pipeline. The chain's depth is decided
/// by the type nesting and is expanded recursively at compile time by
/// [`StaticChain`] — no need to hand-write a `pipelineN` for each N.
///
/// # Why not a const-generic DAG
///
/// Stable Rust cannot express an "arbitrary edge table" with const generics
/// while keeping port type safety: an edge table `(usize, usize)` is
/// value-level information, while endpoint port types are type-level
/// information — the mapping between them needs GAT /
/// `generic_const_exprs`. `Chain` reaches the same goal (arbitrary-depth
/// chains) with recursive nested types; fan-out/fan-in is composed with
/// `StraightSplit`/`StraightMerge`, forming a compile-time expression of
/// arbitrary DAGs.
///
/// # Zero cost
///
/// `StaticChain::run_all` is fully monomorphized: every stage's
/// `process_straight` and `StraightLink::convert` are concrete functions,
/// fused into a single loop under `--release` + `#[inline]`. No
/// `Box<dyn Any>`, no trait dispatch, arbitrary depth.
pub struct Chain<Head, Tail, L> {
    _marker: PhantomData<(Head, Tail, L)>,
}

impl<Head, Tail, L> Chain<Head, Tail, L> {
    /// Construct a chain type value (a pure type marker with no runtime
    /// representation).
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<Head, Tail, L> Default for Chain<Head, Tail, L> {
    fn default() -> Self {
        Self::new()
    }
}

/// The recursive execution contract for compile-time linear chains.
///
/// - A single machine `M: StraightMachine` implements it automatically (base
///   case).
/// - `Chain<Head, Tail>` is implemented as "run Head →
///   `StraightLink::convert` conversion → recursively `Tail::run_all`"
///   (recursive step), expanded at compile time to arbitrary depth.
///
/// Streaming execution contract (P0 paradigm shift: execution-shape
/// isomorphism).
///
/// Holds the combination of all machines' State on the chain (a type tuple,
/// expanded at compile time), streaming element by element through — a single
/// for loop + nested calls + only the out `Vec`, isomorphic to the execution
/// shape of a hand-written loop (eliminating the shape difference of batch
/// transits, `ε → 0`). This answers "what is the better implementation": the
/// static path evolves from "recursive batch transit" to a "linear streaming
/// state machine".
pub trait FlowThrough: Sized {
    /// The head machine type (needed by the recursive step:
    /// `StraightLink<Prev, Self::Head>`).
    ///
    /// `process_one`'s input type is
    /// `<Self::Head as StraightMachine>::StraightIn` — sources/destinations
    /// are fixed by the type system at compile time, with zero validation at
    /// physical execution (P0).
    type Head: StraightMachine;
    /// The per-element output type (the tail machine's payload).
    type Out;
    /// The combination of all machines' State on the chain (a type tuple).
    type States;

    /// Initialize all machines' State (once, for use by the whole batch
    /// streaming).
    fn new_states() -> Result<Self::States, StaticExecError>;
    /// Process one element: one input flows through the whole chain, producing
    /// one output.
    fn process_one(
        states: &mut Self::States,
        input: <Self::Head as StraightMachine>::StraightIn,
    ) -> Self::Out;
    /// Clean up all machines' State (once, at the end of the batch).
    fn cleanup(states: Self::States) -> Result<(), StaticExecError>;
}

/// The recursive execution contract for compile-time linear chains.
///
/// - A single machine `M: StraightMachine` implements it automatically (base
///   case).
/// - `Chain<Head, Tail>` is implemented as "run Head →
///   `StraightLink::convert` conversion → recursively `Tail::process_one`"
///   (recursive step), expanded at compile time to arbitrary depth.
///
/// `run_all` consumes all inputs and returns the final outputs, **streaming
/// internally** (`FlowThrough`): all machines' State initialized once, then
/// elements flow through the whole chain one by one — no intermediate `Vec`
/// transit; the execution shape is isomorphic to a hand-written loop.
pub trait StaticChain: FlowThrough {
    /// Execute the entire chain in one shot. The input type is the head
    /// machine's raw input.
    ///
    /// The default implementation is **streaming**: `new_states` initializes
    /// all State once → `process_one` per element → `cleanup` at batch end.
    /// Only the out `Vec` is allocated (`with_capacity`).
    fn run_all(
        inputs: Vec<<Self::Head as StraightMachine>::StraightIn>,
    ) -> Result<Vec<Self::Out>, StaticExecError> {
        let mut states = Self::new_states()?;
        let mut outputs = Vec::with_capacity(inputs.len());
        for input in inputs {
            outputs.push(Self::process_one(&mut states, input));
        }
        Self::cleanup(states)?;
        Ok(outputs)
    }
}

// ── StaticTopology blueprint projection (T1) ─────────────────────────────────
//
// Every type implementing `StaticChain` (a `StraightMachine` single machine,
// the `Chain`/`Diamond`/`Composite` combinators) is the **compile-time
// projection** [`StaticTopology`] of the blueprint `Topology`: the shape is
// fully known at compile time, the execution form is monomorphized by the type
// system, and there is no runtime topology object. This blanket impl makes the
// whole static execution path (this module's combinators + the Straight
// contract) serve as the only implementation surface of `StaticTopology`,
// usable by generic constraints that require `T: StaticTopology`.
impl<T: StaticChain> StaticTopology for T {}

// Base case: a single machine (any StraightMachine machine is a one-stage
// chain).
impl<M> FlowThrough for M
where
    M: StraightMachine,
{
    type Head = M;
    type Out = M::StraightOut;
    type States = M::State;

    fn new_states() -> Result<Self::States, StaticExecError> {
        let ctx = MachineContext::new(M::name());
        M::init(&ctx).map_err(|e| StaticExecError::InitFailed {
            machine: M::name(),
            reason: e.to_string(),
        })
    }
    fn process_one(state: &mut Self::States, input: M::StraightIn) -> Self::Out {
        M::process_straight(state, input)
    }
    fn cleanup(state: Self::States) -> Result<(), StaticExecError> {
        let ctx = MachineContext::new(M::name());
        M::cleanup(state, &ctx).map_err(|e| StaticExecError::CleanupFailed {
            machine: M::name(),
            reason: e.to_string(),
        })
    }
}

impl<M> StaticChain for M where M: StraightMachine {}

// Recursive step: Head → Tail.
impl<Head, Tail, L> FlowThrough for Chain<Head, Tail, L>
where
    Head: StraightMachine,
    Tail: StaticChain,
    L: StraightLink<Head, Tail::Head>,
{
    type Head = Head;
    type Out = Tail::Out;
    type States = (Head::State, Tail::States);

    fn new_states() -> Result<Self::States, StaticExecError> {
        let ctx = MachineContext::new(Head::name());
        let head_state = Head::init(&ctx).map_err(|e| StaticExecError::InitFailed {
            machine: Head::name(),
            reason: e.to_string(),
        })?;
        let tail_states = Tail::new_states()?;
        Ok((head_state, tail_states))
    }
    fn process_one(
        (head_state, tail_states): &mut Self::States,
        input: Head::StraightIn,
    ) -> Self::Out {
        // Streaming: values flow directly through Head → Link → Tail, with no
        // intermediate Vec.
        let head_out = Head::process_straight(head_state, input);
        let tail_in = L::convert(head_out);
        Tail::process_one(tail_states, tail_in)
    }
    fn cleanup((head_state, tail_states): Self::States) -> Result<(), StaticExecError> {
        let ctx = MachineContext::new(Head::name());
        Head::cleanup(head_state, &ctx).map_err(|e| StaticExecError::CleanupFailed {
            machine: Head::name(),
            reason: e.to_string(),
        })?;
        Tail::cleanup(tail_states)
    }
}

impl<Head, Tail, L> StaticChain for Chain<Head, Tail, L>
where
    Head: StraightMachine,
    Tail: StaticChain,
    L: StraightLink<Head, Tail::Head>,
{
}

// ════════════════════════════════════════════════════════════════════════════
// Section 4.6: Diamond — compile-time diamond combinator (fork → two arms → join)
// ════════════════════════════════════════════════════════════════════════════

/// Compile-time diamond combinator: `A → Split → (Left, Right) → Merge → Down`.
///
/// This is the core building block that moves the static path from "linear +
/// independent fork/join" toward "arbitrary DAGs": an upstream `A` forks via
/// [`StraightSplit`] into two **arbitrary-depth chains** ([`StaticChain`]),
/// then joins via [`StraightMerge`] zip-pairing into one downstream chain.
/// The left/right arms and the downstream may be single machines (`FusedInline`
/// automatically implements `StaticChain`) or arbitrarily nested [`Chain`]s.
///
/// The diamond is the minimal complete composition of fan-out + fan-in.
/// `Diamond` expands the upstream + two arms + downstream topology at compile
/// time in one shot, eliminating the type friction of the intermediate
/// connections between fork and join.
///
/// # Composability
///
/// `Diamond` implements [`StaticChain`], so it is at the same level as a
/// single machine: it can be embedded as a section of an arbitrarily deep
/// chain —
///
/// ```text
/// Chain<X, Diamond<A, Left, Right, Down, S, LB, LC, M>, LX>   // X → diamond
/// Chain<Diamond<A, Left, Right, Down, S, LB, LC, M>, Y, LD>   // diamond → Y
/// ```
///
/// And a diamond's arms can themselves be `Chain`s (even another `Diamond`),
/// so "fork → two chains → join → downstream chain" can nest recursively,
/// approaching arbitrary DAGs.
///
/// # Zero cost
///
/// `run_all` is fully monomorphized: `A::process_straight`, each arm machine's
/// `process_straight`, `S::split`, `LB/LC::convert`, `M::merge` are all
/// concrete raw functions, fused into a single loop under `--release` +
/// `#[inline]`. No `Box<dyn Any>`, no trait dispatch, no port enum labels (P0).
///
/// # Type parameters
///
/// - `A` upstream (a single-machine `StraightMachine`), `Left`/`Right` the two
///   arms (`StaticChain`), `Down` downstream (`StaticChain`)
/// - `S: StraightSplit<A::StraightOut, Left = A::StraightOut, Right = A::StraightOut>`:
///   the fork (raw payload, no enum labels)
/// - `LB: StraightLink<A, Left::Head>`、`LC: StraightLink<A, Right::Head>`:
///   raw-payload conversions after the fork (targets are the two arms' head
///   machines respectively)
/// - `M: StraightMerge<Left::Output, Right::Output, Output = Down::Head::StraightIn>`:
///   the join (zip-pairs the two arms' tail raw outputs, merging into the
///   downstream head machine's raw input)
pub struct Diamond<A, Left, Right, Down, S, LB, LC, M> {
    _marker: PhantomData<(A, Left, Right, Down, S, LB, LC, M)>,
}

impl<A, Left, Right, Down, S, LB, LC, M> Diamond<A, Left, Right, Down, S, LB, LC, M> {
    /// Construct a diamond type value (a pure type marker with no runtime
    /// representation).
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<A, Left, Right, Down, S, LB, LC, M> Default for Diamond<A, Left, Right, Down, S, LB, LC, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A, Left, Right, Down, S, LB, LC, M> FlowThrough for Diamond<A, Left, Right, Down, S, LB, LC, M>
where
    A: StraightMachine,
    Left: StaticChain,
    Right: StaticChain,
    Down: StaticChain,
    S: StraightSplit<A::StraightOut, Left = A::StraightOut, Right = A::StraightOut>,
    LB: StraightLink<A, Left::Head>,
    LC: StraightLink<A, Right::Head>,
    M: StraightMerge<Left::Out, Right::Out, Output = <Down::Head as StraightMachine>::StraightIn>,
{
    type Head = A;
    type Out = Down::Out;
    type States = (A::State, Left::States, Right::States, Down::States);

    fn new_states() -> Result<Self::States, StaticExecError> {
        let ctx_a = MachineContext::new(A::name());
        let a_state = A::init(&ctx_a).map_err(|e| StaticExecError::InitFailed {
            machine: A::name(),
            reason: e.to_string(),
        })?;
        let left_states = Left::new_states()?;
        let right_states = Right::new_states()?;
        let down_states = Down::new_states()?;
        Ok((a_state, left_states, right_states, down_states))
    }

    fn process_one(
        (a_state, left_states, right_states, down_states): &mut Self::States,
        input: A::StraightIn,
    ) -> Self::Out {
        // Streaming: one input flows through A → Split → both arms → Merge →
        // Down, with no intermediate Vec.
        let a_out = A::process_straight(a_state, input);
        let (left, right) = S::split(a_out);
        let left_out = Left::process_one(left_states, LB::convert(left));
        let right_out = Right::process_one(right_states, LC::convert(right));
        let merged = M::merge(left_out, right_out);
        Down::process_one(down_states, merged)
    }

    fn cleanup(
        (a_state, left_states, right_states, down_states): Self::States,
    ) -> Result<(), StaticExecError> {
        let ctx_a = MachineContext::new(A::name());
        A::cleanup(a_state, &ctx_a).map_err(|e| StaticExecError::CleanupFailed {
            machine: A::name(),
            reason: e.to_string(),
        })?;
        Left::cleanup(left_states)?;
        Right::cleanup(right_states)?;
        Down::cleanup(down_states)
    }
}

impl<A, Left, Right, Down, S, LB, LC, M> StaticChain for Diamond<A, Left, Right, Down, S, LB, LC, M>
where
    A: StraightMachine,
    Left: StaticChain,
    Right: StaticChain,
    Down: StaticChain,
    S: StraightSplit<A::StraightOut, Left = A::StraightOut, Right = A::StraightOut>,
    LB: StraightLink<A, Left::Head>,
    LC: StraightLink<A, Right::Head>,
    M: StraightMerge<Left::Out, Right::Out, Output = <Down::Head as StraightMachine>::StraightIn>,
{
}

// ════════════════════════════════════════════════════════════════════════════
// Section 4.7: Composite — typed composite node (hierarchical abstraction)
// ════════════════════════════════════════════════════════════════════════════

/// Typed composite node: wraps a sub-topology into a single node (naming /
/// hierarchy / reuse).
///
/// `Composite<Inner>` transparently forwards `Inner: FlowThrough`'s
/// `Head`/`Out`/`States` and `new_states`/`process_one`/`cleanup` — to the
/// outside it looks like a **single-in/single-out node**, and can be nested
/// into [`Chain`]/[`Diamond`]/other [`Composite`]s. Execution is **fully
/// equivalent** to directly expanding `Inner` (forwarding is inlining, zero
/// extra cost).
///
/// Value:
/// - **Abstraction**: name subsystems (`type Pipeline = Composite<Chain<...>>`),
///   hide internal structure, prevent misuse;
/// - **Hierarchy**: the nodes of a top-level series-parallel tree can be
///   composites (the static form of `structural-model.md` Definition 4.2);
/// - **Reuse**: the same composite type used in multiple places (e.g. a shared
///   subsystem in multi-way parallelism).
///
/// Note: composites provide **abstraction**, not an extension of the graph
/// class (`FlowThrough` is a single-in/single-out algebra; multi-in/multi-out
/// sub-topologies need `run_parallel` or the dynamic path).
pub struct Composite<Inner>(core::marker::PhantomData<Inner>);

impl<Inner> FlowThrough for Composite<Inner>
where
    Inner: FlowThrough,
{
    type Head = Inner::Head;
    type Out = Inner::Out;
    type States = Inner::States;

    fn new_states() -> Result<Self::States, StaticExecError> {
        Inner::new_states()
    }
    fn process_one(
        states: &mut Self::States,
        input: <Self::Head as StraightMachine>::StraightIn,
    ) -> Self::Out {
        Inner::process_one(states, input)
    }
    fn cleanup(states: Self::States) -> Result<(), StaticExecError> {
        Inner::cleanup(states)
    }
}

impl<Inner> StaticChain for Composite<Inner>
where
    Inner: StaticChain,
{
}

/// Independently execute two [`FlowThrough`] chains in parallel (the first
/// building block of multi-stream static expression).
///
/// Each chain is single-in/single-out with **independent inputs and outputs**
/// (non-interfering state tuples) — under the synchronous batch model,
/// sequential execution and parallel execution give the same results (no
/// shared state), so they are semantically equivalent. This is the starting
/// point of "multi-stream": `run_parallel::<Composite<A>, Composite<B>>(...)`
/// runs two named independent subsystems in parallel.
///
/// Each stream independently calls `new_states`/`process_one`/`cleanup`, with
/// only the out `Vec` allocated (`with_capacity`) — the same zero-cost shape
/// as [`FlowThrough`].
pub fn run_parallel<A: FlowThrough, B: FlowThrough>(
    a_inputs: Vec<<A::Head as StraightMachine>::StraightIn>,
    b_inputs: Vec<<B::Head as StraightMachine>::StraightIn>,
) -> Result<(Vec<A::Out>, Vec<B::Out>), StaticExecError> {
    let mut a_states = A::new_states()?;
    let mut a_out = Vec::with_capacity(a_inputs.len());
    for x in a_inputs {
        a_out.push(A::process_one(&mut a_states, x));
    }
    A::cleanup(a_states)?;

    let mut b_states = B::new_states()?;
    let mut b_out = Vec::with_capacity(b_inputs.len());
    for x in b_inputs {
        b_out.push(B::process_one(&mut b_states, x));
    }
    B::cleanup(b_states)?;

    Ok((a_out, b_out))
}

// ════════════════════════════════════════════════════════════════════════════
// Section 5: Unit tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declare_ports;
    use crate::machine::{CleanupError, InitError, Machine, SingleOutput};
    use crate::port::MachineContext;

    // ── Test machines (Machine enum contract + StraightMachine raw-payload contract) ─

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct DoublerPorts {
            input type DoublerInput {
                x[Data] => i32,
            }
            output type DoublerOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct Doubler;
    impl Machine for Doubler {
        type State = ();
        type Input = DoublerInput;
        type Output = DoublerOutput;
        type Ports = DoublerPorts;
        type ProcessOutput = SingleOutput<DoublerOutput>;
        fn name() -> &'static str { "doubler" }
        fn config_schema() -> crate::port::ConfigSchema { crate::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
        fn process(_: &mut (), _: &MachineContext, input: DoublerInput) -> SingleOutput<DoublerOutput> {
            match input {
                DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
            }
        }
        fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl StraightMachine for Doubler {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n * 2 }
    }

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct AdderPorts {
            input type AdderInput {
                x[Data] => i32,
            }
            output type AdderOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct Adder;
    impl Machine for Adder {
        type State = i32;
        type Input = AdderInput;
        type Output = AdderOutput;
        type Ports = AdderPorts;
        type ProcessOutput = SingleOutput<AdderOutput>;
        fn name() -> &'static str { "adder" }
        fn config_schema() -> crate::port::ConfigSchema { crate::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<i32, InitError> { Ok(0) }
        fn process(state: &mut i32, _: &MachineContext, input: AdderInput) -> SingleOutput<AdderOutput> {
            match input {
                AdderInput::x(n) => {
                    *state += n;
                    SingleOutput::Yield(AdderOutput::y(*state))
                }
            }
        }
        fn cleanup(_: i32, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl StraightMachine for Adder {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(state: &mut i32, n: i32) -> i32 {
            *state += n;
            *state
        }
    }

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct TriplerPorts {
            input type TriplerInput {
                x[Data] => i32,
            }
            output type TriplerOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct Tripler;
    impl Machine for Tripler {
        type State = ();
        type Input = TriplerInput;
        type Output = TriplerOutput;
        type Ports = TriplerPorts;
        type ProcessOutput = SingleOutput<TriplerOutput>;
        fn name() -> &'static str { "tripler" }
        fn config_schema() -> crate::port::ConfigSchema { crate::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
        fn process(_: &mut (), _: &MachineContext, input: TriplerInput) -> SingleOutput<TriplerOutput> {
            match input {
                TriplerInput::x(n) => SingleOutput::Yield(TriplerOutput::y(n * 3)),
            }
        }
        fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl StraightMachine for Tripler {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n * 3 }
    }

    // ── Raw-payload merge (StraightMerge) ─────────────────────────────────

    struct Sum;
    impl StraightMerge<i32, i32> for Sum {
        type Output = i32;
        #[inline]
        fn merge(a: i32, b: i32) -> i32 { a + b }
    }

    // ══ Straight contract unit tests ══════════════════════════════════════

    #[test]
    fn straight_machine_single() {
        // Single-machine pass-through: raw payload, no enum.
        let outputs = Doubler::run_all(vec![1, 2, 3]).expect("doubler");
        assert_eq!(outputs, vec![2, 4, 6]);
    }

    #[test]
    fn straight_machine_empty() {
        let outputs = Doubler::run_all(vec![]).expect("empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn straight_id_convert() {
        // StraightId: identity conversion when the payload types are the same
        // (i32 Into i32).
        let x: i32 = <StraightId as StraightLink<Doubler, Adder>>::convert(7);
        assert_eq!(x, 7);
    }

    #[test]
    fn straight_clone_split_duplicates() {
        let (a, b) = StraightClone::split(42i32);
        assert_eq!(a, 42);
        assert_eq!(b, 42);
    }

    #[test]
    fn straight_merge_sums() {
        assert_eq!(Sum::merge(3, 4), 7);
    }

    // ══ StaticChain: Chain tests ═════════════════════════════════════════

    #[test]
    fn chain_three_stage_recursive() {
        // Doubler → Adder → Tripler (a 3-level recursive chain, StraightId links)
        // input [1]: D(2) → A(2) → T(6)
        type Chain3 = Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>;
        let outputs = Chain3::run_all(vec![1]).expect("chain3");
        assert_eq!(outputs, vec![6]);
    }

    #[test]
    fn chain_recursive_multi_input() {
        // Doubler → Adder (Adder accumulates across inputs)
        // inputs [1,2,3]: D(2,4,6) → A(2,6,12)
        type Chain2 = Chain<Doubler, Adder, StraightId>;
        let outputs = Chain2::run_all(vec![1, 2, 3]).expect("chain2");
        assert_eq!(outputs, vec![2, 6, 12]);
    }

    #[test]
    fn chain_empty_inputs() {
        type Chain3 = Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>;
        let outputs = Chain3::run_all(vec![]).expect("chain3 empty");
        assert!(outputs.is_empty());
    }

    // ══ Diamond tests ════════════════════════════════════════════════════

    type DiamondShape = Diamond<
        Doubler,
        Adder,
        Tripler,
        Adder,
        StraightClone,
        StraightId,
        StraightId,
        Sum,
    >;

    #[test]
    fn diamond_runs_split_then_merge() {
        // Doubler → StraightClone → (Adder, Tripler) → Sum → Adder
        // inputs [1, 2]: D(2,4) → split(2,2),(4,4) → A(2,6), T(6,12) → Sum(8,18) → A(8,26)
        let outputs = DiamondShape::run_all(vec![1, 2]).expect("diamond");
        assert_eq!(outputs, vec![8, 26]);
    }

    #[test]
    fn diamond_empty_inputs() {
        let outputs = DiamondShape::run_all(vec![]).expect("diamond empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn diamond_embeds_as_chain_tail() {
        // Diamond implements StaticChain, so it can be embedded as a Chain's
        // Tail.
        // Chain<Doubler, DiamondShape, StraightId>: outer Doubler → diamond
        type ChainWithDiamond = Chain<Doubler, DiamondShape, StraightId>;
        // input [1]: outer D(2) → diamond D(4) → split(4,4) → A(4), T(12) → Sum(16) → A(16)
        let outputs = ChainWithDiamond::run_all(vec![1]).expect("chain+diamond");
        assert_eq!(outputs, vec![16]);
    }

    #[test]
    fn diamond_arms_are_chains() {
        // The diamond's two arms are arbitrary-depth chains (2 stages each):
        // left arm Adder→Doubler, right arm Tripler→Doubler.
        type LeftArm = Chain<Adder, Doubler, StraightId>;
        type RightArm = Chain<Tripler, Doubler, StraightId>;
        type DChainArms = Diamond<
            Doubler,
            LeftArm,
            RightArm,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >;
        // input [1]: D(2) → split(2,2)
        //   left arm A→D: 2 → A(2) → D(4)
        //   right arm T→D: 2 → T(6) → D(12)
        //   Sum(16) → downstream A(16)
        let outputs = DChainArms::run_all(vec![1]).expect("diamond chain arms");
        assert_eq!(outputs, vec![16]);
    }

    // ══ Composite tests (Section 4.7) ════════════════════════════════════

    #[test]
    fn composite_matches_direct_chain() {
        // Composite<Chain3> gives the same result as direct Chain3
        // (transparent forwarding, zero extra cost).
        type Direct = Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>;
        type Wrapped = Composite<Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>>;
        let d = Direct::run_all(vec![1, 2]).expect("direct");
        let w = Wrapped::run_all(vec![1, 2]).expect("composite");
        assert_eq!(d, w, "composite must be semantically identical to its inner topology");
        assert_eq!(d, vec![6, 18]);
    }

    #[test]
    fn composite_embeds_in_chain() {
        // Chain<Doubler, Composite<Chain<Adder, Tripler>>, StraightId>:
        // a composite as the chain's Tail (hierarchy: the nodes of a top-level
        // series-parallel tree are composites).
        type Sub = Composite<Chain<Adder, Tripler, StraightId>>;
        type Top = Chain<Doubler, Sub, StraightId>;
        // input [1]: D(2) → A(2) → T(6)
        let outputs = Top::run_all(vec![1]).expect("chain with composite tail");
        assert_eq!(outputs, vec![6]);
    }

    #[test]
    fn composite_embeds_in_diamond_arm() {
        // A diamond's arms are Composites (each arm wraps a chain).
        type LeftArm = Composite<Chain<Adder, Tripler, StraightId>>;
        type RightArm = Composite<Chain<Tripler, Doubler, StraightId>>;
        type D = Diamond<
            Doubler,
            LeftArm,
            RightArm,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >;
        // input [1]: D(2) → split(2,2)
        //   left arm A→T: 2 → A(2) → T(6)
        //   right arm T→D: 2 → T(6) → D(12)
        //   Sum(18) → downstream A(18)
        let outputs = D::run_all(vec![1]).expect("diamond with composite arms");
        assert_eq!(outputs, vec![18]);
    }

    // ══ Parallel tests (Section 4.7) ═════════════════════════════════════

    #[test]
    fn parallel_runs_two_independent_chains() {
        // Two independent chains: A chain Doubler→Adder; B chain Tripler.
        // Independent inputs and outputs; results match running each alone.
        type A = Chain<Doubler, Adder, StraightId>;
        type B = Tripler;
        let (a_out, b_out) = run_parallel::<A, B>(vec![1, 2, 3], vec![10]).expect("parallel");
        // A: D(2,4,6) → A accumulates(2,6,12); B: T(30)
        assert_eq!(a_out, vec![2, 6, 12]);
        assert_eq!(b_out, vec![30]);
    }

    #[test]
    fn parallel_with_composite_subsystems() {
        // Run two named composite subsystems in parallel (multi-stream +
        // hierarchy combined).
        type SubA = Composite<Chain<Doubler, Tripler, StraightId>>;
        type SubB = Composite<Chain<Adder, Doubler, StraightId>>;
        let (a_out, b_out) = run_parallel::<SubA, SubB>(vec![2], vec![3]).expect("parallel composite");
        // SubA: D(4) → T(12); SubB: A(3) → D(6)
        assert_eq!(a_out, vec![12]);
        assert_eq!(b_out, vec![6]);
    }

    #[test]
    fn diamond_arm_is_diamond() {
        // A diamond inside a diamond: the outer left arm is itself a complete
        // diamond — recursive completeness.
        type InnerDiamond = Diamond<
            Doubler,
            Adder,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >;
        type OuterDiamond = Diamond<
            Doubler,
            InnerDiamond,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >;
        // input [1]: outer D(2) → split(2,2)
        //   left arm InnerDiamond(2): D(4) → split(4,4) → A(4), T(12) → Sum(16) → A(16)
        //   right arm Tripler(2): 6
        //   Sum(16+6=22) → downstream A(22)
        let outputs = OuterDiamond::run_all(vec![1]).expect("diamond in diamond");
        assert_eq!(outputs, vec![22]);
    }

    #[test]
    fn static_exec_error_display() {
        let e = StaticExecError::InitFailed {
            machine: "doubler",
            reason: "out of memory".into(),
        };
        let s = alloc::format!("{e}");
        assert!(s.contains("doubler"));
        assert!(s.contains("out of memory"));

        let e = StaticExecError::MachineDone {
            machine: "source",
            processed: 100,
        };
        let s = alloc::format!("{e}");
        assert!(s.contains("source"));
        assert!(s.contains("100"));
    }
}
