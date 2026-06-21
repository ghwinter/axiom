/// Tests for newly added axiom features:
/// - observe_is_connected() detection
/// - FuncWithScratch scratch buffer reuse
/// - MachineContext snapshot mechanism

use axiom::prelude_all::*;
use axiom::machine::{ProcessOutput, InitError, CleanupError};
use axiom::port::MachineContext;
use std::sync::Arc;

// ════════════════════════════════════════════════════════════
// observe_is_connected tests
// ════════════════════════════════════════════════════════════

struct ObserveAwareMachine;

#[derive(Default)]
struct ObserveState {
    formatted_count: usize,
}

impl Machine for ObserveAwareMachine {
    type State = ObserveState;
    type Input = i32;
    type Output = i32;
    type Observe = String;

    fn name() -> &'static str { "observe_aware" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<i32>("in"))
        .with(PortDecl::output::<i32>("out"))
        .with(PortDecl::observe::<String>("log"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<ObserveState, InitError> {
        Ok(ObserveState::default())
    }

    fn process(state: &mut ObserveState, ctx: &MachineContext, input: i32) -> ProcessOutput<i32> {
        // Only format the expensive observe string if someone is watching.
        if ctx.observe_is_connected() {
            state.formatted_count += 1;
            let _observe = format!("processed {}", input);
            // In reality, push to observe port here.
        }
        ProcessOutput::Yield(input * 2)
    }

    fn cleanup(_state: ObserveState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        Ok(())
    }
}

#[test]
fn test_observe_disconnected_by_default() {
    let ctx = MachineContext::new("test");
    assert!(!ctx.observe_is_connected(), "no consumer yet");
}

#[test]
fn test_observe_connected_after_connect() {
    let ctx = MachineContext::new("test");
    ctx.observe_connect();
    assert!(ctx.observe_is_connected());
}

#[test]
fn test_observe_disconnect() {
    let ctx = MachineContext::new("test");
    ctx.observe_connect();
    ctx.observe_disconnect();
    assert!(!ctx.observe_is_connected());
}

#[test]
fn test_observe_aware_machine_skips_formatting_when_disconnected() {
    let ctx = MachineContext::new("observe_aware");
    let mut state = ObserveAwareMachine::init(&ctx).unwrap();

    // Process without observer — should NOT increment formatted_count.
    let _ = ObserveAwareMachine::process(&mut state, &ctx, 42);
    assert_eq!(state.formatted_count, 0, "should skip formatting when disconnected");
}

#[test]
fn test_observe_aware_machine_formats_when_connected() {
    let ctx = MachineContext::new("observe_aware");
    ctx.observe_connect();  // simulate observer connection
    let mut state = ObserveAwareMachine::init(&ctx).unwrap();

    let _ = ObserveAwareMachine::process(&mut state, &ctx, 42);
    assert_eq!(state.formatted_count, 1, "should format when observed");
}

#[test]
fn test_observe_connect_disconnect_toggle() {
    let ctx = MachineContext::new("observe_aware");
    let mut state = ObserveAwareMachine::init(&ctx).unwrap();

    // Observer connects
    ctx.observe_connect();
    let _ = ObserveAwareMachine::process(&mut state, &ctx, 1);
    assert_eq!(state.formatted_count, 1);

    // Observer disconnects
    ctx.observe_disconnect();
    let _ = ObserveAwareMachine::process(&mut state, &ctx, 2);
    assert_eq!(state.formatted_count, 1, "should not increment when disconnected again");
}

#[test]
fn test_multiple_observers() {
    let ctx = MachineContext::new("test");
    ctx.observe_connect();  // observer 1
    ctx.observe_connect();  // observer 2
    assert!(ctx.observe_is_connected());
    ctx.observe_disconnect(); // observer 1 leaves
    assert!(ctx.observe_is_connected(), "observer 2 still connected");
    ctx.observe_disconnect(); // observer 2 leaves
    assert!(!ctx.observe_is_connected());
}

// ════════════════════════════════════════════════════════════
// FuncWithScratch tests
// ════════════════════════════════════════════════════════════

/// A FuncWithScratch that parses &str to i32 and reuses its String scratch.
struct ParseWithScratch;

impl Func for ParseWithScratch {
    type Input = &'static str;
    type Output = i32;
    fn name() -> &'static str { "parse_with_scratch" }
    fn call(input: &'static str) -> i32 {
        input.parse().unwrap_or(0)
    }
}

impl FuncWithScratch for ParseWithScratch {
    /// Scratch is String — reused across calls, reallocates only if
    /// the input grows beyond the previous max capacity.
    type Scratch = String;

    fn call_with(input: &'static str, scratch: &mut String) -> i32 {
        scratch.clear();
        scratch.push_str(input);
        scratch.parse().unwrap_or(0)
    }
}

#[test]
fn test_func_with_scratch_basic() {
    let result = ParseWithScratch::call("42");
    assert_eq!(result, 42);
}

#[test]
fn test_func_with_scratch_scratched_trait() {
    // Scratched wraps a FuncWithScratch into a plain Func,
    // allocating a fresh scratch on each call.
    let result = <Scratched<ParseWithScratch> as Func>::call("99");
    assert_eq!(result, 99);
}

#[test]
fn test_func_with_scratch_buffer_reuse() {
    let mut scratch = <ParseWithScratch as FuncWithScratch>::Scratch::default();

    // First call allocates.
    let r1 = ParseWithScratch::call_with("100", &mut scratch);
    assert_eq!(r1, 100);
    let cap_after_first = scratch.capacity();

    // Second call reuses allocated capacity.
    let r2 = ParseWithScratch::call_with("200", &mut scratch);
    assert_eq!(r2, 200);
    let cap_after_second = scratch.capacity();

    // Capacity should not grow for same-size input.
    assert!(
        cap_after_second <= cap_after_first || cap_after_first == 0,
        "capacity should not needlessly increase"
    );
}

// ── Two-step pipeline test ─────────────────────────────────

struct Double;
impl Func for Double {
    type Input = i32; type Output = i32;
    fn name() -> &'static str { "double" }
    fn call(x: i32) -> i32 { x * 2 }
}
impl FuncWithScratch for Double {
    type Scratch = ();
    fn call_with(x: i32, _scratch: &mut ()) -> i32 { x * 2 }
}

struct Triple;
impl Func for Triple {
    type Input = i32; type Output = i32;
    fn name() -> &'static str { "triple" }
    fn call(x: i32) -> i32 { x * 3 }
}
impl FuncWithScratch for Triple {
    type Scratch = ();
    fn call_with(x: i32, _scratch: &mut ()) -> i32 { x * 3 }
}

#[test]
fn test_pipeline_two_steps() {
    type DoubleTriple = FuncScratchPipeline<(Double, Triple)>;

    let mut scratch = <DoubleTriple as FuncWithScratch>::Scratch::default();
    let result = DoubleTriple::call_with(5, &mut scratch);
    // Double(5) = 10, Triple(10) = 30
    assert_eq!(result, 30);
}

#[test]
fn test_pipeline_two_steps_via_func_trait() {
    type DoubleTriple = FuncScratchPipeline<(Double, Triple)>;
    let result = <DoubleTriple as Func>::call(7);
    assert_eq!(result, 42);
}

// ── Three-step pipeline test ───────────────────────────────

#[test]
fn test_pipeline_three_steps() {
    // This requires a third Func that outputs the same type as its input.
    // We can reuse Triple since Triple: Func<Input=i32, Output=i32>.
    type TripleTripleTriple = FuncScratchPipeline<(Triple, Triple, Triple)>;

    let mut scratch = <TripleTripleTriple as FuncWithScratch>::Scratch::default();
    let result = TripleTripleTriple::call_with(2, &mut scratch);
    // 2 * 3 * 3 * 3 = 54
    assert_eq!(result, 54);
}

// ════════════════════════════════════════════════════════════
// MachineContext snapshot tests
// ════════════════════════════════════════════════════════════

#[test]
fn test_snapshot_none_by_default() {
    let ctx = MachineContext::new("test");
    assert!(ctx.snapshot().is_none());
}

#[test]
fn test_snapshot_after_set() {
    let mut ctx = MachineContext::new("test");
    let snapshot_data = vec![1, 2, 3, 4];
    let cloned = snapshot_data.clone();

    ctx.set_snapshot_fn(Arc::new(move || Some(cloned.clone())));

    let result = ctx.snapshot();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), vec![1, 2, 3, 4]);
}

