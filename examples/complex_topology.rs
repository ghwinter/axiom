/// Complex topology example: multi-threaded, backpressure, control vs data blur.
///
/// Run: cargo run --example complex_topology

extern crate axiom;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, LazyLock};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use axiom::prelude_all::*;

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

#[derive(Debug, Clone, PartialEq)]
struct Aggregate {
    src: u32,
    samples: Vec<(SensorKind, f64)>,
    seq: u64,
}

// ════════════════════════════════════════════════════════════
// Port type helpers — one per Machine
// ════════════════════════════════════════════════════════════

// Sensor: input(Tick: u64), no data output (uses raw channel for data, observe for log)
mod sensor_ports {
    use axiom::portset::{HasPortInfo, PortSet};
    use axiom::port::{PortSchema, PortDecl};
    use axiom::flow::FlowKind;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Input {
        Tick(u64),
    }
    #[derive(Debug, Clone, PartialEq)]
    pub enum Output {}

    impl HasPortInfo for Input {
        fn port_name(&self) -> &'static str { match self { Self::Tick(_) => "tick" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Tick(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Tick(_) => core::any::TypeId::of::<u64>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Tick(_) => core::any::type_name::<u64>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "tick" => { let v: Box<u64> = payload.downcast().ok()?; Some(Self::Tick(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Tick(v) => Box::new(v) } }
    }
    impl HasPortInfo for Output {
        fn port_name(&self) -> &'static str { match *self {} }
        fn flow_kind(&self) -> FlowKind { match *self {} }
        fn payload_type_id(&self) -> core::any::TypeId { match *self {} }
        fn payload_type_name(&self) -> &'static str { match *self {} }
        fn from_port_name(_: &str, _: Box<dyn core::any::Any + Send>) -> Option<Self> { None }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self {} }
    }

    pub struct Ports;
    impl PortSet for Ports {
        type Input = Input;
        type Output = Output;
        fn port_schema() -> PortSchema {
            PortSchema::new().with(PortDecl::input::<u64>("tick"))
        }
    }
}

// Controller: input(Tick: u64), output(Out: Aggregate)
mod ctrl_ports {
    use axiom::portset::{HasPortInfo, PortSet};
    use axiom::port::{PortSchema, PortDecl};
    use axiom::flow::FlowKind;
    use crate::Aggregate;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Input {
        Tick(u64),
    }
    #[derive(Debug, Clone, PartialEq)]
    pub enum Output {
        Out(Aggregate),
    }

    impl HasPortInfo for Input {
        fn port_name(&self) -> &'static str { match self { Self::Tick(_) => "tick" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Tick(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Tick(_) => core::any::TypeId::of::<u64>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Tick(_) => core::any::type_name::<u64>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "tick" => { let v: Box<u64> = payload.downcast().ok()?; Some(Self::Tick(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Tick(v) => Box::new(v) } }
    }
    impl HasPortInfo for Output {
        fn port_name(&self) -> &'static str { match self { Self::Out(_) => "out" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Out(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Out(_) => core::any::TypeId::of::<Aggregate>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Out(_) => core::any::type_name::<Aggregate>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "out" => { let v: Box<Aggregate> = payload.downcast().ok()?; Some(Self::Out(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Out(v) => Box::new(v) } }
    }

    pub struct Ports;
    impl PortSet for Ports {
        type Input = Input;
        type Output = Output;
        fn port_schema() -> PortSchema {
            PortSchema::new()
                .with(PortDecl::input::<u64>("tick"))
                .with(PortDecl::output::<Aggregate>("out"))
        }
    }
}

// SafetyMonitor: input(In: Aggregate), output(Out: String)
mod safety_ports {
    use axiom::portset::{HasPortInfo, PortSet};
    use axiom::port::{PortSchema, PortDecl};
    use axiom::flow::FlowKind;
    use crate::Aggregate;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Input {
        In(Aggregate),
    }
    #[derive(Debug, Clone, PartialEq)]
    pub enum Output {
        Out(String),
    }

    impl HasPortInfo for Input {
        fn port_name(&self) -> &'static str { match self { Self::In(_) => "in" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::In(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::In(_) => core::any::TypeId::of::<Aggregate>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::In(_) => core::any::type_name::<Aggregate>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "in" => { let v: Box<Aggregate> = payload.downcast().ok()?; Some(Self::In(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::In(v) => Box::new(v) } }
    }
    impl HasPortInfo for Output {
        fn port_name(&self) -> &'static str { match self { Self::Out(_) => "out" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Out(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Out(_) => core::any::TypeId::of::<String>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Out(_) => core::any::type_name::<String>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "out" => { let v: Box<String> = payload.downcast().ok()?; Some(Self::Out(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Out(v) => Box::new(v) } }
    }

