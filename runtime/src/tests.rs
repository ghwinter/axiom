//! Runtime unit tests — covering configuration, materialization, routing, determinism,
//! shutdown propagation, fan-in, and the B-tier carriers (Overwriting/Latest/NonBlocking).

use crate::*;
use axiom::declare_ports;
use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::{LinkKind, LinkSpec};
use axiom::machine::Machine;
use axiom::port::MachineContext;
use axiom::resource::MachinePhysicalSpec;

declare_ports! {
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
    type ProcessOutput = axiom::machine::SingleOutput<DoublerOutput>;

    fn name() -> &'static str { "doubler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Self::State, axiom::machine::InitError> {
        Ok(())
    }

    fn process(
        _state: &mut Self::State,
        _ctx: &MachineContext,
        input: DoublerInput,
    ) -> Self::ProcessOutput {
        match input {
            DoublerInput::x(n) => axiom::machine::SingleOutput::Yield(DoublerOutput::y(n * 2)),
        }
    }

    fn cleanup(_state: Self::State, _ctx: &MachineContext) -> Result<(), axiom::machine::CleanupError> {
        Ok(())
    }
}

// Doubler's output is a SingleOutput (exactly one output), satisfying FusedInline's type
// constraints — safe to enter a fusion pipeline.
impl axiom::machine::FusedInline for Doubler {}

// Doubler is marked Moore-semantics — used by the S3-2 contract check tests: a machine
// declaring `is_moore` must be registered via `register_moore` (the type-level guarantee of
// `M: Moore`).
impl axiom::machine::Moore for Doubler {}

#[test]
fn runtime_config_defaults_to_sequential() {
    let cfg = RuntimeConfig::default();
    assert_eq!(cfg.mode, ExecMode::Sequential);
    assert_eq!(cfg.max_ticks, Some(1_000_000));
}

#[test]
fn runtime_config_inline_has_no_tick_limit() {
    let cfg = RuntimeConfig::inline();
    assert_eq!(cfg.mode, ExecMode::Inline);
    assert_eq!(cfg.max_ticks, None);
}

#[test]
fn runtime_config_parallel_n_workers() {
    let cfg = RuntimeConfig::parallel(8);
    assert_eq!(cfg.mode, ExecMode::Parallel(8));
}

#[test]
fn runtime_holds_config_and_empty_topology() {
    let rt = Runtime::default();
    assert_eq!(rt.config().mode, ExecMode::Sequential);
    assert!(rt.topology().is_none());
}

#[test]
fn registry_register_and_build() {
    let mut registry = Registry::new();
    registry.register::<Doubler>("doubler");

    let ctx = MachineContext::new("test_doubler");
    let machine = registry.build("doubler", ctx).expect("build");
    assert_eq!(machine.name(), "test_doubler");
}

#[test]
fn runtime_materialize_single_machine() {
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));

    rt.materialize(&spec).expect("materialize");
    assert!(rt.topology().is_some());
    assert_eq!(rt.topology().unwrap().machines.len(), 1);
}

#[test]
fn runtime_materialize_rejects_dangling_port() {
    // validate_endpoint fix: when a link references a nonexistent port, materialization
    // reports DanglingRef (rather than inject silently returning Idle at tick time and
    // swallowing the message).
    // Two machines are used to avoid triggering DynamicTopology::validate's SelfLoop check —
    // this specifically tests the runtime's port-existence validation, not the core's cycle
    // check.
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "nonexistent"), ("d2", "x"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port } if machine == "d1" && port == "nonexistent"),
        "expected DanglingRef for nonexistent port, got {err:?}"
    );
}

#[test]
fn runtime_materialize_rejects_wrong_port_direction() {
    // validate_endpoint fix: the src port must be an output port (PortDir::Out).
    // DoublerInput::x is an input port — it should be rejected as a link's out end.
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "x"), ("d2", "x"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port } if machine == "d1" && port == "x"),
        "expected DanglingRef for input port used as source, got {err:?}"
    );
}

#[test]
fn runtime_materialize_rejects_moore_declared_on_plain_register() {
    // S3-2: `is_moore` declared true, but the type is registered via plain `register` (no
    // Moore guarantee) → declaration mismatches implementation, rejected at deployment time
    // with `MooreMismatch`.
    // (Declaring Moore is only allowed when registered via `register_moore`; see the next
    // test.)
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()).moore());

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::MooreMismatch { ref machine, ref machine_type } if machine == "d1" && machine_type == "doubler"),
        "expected MooreMismatch for Moore-declared machine registered without Moore guarantee, got {err:?}"
    );
}

#[test]
fn runtime_materialize_accepts_moore_declared_on_moore_registered() {
    // S3-2: type registered via `register_moore` (the type-level guarantee of `M: Moore`) +
    // `is_moore` declaration → contract consistent, materialization succeeds.
    let mut rt = Runtime::default();
    rt.register_moore::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()).moore());

    rt.materialize(&spec).expect("materialize");
    assert!(rt.topology().is_some());
    assert_eq!(rt.topology().unwrap().machines.len(), 1);
}

#[test]
fn runtime_materialize_accepts_moore_type_without_declaration() {
    // S3-2: the type implements Moore and is registered via `register_moore`, but the
    // deployment does not declare `is_moore` (conservative declaration) → legal.
    // Over-declaration is the inconsistency; under-declaration is safe.
    let mut rt = Runtime::default();
    rt.register_moore::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));

    rt.materialize(&spec).expect("materialize");
    assert!(rt.topology().is_some());
}

