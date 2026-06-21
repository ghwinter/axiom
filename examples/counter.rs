/// Counter example: demonstrates Func and Machine working together.
///
/// Run: cargo run --example counter

extern crate axiom;

use axiom::prelude_all::*;
use axiom::runtime::LinearRuntime;

// ── Func: ParseInt ──────────────────────────────────────────

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

// ── Port types for Accumulator ──────────────────────────────

// Manually defined port enums for a single-input, single-output Machine.

#[derive(Debug, Clone, PartialEq)]
pub enum AccInput {
    Input(i32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AccOutput {
    Output((i32, i32)),
}

impl HasPortInfo for AccInput {
    fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<i32>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<i32>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "input" => { let v: Box<i32> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
}

impl HasPortInfo for AccOutput {
    fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<(i32, i32)>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<(i32, i32)>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "output" => { let v: Box<(i32, i32)> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
}

/// PortSet connecting AccInput/AccOutput to a PortSchema.
pub struct AccPorts;

impl PortSet for AccPorts {
    type Input = AccInput;
    type Output = AccOutput;
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<i32>("input"))
            .with(PortDecl::output::<(i32, i32)>("output"))
    }
}

// ── Machine: Accumulator ────────────────────────────────────

struct Accumulator;

#[derive(Default)]
struct AccState { total: i32, count: usize }

impl Machine for Accumulator {
    type State = AccState;
    type Input = AccInput;
    type Output = AccOutput;
    type Ports = AccPorts;

    fn name() -> &'static str { "accumulator" }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<AccState, InitError> {
        Ok(AccState::default())
    }

    fn process(
        state: &mut AccState,
        ctx: &MachineContext,
        input: AccInput,
    ) -> ProcessOutput<AccOutput> {
        match input {
            AccInput::Input(v) => {
                if ctx.observe_is_connected() { /* push observation */ }
                state.total += v;
                state.count += 1;
                ProcessOutput::Yield(AccOutput::Output((state.total, v)))
            }
        }
    }

    fn cleanup(state: AccState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        println!("[accumulator] cleanup: {} values, total={}", state.count, state.total);
        Ok(())
    }
}

// ── Main ────────────────────────────────────────────────────

fn main() {
    println!("═══ axiom: counter example ═══");

    // Stage 1: Func — parse strings → ints
    let raw = vec!["10", "20", "30", "40", "50"];
    let parsed: Vec<i32> = raw.iter()
        .map(|s| ParseInt::call(s).expect("parse failed"))
        .collect();
    println!("[parse_int] {:?}", parsed);

    // Stage 2: Machine — accumulate (wrap ints into port enum)
    let inputs: Vec<AccInput> = parsed.into_iter().map(AccInput::Input).collect();
    let outputs = LinearRuntime::run::<Accumulator>("counter", inputs)
        .expect("linear runtime failed");

    // Unwrap output port values
    let values: Vec<(i32, i32)> = outputs.into_iter().map(|o| match o {
        AccOutput::Output(v) => v,
    }).collect();
    println!("[accumulator] outputs: {:?}", values);
    println!("═══ final: {} ═══", values.last().map(|(t,_)| t).unwrap_or(&0));
}
