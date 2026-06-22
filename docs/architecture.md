# axiom 架构参考

> 本文档描述了 axiom 的架构组件。如果你在找快速入门，回到 README。

---

## Two computational primitives

| Primitive | Memory | State | Observable | Controllable | Connection |
|-----------|--------|-------|------------|--------------|------------|
| `Func`    | Stack  | None  | No         | No           | Inline call |
| `Machine` | Heap   | `S`   | Yes        | Yes          | Ports (BoundedBuf / Channel / Latest / CasFreeRing / SharedState) |

`Func(I) -> O` — a pure function. Stack frame, instant, unobservable. The same input always produces the same output. Used for parsing, serialization, mathematical transforms.

`Machine(S, I, O, δ)` — a state machine with typed port interface. The Machine trait now reflects the mathematical interface-set model:

```rust
pub trait Machine: Send + Sync + 'static {
    type State: Send + 'static;
    type Input: HasPortInfo;     // port enum, one variant per input port
    type Output: HasPortInfo;    // port enum, one variant per output port
    type Ports: PortSet;         // connects Input/Output enums to PortSchema

    fn port_schema() -> PortSchema  // auto-derived from Self::Ports::port_schema()
    where Self: Sized;
    // ... init, process, cleanup ...
}
```

The IO-Object is exactly `(S, I, O, δ)` — no more, no less. Observe and Control are port annotations (`FlowKind`), not type parameters. The `type Input`/`type Output` are now **interface sets** (port enums), closing the gap between type-space and value-space port declarations.

---

## Port interface sets (PortSet)

A Machine's input and output are **sets of ports** $\Gamma = \{p_1, p_2, \ldots\}$, not single values. This is enforced by the `PortSet` trait:

```rust
pub trait PortSet: Send + Sync + 'static {
    type Input: HasPortInfo;    // enum: one variant per input port
    type Output: HasPortInfo;   // enum: one variant per output port
    fn port_schema() -> PortSchema;
}
```

### declare_ports! macro

The `declare_ports!` macro generates all three types (Input enum, Output enum, PortSet impl) from a single declaration:

```rust
declare_ports! {
    pub struct TrainerPorts {
        input type TrainerInput {
            batch[Data]    => Batch,
            ctrl[Control]  => ControlSignal,
        }
        output type TrainerOutput {
            loss[Data]        => Loss,
            model_delta[Data] => ModelDelta,
            stats[Observe]    => ModuleStats,
        }
    }
}

impl Machine for Trainer {
    type State = TrainerState;
    type Input = TrainerInput;    // compiler-checked enum
    type Output = TrainerOutput;
    type Ports = TrainerPorts;    // port_schema() auto-derived
    // ...
}
```

### Single-port convenience

For machines with exactly one input and one output port (common for simple cases):

```rust
impl Machine for Doubler {
    type State = ();
    type Input = In<i32>;       // single-input wrapper
    type Output = Out<i32>;     // single-output wrapper
    type Ports = SinglePorts<i32>;
    // port_schema() auto-derived: one "input" port + one "output" port
}
```

### Zero-variant edge cases

- `NoOutput` — zero-variant enum for machines with no output ports (e.g. `Sink`)
- `NoInput` — zero-variant enum for machines with no input ports (e.g. `Source`)
- Both are uninhabited: `ProcessOutput::Yield(NoOutput)` can never be constructed

---

## Three-layer structure

```
Layer 0: Entity  = (S, name)                         persistent existence
Layer 1: Ports   = PortSchema + PortDecl(dir × flow × type)   communication
Layer 2: Machine = Entity + ports + process(I) → O   computation
```

`Entity` is the lightest declaration — just a named state container. `Machine` extends it with typed I/O and a process function. In Rust, they are separate traits (Machine does not require Entity as a supertrait).

---

## Ports

A port is `PortDecl { name, dir: PortDir, flow: FlowKind, type_id }`. The three dimensions are orthogonal:

