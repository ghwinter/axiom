/// Integration tests for axiom core abstractions.
///
/// Tests the Func and Machine traits, PortDecl compatibility,
/// DeploySpec validation, and Clock implementations.

use axiom::prelude_all::*;
use axiom::time::Clock;

// ════════════════════════════════════════════════════════════
// Func tests
// ════════════════════════════════════════════════════════════

struct Double;

impl Func for Double {
    type Input = i32;
    type Output = i32;
    fn name() -> &'static str { "double" }
    fn call(x: i32) -> i32 { x * 2 }
    fn cost_estimate() -> CostEstimate { CostEstimate::Trivial }
}

#[test]
fn test_func_basic() {
    assert_eq!(Double::call(5), 10);
    assert_eq!(Double::call(-3), -6);
    assert_eq!(Double::call(0), 0);
}

#[test]
fn test_func_properties() {
    assert!(Double::cost_estimate() <= CostEstimate::Expensive);
    assert!(!Double::nondeterministic());
}

struct Identity<T>(std::marker::PhantomData<T>);

impl<T: Copy + Send + Sync + 'static> Func for Identity<T> {
    type Input = T;
    type Output = T;
    fn name() -> &'static str { "identity" }
    fn call(x: T) -> T { x }
}

#[test]
fn test_func_generic() {
    assert_eq!(Identity::call(42), 42);
    assert_eq!(Identity::call("hello"), "hello");
    assert_eq!(Identity::call(3.14), 3.14);
}

#[test]
fn test_func_composition() {
    // Manually compose two Funcs
    let x = 7;
    let doubled = Double::call(x);
    let quad = Double::call(doubled);
    assert_eq!(quad, 28);
}

// ════════════════════════════════════════════════════════════
// Machine tests
// ════════════════════════════════════════════════════════════

struct SimpleCounter;

#[derive(Debug, PartialEq)]
struct CounterState { count: i32 }

impl Machine for SimpleCounter {
    type State = CounterState;
    type Input = i32;
    type Output = i32;
    type Observe = ();

    fn name() -> &'static str { "counter" }

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<i32>("in"))
            .with(PortDecl::output::<i32>("out"))
    }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<CounterState, InitError> {
        Ok(CounterState { count: 0 })
    }

    fn process(
        state: &mut CounterState,
        _ctx: &MachineContext,
        input: i32,
    ) -> ProcessOutput<i32> {
        state.count += input;
        ProcessOutput::Yield(state.count)
    }

    fn cleanup(state: CounterState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        // Ensure final state is correct
        assert!(state.count > 0, "counter should have been incremented");
        Ok(())
    }

    fn deterministic() -> bool { true }
}

#[test]
fn test_machine_init() {
    let ctx = MachineContext::new("test");
    let state = SimpleCounter::init(&ctx).unwrap();
    assert_eq!(state.count, 0);
}

#[test]
fn test_machine_process() {
    let ctx = MachineContext::new("test");
    let mut state = SimpleCounter::init(&ctx).unwrap();

    let r1 = SimpleCounter::process(&mut state, &ctx, 5);
    assert!(matches!(r1, ProcessOutput::Yield(5)));

    let r2 = SimpleCounter::process(&mut state, &ctx, 3);
    assert!(matches!(r2, ProcessOutput::Yield(8)));
}

#[test]
fn test_machine_full_lifecycle() {
    let ctx = MachineContext::new("test");
    let mut state = SimpleCounter::init(&ctx).unwrap();

    for val in [10, 20, 30] {
        let _ = SimpleCounter::process(&mut state, &ctx, val);
    }

    let result = SimpleCounter::process(&mut state, &ctx, 0);
    assert!(matches!(result, ProcessOutput::Yield(60)));

    let _ = SimpleCounter::cleanup(state, &ctx);
}

#[test]
fn test_machine_nondeterministic_default() {
    // SimpleCounter explicitly returns true
    assert!(SimpleCounter::deterministic());
}

// ════════════════════════════════════════════════════════════
// Port schema + link compat tests
// ════════════════════════════════════════════════════════════

