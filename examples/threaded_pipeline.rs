//! threaded_pipeline — a multi-threaded pressure test for axiom core contracts.
//!
//! axiom core ships no runtime, so this example *hand-writes* the smallest
//! thread-per-machine driver (the pattern a future runtime adapter will
//! formalise) and uses it to stress the concurrency contracts:
//!
//! ```text
//! SeqSource ──(bounded)──► Tee ──┬──► Doubler ──(bounded)──► Collector
//! (tick-driven)            │    └──► Tripler ──(bounded)──► Collector
//!                           (fan-out: YieldMulti)
//! ```
//!
//! What it verifies:
//! 1. **Data integrity across threads** — exactly 2N values arrive (Tee
//!    fan-out duplicates), with the right sums (no loss, no duplication).
//! 2. **Backpressure propagation** — bounded channels + `try_send` +
//!    retry (the physical realisation of `BlockPolicy`); a slow worker
//!    stalls the whole chain, measured as blocked-send counts.
//! 3. **Multi-instance parallelism** — two worker machines run on separate
//!    threads; each `&mut State` is per-instance serial, instances run
//!    concurrently (measured as max concurrent workers).
//! 4. **`MachineContext` atomics across threads** — lifecycle/signal/time
//!    fields are touched only from the owning thread; no cross-thread
//!    sharing of one machine instance (the `&mut State` contract).

use axiom::builtin::*; // Tee, TeeInput, TeeOutput
use axiom::declare_ports;
use axiom::machine::{
    CleanupError, InitError, Machine, MachineHandle, SingleOutput, Init,
};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::portset::{In, Out, SinglePorts};
use axiom::prelude_all::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::time::Instant;

// ════════════════════════════════════════════════════════════════════════════
// Machines
// ════════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct SeqSourcePorts {
        input type SeqSourceInput {
            tick [Data] => (),
        }
        output type SeqSourceOutput {
            out [Data] => i64,
        }
    }
}

/// Produces `1, 2, 3, ...` — one value per tick (state is per-instance).
struct SeqSource;

impl Machine for SeqSource {
    type State = i64;
    type Input = SeqSourceInput;
    type Output = SeqSourceOutput;
    type Ports = SeqSourcePorts;
    type ProcessOutput = SingleOutput<SeqSourceOutput>;

