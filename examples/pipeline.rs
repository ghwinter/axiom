/// Pipeline example: two Machines chained together.
///
/// Run: cargo run --example pipeline

extern crate axiom;

use axiom::prelude_all::*;
use axiom::runtime::LinearRuntime;

// ── Machine: Splitter ──────────────────────────────────────

struct Splitter;

#[derive(Default)]
struct SplitState { processed: usize }

impl Machine for Splitter {
    type State = SplitState;
    type Input = Vec<String>;
    type Output = (String, String);


    fn name() -> &'static str { "splitter" }

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<Vec<String>>("in"))
            .with(PortDecl::output::<(String, String)>("out"))
    }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SplitState, InitError> {
        Ok(SplitState::default())
    }

    fn process(
        state: &mut SplitState,
        _ctx: &MachineContext,
        input: Vec<String>,
    ) -> ProcessOutput<(String, String)> {
        state.processed += 1;
        let s = &input[0];
        if let Some(comma) = s.find(',') {
            let a = s[..comma].trim().to_string();
            let b = s[comma + 1..].trim().to_string();
            ProcessOutput::Yield((a, b))
        } else {
            ProcessOutput::Yield((s.clone(), String::new()))
        }
    }

    fn cleanup(state: SplitState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        println!("[splitter] processed {} batches", state.processed);
        Ok(())
    }
}

// ── Machine: Merger ────────────────────────────────────────

struct Merger;

#[derive(Default)]
struct MergeState { merged: usize }

impl Machine for Merger {
    type State = MergeState;
    type Input = (String, String);
    type Output = String;


    fn name() -> &'static str { "merger" }

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<(String, String)>("in"))
            .with(PortDecl::output::<String>("out"))
    }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<MergeState, InitError> { Ok(MergeState::default()) }

    fn process(
        state: &mut MergeState,
        _ctx: &MachineContext,
        input: (String, String),
    ) -> ProcessOutput<String> {
        state.merged += 1;
        let result = format!("[{}] {} <-> {}", state.merged, input.0, input.1);
        println!("  [merger] {}", result);
        ProcessOutput::Yield(result)
    }

    fn cleanup(state: MergeState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        println!("[merger] merged {} items", state.merged);
        Ok(())
    }
}

// ── Main ───────────────────────────────────────────────────

fn main() {
    println!("═══ axiom: pipeline example ═══");

    let batches = vec![
        vec!["alpha, beta".to_string()],
        vec!["gamma, delta".to_string()],
        vec!["epsilon".to_string()],
        vec!["zeta, eta".to_string()],
    ];

    // Run splitter
    let split = LinearRuntime::run::<Splitter>("splitter", batches)
        .expect("linear runtime failed");
    println!("[splitter] → {} pairs", split.len());

    // Pipe into merger
    let merged = LinearRuntime::run::<Merger>("merger", split)
        .expect("linear runtime failed");
    println!("═══ {} strings produced ═══", merged.len());
}
