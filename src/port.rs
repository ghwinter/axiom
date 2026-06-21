use core::any::TypeId;
use core::sync::atomic::{AtomicUsize, AtomicU8, AtomicU64, Ordering};
use std::sync::Arc;

use crate::flow::FlowKind;

// ── Port direction ────────────────────────────────────────────────────────────

/// The direction of data flow through a port.
///
/// Direction is orthogonal to [`FlowKind`]: an output port can carry data,
/// control, or observation information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDir {
    /// Data flows into the entity.
    In,
    /// Data flows out of the entity.
    Out,
}

// ── Port declaration ─────────────────────────────────────────────────────────

/// A single port declaration in an entity's port schema.
///
/// Three orthogonal dimensions:
/// - **Direction** (`PortDir`): in or out.
/// - **Semantic kind** (`FlowKind`): data, control, or observation.
/// - **Type**: the Rust type of data crossing this port.
#[derive(Debug, Clone)]
pub struct PortDecl {
    pub name: &'static str,
    pub dir: PortDir,
    pub flow: FlowKind,
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub schema_ver: u32,
    pub description: &'static str,
}

impl PortDecl {
    // ── Data ports ─────────────────────────────────────────

    pub fn input<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::In, FlowKind::Data)
    }

    pub fn output<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::Out, FlowKind::Data)
    }

    // ── Control ports ──────────────────────────────────────

    pub fn ctrl_in<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::In, FlowKind::Control)
    }

    pub fn ctrl_out<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::Out, FlowKind::Control)
    }

    // ── Observation ports ──────────────────────────────────

    pub fn observe<T: 'static>(name: &'static str) -> Self {
        Self::new::<T>(name, PortDir::Out, FlowKind::Observe)
    }

    // ── Generic constructor ────────────────────────────────

    pub fn new<T: 'static>(name: &'static str, dir: PortDir, flow: FlowKind) -> Self {
        Self {
            name,
            dir,
            flow,
            type_id: TypeId::of::<T>(),
            type_name: core::any::type_name::<T>(),
            schema_ver: 0,
            description: "",
        }
    }

    pub fn with_schema_ver(mut self, ver: u32) -> Self {
        self.schema_ver = ver;
        self
    }

    pub fn with_description(mut self, desc: &'static str) -> Self {
        self.description = desc;
        self
    }
}

// ── Port schema ───────────────────────────────────────────────────────────────

/// The complete set of ports an entity exposes.
#[derive(Debug, Clone)]
pub struct PortSchema {
    ports: Vec<PortDecl>,
    // Cached indices for fast lookup.
    primary_in: Option<usize>,
    primary_out: Option<usize>,
    observe_out: Option<usize>,
}

impl PortSchema {
    pub fn new() -> Self {
        Self {
            ports: Vec::new(),
            primary_in: None,
            primary_out: None,
            observe_out: None,
        }
    }

    pub fn with(mut self, decl: PortDecl) -> Self {
        // Definition 2.2: Γ is a *set* — no duplicate port names allowed.
        // This is a programming error (static declaration), so panic early.
        if let Some(existing) = self.ports.iter().find(|p| p.name == decl.name) {
            panic!(
                "PortSchema duplicate port name '{}': existing {:?}:{:?}, new {:?}:{:?}",
                decl.name, existing.dir, existing.flow, decl.dir, decl.flow
            );
        }
        let idx = self.ports.len();
        match (decl.dir, &decl.flow) {
            (PortDir::In, FlowKind::Data) if self.primary_in.is_none() => {
                self.primary_in = Some(idx);
            }
            (PortDir::Out, FlowKind::Data) if self.primary_out.is_none() => {
                self.primary_out = Some(idx);
            }
            (PortDir::Out, FlowKind::Observe) if self.observe_out.is_none() => {
                self.observe_out = Some(idx);
            }
            _ => {}
        }
        self.ports.push(decl);
        self
    }

    pub fn ports(&self) -> &[PortDecl] { &self.ports }
    pub fn is_empty(&self) -> bool { self.ports.is_empty() }
    pub fn len(&self) -> usize { self.ports.len() }

