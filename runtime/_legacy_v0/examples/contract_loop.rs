//! contract_loop — the deployment guard is a *closed loop*, not a decoration.
//!
//! axiom's design principle §0.4 is "declaration ↔ redemption correspondence":
//! what the runtime *declares* at the contract layer must be *enforced* when
//! physics is created. This example proves that correspondence end-to-end —
//! every check runs through the real `Runtime::materialize`, which validates
//! the blueprint **before** any thread or channel exists:
//!
//! ```text
//!   declaration (contract.rs) ──► validation rule (deploy.rs) ──► materialize guard ──► physics
//!   FlowKind / LinkKind / mode        CycleRule / matrix        validate_deep + check_spec
//! ```
//!
//! Three closed loops are demonstrated:
//!
//! **Loop A — cycle rule is mode-aware.** The *same* non-Moore cycle `a → b → a`:
//! - under `Sequential` the runtime declares `LinkDelay::Zero` → `CycleRule::RequireMoore`
//!   → `materialize` **rejects** the blueprint (a zero-delay cycle is an unbounded
//!   recursion inside one tick);
//! - under `Parallel(2)` it declares `LinkDelay::OneTick` → `CycleRule::AnyDelay`
//!   → `materialize` **accepts** the identical blueprint and the cycle **actually runs**
//!   to a fixed point (real channels break the algebraic loop).
//!
//! The cycle rule *trusts* `MachineInstance::is_moore` to decide safety — so Loop A is only
//! honest if that declaration cannot be lied about. The closing step proves the
//! correspondence: declaring `.moore()` on a type registered via plain `register` (no
//! type-level `Moore` guarantee) is **rejected** by `materialize` with `MooreMismatch` —
//! declaration ↔ implementation, not declaration → assumption. The fused path (`register_fused`)
//! gets the **same** contract via `register_fused_moore`, closing the gap where fused
//! machines could never be honestly declared Moore.
//!
//! **Loop B — the FlowKind×carrier matrix.** A `probe` with an `[Observe]` output feeds a
//! `telemetry` machine with an `[Observe]` input. The *same* topology differs only in the
//! carrier:
//! - a blocking `BoundedBuf` back-pressures the Observe source → `materialize` **rejects**
//!   with `CarrierViolatesSemantics`;
//! - a dropping `Channel(drop=true)` never blocks the source → `materialize` **accepts**
//!   and the sample actually crosses the link and comes back out of `telemetry` as a
//!   terminal output (the observed value is visible in the result).
//!
//! **Loop C — the capability audit.** A topology demanding subprocess execution is rejected
//! by `check_spec` before materialization — the runtime honestly declares it cannot honor
//! that hint, and the guard refuses to deploy it.
//!
//! The point of the exercise: none of these rules lives in a doc comment or a unit-test
//! only. Each is a guard that `materialize` runs on the real blueprint, so a topology
//! that *looks* deployable but violates a declared contract is stopped before physics —
//! and one that satisfies it deploys and runs.
//!
//! Run: cargo run --manifest-path runtime/Cargo.toml --example contract_loop

use axiom::declare_ports;
use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec, ReadPolicy, WritePolicy};
use axiom::machine::{CleanupError, FusedInline, InitError, Machine, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::resource::{MachinePhysicalSpec, RestartPolicy, SubprocessSpec};
use axiom::runtime_contract::RuntimeContract;
use axiom_runtime::{Runtime, RuntimeConfig, RuntimeError};

// ════════════════════════════════════════════════════════════════════════
// Machine A1: Counter — a Data-in/Data-out machine used by the cycle demo.
// (Returns Done once the count reaches a threshold — this is what makes a
// Parallel cycle *terminate* instead of hanging.)
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct CounterPorts {
        input type CounterInput { tick [Data] => i64 }
        output type CounterOutput { val [Data] => i64 }
    }
}

pub struct Counter;

impl Machine for Counter {
    type State = i64;
    type Input = CounterInput;
    type Output = CounterOutput;
    type Ports = CounterPorts;
    type ProcessOutput = SingleOutput<CounterOutput>;

