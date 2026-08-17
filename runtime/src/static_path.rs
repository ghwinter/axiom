//! Static execution paths — zero-cost batched execution for compile-time-known topologies.
//!
//! # Role
//!
//! This module is the execution side of `axiom::static_exec` (the core type contract). It provides
//! concrete functions that drive `FusedInline` machines according to a compile-time-known topology —
//! fully monomorphized, no `Box<dyn Any>`, no trait dispatch, no heap allocation/messages.
//!
//! # Execution model: synchronous batched topology order
//!
//! Unlike the dynamic path (`Runtime::materialize` → per-message `tick` driving), the static path
//! uses a **synchronous batched** model:
//!
//! 1. Input `Vec<I>` → executed per machine in topology order
//! 2. Each machine's outputs are collected as `Vec<Output>`
//! 3. Fan-out splits via `StraightSplit` (a `Diamond` arm)
//! 4. Fan-in merges via `StraightMerge` (the `Diamond` join)
//! 5. Final output `Vec<O>`
//!
//! This is the natural generalization of the original linear probe (the deleted
//! `LinearRuntime::pipeline3`) to arbitrary DAGs — extending from
//! `inputs.iter().map(|x| c(b(a(x)))).collect()` to support branching and joining.
//!
//! # Comparison with the dynamic path
//!
//! | Dimension | Static path (this module) | Dynamic path (`Runtime`) |
//! |-----------|----------------------------|--------------------------|
//! | Topology decided | compile time | runtime (`DynamicTopology`) |
//! | Type erasure | none (concrete types monomorphized) | `Box<dyn Any>` |
//! | Per-message cost | zero (no heap allocation) | ~5x (heap allocation + dispatch) |
//! | Topology capability | serial-parallel DAG + single-machine feedback loop | arbitrary DAG + cycles |
//! | IO/async | unsupported | supported (`IoReactor`) |
//! | Suitable for | fixed pipelines, hot paths | config-driven, plugins, dynamic topology |
//!
//! # Combinators: the only static entry points
//!
//! The only static execution entry points of this module are **combinators** + the `Straight` contract:
//!
//! - `Chain` (arbitrary-depth linear chain) + `pipeline_chain`
//! - `Diamond` (fork-join; arms and downstream can be arbitrary chains) + `diamond`
//! - `feedback` (single-machine feedback loop)
//!
//! They compose recursively to express serial-parallel DAGs and feedback loops — the expressive
//! core of the static path. The old fixed-N convenience functions
//! (`pipeline2`/`pipeline3`/`fanout2`/`fanin2`) were removed in a breaking refactor — `Chain`/
//! `Diamond` are their superset, at arbitrary depth.
//!
//! # Safety
//!
//! The static path is based on the `Straight` contract (`StraightMachine`/`StaticChain`):
//! single-port, fixed-output machines pass raw payloads directly. `MultiOutput` (including
//! `YieldMulti`, runtime-determined output count) is rejected at the type level — the static path
//! handles compile-time-known output counts and cannot handle runtime-decided fan-out.

use axiom::machine::{CleanupError, InitError};
use axiom::port::MachineContext;
use axiom::static_exec::{
    Diamond, StaticChain, StaticExecError,
    StraightLink, StraightMachine, StraightMerge, StraightSplit,
};

use alloc::format;
use alloc::vec::Vec;

// ── Helpers: error conversion ───────────────────────────────────────────────────────────

fn init_err(machine: &'static str, e: InitError) -> StaticExecError {
    StaticExecError::InitFailed {
        machine,
        reason: format!("{e}"),
    }
}

