//! Graph-theoretic topology analysis for [`DeploySpec`].
//!
//! A `DeploySpec` is a labeled directed multigraph $\Sigma = (V, E, \ell)$
//! (see `docs/architecture.md` §1). This module provides the static analysis
//! algorithms described in `docs/architecture.md` §3, split into two tiers:
//!
//! - **Enforced** (wired into [`DeploySpec::validate_deep`](crate::deploy::DeploySpec::validate_deep)):
//!   edge-degree constraints, Inline acyclicity. These are correctness
//!   invariants — a violation is a `ValidationError`, not a warning.
//!
//! - **Advisory** (returned by [`analyze`]): feedback loops (SCC), single
//!   points of failure (dominator analysis), observability completeness
//!   (reachability), orphan detection. These describe potential design
//!   issues but do not make the topology invalid.
//!
//! # Algorithm references
//!
//! | Algorithm | Reference |
//! |-----------|-----------|
//! | Kahn's topological sort | Kahn (1962); same approach as `topology::detect_cycle` |
//! | Tarjan's SCC | Tarjan (1972), iterative formulation |
//! | BFS reachability | Standard |
//! | Dominator analysis | Cooper–Harvey–Kennedy (2001), iterative data-flow |
//!
//! All algorithms are **iterative** (no recursion) to avoid stack overflow on
//! large topologies and to remain compatible with `no_std`.

#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use crate::compat::{HashMap, HashSet, VecDeque};
use crate::deploy::DeploySpec;
use crate::link::LinkKind;
use crate::port::PortSchema;

// ════════════════════════════════════════════════════════════════════════════
// Section 1: Public types (warnings, report)
// ════════════════════════════════════════════════════════════════════════════

/// An advisory warning from topology analysis.
///
/// Warnings describe potential design issues. They do **not** make a topology
/// invalid — [`validate_deep`](crate::deploy::DeploySpec::validate_deep) is the
/// authority on validity. A clean topology may still produce warnings.
#[derive(Debug, Clone)]
pub enum TopologyWarning {
    /// An observe port has no directed path to any collector/sink machine.
    ///
    /// Theorem 7.2 (observability completeness): an observe output is consumed
    /// iff a directed path exists from the observe port to a collector. A
    /// disconnected observe port silently loses its data.
    DisconnectedObserve {
        machine: String,
        port: String,
    },

    /// A feedback loop (strongly connected component with size > 1) was
    /// detected.
    ///
    /// `all_moore` indicates whether every machine on the loop declares Moore
    /// semantics — if so, the loop is algebraically safe (Theorem 1.2a).
    /// `has_inline` indicates whether any edge in the loop is `Inline` — if
    /// so, the loop is a deadlock (caught as an error by `validate_deep`, but
    /// reported here for completeness).
    FeedbackLoop {
        machines: Vec<String>,
        all_moore: bool,
        has_inline: bool,
    },

    /// A machine is a single point of failure: every path from any source to
    /// at least one sink passes through it. If this machine fails, the listed
    /// sinks become unreachable.
    SinglePointOfFailure {
        vertex: String,
        threatens: Vec<String>,
    },

    /// A machine with no inbound or no outbound edges (or both). May indicate
    /// a configuration error — or an intentional Source/Sink terminal.
    Orphan {
        machine: String,
        has_inbound: bool,
        has_outbound: bool,
    },
}

impl core::fmt::Display for TopologyWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DisconnectedObserve { machine, port } => write!(
                f,
                "disconnected observe port '{}::{}' — no path to any collector",
                machine, port,
            ),
            Self::FeedbackLoop { machines, all_moore, has_inline } => {
                write!(
                    f,
                    "feedback loop: {} | all_moore={} has_inline={}",
                    machines.join(" → "),
                    all_moore,
                    has_inline,
                )
            }
            Self::SinglePointOfFailure { vertex, threatens } => write!(
                f,
                "SPOF '{}' threatens sinks: {}",
                vertex,
                threatens.join(", "),
            ),
            Self::Orphan { machine, has_inbound, has_outbound } => {
                let kind = match (has_inbound, has_outbound) {
                    (false, false) => "isolated (no in, no out)",
                    (false, true) => "root (no inbound)",
                    (true, false) => "leaf (no outbound)",
                    _ => unreachable!(),
                };
                write!(f, "orphan machine '{}': {}", machine, kind)
            }
        }
    }
}

/// A structured feedback loop (SCC with size > 1).
#[derive(Debug, Clone)]
pub struct FeedbackLoop {
    /// Machine names on the loop, in SCC discovery order.
    pub machines: Vec<String>,
    /// Whether every machine on the loop declares Moore semantics.
    pub all_moore: bool,
    /// Whether any edge in the loop is `LinkKind::Inline`.
    pub has_inline: bool,
}

/// A single point of failure.
#[derive(Debug, Clone)]
pub struct SinglePointOfFailure {
    /// The machine whose failure disconnects the topology.
    pub vertex: String,
    /// Sinks that become unreachable if `vertex` fails.
    pub threatens: Vec<String>,
}

/// A report of advisory topology analysis.
///
/// Produced by [`analyze`]. An empty report means no advisory issues were
/// found — but this does **not** mean the topology is valid; call
/// [`validate_deep`](crate::deploy::DeploySpec::validate_deep) for validity.
#[derive(Debug, Clone, Default)]
pub struct TopologyReport {
    pub warnings: Vec<TopologyWarning>,
}