    pub fn find(&self, name: &str) -> Option<&PortDecl> {
        self.ports.iter().find(|p| p.name == name)
    }

    /// All input ports (any flow kind).
    pub fn inputs(&self) -> impl Iterator<Item = &PortDecl> {
        self.ports.iter().filter(|p| p.dir == PortDir::In)
    }

    /// All output ports (any flow kind).
    pub fn outputs(&self) -> impl Iterator<Item = &PortDecl> {
        self.ports.iter().filter(|p| p.dir == PortDir::Out)
    }

    /// All observe ports (output + Observe flow).
    pub fn observe_ports(&self) -> impl Iterator<Item = &PortDecl> {
        self.ports.iter().filter(|p| p.dir == PortDir::Out && p.flow == FlowKind::Observe)
    }

    /// Validate that this schema satisfies the mathematical definition of an
    /// interface set (Definition 2.2): no duplicate names, each port has a
    /// valid type. Returns `Ok(())` if valid, `Err(reason)` otherwise.
    pub fn validate(&self) -> Result<(), &'static str> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.ports {
            if !seen.insert(p.name) {
                return Err("duplicate port name in schema");
            }
        }
        Ok(())
    }

    pub fn primary_input(&self) -> Option<&PortDecl> {
        self.primary_in.map(|i| &self.ports[i])
    }

    pub fn primary_output(&self) -> Option<&PortDecl> {
        self.primary_out.map(|i| &self.ports[i])
    }

    pub fn observe_port(&self) -> Option<&PortDecl> {
        self.observe_out.map(|i| &self.ports[i])
    }
}

// ── Link-compatibility check ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkCompat {
    Compatible,
    Migrate { from_ver: u32, to_ver: u32 },
    Incompatible { reason: &'static str },
}

impl PortDecl {
    pub fn can_link_to(&self, other: &PortDecl) -> LinkCompat {
        if self.dir != PortDir::Out {
            return LinkCompat::Incompatible { reason: "source port is not an output" };
        }
        if other.dir != PortDir::In {
            return LinkCompat::Incompatible { reason: "target port is not an input" };
        }
        if self.type_id != other.type_id {
            return LinkCompat::Incompatible { reason: "type mismatch" };
        }
        // FlowKind must match (Data↔Data, Control↔Control, Observe↔In ports don't connect)
        if self.flow != other.flow {
            return LinkCompat::Incompatible { reason: "flow kind mismatch" };
        }
        let ver_diff = if self.schema_ver > other.schema_ver {
            self.schema_ver - other.schema_ver
        } else {
            other.schema_ver - self.schema_ver
        };
        match ver_diff {
            0 => LinkCompat::Compatible,
            1 => LinkCompat::Migrate {
                from_ver: self.schema_ver.min(other.schema_ver),
                to_ver: self.schema_ver.max(other.schema_ver),
            },
            _ => LinkCompat::Incompatible { reason: "schema version drift > 1" },
        }
    }
}

// ── Port registry (runtime) ───────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct PortRegistry {
    entries: Vec<PortEntry>,
}

#[derive(Debug)]
pub struct PortEntry {
    pub name: &'static str,
    pub dir: PortDir,
    pub flow: FlowKind,
    pub type_name: &'static str,
    pub schema_ver: u32,
}

impl PortRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, decl: &PortDecl) {
        self.entries.push(PortEntry {
            name: decl.name,
            dir: decl.dir,
            flow: decl.flow,
            type_name: decl.type_name,
            schema_ver: decl.schema_ver,
        });
    }

    pub fn entries(&self) -> &[PortEntry] { &self.entries }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

// ── Config schema ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ConfigDecl {
    pub key: &'static str,
    pub type_name: &'static str,
    pub description: &'static str,
}