    fn name() -> &'static str { "counter" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i64, InitError> { Ok(0) }
    #[inline]
    fn process(state: &mut i64, _: &MachineContext, input: CounterInput)
        -> SingleOutput<CounterOutput> {
        let CounterInput::tick(n) = input;
        *state += n;
        if *state >= 10 {
            SingleOutput::Done
        } else {
            SingleOutput::Yield(CounterOutput::val(*state))
        }
    }
    fn cleanup(_: i64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Machine A4: Doubler — a pure fused + Moore machine (output depends only on
// the pre-update state / input). It is the minimal witness for the fused-path
// Moore contract channel: registered via `register_fused_moore` it may be
// *honestly* declared `.moore()`; registered via plain `register_fused` it
// would be mis-rejected (the gap this example closes).
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DoublerPorts {
        input type DoublerInput { x [Data] => i64 }
        output type DoublerOutput { y [Data] => i64 }
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
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(_: &mut (), _: &MachineContext, input: DoublerInput)
        -> SingleOutput<DoublerOutput> {
        let DoublerInput::x(n) = input;
        SingleOutput::Yield(DoublerOutput::y(n * 2))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}
impl FusedInline for Doubler {}
impl axiom::machine::Moore for Doubler {}

// ════════════════════════════════════════════════════════════════════════
// Machine B1: Probe — emits an `[Observe]` stream (a state snapshot for
// external consumption; by contract it must not back-pressure its source).
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ProbePorts {
        input type ProbeInput { start [Data] => i64 }
        output type ProbeOutput { samples [Observe] => i64 }
    }
}

pub struct Probe;

impl Machine for Probe {
    type State = u64;
    type Input = ProbeInput;
    type Output = ProbeOutput;
    type Ports = ProbePorts;
    type ProcessOutput = SingleOutput<ProbeOutput>;

    fn name() -> &'static str { "probe" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<u64, InitError> { Ok(0) }
    #[inline]
    fn process(state: &mut u64, _: &MachineContext, input: ProbeInput)
        -> SingleOutput<ProbeOutput> {
        let ProbeInput::start(v) = input;
        *state += 1;
        SingleOutput::Yield(ProbeOutput::samples(v))
    }
    fn cleanup(_: u64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// Machine B2: Telemetry — an `[Observe]`-only machine: it records the
// observation stream and re-exposes the latest sample on an `[Observe]`
// output (no Data/Control edge, so the observed value is provably visible
// as a terminal output instead of vanishing into an unreadable sink).
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct TelemetryPorts {
        input type TelemetryInput { samples [Observe] => i64 }
        output type TelemetryOutput { latest [Observe] => i64 }
    }
}

pub struct Telemetry;

impl Machine for Telemetry {
    type State = Vec<i64>;
    type Input = TelemetryInput;
    type Output = TelemetryOutput;
    type Ports = TelemetryPorts;
    type ProcessOutput = SingleOutput<TelemetryOutput>;

    fn name() -> &'static str { "telemetry" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<Vec<i64>, InitError> { Ok(Vec::new()) }
    #[inline]
    fn process(state: &mut Vec<i64>, _: &MachineContext, input: TelemetryInput)
        -> SingleOutput<TelemetryOutput> {
        let TelemetryInput::samples(v) = input;
        state.push(v);
        SingleOutput::Yield(TelemetryOutput::latest(v))
    }
    fn cleanup(_: Vec<i64>, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// The non-Moore cycle a → b → a — identical blueprint, two runtimes.
// ════════════════════════════════════════════════════════════════════════

fn cycle_spec() -> DynamicTopology {
    DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "counter", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "counter", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("a", "val"), ("b", "tick"),
            LinkKind::Channel { capacity: 8, drop_when_full: false },
        ))
        .with_link(LinkSpec::new(
            ("b", "val"), ("a", "tick"),
            LinkKind::Channel { capacity: 8, drop_when_full: false },
        ))
}

fn blocking_buf(capacity: usize) -> LinkKind {
    LinkKind::BoundedBuf {
        capacity,
        write_policy: WritePolicy::Blocking,
        read_policy: ReadPolicy::Blocking,
    }
}

fn dropping_channel(capacity: usize) -> LinkKind {
    LinkKind::Channel { capacity, drop_when_full: true }
}

/// A probe → telemetry observation edge; the carrier is the only free variable.
fn observe_spec(carrier: LinkKind) -> DynamicTopology {
    DynamicTopology::new()
        .with_machine(MachineInstance::new("probe", "probe", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("telemetry", "telemetry", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("probe", "samples"), ("telemetry", "samples"), carrier))
}

fn is_contract_violation(err: &RuntimeError, contract: &str) -> bool {
    matches!(err, RuntimeError::ContractViolation { contract: c, .. } if *c == contract)
}

// ════════════════════════════════════════════════════════════════════════
// main
// ════════════════════════════════════════════════════════════════════════

fn main() {
    println!("=== contract_loop: the deployment guard is a closed loop ===\n");

    // ── Loop A: the cycle rule is mode-aware ─────────────────────────────
    // The same non-Moore cycle; only the runtime's declared physics differs.
    println!("[Loop A] cycle rule = f(declared link delay)");
    let spec = cycle_spec();

    // A1. Sequential: LinkDelay::Zero → RequireMoore → rejected before physics.
    let mut seq = Runtime::new(RuntimeConfig::sequential());
    seq.register::<Counter>("counter");
    match seq.materialize(&spec) {
        Err(err) if is_contract_violation(&err, "validate_deep") => {
            println!("  Sequential  : reject  — non-Moore cycle, zero-delay driver (RequireMoore): {err}");
        }
        other => panic!("Sequential must reject the cycle, got: {other:?}"),
    }

    // A2. Parallel: LinkDelay::OneTick → AnyDelay → the SAME blueprint deploys and runs.
    let mut par = Runtime::new(RuntimeConfig::parallel(2));
    par.register::<Counter>("counter");
    par.materialize(&spec).expect("Parallel must accept the cycle");
    let results = par
        .tick(vec![("a".to_string(), "tick".to_string(), Box::new(1i64))])
        .expect("tick");
    assert!(results.len() < 100, "Parallel cycle must terminate, got {} terminal outputs", results.len());
    println!("  Parallel(2) : accept  — non-Moore cycle, one-tick-per-hop driver (AnyDelay)");
    println!("                 and it actually runs: {} terminal outputs before quiescing", results.len());

    // A3. The Moore declaration cannot be lied about — closes Loop A's trust.
    // The cycle rule above *relies* on `is_moore` to decide cycle safety. If a
    // deployer could claim Moore on a type that was only `register`ed (no
    // type-level `Moore` guarantee), a false Moore could "break" a real
    // algebraic loop and the rule would be decoration. `register_moore` is the
    // only path that records the type-level truth; `materialize` rejects the lie.
    let liar = DynamicTopology::new()
        .with_machine(
            MachineInstance::new("a", "counter", MachinePhysicalSpec::default())
                .moore(), // ← the lie: declares Moore on a plain-registered type
        )
        .with_machine(MachineInstance::new("b", "counter", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("a", "val"), ("b", "tick"),
            LinkKind::Channel { capacity: 8, drop_when_full: false },
        ));
    let mut liar_rt = Runtime::new(RuntimeConfig::parallel(2));
    liar_rt.register::<Counter>("counter"); // plain register — is_moore() == false
    match liar_rt.materialize(&liar) {
        Err(RuntimeError::MooreMismatch { machine, machine_type }) => {
            println!("  Moore lie   : reject  — `{machine}` declares Moore, but type `{machine_type}`");
            println!("                 was only `register`ed (no register_moore guarantee)");
        }
        other => panic!("a Moore declaration without register_moore must be rejected, got: {other:?}"),
    }

    // A4. The fused path gets its Moore contract channel — the gap, now closed.
    // `Doubler` is a genuine `FusedInline + Moore` machine. Prior to this fix the
    // fused registrar reported `is_moore() == false` unconditionally, so an *honest*
    // `.moore()` declaration on a fused type was mis-rejected as `MooreMismatch`.
    // With `register_fused_moore` now recording the type-level truth:
    //   - registering via plain `register_fused`  → honest `.moore()` is still rejected
    //     (the fixed old-channel behaviour, made explicit rather than silent);
    //   - registering via `register_fused_moore`  → the SAME honest declaration passes.
    let fused_topology = DynamicTopology::new()
        .with_machine(
            MachineInstance::new("d", "doubler", MachinePhysicalSpec::default())
                .moore(), // a truthful declaration
        );

    // 4a. Fused channel WITHOUT the Moore guarantee — the lie (or the gap).
    let mut fused_plain = Runtime::new(RuntimeConfig::parallel(2));
    fused_plain.register_fused::<Doubler>("doubler");
    match fused_plain.materialize(&fused_topology) {
        Err(RuntimeError::MooreMismatch { machine, machine_type }) => {
            println!("  fused channel: reject  — `{machine}` declares Moore on `{machine_type}`,");
            println!("                 but register_fused recorded no Moore guarantee");
        }
        other => panic!("fused non-Moore register must reject an honest Moore declaration, got: {other:?}"),
    }

    // 4b. Fused channel WITH the Moore guarantee — the same declaration now passes.
    let mut fused_moore = Runtime::new(RuntimeConfig::parallel(2));
    fused_moore.register_fused_moore::<Doubler>("doubler");
    fused_moore
        .materialize(&fused_topology)
        .expect("a genuine fused+Moore machine may honestly declare Moore");
    println!("  fused+Moore  : accept  — register_fused_moore records the Moore guarantee,");
    println!("                 so the truthful fused declaration deploys.");

    // ── Loop B: the FlowKind×carrier matrix ──────────────────────────────
    // The same Observe edge; only the carrier differs. The matrix classifies
    // (Observe, carrier) and materialize enforces the classification.
    println!("\n[Loop B] FlowKind×carrier matrix enforced at materialize");

    // B1. Observe on a blocking BoundedBuf → Violates → rejected before physics.
    let mut rt_bad = Runtime::default();
    rt_bad.register::<Probe>("probe");
    rt_bad.register::<Telemetry>("telemetry");
    match rt_bad.materialize(&observe_spec(blocking_buf(8))) {
        Err(err) if is_contract_violation(&err, "validate_deep") => {
            println!("  BoundedBuf(Blocking): reject  — Observe edge must not back-pressure its source: {err}");
        }
        other => panic!("Observe-on-blocking must be rejected, got: {other:?}"),
    }

    // B2. The same Observe edge on a dropping Channel → Recommended → deploys and runs.
    let mut rt_ok = Runtime::default();
    rt_ok.register::<Probe>("probe");
    rt_ok.register::<Telemetry>("telemetry");
    rt_ok.materialize(&observe_spec(dropping_channel(8)))
        .expect("Observe on a dropping channel must deploy");
    let out = rt_ok
        .tick(vec![("probe".to_string(), "start".to_string(), Box::new(7i64))])
        .expect("tick");
    println!("  Channel(drop=true): accept  — sample crosses the link; terminal outputs: {out:?}");
    let seen: Vec<i64> = out
        .iter()
        .filter_map(|r| {
            if let axiom_runtime::ProcessResult::Yield { value, .. } = r {
                value.downcast_ref::<i64>().copied()
            } else {
                None
            }
        })
        .collect();
    assert!(seen.contains(&7), "the observed value 7 must reach telemetry, got {seen:?}");
    println!("                 and the observed value {seen:?} actually arrived at telemetry");

    // ── Loop C: the capability audit ─────────────────────────────────────
    // The runtime honestly declares it cannot spawn subprocesses; check_spec
    // refuses a blueprint that demands one — before any physics exists.
    println!("\n[Loop C] capability audit (check_spec) before materialize");
    let rt = Runtime::default();
    let hostile = DynamicTopology::new().with_machine(MachineInstance::new(
        "worker",
        "probe",
        MachinePhysicalSpec {
            execution: axiom::resource::ExecutionHint::Subprocess(SubprocessSpec {
                executable: "isolated-worker".into(),
                args: vec![],
                restart: RestartPolicy::Never,
            }),
            ..MachinePhysicalSpec::default()
        },
    ));
    let report = rt.check_spec(&hostile, &axiom::compat::HashMap::new());
    assert!(!report.is_ok(), "subprocess execution must be rejected");
    assert!(
        report.violations.iter().any(|v| v.rule_id == "runtime-exec-mode"),
        "expected a runtime-exec-mode violation, got {:?}",
        report.violations,
    );
    println!("  subprocess hint: reject — runtime declares exec_modes.subprocess=false, rule_id={}",
        report.violations[0].rule_id);

    println!("\n✓ closed loop verified: every declaration (LinkDelay, FlowKind×carrier, exec-mode,");
    println!("  is_moore↔implementation) is enforced by materialize/check_spec on the real blueprint");
    println!("  — before physics exists — and every accepted blueprint actually deploys and runs.");
}