#[test]
fn runtime_tick_processes_input() {
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));

    rt.materialize(&spec).expect("materialize");

    // tick signature: (machine, port, payload) — inject 21 on port x
    let results = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(21i32))])
        .expect("tick");
    assert_eq!(results.len(), 1);
    match &results[0] {
        ProcessResult::Yield { port, value } => {
            assert_eq!(*port, "y");
            let v = value.downcast_ref::<i32>().expect("i32 payload");
            assert_eq!(*v, 42);
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn runtime_routes_output_to_downstream() {
    // Chain topology: d1.y ──► d2.x (Doubler → Doubler)
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));

    rt.materialize(&spec).expect("materialize");

    // Input 3 → d1 yields 6 → routed to d2 → yields 12 (terminal output, no downstream)
    let results = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(results.len(), 1, "exactly one terminal output");
    match &results[0] {
        ProcessResult::Yield { port, value } => {
            assert_eq!(*port, "y", "terminal output on d2's y port");
            let v = value.downcast_ref::<i32>().expect("i32 payload");
            assert_eq!(*v, 12);
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn runtime_routes_fanout_via_tee() {
    // Fan-out topology: source ──► Tee ──┬──► d2
    //                                  └──► d3
    // Uses the built-in Tee<i32> (MultiOutput fan-out) to verify routing of multiple outputs.
    use axiom::builtin::{Tee, TeeInput, TeeOutput};

    struct Src;
    impl Machine for Src {
        type State = ();
        type Input = axiom::portset::In<i32>;
        type Output = axiom::portset::Out<i32>;
        type Ports = axiom::portset::SinglePorts<i32>;
        type ProcessOutput = axiom::machine::SingleOutput<Self::Output>;
        fn name() -> &'static str { "src" }
        fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
        fn process(_: &mut Self::State, _: &MachineContext, input: Self::Input)
            -> Self::ProcessOutput {
            let axiom::portset::In(v) = input;
            axiom::machine::SingleOutput::Yield(axiom::portset::Out(v))
        }
        fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
    }
    impl axiom::machine::FusedInline for Src {}

    let mut rt = Runtime::default();
    rt.register::<Src>("src");
    rt.register::<Tee<i32>>("tee");
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("s", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("t", "tee", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("s", "output"), ("t", "input"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_a"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_b"), ("d3", "x"), LinkKind::Inline));

    rt.materialize(&spec).expect("materialize");

    // Input 5 → src yields 5 → Tee fans out two 5s → d2/d3 each ×2 → two terminal 10s
    let results = rt
        .tick(vec![("s".to_string(), "input".to_string(), Box::new(5i32))])
        .expect("tick");
    assert_eq!(results.len(), 2, "two terminal outputs from fan-out");
    let mut vals: Vec<i32> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    vals.sort();
    assert_eq!(vals, vec![10, 10]);
    // The Tee input port's payload type is TeeInput<i32> (constructed by from_port_name)
    let _ = TeeInput::Input(1i32);
    let _ = TeeOutput::OutputA(1i32);
}

#[test]
fn runtime_parallel_chain_matches_sequential() {
    // The chain topology under Parallel(2) produces the same result as Sequential (3 → 6 → 12).
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));

    let mut seq = Runtime::new(RuntimeConfig::sequential());
    seq.register::<Doubler>("doubler");
    seq.materialize(&spec).expect("materialize");
    let seq_out = seq
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("sequential tick");

    let mut par = Runtime::new(RuntimeConfig::parallel(2));
    par.register::<Doubler>("doubler");
    par.materialize(&spec).expect("materialize");
    let par_out = par
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("parallel tick");

    assert_eq!(seq_out.len(), 1);
    assert_eq!(par_out.len(), 1);
    let sv = match &seq_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    let pv = match &par_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(sv, 12);
    assert_eq!(pv, 12, "Parallel chain must produce the same terminal value");
}

#[test]
fn runtime_parallel_boundedbuf_matches_sequential() {
    // The BoundedBuf chain (capacity=2, Blocking) under Parallel(2) uses the sync_channel
    // blocking-backpressure path; the result must match Sequential (3 → 6 → 12).
    // This locks R001: determinism still holds for bounded carriers — backpressure is a
    // physical parameter, not a semantic one.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::BoundedBuf {
                capacity: 2,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ));

    let run = |cfg: RuntimeConfig| -> i32 {
        let mut rt = Runtime::new(cfg);
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
            .expect("tick");
        match &out[0] {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }
    };

    assert_eq!(run(RuntimeConfig::sequential()), 12);
    assert_eq!(
        run(RuntimeConfig::parallel(2)), 12,
        "BoundedBuf Parallel must match Sequential (sync_channel backpressure is transparent)"
    );
}

#[test]
fn runtime_parallel_channel_drop_matches_sequential() {
    // Channel { capacity=4, drop_when_full=true } uses the sync_channel + try_send path. In
    // a single-message scenario no dropping is triggered, so the result matches Sequential —
    // locking that the Channel carrier's physicalization does not change semantics under
    // normal delivery.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::Channel { capacity: 4, drop_when_full: true },
        ));

    let run = |cfg: RuntimeConfig| -> i32 {
        let mut rt = Runtime::new(cfg);
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
            .expect("tick");
        match &out[0] {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }
    };

    assert_eq!(run(RuntimeConfig::sequential()), 12);
    assert_eq!(run(RuntimeConfig::parallel(2)), 12);
}

#[test]
fn runtime_parallel_fanout_matches_sequential() {
    // The fan-out topology under Parallel(2) produces the same result as Sequential (5 → 10, 10).
    use axiom::builtin::Tee;

    struct Src;
    impl Machine for Src {
        type State = ();
        type Input = axiom::portset::In<i32>;
        type Output = axiom::portset::Out<i32>;
        type Ports = axiom::portset::SinglePorts<i32>;
        type ProcessOutput = axiom::machine::SingleOutput<Self::Output>;
        fn name() -> &'static str { "src" }
        fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
        fn process(_: &mut Self::State, _: &MachineContext, input: Self::Input)
            -> Self::ProcessOutput {
            let axiom::portset::In(v) = input;
            axiom::machine::SingleOutput::Yield(axiom::portset::Out(v))
        }
        fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
    }
    impl axiom::machine::FusedInline for Src {}

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("s", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("t", "tee", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("s", "output"), ("t", "input"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_a"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_b"), ("d3", "x"), LinkKind::Inline));

    let run = |cfg: RuntimeConfig| -> Vec<i32> {
        let mut rt = Runtime::new(cfg);
        rt.register::<Src>("src");
        rt.register::<Tee<i32>>("tee");
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("s".to_string(), "input".to_string(), Box::new(5i32))])
            .expect("tick");
        let mut vals: Vec<i32> = out.iter().map(|r| match r {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }).collect();
        vals.sort();
        vals
    };

    assert_eq!(run(RuntimeConfig::sequential()), vec![10, 10]);
    assert_eq!(run(RuntimeConfig::parallel(2)), vec![10, 10], "Parallel fan-out must match");
}

#[test]
fn runtime_done_stops_machine_sequential() {
    // A1: Done = shutdown signal — once a machine returns Done it no longer receives new
    // inputs (backlog dropped).
    // Stopper returns Done on its 2nd process; inject 3 messages → output [1] (1st yields),
    // the 2nd Done stops the machine, the 3rd is dropped. Without shutdown (old behavior) the
    // output would be [1, 2].
    use axiom::machine::{CleanupError, InitError, SingleOutput};
    use axiom::port::ConfigSchema;

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct StopperPorts {
            input type StopperInput { x [Data] => i64 }
            output type StopperOutput { y [Data] => i64 }
        }
    }

    pub struct Stopper;
    impl Machine for Stopper {
        type State = u64;
        type Input = StopperInput;
        type Output = StopperOutput;
        type Ports = StopperPorts;
        type ProcessOutput = SingleOutput<StopperOutput>;
        fn name() -> &'static str { "stopper" }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<u64, InitError> { Ok(0) }
        #[inline]
        fn process(state: &mut u64, _: &MachineContext, input: StopperInput)
            -> SingleOutput<StopperOutput> {
            let StopperInput::x(n) = input;
            *state += 1;
            if *state >= 2 {
                // Done machines go through the `unified` conversion — here the unified type is
                // constructed directly.
                let _ = n;
                SingleOutput::Done
            } else {
                SingleOutput::Yield(StopperOutput::y(n))
            }
        }
        fn cleanup(_: u64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Stopper>("stopper");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("s", "stopper", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let results = rt
        .tick(vec![
            ("s".to_string(), "x".to_string(), Box::new(10i64)),
            ("s".to_string(), "x".to_string(), Box::new(20i64)),
            ("s".to_string(), "x".to_string(), Box::new(30i64)),
        ])
        .expect("tick");

    // Shutdown in effect: only the 1st message produces output; the 2nd returns Done, the 3rd
    // is dropped.
    let vals: Vec<i64> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i64>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    assert_eq!(vals, vec![10], "Done must stop the machine; backlog dropped");
}

#[test]
fn runtime_done_stops_machine_parallel() {
    // A1 in Parallel form: the thread exits immediately upon receiving Done, no longer
    // processing the backlog.
    use axiom::machine::{CleanupError, InitError, SingleOutput};
    use axiom::port::ConfigSchema;

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct StopperPorts {
            input type StopperInput { x [Data] => i64 }
            output type StopperOutput { y [Data] => i64 }
        }
    }

    pub struct Stopper;
    impl Machine for Stopper {
        type State = u64;
        type Input = StopperInput;
        type Output = StopperOutput;
        type Ports = StopperPorts;
        type ProcessOutput = SingleOutput<StopperOutput>;
        fn name() -> &'static str { "stopper" }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<u64, InitError> { Ok(0) }
        #[inline]
        fn process(state: &mut u64, _: &MachineContext, input: StopperInput)
            -> SingleOutput<StopperOutput> {
            let StopperInput::x(n) = input;
            *state += 1;
            if *state >= 2 {
                SingleOutput::Done
            } else {
                SingleOutput::Yield(StopperOutput::y(n))
            }
        }
        fn cleanup(_: u64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }

    let mut rt = Runtime::new(RuntimeConfig::parallel(2));
    rt.register::<Stopper>("stopper");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("s", "stopper", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let results = rt
        .tick(vec![
            ("s".to_string(), "x".to_string(), Box::new(10i64)),
            ("s".to_string(), "x".to_string(), Box::new(20i64)),
            ("s".to_string(), "x".to_string(), Box::new(30i64)),
        ])
        .expect("tick");

    let vals: Vec<i64> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i64>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    assert_eq!(vals, vec![10], "Parallel: Done must exit the thread; backlog dropped");
}

#[test]
fn runtime_fanin_merges_multi_source_parallel() {
    // A2: fan-in — two entry machines (d1, d2) merge into the same Consumer (Doubler),
    // consumed via forward-thread merging under Parallel.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("c", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("c", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("c", "x"), LinkKind::Inline));

    let run = |cfg: RuntimeConfig| -> Vec<i32> {
        let mut rt = Runtime::new(cfg);
        rt.register::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![
                ("d1".to_string(), "x".to_string(), Box::new(3i32)),
                ("d2".to_string(), "x".to_string(), Box::new(5i32)),
            ])
            .expect("tick");
        let mut vals: Vec<i32> = out.iter().map(|r| match r {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }).collect();
        vals.sort();
        vals
    };

    // Both Sequential (BFS merges naturally) and Parallel (forward-thread merging) converge
    // to {12, 20} (3→6→12, 5→10→20: c is a Doubler, doubling again).
    let seq = run(RuntimeConfig::sequential());
    let par = run(RuntimeConfig::parallel(2));
    assert_eq!(seq, vec![12, 20]);
    assert_eq!(par, vec![12, 20], "fan-in must merge in Parallel too");
}

#[test]
fn runtime_shutdown_cleans_up() {
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));

    rt.materialize(&spec).expect("materialize");
    rt.shutdown().expect("shutdown");
    assert!(rt.topology().is_none());
}

