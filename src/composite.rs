//! **Maturity: experimental** (an extension; advanced as part of the core, not dropped).
//!
//! Composite Machines — a sub-topology wrapped as a single `machine_type`.
//!
//! # Placement (homed under the anti-narrowing rule)
//!
//! A composite Machine is a **structural definition capability**: a
//! `DynamicTopology` (sub-topology) plus a port-mapping table. It belongs to
//! the structural layer of axiom core, **not** to the runtime's execution
//! capabilities — because:
//!
//! 1. `CompositeSpec` is pure data (a `DynamicTopology` plus two mapping tables);
//! 2. `expand_composites` is a pure data transformation (`DynamicTopology → DynamicTopology`);
//! 3. expansion does not depend on any execution primitive (no threads, no channels, no reactor).
//!
//! Placing it in the runtime would prevent core from expressing nested
//! topologies on its own — violating the "single-function narrowing"
//! prohibition of the anti-narrowing rule (`docs/philosophy.md`
//! §"The structural scope constraint"). This module homes it in core so that
//! core can independently define nested topologies of arbitrary depth.
//!
//! # Design
//!
//! A composite Machine is a `DynamicTopology` (sub-topology) plus a
//! port-mapping table. Once registered as a `machine_type`,
//! `expand_composites` processes each instance of that type as follows:
//!
//! 1. expand sub-machines — namespaced as `parent.sub` (avoids name collisions);
//! 2. expand sub-links — both machine names get the `parent.` prefix;
//! 3. redirect external links — links pointing at the composite instance are
//!    retargeted to sub-machines according to the port-mapping table.
//!
//! Nesting (a sub-machine that is itself a composite) is handled by looping
//! the expansion until no composite instance remains.
//!
//! # Port mapping
//!
//! - `input_map`: external input port name → (sub-machine name, sub-port name)
//! - `output_map`: external output port name → (sub-machine name, sub-port name)
//!
//! When the `in_port` of an external link `(src, sport) → (comp, in_port)`
//! hits `input_map`, it is rewritten as `(src, sport) → (comp.sub_machine,
//! sub_port)`. The output side works the same way.
//!
//! # Relationship with fusion
//!
//! Expansion is a structural-layer operation — it happens before any
//! materialization, endpoint validation, or fusion. Fusion sees the expanded
//! flat topology, and the composite boundary has disappeared. This allows a
//! `FusedPipeline` to fuse across the original composite boundary (if the
//! sub-machines are `FusedInline` with `Inline` links).

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
#[cfg(not(feature = "std"))]
use alloc::format;
use crate::deploy::{DynamicTopology, MachineInstance};
use crate::link::LinkSpec;
use crate::topology::Topology;

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ════════════════════════════════════════════════════════════════════════════
// Section 1: CompositeSpec — the pure data structure
// ════════════════════════════════════════════════════════════════════════════

/// A composite Machine definition — a sub-topology plus port mappings.
///
/// This is a **structural-layer** object: it only describes "what this
/// composite type looks like" and contains no execution logic. Once a
/// `CompositeSpec` is registered as a `machine_type`, [`expand_composites`]
/// replaces instances of that type with the expanded sub-topology.
///
/// # Validation
///
/// Call [`validate`](Self::validate) to check the integrity of the port
/// mappings:
/// - the sub-machines referenced by `input_map` / `output_map` must be in `spec.machines`;
/// - ideally the referenced sub-ports should exist in that sub-machine's
///   `PortSchema` (this requires the runtime to provide schemas; the core
///   layer only checks that the machine names exist).
///
/// # Serialization
///
/// Under the `serialize` feature, `CompositeSpec` round-trips through Serde,
/// consistent with `DynamicTopology` — supporting nested topologies loaded
/// from TOML/JSON configuration files.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize, serde::Deserialize))]
pub struct CompositeSpec {
    /// The sub-topology (machines + links + funcs + settings).
    pub spec: DynamicTopology,
    /// External input port → (sub-machine name, sub-port name).
    pub input_map: BTreeMap<String, (String, String)>,
    /// External output port → (sub-machine name, sub-port name).
    pub output_map: BTreeMap<String, (String, String)>,
}

