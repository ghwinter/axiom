/// Counter example: demonstrates Func and Machine working together.
///
/// Run: cargo run --example counter

extern crate axiom;

use axiom::prelude_all::*;

// ── Inline runtime (minimal, synchronous) ─────────────────

fn run_machine<M: Machine>(
    name: &'static str,
    inputs: Vec<M::Input>,
) -> Vec<M::Output> {
    let ctx = MachineContext::new(name);
    let mut state = M::init(&ctx).expect("init failed");
    let mut outputs = Vec::new();

    for input in inputs {
        match M::process(&mut state, &ctx, input) {
            ProcessOutput::Yield(out) => outputs.push(out),
            ProcessOutput::Idle => {}
            ProcessOutput::Done => break,
        }
    }
    let _ = M::cleanup(state, &ctx);
    outputs
}

// ── Func: ParseInt ─────────────────────────────────────────

struct ParseInt;

impl Func for ParseInt {
    type Input = &'static str;
    type Output = Result<i32, String>;

    fn name() -> &'static str { "parse_int" }

    fn call(input: &'static str) -> Result<i32, String> {
        input.parse::<i32>().map_err(|e| format!("{}: {}", input, e))
    }

    fn cost_estimate() -> CostEstimate { CostEstimate::Cheap }
    fn nondeterministic() -> bool { false }
}

// ── Machine: Accumulator ───────────────────────────────────

struct Accumulator;

#[derive(Default)]
struct AccState { total: i32, count: usize }

impl Machine for Accumulator {
    type State = AccState;
    type Input = i32;
    type Output = (i32, i32);


    fn name() -> &'static str { "accumulator" }

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<i32>("in"))
            .with(PortDecl::output::<(i32, i32)>("out"))
    }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<AccState, InitError> {
        Ok(AccState::default())
    }

    fn process(
        state: &mut AccState,
        ctx: &MachineContext,
        input: i32,
    ) -> ProcessOutput<(i32, i32)> {
        // Skip expensive observe formatting if nobody is watching.
        if ctx.observe_is_connected() {
            // In a real system, push an observation event here.
        }
        state.total += input;
        state.count += 1;
        ProcessOutput::Yield((state.total, input))
    }

    fn cleanup(state: AccState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        println!("[accumulator] cleanup: {} values, total={}", state.count, state.total);
        Ok(())
    }
}

// ── Main ───────────────────────────────────────────────────

fn main() {
    println!("═══ axiom: counter example ═══");

    // Stage 1: Func — parse strings → ints
    let raw = vec!["10", "20", "30", "40", "50"];
    let parsed: Vec<i32> = raw.iter()
        .map(|s| ParseInt::call(s).expect("parse failed"))
        .collect();
    println!("[parse_int] {:?}", parsed);

    // Stage 2: Machine — accumulate
    let outputs = run_machine::<Accumulator>("counter", parsed);
    println!("[accumulator] outputs: {:?}", outputs);
    println!("═══ final: {} ═══", outputs.last().map(|(t,_)| t).unwrap_or(&0));
}
