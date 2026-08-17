//! The axiom unified runtime — the `Runtime` structure and the `materialize`/`tick`/`shutdown`
//! lifecycle.
//!
//! The driving loop dispatches by `RuntimeConfig::mode`:
//! - `Sequential` / `Inline` → single-threaded BFS (direct move delivery);
//! - `Parallel(n)` → one OS thread per machine + channel carriers.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use axiom::port::PortDir;

use crate::carrier::{channel_for, ChanReceiver, ChanSender, RoutedMsg};
use crate::config::RuntimeConfig;
use crate::erasure::{ProcessResult, RunningMachine};
use crate::error::RuntimeError;
use crate::io::{IoInterest, IoReactor, IoToken, RawIo};
use crate::registry::Registry;
use crate::routing::{has_cycle, mark_stopped, route_parallel_outputs, validate_endpoint};
use crate::topology::{LiveTopology, PhysicalLink, TopologyIds};

/// The axiom unified runtime.
pub struct Runtime {
    config: RuntimeConfig,
    registry: Registry,
    topology: Option<LiveTopology>,
    /// The scheduler (an internal subsystem contract: Sequential/Parallel/custom, selected at
    /// construction from `RuntimeConfig::mode` — see `scheduler.rs`). `Option` lets `tick`
    /// take it out (avoiding the double borrow of `&self.scheduler` and `&mut self`).
    scheduler: Option<Box<dyn crate::scheduler::Scheduler>>,
    /// IO multiplexing routing table: token → (machine_name, port_name).
    /// Filled by `register_io`, queried by `run_io` — converts reactor readiness events into
    /// `(machine, port, IoEvent)` inputs injected into the tick loop.
    io_routing: BTreeMap<IoToken, (String, String)>,
}

impl Runtime {
    pub fn new(config: RuntimeConfig) -> Self {
        let scheduler = crate::scheduler::default_scheduler(&config);
        Self { config, registry: Registry::new(), topology: None, scheduler: Some(scheduler), io_routing: BTreeMap::new() }
    }

    pub fn default() -> Self {
        Self::new(RuntimeConfig::default())
    }

    pub fn config(&self) -> &RuntimeConfig { &self.config }

    pub fn register<M>(&mut self, machine_type: &str)
    where
        M: axiom::machine::Machine,
        M::Input: core::any::Any + Send,
        M::Output: core::any::Any + Send,
    {
        self.registry.register::<M>(machine_type);
    }

    /// Register a **Moore-semantics** machine — `M: axiom::machine::Moore` guarantees at the
    /// type level that output depends only on the pre-update state. At materialization the
    /// `is_moore` declaration is checked against the implementation (S3-2): only types
    /// registered via `register_moore` may declare Moore semantics.
    pub fn register_moore<M>(&mut self, machine_type: &str)
    where
        M: axiom::machine::Machine + axiom::machine::Moore,
        M::Input: core::any::Any + Send,
        M::Output: core::any::Any + Send,
    {
        self.registry.register_moore::<M>(machine_type);
    }

    /// Register a fusable machine — `M: FusedInline` guarantees a fixed output count at the
    /// type level, so `materialize` can fold it into a `FusedPipeline` chain (eliminating
    /// per-hop routing overhead).
    pub fn register_fused<M>(&mut self, machine_type: &str)
    where
        M: axiom::machine::Machine + axiom::machine::FusedInline,
        M::Input: core::any::Any + Send + axiom::portset::Pack,
        M::Output: core::any::Any + Send + axiom::portset::Unpack,
        M::ProcessOutput: axiom::machine::FusedCompatible,
    {
        self.registry.register_fused::<M>(machine_type);
    }

    /// Register a composite Machine — a sub-topology plus port mapping wrapped into a single
    /// `machine_type`.
    ///
    /// When `materialize` encounters an instance of such a type it expands it recursively: the
    /// sub-machines are namespaced as `parent.sub`, and external links are redirected to the
    /// sub-machines via the port mapping table. Expansion happens before machine construction,
    /// endpoint validation and fusion — fusion sees the expanded flat topology, so a
    /// `FusedPipeline` can fuse across the original composite boundaries.
    ///
    /// Nested composites (a composite `machine_type` used again inside a sub-topology) are
    /// handled by iterative expansion until no composite remains (depth cap 64, preventing
    /// infinite expansion from misconfiguration).
    pub fn register_composite(&mut self, machine_type: &str, spec: axiom::composite::CompositeSpec) {
        self.registry.register_composite(machine_type, spec);
    }