// Blueprint concept: `CompositeSpec` is `Topology`'s **subgraph-reuse
// mechanism** — a composite definition is itself a topology (its internal
// `DynamicTopology` is the runtime projection). See [`Topology`].
impl Topology for CompositeSpec {}

impl CompositeSpec {
    /// Create a composite definition — a sub-topology plus empty port mappings
    /// (fill them in later with `with_input`/`with_output`).
    pub fn new(spec: DynamicTopology) -> Self {
        Self {
            spec,
            input_map: BTreeMap::new(),
            output_map: BTreeMap::new(),
        }
    }

    /// Declare a mapping for an external input port: `ext_port` → `(sub_machine, sub_port)`.
    pub fn with_input(mut self, ext_port: &str, sub_machine: &str, sub_port: &str) -> Self {
        self.input_map.insert(
            ext_port.to_string(),
            (sub_machine.to_string(), sub_port.to_string()),
        );
        self
    }

    /// Declare a mapping for an external output port: `ext_port` → `(sub_machine, sub_port)`.
    pub fn with_output(mut self, ext_port: &str, sub_machine: &str, sub_port: &str) -> Self {
        self.output_map.insert(
            ext_port.to_string(),
            (sub_machine.to_string(), sub_port.to_string()),
        );
        self
    }