fn cleanup_err(machine: &'static str, e: CleanupError) -> StaticExecError {
    StaticExecError::CleanupFailed {
        machine,
        reason: format!("{e}"),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Linear pipeline
// ════════════════════════════════════════════════════════════════════════════

/// Arbitrary-depth linear pipeline (compile-time recursive chain).
///
/// The depth is determined by types rather than hand-written functions: nesting the `Chain`
/// combinator yields an arbitrary N-stage chain (roadmap C1):
///
/// ```ignore
/// use axiom::static_exec::Chain;
/// use axiom_runtime::static_path::pipeline_chain;
///
/// // 4-stage chain: Doubler → Tripler → Adder → Negater
/// type MyChain = Chain<Doubler, Chain<Tripler, Chain<Adder, Negater>>, DToT>;
/// let outputs = pipeline_chain::<MyChain>(vec![/* inputs */])?;
/// ```
///
/// Expanded recursively at compile time by [`StaticChain`] — no `Box<dyn Any>`, no trait
/// dispatch, zero cost (replacing the removed fixed `pipelineN` functions).
pub fn pipeline_chain<C: axiom::static_exec::StaticChain>(
    inputs: Vec<
        <<C as axiom::static_exec::FlowThrough>::Head as axiom::static_exec::StraightMachine>::StraightIn,
    >,
) -> Result<Vec<C::Out>, StaticExecError> {
    C::run_all(inputs)
}

// ════════════════════════════════════════════════════════════════════════════
// Diamond: A → Split → (Left, Right) → Merge → Down
// ════════════════════════════════════════════════════════════════════════════

/// Diamond execution: `A → Split → (Left, Right) → Merge → Down`.
///
/// Convenience entry point for the [`Diamond`] combinator, equivalent to
/// `<Diamond<A, Left, Right, Down, S, LB, LC, M> as StaticChain>::run_all(inputs)`.
///
/// One upstream forks via `S::split` into two arbitrary-depth chains (`Left`/`Right`, each a
/// single machine or a [`Chain`]), then joins via `M::merge` with zip pairing into one downstream
/// chain (`Down`). Expands the upstream + both arms + downstream topology in one pass, without
/// manually wiring the fork and join (the old `fanout2`/`fanin2` had mismatched endpoint types
/// and were removed).
///
/// # Zero cost
///
/// Fully monomorphized: `A::process`, each machine's `process` in both arms and the downstream
/// chain, `S::split`, `LB/LC::extract`, and `M::merge` are all concrete functions, fused under
/// `--release` + `#[inline]`. No `Box<dyn Any>`, no trait dispatch. See
/// `axiom::static_exec::Diamond`.
pub fn diamond<A, Left, Right, Down, S, LB, LC, M>(
    inputs: Vec<A::StraightIn>,
) -> Result<Vec<Down::Out>, StaticExecError>
where
    A: StraightMachine,
    Left: StaticChain,
    Right: StaticChain,
    Down: StaticChain,
    S: StraightSplit<A::StraightOut, Left = A::StraightOut, Right = A::StraightOut>,
    LB: StraightLink<A, Left::Head>,
    LC: StraightLink<A, Right::Head>,
    M: StraightMerge<
        Left::Out,
        Right::Out,
        Output = <Down::Head as StraightMachine>::StraightIn,
    >,
{
    <Diamond<A, Left, Right, Down, S, LB, LC, M> as StaticChain>::run_all(inputs)
}

// ════════════════════════════════════════════════════════════════════════════
// Feedback loop: A's output fed back into A's input with a one-tick delay
// ════════════════════════════════════════════════════════════════════════════

/// Feedback loop: `A`'s output is fed back into `A`'s input with a one-tick delay.
///
/// The static path's first step from acyclic DAGs toward **deterministic cyclic** topologies:
/// a single-machine self-feedback loop. Each tick, `A` consumes "external input + the previous
/// tick's output" and produces a new output — which is both this round's result and, via an
/// implicit delay (equivalent to [`Latch`](axiom::builtin::Latch)'s one-tick delay), fed back as
/// the next tick's input.
///
/// Loop semantics (`t` is the tick index):
///
/// ```text
/// output[0] = A(merge(input[0], initial))
/// output[t] = A(merge(input[t], output[t-1]))
/// ```
///
/// The loop is split into an "acyclic body `A` + delayed back edge": the delay is simulated by
/// internal state, and the first tick's feedback is the caller-supplied `initial`. This is the key
/// to expressing a cyclic topology on the acyclic synchronous-batched model — an explicit delay
/// lets the loop be statically monomorphized without the implicit delay of runtime channels.
///
/// # Zero cost
///
/// Each tick's `M::merge` and `A::process_straight` are raw functions, monomorphized; no
/// `Box<dyn Any>`, no trait dispatch, no port enumeration labels (P0). Unlike the batched model
/// of `Chain`/`Diamond`, `feedback` interleaves tick by tick (each tick's output is fed back
/// immediately) — that is exactly the loop's execution semantics.
///
/// # Type parameters
///
/// - `A`: the machine on the loop (`StraightMachine`, raw payload)
/// - `M: StraightMerge<A::StraightIn, A::StraightOut, Output = A::StraightIn>`: merges the
///   external input (`A::StraightIn`) with the feedback (`A::StraightOut`) into `A`'s new input
/// - `initial`: the first tick's feedback value (explicit, avoiding an implicit `Default`)
pub fn feedback<A, M>(
    inputs: Vec<A::StraightIn>,
    initial: A::StraightOut,
) -> Result<Vec<A::StraightOut>, StaticExecError>
where
    A: StraightMachine,
    A::StraightOut: Clone,
    M: StraightMerge<A::StraightIn, A::StraightOut, Output = A::StraightIn>,
{
    let ctx = MachineContext::new(A::name());
    let mut state = A::init(&ctx).map_err(|e| init_err(A::name(), e))?;

    let mut prev: A::StraightOut = initial;
    let mut outputs = Vec::with_capacity(inputs.len());

    for input in inputs {
        // Raw payload passed directly: merge consumes the feedback (move), process has no
        // enum. The result and the feedback are two consumers — one Clone (business dispatch,
        // not a tag tax).
        let merged = M::merge(input, prev);
        let out = A::process_straight(&mut state, merged);
        outputs.push(out.clone());
        prev = out;
    }

    A::cleanup(state, &ctx).map_err(|e| cleanup_err(A::name(), e))?;
    Ok(outputs)
}

// ════════════════════════════════════════════════════════════════════════════
// Unit tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::declare_ports;
    use axiom::machine::{FusedInline, Machine, SingleOutput};
    use axiom::port::MachineContext;
    use axiom::static_exec::{
        Chain, StraightClone, StraightId, StraightMachine, StraightMerge,
    };

    // ── Test machines ──────────────────────────────────────────────────────────

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
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
        fn process(
            _: &mut (),
            _: &MachineContext,
            input: DoublerInput,
        ) -> SingleOutput<DoublerOutput> {
            match input {
                DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
            }
        }
        fn cleanup(
            _: (),
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for Doubler {}
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
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<i32, axiom::machine::InitError> { Ok(0) }
        fn process(
            state: &mut i32,
            _: &MachineContext,
            input: AdderInput,
        ) -> SingleOutput<AdderOutput> {
            match input {
                AdderInput::x(n) => {
                    *state += n;
                    SingleOutput::Yield(AdderOutput::y(*state))
                }
            }
        }
        fn cleanup(
            _: i32,
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for Adder {}
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
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
        fn process(
            _: &mut (),
            _: &MachineContext,
            input: TriplerInput,
        ) -> SingleOutput<TriplerOutput> {
            match input {
                TriplerInput::x(n) => SingleOutput::Yield(TriplerOutput::y(n * 3)),
            }
        }
        fn cleanup(
            _: (),
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for Tripler {}
    impl StraightMachine for Tripler {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n * 3 }
    }

    // ── pipeline_chain tests (compile-time recursive chain, Straight raw payload) ─

    #[test]
    fn pipeline_chain_4_stage_recursive() {
        // Doubler → Adder → Doubler → Adder (4-level recursive chain, arbitrary depth)
        type Chain4 = Chain<
            Doubler,
            Chain<Adder, Chain<Doubler, Adder, StraightId>, StraightId>,
            StraightId,
        >;
        // inputs [1, 2]: 1→D(2)→A1(2)→D(4)→A2(4); 2→D(4)→A1(6)→D(12)→A2(16)
        let outputs = pipeline_chain::<Chain4>(vec![1, 2]).expect("chain4");
        assert_eq!(outputs, vec![4, 16]);
    }

    #[test]
    fn pipeline_chain_3_stage_recursive() {
        // Doubler → Tripler → Adder (3 stages, StraightId raw links)
        type Chain3 = Chain<Doubler, Chain<Tripler, Adder, StraightId>, StraightId>;
        // input [2]: 2→D(4)→T(12)→A(12)
        let outputs = pipeline_chain::<Chain3>(vec![2]).expect("chain3");
        assert_eq!(outputs, vec![12]);
    }

    #[test]
    fn pipeline_chain_empty_inputs() {
        type Chain4 = Chain<
            Doubler,
            Chain<Adder, Chain<Doubler, Adder, StraightId>, StraightId>,
            StraightId,
        >;
        let outputs = pipeline_chain::<Chain4>(vec![]).expect("chain4 empty");
        assert!(outputs.is_empty());
    }

    // ── diamond tests (Straight raw payload) ──────────────────────────────────

    /// Raw merge: sum.
    struct Sum;
    impl StraightMerge<i32, i32> for Sum {
        type Output = i32;
        #[inline]
        fn merge(a: i32, b: i32) -> i32 {
            a + b
        }
    }

    #[test]
    fn diamond_runs_split_then_merge() {
        // Diamond: Doubler → StraightClone → (Adder, Tripler) → Sum → Adder
        // inputs [1, 2]: D(2,4) → split(2,2),(4,4) → A(2,6), T(6,12) → Sum(8,18) → A(8,26)
        let outputs = diamond::<
            Doubler,
            Adder,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >(vec![1, 2])
        .expect("diamond");
        assert_eq!(outputs, vec![8, 26]);
    }

    #[test]
    fn diamond_empty_inputs() {
        let outputs = diamond::<
            Doubler,
            Adder,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >(vec![])
        .expect("diamond empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn diamond_downstream_is_chain() {
        // The diamond downstream is a 2-stage chain: Chain<Adder, Doubler, StraightId>.
        type DownChain = Chain<Adder, Doubler, StraightId>;
        // input [1]: D(2) → split(2,2) → A(2), T(6) → Sum(8) → DownChain: A(8)→D(16)
        let outputs = diamond::<
            Doubler,
            Adder,
            Tripler,
            DownChain,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >(vec![1])
        .expect("diamond downstream chain");
        assert_eq!(outputs, vec![16]);
    }

    // ── feedback tests (Straight raw payload) ─────────────────────────────────

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct PassPorts {
            input type PassInput {
                x[Data] => i32,
            }
            output type PassOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct PassThrough;
    impl Machine for PassThrough {
        type State = ();
        type Input = PassInput;
        type Output = PassOutput;
        type Ports = PassPorts;
        type ProcessOutput = SingleOutput<PassOutput>;
        fn name() -> &'static str { "pass" }
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
        fn process(
            _: &mut (),
            _: &MachineContext,
            input: PassInput,
        ) -> SingleOutput<PassOutput> {
            match input {
                PassInput::x(n) => SingleOutput::Yield(PassOutput::y(n)),
            }
        }
        fn cleanup(
            _: (),
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for PassThrough {}
    impl StraightMachine for PassThrough {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n }
    }

    #[test]
    fn feedback_prefix_sum() {
        // Prefix sum: output[t] = input[t] + output[t-1]
        // A = PassThrough (pass-through), M = Sum (StraightMerge), initial = 0
        // input [1,2,3] → output [1, 3, 6]
        let outputs = feedback::<PassThrough, Sum>(vec![1, 2, 3], 0).expect("feedback");
        assert_eq!(outputs, vec![1, 3, 6]);
    }

    #[test]
    fn feedback_empty_inputs() {
        let outputs = feedback::<PassThrough, Sum>(vec![], 0).expect("feedback empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn feedback_nonzero_initial() {
        // Nonzero initial feedback: output[0] = input[0] + initial
        let outputs = feedback::<PassThrough, Sum>(vec![5], 100).expect("feedback initial");
        assert_eq!(outputs, vec![105]);
    }
}
