//! http_declarative — declarative acceptance of the `http_tutorial` topology.
//!
//! The same graph (Receiver → Calculator → Persister), but instead of hand-writing a
//! `MachineHandle` drive loop, the graph is handed to `axiom-runtime`:
//!
//! ```text
//!   register the three machine types ─► materialize(DynamicTopology) ─► tick(inputs)
//! ```
//!
//! This example also demonstrates the **deployment guard**: before any physics is
//! created, the topology is audited twice —
//!
//! 1. `DynamicTopology::validate_deep` — port existence, type/flow compatibility,
//!    the FlowKind×carrier materialization matrix, cycle safety, edge-degree
//!    constraints (S3-1/S3-2);
//! 2. `RuntimeContract::check_spec` — the runtime declares its capabilities
//!    (link kinds, backpressure actions, execution modes), and a topology that
//!    demands something the runtime cannot honor is rejected *before* deploy.
//!
//! Verified:
//! 1. `Sequential` mode: 3 injected requests → 3 terminal statuses
//!    (`balance=10/5/8`) — Calculator's `status` observation port has no
//!    downstream, so it is collected as a terminal output; the `balance` data
//!    port is routed to Persister.
//! 2. `Parallel(4)` mode: the same spec yields the same result (R001 determinism).
//! 3. A deliberately incompatible spec (a machine demanding subprocess execution)
//!    is rejected by `check_spec` — the guard is not decorative.
//!
//! Run: cargo run --manifest-path runtime/Cargo.toml --example http_declarative

use axiom::compat::HashMap;
use axiom::declare_ports;
use axiom::deploy::{DynamicTopology, MachineInstance, ValidationReport};
use axiom::link::{LinkKind, LinkSpec};
use axiom::machine::{CleanupError, InitError, Machine, SingleOutput, TupleOutput};
use axiom::port::{ConfigSchema, MachineContext, PortSchema};
use axiom::resource::{MachinePhysicalSpec, RestartPolicy, SubprocessSpec};
use axiom::runtime_contract::RuntimeContract;
use axiom_runtime::{ProcessResult, Runtime, RuntimeConfig};

// ════════════════════════════════════════════════════════════════════════
// Data types (identical to axiom's examples/http_tutorial.rs)
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    delta: i64,
    src: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRequest {
    delta: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    value: i64,
}

// ════════════════════════════════════════════════════════════════════════
// Module 1: Receiver — receives + parses
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ReceiverPorts {
        input type ReceiverInput {
            raw [Data] => RawRequest,
        }
        output type ReceiverOutput {
            parsed [Data] => ParsedRequest,
        }
    }
}

pub struct Receiver;

impl Machine for Receiver {
    type State = u64;
    type Input = ReceiverInput;
    type Output = ReceiverOutput;
    type Ports = ReceiverPorts;
    type ProcessOutput = SingleOutput<ReceiverOutput>;

    fn name() -> &'static str { "receiver" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<u64, InitError> { Ok(0) }
    #[inline]
    fn process(
        state: &mut u64,
        _: &MachineContext,
        input: ReceiverInput,
    ) -> SingleOutput<ReceiverOutput> {
        let ReceiverInput::raw(req) = input;
        *state += 1;
        SingleOutput::Yield(ReceiverOutput::parsed(ParsedRequest { delta: req.delta }))
    }
    fn cleanup(_: u64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 2: Calculator — core logic, state changes with data
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct CalculatorPorts {
        input type CalculatorInput {
            apply [Data] => ParsedRequest,
        }
        output type CalculatorOutput {
            balance [Data]    => Balance,
            status  [Observe] => String,
        }
    }
}

pub struct Calculator;

impl Machine for Calculator {
    type State = i64;
    type Input = CalculatorInput;
    type Output = CalculatorOutput;
    type Ports = CalculatorPorts;
    type ProcessOutput = TupleOutput<CalculatorOutput>;

    fn name() -> &'static str { "calculator" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i64, InitError> { Ok(0) }
    #[inline]
    fn process(
        state: &mut i64,
        _: &MachineContext,
        input: CalculatorInput,
    ) -> TupleOutput<CalculatorOutput> {
        let CalculatorInput::apply(req) = input;
        *state += req.delta;
        TupleOutput::Yield(
            CalculatorOutput::balance(Balance { value: *state }),
            CalculatorOutput::status(format!("balance={}", *state)),
        )
    }
    fn cleanup(_: i64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 3: Persister — persistence (in-memory history)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PersisterPorts {
        input type PersisterInput {
            save [Data] => Balance,
        }
        output type PersisterOutput {
            // pure sink: no output ports
        }
    }
}

pub struct Persister;

impl Machine for Persister {
    type State = Vec<i64>;
    type Input = PersisterInput;
    type Output = PersisterOutput;
    type Ports = PersisterPorts;
    type ProcessOutput = SingleOutput<PersisterOutput>;

