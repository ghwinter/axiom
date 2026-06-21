use core::any::TypeId;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ── Port direction ────────────────────────────────────────────────────────────

/// The direction of data flow through a port.
///
/// # Semantics
/// - `In`: data flows into the machine. The machine consumes it.
/// - `Out`: data flows out of the machine. Other machines may consume it.
/// - `Observe`: structured observation data flows out of the machine.
///   Observe ports are read-only from the outside and are never connected to
///   `In` ports — they are connected to a `Collector` sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDir {
    In,
    Out,
    Observe,
}

// ── Port declaration (compile-time / schema) ─────────────────────────────────

/// A single port declaration in a machine's port schema.
///
/// Includes type metadata for link-time compatibility checking
/// and a schema version for evolution support.
#[derive(Debug, Clone)]
pub struct PortDecl {
    pub name: &'static str,
    pub dir: PortDir,
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub schema_ver: u32,
    pub description: &'static str,
}

impl PortDecl {
    /// Declare an input port.
    pub fn input<T: 'static>(name: &'static str) -> Self {
        Self {
            name,
            dir: PortDir::In,
            type_id: TypeId::of::<T>(),
            type_name: core::any::type_name::<T>(),
            schema_ver: 0,
            description: "",
        }
    }

    /// Declare an output port.
    pub fn output<T: 'static>(name: &'static str) -> Self {
        Self {
            name,
            dir: PortDir::Out,
            type_id: TypeId::of::<T>(),
            type_name: core::any::type_name::<T>(),
            schema_ver: 0,
            description: "",
        }
    }

    /// Declare an observation port.
    pub fn observe<T: 'static>(name: &'static str) -> Self {
        Self {
            name,
            dir: PortDir::Observe,
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

/// The complete set of ports a machine exposes.
#[derive(Debug, Clone)]
pub struct PortSchema {
    ports: Vec<PortDecl>,
    // Index to the primary input/output for quick lookup.
    primary_in: Option<usize>,
    primary_out: Option<usize>,
    observe_idx: Option<usize>,
}

impl PortSchema {
    pub fn new() -> Self {
        Self {
            ports: Vec::new(),
            primary_in: None,
            primary_out: None,
            observe_idx: None,
        }
    }

    pub fn with(mut self, decl: PortDecl) -> Self {
        let idx = self.ports.len();
        match decl.dir {
            PortDir::In if self.primary_in.is_none() => self.primary_in = Some(idx),
            PortDir::Out if self.primary_out.is_none() => self.primary_out = Some(idx),
            PortDir::Observe if self.observe_idx.is_none() => self.observe_idx = Some(idx),
            _ => {}
        }
        self.ports.push(decl);
        self
    }

    pub fn ports(&self) -> &[PortDecl] {
        &self.ports
    }

    pub fn find(&self, name: &str) -> Option<&PortDecl> {
        self.ports.iter().find(|p| p.name == name)
    }

    pub fn primary_input(&self) -> Option<&PortDecl> {
        self.primary_in.map(|i| &self.ports[i])
    }

    pub fn primary_output(&self) -> Option<&PortDecl> {
        self.primary_out.map(|i| &self.ports[i])
    }

    pub fn observe_port(&self) -> Option<&PortDecl> {
        self.observe_idx.map(|i| &self.ports[i])
    }
}

// ── Link-compatibility check ──────────────────────────────────────────────────

/// Result of checking whether two port declarations can be linked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkCompat {
    /// Ports are compatible.
    Compatible,
    /// Ports are compatible with an automatic schema migration (version drift ≤ 1).
    Migrate { from_ver: u32, to_ver: u32 },
    /// Ports are incompatible.
    Incompatible { reason: &'static str },
}

impl PortDecl {
    /// Check whether this port can be linked to `other`.
    ///
    /// Rules:
    /// - `self.dir` must be `Out`, `other.dir` must be `In`.
    /// - `self.type_id == other.type_id`.
    /// - Schema versions must differ by at most 1.
    pub fn can_link_to(&self, other: &PortDecl) -> LinkCompat {
        if self.dir != PortDir::Out {
            return LinkCompat::Incompatible {
                reason: "source port is not an output",
            };
        }
        if other.dir != PortDir::In {
            return LinkCompat::Incompatible {
                reason: "target port is not an input",
            };
        }
        if self.type_id != other.type_id {
            return LinkCompat::Incompatible {
                reason: "type mismatch",
            };
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
            _ => LinkCompat::Incompatible {
                reason: "schema version drift > 1",
            },
        }
    }
}

// ── Port registry (runtime) ───────────────────────────────────────────────────

/// Runtime registry of a machine's port statistics.
///
/// Populated during `Machine::init` and readable by snapshotter tools.
/// Does NOT hold data handles — it holds only `&'static str` metadata
/// and a reference to the underlying buffer's stats.
#[derive(Debug, Default)]
pub struct PortRegistry {
    entries: Vec<PortEntry>,
}

/// A registered port with its runtime statistics handle.
#[derive(Debug)]
pub struct PortEntry {
    pub name: &'static str,
    pub dir: PortDir,
    pub type_name: &'static str,
    pub schema_ver: u32,
}

impl PortRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, decl: &PortDecl) {
        self.entries.push(PortEntry {
            name: decl.name,
            dir: decl.dir,
            type_name: decl.type_name,
            schema_ver: decl.schema_ver,
        });
    }

    pub fn entries(&self) -> &[PortEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── Config schema ─────────────────────────────────────────────────────────────

/// Declaration of a configuration parameter.
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

/// The set of configuration parameters a machine exposes.
#[derive(Debug, Clone, Default)]
pub struct ConfigSchema {
    decls: Vec<ConfigDecl>,
}

impl ConfigSchema {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, decl: ConfigDecl) -> Self {
        self.decls.push(decl);
        self
    }

    pub fn decls(&self) -> &[ConfigDecl] {
        &self.decls
    }
}

// ── MachineContext ────────────────────────────────────────────────────────────

/// Context provided to a Machine during its lifecycle.
///
/// The context carries the machine's identity, observation state,
/// and a snapshot mechanism for pull-based state queries.
///
/// # Observation detection
/// `observe_is_connected()` returns true if at least one consumer
/// has subscribed to this machine's observe_out port.
/// The runtime sets this flag when establishing or tearing down a link.
/// Use it to skip expensive observation code when nobody is watching.
///
/// # State snapshots
/// `snapshot()` returns a byte-serialized copy of the machine's current
/// State, if the machine supports checkpointing (Machine::checkpoint).
/// The runtime wires this up during init.
pub struct MachineContext {
    /// Human-readable machine name.
    pub name: &'static str,

    /// Number of active consumers on the observe_out port.
    /// Incremented by the runtime when a link is established.
    /// Decremented when a link is torn down.
    pub(crate) observe_count: Arc<AtomicUsize>,

    /// Snapshot function: captures the current State and serializes it.
    /// Set by the runtime after init, using Machine::checkpoint.
    pub(crate) snapshot_fn: Option<Arc<dyn Fn() -> Option<Vec<u8>> + Send + Sync>>,
}

impl core::fmt::Debug for MachineContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MachineContext")
            .field("name", &self.name)
            .field("observe_count", &self.observe_count.load(Ordering::Relaxed))
            .field("has_snapshot_fn", &self.snapshot_fn.is_some())
            .finish()
    }
}