#[test]
fn runtime_parallel_nonblocking_read_policy() {
    // B-tier: ReadPolicy::NonBlocking — the machine thread polls with try_recv + yield (does
    // not block the thread) and exits on disconnection (cascade shutdown). Functionally
    // equivalent to Blocking.
    use axiom::link::ReadPolicy;
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: ReadPolicy::NonBlocking,
            },
        ));

    let mut rt = Runtime::new(RuntimeConfig::parallel(2));
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");
    let out = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");

    let vals: Vec<i32> = out.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();
    assert_eq!(vals, vec![12], "NonBlocking polling must deliver the same result");
}

// ════════════════════════════════════════════════════════════════════════════
// pipelineN fusion tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn fusion_fused_chain_matches_non_fused_result() {
    // The tick result of the fused chain d1→d2→d3 (all FusedInline + Inline links) must
    // match the non-fused result (3 → 6 → 12 → 24).
    // Registered via register_fused — materialize fuses d1, d2, d3 into a single
    // FusedPipeline.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("d3", "x"), LinkKind::Inline));

    // Fused path
    let mut rt_fused = Runtime::new(RuntimeConfig::sequential());
    rt_fused.register_fused::<Doubler>("doubler");
    rt_fused.materialize(&spec).expect("materialize fused");
    let fused_out = rt_fused
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick fused");

    // Non-fused path (register, not register_fused)
    let mut rt_plain = Runtime::new(RuntimeConfig::sequential());
    rt_plain.register::<Doubler>("doubler");
    rt_plain.materialize(&spec).expect("materialize plain");
    let plain_out = rt_plain
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick plain");

    assert_eq!(fused_out.len(), 1);
    assert_eq!(plain_out.len(), 1);
    let fv = match &fused_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    let pv = match &plain_out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(pv, 24, "non-fused 3-hop chain: 3→6→12→24");
    assert_eq!(fv, 24, "fused chain must produce same result");
}

#[test]
fn fusion_reduces_machine_count() {
    // The machine count in the fused topology should decrease (3 machines → 1 FusedPipeline).
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("d3", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 1, "3-stage chain must fuse to 1 machine");
    assert_eq!(topo.links.len(), 0, "internal links absorbed by FusedPipeline");
    assert_eq!(topo.topo_order.len(), 1, "topo_order reduced to chain head");
}

#[test]
fn fusion_does_not_trigger_for_non_fused_register() {
    // Machines registered via register (not register_fused) are not fused —
    // is_fused_compatible() returns false.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 2, "non-fused register must not fuse");
    assert_eq!(topo.links.len(), 1, "link preserved");
}

#[test]
fn fusion_does_not_trigger_for_bounded_buf_link() {
    // A BoundedBuf link is not a fusion candidate — even if both endpoint machines are
    // fusable.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"), ("d2", "x"),
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 2, "BoundedBuf link must not fuse");
}

#[test]
fn fusion_partial_chain_only_fuses_fused_inline_segment() {
    // Mixed chain: d1(FusedInline) → Inline → d2(FusedInline) → BoundedBuf → d3(FusedInline)
    // Only d1→d2 fuses (Inline + both ends fusable); d2→d3 is a BoundedBuf, not fused.
    // After fusion: 1 FusedPipeline(d1,d2) + 1 standalone d3, 1 BoundedBuf link.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(
            ("d2", "y"), ("d3", "x"),
            LinkKind::BoundedBuf {
                capacity: 4,
                write_policy: axiom::link::WritePolicy::Blocking,
                read_policy: axiom::link::ReadPolicy::Blocking,
            },
        ));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 2, "d1+d2 fused, d3 standalone");
    assert_eq!(topo.links.len(), 1, "only BoundedBuf link remains");
    // Result check: 3 → 6(d1) → 12(d2) → 24(d3)
    let out = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 24);
}

#[test]
fn fusion_parallel_matches_sequential() {
    // The fused chain under Parallel produces the same result as Sequential (R001
    // determinism still holds for fusion).
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("d2", "y"), ("d3", "x"), LinkKind::Inline));

    let run = |cfg: RuntimeConfig| -> i32 {
        let mut rt = Runtime::new(cfg);
        rt.register_fused::<Doubler>("doubler");
        rt.materialize(&spec).expect("materialize");
        let out = rt
            .tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))])
            .expect("tick");
        match &out[0] {
            ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
            other => panic!("expected Yield, got {other:?}"),
        }
    };

    assert_eq!(run(RuntimeConfig::sequential()), 24);
    assert_eq!(run(RuntimeConfig::parallel(2)), 24, "fused Parallel must match Sequential");
}