    /// Materialize a `DynamicTopology` — interpret the pure-data topology as runtime entities.
    ///
    /// Materialization steps:
    /// 1. `DynamicTopology::validate()` structural checks (name uniqueness, self-loops, degree
    ///    constraints);
    /// 2. **Composite expansion** — replace composite instances registered via
    ///    `register_composite` with namespaced sub-topologies + redirected external links
    ///    (recursive to any depth);
    /// 3. Build machine instances from the expanded `machine_type`s;
    /// 4. `validate_endpoint` checks port existence + direction;
    /// 5. `apply_fusion` fuses adjacent FusedInline chains.
    pub fn materialize(&mut self, spec: &axiom::deploy::DynamicTopology) -> Result<(), RuntimeError> {
        spec.validate().map_err(|e| RuntimeError::InitFailed {
            machine: "<spec>".into(),
            error: axiom::machine::InitError::Other(format!("validate failed: {e:?}")),
        })?;

        // Composite expansion: replace composite machine_type instances with sub-topologies
        // (namespaced). Expansion runs after validate (the raw structure is legal) and before
        // machine construction (the expanded topology is the real one). Fusion sees the
        // expanded flat topology — composite boundaries have disappeared, so a FusedPipeline
        // can fuse across the original composite boundaries.
        let (expanded_machines, expanded_links) = axiom::composite::expand_composites(
            spec.machines.clone(),
            spec.links.clone(),
            self.registry.composites(),
        ).map_err(|e| match e {
            axiom::composite::CompositeError::TooDeep { depth, hint } => {
                RuntimeError::CompositeTooDeep { depth, hint }
            }
            other => RuntimeError::InitFailed {
                machine: "<composite>".into(),
                error: axiom::machine::InitError::Other(format!("composite error: {other}")),
            },
        })?;

        let mut machines: BTreeMap<String, Box<dyn RunningMachine>> = BTreeMap::new();

        for instance in &expanded_machines {
            let machine_type = instance.machine_type.as_ref();
            let name = instance.name.as_ref();

            // S3-2: Moore declaration contract check — a machine whose `is_moore` is declared
            // true must have its type registered via `register_moore` (the type-level guarantee
            // that it implements the `Moore` trait). A mismatch between declaration and
            // implementation (e.g. a machine registered via plain `register` declared Moore)
            // would mislead `validate_deep`'s cycle-safety analysis (falsely believing it can
            // break a feedback loop) — rejected at deployment time.
            if instance.is_moore && !self.registry.is_moore(machine_type) {
                return Err(RuntimeError::MooreMismatch {
                    machine: name.to_string(),
                    machine_type: machine_type.to_string(),
                });
            }

            // MachineContext accepts Cow: clone instance.name (a Cow) directly — zero-copy
            // when borrowed, one transfer when owned — no need to leak to 'static.
            let ctx = axiom::port::MachineContext::new(instance.name.clone());

            let machine = self.registry.build(machine_type, ctx)?;
            machines.insert(name.to_string(), machine);
        }

        for link in &expanded_links {
            validate_endpoint(&machines, link.out.0.as_ref(), link.out.1.as_ref(), PortDir::Out)?;
            validate_endpoint(&machines, link.into.0.as_ref(), link.into.1.as_ref(), PortDir::In)?;
        }

        let topo_order: Vec<String> = expanded_machines.iter()
            .map(|m| m.name.as_ref().to_string())
            .collect();

        let links: Vec<PhysicalLink> = expanded_links.iter()
            .map(|l| PhysicalLink {
                src_machine: l.out.0.as_ref().to_string(),
                src_port: l.out.1.as_ref().to_string(),
                dst_machine: l.into.0.as_ref().to_string(),
                dst_port: l.into.1.as_ref().to_string(),
                kind: l.kind.clone(),
            })
            .collect();

        // pipelineN fusion: replace adjacent FusedInline machine chains with a FusedPipeline,
        // eliminating the per-hop route lookup and queue overhead. apply_fusion rebuilds
        // machines/links/topo_order/machine_index/in_degree so that subsequent ticks see the
        // fused topology (links outside the chain point at the chain head's name; in-chain
        // links are internalized).
        let (machines, links, topo_order, machine_index, in_degree) =
            crate::fusion::apply_fusion(machines, links, topo_order);

        // P2: build the routing index — a materialization-time fact (covering the fused
        // topology), giving O(log L) lookups on the tick hot path and eliminating
        // route_target's O(L) linear scan.
        let mut route_map: BTreeMap<String, BTreeMap<String, (String, String)>> = BTreeMap::new();
        for l in &links {
            route_map
                .entry(l.src_machine.clone())
                .or_default()
                .insert(l.src_port.clone(), (l.dst_machine.clone(), l.dst_port.clone()));
        }

        // P0: ID-based routing index — no string matching or String clone on the tick hot
        // path. Machines are numbered by topo_order (= machine_index values), ports by their
        // inputs()/outputs() order in the schema (&'static str, known at compile time).
        let mut route_by_src: Vec<Vec<(&'static str, usize, u16)>> =
            vec![Vec::new(); machines.len()];
        let mut out_port_names: Vec<Vec<&'static str>> = Vec::with_capacity(machines.len());
        let mut in_port_names: Vec<Vec<&'static str>> = Vec::with_capacity(machines.len());
        for name in &topo_order {
            let schema = machines[name].port_schema();
            out_port_names.push(schema.outputs().map(|p| p.name).collect());
            in_port_names.push(schema.inputs().map(|p| p.name).collect());
        }
        for l in &links {
            let src_id = *machine_index.get(&l.src_machine).expect("link src machine indexed");
            let dst_id = *machine_index.get(&l.dst_machine).expect("link dst machine indexed");
            // src_port (a String) is matched against the schema-ordered &'static str
            // (validate_endpoint already guarantees its existence); dst_port resolves to the
            // target machine's input port ID.
            let src_port = out_port_names[src_id]
                .iter()
                .copied()
                .find(|p| *p == l.src_port.as_str())
                .unwrap_or("");
            let dst_pid = in_port_names[dst_id]
                .iter()
                .position(|p| *p == l.dst_port.as_str())
                .unwrap_or(0) as u16;
            route_by_src[src_id].push((src_port, dst_id, dst_pid));
        }
        let ids = TopologyIds { route_by_src, out_port_names, in_port_names };

        self.topology = Some(LiveTopology {
            machines,
            links,
            topo_order,
            machine_index,
            in_degree,
            route_map,
            ids,
        });
        Ok(())
    }

    pub fn topology(&self) -> Option<&LiveTopology> { self.topology.as_ref() }

    /// The driving loop: dispatches by `RuntimeConfig::mode`.
    ///
    /// - `Sequential` / `Inline`: single-threaded BFS driving (direct move delivery).
    /// - `Parallel(n)`: one thread per machine + channel carriers (see [`Self::drive_parallel`]).
    ///
    /// Delegated through [`crate::scheduler::Scheduler`] — the scheduling strategy is a
    /// replaceable internal subsystem (structural consistency: the runtime itself is organized
    /// as "modules + contracts").
    pub fn tick(
        &mut self,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        // Take the scheduler out (avoiding the double borrow of &self.scheduler and &mut self),
        // and put it back after tick — the scheduler is pure policy (no cross-tick state).
        let scheduler = self.scheduler.take().expect("runtime scheduler present");
        let result = scheduler.tick(self, inputs);
        self.scheduler = Some(scheduler);
        result
    }

    /// Single-threaded BFS driving loop: inject external inputs (machine, port, payload) →
    /// process → route outputs to downstream machines per `LinkSpec` → propagate level by
    /// level until no new output remains.
    ///
    /// # Routing semantics
    ///
    /// Each output value (matched against `PhysicalLink` by port name):
    /// - If it hits a link → build the downstream input via `HasPortInfo::from_port_name` and
    ///   enqueue it (BFS level-by-level propagation);
    /// - If it misses (terminal machine / observation port with no downstream) → collect it
    ///   and return it as a final output.
    ///
    /// # LinkKind physicalization (Sequential mode)
    ///
    /// Under single-threaded sequential driving, every link kind (Inline/BoundedBuf/Channel/...)
    /// physicalizes to **direct move delivery**: producer and consumer run interleaved on the
    /// same thread, buffers never back up, and boundedness has no physical meaning — direct
    /// delivery is the equivalent physics.
    pub(crate) fn drive_sequential(
        &mut self,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        let topology = self.topology.as_mut().ok_or_else(|| RuntimeError::InitFailed {
            machine: "<none>".into(),
            error: axiom::machine::InitError::Other("runtime not materialized".into()),
        })?;

        let max_ticks = self.config.max_ticks;
        let ids = &topology.ids;
        let topo_order = &topology.topo_order;

        // P0: ID-based queue — (machine_id, port_id, payload), no String clone.
        // Externally injected (machine name, port name) is resolved to IDs once, at enqueue
        // time.
        let mut queue: std::collections::VecDeque<(usize, u16, Box<dyn core::any::Any + Send>)> =
            std::collections::VecDeque::with_capacity(inputs.len());
        for (name, port, payload) in inputs {
            let mid = *topology
                .machine_index
                .get(&name)
                .ok_or_else(|| RuntimeError::DanglingRef { machine: name.clone(), port: port.clone() })?;
            let pid = ids.in_port_names[mid]
                .iter()
                .position(|p| *p == port.as_str())
                .ok_or_else(|| RuntimeError::DanglingRef { machine: name.clone(), port: port.clone() })? as u16;
            queue.push_back((mid, pid, payload));
        }

        let mut outputs: Vec<ProcessResult> = Vec::new();
        let mut ticks: u64 = 0;

        // A1 shutdown propagation: pending_sources = a cloned copy of in_degree (indexed by
        // topo_order); stopped = a bit set of stopped machines (by ID, O(1) check).
        let mut pending_sources: Vec<usize> = topology.in_degree.clone();
        let machine_index = &topology.machine_index;
        let mut stopped: Vec<bool> = vec![false; topology.machines.len()];

        // H2 fairness: per-machine per-round processing quota (None = unlimited FIFO, the
        // default). Messages for a machine at its quota are deferred to the next round (quota
        // reset) — preventing a single flooding source from monopolizing BFS and starving other
        // sources.
        let fairness = self.config.max_messages_per_machine;
        let mut processed: Vec<u64> = vec![0; topology.machines.len()];
        let mut deferred: std::collections::VecDeque<(usize, u16, Box<dyn core::any::Any + Send>)> =
            std::collections::VecDeque::new();

        loop {
            let Some((mid, pid, payload)) = queue.pop_front() else {
                if deferred.is_empty() {
                    break;
                }
                // Round end: deferred messages move to the next round (quota reset),
                // propagation continues.
                std::mem::swap(&mut queue, &mut deferred);
                processed.fill(0);
                continue;
            };

            // Fairness: machine at quota → defer to the next round, prioritize other machines.
            if let Some(quota) = fairness {
                if quota > 0 && processed[mid] >= quota {
                    deferred.push_back((mid, pid, payload));
                    continue;
                }
            }
            processed[mid] += 1;

            // Stopped machine: drop subsequent messages (Done is a shutdown signal, not Idle).
            if stopped[mid] {
                continue;
            }
            ticks += 1;
            if let Some(limit) = max_ticks {
                if ticks > limit {
                    return Err(RuntimeError::TickLimitExceeded { ticks });
                }
            }

            let name = &topo_order[mid];
            let machine = topology.machines.get_mut(name).ok_or_else(|| {
                RuntimeError::DanglingRef { machine: name.clone(), port: String::new() }
            })?;
            let result = machine.inject(pid, payload);

            // Done = shutdown signal: this machine stops, and downstream machines whose
            // "in-edges are all from stopped sources" stop in cascade (explicit propagation,
            // not just ignoring).
            if matches!(result, ProcessResult::Done) {
                mark_stopped(mid, name, &mut stopped, &mut pending_sources, machine_index, &topology.links);
                continue;
            }

            // Routing: outputs look up downstream by port name; those without a downstream are
            // collected as terminal outputs.
            // P0: linear scan over route_by_src[mid] (output ports are usually 1-2), comparing
            // the yielded port name (&'static str) directly, no string-table lookup.
            match result {
                ProcessResult::Idle => {}
                ProcessResult::Yield { port, value } => {
                    if let Some((_, dst_mid, dst_pid)) = ids.route_by_src[mid]
                        .iter()
                        .find(|(sp, _, _)| *sp == port)
                    {
                        queue.push_back((*dst_mid, *dst_pid, value));
                    } else {
                        outputs.push(ProcessResult::Yield { port, value });
                    }
                }
                ProcessResult::YieldMulti { outputs: list } => {
                    for (port, value) in list {
                        if let Some((_, dst_mid, dst_pid)) = ids.route_by_src[mid]
                            .iter()
                            .find(|(sp, _, _)| *sp == port)
                        {
                            queue.push_back((*dst_mid, *dst_pid, value));
                        } else {
                            outputs.push(ProcessResult::Yield { port, value });
                        }
                    }
                }
                ProcessResult::Done => unreachable!("handled above"),
            }
        }
        Ok(outputs)
    }

    /// Multi-threaded driving: one OS thread per machine, links physicalized into real channels
    /// by `LinkKind`.
    ///
    /// # Physical carriers (selected by `LinkKind`, see [`carrier::channel_for`])
    ///
    /// - `BoundedBuf { Blocking }` / `Channel { !drop_when_full }` →
    ///   `sync_channel(capacity)` + blocking `send` (natural backpressure);
    /// - `BoundedBuf { Dropping }` / `Channel { drop_when_full }` →
    ///   `sync_channel` + `try_send` (drops the new message when full);
    /// - `BoundedBuf { Overwriting }` → **custom bounded overwrite carrier** (overwrites the
    ///   oldest when full — the native semantics, not a `try_send` approximation);
    /// - `Latest` / `SharedState` → **single-slot overwrite carrier** (the reader sees the
    ///   latest value);
    /// - `Inline` / `CasFreeRing` → unbounded `channel` (a cross-thread Inline is the semantic
    ///   migration of function-call → channel; the CasFreeRing lock-free carrier belongs to
    ///   embedded scenarios).
    ///
    /// `ReadPolicy::NonBlocking` is physicalized: with a single in-edge + BoundedBuf the thread
    /// polls via `try_recv` + `yield_now` (does not block the thread).
    ///
    /// # fan-in support
    ///
    /// A target machine may have multiple in-edges: one receiver per in-edge, and the machine
    /// thread consumes them merged via forward threads (injected in arrival order). Under
    /// fan-in `NonBlocking` degrades to blocking (the merged channel uses a blocking recv).
    ///
    /// # Limitations
    ///
    /// - Every machine must have an input port (Source-like machines without inputs are not yet
    ///   supported);
    /// - **Cyclic topologies**: threads in a cycle cannot cascade-shutdown via channel
    ///   disconnection (they keep each other alive), so a global `stop_signal`
    ///   (`Arc<AtomicBool>`) + a per-thread tick counter drives them — any thread reaching
    ///   `Done` or exceeding the tick limit triggers a global shutdown. Acyclic topologies
    ///   keep the existing channel-disconnection cascade shutdown path.
    ///
    /// # Shutdown: channel-disconnection cascade
    ///
    /// After tick injection, drop all entry senders → entry threads' `recv` returns `None` →
    /// they exit → drop their own output senders → downstream `recv` disconnects → cascade
    /// stops → `thread::scope` converges. Terminal outputs are collected through the result
    /// channel.
    pub(crate) fn drive_parallel(
        &mut self,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        use std::sync::mpsc;

        let topology = self.topology.as_mut().ok_or_else(|| RuntimeError::InitFailed {
            machine: "<none>".into(),
            error: axiom::machine::InitError::Other("runtime not materialized".into()),
        })?;
        let ids = &topology.ids;

        // ── 1. Build link channel carriers ──────────────────────────────────
        // A2: fan-in support — each target machine may have multiple in-edges, one receiver per
        // in-edge, and the machine thread consumes them merged via forward threads (see the
        // spawn logic below).
        // The channel carries (port_name, payload) — the port name travels with the message;
        // the thread loop injects with the received port name rather than a fixed port. This
        // unifies the inject semantics of Sequential/Parallel and prepares for future machines
        // with multiple input ports.

        // Output routing table: src_machine → (src_port → (dst_port, downstream carrier)).
        // dst_port travels with the link — it is attached to the message when routing, and the
        // downstream thread injects with it.
        // Grouped by machine; each machine thread owns **its own set of senders** — when the
        // thread exits it drops them → downstream recv disconnects → the cascade shutdown can
        // take effect. (If they were centralized in function-level variables, thread exit would
        // only drop references while the downstream senders stay alive, and downstream threads
        // would block forever — deadlock.)
        let mut out_routes: BTreeMap<
            String,
            BTreeMap<String, (String, ChanSender)>,
        > = BTreeMap::new();
        // Input receiver table: dst_machine → the list of receivers for its in-edges (A2
        // fan-in). dst_port is no longer stored — the port name travels with the message; the
        // receiver only needs to receive messages.
        let mut in_routes: BTreeMap<String, Vec<ChanReceiver>> = BTreeMap::new();
        // read_policy of single-in-edge machines (used for NonBlocking polling); defaults to
        // Blocking under fan-in or without a BoundedBuf in-edge.
        let mut in_policies: BTreeMap<String, axiom::link::ReadPolicy> = BTreeMap::new();
        for link in &topology.links {
            // Physicalize the carrier by LinkKind (see channel_for's carrier matrix).
            let (tx, rx) = channel_for(&link.kind);
            out_routes
                .entry(link.src_machine.clone())
                .or_default()
                .insert(link.src_port.clone(), (link.dst_port.clone(), tx));
            in_routes.entry(link.dst_machine.clone()).or_default().push(rx);
            if let axiom::link::LinkKind::BoundedBuf { read_policy, .. } = &link.kind {
                // Only effective with a single in-edge (fan-in is merged blockingly by forward
                // threads).
                in_policies.entry(link.dst_machine.clone()).or_insert(*read_policy);
            }
        }

        // Entry machines (no in-edges): tick holds the injection senders. Entry channels are
        // always unbounded (external injection should not be blocked by backpressure).
        // Source-like machines (no input ports) cannot be driven, because inject has no port to
        // match — error out directly.
        //
        // Special handling for cyclic topologies: every machine in a cycle has in-edges, but
        // external inputs still need injection. Create an extra entry channel for machines
        // referenced by external inputs — its receiver is forward-merged into the machine
        // thread together with the link carriers.
        let mut entry_txs: BTreeMap<String, mpsc::Sender<RoutedMsg>> = BTreeMap::new();
        let mut entry_rxs: BTreeMap<String, mpsc::Receiver<RoutedMsg>> = BTreeMap::new();
        for (name, machine) in &topology.machines {
            if in_routes.contains_key(name) {
                continue;
            }
            if machine.port_schema().inputs().next().is_none() {
                return Err(RuntimeError::UnsupportedTopology {
                    machine: name.clone(),
                    reason: "machine has no input port (Source-like) is not supported in Parallel mode".into(),
                });
            }
            let (tx, rx) = mpsc::channel::<RoutedMsg>();
            entry_txs.insert(name.clone(), tx);
            entry_rxs.insert(name.clone(), rx);
        }

        // Cycle detection: threads in a cycle cannot cascade-shutdown via channel disconnection
        // (they keep each other alive), so a global stop_signal + tick limit drives them
        // instead. Must be computed before the entry-channel handling — when cyclic, machines
        // that "already have in-edges but are referenced by external inputs" need an extra
        // entry channel (every machine in a cycle has in-edges, otherwise external inputs would
        // have nowhere to inject).
        let cyclic = has_cycle(&topology.topo_order, &topology.links);
        let stop_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let max_ticks = self.config.max_ticks.unwrap_or(1_000_000);

        // Cyclic topology: create entry channels for machines that are referenced by external
        // inputs but already have in-edges. These machines' threads consume from both the link
        // carriers and the entry channel (forward-merged).
        if cyclic {
            for (name, _, _) in &inputs {
                if !entry_txs.contains_key(name) && topology.machines.contains_key(name) {
                    let (tx, rx) = mpsc::channel::<RoutedMsg>();
                    entry_txs.insert(name.clone(), tx);
                    // The entry rx goes straight into in_routes (forward-merged together with
                    // the link carriers); it is not put into entry_rxs — to avoid double
                    // ownership.
                    in_routes.entry(name.clone()).or_default().push(ChanReceiver::Mpsc(rx));
                }
            }
        }

        // Result-collection channel (terminal outputs).
        let (result_tx, result_rx) = mpsc::channel::<ProcessResult>();

        // ── 2. Scoped threads: one per machine; injection and cascade shutdown also inside
        //        the scope ──
        // (Threads recv immediately after spawn; if injection happened outside the scope, the
        //  scope would block waiting for the threads to join while the threads wait forever for
        //  input — deadlock.)
        let machines: Vec<(&String, &mut Box<dyn RunningMachine>)> =
            topology.machines.iter_mut().collect();

        // Pre-injection validation: all external inputs must target entry machines.
        for (name, _, _) in &inputs {
            if !entry_txs.contains_key(name) {
                return Err(RuntimeError::DanglingRef {
                    machine: name.clone(),
                    port: "<entry>".to_string(),
                });
            }
        }

        std::thread::scope(|s| {
            for (name, machine) in machines {
                // The machine's input receiver: in_routes for machines with in-edges (possibly
                // multiple, A2 fan-in merged via forward threads), otherwise the entry
                // machine's entry_rxs.
                // The port name travels with the message (not stored on the receiver side) —
                // unifying the inject semantics of Sequential/Parallel.
                let rx = match in_routes.remove(name) {
                    Some(mut v) if v.len() == 1 => v.pop().expect("len 1"),
                    Some(v) => {
                        // fan-in: one forward thread per in-edge → merged channel → this thread
                        // recvs the merged rx (in arrival order). After all forward threads exit
                        // (upstream sender disconnection / stop_signal), the merged rx recv
                        // disconnects → cascade shutdown.
                        let (merge_tx, merge_rx) = mpsc::channel::<RoutedMsg>();
                        let stop_fwd = stop_signal.clone();
                        for rx in v {
                            let merge_tx = merge_tx.clone();
                            let stop_fwd = stop_fwd.clone();
                            s.spawn(move || {
                                if stop_fwd.load(std::sync::atomic::Ordering::Relaxed) {
                                    return;
                                }
                                // When cyclic, use try_recv + yield (avoiding a blocked forward
                                // thread that would prevent stop_signal from propagating); when
                                // acyclic, use a blocking recv.
                                if cyclic {
                                    loop {
                                        if stop_fwd.load(std::sync::atomic::Ordering::Relaxed) {
                                            break;
                                        }
                                        match rx.try_recv() {
                                            Ok(Some(msg)) => { let _ = merge_tx.send(msg); }
                                            Ok(None) => std::thread::yield_now(),
                                            Err(()) => break,
                                        }
                                    }
                                } else {
                                    while let Some(msg) = rx.recv() {
                                        let _ = merge_tx.send(msg);
                                    }
                                }
                            });
                        }
                        drop(merge_tx);
                        ChanReceiver::Mpsc(merge_rx)
                    }
                    None => ChanReceiver::Mpsc(
                        entry_rxs.remove(name).expect("entry machine has an entry channel"),
                    ),
                };
                // Owns this machine's output senders (dropped on exit → downstream cascade
                // shutdown).
                let my_routes = out_routes.remove(name).unwrap_or_default();
                let result_tx = &result_tx;
                // NonBlocking: single in-edge + BoundedBuf read_policy == NonBlocking.
                let non_blocking = in_policies.get(name).copied()
                    == Some(axiom::link::ReadPolicy::NonBlocking);
                let stop = stop_signal.clone();
                // P0: this machine's input port name table (used for String port → port_id).
                let mid = topology.machine_index.get(name).copied().unwrap_or(0);
                let in_names = &ids.in_port_names[mid];

                s.spawn(move || {
                    let handle: &mut Box<dyn RunningMachine> = machine;
                    // port name → port ID (linear scan; input ports are usually 1-2).
                    let pid_of = |port: &str| -> u16 {
                        in_names.iter().position(|p| *p == port).unwrap_or(0) as u16
                    };

                    if cyclic {
                        // Cyclic mode: driven by the global stop_signal + tick limit.
                        // Threads in a cycle cannot shut down via channel disconnection (they
                        // keep each other alive), so try_recv + yield + tick counting is used
                        // instead. Any thread hitting Done / the tick limit → set stop_signal →
                        // global shutdown.
                        let mut ticks: u64 = 0;
                        loop {
                            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            }
                            match rx.try_recv() {
                                Ok(Some((port, payload))) => {
                                    ticks += 1;
                                    if ticks > max_ticks {
                                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                                        break;
                                    }
                                    let result = handle.inject(pid_of(&port), payload);
                                    if matches!(result, ProcessResult::Done) {
                                        stop.store(true, std::sync::atomic::Ordering::Relaxed);
                                        break;
                                    }
                                    route_parallel_outputs(result, &my_routes, result_tx);
                                }
                                Ok(None) => std::thread::yield_now(),
                                Err(()) => break,
                            }
                        }
                    } else if non_blocking {
                        // ReadPolicy::NonBlocking: try_recv + yield (polling scheduling, does
                        // not block the thread); Err(()) = disconnection → exit (cascade
                        // shutdown).
                        loop {
                            match rx.try_recv() {
                                Ok(Some((port, payload))) => {
                                    let result = handle.inject(pid_of(&port), payload);
                                    if matches!(result, ProcessResult::Done) {
                                        break;
                                    }
                                    route_parallel_outputs(result, &my_routes, result_tx);
                                }
                                Ok(None) => std::thread::yield_now(),
                                Err(()) => break,
                            }
                        }
                    } else {
                        // Default (Blocking): blocking recv.
                        while let Some((port, payload)) = rx.recv() {
                            let result = handle.inject(pid_of(&port), payload);
                            // A1: Done = shutdown signal — exit immediately, no longer
                            // processing the channel backlog.
                            if matches!(result, ProcessResult::Done) {
                                break;
                            }
                            route_parallel_outputs(result, &my_routes, result_tx);
                        }
                    }
                    // rx disconnection / Done / stop_signal → thread exit → my_routes dropped →
                    // downstream recv disconnects → cascade shutdown (acyclic) or stop_signal
                    // propagation (cyclic).
                });
            }

            // Inject external inputs (threads already started recv, so sends are consumed
            // immediately). The port name is sent with the message — the thread loop injects
            // with the received port name.
            // Use get rather than remove: multiple inputs may inject into **the same** entry
            // machine (remove would fail on the second one after the first — a real bug caught
            // by the http_declarative acceptance test). The senders are dropped uniformly after
            // the injection loop.
            for (name, port, payload) in inputs {
                let tx = entry_txs.get(&name).expect("validated entry");
                let _ = tx.send((port, payload));
            }
            // Release all entry senders: entry threads' recv disconnects → cascade shutdown →
            // the scope converges (all threads join).
            drop(entry_txs);
        });

        // ── 4. Collect terminal outputs (until the result channel disconnects) ──────────
        // Must drop result_tx: threads in the scope only borrowed a reference to it, the
        // top-level sender still lives, otherwise recv would never see Err(disconnected).
        drop(result_tx);
        let mut outputs = Vec::new();
        while let Ok(r) = result_rx.recv() {
            outputs.push(r);
        }
        Ok(outputs)
    }

