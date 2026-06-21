use axiom::prelude_all::*;
use axiom::machine::{ProcessOutput, InitError, CleanupError};
use axiom::port::MachineContext;
use std::sync::Arc;

// ════════════════════════════════════════════════════════════
// observe_is_connected tests
// ════════════════════════════════════════════════════════════

struct ObserveAwareMachine;

#[derive(Default)]
struct ObserveState { formatted_count: usize }

impl Machine for ObserveAwareMachine {
    type State = ObserveState;
    type Input = i32;
    type Output = i32;


    fn name() -> &'static str { "observe_aware" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<i32>("out"))
        .with(PortDecl::observe::<String>("log"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<ObserveState, InitError> { Ok(ObserveState::default()) }

    fn process(state: &mut ObserveState, ctx: &MachineContext, input: i32) -> ProcessOutput<i32> {
        if ctx.observe_is_connected() {
            state.formatted_count += 1;
        }
        ProcessOutput::Yield(input * 2)
    }

    fn cleanup(_state: ObserveState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

#[test] fn test_observe_disconnected_by_default() {
    let ctx = MachineContext::new("test");
    assert!(!ctx.observe_is_connected());
}

#[test] fn test_observe_connected_after_connect() {
    let ctx = MachineContext::new("test");
    ctx.observe_connect();
    assert!(ctx.observe_is_connected());
}

#[test] fn test_observe_disconnect() {
    let ctx = MachineContext::new("test");
    ctx.observe_connect();
    ctx.observe_disconnect();
    assert!(!ctx.observe_is_connected());
}

#[test] fn test_observe_aware_machine_skips_formatting_when_disconnected() {
    let ctx = MachineContext::new("observe_aware");
    let mut state = ObserveAwareMachine::init(&ctx).unwrap();
    let _ = ObserveAwareMachine::process(&mut state, &ctx, 42);
    assert_eq!(state.formatted_count, 0);
}

#[test] fn test_observe_aware_machine_formats_when_connected() {
    let ctx = MachineContext::new("observe_aware");
    ctx.observe_connect();
    let mut state = ObserveAwareMachine::init(&ctx).unwrap();
    let _ = ObserveAwareMachine::process(&mut state, &ctx, 42);
    assert_eq!(state.formatted_count, 1);
}

#[test] fn test_observe_connect_disconnect_toggle() {
    let ctx = MachineContext::new("observe_aware");
    let mut state = ObserveAwareMachine::init(&ctx).unwrap();
    ctx.observe_connect();
    let _ = ObserveAwareMachine::process(&mut state, &ctx, 1);
    assert_eq!(state.formatted_count, 1);
    ctx.observe_disconnect();
    let _ = ObserveAwareMachine::process(&mut state, &ctx, 2);
    assert_eq!(state.formatted_count, 1);
}

#[test] fn test_multiple_observers() {
    let ctx = MachineContext::new("test");
    ctx.observe_connect();
    ctx.observe_connect();
    assert!(ctx.observe_is_connected());
    ctx.observe_disconnect();
    assert!(ctx.observe_is_connected());
    ctx.observe_disconnect();
    assert!(!ctx.observe_is_connected());
}

// ════════════════════════════════════════════════════════════
// FuncWithScratch tests
// ════════════════════════════════════════════════════════════

struct ParseWithScratch;

impl Func for ParseWithScratch {
    type Input = &'static str;
    type Output = i32;
    fn name() -> &'static str { "parse" }
    fn call(input: &'static str) -> i32 { input.parse().unwrap_or(0) }
}
impl FuncWithScratch for ParseWithScratch {
    type Scratch = String;
    fn call_with(input: &'static str, scratch: &mut String) -> i32 {
        scratch.clear(); scratch.push_str(input);
        scratch.parse().unwrap_or(0)
    }
}

#[test] fn test_func_with_scratch_basic() {
    assert_eq!(ParseWithScratch::call("42"), 42);
}

#[test] fn test_func_with_scratch_scratched_trait() {
    assert_eq!(<Scratched<ParseWithScratch> as Func>::call("99"), 99);
}