#[test]
fn fusion_fanout_not_fused() {
    // The fan-out topology (Tee) does not meet the fusion conditions — Tee's MultiOutput does
    // not implement FusedInline.
    // Doubler is registered via register_fused (fusable), but Tee via register (not fusable).
    // d1(FusedInline) → Inline → tee(non-FusedInline) → no fusion on fan-out.
    use axiom::builtin::Tee;

    struct Src;
    impl Machine for Src {
        type State = ();
        type Input = axiom::portset::In<i32>;
        type Output = axiom::portset::Out<i32>;
        type Ports = axiom::portset::SinglePorts<i32>;
        type ProcessOutput = axiom::machine::SingleOutput<Self::Output>;
        fn name() -> &'static str { "src" }
        fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
        fn process(_: &mut Self::State, _: &MachineContext, input: Self::Input)
            -> Self::ProcessOutput {
            let axiom::portset::In(v) = input;
            axiom::machine::SingleOutput::Yield(axiom::portset::Out(v))
        }
        fn cleanup(_: Self::State, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
    }
    impl axiom::machine::FusedInline for Src {}

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("s", "src", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("t", "tee", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d3", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("s", "output"), ("t", "input"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_a"), ("d2", "x"), LinkKind::Inline))
        .with_link(LinkSpec::new(("t", "output_b"), ("d3", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Src>("src");
    rt.register::<Tee<i32>>("tee"); // Tee does not implement FusedInline
    rt.register_fused::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    // Tee's fan-out prevents fusion — all 4 machines stay independent.
    assert_eq!(topo.machines.len(), 4, "fan-out via Tee must not fuse");
    assert_eq!(topo.links.len(), 3, "all links preserved");
}

// ════════════════════════════════════════════════════════════════════════════
// Parallel cyclic topology tests
// ════════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct CounterPorts {
        input type CounterInput { tick [Data] => i64 }
        output type CounterOutput { val [Data] => i64 }
    }
}

/// Counter machine: increments a count on each process and returns Done once the threshold
/// (hardcoded 10) is reached. Used for cyclic-topology tests — machines in a cycle trigger
/// global shutdown via Done.
/// (The threshold is not configured via a struct field: Machine::init only accepts a
/// MachineContext, with no path for injected constructor parameters; a field would become dead
/// code, so it is inlined directly in process.)
pub struct Counter;

impl Machine for Counter {
    type State = i64;
    type Input = CounterInput;
    type Output = CounterOutput;
    type Ports = CounterPorts;
    type ProcessOutput = axiom::machine::SingleOutput<CounterOutput>;

    fn name() -> &'static str { "counter" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i64, axiom::machine::InitError> { Ok(0) }
    #[inline]
    fn process(state: &mut i64, _: &MachineContext, input: CounterInput)
        -> axiom::machine::SingleOutput<CounterOutput> {
        let CounterInput::tick(n) = input;
        *state += n;
        if *state >= 10 {
            axiom::machine::SingleOutput::Done
        } else {
            axiom::machine::SingleOutput::Yield(CounterOutput::val(*state))
        }
    }
    fn cleanup(_: i64, _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}

#[test]
fn runtime_parallel_cycle_terminates_via_done() {
    // Cyclic topology: a → b → a (a self-sustaining feedback loop).
    // a and b feed each other until a's count >= 10 returns Done → global shutdown.
    // Without the stop_signal path this test would hang (deadlock).
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "counter", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "counter", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "val"), ("b", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }))
        .with_link(LinkSpec::new(("b", "val"), ("a", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }));

    let mut rt = Runtime::new(RuntimeConfig::parallel(2));
    rt.register::<Counter>("counter");
    rt.materialize(&spec).expect("materialize");

    // Inject initial value 1 → a counts 1 → b counts 1 → a counts 2 → ... → reaches 10 Done.
    let results = rt
        .tick(vec![("a".to_string(), "tick".to_string(), Box::new(1i64))])
        .expect("tick");

    // Cycle machines' outputs are either routed to the other (non-terminal) or dropped on
    // Done. Terminal outputs = the last few vals before Done (collected when there is no
    // downstream route).
    // Since a and b route to each other, terminal outputs may be empty or few — the key point
    // is that tick does not hang.
    println!("cycle test: {} terminal outputs", results.len());
}

#[test]
fn runtime_parallel_cycle_terminates_via_tick_limit() {
    // Cyclic topology + machines without Done — terminated by max_ticks.
    // Doubler never returns Done, so the cycle would run forever — max_ticks protects it.
    //
    // Value constraints: Doubler doubles the value each hop, so i32 overflows at ~16 hops
    // (2^31 > i32::MAX). max_ticks is a **per-thread** counter (d1/d2 in the cycle are
    // independent), and d1/d2 alternate serially through channels (d1's k-th hop needs d2's
    // (k-1)-th output), so with max_ticks=10 each thread injects at most 10 times — the max
    // is d2's 10th-hop output = 4^10 ≈ 1e6, well within the i32 range. Verifies that "without
    // Done, max_ticks drives stop_signal to terminate the cycle".
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Channel { capacity: 4, drop_when_full: true }))
        .with_link(LinkSpec::new(("d2", "y"), ("d1", "x"), LinkKind::Channel { capacity: 4, drop_when_full: true }));

    let mut rt = Runtime::new(RuntimeConfig {
        mode: ExecMode::Parallel(2),
        max_ticks: Some(10),
        max_messages_per_machine: None,
    });
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    // Inject 1 → d1 yields 2 → d2 yields 4 → d1 yields 8 → ... until max_ticks.
    // Key point: no hang (the max_ticks limit triggers stop_signal).
    let _ = rt
        .tick(vec![("d1".to_string(), "x".to_string(), Box::new(1i32))])
        .expect("tick");
    // Reaching here means tick did not hang — the test passes.
}

#[test]
fn runtime_parallel_cycle_matches_sequential() {
    // The cyclic topology must terminate under both Sequential and Parallel (Sequential via
    // max_ticks, Parallel via stop_signal). Verifies both produce a result.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("a", "counter", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("b", "counter", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("a", "val"), ("b", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }))
        .with_link(LinkSpec::new(("b", "val"), ("a", "tick"), LinkKind::Channel { capacity: 8, drop_when_full: false }));

    let run = |cfg: RuntimeConfig| -> usize {
        let mut rt = Runtime::new(cfg);
        rt.register::<Counter>("counter");
        rt.materialize(&spec).expect("materialize");
        rt.tick(vec![("a".to_string(), "tick".to_string(), Box::new(1i64))])
            .expect("tick").len()
    };

    // Neither hangs — the key check. The result lengths may differ (Sequential BFS order vs
    // Parallel thread interleaving), but both must terminate.
    let seq_len = run(RuntimeConfig::sequential());
    let par_len = run(RuntimeConfig::parallel(2));
    // Both must be finite (no hang).
    assert!(seq_len < 100, "Sequential cycle must terminate");
    assert!(par_len < 100, "Parallel cycle must terminate");
}

// ════════════════════════════════════════════════════════════════════════════
// IO multiplexing integration tests
// ════════════════════════════════════════════════════════════════════════════

use crate::io::{IoEvent, IoInterest, IoReactor, IoToken, ManualReactor};
use core::time::Duration;

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct IoHandlerPorts {
        input type IoHandlerInput { ready [Data] => IoEvent }
        output type IoHandlerOutput { result [Data] => i64 }
    }
}

/// IO-readiness handling machine: on receiving an `IoEvent` input, it yields a numeric tag by
/// readiness (readable=1, writable=2, both=3). Used to verify run_io routes reactor
/// readiness events correctly to a machine's input port.
pub struct IoHandler;

impl Machine for IoHandler {
    type State = ();
    type Input = IoHandlerInput;
    type Output = IoHandlerOutput;
    type Ports = IoHandlerPorts;
    type ProcessOutput = axiom::machine::SingleOutput<IoHandlerOutput>;