impl ConfigDecl {
    pub fn new<T: 'static>(key: &'static str, description: &'static str) -> Self {
        Self {
            key,
            type_name: core::any::type_name::<T>(),
            description,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConfigSchema {
    decls: Vec<ConfigDecl>,
}

impl ConfigSchema {
    pub fn new() -> Self { Self::default() }
    pub fn with(mut self, decl: ConfigDecl) -> Self {
        self.decls.push(decl);
        self
    }
    pub fn decls(&self) -> &[ConfigDecl] { &self.decls }
}

// ── MachineContext ────────────────────────────────────────────────────────────

/// Context provided to a Machine during its lifecycle.
///
/// Carries observation detection, output connection tracking,
/// snapshot capabilities, lifecycle state, signal polling, time access,
/// and initial value injection (工程修补 7.5.4).
pub struct MachineContext {
    pub name: &'static str,
    /// Number of active consumers on observation ports.
    pub(crate) observe_count: Arc<AtomicUsize>,
    /// Number of active consumers on data/control output ports.
    /// Machines can query this to skip expensive computation when
    /// nobody is listening (Theorem 7.2 — observability requires links).
    pub(crate) output_count: Arc<AtomicUsize>,
    /// Snapshot function (wired by runtime).
    pub(crate) snapshot_fn: Option<Arc<dyn Fn() -> Option<Vec<u8>> + Send + Sync>>,
    /// Initial value injection for Source-like machines (工程修补 7.5.4).
    /// Stored as type-erased `Arc<dyn Any + Send + Sync>`; machines downcast in `init()`.
    pub(crate) initial_value: Option<Arc<dyn core::any::Any + Send + Sync>>,
    /// Current lifecycle phase (set by runtime).
    lifecycle: AtomicU8,
    /// Pending system signals count (inc by runtime, polled by machine).
    signal_flag: AtomicU8,
    /// Current time in ms since epoch (set by runtime each tick).
    time_ms: AtomicU64,
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

/// The phase a Machine is currently in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Init     = 0,
    Running  = 1,
    Stopping = 2,
    Stopped  = 3,
}

impl Lifecycle {
    /// Progress to the next phase (monotonic forward only).
    pub fn next(self) -> Option<Self> {
        match self {
            Lifecycle::Init => Some(Lifecycle::Running),
            Lifecycle::Running => Some(Lifecycle::Stopping),
            Lifecycle::Stopping => Some(Lifecycle::Stopped),
            Lifecycle::Stopped => None,
        }
    }
    pub fn is_active(self) -> bool { self == Lifecycle::Running }
    pub fn is_terminal(self) -> bool { self == Lifecycle::Stopped }
}

// ── SystemSignal ──────────────────────────────────────────────────────────────

/// A signal sent from the runtime to a Machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemSignal {
    /// Request graceful shutdown after current process() completes.
    Shutdown,
    /// Request a state checkpoint (if supported).
    Checkpoint,
}

impl core::fmt::Debug for MachineContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MachineContext")
            .field("name", &self.name)
            .field("observe_count", &self.observe_count.load(Ordering::Relaxed))
            .field("output_count", &self.output_count.load(Ordering::Relaxed))
            .field("has_snapshot_fn", &self.snapshot_fn.is_some())
            .field("lifecycle", &self.lifecycle())
            .field("time_ms", &self.time_ms.load(Ordering::Relaxed))
            .finish()
    }
}