| Dimension | Values |
|-----------|--------|
| Direction | `In`, `Out` |
| FlowKind  | `Data`, `Control`, `Observe` |
| Type      | Any Rust type (via TypeId) |

Convenience constructors:

```rust
PortDecl::input::<T>("name")      // In + Data
PortDecl::output::<T>("name")     // Out + Data
PortDecl::ctrl_in::<T>("name")    // In + Control
PortDecl::ctrl_out::<T>("name")   // Out + Control
PortDecl::observe::<T>("name")    // Out + Observe
```

Port compatibility is checked at link time: same TypeId + same FlowKind + version drift ≤ 1.

---

## Links

A `LinkSpec` describes a physical connection between two ports. The `LinkKind` determines the strategy:

| Kind | Physics | When |
|------|---------|------|
| `Inline` | Function call, zero allocation | Same-thread, Func→Func or Machine→Func |
| `BoundedBuf` | Lock-based ring buffer, configurable backpressure | Cross-thread, producer-consumer |
| `Channel` | MPSC channel | Multiple producers, single consumer |
| `Latest` | Single overwrite slot | Status feed, UI refresh |
| `CasFreeRing` | Lock-free SPSC, fixed address | Interrupt → main-loop, embedded |
| `SharedState` | Arc\<RwLock\<T\>\> | Config distribution, shared metrics |

---

## Deployment

A `DeploySpec` is a pure data structure: it describes what Machines exist, how they connect, and with what physical resources. It does not execute anything. A runtime adapter interprets the spec.

The same Machine type can be deployed differently:
- **Backtest**: `CpuBound`, deterministic, `Inline` links, zero allocation
- **Production**: `Async` or `ThreadPool`, nondeterministic, `BoundedBuf` links, backpressure
- **Embedded**: `CpuBound`, static allocation, `CasFreeRing` links, no heap

The Machine implementation does not change. Only the DeploySpec changes.

---

## Runtime comparison

`Machine::process()` is synchronous by design:

```
Tokio:      tokio::task::spawn_blocking(|| machine.process(state, ctx, input))
Rayon:      rayon::scope(|s| s.spawn(|_| machine.process(state, ctx, input)))
Dedicated:  loop { machine.process(state, ctx, input) }
Inline:     machine.process(state, ctx, input) — zero runtime overhead
```

The same `Machine` implementation, zero modifications.

| Runtime | Good at | Not good at | axiom deployment hint |
|---------|---------|-------------|----------------------|
| **None (inline)** | CPU-bound loops, zero overhead | IO, networking, concurrency | `Inline` links |
| **Tokio** | Async IO, networking, HTTP, WS | CPU-bound work on async workers | `Async` for IO, `CpuBound` for compute |
| **Rayon** | Data parallelism, batch processing | Async IO, low-latency interactive | `CpuBoundN(n)` over instances |
| **Embassy** | no_std embedded | Heap-heavy workloads | `Async` with no_std |
| **Dedicated thread** | Hard real-time, CPU affinity | Complex IO multiplexing | `CpuBound` + core_affinity |

### The &mut State constraint

A single Machine cannot process multiple inputs in parallel — `process()` takes `&mut State`. Parallelism happens at the instance level:

```rust
// Multiple machine instances, each on its own thread
let results: Vec<_> = configs.par_iter()
    .map(|cfg| {
        let mut m = MyMachine::init(&ctx).unwrap();
        for input in &inputs { m.process(&mut m, &ctx, input); }
        m
    })
    .collect();
```

`Func` has no such constraint — `Func::call(input)` is stateless and thread-safe.

### Hard real-time

Hard real-time is a deployment question, not a runtime question:

1. Deploy as `CpuBound` on a dedicated OS thread
2. Pin to a specific core via `core_affinity`
3. Pre-allocate all working memory in `init()`
4. Use `CasFreeRing` links (lock-free)

These constraints live in `MachinePhysicalSpec`. No Machine code changes.

