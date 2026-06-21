/// Complex topology example: multi-threaded, backpressure, control vs data blur.
///
/// # Scenario: "Factory Floor Telemetry"
///
/// ## Modules
/// - **Sensor1/2/3**: generate data at different rates, each owns its output buffer
/// - **Controller1/2**: consume sensor data, produce aggregated output and control
/// - **SafetyMonitor**: consumes all controller streams, broadcasts emergency stop
/// - **PersistentStore**: snapshots all observe events to in-memory "disk"
/// - **Reporter**: reads store, produces summaries
///
/// ## Topology
///
/// ```text
/// Sensor1 ──[data]──▶ Controller1 ──[data]──▶ SafetyMonitor
/// Sensor2 ──[data]──▶ Controller1              │
/// Sensor3 ──[data]──▶ Controller2              │
///                  Controller1 ──[ctrl]──▶ Sensor1
///                  SafetyMonitor ──[ctrl]──▶ Controller1, Controller2
///                  ALL ──[observe]──▶ PersistentStore ──[data]──▶ Reporter
/// ```
///
/// ## Key demonstrations
///
/// 1. **Buffer ownership**: each Sensor's output channel is a field in its State.
///    The channel belongs to the producer structurally, even though the consumer reads it.
///
/// 2. **Control is data**: SafetyMonitor's "emergency stop" and Controller1's
///    "change sampling rate" use the same channel mechanism as data transfers.
///    The distinction is semantic — the receiving module interprets the value differently.
///
/// 3. **Boundary blur**: Controller1 both reads from Sensor1's buffer AND writes to
///    Sensor1's control channel. Physically, both are memory writes. The "boundary"
///    between modules is a convention enforced by which State fields each process()
///    touches, not a physical barrier.
///
/// 4. **In-memory persistence**: uses `std::sync::LazyLock<Mutex<Vec<u8>>>` as "disk".
///    No files. Machine::checkpoint targets memory, not a filesystem.
///
/// 5. **Multi-thread**: 8 OS threads, each with independent event loops.
///    Different modules run at different rates (10ms, 15ms, 40ms, etc.).
///    Channel capacity creates backpressure naturally.
///
/// Run: cargo run --example complex_topology

extern crate axiom;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, LazyLock};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use axiom::prelude_all::*;
use axiom::machine::{ProcessOutput, InitError, CleanupError};
use axiom::port::MachineContext;

// ════════════════════════════════════════════════════════════
// Shared types
// ════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq)]
enum SensorKind { Temperature, Pressure, Vibration }

#[derive(Debug, Clone)]
struct Sample {
    kind: SensorKind,
    value: f64,
    seq: u64,
}

#[derive(Debug, Clone)]
struct Aggregate {
    src: u32,
    samples: Vec<(SensorKind, f64)>,
    seq: u64,
}

// ════════════════════════════════════════════════════════════
// Modules
// ════════════════════════════════════════════════════════════

// ── Sensor ─────────────────────────────────────────────────

struct SensorState {
    kind: SensorKind,
    value: f64,
    seq: u64,
    /// Output buffer: owned by this module. Consumer reads from it.
    output: Sender<Sample>,
    /// Control input: external module writes here, we read.
    ctrl_in: Receiver<u64>,
    interval: u64,
}

struct Sensor;

impl Machine for Sensor {
    type State = SensorState;
    type Input = u64;
    type Output = ();


