//! Streaming contract tests (A3) — pull-model output for Machines.

use axiom::machine::{CleanupError, InitError, Machine, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::portset::{In, Out, SinglePorts};
use axiom::stream::StreamingMachine;

/// Produces rows `0..n` lazily; state is the cursor.
struct RowSource;

impl Machine for RowSource {
    type State = i64; // cursor position
    type Input = In<i64>; // row count n
    type Output = Out<i64>;
    type Ports = SinglePorts<i64>;
    type ProcessOutput = SingleOutput<Out<i64>>;

    fn name() -> &'static str { "row_source" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i64, InitError> { Ok(0) }
    fn process(s: &mut i64, _: &MachineContext, _: In<i64>) -> SingleOutput<Out<i64>> {
        // Push path: one step of the same cursor (kept for non-stream users).
        let v = *s;
        *s += 1;
        SingleOutput::Yield(Out(v))
    }
    fn cleanup(_: i64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

impl StreamingMachine for RowSource {
    type StreamError = String;

    fn process_stream<'a>(
        state: &'a mut i64,
        _ctx: &MachineContext,
        input: In<i64>,
    ) -> Result<impl Iterator<Item = Out<i64>> + 'a, String> {
        let In(n) = input;
        if n < 0 {
            return Err(format!("negative row count: {}", n));
        }
        // State is the cursor; the FIRST next() resets it (strict laziness:
        // construction has no side effects), each next() advances it.
        let mut started = false;
        Ok(std::iter::from_fn(move || {
            if !started {
                *state = 0;
                started = true;
            }
            if *state < n {
                let v = *state;
                *state += 1;
                Some(Out(v))
            } else {
                None
            }
        }))
    }
}

#[test]
fn streaming_is_lazy() {
    let ctx = MachineContext::new("rows");
    let mut state = 0i64;
    {
        let iter = RowSource::process_stream(&mut state, &ctx, In(10)).unwrap();
        // Dropped without consuming a single item: construction has no
        // side effects (laziness), and the `&mut State` borrow ends here.
        drop(iter);
    }
    assert_eq!(state, 0);
}

#[test]
fn streaming_cursor_advances_per_next() {
    let ctx = MachineContext::new("rows");
    let mut state = 0i64;
    let (a, b) = {
        let mut iter = RowSource::process_stream(&mut state, &ctx, In(10)).unwrap();
        let a = iter.next();
        let b = iter.next();
        (a, b)
    }; // iterator dropped: borrow released (impl Trait may have a destructor)
    assert_eq!(a, Some(Out(0)));
    assert_eq!(b, Some(Out(1)));
    assert_eq!(state, 2); // cursor advanced once per next()
}

#[test]
fn streaming_drains_to_end() {
    let ctx = MachineContext::new("rows");
    let mut state = 0i64;
    let rows: Vec<i64> = RowSource::process_stream(&mut state, &ctx, In(5))
        .unwrap()
        .map(|Out(v)| v)
        .collect();
    assert_eq!(rows, vec![0, 1, 2, 3, 4]);
    assert_eq!(state, 5); // cursor fully advanced
}

#[test]
fn streaming_error_is_construction_time() {
    let ctx = MachineContext::new("rows");
    let mut state = 0i64;
    let err = match RowSource::process_stream(&mut state, &ctx, In(-1)) {
        Err(e) => e,
        Ok(_) => panic!("expected construction error"),
    };
    assert!(err.contains("negative"));
    // Failed construction leaves no partial stream and no cursor movement.
    assert_eq!(state, 0);
}

#[test]
fn streaming_zero_rows_is_empty() {
    let ctx = MachineContext::new("rows");
    let mut state = 42i64;
    {
        let mut iter = RowSource::process_stream(&mut state, &ctx, In(0)).unwrap();
        assert_eq!(iter.next(), None);
    } // iterator dropped: borrow released
    assert_eq!(state, 0); // cursor reset on construction
}

/// A machine implementing BOTH push (`Machine`) and pull (`StreamingMachine`)
/// contracts — the two coexist; the push path is for eager consumers, the
/// pull path for lazy ones. (Streaming and `FusedInline` remain mutually
/// exclusive by contract, documented in `stream.rs`.)
#[test]
fn push_and_pull_coexist() {
    let ctx = MachineContext::new("rows");
    let mut state = 0i64;

    // Push path via Machine::process.
    let out = RowSource::process(&mut state, &ctx, In(0));
    assert_eq!(out, SingleOutput::Yield(Out(0)));
    assert_eq!(state, 1);

    // Pull path on the same instance afterwards.
    let rows: Vec<i64> = RowSource::process_stream(&mut state, &ctx, In(3))
        .unwrap()
        .map(|Out(v)| v)
        .collect();
    assert_eq!(rows, vec![0, 1, 2]);
}