impl TopologyReport {
    /// No warnings were produced.
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }

    /// Number of warnings.
    pub fn len(&self) -> usize {
        self.warnings.len()
    }

    /// Iterate over warnings.
    pub fn iter(&self) -> impl Iterator<Item = &TopologyWarning> {
        self.warnings.iter()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 2: Graph construction helpers
// ════════════════════════════════════════════════════════════════════════════

/// Collect machine names from the spec, sorted for deterministic output.
fn machine_names_sorted(spec: &DeploySpec) -> Vec<&str> {
    let mut names: Vec<&str> = spec.machines.iter().map(|m| m.name.as_ref()).collect();
    names.sort();
    names
}

/// Build a set of machine name references for fast membership testing.
fn machine_name_set(spec: &DeploySpec) -> HashSet<&str> {
    spec.machines.iter().map(|m| m.name.as_ref()).collect()
}

/// Build an adjacency list from *all* machine-to-machine links (any `LinkKind`).
///
/// Funcs are excluded — they have no output edges and cannot participate in a
/// cycle. Self-loops are excluded (already rejected by `validate()`).
/// Edges to the same destination are kept (multigraph).
fn build_adjacency_all(spec: &DeploySpec) -> HashMap<&str, Vec<&str>> {
    let machine_set = machine_name_set(spec);
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in &spec.machines {
        adj.entry(m.name.as_ref()).or_default();
    }
    for link in &spec.links {
        let src: &str = link.out.0.as_ref();
        let dst: &str = link.into.0.as_ref();
        if src == dst { continue; } // skip self-loops
        if !machine_set.contains(src) || !machine_set.contains(dst) { continue; }
        adj.entry(src).or_default().push(dst);
    }
    // Sort adjacency lists for deterministic traversal.
    for neighbors in adj.values_mut() {
        neighbors.sort();
    }
    adj
}

/// Build an adjacency list from `Inline` links only.
fn build_adjacency_inline(spec: &DeploySpec) -> HashMap<&str, Vec<&str>> {
    let machine_set = machine_name_set(spec);
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for m in &spec.machines {
        adj.entry(m.name.as_ref()).or_default();
    }
    for link in &spec.links {
        if !matches!(link.kind, LinkKind::Inline) { continue; }
        let src: &str = link.out.0.as_ref();
        let dst: &str = link.into.0.as_ref();
        if src == dst { continue; }
        if !machine_set.contains(src) || !machine_set.contains(dst) { continue; }
        adj.entry(src).or_default().push(dst);
    }
    for neighbors in adj.values_mut() {
        neighbors.sort();
    }
    adj
}

// ════════════════════════════════════════════════════════════════════════════
// Section 3: Core algorithms
// ════════════════════════════════════════════════════════════════════════════

/// Kahn's topological sort.
///
/// Returns `Ok(order)` if the graph is acyclic, or `Err(cycle_nodes)` if a
/// cycle exists. `cycle_nodes` lists all nodes that remain after Kahn's
/// pruning (i.e., nodes with non-zero in-degree at the end).
fn kahn_toposort<'a>(
    adj: &HashMap<&'a str, Vec<&'a str>>,
    nodes: &[&'a str],
) -> Result<Vec<&'a str>, Vec<&'a str>> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for &n in nodes {
        in_degree.insert(n, 0);
    }
    for neighbors in adj.values() {
        for &dst in neighbors {
            *in_degree.entry(dst).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = nodes.iter()
        .filter(|&&n| *in_degree.get(n).unwrap_or(&0) == 0)
        .copied()
        .collect();

    let mut order: Vec<&str> = Vec::with_capacity(nodes.len());
    while let Some(node) = queue.pop_front() {
        order.push(node);
        if let Some(neighbors) = adj.get(node) {
            for &neighbor in neighbors {
                if let Some(d) = in_degree.get_mut(neighbor) {
                    *d -= 1;
                    if *d == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    if order.len() == nodes.len() {
        Ok(order)
    } else {
        // Remaining nodes with in-degree > 0 are on cycles.
        let cycle: Vec<&str> = nodes.iter()
            .filter(|&&n| *in_degree.get(n).unwrap_or(&0) > 0)
            .copied()
            .collect();
        Err(cycle)
    }
}

/// Tarjan's strongly connected components (iterative formulation).
///
/// Returns a list of SCCs. Each SCC is a `Vec` of node names. SCCs with
/// size > 1 are feedback loops. Singleton SCCs are not loops (unless they
/// have a self-loop, but self-loops are already rejected by `validate()`).
///
/// # Complexity
///
/// O(V + E) — single DFS with lowlink tracking.
fn tarjan_scc<'a>(
    adj: &HashMap<&'a str, Vec<&'a str>>,
    nodes: &[&'a str],
) -> Vec<Vec<&'a str>> {
    let mut index_counter: usize = 0;
    let mut stack: Vec<&str> = Vec::new();
    let mut on_stack: HashSet<&str> = HashSet::new();
    let mut indices: HashMap<&str, usize> = HashMap::new();
    let mut lowlinks: HashMap<&str, usize> = HashMap::new();
    let mut result: Vec<Vec<&str>> = Vec::new();

    for &start in nodes {
        if indices.contains_key(start) {
            continue;
        }

        // Iterative DFS work stack: (node, next-neighbor-index).
        let mut work: Vec<(&str, usize)> = Vec::new();
        indices.insert(start, index_counter);
        lowlinks.insert(start, index_counter);
        index_counter += 1;
        stack.push(start);
        on_stack.insert(start);
        work.push((start, 0));

        while let Some(&(node, i)) = work.last() {
            let neighbors = adj.get(node).map(|v| v.as_slice()).unwrap_or(&[]);
            if i < neighbors.len() {
                // Advance the child pointer.
                work.last_mut().unwrap().1 = i + 1;
                let next = neighbors[i];

                if !indices.contains_key(next) {
                    // Tree edge — recurse.
                    indices.insert(next, index_counter);
                    lowlinks.insert(next, index_counter);
                    index_counter += 1;
                    stack.push(next);
                    on_stack.insert(next);
                    work.push((next, 0));
                } else if on_stack.contains(next) {
                    // Back edge — update lowlink.
                    let next_idx = indices[next];
                    let ll = lowlinks.get_mut(node).unwrap();
                    if next_idx < *ll {
                        *ll = next_idx;
                    }
                }
                // Cross/forward edge to a finished node: ignore.
            } else {
                // All neighbors processed — check if `node` is an SCC root.
                let node_lowlink = *lowlinks.get(node).unwrap();
                let node_index = *indices.get(node).unwrap();
                if node_lowlink == node_index {
                    let mut scc: Vec<&str> = Vec::new();
                    loop {
                        let top = stack.pop().unwrap();
                        on_stack.remove(top);
                        scc.push(top);
                        if top == node {
                            break;
                        }
                    }
                    result.push(scc);
                }
                work.pop();
                // Propagate lowlink to parent.
                if let Some(&(parent, _)) = work.last() {
                    let child_lowlink = *lowlinks.get(node).unwrap();
                    let parent_ll = lowlinks.get_mut(parent).unwrap();
                    if child_lowlink < *parent_ll {
                        *parent_ll = child_lowlink;
                    }
                }
            }
        }
    }

    result
}

/// BFS reachability from `source` along the adjacency list.
///
/// Returns the set of nodes reachable from `source` (excluding `source`
/// itself, unless there is a cycle back to it).
fn bfs_reachable<'a>(
    adj: &HashMap<&'a str, Vec<&'a str>>,
    source: &'a str,
) -> HashSet<&'a str> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&str> = VecDeque::new();

    if let Some(neighbors) = adj.get(source) {
        for &n in neighbors {
            if visited.insert(n) {
                queue.push_back(n);
            }
        }
    }

    while let Some(node) = queue.pop_front() {
        if let Some(neighbors) = adj.get(node) {
            for &n in neighbors {
                if visited.insert(n) {
                    queue.push_back(n);
                }
            }
        }
    }

    visited
}

/// Cooper–Harvey–Kennedy iterative dominator analysis.
///
/// Computes the immediate dominator of each node reachable from a virtual
/// root. The virtual root connects to all `roots` (source vertices).
///
/// Returns `doms` where `doms[node_index]` is the index of the immediate
/// dominator, or `usize::MAX` if the node is unreachable or is the virtual
/// root itself.
///
/// # References
///
/// Cooper, Harvey, Kennedy. "A Simple, Fast Dominance Algorithm." (2001).
/// O(n²) worst-case, near-linear in practice for sparse graphs.
fn chk_dominators(
    adj: &[Vec<usize>],
    num_real_nodes: usize,
    virtual_root: usize,
) -> Vec<usize> {
    let total = num_real_nodes + 1; // +1 for virtual root

    // ── 1. Iterative postorder DFS from virtual root ──────────────────────
    let mut postorder: Vec<usize> = Vec::with_capacity(total);
    let mut postorder_idx: Vec<usize> = vec![usize::MAX; total];
    {
        let mut visited = vec![false; total];
        let mut work: Vec<(usize, usize)> = Vec::new();
        visited[virtual_root] = true;
        work.push((virtual_root, 0));
        while let Some(&(node, i)) = work.last() {
            let neighbors = &adj[node];
            if i < neighbors.len() {
                work.last_mut().unwrap().1 = i + 1;
                let child = neighbors[i];
                if !visited[child] {
                    visited[child] = true;
                    work.push((child, 0));
                }
            } else {
                let idx = postorder.len();
                postorder_idx[node] = idx;
                postorder.push(node);
                work.pop();
            }
        }
    }

    // ── 2. Initialize dominators ──────────────────────────────────────────
    let mut dom: Vec<usize> = vec![usize::MAX; total];
    dom[virtual_root] = virtual_root;

    // ── 3. Build predecessor lists ────────────────────────────────────────
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); total];
    for u in 0..total {
        for &v in &adj[u] {
            preds[v].push(u);
        }
    }

    // ── 4. Iterate until fixpoint (reverse postorder) ─────────────────────
    let mut changed = true;
    while changed {
        changed = false;
        // Reverse postorder = postorder reversed (root is last in postorder).
        for &node in postorder.iter().rev() {
            if node == virtual_root {
                continue;
            }

            let mut new_idom: usize = usize::MAX;
            for &pred in &preds[node] {
                if dom[pred] != usize::MAX {
                    if new_idom == usize::MAX {
                        new_idom = pred;
                    } else {
                        new_idom = chk_intersect(new_idom, pred, &dom, &postorder_idx);
                    }
                }
            }

            // Unreachable nodes have no reachable predecessor — skip.
            if new_idom == usize::MAX {
                continue;
            }

            if dom[node] != new_idom {
                dom[node] = new_idom;
                changed = true;
            }
        }
    }

    dom
}

/// CHK `intersect` helper: walk up the dominator tree using postorder indices.
fn chk_intersect(
    mut b1: usize,
    mut b2: usize,
    dom: &[usize],
    postorder_idx: &[usize],
) -> usize {
    while b1 != b2 {
        while postorder_idx[b1] < postorder_idx[b2] {
            b1 = dom[b1];
        }
        while postorder_idx[b2] < postorder_idx[b1] {
            b2 = dom[b2];
        }
    }
    b1
}

// ════════════════════════════════════════════════════════════════════════════
// Section 4: Public API — enforced checks (called by validate_deep)
// ════════════════════════════════════════════════════════════════════════════

/// Check per-port edge-degree constraints for constrained `LinkKind` variants.
///
/// Constraints (from `docs/architecture.md` §2):
/// - `Inline`: outdeg(src port) ≤ 1
/// - `Channel`: indeg(dst port) ≤ 1 (single consumer)
/// - `CasFreeRing`: outdeg(src port) ≤ 1, indeg(dst port) ≤ 1 (SPSC)
///
/// Returns a sorted list of violations (machine, port, kind, limit, actual).
/// An empty vector means no violations.
pub fn degree_violations(spec: &DeploySpec) -> Vec<DegreeViolation> {
    // Key: (machine, port, is_outgoing, kind_tag)
    // We count only constrained kinds.
    let mut counts: HashMap<(&str, &str, bool, u8), usize> = HashMap::new();

    for link in &spec.links {
        let (src_m, src_p) = (link.out.0.as_ref(), link.out.1.as_ref());
        let (dst_m, dst_p) = (link.into.0.as_ref(), link.into.1.as_ref());

        let tag = match &link.kind {
            LinkKind::Inline => 0u8,
            LinkKind::Channel { .. } => 1u8,
            LinkKind::CasFreeRing { .. } => 2u8,
            _ => continue, // unconstrained kinds (BoundedBuf, Latest, SharedState)
        };

        *counts.entry((src_m, src_p, true, tag)).or_insert(0) += 1;
        *counts.entry((dst_m, dst_p, false, tag)).or_insert(0) += 1;
    }

    let mut violations: Vec<DegreeViolation> = Vec::new();
    for ((machine, port, is_out, tag), &count) in &counts {
        let (limit, kind_name, dir_name) = match (tag, is_out) {
            (0, true) => (1, "Inline", "output"),
            (1, false) => (1, "Channel", "input"),
            (2, true) => (1, "CasFreeRing", "output"),
            (2, false) => (1, "CasFreeRing", "input"),
            _ => continue, // no constraint for this (kind, direction) pair
        };
        if count > limit {
            violations.push(DegreeViolation {
                machine: machine.to_string(),
                port: port.to_string(),
                link_kind: kind_name.to_string(),
                direction: dir_name.to_string(),
                limit,
                actual: count,
            });
        }
    }

    // Sort for deterministic error reporting.
    violations.sort_by(|a, b| {
        (&a.machine, &a.port, &a.link_kind).cmp(&(&b.machine, &b.port, &b.link_kind))
    });
    violations
}

/// A degree-constraint violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegreeViolation {
    pub machine: String,
    pub port: String,
    pub link_kind: String,
    pub direction: String,
    pub limit: usize,
    pub actual: usize,
}

