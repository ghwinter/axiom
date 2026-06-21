/// Pipeline example: two Machines chained together.
///
/// Run: cargo run --example pipeline

extern crate axiom;

use axiom::prelude_all::*;
use axiom::runtime::LinearRuntime;

// ════════════════════════════════════════════════════════════
// Port types for Splitter
// ════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum SplitterInput {
    Input(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SplitterOutput {
    Output((String, String)),
}

impl HasPortInfo for SplitterInput {
    fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<Vec<String>>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<Vec<String>>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "input" => { let v: Box<Vec<String>> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
}

impl HasPortInfo for SplitterOutput {
    fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<(String, String)>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<(String, String)>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "output" => { let v: Box<(String, String)> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
}

/// PortSet for Splitter.
pub struct SplitterPorts;

impl PortSet for SplitterPorts {
    type Input = SplitterInput;
    type Output = SplitterOutput;
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<Vec<String>>("input"))
            .with(PortDecl::output::<(String, String)>("output"))
    }
}

// ════════════════════════════════════════════════════════════
// Port types for Merger
// ════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum MergerInput {
    Input((String, String)),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MergerOutput {
    Output(String),
}

impl HasPortInfo for MergerInput {
    fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<(String, String)>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<(String, String)>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "input" => { let v: Box<(String, String)> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
}

impl HasPortInfo for MergerOutput {
    fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<String>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<String>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "output" => { let v: Box<String> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
}

/// PortSet for Merger.
pub struct MergerPorts;

impl PortSet for MergerPorts {
    type Input = MergerInput;
    type Output = MergerOutput;
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<(String, String)>("input"))
            .with(PortDecl::output::<String>("output"))
    }
}

// ── Machine: Splitter ───────────────────────────────────────

struct Splitter;

#[derive(Default)]
struct SplitState { processed: usize }

impl Machine for Splitter {
    type State = SplitState;
    type Input = SplitterInput;
    type Output = SplitterOutput;
    type Ports = SplitterPorts;

    fn name() -> &'static str { "splitter" }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SplitState, InitError> {
        Ok(SplitState::default())
    }

    fn process(
        state: &mut SplitState,
        _ctx: &MachineContext,
        input: SplitterInput,
    ) -> ProcessOutput<SplitterOutput> {
        match input {
            SplitterInput::Input(v) => {
                state.processed += 1;
                let s = &v[0];
                if let Some(comma) = s.find(',') {
                    let a = s[..comma].trim().to_string();
                    let b = s[comma + 1..].trim().to_string();
                    ProcessOutput::Yield(SplitterOutput::Output((a, b)))
                } else {
                    ProcessOutput::Yield(SplitterOutput::Output((s.clone(), String::new())))
                }
            }
        }
    }

    fn cleanup(state: SplitState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        println!("[splitter] processed {} batches", state.processed);
        Ok(())
    }
}

// ── Machine: Merger ─────────────────────────────────────────

struct Merger;

#[derive(Default)]
struct MergeState { merged: usize }

impl Machine for Merger {
    type State = MergeState;
    type Input = MergerInput;
    type Output = MergerOutput;
    type Ports = MergerPorts;

    fn name() -> &'static str { "merger" }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<MergeState, InitError> { Ok(MergeState::default()) }

    fn process(
        state: &mut MergeState,
        _ctx: &MachineContext,
        input: MergerInput,
    ) -> ProcessOutput<MergerOutput> {
        match input {
            MergerInput::Input(v) => {
                state.merged += 1;
                let result = format!("[{}] {} <-> {}", state.merged, v.0, v.1);
                println!("  [merger] {}", result);
                ProcessOutput::Yield(MergerOutput::Output(result))
            }
        }
    }

    fn cleanup(state: MergeState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        println!("[merger] merged {} items", state.merged);
        Ok(())
    }
}

// ── Main ────────────────────────────────────────────────────

fn main() {
    println!("═══ axiom: pipeline example ═══");

    let batches = vec![
        vec!["alpha, beta".to_string()],
        vec!["gamma, delta".to_string()],
        vec!["epsilon".to_string()],
        vec!["zeta, eta".to_string()],
    ];

    // Run splitter — wrap values in port enum
    let split_inputs: Vec<SplitterInput> = batches.into_iter().map(SplitterInput::Input).collect();
    let split = LinearRuntime::run::<Splitter>("splitter", split_inputs)
        .expect("linear runtime failed");
    println!("[splitter] → {} pairs", split.len());

    // Unwrap splitter output, re-wrap as merger input
    let split_pairs: Vec<(String, String)> = split.into_iter().map(|o| match o {
        SplitterOutput::Output(v) => v,
    }).collect();

    let merge_inputs: Vec<MergerInput> = split_pairs.into_iter().map(MergerInput::Input).collect();
    let merged = LinearRuntime::run::<Merger>("merger", merge_inputs)
        .expect("linear runtime failed");
    println!("═══ {} strings produced ═══", merged.len());
}
