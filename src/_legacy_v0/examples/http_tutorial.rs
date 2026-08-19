//! axiom's first application: a minimal HTTP server (teaching example).
//!
//! # Graph structure (abstraction layer — semantic topology)
//!
//! ```text
//!      ┌────────────────────┐  parsed   ┌─────────────────────┐  balance  ┌──────────────────┐
//!      │      Receiver      │ (Data)    │     Calculator      │ (Data)   │     Persister    │
//! raw  │  State: u64        │──────────►│  State: i64         │─────────►│  State: Vec<i64> │
//! ────►│  ┌raw [Data]───┐   │           │  ┌apply [Data]───┐  │          │  ┌save [Data]───┐ │
//!      │  └►parsed[Data]┘   │           │  └►balance[Data]┘  │          │  └►(history snapshot)   │ │
//!      └────────────────────┘           │   status[Observe]──┼──┐       └──────────────────┘
//!                                       └─────────────────────┘  │
//!                                                                ▼
//!                                                      [ log / monitoring ]
//!                                                        (observation port)
//!
//! Links (LinkKind): both edges are `BoundedBuf { cap: 16, Blocking }` —
//! declaring the physical semantics of "bounded buffer + backpressure" (deployers can switch to
//! Inline / Channel without changing a single character in the three modules' code).
//! ```
//!
//! # Physical process (handwritten driver = minimal runtime, single-threaded sequential execution)
//!
//! ```text
//!  main thread (the only thread — no locks, no channels; links degenerate to direct function calls)
//!
//!  ┌──────────────────────────────────────────────────────────────────────┐
//!  │ ① Receiver::process(raw)       ← stack frame; State(u64) on heap    │
//!  │ ② Calculator::process(parsed)  ← stack frame; State(i64) heap += δ  │
//!  │ ③ Persister::process(balance)  ← stack frame; State(Vec<i64>) push  │
//!  └──────────────────────────────────────────────────────────────────────┘
//!
//!  Data movement (physical carrier):
//!   RawRequest ─move→ Receiver stack ─move→ ParsedRequest (constructed on stack)
//!   ParsedRequest ─move→ Calculator stack ─move→ Balance (constructed on stack)
//!   Balance ─move→ Persister stack ─push→ Vec<i64> (heap, may realloc)
//!
//!  Note: under a single-threaded driver, a BoundedBuf link "materializes" as a direct move — no buffer,
//!  no locks — identical to the physical expansion of an Inline link (abstraction resolution: one edge
//!  on the graph, physically just a function call). If a future runtime places Receiver and Calculator on
//!  different threads, only then does BoundedBuf materialize as a real ring buffer + locks.
//! ```
//!
//! Each of the three modules is a `Machine`:
//! - **Receiver**: receives raw requests, parses out the operand (counts received; no business state)
//! - **Calculator**: core computation, holds the running balance; state changes with data,
//!   and each change drives downstream persistence through the balance port
//! - **Persister**: persists every balance snapshot (in-memory history; switching to disk only changes this one spot)
//!
//! axiom core ships without a runtime, so this example hand-writes a minimal driver (one handle per
//! Machine, sequential process) — it is the minimal equivalent of the code a future runtime adapter
//! would auto-generate from `DynamicTopology`. The `DynamicTopology` at the end declares exactly the
//! same topology as the driver (pure data, serializable, verifiable).

use axiom::declare_ports;
use axiom::machine::{
    CleanupError, InitError, Machine, MachineHandle, SingleOutput, TupleOutput, Init,
};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::prelude_all::*; // DynamicTopology / MachineInstance / LinkSpec / LinkKind etc.

// ════════════════════════════════════════════════════════════════════════
// Data types — messages that flow between modules
// ════════════════════════════════════════════════════════════════════════

/// Simulates a raw request read from a socket (in the real world this would be a byte stream + parsing).
#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    delta: i64,
    src: String,
}

/// Request after Receiver's parsing: keeps only the operand.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRequest {
    delta: i64,
}

/// Balance snapshot produced by Calculator.
#[derive(Debug, Clone, PartialEq)]
pub struct Balance {
    value: i64,
}

// ════════════════════════════════════════════════════════════════════════
// Module 1: Receiver — receive + parse
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
    type State = u64; // received count
    type Input = ReceiverInput;
    type Output = ReceiverOutput;
    type Ports = ReceiverPorts;
    type ProcessOutput = SingleOutput<ReceiverOutput>; // 1:1 machine

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
        // simulate protocol parsing: drop src, extract only the operand
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
            balance [Data]    => Balance, // data port: drives downstream persistence
            status  [Observe] => String,  // observation port: for log/monitoring
        }
    }
}