    fn name() -> &'static str { "persister" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<Vec<i64>, InitError> { Ok(Vec::new()) }
    #[inline]
    fn process(
        state: &mut Vec<i64>,
        _: &MachineContext,
        input: PersisterInput,
    ) -> SingleOutput<PersisterOutput> {
        let PersisterInput::save(b) = input;
        state.push(b.value);
        SingleOutput::Idle
    }
    fn cleanup(_: Vec<i64>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Topology + drive
// ════════════════════════════════════════════════════════════════════════

fn topology() -> DynamicTopology {
    DynamicTopology::new()
        .with_machine(MachineInstance::new("receiver", "receiver", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("calc", "calculator", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("persist", "persister", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("receiver", "parsed"),
            ("calc", "apply"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("calc", "balance"),
            ("persist", "save"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ))
}

/// Per-machine port schemas — the type information `validate_deep` needs.
fn schemas() -> HashMap<&'static str, PortSchema> {
    let mut m = HashMap::new();
    m.insert("receiver", Receiver::port_schema());
    m.insert("calc", Calculator::port_schema());
    m.insert("persist", Persister::port_schema());
    m
}

/// The deployment guard: audit the topology before creating any physics.
///
/// Returns the guard report so the caller can assert on it.
fn guard(topo: &DynamicTopology, rt: &Runtime) -> ValidationReport {
    let schemas = schemas();
    // S3-1/S3-2: port existence, type/flow compatibility, FlowKind×carrier
    // matrix, cycle safety, edge-degree constraints.
    topo.validate_deep(&schemas).expect("deep validation must pass");
    // RuntimeContract: the runtime declares what it can honor; the topology
    // must fit inside that before materialization.
    let report = rt.check_spec(topo, &schemas);
    assert!(report.is_ok(), "topology incompatible with runtime: {:?}", report.violations);
    report
}

fn run(cfg: RuntimeConfig) -> Vec<String> {
    let mut rt = Runtime::new(cfg);
    rt.register::<Receiver>("receiver");
    rt.register::<Calculator>("calculator");
    rt.register::<Persister>("persister");

    let topo = topology();
    guard(&topo, &rt);
    rt.materialize(&topo).expect("materialize");

    let requests = vec![
        RawRequest { delta: 10, src: "client-1".into() },
        RawRequest { delta: -5, src: "client-2".into() },
        RawRequest { delta: 3, src: "client-1".into() },
    ];
    let inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)> = requests
        .into_iter()
        .map(|r| ("receiver".to_string(), "raw".to_string(), Box::new(r) as Box<dyn core::any::Any + Send>))
        .collect();

    let out = rt.tick(inputs).expect("tick");
    // Terminal outputs = the `status` observation port (no downstream);
    // `balance` is routed to Persister (Idle).
    out.into_iter()
        .filter_map(|r| match r {
            ProcessResult::Yield { value, .. } => value.downcast::<String>().ok().map(|b| *b),
            _ => None,
        })
        .collect()
}

fn main() {
    let seq = run(RuntimeConfig::sequential());
    let par = run(RuntimeConfig::parallel(4));

    println!("sequential: {:?}", seq);
    println!("parallel(4): {:?}", par);

    let expected = vec!["balance=10", "balance=5", "balance=8"];
    assert_eq!(seq, expected, "Sequential must yield the 3 statuses in order");
    assert_eq!(par, expected, "Parallel(4) must yield the same statuses (R001 determinism)");

    // The guard is not decorative: a topology demanding subprocess execution is
    // rejected by check_spec before materialization.
    let rt = Runtime::default();
    let hostile = DynamicTopology::new().with_machine(MachineInstance::new(
        "worker",
        "receiver",
        MachinePhysicalSpec {
            execution: axiom::resource::ExecutionHint::Subprocess(SubprocessSpec {
                executable: "isolated-worker".into(),
                args: vec![],
                restart: RestartPolicy::Never,
            }),
            ..MachinePhysicalSpec::default()
        },
    ));
    let report = rt.check_spec(&hostile, &schemas());
    assert!(!report.is_ok(), "subprocess execution must be rejected");
    assert!(
        report.violations.iter().any(|v| v.rule_id == "runtime-exec-mode"),
        "expected a runtime-exec-mode violation, got {:?}",
        report.violations,
    );

    println!("✓ http_tutorial declared declaratively: Sequential == Parallel, statuses correct");
    println!("✓ deployment guard: validate_deep + check_spec pass for the valid topology, reject the incompatible one");
}