    fn name() -> &'static str { "io_handler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
    #[inline]
    fn process(_: &mut (), _: &MachineContext, input: IoHandlerInput)
        -> axiom::machine::SingleOutput<IoHandlerOutput> {
        let IoHandlerInput::ready(event) = input;
        let mut v: i64 = 0;
        if event.readiness.is_readable() { v += 1; }
        if event.readiness.is_writable() { v += 2; }
        axiom::machine::SingleOutput::Yield(IoHandlerOutput::result(v))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}

#[test]
fn io_manual_reactor_routes_event_to_machine() {
    // ManualReactor pre-injects one READABLE event → run_io polls the event → routes by token
    // to machine "h"'s "ready" port → process yields result(1).
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register_io");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "one terminal output from IoHandler");
    match &results[0] {
        ProcessResult::Yield { value, .. } => {
            let v = value.downcast_ref::<i64>().expect("i64 payload");
            assert_eq!(*v, 1, "readable event → result(1)");
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn io_manual_reactor_routes_multiple_events() {
    // One event per token — verifies multiple events route to different machines.
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h1", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("h2", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h1", "ready", 0, IoInterest::READ_WRITE)
        .expect("register h1");
    rt.register_io(&mut reactor, IoToken(1), "h2", "ready", 1, IoInterest::WRITABLE)
        .expect("register h2");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READ_WRITE });
    reactor.push_event(IoEvent { token: IoToken(1), readiness: IoInterest::WRITABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 2, "two terminal outputs");
    let mut vals: Vec<i64> = results.iter().map(|r| match r {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i64>().unwrap(),
        _ => panic!("expected Yield"),
    }).collect();
    vals.sort();
    assert_eq!(vals, vec![2, 3], "READ_WRITE→3, WRITABLE→2");
}

#[test]
fn io_unregistered_token_event_is_dropped() {
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    // token 999 is unregistered — the event should be dropped.
    reactor.push_event(IoEvent { token: IoToken(999), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 0, "unregistered token event dropped");
}

#[test]
fn io_deregister_removes_routing() {
    // After deregister_io, this token's events are no longer routed.
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");
    rt.deregister_io(&mut reactor, 0, IoToken(0)).expect("deregister");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 0, "deregistered token event dropped");
}

#[test]
fn io_run_io_merges_external_inputs() {
    // run_io injects external inputs + IO events together — both must be processed.
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    rt.register::<Doubler>("doubler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d", "doubler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    // External input: doubler receives 5 → yields 10
    let external = vec![("d".to_string(), "x".to_string(), Box::new(5i32) as Box<dyn core::any::Any + Send>)];
    let results = rt
        .run_io(&mut reactor, external, Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 2, "one IO output + one external output");
}

#[cfg(target_os = "windows")]
#[test]
fn io_wsa_reactor_detects_tcp_readability() {
    // Real WSA reactor: TCP listener registered READABLE → client connects → poll detects
    // READABLE (FD_ACCEPT) → IoEvent yielded.
    use std::net::{TcpListener, TcpStream};
    use std::os::windows::io::AsRawSocket;
    use crate::io::wsa::WsaReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("addr");
    let raw = listener.as_raw_socket();

    let mut reactor = WsaReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::READABLE, IoToken(42))
        .expect("register");

    // Poll before connecting should have no events (timeout=0, non-blocking).
    let no_events = reactor.poll(Some(Duration::from_millis(0))).expect("poll empty");
    assert!(no_events.is_empty(), "no events before connection");

    // Client connects → listener can accept → READABLE ready.
    let _client = TcpStream::connect(addr).expect("connect");

    // Poll waiting for readiness (give the OS a moment to propagate the event).
    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "should detect readable after connect");
    let found = events.iter().any(|e| e.token == IoToken(42) && e.readiness.is_readable());
    assert!(found, "token 42 readable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

// ════════════════════════════════════════════════════════════════════════════
// Composite Machine tests
// ════════════════════════════════════════════════════════════════════════════

/// Build a "doubler_pair" composite definition: internally d1 --Inline--> d2,
/// external ports "in" → d1.x and "out" → d2.y. Two hops of Doubler = ×4.
fn doubler_pair_composite() -> CompositeSpec {
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));
    CompositeSpec::new(spec)
        .with_input("in", "d1", "x")
        .with_output("out", "d2", "y")
}

#[test]
fn composite_single_layer_expands_and_routes() {
    // input_map redirection: entry.y → comp.in is rewritten to entry.y → comp.d1.x
    // Topology: entry(Doubler) --Inline--> comp(DoublerPair)
    // After expansion: entry --Inline--> comp.d1 --Inline--> comp.d2
    // tick: 3 → 6(entry) → 12(comp.d1) → 24(comp.d2) → terminal output 24
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 3, "entry + comp.d1 + comp.d2");
    assert_eq!(topo.links.len(), 2, "entry→comp.d1 + comp.d1→comp.d2");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(out.len(), 1);
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 24, "3→6→12→24");
}

#[test]
fn composite_output_redirect_to_downstream() {
    // output_map redirection: comp.out → sink.x is rewritten to comp.d2.y → sink.x
    // Topology: entry → comp → sink
    // After expansion: entry → comp.d1 → comp.d2 → sink
    // tick: 3 → 6 → 12 → 24 → 48
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("sink", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline))
        .with_link(LinkSpec::new(("comp", "out"), ("sink", "x"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 4, "entry + comp.d1 + comp.d2 + sink");
    assert_eq!(topo.links.len(), 3, "entry→comp.d1 + comp.d1→comp.d2 + comp.d2→sink");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(out.len(), 1);
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 48, "3→6→12→24→48");
}

#[test]
fn composite_nested_recursive_expansion() {
    // Nested composite: quad = pair1 --Inline--> pair2, where pair is itself a composite.
    // After expansion: quad.p1.d1 → quad.p1.d2 → quad.p2.d1 → quad.p2.d2
    // External: entry → quad
    // Full chain: entry → quad.p1.d1 → quad.p1.d2 → quad.p2.d1 → quad.p2.d2
    // 5 Doublers (×2^5=×32), 3 × 32 = 96
    let quad_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("p1", "doubler_pair", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("p2", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("p1", "out"), ("p2", "in"), LinkKind::Inline));
    let quad_comp = CompositeSpec::new(quad_spec)
        .with_input("in", "p1", "in")
        .with_output("out", "p2", "out");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("quad", "doubler_quad", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("quad", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.register_composite("doubler_quad", quad_comp);
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 5, "entry + 4 sub doublers");
    assert_eq!(topo.links.len(), 4, "entry→p1.d1 + 3 internal links");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    assert_eq!(out.len(), 1);
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 96, "3 × 2^5 = 96");
}

#[test]
fn composite_fusion_crosses_boundary() {
    // Fusion across a composite boundary: Doubler registered via register_fused; both the
    // internal d1→d2 and the external entry→comp.d1 are Inline + FusedInline → after
    // expansion the 3 machines fuse into a single FusedPipeline (machine count 1).
    // This verifies expansion happens before fusion — composite boundaries are transparent
    // to fusion.
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register_fused::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());
    rt.materialize(&spec).expect("materialize");

    let topo = rt.topology().expect("topology");
    assert_eq!(topo.machines.len(), 1, "entry + comp.d1 + comp.d2 fuse to 1");
    assert_eq!(topo.links.len(), 0, "all links absorbed by FusedPipeline");

    let out = rt
        .tick(vec![("entry".to_string(), "x".to_string(), Box::new(3i32))])
        .expect("tick");
    let v = match &out[0] {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().unwrap(),
        other => panic!("expected Yield, got {other:?}"),
    };
    assert_eq!(v, 24, "3→6→12→24");
}

// ════════════════════════════════════════════════════════════════════════════
// IO boundary and error-path tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn io_empty_poll_returns_empty() {
    // ManualReactor with no preloaded events returns an empty Vec from poll — verifies
    // empty-reactor behavior.
    let mut reactor = ManualReactor::new();
    let events = reactor.poll(Some(Duration::from_millis(100))).expect("poll");
    assert!(events.is_empty(), "no pending events → empty poll");
}