    fn name() -> &'static str { "sensor" }
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<u64>("tick"))
            .with(PortDecl::observe::<String>("log"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SensorState, InitError> {
        let (_tx, rx) = mpsc::channel::<Sample>();
        let (_ctx, ctrl_rx) = mpsc::channel::<u64>();
        Ok(SensorState {
            kind: SensorKind::Temperature,
            value: 25.0, seq: 0,
            output: rx.try_recv().map(|_| unreachable!()).err().map(|_| {
                let (tx, _) = mpsc::channel::<Sample>();
                tx
            }).unwrap_or_else(|| { let (tx, _) = mpsc::channel(); tx }),
            ctrl_in: ctrl_rx,
            interval: 100,
        })
    }

    fn process(s: &mut SensorState, _ctx: &MachineContext, _tick: u64) -> ProcessOutput<()> {
        s.seq += 1;

        // Check control channel: did someone command a new interval?
        match s.ctrl_in.try_recv() {
            Ok(v) => { s.interval = v; }
            Err(TryRecvError::Disconnected) => return ProcessOutput::Done,
            Err(TryRecvError::Empty) => {}
        }

        // Simulate reading
        let noise = (s.seq as f64 * 0.1).sin() * 5.0;
        s.value = match s.kind {
            SensorKind::Temperature => 25.0 + noise + (s.seq as f64 % 7.0),
            SensorKind::Pressure => 1013.0 + noise * 2.0,
            SensorKind::Vibration => 0.5 + (noise * 0.1).abs(),
        };

        let _ = s.output.send(Sample {
            kind: s.kind, value: s.value, seq: s.seq,
        });

        ProcessOutput::Idle
    }

    fn cleanup(_s: SensorState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ── Controller ─────────────────────────────────────────────

struct CtrlState {
    id: u32,
    inputs: Vec<(SensorKind, Receiver<Sample>)>,
    output: Sender<Aggregate>,
    stop_flag: Arc<AtomicBool>,
    ctrl_out: Option<Sender<u64>>,
    seq: u64,
}

struct Controller;

impl Machine for Controller {
    type State = CtrlState;
    type Input = u64;
    type Output = Aggregate;


    fn name() -> &'static str { "controller" }
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<u64>("tick"))
            .with(PortDecl::output::<Aggregate>("out"))
            .with(PortDecl::observe::<String>("log"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<CtrlState, InitError> {
        let (tx, _) = mpsc::channel();
        Ok(CtrlState {
            id: 0, inputs: vec![], output: tx,
            stop_flag: Arc::new(AtomicBool::new(false)),
            ctrl_out: None, seq: 0,
        })
    }

    fn process(s: &mut CtrlState, _ctx: &MachineContext, _tick: u64) -> ProcessOutput<Aggregate> {
        s.seq += 1;

        // Stop flag overrides everything (control from SafetyMonitor).
        if s.stop_flag.load(Ordering::Acquire) {
            return ProcessOutput::Idle;
        }

        // Read from all input buffers (data from sensors).
        let mut samples = Vec::new();
        for (kind, rx) in &s.inputs {
            match rx.try_recv() {
                Ok(sample) => samples.push((*kind, sample.value)),
                Err(_) => {}
            }
        }
        if samples.is_empty() {
            return ProcessOutput::Idle;
        }

        // Every 3 cycles, send control signal to sensor1.
        if s.id == 1 && s.seq % 3 == 0 {
            if let Some(ref ctrl) = s.ctrl_out {
                // SAME physical operation as data send — different semantic label.
                let new_interval = 50 + (s.seq % 5) * 30;
                let _ = ctrl.send(new_interval);
            }
        }

        ProcessOutput::Yield(Aggregate { src: s.id, samples, seq: s.seq })
    }

    fn cleanup(_s: CtrlState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ── SafetyMonitor ──────────────────────────────────────────

struct SafetyState {
    stop_flags: Vec<Arc<AtomicBool>>,
    cycle: u64,
}

struct SafetyMonitor;

impl Machine for SafetyMonitor {
    type State = SafetyState;
    type Input = Aggregate;
    type Output = String;


    fn name() -> &'static str { "safety" }
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<Aggregate>("in"))
            .with(PortDecl::output::<String>("out"))
            .with(PortDecl::observe::<String>("log"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SafetyState, InitError> {
        Ok(SafetyState { stop_flags: vec![], cycle: 0 })
    }

    fn process(s: &mut SafetyState, _ctx: &MachineContext, agg: Aggregate) -> ProcessOutput<String> {
        s.cycle += 1;

        for (kind, val) in &agg.samples {
            let danger = match kind {
                SensorKind::Temperature => *val > 50.0 || *val < -10.0,
                SensorKind::Pressure => *val > 1100.0 || *val < 900.0,
                SensorKind::Vibration => *val > 10.0,
            };
            if danger {
                for flag in &s.stop_flags {
                    flag.store(true, Ordering::Release);
                }
                return ProcessOutput::Yield(format!(
                    "DANGER src={} {}={:.1}", agg.src,
                    match kind { SensorKind::Temperature => "T", SensorKind::Pressure => "P", SensorKind::Vibration => "V" },
                    val));
            }
        }
        ProcessOutput::Idle
    }

    fn cleanup(_s: SafetyState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ── PersistentStore (in-memory "disk") ─────────────────────

/// In-memory disk: a byte vector behind a Mutex. No files.
static MEMDISK: LazyLock<Mutex<Vec<u8>>> = LazyLock::new(|| Mutex::new(Vec::with_capacity(65536)));

struct StoreState {
    buffer: Vec<String>,
    cycle: u64,
}

struct PersistentStore;

impl Machine for PersistentStore {
    type State = StoreState;
    type Input = String;
    type Output = ();


    fn name() -> &'static str { "store" }
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<String>("in"))
            .with(PortDecl::observe::<String>("log"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<StoreState, InitError> {
        Ok(StoreState { buffer: vec![], cycle: 0 })
    }

    fn process(s: &mut StoreState, _ctx: &MachineContext, event: String) -> ProcessOutput<()> {
        s.cycle += 1;
        s.buffer.push(event);

        // Flush every 10 events to in-memory disk.
        if s.buffer.len() >= 10 {
            let data: Vec<u8> = s.buffer.join("\n").into_bytes();
            let mut disk = MEMDISK.lock().unwrap();
            let header = format!("BLOCK:{:08x}:{}:", disk.len(), data.len());
            disk.extend_from_slice(header.as_bytes());
            disk.extend_from_slice(&data);
            disk.push(b'\n');
            s.buffer.clear();
        }
        ProcessOutput::Idle
    }

    fn cleanup(s: StoreState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        if !s.buffer.is_empty() {
            let data: Vec<u8> = s.buffer.join("\n").into_bytes();
            let mut disk = MEMDISK.lock().unwrap();
            let header = format!("FINAL:{:08x}:{}:", disk.len(), data.len());
            disk.extend_from_slice(header.as_bytes());
            disk.extend_from_slice(&data);
            disk.push(b'\n');
        }
        Ok(())
    }
}

// ── Reporter ───────────────────────────────────────────────

struct RepState { offset: usize, total: usize }

struct Reporter;

impl Machine for Reporter {
    type State = RepState;
    type Input = u64;
    type Output = String;


    fn name() -> &'static str { "reporter" }
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<u64>("wake"))
            .with(PortDecl::output::<String>("report"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<RepState, InitError> {
        Ok(RepState { offset: 0, total: 0 })
    }

    fn process(s: &mut RepState, _ctx: &MachineContext, _wake: u64) -> ProcessOutput<String> {
        let disk = MEMDISK.lock().unwrap();
        let new_data = &disk[s.offset..];
        if new_data.is_empty() {
            return ProcessOutput::Idle;
        }
        let blocks = new_data.split(|&b| b == b'\n').filter(|b| !b.is_empty()).count();
        s.total += blocks;
        s.offset = disk.len();
        ProcessOutput::Yield(format!("report: {} new, {} total, disk={}KB",
            blocks, s.total, disk.len() / 1024))
    }

    fn cleanup(_s: RepState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════
// Multi-threaded deployment
// ════════════════════════════════════════════════════════════

fn main() {
    println!("═══ axiom: complex topology ═══");
    println!("  Threads: 8  |  Channels: 9  |  Memory-disk: 64KB\n");

    // ── Allocate shared control signals ────────────────────
    let stop1 = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::new(AtomicBool::new(false));

    // ── Allocate channels (both data and control) ──────────
    // Data: sensor → controller
    let (s1_tx, s1_rx) = mpsc::channel::<Sample>();
    let (s2_tx, s2_rx) = mpsc::channel::<Sample>();
    let (s3_tx, s3_rx) = mpsc::channel::<Sample>();
    // Data: controller → safety
    let (c1_tx, c1_sm) = mpsc::channel::<Aggregate>();
    let (c2_tx, c2_sm) = mpsc::channel::<Aggregate>();
    // Data: observe events → store
    let (obs_tx, obs_rx) = mpsc::channel::<String>();
    // Control: controller1 → sensor1 (changes sampling interval)
    let (ctl_tx, ctl_rx) = mpsc::channel::<u64>();

    // ── Init all modules (shared State on heap) ────────────

    let ctx1 = MachineContext::new("sensor1");
    let mut s1 = Sensor::init(&ctx1).unwrap();
    s1.kind = SensorKind::Temperature;
    s1.output = s1_tx;
    s1.ctrl_in = ctl_rx;  // ← receives control from controller1

    let ctx2 = MachineContext::new("sensor2");
    let mut s2 = Sensor::init(&ctx2).unwrap();
    s2.kind = SensorKind::Pressure;
    s2.output = s2_tx;

    let ctx3 = MachineContext::new("sensor3");
    let mut s3 = Sensor::init(&ctx3).unwrap();
    s3.kind = SensorKind::Vibration;
    s3.output = s3_tx;

    let ctx_c1 = MachineContext::new("ctrl1");
    let mut c1 = Controller::init(&ctx_c1).unwrap();
    c1.id = 1;
    c1.inputs = vec![(SensorKind::Temperature, s1_rx), (SensorKind::Pressure, s2_rx)];
    c1.output = c1_tx;
    c1.stop_flag = Arc::clone(&stop1);
    c1.ctrl_out = Some(ctl_tx);  // ← sends control to sensor1

    let ctx_c2 = MachineContext::new("ctrl2");
    let mut c2 = Controller::init(&ctx_c2).unwrap();
    c2.id = 2;
    c2.inputs = vec![(SensorKind::Vibration, s3_rx)];
    c2.output = c2_tx;
    c2.stop_flag = Arc::clone(&stop2);

    let ctx_sm = MachineContext::new("safety");
    let mut sm = SafetyMonitor::init(&ctx_sm).unwrap();
    sm.stop_flags = vec![Arc::clone(&stop1), Arc::clone(&stop2)];

    let ctx_store = MachineContext::new("store");
    let mut store = PersistentStore::init(&ctx_store).unwrap();

    let ctx_rep = MachineContext::new("reporter");
    let mut reporter = Reporter::init(&ctx_rep).unwrap();

    let tick = Arc::new(AtomicU64::new(0));

    // ── Spawn 8 threads ────────────────────────────────────
    let mut handles = Vec::new();

    // Sensor threads: each runs at its own rate
    {
        let t = Arc::clone(&tick);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let _ = Sensor::process(&mut s1, &ctx1, t.fetch_add(1, Ordering::Relaxed));
                thread::sleep(Duration::from_millis(10));
            }
            Sensor::cleanup(s1, &ctx1).ok();
        }));
    }
    {
        let t = Arc::clone(&tick);
        handles.push(thread::spawn(move || {
            for _ in 0..80 {
                let _ = Sensor::process(&mut s2, &ctx2, t.fetch_add(1, Ordering::Relaxed));
                thread::sleep(Duration::from_millis(15));
            }
            Sensor::cleanup(s2, &ctx2).ok();
        }));
    }
    {
        let t = Arc::clone(&tick);
        handles.push(thread::spawn(move || {
            for _ in 0..30 {
                let _ = Sensor::process(&mut s3, &ctx3, t.fetch_add(1, Ordering::Relaxed));
                thread::sleep(Duration::from_millis(40));
            }
            Sensor::cleanup(s3, &ctx3).ok();
        }));
    }

    // Controller threads: read from sensors, write to safety monitor + observe events
    {
        let obs = obs_tx.clone();
        handles.push(thread::spawn(move || {
            for i in 0..120 {
                match Controller::process(&mut c1, &ctx_c1, i) {
                    ProcessOutput::Yield(agg) => { let _ = c1.output.send(agg); }
                    _ => {}
                }
                let _ = obs.send(format!("C1:{}", i));
                thread::sleep(Duration::from_millis(20));
            }
            Controller::cleanup(c1, &ctx_c1).ok();
        }));
    }
    {
        let obs = obs_tx.clone();
        handles.push(thread::spawn(move || {
            for i in 0..120 {
                match Controller::process(&mut c2, &ctx_c2, i) {
                    ProcessOutput::Yield(agg) => { let _ = c2.output.send(agg); }
                    _ => {}
                }
                let _ = obs.send(format!("C2:{}", i));
                thread::sleep(Duration::from_millis(25));
            }
            Controller::cleanup(c2, &ctx_c2).ok();
        }));
    }

    // Safety monitor thread
    handles.push(thread::spawn(move || {
        for _ in 0..100 {
            for rx in [&c1_sm, &c2_sm] {
                if let Ok(agg) = rx.try_recv() {
                    match SafetyMonitor::process(&mut sm, &ctx_sm, agg) {
                        ProcessOutput::Yield(alert) => eprintln!("[safety] {}", alert),
                        _ => {}
                    }
                }
            }
            thread::sleep(Duration::from_millis(30));
        }
        SafetyMonitor::cleanup(sm, &ctx_sm).ok();
    }));

    // Store thread: consumes observe events
    handles.push(thread::spawn(move || {
        for _ in 0..200 {
            match obs_rx.try_recv() {
                Ok(event) => { let _ = PersistentStore::process(&mut store, &ctx_store, event); }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            thread::sleep(Duration::from_millis(10));
        }
        PersistentStore::cleanup(store, &ctx_store).ok();
    }));

    // Reporter thread
    handles.push(thread::spawn(move || {
        for i in 0..50 {
            match Reporter::process(&mut reporter, &ctx_rep, i) {
                ProcessOutput::Yield(r) => println!("[reporter] {}", r),
                _ => {}
            }
            thread::sleep(Duration::from_millis(50));
        }
        Reporter::cleanup(reporter, &ctx_rep).ok();
    }));

    // ── Join all threads ───────────────────────────────────
    for (i, h) in handles.into_iter().enumerate() {
        h.join().expect(&format!("thread {} failed", i));
    }

    // ── Final report ───────────────────────────────────────
    let disk = MEMDISK.lock().unwrap();
    let blocks = disk.split(|&b| b == b'\n').filter(|b| !b.is_empty()).count();
    println!("\n═══ shutdown ═══");
    println!("  disk blocks={}  size={}B ({}KB)", blocks, disk.len(), disk.len() / 1024);
    println!("\n  Observations:");
    println!("  1. Each module's output buffer is a field in its State.");
    println!("  2. Control signals use the same channel mechanism as data.");
    println!("  3. SafetyMonitor's `flag.store(true)` and Controller1's");
    println!("     `ctrl.send(interval)` are BOTH channel writes —");
    println!("     physically identical, semantically distinct.");
    println!("  4. PersistentStore targets in-memory Vec<u8>, not disk files.");
    println!("  5. Backpressure: full mpsc channels cause send() to block,");
    println!("     slowing the producer naturally.");
}