    /// Validate the **machine-name existence** of the port mappings.
    ///
    /// This is the complete check the core layer can perform — the
    /// `sub_machine` referenced by `input_map` / `output_map` must exist in
    /// `spec.machines`. Port-name existence requires `PortSchema` (provided
    /// by the runtime) and is not checked at the core layer.
    ///
    /// # Errors
    ///
    /// - [`CompositeError::DanglingInputMapping`]: the sub-machine referenced by `input_map` does not exist;
    /// - [`CompositeError::DanglingOutputMapping`]: the sub-machine referenced by `output_map` does not exist;
    /// - [`CompositeError::DuplicatePortMapping`]: the same external port name
    ///   appears in both `input_map` and `output_map` (a port cannot be both
    ///   input and output).
    ///
    /// # Not checked
    ///
    /// - sub-port name existence (requires `PortSchema`);
    /// - the correctness of links inside the sub-topology (handled by
    ///   `DynamicTopology::validate_deep`);
    /// - composite self-reference (guarded by `expand_composites`'s depth limit).
    pub fn validate(&self) -> Result<(), CompositeError> {
        let sub_machine_names: crate::compat::HashSet<&str> =
            self.spec.machines.iter().map(|m| m.name.as_ref()).collect();

        // 1. The sub-machines referenced by input_map must exist.
        for (ext_port, (sub_m, _sub_p)) in &self.input_map {
            if !sub_machine_names.contains(sub_m.as_str()) {
                return Err(CompositeError::DanglingInputMapping {
                    ext_port: ext_port.clone(),
                    sub_machine: sub_m.clone(),
                });
            }
        }

        // 2. The sub-machines referenced by output_map must exist.
        for (ext_port, (sub_m, _sub_p)) in &self.output_map {
            if !sub_machine_names.contains(sub_m.as_str()) {
                return Err(CompositeError::DanglingOutputMapping {
                    ext_port: ext_port.clone(),
                    sub_machine: sub_m.clone(),
                });
            }
        }

        // 3. The same external port name must not appear in both input_map and output_map.
        for ext_port in self.input_map.keys() {
            if self.output_map.contains_key(ext_port) {
                return Err(CompositeError::DuplicatePortMapping {
                    ext_port: ext_port.clone(),
                });
            }
        }

        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 2: expand_composites — the pure data transformation
// ════════════════════════════════════════════════════════════════════════════

/// Recursively expand composite Machines — replace every instance whose
/// `machine_type` matches a registered composite with its sub-topology
/// (namespaced) and redirect external links.
///
/// This is a **pure data transformation**: `DynamicTopology → DynamicTopology`.
/// It does not depend on any execution primitive.
///
/// The expansion loops until no composite instance remains (handling nesting
/// of arbitrary depth). Returns `Err` when the nesting depth exceeds 64
/// (almost certainly a configuration error caused by composite self-reference).
///
/// # Arguments
///
/// - `machines`: the list of machines to expand (usually from `DynamicTopology::machines`);
/// - `links`: the list of links to expand (usually from `DynamicTopology::links`);
/// - `composites`: the `machine_type → CompositeSpec` registry.
///
/// # Returns
///
/// - `Ok((machines, links))`: the expanded flat topology, with no composite instances remaining;
/// - `Err(CompositeError::TooDeep)`: nesting depth exceeded 64, almost
///   certainly a composite self-reference (the sub-topology contains an
///   instance of its own type).
///
/// # Algorithm
///
/// Each iteration:
/// 1. scan `machines`, expanding instances whose type matches a composite into
///    sub-machines (namespaced);
/// 2. scan `links`, redirecting links whose endpoints hit a composite instance
///    according to the port mappings;
/// 3. if no expansion happened this round, return the current result;
///    otherwise continue to the next round.
///
/// Complexity: O(N+M) per round, where N = number of machines and M = number
/// of links. Worst case O(D·(N+M)), where D = depth.
pub fn expand_composites(
    mut machines: Vec<MachineInstance>,
    mut links: Vec<LinkSpec>,
    composites: &BTreeMap<String, CompositeSpec>,
) -> Result<(Vec<MachineInstance>, Vec<LinkSpec>), CompositeError> {
    // Safety valve: prevents runaway infinite recursion (a composite
    // self-reference would otherwise expand forever).
    // Normal nesting depth is < 10; exceeding 64 is almost certainly a
    // configuration error.
    const MAX_DEPTH: usize = 64;
    for _depth in 0..MAX_DEPTH {
        let mut next_machines: Vec<MachineInstance> = Vec::new();
        let mut next_links: Vec<LinkSpec> = Vec::new();
        // Snapshot of this round's expanded composite instance names →
        // (input_map, output_map).
        let mut port_maps: BTreeMap<
            String,
            (&BTreeMap<String, (String, String)>, &BTreeMap<String, (String, String)>),
        > = BTreeMap::new();
        let mut found_composite = false;

        // ── Expand machines ──
        for m in &machines {
            if let Some(comp) = composites.get(m.machine_type.as_ref()) {
                found_composite = true;
                let prefix = m.name.as_ref();
                port_maps.insert(prefix.to_string(), (&comp.input_map, &comp.output_map));

                for sub_m in &comp.spec.machines {
                    let mut expanded = sub_m.clone();
                    expanded.name = Cow::Owned(format!("{}.{}", prefix, sub_m.name));
                    next_machines.push(expanded);
                }
                // The sub-topology's links — namespace both endpoints.
                for sub_l in &comp.spec.links {
                    next_links.push(LinkSpec {
                        out: (
                            Cow::Owned(format!("{}.{}", prefix, sub_l.out.0)),
                            sub_l.out.1.clone(),
                        ),
                        into: (
                            Cow::Owned(format!("{}.{}", prefix, sub_l.into.0)),
                            sub_l.into.1.clone(),
                        ),
                        kind: sub_l.kind.clone(),
                    });
                }
            } else {
                next_machines.push(m.clone());
            }
        }

        // ── Redirect external links ──
        for l in &links {
            let src_machine = l.out.0.as_ref();
            let src_port = l.out.1.as_ref();
            let dst_machine = l.into.0.as_ref();
            let dst_port = l.into.1.as_ref();

            // The source endpoint is a composite instance → redirect via output_map.
            let new_out = if let Some((_, output_map)) = port_maps.get(src_machine) {
                if let Some((sub_m, sub_p)) = output_map.get(src_port) {
                    (
                        Cow::Owned(format!("{}.{}", src_machine, sub_m)),
                        Cow::Owned(sub_p.clone()),
                    )
                } else {
                    (l.out.0.clone(), l.out.1.clone())
                }
            } else {
                (l.out.0.clone(), l.out.1.clone())
            };

            // The destination endpoint is a composite instance → redirect via input_map.
            let new_into = if let Some((input_map, _)) = port_maps.get(dst_machine) {
                if let Some((sub_m, sub_p)) = input_map.get(dst_port) {
                    (
                        Cow::Owned(format!("{}.{}", dst_machine, sub_m)),
                        Cow::Owned(sub_p.clone()),
                    )
                } else {
                    (l.into.0.clone(), l.into.1.clone())
                }
            } else {
                (l.into.0.clone(), l.into.1.clone())
            };

            next_links.push(LinkSpec {
                out: new_out,
                into: new_into,
                kind: l.kind.clone(),
            });
        }

        machines = next_machines;
        links = next_links;

        if !found_composite {
            // All composites have been expanded — exit normally.
            return Ok((machines, links));
        }
        // Still contains composite instances but the depth budget is
        // exhausted — a configuration error (most likely a composite
        // self-reference). The loop falls through to the Err below.
    }

    Err(CompositeError::TooDeep {
        depth: MAX_DEPTH,
        hint: "composite machine_type may be self-referential (its sub-topology \
               contains an instance of itself). Check composite definitions for \
               cycles."
            .into(),
    })
}

// ════════════════════════════════════════════════════════════════════════════
// Section 3: Error types
// ════════════════════════════════════════════════════════════════════════════

/// An error from a composite Machine definition or its expansion.
///
/// This is a core-layer error — both `CompositeSpec::validate` and
/// `expand_composites` return this type. The runtime's
/// `RuntimeError::CompositeTooDeep` is its execution-layer mirror (the runtime
/// converts `CompositeError` into `RuntimeError` during materialization).
#[derive(Debug)]
pub enum CompositeError {
    /// The sub-machine referenced by `input_map` does not exist in `spec.machines`.
    DanglingInputMapping {
        /// The external input port name.
        ext_port: String,
        /// The referenced sub-machine name (does not exist).
        sub_machine: String,
    },
    /// The sub-machine referenced by `output_map` does not exist in `spec.machines`.
    DanglingOutputMapping {
        /// The external output port name.
        ext_port: String,
        /// The referenced sub-machine name (does not exist).
        sub_machine: String,
    },
    /// The same external port name appears in both `input_map` and `output_map`.
    ///
    /// A port cannot be both input and output — this is a direction conflict.
    DuplicatePortMapping {
        /// The conflicting external port name.
        ext_port: String,
    },
    /// Composite Machine nesting depth exceeded the limit (possibly an
    /// infinite expansion caused by composite self-reference).
    TooDeep {
        /// The depth limit that was reached.
        depth: usize,
        /// A diagnostic hint.
        hint: String,
    },
}

impl core::fmt::Display for CompositeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DanglingInputMapping { ext_port, sub_machine } => write!(
                f,
                "composite input_map references non-existent sub-machine: \
                 ext_port `{ext_port}` → `{sub_machine}` (not in spec.machines)"
            ),
            Self::DanglingOutputMapping { ext_port, sub_machine } => write!(
                f,
                "composite output_map references non-existent sub-machine: \
                 ext_port `{ext_port}` → `{sub_machine}` (not in spec.machines)"
            ),
            Self::DuplicatePortMapping { ext_port } => write!(
                f,
                "external port `{ext_port}` appears in both input_map and output_map \
                 (a port cannot be both input and output)"
            ),
            Self::TooDeep { depth, hint } => write!(
                f,
                "composite expansion exceeded depth {depth}: {hint}"
            ),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CompositeError {}

// ════════════════════════════════════════════════════════════════════════════
// Section 4: Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::{DynamicTopology, MachineInstance};
    use crate::link::{LinkKind, LinkSpec};
    use crate::resource::MachinePhysicalSpec;

    // ── Test helpers ─────────────────────────────────────────────────────

    fn machine(name: &'static str) -> MachineInstance {
        MachineInstance::new(name, "test", MachinePhysicalSpec::default())
    }

    fn inline(a: &'static str, pa: &'static str, b: &'static str, pb: &'static str) -> LinkSpec {
        LinkSpec::new((a, pa), (b, pb), LinkKind::Inline)
    }

    /// Build a simple composite: sub-topology `inner → inner2`, external ports `in`/`out`.
    fn simple_composite() -> CompositeSpec {
        let spec = DynamicTopology::new()
            .with_machine(machine("inner"))
            .with_machine(machine("inner2"))
            .with_link(inline("inner", "y", "inner2", "x"));
        CompositeSpec::new(spec)
            .with_input("in", "inner", "x")
            .with_output("out", "inner2", "y")
    }

    // ══════════════════════════════════════════════════════════════════
    // validate() — port-mapping integrity
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn validate_ok_simple_composite() {
        let comp = simple_composite();
        assert!(comp.validate().is_ok(), "simple composite should validate");
    }

    #[test]
    fn validate_ok_empty_port_maps() {
        // Empty port mappings are also valid — a composite may only have
        // internal links and no external ports.
        let spec = DynamicTopology::new().with_machine(machine("inner"));
        let comp = CompositeSpec::new(spec);
        assert!(comp.validate().is_ok());
    }

    #[test]
    fn validate_rejects_dangling_input_mapping() {
        // The sub-machine "nonexistent" referenced by input_map is not in spec.machines.
        let spec = DynamicTopology::new().with_machine(machine("inner"));
        let comp = CompositeSpec::new(spec).with_input("in", "nonexistent", "x");
        let err = comp.validate().unwrap_err();
        match err {
            CompositeError::DanglingInputMapping { ext_port, sub_machine } => {
                assert_eq!(ext_port, "in");
                assert_eq!(sub_machine, "nonexistent");
            }
            other => panic!("expected DanglingInputMapping, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_dangling_output_mapping() {
        // The sub-machine "ghost" referenced by output_map is not in spec.machines.
        let spec = DynamicTopology::new().with_machine(machine("inner"));
        let comp = CompositeSpec::new(spec).with_output("out", "ghost", "y");
        let err = comp.validate().unwrap_err();
        match err {
            CompositeError::DanglingOutputMapping { ext_port, sub_machine } => {
                assert_eq!(ext_port, "out");
                assert_eq!(sub_machine, "ghost");
            }
            other => panic!("expected DanglingOutputMapping, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_duplicate_port_mapping() {
        // The same external port "x" appears in both input_map and output_map.
        let spec = DynamicTopology::new()
            .with_machine(machine("inner"))
            .with_machine(machine("inner2"));
        let comp = CompositeSpec::new(spec)
            .with_input("x", "inner", "p1")
            .with_output("x", "inner2", "p2");
        let err = comp.validate().unwrap_err();
        match err {
            CompositeError::DuplicatePortMapping { ext_port } => {
                assert_eq!(ext_port, "x");
            }
            other => panic!("expected DuplicatePortMapping, got {other:?}"),
        }
    }

    #[test]
    fn validate_ok_multiple_inputs_outputs() {
        // Multiple inputs and outputs — all reference existing sub-machines.
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"));
        let comp = CompositeSpec::new(spec)
            .with_input("in1", "a", "x")
            .with_input("in2", "b", "x")
            .with_output("out1", "c", "y")
            .with_output("out2", "c", "z");
        assert!(comp.validate().is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // expand_composites() — basic expansion
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn expand_no_composites_returns_unchanged() {
        // No composite instances — return the original machines/links (cloned).
        let machines = vec![machine("a"), machine("b")];
        let links = vec![inline("a", "y", "b", "x")];
        let composites = BTreeMap::new();
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");
        assert_eq!(out_m.len(), 2);
        assert_eq!(out_l.len(), 1);
        assert_eq!(out_m[0].name.as_ref(), "a");
        assert_eq!(out_m[1].name.as_ref(), "b");
    }

    #[test]
    fn expand_single_composite_replaces_instance() {
        // One composite instance "root" → expands to root.inner + root.inner2.
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![MachineInstance::new("root", "comp", MachinePhysicalSpec::default())];
        let links = vec![];
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        assert_eq!(out_m.len(), 2, "composite expands to 2 sub-machines");
        assert_eq!(out_m[0].name.as_ref(), "root.inner");
        assert_eq!(out_m[1].name.as_ref(), "root.inner2");
        assert_eq!(out_l.len(), 1, "sub-topology has 1 internal link");
        assert_eq!(out_l[0].out.0.as_ref(), "root.inner");
        assert_eq!(out_l[0].into.0.as_ref(), "root.inner2");
    }

    #[test]
    fn expand_redirects_external_input_link() {
        // The external link (ext, y) → (root, in) hits input_map → redirect to (root.inner, x).
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("root", "comp", MachinePhysicalSpec::default()),
            machine("ext"),
        ];
        let links = vec![inline("ext", "y", "root", "in")];
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        assert_eq!(out_m.len(), 3, "ext + 2 sub-machines");
        // The external link should be redirected to root.inner.x
        let redirected = out_l
            .iter()
            .find(|l| l.out.0.as_ref() == "ext" && l.out.1.as_ref() == "y")
            .expect("external link should exist");
        assert_eq!(redirected.into.0.as_ref(), "root.inner");
        assert_eq!(redirected.into.1.as_ref(), "x");
    }

    #[test]
    fn expand_redirects_external_output_link() {
        // The external link (root, out) → (ext, x) hits output_map → redirect to (root.inner2, y).
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("root", "comp", MachinePhysicalSpec::default()),
            machine("ext"),
        ];
        let links = vec![inline("root", "out", "ext", "x")];
        let (out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        assert_eq!(out_m.len(), 3);
        let redirected = out_l
            .iter()
            .find(|l| l.into.0.as_ref() == "ext" && l.into.1.as_ref() == "x")
            .expect("external link should exist");
        assert_eq!(redirected.out.0.as_ref(), "root.inner2");
        assert_eq!(redirected.out.1.as_ref(), "y");
    }

    #[test]
    fn expand_unmapped_external_port_passes_through() {
        // The external port "unknown" is not in input_map/output_map → the link stays as-is
        // (pointing at root.unknown; validate_deep will report a DanglingRef later).
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("root", "comp", MachinePhysicalSpec::default()),
            machine("ext"),
        ];
        let links = vec![inline("ext", "y", "root", "unknown")];
        let (_out_m, out_l) = expand_composites(machines, links, &composites).expect("expand");

        // Unmapped ports stay as-is — validate_deep will catch them later.
        let unredirected = out_l
            .iter()
            .find(|l| l.out.0.as_ref() == "ext")
            .expect("external link should exist");
        assert_eq!(unredirected.into.0.as_ref(), "root");
        assert_eq!(unredirected.into.1.as_ref(), "unknown");
    }

    // ══════════════════════════════════════════════════════════════════
    // expand_composites() — nesting and depth
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn expand_nested_composite_two_levels() {
        // The outer composite "outer" contains an inner composite "inner" instance.
        // After expansion: root.outer.inner.sub1, root.outer.inner.sub2
        let inner_spec = DynamicTopology::new()
            .with_machine(machine("sub1"))
            .with_machine(machine("sub2"))
            .with_link(inline("sub1", "y", "sub2", "x"));
        let inner_comp = CompositeSpec::new(inner_spec)
            .with_input("in", "sub1", "x")
            .with_output("out", "sub2", "y");

        let outer_spec = DynamicTopology::new()
            .with_machine(MachineInstance::new("inner_inst", "inner_type", MachinePhysicalSpec::default()));
        let outer_comp = CompositeSpec::new(outer_spec)
            .with_input("in", "inner_inst", "in")
            .with_output("out", "inner_inst", "out");

        let mut composites = BTreeMap::new();
        composites.insert("inner_type".to_string(), inner_comp);
        composites.insert("outer_type".to_string(), outer_comp);

        let machines = vec![MachineInstance::new("root", "outer_type", MachinePhysicalSpec::default())];
        let (out_m, _out_l) = expand_composites(machines, vec![], &composites).expect("expand");

        // Two rounds of expansion: outer → inner_inst.inner_type → sub1/sub2
        assert_eq!(out_m.len(), 2);
        assert_eq!(out_m[0].name.as_ref(), "root.inner_inst.sub1");
        assert_eq!(out_m[1].name.as_ref(), "root.inner_inst.sub2");
    }

    #[test]
    fn expand_self_referential_composite_reports_too_deep() {
        // The composite "loop" has a sub-topology containing an instance of its own type
        // → infinite expansion → TooDeep.
        let loop_spec = DynamicTopology::new().with_machine(MachineInstance::new(
            "inner",
            "loop",
            MachinePhysicalSpec::default(),
        ));
        let loop_comp = CompositeSpec::new(loop_spec)
            .with_input("in", "inner", "in")
            .with_output("out", "inner", "out");

        let mut composites = BTreeMap::new();
        composites.insert("loop".to_string(), loop_comp);

        let machines = vec![MachineInstance::new("root", "loop", MachinePhysicalSpec::default())];
        let err = expand_composites(machines, vec![], &composites).unwrap_err();
        match err {
            CompositeError::TooDeep { depth, .. } => {
                assert_eq!(depth, 64, "MAX_DEPTH = 64");
            }
            other => panic!("expected TooDeep, got {other:?}"),
        }
    }

    #[test]
    fn expand_multiple_composites_in_parallel() {
        // Two independent composite instances expand simultaneously — namespaces don't collide.
        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), simple_composite());

        let machines = vec![
            MachineInstance::new("a", "comp", MachinePhysicalSpec::default()),
            MachineInstance::new("b", "comp", MachinePhysicalSpec::default()),
        ];
        let (out_m, out_l) = expand_composites(machines, vec![], &composites).expect("expand");

        assert_eq!(out_m.len(), 4, "2 composites × 2 sub-machines each");
        // The sub-machine namespaces of the two composite instances differ.
        let names: Vec<&str> = out_m.iter().map(|m| m.name.as_ref()).collect();
        assert!(names.contains(&"a.inner"));
        assert!(names.contains(&"a.inner2"));
        assert!(names.contains(&"b.inner"));
        assert!(names.contains(&"b.inner2"));
        // Each composite has 1 internal link.
        assert_eq!(out_l.len(), 2);
    }

    // ══════════════════════════════════════════════════════════════════
    // expand_composites() — edge cases
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn expand_empty_machines_returns_empty() {
        let (out_m, out_l) =
            expand_composites(vec![], vec![], &BTreeMap::new()).expect("expand");
        assert!(out_m.is_empty());
        assert!(out_l.is_empty());
    }

    #[test]
    fn expand_unregistered_composite_type_passes_through() {
        // The machine_type "unknown_comp" is not registered — the instance is kept as-is.
        let machines = vec![MachineInstance::new(
            "x",
            "unknown_comp",
            MachinePhysicalSpec::default(),
        )];
        let (out_m, _out_l) =
            expand_composites(machines, vec![], &BTreeMap::new()).expect("expand");
        assert_eq!(out_m.len(), 1);
        assert_eq!(out_m[0].name.as_ref(), "x");
        assert_eq!(out_m[0].machine_type.as_ref(), "unknown_comp");
    }

    #[test]
    fn expand_preserves_machine_physical_spec() {
        // The expanded sub-machines should preserve the original sub-machine's physical spec.
        use crate::resource::ExecutionHint;
        let mut inner_physical = MachinePhysicalSpec::default();
        inner_physical.execution = ExecutionHint::CpuBound;
        let inner = MachineInstance::new("inner", "test", inner_physical);
        let spec = DynamicTopology::new().with_machine(inner);
        let comp = CompositeSpec::new(spec);

        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), comp);

        let machines = vec![MachineInstance::new("root", "comp", MachinePhysicalSpec::default())];
        let (out_m, _) = expand_composites(machines, vec![], &composites).expect("expand");

        assert_eq!(out_m.len(), 1);
        assert!(matches!(
            out_m[0].physical.execution,
            ExecutionHint::CpuBound
        ));
    }

    #[test]
    fn expand_preserves_link_kind() {
        // The expanded links should preserve the original LinkKind (including BoundedBuf's parameters).
        use crate::link::{ReadPolicy, WritePolicy};
        let bounded = LinkSpec::new(
            ("inner", "y"),
            ("inner2", "x"),
            LinkKind::BoundedBuf {
                capacity: 42,
                write_policy: WritePolicy::Dropping,
                read_policy: ReadPolicy::NonBlocking,
            },
        );
        let spec = DynamicTopology::new()
            .with_machine(machine("inner"))
            .with_machine(machine("inner2"))
            .with_link(bounded);
        let comp = CompositeSpec::new(spec);

        let mut composites = BTreeMap::new();
        composites.insert("comp".to_string(), comp);

        let machines = vec![MachineInstance::new("root", "comp", MachinePhysicalSpec::default())];
        let (out_m, out_l) = expand_composites(machines, vec![], &composites).expect("expand");

        assert_eq!(out_m.len(), 2);
        assert_eq!(out_l.len(), 1);
        match &out_l[0].kind {
            LinkKind::BoundedBuf {
                capacity,
                write_policy,
                read_policy,
            } => {
                assert_eq!(*capacity, 42);
                assert_eq!(*write_policy, WritePolicy::Dropping);
                assert_eq!(*read_policy, ReadPolicy::NonBlocking);
            }
            other => panic!("expected BoundedBuf, got {other:?}"),
        }
    }

    // ══════════════════════════════════════════════════════════════════
    // CompositeSpec builder API
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn builder_with_input_chains() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"));
        let comp = CompositeSpec::new(spec)
            .with_input("in1", "a", "x")
            .with_input("in2", "b", "x");
        assert_eq!(comp.input_map.len(), 2);
        assert!(comp.input_map.contains_key("in1"));
        assert!(comp.input_map.contains_key("in2"));
    }

