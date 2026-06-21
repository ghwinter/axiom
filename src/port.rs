use core::any::TypeId;
use core::sync::atomic::{AtomicUsize, Ordering};
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
/// Carries observation detection and snapshot capabilities.
pub struct MachineContext {
    pub name: &'static str,
    /// Number of active consumers on observation ports.
    pub(crate) observe_count: Arc<AtomicUsize>,
    /// Snapshot function (wired by runtime).
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

    /// Returns `true` if at least one consumer is connected to any of this
    /// machine's observation ports.
    #[inline]
    pub fn observe_is_connected(&self) -> bool {
        self.observe_count.load(Ordering::Relaxed) > 0
    }

    /// Returns a byte-serialized snapshot of state, if available.
    pub fn snapshot(&self) -> Option<Vec<u8>> {
        self.snapshot_fn.as_ref().and_then(|f| f())
    }

    // ── Runtime adapter API ──────────────────────────────────

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
        self.observe_count.fetch_sub(1, Ordering::Relaxed);
    }
}