### Backpressure

Backpressure is a topology problem, not an implementation problem:

```rust
// Option 1: absorb with capacity
LinkKind::BoundedBuf { capacity: 4096, write_policy: Blocking, .. }

// Option 2: decouple with dropping
LinkKind::Channel { capacity: 64, drop_when_full: true }

// Option 3: fan-out to multiple workers
```

No Machine code changes. No runtime changes.

---

## ProcessOutput variants

The result of a single `process()` call:

```rust
pub enum ProcessOutput<O> {
    Yield(O),             // single output on one port
    YieldMulti(Vec<O>),   // multiple outputs, each on its own port (fan-out)
    Idle,                 // no output this tick
    Done,                 // machine finished, triggers cascade shutdown
}
```

`YieldMulti` supports multi-port fan-out in a single tick. For example, a Trainer that produces loss + model_delta + stats simultaneously:

```rust
ProcessOutput::YieldMulti(vec![
    TrainerOutput::loss(loss),
    TrainerOutput::model_delta(model_delta),
    TrainerOutput::stats(stats),
])
```

The runtime delivers each output variant to its target port based on the deployment topology.

---

## MachineContext

The context provided to every Machine lifecycle method:

| Feature | Method | Purpose |
|---------|--------|---------|
| Observation | `observe_is_connected()` | Skip expensive observe formatting when no consumer |
| Output tracking | `output_is_connected()` | Skip computation when downstream disconnected |
| Snapshots | `snapshot()` | Capture serialized state (optional) |
| Lifecycle | `lifecycle()` / `set_lifecycle()` | Init → Running → Stopping → Stopped |
| Signals | `poll_signal()` | Runtime sends Shutdown / Checkpoint |
| Time | `time_ms()` | Wall-clock or simulation time |
| Initial value | `initial_value::<T>()` | Type-safe config injection at deploy time |
| Initial value (set) | `set_initial_value::<T>(value)` | Called by deployer before spawn |

### Initial value injection

Config objects are injected at deploy time, not as trait generics:

```rust
// Deployer side:
let mut ctx = MachineContext::new("data_loader");
ctx.set_initial_value(config.clone());

// Machine side:
fn init(ctx: &MachineContext) -> Result<State, InitError> {
    let config = ctx.initial_value::<Config>().expect("needs Config");
    // ...
}
```

### System signals

| Signal | Effect |
|--------|--------|
| `Shutdown` | Request graceful shutdown after current process() completes |
| `Checkpoint` | Request a state snapshot (machine may serialize State via checkpoint()) |

## Built-in modules

Every built-in uses the same port-enum architecture as user-defined Machines.

| Module | Signature | Ports | Role |
|--------|-----------|-------|------|
| `Identity<I>` | `I → I` | `input[Data] → output[Data]` | Category identity morphism, fills gaps |
| `Sink<I>` | `I → ∅` | `input[Data]` (no output) | Discards input, terminates pipelines |
| `Source<O>` | `∅ → O` | `tick[Data] → output[Data]` | Constant output per tick, useful for testing |
| `Tee<I>` | `I → (I, I)` | `input[Data] → output_a[Data] + output_b[Data]` | Fan-out broadcast via YieldMulti |
| `Latch<T>` | `T → T` | `input[Data] → output[Data]` | Holds last received value |
| `Collector<I>` | `I → ∅` | `input[Data]` (observe: `snapshots`) | Accumulates in State, exposes via observe port |
| `EntityRoot` | `∅` | (none — pure Entity) | System root — exists, does nothing |
| `FuncMachine<F>` | `F::Input → F::Output` | `input[Data] → output[Data]` | Wraps any `Func` as a Machine |

```rust
use axiom::builtin::Identity;
```

---

## Graph-theoretic topology analysis

A deployment topology `DeploySpec` is a **labeled directed multigraph**. Graph theory provides the vocabulary and algorithms to analyze it statically — before any runtime runs.

