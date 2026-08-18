//! pipelineN compile-time fusion — the `materialize` stage replaces a chain of adjacent
//! `FusedInline` machines with a single `FusedPipeline` wrapper, eliminating the per-hop route
//! lookup and queue overhead.
//!
//! # Fusion conditions
//!
//! An Inline link `(src, src_port) → (dst, dst_port)` is a fusion candidate if and only if:
//! 1. `link.kind == Inline`;
//! 2. both the `src` and `dst` machines are `is_fused_compatible()`;
//! 3. `src` has **no other downstream** on that `src_port` (no fan-out);
//! 4. `dst` has **no other upstream** on that port (no fan-in).
//!
//! A maximal chain = from a chain head (no fusion-candidate in-edges) along fusion-candidate
//! out-edges to the chain tail (no fusion-candidate out-edges). Chains of length ≥ 2 are replaced
//! with a `FusedPipeline`.
//!
//! # Overhead elimination
//!
//! Per hop without fusion: `route_target`'s 2 String clones + `VecDeque` push reallocation
//! (amortized 1) + `Box<dyn Any>` (inherent to type erasure, 1) ≈ +4 alloc/hop.
//! After fusion, ports inside the chain are delivered by direct move, eliminating the route lookup
//! and queue overhead, keeping only `Box<dyn Any>` (1) + internal routing (1) ≈ +2 alloc/hop.
//! Net reduction of 2 alloc/hop (verified empirically in R003).
//!
//! # Output handling
//!
//! A fused stage is a **single-input single-output** machine: `register_fused`
//! requires `M::Input: Pack` + `M::Output: Unpack`, and `Unpack` is only
//! generated for single-output port enums (`src/portset.rs`), so every in-chain
//! stage has exactly one data output port. That one output feeds the next
//! stage (in-chain port) or the chain tail (terminal). Non-chain output ports
//! (e.g. an `Observe` port) are collected as terminal outputs. `TupleOutput`
//! machines do **not** enter a fused chain — they lack `Unpack` — so the old
//! claim "one output in-chain + the other terminal" is not realized (and must
//! not be assumed: a widened `FusedCompatible` would need explicit
//! multi-output delivery, never silent dropping).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use axiom::port::PortSchema;

use crate::erasure::{ProcessResult, RunningMachine, ScratchResult};
use crate::error::RuntimeError;
use crate::topology::PhysicalLink;