    pub struct Ports;
    impl PortSet for Ports {
        type Input = Input;
        type Output = Output;
        fn port_schema() -> PortSchema {
            PortSchema::new()
                .with(PortDecl::input::<Aggregate>("in"))
                .with(PortDecl::output::<String>("out"))
        }
    }
}

// PersistentStore: input(In: String), no data output
mod store_ports {
    use axiom::portset::{HasPortInfo, PortSet};
    use axiom::port::{PortSchema, PortDecl};
    use axiom::flow::FlowKind;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Input {
        In(String),
    }
    #[derive(Debug, Clone, PartialEq)]
    pub enum Output {}

    impl HasPortInfo for Input {
        fn port_name(&self) -> &'static str { match self { Self::In(_) => "in" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::In(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::In(_) => core::any::TypeId::of::<String>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::In(_) => core::any::type_name::<String>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "in" => { let v: Box<String> = payload.downcast().ok()?; Some(Self::In(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::In(v) => Box::new(v) } }
    }
    impl HasPortInfo for Output {
        fn port_name(&self) -> &'static str { match *self {} }
        fn flow_kind(&self) -> FlowKind { match *self {} }
        fn payload_type_id(&self) -> core::any::TypeId { match *self {} }
        fn payload_type_name(&self) -> &'static str { match *self {} }
        fn from_port_name(_: &str, _: Box<dyn core::any::Any + Send>) -> Option<Self> { None }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self {} }
    }

    pub struct Ports;
    impl PortSet for Ports {
        type Input = Input;
        type Output = Output;
        fn port_schema() -> PortSchema {
            PortSchema::new().with(PortDecl::input::<String>("in"))
        }
    }
}

// Reporter: input(Wake: u64), output(Report: String)
mod report_ports {
    use axiom::portset::{HasPortInfo, PortSet};
    use axiom::port::{PortSchema, PortDecl};
    use axiom::flow::FlowKind;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Input {
        Wake(u64),
    }
    #[derive(Debug, Clone, PartialEq)]
    pub enum Output {
        Report(String),
    }

    impl HasPortInfo for Input {
        fn port_name(&self) -> &'static str { match self { Self::Wake(_) => "wake" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Wake(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Wake(_) => core::any::TypeId::of::<u64>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Wake(_) => core::any::type_name::<u64>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "wake" => { let v: Box<u64> = payload.downcast().ok()?; Some(Self::Wake(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Wake(v) => Box::new(v) } }
    }
    impl HasPortInfo for Output {
        fn port_name(&self) -> &'static str { match self { Self::Report(_) => "report" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Report(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Report(_) => core::any::TypeId::of::<String>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Report(_) => core::any::type_name::<String>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "report" => { let v: Box<String> = payload.downcast().ok()?; Some(Self::Report(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Report(v) => Box::new(v) } }
    }

    pub struct Ports;
    impl PortSet for Ports {
        type Input = Input;
        type Output = Output;
        fn port_schema() -> PortSchema {
            PortSchema::new()
                .with(PortDecl::input::<u64>("wake"))
                .with(PortDecl::output::<String>("report"))
        }
    }
}

// ════════════════════════════════════════════════════════════
// Machine definitions
// ════════════════════════════════════════════════════════════

// ── Sensor ──────────────────────────────────────────────────

struct SensorState {
    kind: SensorKind,
    value: f64,
    seq: u64,
    output: Sender<Sample>,
    ctrl_in: Receiver<u64>,
    interval: u64,
}

struct Sensor;

impl Machine for Sensor {
    type State = SensorState;
    type Input = sensor_ports::Input;
    type Output = sensor_ports::Output;
    type Ports = sensor_ports::Ports;

