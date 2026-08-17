//! DAG-fusion benchmark: Diamond static path vs handwritten batch loop.
//!
//! Verifies **semantic equivalence and zero cost of DAG fusion**: `Diamond` flattens "split →
//! two paths → merge" into a single driving loop at compile time; after monomorphization there is no
//! `Box<dyn Any>`, no trait dispatch, **no port-enum tag** (P0: StraightMachine passes raw payloads directly).
//!
//! # Acceptance (after the P0 fix + the S3 generic path)
//!
//! - **Semantic equivalence**: `Diamond::run_all` is stage-for-stage equivalent to the handwritten loop.
//! - **Per-input cost**: the static path should be ≈ handwritten (`ε < 5%`, non-invasion axiom);
//!   compare with ~13× before the P0 fix (port-enum-tag tax).
//! - **Zero-cost abstraction (identical execution shape)**: executed through the `T: StaticChain`
//!   generic bound (the contract surface of a unified static entry), it should cost **bit-for-bit the same**
//!   as directly calling `DiamondShape::run_all` — after monomorphization both produce the same execution
//!   shape, and the abstraction adds no runtime overhead (S3 verifies the StaticTopology blueprint as a
//!   zero-sized "compile-time projection" marker, with no runtime topology object).
//!
//! Run with: `cargo bench --bench dag` (release mode).

#[path = "bench_harness.rs"]
mod bench_harness;

use bench_harness::BenchGroup;
use axiom::declare_ports;
use axiom::machine::{CleanupError, InitError, Machine, SingleOutput};
use axiom::port::MachineContext;
use axiom::static_exec::{
    Diamond, StraightClone, StraightId, StraightMachine, StraightMerge,
};
use axiom::static_exec::StaticChain;

// ── Test machines (Machine enum + StraightMachine raw payload) ───────────

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
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    #[inline]
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_: &mut (), _: &MachineContext, input: DoublerInput) -> SingleOutput<DoublerOutput> {
        match input {
            DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
        }
    }
    #[inline]
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
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    #[inline]
    fn init(_: &MachineContext) -> Result<i32, InitError> { Ok(0) }
    fn process(state: &mut i32, _: &MachineContext, input: AdderInput) -> SingleOutput<AdderOutput> {
        match input {
            AdderInput::x(n) => {
                *state = state.wrapping_add(n);
                SingleOutput::Yield(AdderOutput::y(*state))
            }
        }
    }
    #[inline]
    fn cleanup(_: i32, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}
impl StraightMachine for Adder {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(state: &mut i32, n: i32) -> i32 {
        *state = state.wrapping_add(n);
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
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    #[inline]
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_: &mut (), _: &MachineContext, input: TriplerInput) -> SingleOutput<TriplerOutput> {
        match input {
            TriplerInput::x(n) => SingleOutput::Yield(TriplerOutput::y(n * 3)),
        }
    }
    #[inline]
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}
impl StraightMachine for Tripler {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(_: &mut (), n: i32) -> i32 { n * 3 }
}

// ── Raw merge (StraightMerge) ────────────────────────────────────────────

struct Sum;
impl StraightMerge<i32, i32> for Sum {
    type Output = i32;
    #[inline]
    fn merge(a: i32, b: i32) -> i32 { a.wrapping_add(b) }
}

/// Diamond: Doubler → StraightClone → (Adder, Tripler) → Sum → Adder.
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

// ── Handwritten equivalent loop ──────────────────────────────────────────

/// Handwritten equivalent loop: batch model (a single out Vec; values flow straight through with no intermediate staging).
///
/// For each input `x`: Doubler `d = 2x` → StraightClone `(d,d)` → left-arm Adder
/// accumulation → right-arm Tripler `3d` → Sum → downstream Adder accumulation.
fn handwritten(inputs: Vec<i32>) -> Vec<i32> {
    let mut acc_left = 0i32;
    let mut acc_down = 0i32;
    let mut out = Vec::with_capacity(inputs.len());
    for x in inputs {
        let d = x * 2;
        acc_left = acc_left.wrapping_add(d);
        let merged = acc_left.wrapping_add(d * 3);
        acc_down = acc_down.wrapping_add(merged);
        out.push(acc_down);
    }
    out
}