impl MachineContext {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            observe_count: Arc::new(AtomicUsize::new(0)),
            snapshot_fn: None,
        }
    }

    /// Returns `true` if at least one consumer is connected to the
    /// machine's observe_out port. Use this to skip observation
    /// code (formatting, string allocation) when nobody is watching.
    ///
    /// This is a single atomic load — zero overhead when hot.
    #[inline]
    pub fn observe_is_connected(&self) -> bool {
        self.observe_count.load(Ordering::Relaxed) > 0
    }

    /// Returns a byte-serialized snapshot of the machine's current
    /// State, if the machine implements `Machine::checkpoint`.
    ///
    /// This enables pull-based observation: external tools (CLI, TUI,
    /// health checkers) can query state at any time without waiting
    /// for the machine to emit an observe event.
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        self.snapshot_fn.as_ref().and_then(|f| f())
    }

    // ── Runtime adapter API ──────────────────────────────────
    // These methods are used by runtime adapters (e.g., axiom_tokio)
    // to wire up observation detection and snapshot mechanisms.
    // They are not intended for Machine implementors.

    /// Set the snapshot function. Called by the runtime after init,
    /// once the machine's State is allocated.
    pub fn set_snapshot_fn(&mut self, f: Arc<dyn Fn() -> Option<Vec<u8>> + Send + Sync>) {
        self.snapshot_fn = Some(f);
    }

    /// Get a reference to the observe consumer count.
    /// The runtime uses this to register a new consumer.
    pub fn observe_count_handle(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.observe_count)
    }

    /// Manually increment observe count (used by runtimes that
    /// cannot wait for link establishment).
    pub fn observe_connect(&self) {
        self.observe_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Manually decrement observe count.
    pub fn observe_disconnect(&self) {
        self.observe_count.fetch_sub(1, Ordering::Relaxed);
    }
}