#[test] fn test_func_with_scratch_buffer_reuse() {
    let mut scratch = <ParseWithScratch as FuncWithScratch>::Scratch::default();
    assert_eq!(ParseWithScratch::call_with("100", &mut scratch), 100);
    let cap1 = scratch.capacity();
    assert_eq!(ParseWithScratch::call_with("200", &mut scratch), 200);
    assert!(scratch.capacity() <= cap1 || cap1 == 0);
}

struct Double;
impl Func for Double {
    type Input = i32; type Output = i32;
    fn name() -> &'static str { "double" }
    fn call(x: i32) -> i32 { x * 2 }
}
impl FuncWithScratch for Double {
    type Scratch = ();
    fn call_with(x: i32, _s: &mut ()) -> i32 { x * 2 }
}

struct Triple;
impl Func for Triple {
    type Input = i32; type Output = i32;
    fn name() -> &'static str { "triple" }
    fn call(x: i32) -> i32 { x * 3 }
}
impl FuncWithScratch for Triple {
    type Scratch = ();
    fn call_with(x: i32, _s: &mut ()) -> i32 { x * 3 }
}

#[test] fn test_pipeline_two_steps() {
    type P = FuncScratchPipeline<(Double, Triple)>;
    let mut s = <P as FuncWithScratch>::Scratch::default();
    assert_eq!(P::call_with(5, &mut s), 30);
}

#[test] fn test_pipeline_two_steps_via_func_trait() {
    type P = FuncScratchPipeline<(Double, Triple)>;
    assert_eq!(<P as Func>::call(7), 42);
}

#[test] fn test_pipeline_three_steps() {
    type P = FuncScratchPipeline<(Triple, Triple, Triple)>;
    let mut s = <P as FuncWithScratch>::Scratch::default();
    assert_eq!(P::call_with(2, &mut s), 54);
}

// ════════════════════════════════════════════════════════════
// Snapshot tests
// ════════════════════════════════════════════════════════════

#[test] fn test_snapshot_none_by_default() {
    let ctx = MachineContext::new("test");
    assert!(ctx.snapshot().is_none());
}

#[test] fn test_snapshot_after_set() {
    let mut ctx = MachineContext::new("test");
    ctx.set_snapshot_fn(Arc::new(|| Some(vec![1, 2, 3, 4])));
    assert_eq!(ctx.snapshot().unwrap(), vec![1, 2, 3, 4]);
}

#[test] fn test_snapshot_state_machine() {
    struct SnapMachine;
    #[derive(Default)]
    struct SnState { count: i32 }

    impl Machine for SnapMachine {
        type State = SnState;
        type Input = i32;
        type Output = i32;
    
        fn name() -> &'static str { "snap" }
        fn port_schema() -> PortSchema { PortSchema::new()
            .with(PortDecl::input::<i32>("in"))
            .with(PortDecl::output::<i32>("out"))
        }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_ctx: &MachineContext) -> Result<SnState, InitError> { Ok(SnState::default()) }
        fn process(s: &mut SnState, _ctx: &MachineContext, i: i32) -> ProcessOutput<i32> {
            s.count += i; ProcessOutput::Yield(s.count)
        }
        fn cleanup(_s: SnState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }

    // Use Arc<Mutex<SnState>> to avoid borrow issues with the closure.
    let shared = std::sync::Arc::new(std::sync::Mutex::new(SnState::default()));
    let mut ctx = MachineContext::new("snap_test");
    let shared_clone = std::sync::Arc::clone(&shared);
    ctx.set_snapshot_fn(Arc::new(move || {
        Some(shared_clone.lock().unwrap().count.to_le_bytes().to_vec())
    }));

    {
        let mut state = shared.lock().unwrap();
        let _ = SnapMachine::process(&mut state, &ctx, 10);
        let _ = SnapMachine::process(&mut state, &ctx, 20);
        assert_eq!(state.count, 30);
    }

    assert_eq!(ctx.snapshot().unwrap(), vec![30u8, 0, 0, 0]);
}