#[test]
fn io_reregister_updates_routing() {
    // After register token0→(h1, ready), reregister token0→(h2, ready); push a token0 event
    // → run_io routes to h2 (io_routing has been overwritten).
    // Both h1 and h2 would yield result(1), but reregister guarantees only one machine
    // receives the event (result count = 1, not 0 or 2).
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h1", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("h2", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h1", "ready", 0, IoInterest::READABLE)
        .expect("register");
    rt.reregister_io(&mut reactor, IoToken(0), "h2", "ready", 0, IoInterest::READABLE)
        .expect("reregister");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "reregister kept routing active (exactly 1 target)");
    match &results[0] {
        ProcessResult::Yield { value, .. } => {
            let v = value.downcast_ref::<i64>().expect("i64");
            assert_eq!(*v, 1, "READABLE → result(1)");
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn io_deregister_then_event_ignored() {
    // register token0→h1 + token1→h2, deregister token0, push events for both tokens →
    // only h2 responds (the token0 event is dropped).
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h1", "io_handler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("h2", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h1", "ready", 0, IoInterest::READABLE)
        .expect("register h1");
    rt.register_io(&mut reactor, IoToken(1), "h2", "ready", 1, IoInterest::READABLE)
        .expect("register h2");
    rt.deregister_io(&mut reactor, 0, IoToken(0)).expect("deregister h1");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });
    reactor.push_event(IoEvent { token: IoToken(1), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "only h2 responds (token0 deregistered)");
}

#[test]
fn io_multiple_events_same_token() {
    // Push 3 events with the same token → run_io injects 3 times into the same machine → 3
    // outputs.
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });
    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });
    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 3, "3 events same token → 3 outputs");
    for r in &results {
        match r {
            ProcessResult::Yield { value, .. } => {
                let v = value.downcast_ref::<i64>().expect("i64");
                assert_eq!(*v, 1, "each READABLE → result(1)");
            }
            other => panic!("expected Yield, got {other:?}"),
        }
    }
}

#[test]
fn io_read_write_interest_both() {
    // Push one READ_WRITE event → IoHandler yields result(3) (readable +1 + writable +2).
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READ_WRITE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READ_WRITE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(100)))
        .expect("run_io");
    assert_eq!(results.len(), 1);
    match &results[0] {
        ProcessResult::Yield { value, .. } => {
            let v = value.downcast_ref::<i64>().expect("i64");
            assert_eq!(*v, 3, "READ_WRITE → readable(+1) + writable(+2) = 3");
        }
        other => panic!("expected Yield, got {other:?}"),
    }
}

#[test]
fn io_run_io_timeout_returns_partial() {
    // ManualReactor has 1 pending event; run_io with timeout=0ms still returns it —
    // ManualReactor does not actually sleep, so timeout=0 does not skip already-ready events.
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("h", "io_handler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    rt.register_io(&mut reactor, IoToken(0), "h", "ready", 0, IoInterest::READABLE)
        .expect("register");

    reactor.push_event(IoEvent { token: IoToken(0), readiness: IoInterest::READABLE });

    let results = rt
        .run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0)))
        .expect("run_io");
    assert_eq!(results.len(), 1, "timeout=0 still returns pending event");
}

// ════════════════════════════════════════════════════════════════════════════
// Composite Machine error-path tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn composite_too_deep_reports_error() {
    // Self-referencing composite: the "loop" composite's sub-topology contains a machine
    // instance of type "loop". expand_composites still finds a composite after 64 iterations
    // → CompositeTooDeep.
    let loop_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("inner", "loop", MachinePhysicalSpec::default()));
    let loop_comp = CompositeSpec::new(loop_spec);

    let mut rt = Runtime::default();
    rt.register_composite("loop", loop_comp);

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("top", "loop", MachinePhysicalSpec::default()));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::CompositeTooDeep { depth: 64, .. }),
        "expected CompositeTooDeep, got {err:?}"
    );
}

#[test]
fn composite_unknown_type_fails_at_build() {
    // The unregistered composite type "unknown_comp" is not expanded by expand_composites
    // (it is not in the composites map); machine_type stays "unknown_comp", and at build time
    // registry.build cannot find it → InitFailed.
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("c", "unknown_comp", MachinePhysicalSpec::default()));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::InitFailed { ref machine, .. } if machine == "unknown_comp"),
        "expected InitFailed for unknown_comp, got {err:?}"
    );
}

#[test]
fn composite_internal_dangling_port() {
    // The composite's input_map points at a nonexistent sub-machine "nonexistent" — after
    // expansion the external link's into end becomes "comp.nonexistent.x", but machines has
    // no "comp.nonexistent" → validate_endpoint reports DanglingRef.
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");

    let bad_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()));
    let bad_comp = CompositeSpec::new(bad_spec)
        .with_input("in", "nonexistent", "x")
        .with_output("out", "d1", "y");
    rt.register_composite("bad_pair", bad_comp);

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "bad_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "in"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port }
                 if machine == "comp.nonexistent" && port == "x"),
        "expected DanglingRef for comp.nonexistent.x, got {err:?}"
    );
}