    fn name() -> &'static str { "sensor" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SensorState, InitError> {
        let (_tx, rx) = mpsc::channel::<Sample>();
        let (_ctx, ctrl_rx) = mpsc::channel::<u64>();
        Ok(SensorState {
            kind: SensorKind::Temperature, value: 25.0, seq: 0,
            output: rx.try_recv().map(|_| unreachable!()).err().map(|_| {
                let (tx, _) = mpsc::channel::<Sample>(); tx
            }).unwrap_or_else(|| { let (tx, _) = mpsc::channel(); tx }),
            ctrl_in: ctrl_rx, interval: 100,
        })
    }

    fn process(s: &mut SensorState, _ctx: &MachineContext, input: sensor_ports::Input) -> ProcessOutput<sensor_ports::Output> {
        let _tick = match input { sensor_ports::Input::Tick(v) => v };
        s.seq += 1;
        match s.ctrl_in.try_recv() {
            Ok(v) => { s.interval = v; }
            Err(TryRecvError::Disconnected) => return ProcessOutput::Done,
            Err(TryRecvError::Empty) => {}
        }
        let noise = (s.seq as f64 * 0.1).sin() * 5.0;
        s.value = match s.kind {
            SensorKind::Temperature => 25.0 + noise + (s.seq as f64 % 7.0),
            SensorKind::Pressure => 1013.0 + noise * 2.0,
            SensorKind::Vibration => 0.5 + (noise * 0.1).abs(),
        };
        let _ = s.output.send(Sample { kind: s.kind, value: s.value, seq: s.seq });
        ProcessOutput::Idle
    }

    fn cleanup(_s: SensorState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ── Controller ──────────────────────────────────────────────

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
    type Input = ctrl_ports::Input;
    type Output = ctrl_ports::Output;
    type Ports = ctrl_ports::Ports;

    fn name() -> &'static str { "controller" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<CtrlState, InitError> {
        let (tx, _) = mpsc::channel();
        Ok(CtrlState { id: 0, inputs: vec![], output: tx,
            stop_flag: Arc::new(AtomicBool::new(false)), ctrl_out: None, seq: 0 })
    }

    fn process(s: &mut CtrlState, _ctx: &MachineContext, input: ctrl_ports::Input) -> ProcessOutput<ctrl_ports::Output> {
        let _tick = match input { ctrl_ports::Input::Tick(v) => v };
        s.seq += 1;
        if s.stop_flag.load(Ordering::Acquire) { return ProcessOutput::Idle; }

        let mut samples = Vec::new();
        for (kind, rx) in &s.inputs {
            match rx.try_recv() {
                Ok(sample) => samples.push((*kind, sample.value)),
                Err(_) => {}
            }
        }
        if samples.is_empty() { return ProcessOutput::Idle; }

        if s.id == 1 && s.seq % 3 == 0 {
            if let Some(ref ctrl) = s.ctrl_out {
                let new_interval = 50 + (s.seq % 5) * 30;
                let _ = ctrl.send(new_interval);
            }
        }
        ProcessOutput::Yield(ctrl_ports::Output::Out(Aggregate { src: s.id, samples, seq: s.seq }))
    }

    fn cleanup(_s: CtrlState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ── SafetyMonitor ───────────────────────────────────────────

struct SafetyState {
    stop_flags: Vec<Arc<AtomicBool>>,
    cycle: u64,
}

struct SafetyMonitor;

impl Machine for SafetyMonitor {
    type State = SafetyState;
    type Input = safety_ports::Input;
    type Output = safety_ports::Output;
    type Ports = safety_ports::Ports;

    fn name() -> &'static str { "safety" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SafetyState, InitError> {
        Ok(SafetyState { stop_flags: vec![], cycle: 0 })
    }

    fn process(s: &mut SafetyState, _ctx: &MachineContext, input: safety_ports::Input) -> ProcessOutput<safety_ports::Output> {
        let agg = match input { safety_ports::Input::In(v) => v };
        s.cycle += 1;
        for (kind, val) in &agg.samples {
            let danger = match kind {
                SensorKind::Temperature => *val > 50.0 || *val < -10.0,
                SensorKind::Pressure => *val > 1100.0 || *val < 900.0,
                SensorKind::Vibration => *val > 10.0,
            };
            if danger {
                for flag in &s.stop_flags { flag.store(true, Ordering::Release); }
                return ProcessOutput::Yield(safety_ports::Output::Out(format!(
                    "DANGER src={} {}={:.1}", agg.src,
                    match kind { SensorKind::Temperature => "T", SensorKind::Pressure => "P", SensorKind::Vibration => "V" },
                    val)));
            }
        }
        ProcessOutput::Idle
    }

    fn cleanup(_s: SafetyState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ── PersistentStore ─────────────────────────────────────────

static MEMDISK: LazyLock<Mutex<Vec<u8>>> = LazyLock::new(|| Mutex::new(Vec::with_capacity(65536)));

struct StoreState { buffer: Vec<String>, cycle: u64 }

struct PersistentStore;

impl Machine for PersistentStore {
    type State = StoreState;
    type Input = store_ports::Input;
    type Output = store_ports::Output;
    type Ports = store_ports::Ports;

    fn name() -> &'static str { "store" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<StoreState, InitError> {
        Ok(StoreState { buffer: vec![], cycle: 0 })
    }

    fn process(s: &mut StoreState, _ctx: &MachineContext, input: store_ports::Input) -> ProcessOutput<store_ports::Output> {
        let event = match input { store_ports::Input::In(v) => v };
        s.cycle += 1;
        s.buffer.push(event);
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

// ── Reporter ────────────────────────────────────────────────

struct RepState { offset: usize, total: usize }

struct Reporter;

impl Machine for Reporter {
    type State = RepState;
    type Input = report_ports::Input;
    type Output = report_ports::Output;
    type Ports = report_ports::Ports;

    fn name() -> &'static str { "reporter" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<RepState, InitError> {
        Ok(RepState { offset: 0, total: 0 })
    }

    fn process(s: &mut RepState, _ctx: &MachineContext, input: report_ports::Input) -> ProcessOutput<report_ports::Output> {
        let _wake = match input { report_ports::Input::Wake(v) => v };
        let disk = MEMDISK.lock().unwrap();
        let new_data = &disk[s.offset..];
        if new_data.is_empty() { return ProcessOutput::Idle; }
        let blocks = new_data.split(|&b| b == b'\n').filter(|b| !b.is_empty()).count();
        s.total += blocks;
        s.offset = disk.len();
        ProcessOutput::Yield(report_ports::Output::Report(format!(
            "report: {} new, {} total, disk={}KB", blocks, s.total, disk.len() / 1024)))
    }

    fn cleanup(_s: RepState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════
// Main — multi-threaded deployment
// ════════════════════════════════════════════════════════════

fn main() {
    println!("═══ axiom: complex topology ═══");
    println!("  Threads: 8  |  Channels: 9  |  Memory-disk: 64KB\n");

    let stop1 = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::new(AtomicBool::new(false));

    let (s1_tx, s1_rx) = mpsc::channel::<Sample>();
    let (s2_tx, s2_rx) = mpsc::channel::<Sample>();
    let (s3_tx, s3_rx) = mpsc::channel::<Sample>();
    let (c1_tx, c1_sm) = mpsc::channel::<Aggregate>();
    let (c2_tx, c2_sm) = mpsc::channel::<Aggregate>();
    let (obs_tx, obs_rx) = mpsc::channel::<String>();
    let (ctl_tx, ctl_rx) = mpsc::channel::<u64>();

    let ctx1 = MachineContext::new("sensor1");
    let mut s1 = Sensor::init(&ctx1).unwrap();
    s1.kind = SensorKind::Temperature; s1.output = s1_tx; s1.ctrl_in = ctl_rx;

    let ctx2 = MachineContext::new("sensor2");
    let mut s2 = Sensor::init(&ctx2).unwrap();
    s2.kind = SensorKind::Pressure; s2.output = s2_tx;

    let ctx3 = MachineContext::new("sensor3");
    let mut s3 = Sensor::init(&ctx3).unwrap();
    s3.kind = SensorKind::Vibration; s3.output = s3_tx;

    let ctx_c1 = MachineContext::new("ctrl1");
    let mut c1 = Controller::init(&ctx_c1).unwrap();
    c1.id = 1; c1.inputs = vec![(SensorKind::Temperature, s1_rx), (SensorKind::Pressure, s2_rx)];
    c1.output = c1_tx; c1.stop_flag = Arc::clone(&stop1); c1.ctrl_out = Some(ctl_tx);

    let ctx_c2 = MachineContext::new("ctrl2");
    let mut c2 = Controller::init(&ctx_c2).unwrap();
    c2.id = 2; c2.inputs = vec![(SensorKind::Vibration, s3_rx)];
    c2.output = c2_tx; c2.stop_flag = Arc::clone(&stop2);

    let ctx_sm = MachineContext::new("safety");
    let mut sm = SafetyMonitor::init(&ctx_sm).unwrap();
    sm.stop_flags = vec![Arc::clone(&stop1), Arc::clone(&stop2)];

    let ctx_store = MachineContext::new("store");
    let mut store = PersistentStore::init(&ctx_store).unwrap();

    let ctx_rep = MachineContext::new("reporter");
    let mut reporter = Reporter::init(&ctx_rep).unwrap();

    let tick = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::new();

    // Sensor threads
    {
        let t = Arc::clone(&tick);
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let _ = Sensor::process(&mut s1, &ctx1, sensor_ports::Input::Tick(t.fetch_add(1, Ordering::Relaxed)));
                thread::sleep(Duration::from_millis(10));
            }
            Sensor::cleanup(s1, &ctx1).ok();
        }));
    }
    {
        let t = Arc::clone(&tick);
        handles.push(thread::spawn(move || {
            for _ in 0..80 {
                let _ = Sensor::process(&mut s2, &ctx2, sensor_ports::Input::Tick(t.fetch_add(1, Ordering::Relaxed)));
                thread::sleep(Duration::from_millis(15));
            }
            Sensor::cleanup(s2, &ctx2).ok();
        }));
    }
    {
        let t = Arc::clone(&tick);
        handles.push(thread::spawn(move || {
            for _ in 0..30 {
                let _ = Sensor::process(&mut s3, &ctx3, sensor_ports::Input::Tick(t.fetch_add(1, Ordering::Relaxed)));
                thread::sleep(Duration::from_millis(40));
            }
            Sensor::cleanup(s3, &ctx3).ok();
        }));
    }

    // Controller threads
    {
        let obs = obs_tx.clone();
        handles.push(thread::spawn(move || {
            for i in 0..120 {
                match Controller::process(&mut c1, &ctx_c1, ctrl_ports::Input::Tick(i)) {
                    ProcessOutput::Yield(out) => { let ctrl_ports::Output::Out(agg) = out; let _ = c1.output.send(agg); }
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
                match Controller::process(&mut c2, &ctx_c2, ctrl_ports::Input::Tick(i)) {
                    ProcessOutput::Yield(out) => { let ctrl_ports::Output::Out(agg) = out; let _ = c2.output.send(agg); }
                    _ => {}
                }
                let _ = obs.send(format!("C2:{}", i));
                thread::sleep(Duration::from_millis(25));
            }
            Controller::cleanup(c2, &ctx_c2).ok();
        }));
    }

    // Safety monitor
    handles.push(thread::spawn(move || {
        for _ in 0..100 {
            for rx in [&c1_sm, &c2_sm] {
                if let Ok(agg) = rx.try_recv() {
                    match SafetyMonitor::process(&mut sm, &ctx_sm, safety_ports::Input::In(agg)) {
                        ProcessOutput::Yield(alert) => eprintln!("[safety] {}", match alert { safety_ports::Output::Out(s) => s }),
                        _ => {}
                    }
                }
            }
            thread::sleep(Duration::from_millis(30));
        }
        SafetyMonitor::cleanup(sm, &ctx_sm).ok();
    }));

    // Store
    handles.push(thread::spawn(move || {
        for _ in 0..200 {
            match obs_rx.try_recv() {
                Ok(event) => { let _ = PersistentStore::process(&mut store, &ctx_store, store_ports::Input::In(event)); }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            thread::sleep(Duration::from_millis(10));
        }
        PersistentStore::cleanup(store, &ctx_store).ok();
    }));

    // Reporter
    handles.push(thread::spawn(move || {
        for i in 0..50 {
            match Reporter::process(&mut reporter, &ctx_rep, report_ports::Input::Wake(i)) {
                ProcessOutput::Yield(r) => println!("[reporter] {}", match r { report_ports::Output::Report(s) => s }),
                _ => {}
            }
            thread::sleep(Duration::from_millis(50));
        }
        Reporter::cleanup(reporter, &ctx_rep).ok();
    }));

    for (i, h) in handles.into_iter().enumerate() {
        h.join().expect(&format!("thread {} failed", i));
    }

    let disk = MEMDISK.lock().unwrap();
    let blocks = disk.split(|&b| b == b'\n').filter(|b| !b.is_empty()).count();
    println!("\n═══ shutdown ═══");
    println!("  disk blocks={}  size={}B ({}KB)", blocks, disk.len(), disk.len() / 1024);
    println!("\n  Observations:");
    println!("  1. Each module's output buffer is a field in its State.");
    println!("  2. Control signals use the same channel mechanism as data.");
    println!("  3. SafetyMonitor's `flag.store(true)` and Controller1's `ctrl.send(interval)` are BOTH channel writes — physically identical, semantically distinct.");
    println!("  4. PersistentStore targets in-memory Vec<u8>, not disk files.");
    println!("  5. Backpressure: full mpsc channels cause send() to block, slowing the producer naturally.");
}