### 1. The deployment graph model

**Definition (deployment graph).**
A deployment is a labeled directed multigraph

$$\Sigma = (V, E, \ell)$$

where:

| Symbol | Code | Meaning |
|--------|------|---------|
| $V$ | `DeploySpec::machines` $\cup$ `DeploySpec::funcs` | Vertices: computation units |
| $E$ | `DeploySpec::links` | Directed edges: connections |
| $\ell: E \to \text{LinkKind}$ | `LinkSpec::kind` | Edge label: physical strategy |
| $\text{in}_M$ | `Machin::port_schema().inputs()` | Incoming edges to $M$ |
| $\text{out}_M$ | `Machine::port_schema().outputs()` | Outgoing edges from $M$ |

Each edge $e \in E$ carries metadata beyond the label:

```
e = (src_machine, src_port, dst_machine, dst_port, link_kind)
    ├─ source vertex ──┬─ source port ─┼─ target vertex ─┬─ target port ─┴─ physics ─┘
    └──────────────────┘               └─────────────────┘
```

**Definition (edge compatibility).**
An edge $e$ connecting $\text{out}_A$ to $\text{in}_B$ is **compatible** iff:
- $\text{type(out}_A) = \text{type(in}_B)$ — TypeId match
- $\text{flow(out}_A) = \text{flow(in}_B)$ — FlowKind match
- $|\text{ver(out}_A) - \text{ver(in}_B)| \le 1$ — Schema version drift bound

This is enforced at link time by `LinkCompat::check()`.

### 2. LinkKind as edge classification

Each `LinkKind` constrains where in the graph the edge can appear:

| Edge kind | Degree constraint | Cycle constraint | Thread boundary |
|-----------|-------------------|------------------|-----------------|
| `Inline` | $\text{outdeg(src)} \le 1$ | **Must not** participate in any cycle | Must be intrasame-thread |
| `BoundedBuf` | None | Permitted (feedback loops) | Cross-thread or same-thread |
| `Channel` | $\text{indeg(dst)} = 1$ (single consumer) | Permitted | Cross-thread |
| `Latest` | None | Permitted | Cross-thread |
| `CasFreeRing` | $\text{outdeg(src)} \le 1$, $\text{indeg(dst)} \le 1$ (SPSC) | Permitted | Cross-thread or ISR→main |
| `SharedState` | None | Permitted (no active data flow) | Cross-thread |

**Theorem (Inline cycle → deadlock).**
If subgraph $\Sigma' \subseteq \Sigma$ consists only of Inline edges and contains a directed cycle, then executing $\Sigma'$ deadlocks: each vertex waits for its predecessor, which waits for its predecessor, which waits for its predecessor...

*Proof.* Inline edges are synchronous function calls: the caller blocks until the callee returns. A cycle of synchronous calls is a textbook deadlock. $\square$

**Corollary (Inline embedding constraint).**
The subgraph induced by Inline edges must be a **DAG** (directed acyclic graph). Equivalently, the transitive closure of Inline edges must be a partial order.

### 3. Static analysis algorithms

The following graph algorithms can be run on $\Sigma$ before deployment:

#### 3a. Topological sort (Inline-DAG)

```rust
fn inline_topological_order(spec: &DeploySpec) -> Result<Vec<Vertex>, CycleError> {
    // Build subgraph of Inline edges only.
    // Run Kahn's algorithm or DFS-based topological sort.
    // If a cycle is detected, return the cycle vertices for error reporting.
}
```

**Purpose:** Determine execution order for machines connected via Inline links on the same thread.

**Implementation status:** Not yet implemented (`DeploySpec::validate()` patch 7.5.5).

#### 3b. Strongly connected components

```rust
fn feedback_loops(spec: &DeploySpec) -> Vec<Vec<Vertex>> {
    // Run Kosaraju or Tarjan on the full graph.
    // Return all SCCs with size > 1 — these are feedback loops.
    // For each SCC, verify that no edge within it is Inline.
}
```