    /// Clean up all machines (cleanup in reverse order).
    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        if let Some(mut topology) = self.topology.take() {
            while let Some((_, machine)) = topology.machines.pop_last() {
                machine.cleanup()?;
            }
        }
        Ok(())
    }

    // ── IO multiplexing integration ───────────────────────────────────────
    //
    // External-registration model: the caller creates an IO source (e.g. TcpListener), takes
    // the raw fd/socket, and registers a token→(machine, port) mapping + reactor interest via
    // register_io. run_io polls the reactor → converts readiness events into inputs → merges
    // the external inputs → tick.
    //
    // The Machine::process signature is unchanged — the machine receives an IoEvent as an
    // ordinary typed input (the input port type includes an `IoEvent` variant) and performs the
    // actual IO (read/write/accept) inside process.

    /// Register an IO source's readiness interest + routing mapping.
    ///
    /// - `token`: a caller-assigned token associating readiness events with a machine.
    /// - `machine` / `port`: the target machine name and input port name to inject on
    ///   readiness.
    /// - `raw`: the OS-level fd (Unix) / socket (Windows).
    /// - `interest`: READABLE / WRITABLE / READ_WRITE.
    pub fn register_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        token: IoToken,
        machine: &str,
        port: &str,
        raw: RawIo,
        interest: IoInterest,
    ) -> Result<(), RuntimeError> {
        reactor.register(raw, interest, token).map_err(|e| RuntimeError::IoFailed { error: e })?;
        self.io_routing.insert(token, (machine.to_string(), port.to_string()));
        Ok(())
    }

    /// Update a registered IO source's interest (rearm under the readiness model).
    pub fn reregister_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        token: IoToken,
        machine: &str,
        port: &str,
        raw: RawIo,
        interest: IoInterest,
    ) -> Result<(), RuntimeError> {
        reactor.reregister(raw, interest, token).map_err(|e| RuntimeError::IoFailed { error: e })?;
        self.io_routing.insert(token, (machine.to_string(), port.to_string()));
        Ok(())
    }

    /// Deregister an IO source.
    pub fn deregister_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        raw: RawIo,
        token: IoToken,
    ) -> Result<(), RuntimeError> {
        reactor.deregister(raw).map_err(|e| RuntimeError::IoFailed { error: e })?;
        self.io_routing.remove(&token);
        Ok(())
    }

    /// IO-aware single drive: poll the reactor → convert readiness events into inputs → merge
    /// the external inputs → call the existing tick loop → return terminal outputs.
    ///
    /// - `timeout`: the wait limit passed to the reactor poll. `None` = block until an event;
    ///   `Some(0)` = non-blocking (return what is currently ready immediately).
    /// - Readiness events with unregistered tokens are dropped (the reactor may report sources
    ///   that have been deregistered).
    pub fn run_io<R: IoReactor>(
        &mut self,
        reactor: &mut R,
        external_inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
        timeout: Option<core::time::Duration>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        let io_events = reactor
            .poll(timeout)
            .map_err(|e| RuntimeError::IoFailed { error: e })?;

        let mut inputs = external_inputs;
        for event in io_events {
            if let Some((machine, port)) = self.io_routing.get(&event.token) {
                inputs.push((machine.clone(), port.clone(), Box::new(event)));
            }
        }
        self.tick(inputs)
    }
}