    #[test]
    fn builder_with_output_chains() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"));
        let comp = CompositeSpec::new(spec)
            .with_output("out1", "a", "y")
            .with_output("out2", "b", "y");
        assert_eq!(comp.output_map.len(), 2);
        assert!(comp.output_map.contains_key("out1"));
        assert!(comp.output_map.contains_key("out2"));
    }

    #[test]
    fn builder_with_input_and_output() {
        let spec = DynamicTopology::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"));
        let comp = CompositeSpec::new(spec)
            .with_input("in", "a", "x")
            .with_output("out", "b", "y");
        assert_eq!(comp.input_map.len(), 1);
        assert_eq!(comp.output_map.len(), 1);
        assert!(comp.validate().is_ok());
    }

    // ══════════════════════════════════════════════════════════════════
    // Error Display
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn error_display_dangling_input() {
        let err = CompositeError::DanglingInputMapping {
            ext_port: "in".into(),
            sub_machine: "ghost".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("input_map"));
        assert!(s.contains("in"));
        assert!(s.contains("ghost"));
    }

    #[test]
    fn error_display_dangling_output() {
        let err = CompositeError::DanglingOutputMapping {
            ext_port: "out".into(),
            sub_machine: "phantom".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("output_map"));
        assert!(s.contains("out"));
        assert!(s.contains("phantom"));
    }

    #[test]
    fn error_display_duplicate_port() {
        let err = CompositeError::DuplicatePortMapping {
            ext_port: "x".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("x"));
        assert!(s.contains("both"));
    }

    #[test]
    fn error_display_too_deep() {
        let err = CompositeError::TooDeep {
            depth: 64,
            hint: "self-referential".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("64"));
        assert!(s.contains("self-referential"));
    }
}
