use axiom::prelude_all::*;
use std::sync::Arc;

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

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct SnapPorts {
        input type SnapInput {
            in_ [Data] => i32,
        }
        output type SnapOutput {
            out [Data] => i32,
        }
    }
}

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
        type Input = SnapInput;
        type Output = SnapOutput;
        type Ports = SnapPorts;
        type ProcessOutput = SingleOutput<Self::Output>;

        fn name() -> &'static str { "snap" }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }
        fn init(_ctx: &MachineContext) -> Result<SnState, InitError> { Ok(SnState::default()) }
        fn process(s: &mut SnState, _ctx: &MachineContext, input: SnapInput) -> SingleOutput<SnapOutput> {
            let i = match input { SnapInput::in_(v) => v };
            s.count += i; SingleOutput::Yield(SnapOutput::out(s.count))
        }
        fn cleanup(_s: SnState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }

    let shared = std::sync::Arc::new(std::sync::Mutex::new(SnState::default()));
    let mut ctx = MachineContext::new("snap_test");
    let shared_clone = std::sync::Arc::clone(&shared);
    ctx.set_snapshot_fn(Arc::new(move || {
        Some(shared_clone.lock().unwrap().count.to_le_bytes().to_vec())
    }));

    {
        let mut state = shared.lock().unwrap();
        let _ = SnapMachine::process(&mut state, &ctx, SnapInput::in_(10));
        let _ = SnapMachine::process(&mut state, &ctx, SnapInput::in_(20));
        assert_eq!(state.count, 30);
    }

    assert_eq!(ctx.snapshot().unwrap(), vec![30u8, 0, 0, 0]);
}