#[test]
fn test_snapshot_state_machine() {
    struct SnapshotMachine;

    #[derive(Default)]
    struct SnapState { count: i32 }

    impl Machine for SnapshotMachine {
        type State = SnapState;
        type Input = i32;
        type Output = i32;
        type Observe = ();

        fn name() -> &'static str { "snapshot_test" }
        fn port_schema() -> PortSchema { PortSchema::new()
            .with(PortDecl::input::<i32>("in"))
            .with(PortDecl::output::<i32>("out"))
        }
        fn config_schema() -> ConfigSchema { ConfigSchema::new() }

        fn init(_ctx: &MachineContext) -> Result<SnapState, InitError> {
            Ok(SnapState::default())
        }

        fn process(state: &mut SnapState, _ctx: &MachineContext, input: i32) -> ProcessOutput<i32> {
            state.count += input;
            ProcessOutput::Yield(state.count)
        }

        fn cleanup(_state: SnapState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }

        fn checkpoint(state: &SnapState) -> Option<Vec<u8>> {
            Some(state.count.to_le_bytes().to_vec())
        }

        fn restore(state: &mut SnapState, data: &[u8]) -> Result<(), axiom::machine::RestoreError> {
            if data.len() < 4 {
                return Err(axiom::machine::RestoreError::ChecksumMismatch);
            }
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&data[..4]);
            state.count = i32::from_le_bytes(arr);
            Ok(())
        }
    }

    let mut ctx = MachineContext::new("snap_test");
    let mut state = SnapshotMachine::init(&ctx).unwrap();

    // Set up snapshot function that captures state.
    let snapshotter = {
        // We need a way to access state from the closure.
        // In a real runtime, this is done differently.
        // Here we just test that the mechanism works.
        Arc::new(|| None::<Vec<u8>>)
    };
    ctx.set_snapshot_fn(snapshotter);

    // Process some values.
    let _ = SnapshotMachine::process(&mut state, &ctx, 10);
    let _ = SnapshotMachine::process(&mut state, &ctx, 20);
    assert_eq!(state.count, 30);

    // Test restore
    let snapshot = SnapshotMachine::checkpoint(&state);
    assert!(snapshot.is_some());
    assert_eq!(snapshot.unwrap(), vec![30u8, 0, 0, 0]);

    let mut restored = SnapState::default();
    let _ = SnapshotMachine::restore(&mut restored, &[30, 0, 0, 0]);
    assert_eq!(restored.count, 30);
}