#[test]
fn composite_external_link_to_undefined_port() {
    // The external link points at the composite's undefined port "undefined_port" (not in
    // input_map); after expansion the port name stays unchanged and the machine name "comp"
    // no longer exists (expanded into comp.d1/d2) → validate_endpoint reports DanglingRef.
    let mut rt = Runtime::default();
    rt.register::<Doubler>("doubler");
    rt.register_composite("doubler_pair", doubler_pair_composite());

    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("entry", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("comp", "doubler_pair", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("entry", "y"), ("comp", "undefined_port"), LinkKind::Inline));

    let err = rt.materialize(&spec).unwrap_err();
    assert!(
        matches!(err, RuntimeError::DanglingRef { ref machine, ref port }
                 if machine == "comp" && port == "undefined_port"),
        "expected DanglingRef for comp.undefined_port, got {err:?}"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Cross-platform IO reactor real-socket tests (cfg-gated; compiled on Linux/macOS only)
// ════════════════════════════════════════════════════════════════════════════

#[cfg(target_os = "linux")]
#[test]
fn io_epoll_reactor_detects_tcp_readability() {
    // Real Linux epoll TCP listener: registered READABLE → client connects → poll detects
    // READABLE (EPOLLIN) → IoEvent yielded.
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::epoll::EpollReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("addr");
    let raw = listener.as_raw_fd();

    let mut reactor = EpollReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::READABLE, IoToken(42))
        .expect("register");

    // Poll before connecting should have no events (timeout=0, non-blocking).
    let no_events = reactor.poll(Some(Duration::from_millis(0))).expect("poll empty");
    assert!(no_events.is_empty(), "no events before connection");

    // Client connects → listener can accept → READABLE ready.
    let _client = TcpStream::connect(addr).expect("connect");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "should detect readable after connect");
    let found = events.iter().any(|e| e.token == IoToken(42) && e.readiness.is_readable());
    assert!(found, "token 42 readable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

#[cfg(target_os = "linux")]
#[test]
fn io_epoll_reactor_writable() {
    // Linux epoll TCP stream writability test: right after connect the stream is immediately
    // writable (send buffer free) → poll immediately returns WRITABLE (EPOLLOUT).
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::epoll::EpollReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    let stream = TcpStream::connect(addr).expect("connect");
    stream.set_nonblocking(true).expect("nonblocking");
    let raw = stream.as_raw_fd();

    let mut reactor = EpollReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::WRITABLE, IoToken(7))
        .expect("register");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "fresh TCP stream should be writable");
    let found = events.iter().any(|e| e.token == IoToken(7) && e.readiness.is_writable());
    assert!(found, "token 7 writable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

#[cfg(target_os = "macos")]
#[test]
fn io_kqueue_reactor_detects_tcp_readability() {
    // Real macOS kqueue TCP listener: registered READABLE → client connects → poll detects
    // READABLE (EVFILT_READ) → IoEvent yielded.
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::kqueue::KqueueReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let addr = listener.local_addr().expect("addr");
    let raw = listener.as_raw_fd();

    let mut reactor = KqueueReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::READABLE, IoToken(42))
        .expect("register");

    // Poll before connecting should have no events (timeout=0, non-blocking).
    let no_events = reactor.poll(Some(Duration::from_millis(0))).expect("poll empty");
    assert!(no_events.is_empty(), "no events before connection");

    // Client connects → listener can accept → READABLE ready.
    let _client = TcpStream::connect(addr).expect("connect");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "should detect readable after connect");
    let found = events.iter().any(|e| e.token == IoToken(42) && e.readiness.is_readable());
    assert!(found, "token 42 readable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

#[cfg(target_os = "macos")]
#[test]
fn io_kqueue_reactor_writable() {
    // macOS kqueue TCP stream writability test: right after connect the stream is
    // immediately writable → poll immediately returns WRITABLE (EVFILT_WRITE).
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::io::AsRawFd;
    use crate::io::kqueue::KqueueReactor;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    let stream = TcpStream::connect(addr).expect("connect");
    stream.set_nonblocking(true).expect("nonblocking");
    let raw = stream.as_raw_fd();

    let mut reactor = KqueueReactor::new().expect("reactor");
    reactor.register(raw as crate::io::RawIo, IoInterest::WRITABLE, IoToken(7))
        .expect("register");

    let events = reactor.poll(Some(Duration::from_secs(1))).expect("poll");
    assert!(!events.is_empty(), "fresh TCP stream should be writable");
    let found = events.iter().any(|e| e.token == IoToken(7) && e.readiness.is_writable());
    assert!(found, "token 7 writable event found");

    reactor.deregister(raw as crate::io::RawIo).expect("deregister");
}

// ════════════════════════════════════════════════════════════════════════
// CasFreeRing carrier end-to-end (SPSC lock-free ring)
// ════════════════════════════════════════════════════════════════════════

#[test]
fn runtime_casfree_ring_parallel_preserves_semantics() {
    // d1 → d2 uses CasFreeRing (a truly lock-free SPSC ring); under Parallel messages travel
    // through the ring. Semantics must be equivalent to Inline/Channel: exactly once per
    // input, in order.
    use axiom::link::{LinkKind, LinkSpec};
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"),
            ("d2", "x"),
            LinkKind::CasFreeRing {
                capacity: 4,
                storage: axiom::link::MemoryRegion::Heap { size: 1024 },
            },
        ));
    let mut rt = Runtime::new(RuntimeConfig::parallel(2));
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");

    let inputs: Vec<(String, String, Box<dyn std::any::Any + Send>)> =
        (1..=10).map(|n| {
            (
                "d1".to_string(),
                "x".to_string(),
                Box::new(n) as Box<dyn std::any::Any + Send>,
            )
        }).collect();
    let results = rt.tick(inputs).expect("tick");
    // d2 outputs = 4,8,...,40 (doubled by d1, then doubled again by d2).
    let mut outs: Vec<i32> = Vec::new();
    for r in results {
        if let ProcessResult::Yield { value, .. } = r {
            if let Some(v) = value.downcast_ref::<i32>() {
                outs.push(*v);
            }
        }
    }
    outs.sort_unstable();
    let expected: Vec<i32> = (1..=10).map(|n| n * 4).collect();
    assert_eq!(outs, expected, "CasFreeRing 必须恰好一次、不丢不重");
}

#[test]
fn runtime_casfree_ring_sequential_same_as_channel() {
    // Under Sequential (single-threaded): CasFreeRing semantics match Channel (Blocking).
    use axiom::link::{LinkKind, LinkSpec};
    let ring_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"),
            ("d2", "x"),
            LinkKind::CasFreeRing {
                capacity: 4,
                storage: axiom::link::MemoryRegion::Heap { size: 1024 },
            },
        ));
    let mut rt_ring = Runtime::new(RuntimeConfig::sequential());
    rt_ring.register::<Doubler>("doubler");
    rt_ring.materialize(&ring_spec).expect("materialize");

    let ch_spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(
            ("d1", "y"),
            ("d2", "x"),
            LinkKind::Channel { capacity: 4, drop_when_full: false },
        ));
    let mut rt_ch = Runtime::new(RuntimeConfig::sequential());
    rt_ch.register::<Doubler>("doubler");
    rt_ch.materialize(&ch_spec).expect("materialize");

    let inject = |rt: &mut Runtime| -> Vec<i32> {
        let inputs: Vec<(String, String, Box<dyn std::any::Any + Send>)> =
            (1..=5).map(|n| {
                ("d1".to_string(), "x".to_string(), Box::new(n) as Box<dyn std::any::Any + Send>)
            }).collect();
        let mut outs = Vec::new();
        for r in rt.tick(inputs).expect("tick") {
            if let ProcessResult::Yield { value, .. } = r {
                if let Some(v) = value.downcast_ref::<i32>() {
                    outs.push(*v);
                }
            }
        }
        outs.sort_unstable();
        outs
    };

    let from_ring = inject(&mut rt_ring);
    let from_channel = inject(&mut rt_ch);
    assert_eq!(from_ring, from_channel, "CasFreeRing 与 Channel 语义必须一致");
}

// ════════════════════════════════════════════════════════════════════════
// Event-sourced replayer — deterministic replay + time travel
// ════════════════════════════════════════════════════════════════════════

use crate::replay::{ReplayJournal, Replayer};