#[test]
fn test_port_decl_creation() {
    let in_port = PortDecl::input::<i32>("data_in");
    assert_eq!(in_port.dir, PortDir::In);
    assert_eq!(in_port.name, "data_in");

    let out_port = PortDecl::output::<String>("result_out");
    assert_eq!(out_port.dir, PortDir::Out);
    assert_eq!(out_port.name, "result_out");

    let obs_port = PortDecl::observe::<f64>("metric");
    assert_eq!(obs_port.dir, PortDir::Observe);
    assert_eq!(obs_port.name, "metric");
}

#[test]
fn test_port_schema() {
    let schema = PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<String>("out"));

    assert_eq!(schema.ports().len(), 2);
    assert!(schema.primary_input().is_some());
    assert!(schema.primary_output().is_some());
    assert!(schema.observe_port().is_none());
}

#[test]
fn test_link_compat_success() {
    let a = PortDecl::output::<i32>("a");
    let b = PortDecl::input::<i32>("b");
    assert_eq!(a.can_link_to(&b), LinkCompat::Compatible);
}

#[test]
fn test_link_compat_wrong_direction() {
    let a = PortDecl::input::<i32>("a");
    let b = PortDecl::input::<i32>("b");
    assert!(matches!(a.can_link_to(&b), LinkCompat::Incompatible { .. }));
}

#[test]
fn test_link_compat_type_mismatch() {
    let a = PortDecl::output::<i32>("a");
    let b = PortDecl::input::<String>("b");
    assert!(matches!(a.can_link_to(&b), LinkCompat::Incompatible { .. }));
}

#[test]
fn test_link_compat_schema_migration() {
    let a = PortDecl::output::<i32>("a").with_schema_ver(2);
    let b = PortDecl::input::<i32>("b").with_schema_ver(1);
    assert_eq!(a.can_link_to(&b), LinkCompat::Migrate { from_ver: 1, to_ver: 2 });
}

// ════════════════════════════════════════════════════════════
// Deploy spec tests
// ════════════════════════════════════════════════════════════

#[test]
fn test_deploy_spec_empty() {
    let spec = DeploySpec::new();
    assert_eq!(spec.machines.len(), 0);
    assert_eq!(spec.links.len(), 0);
    assert!(spec.validate().is_ok());
}

#[test]
fn test_deploy_spec_with_machine() {
    let spec = DeploySpec::new()
        .with_machine(MachineInstance {
            name: "counter",
            machine_type: "SimpleCounter",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        });

    assert_eq!(spec.machines.len(), 1);
    assert_eq!(spec.machines[0].name, "counter");
}

#[test]
fn test_deploy_spec_validation_unknown_machine() {
    let spec = DeploySpec::new()
        .with_machine(MachineInstance {
            name: "a",
            machine_type: "A",
            physical: MachinePhysicalSpec::default(),
            config_overrides: vec![],
        })
        .with_link(LinkSpec::new(
            ("nonexistent", "out"),
            ("a", "in"),
            LinkKind::Inline,
        ));

    assert!(spec.validate().is_err());
}

// ════════════════════════════════════════════════════════════
// Clock tests
// ════════════════════════════════════════════════════════════

#[test]
fn test_time_tick_creation() {
    let t = TimeTick::from_millis(1000);
    assert_eq!(t.ns, 1_000_000_000);
    assert_eq!(t.as_millis(), 1000);
}

#[test]
fn test_time_tick_duration() {
    let t1 = TimeTick::from_millis(5000);
    let t2 = TimeTick::from_millis(7000);
    let dur = t2.duration_since(t1);
    assert_eq!(dur.as_secs(), 2);
}

#[test]
fn test_replay_clock() {
    let ticks = vec![
        TimeTick::from_millis(100),
        TimeTick::from_millis(200),
        TimeTick::from_millis(300),
    ];
    let mut clock = ReplayClock::new(ticks);

    assert_eq!(clock.now().as_millis(), 100);
    clock.advance(core::time::Duration::from_secs(1));
    assert_eq!(clock.now().as_millis(), 200);
    clock.advance(core::time::Duration::from_secs(1));
    assert_eq!(clock.now().as_millis(), 300);
    assert!(!clock.is_exhausted());
    clock.advance(core::time::Duration::from_secs(1));
    assert!(clock.is_exhausted());
}