/// Streaming Diamond (paradigm validation): each machine's State is initialized once, and a single for loop
/// nests the calls — the execution shape is isomorphic to the handwritten version (no intermediate Vec staging).
///
/// This proves the feasibility of the "streaming flow-through" paradigm: if it is ≈ handwritten, then once
/// StaticChain evolves from "batch recursion via Vec staging" to "linear streaming", ε→0 is reachable.
fn diamond_stream(inputs: Vec<i32>) -> Vec<i32> {
    let mut sa: () = ();
    let mut sl: i32 = 0;
    let mut sr: () = ();
    let mut sd: i32 = 0;
    let mut out = Vec::with_capacity(inputs.len());
    for x in inputs {
        // machine A (Doubler)
        let _ = &mut sa;
        let a = x * 2;
        // StraightClone: split
        let (l, r) = (a, a);
        // left arm (Adder)
        sl = sl.wrapping_add(l);
        let lo = sl;
        // right arm (Tripler)
        let _ = &mut sr;
        let ro = r * 3;
        // Sum: merge
        let m = lo.wrapping_add(ro);
        // downstream (Adder)
        sd = sd.wrapping_add(m);
        out.push(sd);
    }
    out
}

/// StaticTopology generic path: executes through the `T: StaticChain` contract surface (a unified static entry).
///
/// Inside the generic body, sources/sinks are fixed by the type system (`T::Head`'s raw input type), with zero
/// physical execution checks. After monomorphization it should generate the **same execution shape** as a direct
/// call to `T::run_all` — this is the observable promise of the zero-cost abstraction (non-invasion axiom): the
/// abstraction layer does not change the execution shape.
fn run_static<T>(inputs: Vec<i32>) -> Vec<T::Out>
where
    T: StaticChain,
    T::Head: StraightMachine<StraightIn = i32>,
{
    T::run_all(inputs).expect("static run")
}

// ── Semantic-equivalence check (correctness gate before benchmarking) ────

fn verify_semantic_equivalence() {
    let src: Vec<i32> = (0..50).collect();
    let via_diamond = DiamondShape::run_all(src.clone()).expect("diamond");
    let via_hand = handwritten(src.clone());
    let via_stream = diamond_stream(src.clone());
    assert_eq!(via_diamond, via_hand, "Diamond must match handwritten semantics");
    assert_eq!(
        via_diamond, via_stream,
        "streaming Diamond must match batch Diamond semantics"
    );
    // S3: the generic-constraint path must match direct calls bit-for-bit (the abstraction does not change semantics).
    assert_eq!(
        via_diamond,
        run_static::<DiamondShape>(src),
        "generic T: StaticChain path must match direct call semantics"
    );
}

// ── Main ─────────────────────────────────────────────────────────────────

fn main() {
    println!("\n═══ Benchmark: dag fusion (Straight, P0) ════════════════════════════════\n");

    verify_semantic_equivalence();

    // a large batch amortizes fixed overhead (init/cleanup) — for the "per-input cost" comparison.
    let src: Vec<i32> = (0..100_000).collect();

    let mut group = BenchGroup::new("diamond_100k");

    group.bench("static_path (empty, init/cleanup only)", || {
        let out = DiamondShape::run_all(vec![]).expect("diamond");
        std::hint::black_box(out);
    });

    group.bench("static_path (Diamond, straight)", || {
        let out = DiamondShape::run_all(src.clone()).expect("diamond");
        std::hint::black_box(out);
    });

    group.bench("static_path generic (T: StaticChain, monomorphized)", || {
        let out = run_static::<DiamondShape>(src.clone());
        std::hint::black_box(out);
    });

    group.bench("handwritten loop", || {
        let out = handwritten(src.clone());
        std::hint::black_box(out);
    });

    group.bench("streaming (flow-through, paradigm)", || {
        let out = diamond_stream(src.clone());
        std::hint::black_box(out);
    });

    group.finish();
}