**Purpose:** Identify feedback topologies. Every feedback loop must contain at least one BoundedBuf or Channel edge. A loop consisting entirely of Inline edges is a deadlock.

**Engineering rule:** A cycle of Mealy machines connected by BoundedBuf edges is a legal feedback loop (state update lags by one tick). A cycle of Inline edges is illegal.

#### 3c. Reachability

```rust
fn reachable_from(spec: &DeploySpec, source: &str) -> HashSet<&str> {
    // BFS/DFS from source along outgoing edges.
}

fn can_reach(spec: &DeploySpec, source: &str, target: &str) -> bool {
    // BFS/DFS from source, stop when target found.
}
```

**Purpose:**
- **Observation completeness** (Theorem 7.2): All FlowKind::Observe ports are reachable from a collector vertex, or equivalently, all observe-labeled edges lead to a sink that stores/forwards the data.
- **Control reachability**: A controller machine's control outputs reach all intended target machines.
- **Orphan detection**: Vertices with no inbound edges (except Source) or no outbound edges (except Sink) — may indicate configuration errors.

#### 3d. Dominator analysis

```rust
fn single_point_of_failure(spec: &DeploySpec) -> Vec<Vertex> {
    // Compute dominators from root(s).
    // Any vertex that dominates all paths to a critical region is a SPOF.
}
```

**Purpose:** Identify vertices whose failure disconnects the graph. A controller that all data flows through is a single point of failure — its redundancy should be considered at the deployment level.

### 4. Feedback topology and algebraic loops

**Definition (feedback edge).**
An edge $e \in E$ is a **feedback edge** iff it creates a cycle in $\Sigma$ — i.e., $e$ belongs to some strongly connected component with size $> 1$.

**Definition (algebraic loop).**
A cycle $C = (v_1 \to v_2 \to \ldots \to v_k \to v_1)$ in $\Sigma$ is an **algebraic loop** iff every edge in $C$ is `Inline`. This is equivalent to a combinational logic loop in digital circuits — the output of the cycle is undefined because it depends on itself in the same tick.

**Definition (sequential feedback).**
A cycle $C$ is **sequential feedback** iff at least one edge in $C$ is `BoundedBuf` or `Channel`. This is equivalent to a sequential logic loop — the loop has state (the buffer) and computation is well-defined across ticks.

**Theorem (Mealy/Moore separation for graph analysis).**
In a cycle $C$, if every Machine on $C$ is Moore-type ($\lambda: S \to O$, no direct $I \to O$ path), then the cycle is **always well-defined** regardless of edge kind: each machine's output depends only on pre-tick state, not on the current tick's input.

*Practical consequence.* Moore-type machines are feedback-safe. Mealy-type machines in Inline cycles cause algebraic loops.

**Engineering rule.** If you detect a cycle in `DeploySpec::validate()`:
1. If every edge is Inline → **reject** (algebraic loop / deadlock)
2. If at least one edge is BoundedBuf or Channel → **warn** but accept (sequential feedback — check Moore property)
3. If all machines on the cycle are Moore-type → accept silently

### 5. Deployment transformation as graph homomorphism

**Definition (deployment mapping).**
A deployment mapping $\Delta: \Sigma_{\text{abstract}} \to \Sigma_{\text{physical}}$ is a **graph homomorphism** that:
- Maps abstract vertices to physical execution contexts (threads, processes, cores)
- Transforms edge labels from abstract `LinkKind` to concrete physical channels
- Preserves the connectivity structure: if $e: u \to v$ in $\Sigma_{\text{abstract}}$, then $\Delta(e): \Delta(u) \to \Delta(v)$ in $\Sigma_{\text{physical}}$

**Example (same topology, three deployments):**