impl MachineContext {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            observe_count: Arc::new(AtomicUsize::new(0)),
            output_count: Arc::new(AtomicUsize::new(0)),
            snapshot_fn: None,
            initial_value: None,
            lifecycle: AtomicU8::new(Lifecycle::Init as u8),
            signal_flag: AtomicU8::new(0),
            time_ms: AtomicU64::new(0),
        }
    }

    // ── Observation ──────────────────────────────────────

    /// Returns `true` if at least one consumer is connected to any of this
    /// machine's observation ports.
    #[inline]
    pub fn observe_is_connected(&self) -> bool {
        self.observe_count.load(Ordering::Relaxed) > 0
    }

    // ── Output connection ────────────────────────────────

    /// Returns `true` if at least one consumer is connected to any of this
    /// machine's data/control output ports.
    ///
    /// Machines can use this to skip expensive computation when nobody
    /// is listening (Theorem 7.2 — output is reachable iff links exist).
    #[inline]
    pub fn output_is_connected(&self) -> bool {
        self.output_count.load(Ordering::Relaxed) > 0
    }

    // ── Snapshot ─────────────────────────────────────────

    /// Returns a byte-serialized snapshot of state, if available.
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        self.snapshot_fn.as_ref().and_then(|f| f())
    }

    // ── Initial value injection (工程修补 7.5.4) ──────────

    /// Inject an initial value for Source-like machines.
    /// Called by the runtime/deploy layer before `init()`.
    pub fn set_initial_value<V: core::any::Any + Send + Sync + 'static>(&mut self, value: V) {
        self.initial_value = Some(Arc::new(value));
    }

    /// Retrieve the injected initial value, downcasting to `V`.
    /// Returns `None` if no value was injected or type mismatch.
    pub fn initial_value<V: core::any::Any + Send + Sync + 'static>(&self) -> Option<&V> {
        self.initial_value.as_ref()?.downcast_ref::<V>()
    }

    // ── Lifecycle ────────────────────────────────────────

    /// Current lifecycle phase of the machine.
    /// Set by the runtime. Machines can query this to adjust behaviour
    /// during shutdown (e.g. skip non-essential work during Stopping).
    pub fn lifecycle(&self) -> Lifecycle {
        match self.lifecycle.load(Ordering::Acquire) {
            0 => Lifecycle::Init,
            1 => Lifecycle::Running,
            2 => Lifecycle::Stopping,
            _ => Lifecycle::Stopped,
        }
    }

    // ── Time ─────────────────────────────────────────────

    /// Current wall-clock or simulation time in milliseconds.
    /// Set by the runtime before each process() call.
    /// Returns 0 if the runtime does not provide time.
    pub fn time_ms(&self) -> u64 {
        self.time_ms.load(Ordering::Relaxed)
    }

    // ── Runtime adapter API ──────────────────────────────

    /// Set the current lifecycle phase (called by runtime).
    pub fn set_lifecycle(&self, lc: Lifecycle) {
        self.lifecycle.store(lc as u8, Ordering::Release);
    }

    /// Set the current time in ms (called by runtime before process()).
    pub fn set_time_ms(&self, ms: u64) {
        self.time_ms.store(ms, Ordering::Relaxed);
    }

    /// Send a signal to this machine (called by runtime).
    /// 工程修补 7.5.3：接受信号类型参数，支持 Shutdown 和 Checkpoint。
    pub fn send_signal(&self, signal: SystemSignal) {
        let code = match signal {
            SystemSignal::Shutdown => 1,
            SystemSignal::Checkpoint => 2,
        };
        self.signal_flag.store(code, Ordering::Release);
    }

    /// Poll for a pending system signal. Returns the signal type and clears the flag.
    /// Should be called once at the top of each process().
    pub fn poll_signal(&self) -> Option<SystemSignal> {
        let flag = self.signal_flag.swap(0, Ordering::Acquire);
        match flag {
            0 => None,
            1 => Some(SystemSignal::Shutdown),
            2 => Some(SystemSignal::Checkpoint),
            _ => Some(SystemSignal::Shutdown), // 防御性回退
        }
    }

    pub fn set_snapshot_fn(&mut self, f: Arc<dyn Fn() -> Option<Vec<u8>> + Send + Sync>) {
        self.snapshot_fn = Some(f);
    }

    pub fn observe_count_handle(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.observe_count)
    }

    pub fn observe_connect(&self) {
        self.observe_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn observe_disconnect(&self) {
        // 工程修补 7.5.1：防止 fetch_sub 下溢导致 wrap-around。
        // 使用 fetch_update 确保计数不低于零。
        let _ = self.observe_count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            if v == 0 { None } else { Some(v - 1) }
        });
    }

    // ── Output connection adapter API ────────────────────

    pub fn output_count_handle(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.output_count)
    }

    pub fn output_connect(&self) {
        self.output_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn output_disconnect(&self) {
        // 工程修补 7.5.1：防止 fetch_sub 下溢导致 wrap-around。
        let _ = self.output_count.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            if v == 0 { None } else { Some(v - 1) }
        });
    }
}
