//! Materialized live topology — runtime state held by the runtime.
//!
//! `materialize` computes two derived indexes in one pass, avoiding rebuilding
//! tables on the tick hot path:
//! - `machine_index`: machine name → index (for `mark_stopped` O(log M) lookup);
//! - `in_degree`: incoming edge count per machine (for decrement during shutdown
//!   propagation; a copy is taken each tick).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;

use crate::erasure::RunningMachine;

/// ID-based routing index (P0: avoids string matching and String clone on the dynamic path hot path).
///
/// Built once during materialization (including after fusion): machines are numbered
/// by `topo_order` (= `machine_index` value), ports are numbered by the schema's
/// `inputs()`/`outputs()` order (`&'static str`, known at compile time). Tick hot path:
/// - queues carry `(machine_id, port_id)` — no String clone;
/// - shutdown checks index a `Vec<bool>` by ID — O(1);
/// - routing looks up `route_by_src` by ID with a linear scan over the output port table
///   (output ports are usually 1-2).
pub struct TopologyIds {
    /// machine_id → `[(src_port(&'static str), dst_machine_id, dst_port_id)]`.
    /// The runtime compares directly against the port names (`&'static str`) from
    /// `ProcessResult::Yield`.
    pub route_by_src: Vec<Vec<(&'static str, usize, u16)>>,
    /// machine_id → [output port names] (for building the routing table and matching).
    pub out_port_names: Vec<Vec<&'static str>>,
    /// machine_id → [input port names] (for external injection String→ID and inject restoration).
    pub in_port_names: Vec<Vec<&'static str>>,
}

/// Materialized live topology — runtime state held by the runtime.
pub struct LiveTopology {
    pub machines: BTreeMap<String, Box<dyn RunningMachine>>,
    pub links: Vec<PhysicalLink>,
    pub topo_order: Vec<String>,
    /// Machine name → `topo_order` index. The tick hot path uses it to map machine names
    /// to `in_degree` indices, avoiding rebuilding the mapping table on every tick.
    pub machine_index: BTreeMap<String, usize>,
    /// Incoming edge count per machine (in `topo_order` order). Cloned each tick;
    /// `mark_stopped` decrements it — the clone is a single allocation (independent of
    /// link count), preserving the "constant allocation per link" invariant of R002.
    pub in_degree: Vec<usize>,
    /// Routing index: src_machine → (src_port → (dst_machine, dst_port)).
    ///
    /// Built once during materialization (including after fusion); the tick hot path
    /// performs O(log L) lookups — P2: eliminates the O(L) linear scan of `route_target`
    /// over `links` plus a per-message String clone (the src is a compile-time-known
    /// topology and routing is a materialization-time fact — it should not be re-scanned
    /// at runtime).
    pub route_map: BTreeMap<String, BTreeMap<String, (String, String)>>,
    /// ID-based routing index (P0): used on the `drive_sequential` hot path, eliminating
    /// String clones and string matching.
    pub ids: TopologyIds,
}

pub struct PhysicalLink {
    pub src_machine: String,
    pub src_port: String,
    pub dst_machine: String,
    pub dst_port: String,
    /// The physical semantics of the link (determines the channel carrier in Parallel mode).
    pub kind: axiom::link::LinkKind,
}