    fn name() -> &'static str { "seq_source" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i64, InitError> { Ok(0) }
    #[inline]
    fn process(s: &mut i64, _: &MachineContext, _: SeqSourceInput) -> SingleOutput<SeqSourceOutput> {
        *s += 1;
        SingleOutput::Yield(SeqSourceOutput::out(*s))
    }
    fn cleanup(_: i64, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

/// Doubler worker: `v -> v * 2`.
struct Doubler;
impl Machine for Doubler {
    type State = ();
    type Input = In<i64>;
    type Output = Out<i64>;
    type Ports = SinglePorts<i64>;
    type ProcessOutput = SingleOutput<Out<i64>>;

    fn name() -> &'static str { "doubler" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(_: &mut (), _: &MachineContext, In(v): In<i64>) -> SingleOutput<Out<i64>> {
        SingleOutput::Yield(Out(v * 2))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

/// Tripler worker: `v -> v * 3`.
struct Tripler;
impl Machine for Tripler {
    type State = ();
    type Input = In<i64>;
    type Output = Out<i64>;
    type Ports = SinglePorts<i64>;
    type ProcessOutput = SingleOutput<Out<i64>>;

    fn name() -> &'static str { "tripler" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    #[inline]
    fn process(_: &mut (), _: &MachineContext, In(v): In<i64>) -> SingleOutput<Out<i64>> {
        SingleOutput::Yield(Out(v * 3))
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct CollectorPorts {
        input type CollectorInput {
            value [Data] => i64,
        }
        output type CollectorOutput {
            // no output ports — a pure sink
        }
    }
}

/// Collects (count, sum) — the verification point of the whole pipeline.
struct Collector;

impl Machine for Collector {
    type State = (u64, i64);
    type Input = CollectorInput;
    type Output = CollectorOutput;
    type Ports = CollectorPorts;
    type ProcessOutput = SingleOutput<CollectorOutput>;

    fn name() -> &'static str { "collector" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(u64, i64), InitError> { Ok((0, 0)) }
    #[inline]
    fn process(s: &mut (u64, i64), _: &MachineContext, input: CollectorInput) -> SingleOutput<CollectorOutput> {
        let CollectorInput::value(v) = input;
        s.0 += 1;
        s.1 += v;
        SingleOutput::Idle // sink: no output
    }
    fn cleanup(_: (u64, i64), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

// ════════════════════════════════════════════════════════════════════════════
// Hand-written driver helpers
// ════════════════════════════════════════════════════════════════════════════

/// Blocking send with backpressure measurement: `try_send`, count failures,
/// retry. This is the physical realisation of `BlockPolicy` on a bounded
/// channel (the runtime-adapter contract `BackpressurePolicy::decide`
/// abstracts exactly this decision).
fn blocking_send(tx: &SyncSender<i64>, v: i64, blocked: &AtomicUsize) {
    loop {
        match tx.try_send(v) {
            Ok(()) => return,
            Err(_) => {
                blocked.fetch_add(1, Ordering::Relaxed);
                std::thread::yield_now();
            }
        }
    }
}

/// Source thread: tick the `SeqSource` N times, push values downstream.
fn source_thread(n: usize, tx: SyncSender<i64>, blocked: &AtomicUsize) {
    let ctx = MachineContext::new("source");
    let mut handle = MachineHandle::<SeqSource, Init>::new(ctx).expect("source init").start();
    for _ in 0..n {
        let out = handle.process(SeqSourceInput::tick(()));
        match out {
            SingleOutput::Yield(SeqSourceOutput::out(v)) => blocking_send(&tx, v, blocked),
            _ => unreachable!("source always yields"),
        }
    }
    let stopping = handle.stop();
    let stopped = stopping.finish();
    stopped.cleanup().expect("source cleanup");
    drop(tx); // signal EOF downstream
}

/// Tee thread: fan one value out to two workers via `builtin::Tee`.
fn tee_thread(rx: Receiver<i64>, tx_a: SyncSender<i64>, tx_b: SyncSender<i64>, blocked: &AtomicUsize) {
    let ctx = MachineContext::new("tee");
    let mut handle = MachineHandle::<Tee<i64>, Init>::new(ctx).expect("tee init").start();
    while let Ok(v) = rx.recv() {
        let out = handle.process(TeeInput::Input(v));
        match out {
            MultiOutput::YieldMulti(items) => {
                for item in items {
                    match item {
                        TeeOutput::OutputA(a) => blocking_send(&tx_a, a, blocked),
                        TeeOutput::OutputB(b) => blocking_send(&tx_b, b, blocked),
                    }
                }
            }
            _ => unreachable!("tee always yields multi"),
        }
    }
    let stopping = handle.stop();
    let stopped = stopping.finish();
    stopped.cleanup().expect("tee cleanup");
}

/// Worker thread (generic over the machine): recv → process → forward.
fn worker_thread<M>(rx: Receiver<i64>, tx: SyncSender<i64>, blocked: &AtomicUsize, slow: bool, active: &AtomicUsize, max_active: &AtomicUsize)
where
    M: Machine<Input = In<i64>, Output = Out<i64>, State = (), ProcessOutput = SingleOutput<Out<i64>>>,
{
    let ctx = MachineContext::new("worker");
    let mut handle = MachineHandle::<M, Init>::new(ctx).expect("worker init").start();
    let mut i: u64 = 0;
    while let Ok(v) = rx.recv() {
        // Track max concurrency to prove the two workers overlap.
        let cur = active.fetch_add(1, Ordering::SeqCst) + 1;
        max_active.fetch_max(cur, Ordering::SeqCst);
        let out = handle.process(In(v));
        if let SingleOutput::Yield(Out(w)) = out {
            blocking_send(&tx, w, blocked);
        }
        active.fetch_sub(1, Ordering::SeqCst);
        if slow {
            i += 1;
            if i % 200 == 0 {
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
        }
    }
    let stopping = handle.stop();
    let stopped = stopping.finish();
    stopped.cleanup().expect("worker cleanup");
}

/// Collector thread: drain the result channel into the machine state.
fn collector_thread(rx: Receiver<i64>) -> (u64, i64) {
    let ctx = MachineContext::new("collector");
    let mut handle = MachineHandle::<Collector, Init>::new(ctx).expect("collector init").start();
    while let Ok(v) = rx.recv() {
        handle.process(CollectorInput::value(v));
    }
    let (count, sum) = *handle.state();
    let stopping = handle.stop();
    let stopped = stopping.finish();
    stopped.cleanup().expect("collector cleanup");
    (count, sum)
}

// ════════════════════════════════════════════════════════════════════════════
// Run
// ════════════════════════════════════════════════════════════════════════════

fn main() {
    const N: usize = 20_000;
    const CHAN_CAP: usize = 16;

    println!("=== threaded_pipeline: Source → Tee → 2×Worker → Collector ===");
    println!("N = {}, channel capacity = {}\n", N, CHAN_CAP);

    use std::sync::Arc;

    let blocked_source = Arc::new(AtomicUsize::new(0));
    let blocked_tee = Arc::new(AtomicUsize::new(0));
    let blocked_workers = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));

    // Build the link topology with bounded channels (BlockPolicy semantics).
    let (src_tx, tee_rx) = sync_channel::<i64>(CHAN_CAP);
    let (tee_tx_a, work_a_rx) = sync_channel::<i64>(CHAN_CAP);
    let (tee_tx_b, work_b_rx) = sync_channel::<i64>(CHAN_CAP);
    let (a_tx, coll_rx_a) = sync_channel::<i64>(CHAN_CAP);
    let (b_tx, coll_rx_b) = sync_channel::<i64>(CHAN_CAP);

    let t0 = Instant::now();

    let h_src = {
        let b = Arc::clone(&blocked_source);
        std::thread::spawn(move || source_thread(N, src_tx, &*b))
    };
    let h_tee = {
        let b = Arc::clone(&blocked_tee);
        std::thread::spawn(move || tee_thread(tee_rx, tee_tx_a, tee_tx_b, &*b))
    };
    let h_a = {
        let b = Arc::clone(&blocked_workers);
        let act = Arc::clone(&active);
        let mx = Arc::clone(&max_active);
        std::thread::spawn(move || {
            worker_thread::<Doubler>(work_a_rx, a_tx, &*b, true, &*act, &*mx)
        })
    };
    let h_b = {
        let b = Arc::clone(&blocked_workers);
        let act = Arc::clone(&active);
        let mx = Arc::clone(&max_active);
        std::thread::spawn(move || {
            worker_thread::<Tripler>(work_b_rx, b_tx, &*b, true, &*act, &*mx)
        })
    };
    let h_coll_a = std::thread::spawn(move || collector_thread(coll_rx_a));
    let h_coll_b = std::thread::spawn(move || collector_thread(coll_rx_b));

    let (count_a, sum_a) = h_coll_a.join().expect("collector A");
    let (count_b, sum_b) = h_coll_b.join().expect("collector B");
    h_a.join().expect("worker A");
    h_b.join().expect("worker B");
    h_tee.join().expect("tee");
    h_src.join().expect("source");
    let dt = t0.elapsed();

    // ── Verification ────────────────────────────────────────────────────
    // Doubler path receives 1..=N, emits Σ2i; Tripler path Σ3i.
    let expect_sum_a: i64 = 2 * (N as i64) * (N as i64 + 1) / 2;
    let expect_sum_b: i64 = 3 * (N as i64) * (N as i64 + 1) / 2;

    println!("collector A : count = {} (expect {})  sum = {} (expect {})", count_a, N, sum_a, expect_sum_a);
    println!("collector B : count = {} (expect {})  sum = {} (expect {})", count_b, N, sum_b, expect_sum_b);

    let total_blocked =
        blocked_source.load(Ordering::Relaxed) + blocked_tee.load(Ordering::Relaxed) + blocked_workers.load(Ordering::Relaxed);
    let integrity_ok = count_a == N as u64 && count_b == N as u64 && sum_a == expect_sum_a && sum_b == expect_sum_b;
    let backpressure_ok = total_blocked > 0;
    let max_concurrent = max_active.load(Ordering::SeqCst);
    let parallelism_ok = max_concurrent > 1;

    println!("\n--- contract results ---");
    println!("1. data integrity (2N values, correct sums): {}", if integrity_ok { "✓" } else { "✗" });
    println!("2. backpressure occurred (blocked sends):    {}  (source={} tee={} workers={})",
        if backpressure_ok { "✓" } else { "✗" }, blocked_source.load(Ordering::Relaxed), blocked_tee.load(Ordering::Relaxed), blocked_workers.load(Ordering::Relaxed));
    println!("3. multi-instance parallelism (max concurrent): {}  {}", max_concurrent, if parallelism_ok { "✓" } else { "✗" });
    println!("4. per-instance &mut State serialised:        ✓ (no cross-thread sharing; type system)");
    println!("\nelapsed (incl. backpressure stalls): {:?}", dt);

    if !(integrity_ok && backpressure_ok && parallelism_ok) {
        std::process::exit(1);
    }
}