/// Build a Doubler-chain runtime (d1 → d2, Inline link).
fn doubler_chain_runtime() -> Runtime {
    use axiom::link::{LinkKind, LinkSpec};
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("d1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("d2", "doubler", MachinePhysicalSpec::default()))
        .with_link(LinkSpec::new(("d1", "y"), ("d2", "x"), LinkKind::Inline));
    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<Doubler>("doubler");
    rt.materialize(&spec).expect("materialize");
    rt
}

/// Run n batches of inputs (recording the journal + collecting each batch's outputs).
fn run_with_journal(
    build: impl Fn() -> Runtime,
    n: usize,
) -> (Vec<Vec<ProcessResult>>, ReplayJournal) {
    let mut rt = build();
    let mut journal = ReplayJournal::new();
    let mut outputs = Vec::new();
    for i in 0..n {
        journal.end_batch();
        let out = rt
            .tick(vec![(
                "d1".to_string(),
                "x".to_string(),
                Box::new(i as i32 + 1) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        journal.record("d1", "x", &(i as i32 + 1));
        outputs.push(out);
    }
    (outputs, journal)
}

#[test]
fn runtime_replay_forward_matches_original() {
    // Run 10 batches → record the journal → replay to batch 10 → outputs match the original
    // batch by batch.
    let (original, journal) = run_with_journal(doubler_chain_runtime, 10);
    let replayer = Replayer::new(&journal);
    let (_, replayed) = replayer
        .forward_to(10, doubler_chain_runtime)
        .expect("replay");
    assert_eq!(replayed.len(), 10);
    for i in 0..10 {
        assert_eq!(
            replayed[i].len(),
            original[i].len(),
            "第 {i} 批输出数应一致"
        );
    }
    // Value-by-value per batch (Doubler chain: i → 4i).
    let last_replayed = &replayed[9];
    let mut vals: Vec<i32> = Vec::new();
    for r in last_replayed {
        if let ProcessResult::Yield { value, .. } = r {
            if let Some(v) = value.downcast_ref::<i32>() {
                vals.push(*v);
            }
        }
    }
    assert_eq!(vals, vec![40], "重放第 10 批输出 = 40（10*4）");
}

#[test]
fn runtime_replay_timetravel_state_continuation() {
    // Time travel: replay to batch 3 → continue injecting 4..10 → the final state matches
    // the original.
    let (original, journal) = run_with_journal(doubler_chain_runtime, 10);
    let replayer = Replayer::new(&journal);

    // 1. Jump to time point 3 (replaying the first 3 batches).
    let (mut rt3, _) = replayer.forward_to(3, doubler_chain_runtime).expect("replay to 3");

    // 2. Continue injecting batches 4..10 (the same inputs as the original).
    let mut continuation = Vec::new();
    for i in 3..10 {
        let out = rt3
            .tick(vec![(
                "d1".to_string(),
                "x".to_string(),
                Box::new(i as i32 + 1) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick continuation");
        continuation.push(out);
    }

    // 3. Final-state consistency: continuation outputs == original batches 3..10 (equal
    //    values).
    for (i, (a, b)) in continuation.iter().zip(original.iter().skip(3)).enumerate() {
        let va: Vec<i32> = a.iter().filter_map(|r| {
            if let ProcessResult::Yield { value, .. } = r {
                value.downcast_ref::<i32>().map(|v| *v)
            } else { None }
        }).collect();
        let vb: Vec<i32> = b.iter().filter_map(|r| {
            if let ProcessResult::Yield { value, .. } = r {
                value.downcast_ref::<i32>().map(|v| *v)
            } else { None }
        }).collect();
        assert_eq!(va, vb, "时间旅行续接第 {} 批应一致", i + 3);
    }
}

#[test]
fn runtime_replay_verify_api() {
    // verify: a structural-level quick check (type_id + port + variant + count). Exact-value
    // verification is in runtime_replay_timetravel_state_continuation (downcast value
    // comparison).
    let (original, journal) = run_with_journal(doubler_chain_runtime, 5);
    let replayer = Replayer::new(&journal);
    let mismatch = replayer.verify(5, doubler_chain_runtime, original.iter());
    assert_eq!(mismatch, None, "未篡改 journal 应完全一致");

    // Structural tampering: batch 2 injects a wrong type (String instead of i32) →
    // from_port_name downcast fails → that batch's output is Idle (the runtime's defined
    // semantics) → its structure (discriminant) differs from the original Yield → verify
    // catches Some(1).
    let mut tampered = ReplayJournal::new();
    for i in 0..5 {
        tampered.end_batch();
        if i == 1 {
            tampered.record("d1", "x", &"WRONG_TYPE".to_string());
        } else {
            tampered.record("d1", "x", &(i as i32 + 1));
        }
    }
    let replayer2 = Replayer::new(&tampered);
    let mismatch = replayer2.verify(5, doubler_chain_runtime, original.iter());
    assert_eq!(
        mismatch,
        Some(1),
        "类型篡改 → Idle 输出 vs 原始 Yield → 结构不一致被捕获"
    );
}

#[test]
fn runtime_snapshot_replay_deterministic() {
    // Keyless assembled-snapshot test: blueprint (doubler_chain_runtime) + input sequence →
    // record → replay twice → assert determinism. This is the keyless regression gate for
    // "assembled behavior" — no external dependencies, pure replay; fixed blueprint + fixed
    // inputs ⇒ fixed outputs.
    let (original, journal) = run_with_journal(doubler_chain_runtime, 8);

    // First replay (all 8 batches).
    let replayer1 = Replayer::new(&journal);
    let (_, replay1) = replayer1.forward_to(8, doubler_chain_runtime).expect("replay1");

    // Second replay (same journal) — determinism: the two replays match batch by batch.
    let replayer2 = Replayer::new(&journal);
    let (_, replay2) = replayer2.forward_to(8, doubler_chain_runtime).expect("replay2");

    // Helper: extract the i32 Yield value sequence.
    let values = |batch: &[ProcessResult]| -> Vec<i32> {
        batch.iter()
            .filter_map(|r| {
                if let ProcessResult::Yield { value, .. } = r {
                    value.downcast_ref::<i32>().map(|v| *v)
                } else {
                    None
                }
            })
            .collect()
    };

    // 1. The two replays match each other (determinism).
    for (i, (a, b)) in replay1.iter().zip(replay2.iter()).enumerate() {
        assert_eq!(values(a), values(b), "重放确定性：第 {} 批两次重放应一致", i + 1);
    }

    // 2. Replay == original execution (assembled snapshot: blueprint + inputs ⇒ fixed
    //    outputs).
    for (i, (a, b)) in replay1.iter().zip(original.iter()).enumerate() {
        assert_eq!(values(a), values(b), "组装快照：第 {} 批重放 == 原始", i + 1);
    }
}



#[test]
fn fairness_prevents_machine_starvation() {
    // H2: per-machine per-round quota — the flooding machine is deferred to the next round;
    // other machines are not starved.
    let cfg = RuntimeConfig {
        mode: ExecMode::Sequential,
        max_ticks: Some(10_000),
        max_messages_per_machine: Some(1),
    };
    let mut rt = Runtime::new(cfg);
    rt.register::<Doubler>("doubler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("m1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("m2", "doubler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    // flood: m1 gets 100 messages; marker: m2 gets 1 (999).
    let mut inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)> = (0..100)
        .map(|i| ("m1".to_string(), "x".to_string(), Box::new(i as i32) as Box<dyn core::any::Any + Send>))
        .collect();
    inputs.push(("m2".to_string(), "x".to_string(), Box::new(999i32) as Box<dyn core::any::Any + Send>));

    let outputs = rt.tick(inputs).expect("tick");
    let values: Vec<i32> = outputs.iter().map(|o| match o {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().expect("i32 payload"),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();

    assert_eq!(values.len(), 101);
    // Quota 1: m1 hits its quota after 1 message → m2 is prioritized (2nd) — the flood does
    // not starve m2.
    assert_eq!(values[1], 1998, "m2 must be processed in round 2, not starved by m1 flood");
}

#[test]
fn fairness_quota_zero_keeps_fifo() {
    // Quota 0 = unlimited (FIFO preserved): m2's marker is processed last (queued behind the
    // flood).
    let cfg = RuntimeConfig {
        mode: ExecMode::Sequential,
        max_ticks: Some(10_000),
        max_messages_per_machine: Some(0),
    };
    let mut rt = Runtime::new(cfg);
    rt.register::<Doubler>("doubler");
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new("m1", "doubler", MachinePhysicalSpec::default()))
        .with_machine(MachineInstance::new("m2", "doubler", MachinePhysicalSpec::default()));
    rt.materialize(&spec).expect("materialize");

    let mut inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)> = (0..100)
        .map(|i| ("m1".to_string(), "x".to_string(), Box::new(i as i32) as Box<dyn core::any::Any + Send>))
        .collect();
    inputs.push(("m2".to_string(), "x".to_string(), Box::new(999i32) as Box<dyn core::any::Any + Send>));

    let outputs = rt.tick(inputs).expect("tick");
    let values: Vec<i32> = outputs.iter().map(|o| match o {
        ProcessResult::Yield { value, .. } => *value.downcast_ref::<i32>().expect("i32 payload"),
        other => panic!("expected Yield, got {other:?}"),
    }).collect();

    assert_eq!(values.len(), 101);
    // No quota: FIFO — m1's flood is processed first, m2's marker last (the 101st).
    assert_eq!(values[100], 1998, "without quota, FIFO order: m2 processed last");
}