```
                  Abstract topology
               ┌──────────────────┐
               │   reader ──→ parser ──→ writer  │
               └──────────────────┘

Backtest:     CpuBound + Inline (all on one thread, zero allocation)
              Δ: {reader, parser, writer} → {thread_0}
              Δ: Inline(reader→parser) → fn_call
              Δ: Inline(parser→writer)  → fn_call

Production:   Async + BoundedBuf (cross-thread, backpressure)
              Δ: reader → {io_thread}
              Δ: parser → {cpu_thread}
              Δ: writer → {io_thread}
              Δ: Inline(reader→parser) → BoundedBuf(capacity:1024, blocking)
              Δ: Inline(parser→writer)  → BoundedBuf(capacity:1024, blocking)

Embedded:     CpuBound + CasFreeRing (lock-free, static address)
              Δ: {reader, parser, writer} → {core_0}
              Δ: Inline(reader→parser) → CasFreeRing(capacity:64, static:0x2000_4000)
              Δ: Inline(parser→writer)  → CasFreeRing(capacity:64, static:0x2000_4100)
```

**Graph invariant under $\Delta$.**
The abstract graph's **reachability** and **acyclicity** properties are preserved under any valid deployment mapping. A cycle in $\Sigma_{\text{abstract}}$ remains a cycle under $\Delta$; a DAG remains a DAG. This is the graph-theoretic restatement of Theorem 6.1 (deployment invariance):

$$\text{Theorem 6.1} \iff \forall e \in E_{\text{abstract}}: \text{reach}_{\text{abstract}}(e) = \text{reach}_{\text{physical}}(\Delta(e))$$

### 6. Fault tolerance and observability

#### 6a. Minimum cut

**Definition (deployment cut).**
A cut $C \subseteq E$ is a set of edges whose removal disconnects a source set $S \subseteq V$ from a target set $T \subseteq V$.

**Engineering question:** What is the minimum set of link failures that can isolate a critical machine from its controllers or observers?

For the `complex_topology` example (Sensor1/2/3 → Controller1/2 → SafetyMonitor → Store), the minimum cut isolating Store is 1 (the observe channel from SafetyMonitor to Store). This is a single point of failure — a `SharedState` link instead of `Channel` would make Store's data available even if the link drops.

#### 6b. Observability completeness (graph restatement)

**Theorem 7.2 (graph form).**
A machine $M$'s FlowKind::Observe outputs are consumed by an observer $\iff$ there exists a directed path from $M$'s observe port to an observer machine along edges labeled with `FlowKind::Observe`.

```
Algorithm: for each machine M in spec:
    for each observe_port in M.port_schema().observe_ports():
        if no path exists from observe_port to any sink/collector:
            warn("observe port {}.{} is disconnected", M.name, observe_port)
```

#### 6c. Single point of failure (SPOF)

**Definition (SPOF).**
A vertex $v \in V$ is a **single point of failure** for reachability $R \subseteq V$ iff every path from a source $s$ to any $r \in R$ passes through $v$.

**Detection:** Compute dominators from each source vertex in the deployment graph. Any vertex that dominates all paths to a critical region is a SPOF.

**Mitigation (at deployment level):** If SPOF is detected:
- Duplicate the machine instance (`CpuBoundN(2)`)
- Route through a `Channel` with two senders
- Or accept the SPOF and document it

---

### Summary of graph invariants and their code locations

| Invariant | Algorithm | Enforced at | Current status |
|-----------|-----------|-------------|----------------|
| Type compatibility | TypeId + FlowKind match | `LinkCompat::check()` | Implemented |
| Inline acyclicity | Topological sort | `DeploySpec::validate()` | Not yet implemented |
| Feedback loop detection | SCC (Tarjan) | `DeploySpec::validate()` | Not yet implemented |
| SPOF detection | Dominator analysis | Advisory | Not yet implemented |
| Observability completeness | Reachability (BFS) | Advisory | Not yet implemented |
| Edge degree constraints | Counter per port | `DeploySpec::validate()` | Not yet implemented |
| Schema version drift | Version diff check | `LinkCompat::check()` | Implemented |