/// Fused pipeline — wraps N adjacent `FusedInline` machines as a single `RunningMachine`.
///
/// `inject` drives all stages sequentially within a single call: stage[0]'s output is routed to
/// stage[1] per `internal_links`, and so on until the last stage. In-chain ports do not go through
/// queues/route lookup; non-chain ports (observation ports, terminal outputs) are returned as a
/// `ProcessResult`.
pub(crate) struct FusedPipeline {
    /// The machines of the chain. stage[0] is the entry, stage[n-1] is the exit.
    stages: Vec<Box<dyn RunningMachine>>,
    /// `internal_links[i]` = (the output port stage[i] feeds to stage[i+1],
    /// the input port stage[i+1] receives). Length = stages.len() - 1.
    /// Under the single-slot protocol, `chain_port` decides in-chain/terminal, and
    /// `next_input_id` is passed to the next stage as the port argument of process_scratch
    /// (P0: allocation-free inter-stage passing).
    internal_links: Vec<(&'static str, u16)>,
    /// Pipeline name (uses the chain head's machine name; external links still reference it by
    /// this name).
    name: String,
    schema: PortSchema,
}

impl FusedPipeline {
    pub(crate) fn new(
        stages: Vec<Box<dyn RunningMachine>>,
        internal_links: Vec<(&'static str, String)>,
        name: String,
    ) -> Self {
        // The schema takes the first stage's input schema (the entry port) — external links
        // match against this.
        let schema = stages[0].port_schema().clone();
        // At construction, parse next_input (String) into stage[i+1]'s port ID.
        // validate_endpoint already guarantees the port exists — position must hit.
        let internal_links: Vec<(&'static str, u16)> = internal_links
            .iter()
            .enumerate()
            .map(|(i, (chain_port, next_input))| {
                let pid = stages[i + 1]
                    .port_schema()
                    .inputs()
                    .position(|p| p.name == next_input.as_str())
                    .expect("fused internal link port must exist") as u16;
                (*chain_port, pid)
            })
            .collect();
        Self { stages, internal_links, name, schema }
    }

    /// Flatten collected terminal outputs into a [`ProcessResult`].
    fn into_result(
        mut terminal: Vec<(&'static str, Box<dyn core::any::Any + Send>)>,
    ) -> ProcessResult {
        if terminal.is_empty() {
            ProcessResult::Idle
        } else if terminal.len() == 1 {
            let (port, value) = terminal.pop().unwrap();
            ProcessResult::Yield { port, value }
        } else {
            ProcessResult::YieldMulti { outputs: terminal }
        }
    }
}

impl RunningMachine for FusedPipeline {
    fn name(&self) -> &str { &self.name }

    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult {
        // FusedPipeline is driven through inject — process_boxed is not used directly
        // (the entry port is handled by inject). Kept in case of direct external calls.
        self.stages[0].process_boxed(input)
    }

    fn inject(&mut self, port_id: u16, payload: Box<dyn core::any::Any + Send>) -> ProcessResult {
        // Allocation-free inter-stage protocol after the unsafe workaround: a single slot runs
        // through the whole chain — the external input Box is used directly as the slot; each
        // stage's process_scratch takes the raw value from the slot, processes, and writes it
        // back to the same slot via put_output (same-type inter-stage = 0 allocation /
        // cross-type transition point = 1).
        let mut slot: Option<Box<dyn core::any::Any + Send>> = Some(payload);
        let mut terminal: Vec<(&'static str, Box<dyn core::any::Any + Send>)> = Vec::new();
        let mut last_port: &'static str = "";

        for i in 0..self.stages.len() {
            // Port ID: the first stage uses the externally injected port_id; later stages use the
            // in-chain link's specified one.
            let pid = if i == 0 {
                port_id
            } else {
                self.internal_links[i - 1].1
            };
            match self.stages[i].process_scratch(pid, &mut slot) {
                ScratchResult::Done => {
                    // A stage completed (shutdown signal): the **whole chain** must stop, so the
                    // driver propagates the shutdown cascade (`mark_stopped` keyed on
                    // `ProcessResult::Done`). Previously this was folded into the `Idle`/broken
                    // path, returning Idle/terminal — the driver's cancellation never fired and
                    // the completed chain kept processing. Terminal values collected so far are
                    // dropped (not delivered): they are Observe (best-effort, droppable) or
                    // non-chain ports, and delivering them after a completion signal would be
                    // ordering-unsound across fuse boundaries.
                    return ProcessResult::Done;
                }
                ScratchResult::Idle => {
                    // Chain broken without completion: the in-slot value was consumed with no
                    // output; deliver what was collected before the break.
                    return Self::into_result(terminal);
                }
                ScratchResult::Yield(port) => {
                    last_port = port;
                    if i + 1 < self.stages.len() {
                        // In-chain port → the value is already in the slot, continue to the next
                        // stage; otherwise collect it as terminal.
                        if port != self.internal_links[i].0 {
                            if let Some(v) = slot.take() {
                                terminal.push((port, v));
                            }
                        }
                    }
                }
            }
        }

        // The chain-tail value (not broken): move it out of the slot as a terminal output —
        // no new allocation.
        if let Some(v) = slot.take() {
            terminal.push((last_port, v));
        }
        Self::into_result(terminal)
    }

    fn is_done(&self) -> bool {
        self.stages.iter().any(|s| s.is_done())
    }

    fn is_fused_compatible(&self) -> bool { true }

    fn port_schema(&self) -> &PortSchema { &self.schema }

    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError> {
        let inner = *self;
        for stage in inner.stages {
            stage.cleanup()?;
        }
        Ok(())
    }
}

/// Chain-recognition result — a set of linear chains replaceable by a `FusedPipeline`.
#[derive(Debug)]
pub(crate) struct FusionChain {
    /// Names of the machines in the chain (in topological order).
    machines: Vec<String>,
    /// In-chain links (machine[i] → machine[i+1]): (src_port, dst_port).
    internal: Vec<(&'static str, String)>,
}

/// Identify all maximal fusible linear chains.
///
/// Algorithm:
/// 1. Mark each Inline link as a fusion candidate (both endpoint machines fusible + no
///    fan-out/fan-in);
/// 2. Find chain heads (machines with a fusion-candidate out-edge but no fusion-candidate in-edge);
/// 3. Walk from each head along fusion-candidate out-edges to the tail, collecting the chain.
///
/// Does not modify `links` — the caller replaces them in `materialize` based on the returned chains.
pub(crate) fn identify_fusion_chains(
    machine_names: &[String],
    machines: &BTreeMap<String, Box<dyn RunningMachine>>,
    links: &[PhysicalLink],
) -> Vec<FusionChain> {
    // Fusion-candidate links: Inline + both ends fusible + no fan-out (unique src_port) + no fan-in (unique Inline dst in-edge)
    let is_candidate = |link: &PhysicalLink| -> bool {
        if !matches!(link.kind, axiom::link::LinkKind::Inline) {
            return false;
        }
        let src_ok = machines.get(&link.src_machine)
            .map(|m| m.is_fused_compatible())
            .unwrap_or(false);
        let dst_ok = machines.get(&link.dst_machine)
            .map(|m| m.is_fused_compatible())
            .unwrap_or(false);
        if !src_ok || !dst_ok {
            return false;
        }
        // No fan-out: that src_machine's src_port has only this one Inline out-edge.
        let fan_out = links.iter().filter(|l| {
            l.src_machine == link.src_machine
                && l.src_port == link.src_port
                && matches!(l.kind, axiom::link::LinkKind::Inline)
        }).count();
        if fan_out > 1 {
            return false;
        }
        // No fan-in: that dst_machine has only this one Inline in-edge.
        let fan_in = links.iter().filter(|l| {
            l.dst_machine == link.dst_machine
                && matches!(l.kind, axiom::link::LinkKind::Inline)
        }).count();
        if fan_in > 1 {
            return false;
        }
        true
    };

    // Candidate out-edge index: machine_name → (src_port, dst_machine, dst_port)
    let mut candidate_out: BTreeMap<String, (&'static str, String, String)> = BTreeMap::new();
    let mut candidate_in: BTreeMap<String, bool> = BTreeMap::new();
    for link in links {
        if is_candidate(link) {
            // src_port is &'static str — taken from the PortSchema, but PhysicalLink stores a String.
            // Here, link.src_port's &'static str — in fact MachineWrapper's port_name returns
            // &'static str. But PhysicalLink.src_port is a String.
            // Simplified: use leak to turn the String into a &'static str (the chain count is
            // bounded, so leaking is acceptable).
            let src_port: &'static str = Box::leak(link.src_port.clone().into_boxed_str());
            candidate_out.insert(
                link.src_machine.clone(),
                (src_port, link.dst_machine.clone(), link.dst_port.clone()),
            );
            candidate_in.insert(link.dst_machine.clone(), true);
        }
    }

    // Find chain heads: has a candidate out-edge but no candidate in-edge.
    let chain_starts: Vec<&String> = machine_names.iter()
        .filter(|name| candidate_out.contains_key(*name) && !candidate_in.get(*name).copied().unwrap_or(false))
        .collect();

    let mut chains = Vec::new();
    for start in chain_starts {
        let mut chain_machines = vec![start.clone()];
        let mut chain_internal = Vec::new();
        let mut current = start.clone();

        loop {
            if let Some(&(src_port, ref dst_machine, ref dst_port)) = candidate_out.get(&current) {
                chain_internal.push((src_port, dst_port.clone()));
                chain_machines.push(dst_machine.clone());
                current = dst_machine.clone();
            } else {
                break;
            }
        }

        if chain_machines.len() >= 2 {
            chains.push(FusionChain {
                machines: chain_machines,
                internal: chain_internal,
            });
        }
    }

    chains
}

/// Apply fusion replacement to `LiveTopology`'s machines and links.
///
/// Returns `(fused_machines, fused_links, fused_topo_order, fused_machine_index, fused_in_degree)`:
/// - the chain machines are removed and replaced by a single `FusedPipeline` (named after the head);
/// - the in-chain links are removed; out-of-chain links point to the head name (the tail's
///   out-edges) or stay unchanged (the head's in-edges);
/// - `machine_index`/`in_degree`/`topo_order` are rebuilt accordingly.
pub(crate) fn apply_fusion(
    machines: BTreeMap<String, Box<dyn RunningMachine>>,
    links: Vec<PhysicalLink>,
    topo_order: Vec<String>,
) -> (
    BTreeMap<String, Box<dyn RunningMachine>>,
    Vec<PhysicalLink>,
    Vec<String>,
    BTreeMap<String, usize>,
    Vec<usize>,
) {
    let chains = identify_fusion_chains(&topo_order, &machines, &links);

    if chains.is_empty() {
        // No fusible chains — rebuild the indexes directly.
        let machine_index: BTreeMap<String, usize> = topo_order
            .iter().enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        let mut in_degree: Vec<usize> = alloc::vec![0; topo_order.len()];
        for link in &links {
            if let Some(&idx) = machine_index.get(&link.dst_machine) {
                in_degree[idx] += 1;
            }
        }
        return (machines, links, topo_order, machine_index, in_degree);
    }

    // Head → chain (to look up which machines belong to which chain, and which links are in-chain).
    let mut chain_by_head: BTreeMap<String, &FusionChain> = BTreeMap::new();
    // All machine names in chains (for removal).
    let mut fused_machine_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for chain in &chains {
        chain_by_head.insert(chain.machines[0].clone(), chain);
        for name in &chain.machines {
            fused_machine_names.insert(name.clone());
        }
    }

    // Remove the chain machines from machines and build the FusedPipeline.
    let mut machines = machines;
    let mut fused_pipelines: BTreeMap<String, Box<dyn RunningMachine>> = BTreeMap::new();
    for chain in &chains {
        let head = &chain.machines[0];
        let mut stages: Vec<Box<dyn RunningMachine>> = Vec::new();
        for name in &chain.machines {
            let m = machines.remove(name).expect("fused machine exists");
            stages.push(m);
        }
        let pipeline = FusedPipeline::new(
            stages,
            chain.internal.clone(),
            head.clone(),
        );
        fused_pipelines.insert(head.clone(), Box::new(pipeline));
    }

    // Filter links: remove in-chain links, keep out-of-chain links.
    // The tail's out-edges still use the tail name — they must be remapped to the head name.
    let mut chain_head_by_member: BTreeMap<String, String> = BTreeMap::new();
    for chain in &chains {
        for name in &chain.machines {
            chain_head_by_member.insert(name.clone(), chain.machines[0].clone());
        }
    }

    // Decide whether a link is an in-chain link (src and dst are in the same chain, and src is
    // dst's predecessor).
    let is_internal_link = |link: &PhysicalLink| -> bool {
        // Check whether it matches some chain's internal link.
        for chain in &chains {
            for (i, (src_port, dst_port)) in chain.internal.iter().enumerate() {
                if link.src_machine == chain.machines[i]
                    && link.src_port.as_str() == *src_port
                    && link.dst_machine == chain.machines[i + 1]
                    && link.dst_port == *dst_port
                {
                    return true;
                }
            }
        }
        false
    };

    let mut fused_links: Vec<PhysicalLink> = Vec::new();
    for link in links {
        if is_internal_link(&link) {
            continue; // the in-chain link has been internalized into the FusedPipeline
        }
        // Remap src/dst to the head name (if it belongs to some chain).
        let src_machine = chain_head_by_member.get(&link.src_machine)
            .cloned().unwrap_or(link.src_machine.clone());
        let dst_machine = chain_head_by_member.get(&link.dst_machine)
            .cloned().unwrap_or(link.dst_machine.clone());
        fused_links.push(PhysicalLink {
            src_machine,
            src_port: link.src_port,
            dst_machine,
            dst_port: link.dst_port,
            kind: link.kind,
        });
    }

    // Merge machines: the chain machines were already removed; add the FusedPipeline.
    // At this point machines only contains non-chain machines.
    for (head, pipeline) in fused_pipelines {
        machines.insert(head, pipeline);
    }

    // Rebuild topo_order: keep the original order, but only keep the head for chain machines.
    let mut fused_topo_order: Vec<String> = Vec::new();
    for name in &topo_order {
        if fused_machine_names.contains(name) {
            // Keep it only at the head position.
            if chain_by_head.contains_key(name) {
                fused_topo_order.push(name.clone());
            }
        } else {
            fused_topo_order.push(name.clone());
        }
    }

    // Rebuild the indexes.
    let machine_index: BTreeMap<String, usize> = fused_topo_order
        .iter().enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut in_degree: Vec<usize> = alloc::vec![0; fused_topo_order.len()];
    for link in &fused_links {
        if let Some(&idx) = machine_index.get(&link.dst_machine) {
            in_degree[idx] += 1;
        }
    }

    (machines, fused_links, fused_topo_order, machine_index, in_degree)
}