/// Check Inline acyclicity: the subgraph induced by `Inline` edges must be a
/// DAG (Theorem: Inline cycle → deadlock).
///
/// Returns `Ok(())` if the Inline subgraph is acyclic, or `Err(cycle)` with
/// the machine names that form the cycle.
pub fn inline_cycle(spec: &DeploySpec) -> Option<Vec<String>> {
    let nodes = machine_names_sorted(spec);
    let adj = build_adjacency_inline(spec);
    match kahn_toposort(&adj, &nodes) {
        Ok(_) => None,
        Err(cycle_nodes) => Some(cycle_nodes.iter().map(|s| s.to_string()).collect()),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 5: Public API — advisory analysis
// ════════════════════════════════════════════════════════════════════════════

/// Topological order of the Inline subgraph.
///
/// Returns `Ok(order)` if the Inline subgraph is a DAG, or `Err(cycle)` if a
/// cycle exists. The order is a valid execution sequence for Inline-linked
/// machines on the same thread.
pub fn inline_topological_order(spec: &DeploySpec) -> Result<Vec<String>, Vec<String>> {
    let nodes = machine_names_sorted(spec);
    let adj = build_adjacency_inline(spec);
    match kahn_toposort(&adj, &nodes) {
        Ok(order) => Ok(order.iter().map(|s| s.to_string()).collect()),
        Err(cycle) => Err(cycle.iter().map(|s| s.to_string()).collect()),
    }
}

/// Critical-path latency — the longest per-message latency path through the
/// topology (acyclic), summing each machine's
/// [`per_message_latency_us`](crate::resource::MachinePhysicalSpec::per_message_latency_us).
///
/// This is a **latency budget** analysis: a blueprint whose declared critical
/// path exceeds a real-time budget is physically infeasible, regardless of
/// structural validity. Machines with `0` (undeclared) latency contribute zero.
///
/// Returns `Ok(total_us)` for acyclic topologies, or `Err(cycle_nodes)` when a
/// cycle makes the critical path unbounded (a cycle must be broken by a Moore
/// machine / channel delay before a finite latency bound exists).
///
/// # Algorithm
///
/// Longest-path DP over a Kahn topological order:
/// `dist[v] = latency(v) + max(dist[u] for u → v)`.
pub fn critical_path_latency(spec: &DeploySpec) -> Result<u64, Vec<String>> {
    let nodes = machine_names_sorted(spec);
    let adj = build_adjacency_all(spec);

    let order = kahn_toposort(&adj, &nodes)
        .map_err(|cycle| cycle.iter().map(|s| s.to_string()).collect::<Vec<String>>())?;

    let latency: HashMap<&str, u64> = spec
        .machines
        .iter()
        .map(|m| (m.name.as_ref(), m.physical.per_message_latency_us))
        .collect();

    // dist[v] = longest latency path ending at v (inclusive of v's latency).
    let mut dist: HashMap<&str, u64> = HashMap::new();
    for &n in &nodes {
        dist.insert(n, latency.get(n).copied().unwrap_or(0));
    }

    for &node in &order {
        let base = dist.get(node).copied().unwrap_or(0);
        if let Some(neighbors) = adj.get(node) {
            for &nb in neighbors {
                let candidate = base + latency.get(nb).copied().unwrap_or(0);
                let entry = dist.entry(nb).or_insert(0);
                if candidate > *entry {
                    *entry = candidate;
                }
            }
        }
    }

    Ok(dist.values().copied().max().unwrap_or(0))
}

/// Topological levels — the layer of each machine in the topology (acyclic).
///
/// A machine's level is the length (in edges) of the longest path from any
/// source: sources are `0`, a downstream machine is
/// `max(level(u) for u → v) + 1`. This is the basis for **wave scheduling**
/// (dependency-aware execution): machines at the same level are independent
/// and may execute in parallel; levels must execute in order.
///
/// Returns `Ok(levels)` for acyclic topologies, or `Err(cycle_nodes)` when a
/// cycle makes levels undefined (wave scheduling applies to DAGs).
///
/// # Example
///
/// ```text
/// A → (B, C) → D          levels: A=0, B=1, C=1, D=2
/// ```
pub fn topological_levels(spec: &DeploySpec) -> Result<HashMap<String, usize>, Vec<String>> {
    let nodes = machine_names_sorted(spec);
    let adj = build_adjacency_all(spec);

    let order = kahn_toposort(&adj, &nodes)
        .map_err(|cycle| cycle.iter().map(|s| s.to_string()).collect::<Vec<String>>())?;

    let mut level: HashMap<String, usize> = HashMap::new();
    for &n in &nodes {
        level.insert(n.to_string(), 0);
    }

    for &node in &order {
        let lvl = *level.get(node).unwrap_or(&0);
        if let Some(neighbors) = adj.get(node) {
            for &nb in neighbors {
                let candidate = lvl + 1;
                let entry = level.entry(nb.to_string()).or_insert(0);
                if candidate > *entry {
                    *entry = candidate;
                }
            }
        }
    }

    Ok(level)
}

/// Detect feedback loops via Tarjan's SCC algorithm.
///
/// Returns all SCCs with size > 1 (feedback loops). Each `FeedbackLoop`
/// includes whether all machines on it are Moore and whether any edge is
/// Inline.
pub fn feedback_loops(spec: &DeploySpec) -> Vec<FeedbackLoop> {
    let nodes = machine_names_sorted(spec);
    let adj = build_adjacency_all(spec);
    let sccs = tarjan_scc(&adj, &nodes);

    // Build a quick lookup: machine name → is_moore
    let moore_set: HashSet<&str> = spec.machines.iter()
        .filter(|m| m.is_moore)
        .map(|m| m.name.as_ref())
        .collect();

    // Build a set of (src, dst) Inline edges for has_inline check.
    let inline_edges: HashSet<(&str, &str)> = spec.links.iter()
        .filter(|l| matches!(l.kind, LinkKind::Inline))
        .map(|l| (l.out.0.as_ref(), l.into.0.as_ref()))
        .collect();

    let mut loops: Vec<FeedbackLoop> = Vec::new();
    for scc in sccs {
        if scc.len() < 2 {
            continue; // singleton = not a loop (self-loops already rejected)
        }

        let scc_set: HashSet<&str> = scc.iter().copied().collect();

        let all_moore = scc.iter().all(|m| moore_set.contains(m));

        // Check if any edge within the SCC is Inline.
        let has_inline = spec.links.iter().any(|l| {
            let src: &str = l.out.0.as_ref();
            let dst: &str = l.into.0.as_ref();
            scc_set.contains(src) && scc_set.contains(dst) && inline_edges.contains(&(src, dst))
        });

        loops.push(FeedbackLoop {
            machines: scc.iter().map(|s| s.to_string()).collect(),
            all_moore,
            has_inline,
        });
    }
    loops
}

/// All machines reachable from `source` (excluding `source` itself, unless
/// there is a cycle back).
pub fn reachable_from(spec: &DeploySpec, source: &str) -> Vec<String> {
    let adj = build_adjacency_all(spec);
    let mut reached: Vec<&str> = bfs_reachable(&adj, source).into_iter().collect();
    reached.sort();
    reached.iter().map(|s| s.to_string()).collect()
}

/// Whether `target` is reachable from `source`.
pub fn can_reach(spec: &DeploySpec, source: &str, target: &str) -> bool {
    let adj = build_adjacency_all(spec);
    bfs_reachable(&adj, source).contains(target)
}

/// Detect single points of failure via dominator analysis.
///
/// A machine `v` is a SPOF if every path from any source (in-degree 0) to at
/// least one sink (out-degree 0) passes through `v`. The virtual root
/// connects to all sources, so `v` dominates a sink from the virtual root iff
/// it dominates that sink from every source.
pub fn single_points_of_failure(spec: &DeploySpec) -> Vec<SinglePointOfFailure> {
    let names = machine_names_sorted(spec);
    let n = names.len();
    if n == 0 {
        return Vec::new();
    }

    // Map name → index.
    let name_to_idx: HashMap<&str, usize> = names.iter()
        .enumerate()
        .map(|(i, &name)| (name, i))
        .collect();

    // Build index-based adjacency (all edges).
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];
    let machine_set = machine_name_set(spec);
    for link in &spec.links {
        let src: &str = link.out.0.as_ref();
        let dst: &str = link.into.0.as_ref();
        if src == dst { continue; }
        if !machine_set.contains(src) || !machine_set.contains(dst) { continue; }
        if let (Some(&s), Some(&d)) = (name_to_idx.get(src), name_to_idx.get(dst)) {
            adj[s].push(d);
            in_degree[d] += 1;
        }
    }
    for neighbors in &mut adj {
        neighbors.sort();
    }

    // Find sources (in-degree 0) and sinks (out-degree 0).
    let sources: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let sinks: Vec<usize> = (0..n).filter(|&i| adj[i].is_empty()).collect();

    if sources.is_empty() || sinks.is_empty() {
        return Vec::new(); // no sources or no sinks → no SPOF in dominator sense
    }

    // Virtual root connects to all sources.
    let virtual_root = n;
    let mut full_adj = adj.clone();
    full_adj.push(sources.clone()); // virtual root → all sources

    // Run CHK dominator analysis.
    let dom = chk_dominators(&full_adj, n, virtual_root);

    // For each sink, walk the dominator chain to find SPOFs.
    // Sources (in-degree 0) and the sink itself are excluded — sources are
    // entry points (their "failure" = no input, a different concern), and
    // the sink is the chain's starting point.
    let mut spof_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for &sink in &sinks {
        let mut current = dom[sink];
        while current != virtual_root && current != usize::MAX {
            if in_degree[current] > 0 {
                spof_map.entry(current).or_default().push(sink);
            }
            current = dom[current];
        }
    }

    // Convert to sorted output.
    let mut result: Vec<SinglePointOfFailure> = spof_map.into_iter()
        .map(|(spof_idx, threatened_sinks)| {
            let mut sinks_sorted = threatened_sinks;
            sinks_sorted.sort();
            SinglePointOfFailure {
                vertex: names[spof_idx].to_string(),
                threatens: sinks_sorted.iter().map(|&i| names[i].to_string()).collect(),
            }
        })
        .collect();
    result.sort_by(|a, b| a.vertex.cmp(&b.vertex));
    result
}

/// Check observability completeness (Theorem 7.2).
///
/// For each machine's observe ports, verify that a directed path exists to a
/// collector machine (a machine that consumes observe data without re-emitting
/// it — i.e., has no observe output port of its own).
///
/// # Port-level precision
///
/// Unlike machine-level reachability (which would treat all of a machine's
/// observe ports as equally connected), this analysis builds a **port-level**
/// graph and BFSes from each observe port individually. This correctly
/// distinguishes the case where one observe port is wired to a collector
/// while another observe port on the same machine is unwired.
///
/// The port-level graph has two edge kinds:
/// - **Link edges**: `(src_machine, src_port) → (dst_machine, dst_port)` for
///   each `LinkSpec` (data crosses machine boundaries).
/// - **Internal edges**: `(M, in_port) → (M, out_port)` for each input/output
///   pair of a machine (data consumed at any input may be re-emitted at any
///   output — a sound over-approximation of the machine's internal logic).
///
/// Requires `PortSchema` for each machine to identify observe ports and to
/// build internal edges. Machines with unknown schemas are treated as
/// collectors (the "unknown → benign" policy to avoid false positives).
pub fn observe_completeness(
    spec: &DeploySpec,
    schemas: &HashMap<&str, PortSchema>,
) -> Vec<TopologyWarning> {
    // ── 1. Identify collectors (machines with no observe output port) ──────
    // A machine with an unknown schema is assumed to be a collector — this is
    // the "unknown → benign" policy: we avoid false positives by assuming an
    // unknown machine absorbs observe data rather than re-emitting it.
    let is_collector: HashSet<&str> = spec.machines.iter()
        .filter(|m| {
            let schema = schemas.get(m.name.as_ref());
            match schema {
                Some(s) => s.observe_ports().count() == 0,
                None => true, // unknown schema → assume collector (don't warn)
            }
        })
        .map(|m| m.name.as_ref())
        .collect();

    // ── 2. Build port-level adjacency ─────────────────────────────────────
    // Nodes: (machine_name, port_name). Edges:
    //   (a) Link edges: (src_m, src_p) → (dst_m, dst_p) for each LinkSpec.
    //   (b) Internal edges: (M, in_p) → (M, out_p) for each machine's
    //       input/output pair — data consumed at any input may be re-emitted
    //       at any output. Observe ports are outputs; they have no internal
    //       in-edges (only link out-edges).
    type PortNode<'a> = (&'a str, &'a str);
    let machine_set = machine_name_set(spec);
    let mut adj: HashMap<PortNode<'_>, Vec<PortNode<'_>>> = HashMap::new();

    // Internal edges (need schema to know input/output ports).
    for m in &spec.machines {
        let schema = match schemas.get(m.name.as_ref()) {
            Some(s) => s,
            None => continue, // unknown schema — skip internal edges
        };
        let name: &str = m.name.as_ref();
        // Ensure all port nodes exist as keys (even isolated ones).
        for p in schema.inputs() { adj.entry((name, p.name)).or_default(); }
        for p in schema.outputs() { adj.entry((name, p.name)).or_default(); }
        // Internal edges: each input → each output.
        for in_p in schema.inputs() {
            for out_p in schema.outputs() {
                adj.entry((name, in_p.name)).or_default().push((name, out_p.name));
            }
        }
    }

    // Link edges.
    for link in &spec.links {
        let src_m: &str = link.out.0.as_ref();
        let src_p: &str = link.out.1.as_ref();
        let dst_m: &str = link.into.0.as_ref();
        let dst_p: &str = link.into.1.as_ref();
        if !machine_set.contains(src_m) || !machine_set.contains(dst_m) { continue; }
        adj.entry((src_m, src_p)).or_default().push((dst_m, dst_p));
    }

    // ── 3. For each observe port, port-level BFS to any collector ─────────
    let mut warnings: Vec<TopologyWarning> = Vec::new();
    for m in &spec.machines {
        let schema = match schemas.get(m.name.as_ref()) {
            Some(s) => s,
            None => continue,
        };
        for port in schema.observe_ports() {
            let start: PortNode<'_> = (m.name.as_ref(), port.name);
            let mut visited: HashSet<PortNode<'_>> = HashSet::new();
            let mut queue: VecDeque<PortNode<'_>> = VecDeque::new();
            visited.insert(start);
            queue.push_back(start);

            let mut reaches_collector = false;
            while let Some(node) = queue.pop_front() {
                // If this node's machine is a collector, the observe data has
                // a path to a sink. (The start node's machine is never a
                // collector — it has an observe output port by definition.)
                if is_collector.contains(node.0) {
                    reaches_collector = true;
                    break;
                }
                if let Some(neighbors) = adj.get(&node) {
                    for &next in neighbors {
                        if visited.insert(next) {
                            queue.push_back(next);
                        }
                    }
                }
            }

            if !reaches_collector {
                warnings.push(TopologyWarning::DisconnectedObserve {
                    machine: m.name.to_string(),
                    port: port.name.to_string(),
                });
            }
        }
    }

    warnings.sort_by(|a, b| {
        let (ma, pa) = match a {
            TopologyWarning::DisconnectedObserve { machine, port } => (machine.clone(), port.clone()),
            _ => (String::new(), String::new()),
        };
        let (mb, pb) = match b {
            TopologyWarning::DisconnectedObserve { machine, port } => (machine.clone(), port.clone()),
            _ => (String::new(), String::new()),
        };
        (ma, pa).cmp(&(mb, pb))
    });
    warnings
}

/// Detect orphan machines: no inbound edges, no outbound edges, or both.
pub fn orphans(spec: &DeploySpec) -> Vec<TopologyWarning> {
    let machine_set = machine_name_set(spec);
    let mut has_inbound: HashSet<&str> = HashSet::new();
    let mut has_outbound: HashSet<&str> = HashSet::new();

    for link in &spec.links {
        let src: &str = link.out.0.as_ref();
        let dst: &str = link.into.0.as_ref();
        if machine_set.contains(src) {
            has_outbound.insert(src);
        }
        if machine_set.contains(dst) {
            has_inbound.insert(dst);
        }
    }

    let mut warnings: Vec<TopologyWarning> = spec.machines.iter()
        .filter_map(|m| {
            let name = m.name.as_ref();
            let inb = has_inbound.contains(name);
            let outb = has_outbound.contains(name);
            if !inb || !outb {
                Some(TopologyWarning::Orphan {
                    machine: name.to_string(),
                    has_inbound: inb,
                    has_outbound: outb,
                })
            } else {
                None
            }
        })
        .collect();
    warnings.sort_by(|a, b| {
        let ma = match a { TopologyWarning::Orphan { machine, .. } => machine, _ => &String::new() };
        let mb = match b { TopologyWarning::Orphan { machine, .. } => machine, _ => &String::new() };
        ma.cmp(mb)
    });
    warnings
}

/// Run all advisory analyses and collect into a [`TopologyReport`].
///
/// `schemas` is needed for observability completeness. Pass `None` to skip
/// that check (e.g., when schemas are not available).
///
/// This function does **not** validate the topology — it assumes structural
/// validity (call `validate_deep` first). Warnings are advisory.
pub fn analyze(
    spec: &DeploySpec,
    schemas: Option<&HashMap<&str, PortSchema>>,
) -> TopologyReport {
    let mut warnings: Vec<TopologyWarning> = Vec::new();

    // Feedback loops (SCC).
    for fl in feedback_loops(spec) {
        warnings.push(TopologyWarning::FeedbackLoop {
            machines: fl.machines,
            all_moore: fl.all_moore,
            has_inline: fl.has_inline,
        });
    }

    // Single points of failure (dominators).
    for spof in single_points_of_failure(spec) {
        warnings.push(TopologyWarning::SinglePointOfFailure {
            vertex: spof.vertex,
            threatens: spof.threatens,
        });
    }

    // Orphan detection.
    warnings.extend(orphans(spec));

    // Observability completeness (optional — needs schemas).
    if let Some(schemas) = schemas {
        warnings.extend(observe_completeness(spec, schemas));
    }

    TopologyReport { warnings }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 6: Unit tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::{DeploySpec, MachineInstance};
    use crate::link::{LinkKind, LinkSpec, WritePolicy, ReadPolicy, MemoryRegion};
    use crate::port::{PortDecl, PortSchema};
    use crate::resource::MachinePhysicalSpec;

    fn machine(name: &'static str) -> MachineInstance {
        MachineInstance::new(name, "test", MachinePhysicalSpec::default())
    }

    /// Machine with a declared per-message latency (microseconds).
    fn machine_lat(name: &'static str, us: u64) -> MachineInstance {
        let mut physical = MachinePhysicalSpec::default();
        physical.per_message_latency_us = us;
        MachineInstance::new(name, "test", physical)
    }

    fn machine_moore(name: &'static str) -> MachineInstance {
        MachineInstance::new(name, "test", MachinePhysicalSpec::default()).moore()
    }

    fn inline(a: &'static str, pa: &'static str, b: &'static str, pb: &'static str) -> LinkSpec {
        LinkSpec::new((a, pa), (b, pb), LinkKind::Inline)
    }

    fn bounded(a: &'static str, pa: &'static str, b: &'static str, pb: &'static str) -> LinkSpec {
        LinkSpec::new((a, pa), (b, pb), LinkKind::BoundedBuf {
            capacity: 16,
            write_policy: WritePolicy::Blocking,
            read_policy: ReadPolicy::Blocking,
        })
    }

    fn channel(a: &'static str, pa: &'static str, b: &'static str, pb: &'static str) -> LinkSpec {
        LinkSpec::new((a, pa), (b, pb), LinkKind::Channel { capacity: 16, drop_when_full: false })
    }

    fn casfree(a: &'static str, pa: &'static str, b: &'static str, pb: &'static str) -> LinkSpec {
        LinkSpec::new((a, pa), (b, pb), LinkKind::CasFreeRing {
            capacity: 16,
            storage: MemoryRegion::Heap { size: 1024 },
        })
    }

    // ── Kahn topological sort ──────────────────────────────────────────

    #[test]
    fn test_kahn_dag() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(inline("a", "out", "b", "in"))
            .with_link(inline("b", "out", "c", "in"));
        let order = inline_topological_order(&spec).unwrap();
        let pos = |name: &str| order.iter().position(|s| s == name).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("b") < pos("c"));
    }

    #[test]
    fn test_kahn_cycle_detected() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"))
            .with_link(inline("b", "out", "a", "in"));
        let cycle = inline_cycle(&spec);
        assert!(cycle.is_some());
        let cycle = cycle.unwrap();
        assert!(cycle.contains(&"a".to_string()));
        assert!(cycle.contains(&"b".to_string()));
    }

    #[test]
    fn test_inline_cycle_not_triggered_by_bounded() {
        // BoundedBuf cycle is allowed (sequential feedback).
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "a", "in"));
        assert!(inline_cycle(&spec).is_none());
    }

    // ── Tarjan SCC ─────────────────────────────────────────────────────

    #[test]
    fn test_scc_no_loops() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "c", "in"));
        let loops = feedback_loops(&spec);
        assert!(loops.is_empty());
    }

    #[test]
    fn test_scc_feedback_loop() {
        let spec = DeploySpec::new()
            .with_machine(machine_moore("a"))
            .with_machine(machine_moore("b"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "a", "in"));
        let loops = feedback_loops(&spec);
        assert_eq!(loops.len(), 1);
        assert!(loops[0].all_moore);
        assert!(!loops[0].has_inline);
    }

    #[test]
    fn test_scc_has_inline() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "a", "in"));
        let loops = feedback_loops(&spec);
        assert_eq!(loops.len(), 1);
        assert!(loops[0].has_inline);
    }

    // ── BFS reachability ───────────────────────────────────────────────

    #[test]
    fn test_reachable() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_machine(machine("d"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "c", "in"))
            .with_link(bounded("c", "out", "b", "in")); // cycle b→c
        let reach = reachable_from(&spec, "a");
        assert!(reach.contains(&"b".to_string()));
        assert!(reach.contains(&"c".to_string()));
        assert!(!reach.contains(&"d".to_string()));
        assert!(can_reach(&spec, "a", "c"));
        assert!(!can_reach(&spec, "a", "d"));
    }

    // ── Dominator analysis / SPOF ──────────────────────────────────────

    #[test]
    fn test_spof_linear() {
        // a → b → c → d  (b and c are SPOFs for d)
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_machine(machine("d"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "c", "in"))
            .with_link(bounded("c", "out", "d", "in"));
        let spofs = single_points_of_failure(&spec);
        let vertices: Vec<&str> = spofs.iter().map(|s| s.vertex.as_str()).collect();
        assert!(vertices.contains(&"b"));
        assert!(vertices.contains(&"c"));
        assert!(!vertices.contains(&"a")); // source, not a SPOF
        assert!(!vertices.contains(&"d")); // sink, not a SPOF
    }

    #[test]
    fn test_spof_no_spof_with_redundancy() {
        // a → b → d
        // a → c → d  (b and c are NOT SPOFs — redundancy)
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_machine(machine("d"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("a", "out", "c", "in"))
            .with_link(bounded("b", "out", "d", "in"))
            .with_link(bounded("c", "out", "d", "in"));
        let spofs = single_points_of_failure(&spec);
        assert!(spofs.is_empty(), "redundant paths → no SPOF, got: {:?}", spofs);
    }

    // ── Degree constraints ─────────────────────────────────────────────

    #[test]
    fn test_degree_inline_ok() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(inline("a", "out", "b", "in"));
        assert!(degree_violations(&spec).is_empty());
    }

    #[test]
    fn test_degree_inline_violation() {
        // Two Inline links from the same output port → violation
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(inline("a", "out", "b", "in"))
            .with_link(inline("a", "out", "c", "in"));
        let v = degree_violations(&spec);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].machine, "a");
        assert_eq!(v[0].link_kind, "Inline");
        assert_eq!(v[0].actual, 2);
    }

    #[test]
    fn test_degree_channel_violation() {
        // Two Channel links into the same input port → violation
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(channel("a", "out", "c", "in"))
            .with_link(channel("b", "out", "c", "in"));
        let v = degree_violations(&spec);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].machine, "c");
        assert_eq!(v[0].link_kind, "Channel");
    }

    #[test]
    fn test_degree_casfree_spsc() {
        // CasFreeRing: both outdeg and indeg must be ≤ 1
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(casfree("a", "out", "b", "in"))
            .with_link(casfree("c", "out", "b", "in")); // indeg violation
        let v = degree_violations(&spec);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].link_kind, "CasFreeRing");
        assert_eq!(v[0].direction, "input");
    }

    #[test]
    fn test_degree_boundedbuf_no_constraint() {
        // BoundedBuf has no degree constraint — multiple links OK
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(bounded("a", "out", "c", "in"))
            .with_link(bounded("b", "out", "c", "in"));
        assert!(degree_violations(&spec).is_empty());
    }

    // ── Orphan detection ───────────────────────────────────────────────

    #[test]
    fn test_orphans() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))   // root (no inbound)
            .with_machine(machine("b"))   // middle
            .with_machine(machine("c"))   // leaf (no outbound)
            .with_machine(machine("d"))   // isolated
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "c", "in"));
        let warns = orphans(&spec);
        let names: Vec<String> = warns.iter().filter_map(|w| {
            match w {
                TopologyWarning::Orphan { machine, .. } => Some(machine.clone()),
                _ => None,
            }
        }).collect();
        assert!(names.contains(&"a".to_string())); // root
        assert!(names.contains(&"c".to_string())); // leaf
        assert!(names.contains(&"d".to_string())); // isolated
        assert!(!names.contains(&"b".to_string())); // middle — not orphan
    }

    // ── Observe completeness ───────────────────────────────────────────

    #[test]
    fn test_observe_disconnected() {
        let spec = DeploySpec::new()
            .with_machine(machine("sensor"))
            .with_machine(machine("store"));
        let mut schemas = HashMap::new();
        schemas.insert("sensor", PortSchema::new()
            .with(PortDecl::output::<i32>("out"))
            .with(PortDecl::observe::<i32>("metrics")));
        schemas.insert("store", PortSchema::new()
            .with(PortDecl::input::<i32>("in")));
        // No link from sensor::metrics to store → disconnected
        let warns = observe_completeness(&spec, &schemas);
        assert_eq!(warns.len(), 1);
        match &warns[0] {
            TopologyWarning::DisconnectedObserve { machine, port } => {
                assert_eq!(machine, "sensor");
                assert_eq!(port, "metrics");
            }
            _ => panic!("expected DisconnectedObserve"),
        }
    }

    #[test]
    fn test_observe_connected() {
        let spec = DeploySpec::new()
            .with_machine(machine("sensor"))
            .with_machine(machine("store"))
            .with_link(bounded("sensor", "metrics", "store", "in"));
        let mut schemas = HashMap::new();
        schemas.insert("sensor", PortSchema::new()
            .with(PortDecl::observe::<i32>("metrics")));
        schemas.insert("store", PortSchema::new()
            .with(PortDecl::input::<i32>("in"))); // store is a collector (no observe out)
        let warns = observe_completeness(&spec, &schemas);
        assert!(warns.is_empty(), "observe port is connected, got: {:?}", warns);
    }

    // ── Full analyze ───────────────────────────────────────────────────

    #[test]
    fn test_analyze_clean() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        let report = analyze(&spec, None);
        // a is root (orphan), b is leaf (orphan) — expected warnings
        assert!(!report.is_clean());
        assert!(report.warnings.iter().any(|w| matches!(w, TopologyWarning::Orphan { .. })));
    }

    // ── Critical-path latency ──────────────────────────────────────────

    #[test]
    fn critical_path_linear() {
        // A(1) → B(2) → C(3)：关键路径 = 1 + 2 + 3 = 6
        let spec = DeploySpec::new()
            .with_machine(machine_lat("a", 1))
            .with_machine(machine_lat("b", 2))
            .with_machine(machine_lat("c", 3))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "c", "in"));
        assert_eq!(critical_path_latency(&spec).unwrap(), 6);
    }

    #[test]
    fn critical_path_diamond_takes_longer_branch() {
        // A(1) → (B(2), C(3)) → D(4)：关键路径 = 1 + max(2,3) + 4 = 8
        let spec = DeploySpec::new()
            .with_machine(machine_lat("a", 1))
            .with_machine(machine_lat("b", 2))
            .with_machine(machine_lat("c", 3))
            .with_machine(machine_lat("d", 4))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("a", "out", "c", "in"))
            .with_link(bounded("b", "out", "d", "in"))
            .with_link(bounded("c", "out", "d", "in"));
        assert_eq!(critical_path_latency(&spec).unwrap(), 8);
    }

    #[test]
    fn critical_path_undeclared_latency_is_zero() {
        // 全部 latency=0（默认）：关键路径 = 0
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"));
        assert_eq!(critical_path_latency(&spec).unwrap(), 0);
    }

    #[test]
    fn critical_path_cycle_is_unbounded() {
        // A → B → A（环）：关键路径无界，返回 Err
        let spec = DeploySpec::new()
            .with_machine(machine_lat("a", 1))
            .with_machine(machine_lat("b", 1))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "a", "in"));
        assert!(critical_path_latency(&spec).is_err());
    }

    // ── Topological levels（波次调度基础）───────────────────────────────

    #[test]
    fn levels_linear_chain() {
        // A → B → C：A=0, B=1, C=2
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "c", "in"));
        let levels = topological_levels(&spec).unwrap();
        assert_eq!(levels["a"], 0);
        assert_eq!(levels["b"], 1);
        assert_eq!(levels["c"], 2);
    }

    #[test]
    fn levels_diamond() {
        // A → (B, C) → D：A=0, B=1, C=1, D=2
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_machine(machine("c"))
            .with_machine(machine("d"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("a", "out", "c", "in"))
            .with_link(bounded("b", "out", "d", "in"))
            .with_link(bounded("c", "out", "d", "in"));
        let levels = topological_levels(&spec).unwrap();
        assert_eq!(levels["a"], 0);
        assert_eq!(levels["b"], 1);
        assert_eq!(levels["c"], 1);
        assert_eq!(levels["d"], 2);
    }

    #[test]
    fn levels_cycle_is_err() {
        let spec = DeploySpec::new()
            .with_machine(machine("a"))
            .with_machine(machine("b"))
            .with_link(bounded("a", "out", "b", "in"))
            .with_link(bounded("b", "out", "a", "in"));
        assert!(topological_levels(&spec).is_err());
    }
}