pub struct Calculator;

impl Machine for Calculator {
    type State = i64; // running balance — state changes with data
    type Input = CalculatorInput;
    type Output = CalculatorOutput;
    type Ports = CalculatorPorts;
    // one data output + one observation output: fixed dual output (TupleOutput), can enter a fused pipeline
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
        *state += req.delta; // state change
        TupleOutput::Yield(
            CalculatorOutput::balance(Balance { value: *state }),
            CalculatorOutput::status(format!("balance={}", *state)),
        )
    }

    fn cleanup(_: i64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Module 3: Persister — persistence (in-memory history; switching to disk only changes this one spot)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct PersisterPorts {
        input type PersisterInput {
            save [Data] => Balance,
        }
        output type PersisterOutput {
            // no output ports — pure sink
        }
    }
}

pub struct Persister;

impl Machine for Persister {
    type State = Vec<i64>; // history snapshots (simulating disk persistence)
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
        // real implementation: append to a file / write a WAL. Here we use in-memory history.
        state.push(b.value);
        SingleOutput::Idle // sink: no output
    }

    fn cleanup(_: Vec<i64>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Topology declaration (DynamicTopology) — pure data, equivalent to the handwritten driver below
// ════════════════════════════════════════════════════════════════════════

fn declare_topology() -> DynamicTopology {
    DynamicTopology::new()
        .with_machine(MachineInstance::new(
            "receiver", "receiver", MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "calc", "calculator", MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "persist", "persister", MachinePhysicalSpec::default(),
        ))
        .with_link(LinkSpec::new(
            ("receiver", "parsed"),
            ("calc", "apply"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("calc", "balance"),
            ("persist", "save"),
            LinkKind::BoundedBuf {
                capacity: 16,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
}

// ════════════════════════════════════════════════════════════════════════
// main — handwritten minimal driver (prototype of a runtime)
// ════════════════════════════════════════════════════════════════════════

fn main() {
    // Topology declaration: current core has no runtime; it describes "what the graph looks like";
    // a future runtime adapter will materialize the driver code below from it.
    let _spec = declare_topology();

    // Materialize: one handle per Machine (typestate: Init → Running)
    let mut receiver = MachineHandle::<Receiver, Init>::new(MachineContext::new("receiver"))
        .expect("receiver init")
        .start();
    let mut calculator = MachineHandle::<Calculator, Init>::new(MachineContext::new("calc"))
        .expect("calculator init")
        .start();
    let mut persister = MachineHandle::<Persister, Init>::new(MachineContext::new("persist"))
        .expect("persister init")
        .start();

    // simulate an HTTP request stream: /add/10, /add/-5, /add/3
    let requests = vec![
        RawRequest { delta: 10, src: "client-1".into() },
        RawRequest { delta: -5, src: "client-2".into() },
        RawRequest { delta: 3, src: "client-1".into() },
    ];

    for req in requests {
        // Receiver: raw → parsed (the handwritten driver plays the runtime's link delivery)
        let ReceiverOutput::parsed(parsed) = match receiver.process(ReceiverInput::raw(req)) {
            SingleOutput::Yield(o) => o,
            _ => unreachable!(),
        };

        // Calculator: apply → (balance, status)
        let out = calculator.process(CalculatorInput::apply(parsed));
        let (balance, status) = match out {
            TupleOutput::Yield(a, b) => (a, b),
            _ => unreachable!(),
        };

        // status observation port → log; balance data port → Persister
        let s = match status {
            CalculatorOutput::status(s) => s,
            _ => unreachable!(),
        };
        println!("[log] {}", s);

        let b = match balance {
            CalculatorOutput::balance(b) => b,
            _ => unreachable!(),
        };
        let _ = persister.process(PersisterInput::save(b));
    }

    // take the persisted history before graceful shutdown (typestate allows Running to read state)
    let history: Vec<i64> = persister.state().clone();

    // graceful shutdown (typestate: Running → Stopping → Stopped → cleanup)
    let receiver = receiver.stop().finish();
    let calculator = calculator.stop().finish();
    let persister = persister.stop().finish();
    receiver.cleanup().expect("receiver cleanup");
    calculator.cleanup().expect("calculator cleanup");
    persister.cleanup().expect("persister cleanup");

    // verify persisted content
    println!("persisted history: {:?}", history);
    println!("expected          : [10, 5, 8]");
    assert_eq!(history, vec![10, 5, 8]);
}