#[test]
fn test_real_clock_monotonic() {
    let clock = RealClock::new();
    let t1 = clock.now();
    std::thread::sleep(std::time::Duration::from_millis(10));
    let t2 = clock.now();
    assert!(t2.ns > t1.ns, "RealClock should be monotonic");
}

// ════════════════════════════════════════════════════════════
// Resource class tests
// ════════════════════════════════════════════════════════════

#[test]
fn test_resource_class_debug() {
    let static_res = ResourceClass::Static;
    let heap_res = ResourceClass::DynamicHeap { estimated_bytes: 4096 };
    let os_res = ResourceClass::OsResource { kind: "tcp_socket" };

    assert!(core::mem::discriminant(&static_res) != core::mem::discriminant(&heap_res));
    assert!(core::mem::discriminant(&os_res) != core::mem::discriminant(&heap_res));
}

// ════════════════════════════════════════════════════════════
// Machine with Observe port
// ════════════════════════════════════════════════════════════

struct ObservableCounter;

#[derive(Debug, PartialEq)]
struct ObsState { count: i32 }

impl Machine for ObservableCounter {
    type State = ObsState;
    type Input = i32;
    type Output = i32;
    type Observe = String;  // ← has observation data

    fn name() -> &'static str { "observable_counter" }

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<i32>("in"))
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::observe::<String>("log"))
    }

    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<ObsState, InitError> {
        Ok(ObsState { count: 0 })
    }

    fn process(
        state: &mut ObsState,
        _ctx: &MachineContext,
        input: i32,
    ) -> ProcessOutput<i32> {
        state.count += input;
        // Output also carries observation data
        ProcessOutput::Yield(state.count)
    }

    fn cleanup(_state: ObsState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        Ok(())
    }
}

#[test]
fn test_machine_with_observe() {
    let schema = ObservableCounter::port_schema();
    let obs = schema.observe_port();
    assert!(obs.is_some());
    assert_eq!(obs.unwrap().name, "log");
}

// ════════════════════════════════════════════════════════════
// Edge cases
// ════════════════════════════════════════════════════════════

#[test]
fn test_machine_process_idle() {
    struct Idler;
    impl Machine for Idler {
        type State = ();
        type Input = i32;
        type Output = i32;
        type Observe = ();
        fn name() -> &'static str { "idler" }
        fn port_schema() -> PortSchema { PortSchema::new() }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
        fn process(_: &mut (), _: &MachineContext, _: i32) -> ProcessOutput<i32> {
            ProcessOutput::Idle
        }
        fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }

    let ctx = MachineContext::new("idler");
    let mut state = Idler::init(&ctx).unwrap();
    let result = Idler::process(&mut state, &ctx, 42);
    assert!(matches!(result, ProcessOutput::Idle));
}

#[test]
fn test_machine_process_done() {
    struct OneShot;
    impl Machine for OneShot {
        type State = bool;
        type Input = i32;
        type Output = i32;
        type Observe = ();
        fn name() -> &'static str { "oneshot" }
        fn port_schema() -> PortSchema { PortSchema::new() }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_ctx: &MachineContext) -> Result<bool, InitError> { Ok(false) }
        fn process(state: &mut bool, _: &MachineContext, input: i32) -> ProcessOutput<i32> {
            if *state {
                ProcessOutput::Done
            } else {
                *state = true;
                ProcessOutput::Yield(input)
            }
        }
        fn cleanup(state: bool, _: &MachineContext) -> Result<(), CleanupError> {
            assert!(state);
            Ok(())
        }
    }

    let ctx = MachineContext::new("oneshot");
    let mut state = OneShot::init(&ctx).unwrap();

    // First call yields
    let r1 = OneShot::process(&mut state, &ctx, 99);
    assert!(matches!(r1, ProcessOutput::Yield(99)));

    // Second call returns Done
    let r2 = OneShot::process(&mut state, &ctx, 100);
    assert!(matches!(r2, ProcessOutput::Done));
}
