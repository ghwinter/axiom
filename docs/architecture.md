# axiom Architecture Reference

> This document describes the architectural components of axiom. For a quick start, see the README.

---

## Two computational primitives

| Primitive | Memory | State | Observable | Controllable | Connection |
|-----------|--------|-------|------------|--------------|------------|
| `Func`    | Stack  | None  | No         | No           | Inline call |
| `Machine` | Heap   | `S`   | Yes        | Yes          | Ports (BoundedBuf / Channel / Latest / CasFreeRing / SharedState) |

`Func(I) -> O` — a pure function. Stack frame, instant, unobservable. The same input always produces the same output. Used for parsing, serialization, mathematical transforms.

`Machine(S, I, O, δ)` — a state machine with typed port interface. The `Machine` trait reflects the mathematical interface-set model:

```rust
pub trait Machine: Send + Sync + 'static {
    type State: Send + 'static;
    type Input: HasPortInfo;     // port enum, one variant per input port
    type Output: HasPortInfo;    // port enum, one variant per output port
    type Ports: PortSet;         // connects Input/Output enums to PortSchema
    type ProcessOutput: MachineOutput<Self::Output>; // Single | Multi | Tuple

    fn port_schema() -> PortSchema  // auto-derived from Self::Ports::port_schema()
    where Self: Sized;
    // ... init, process, cleanup ...
}
```

The IO-Object is exactly `(S, I, O, δ)` — no more, no less. Observe and Control are port annotations (`FlowKind`), not type parameters. The `type Input`/`type Output` associated types are **interface sets** (port enums), closing the gap between type-space and value-space port declarations.

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
    // Derives are forwarded to the generated enums; add them as needed.
    // axiom does NOT force `Clone` — a non-Clone payload can be a port type.
    #[derive(Debug, Clone, PartialEq)]
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

A `DynamicTopology` is a pure data structure: it describes what Machines exist, how they connect, and with what physical resources. It does not execute anything. A runtime adapter interprets the spec.

The same Machine type can be deployed differently:
- **Backtest**: `CpuBound`, deterministic, `Inline` links, zero allocation
- **Production**: `Async` or `ThreadPool`, nondeterministic, `BoundedBuf` links, backpressure
- **Embedded**: `CpuBound`, static allocation, `CasFreeRing` links, no heap

The Machine implementation does not change. Only the DynamicTopology changes.

---

## Runtime comparison

`Machine::process()` is synchronous by design:

```
Async pool:  spawn_blocking(|| machine.process(state, ctx, input))
Worker pool: scope(|s| s.spawn(|_| machine.process(state, ctx, input)))
Dedicated:   loop { machine.process(state, ctx, input) }
Inline:      machine.process(state, ctx, input) — zero runtime overhead
```

The same `Machine` implementation, zero modifications.

| Runtime | Good at | Not good at | axiom deployment hint |
|---------|---------|-------------|----------------------|
| **None (inline)** | CPU-bound loops, zero overhead | IO, networking, concurrency | `Inline` links |
| **Async IO runtime** | Async IO, networking, HTTP, WS | CPU-bound work on async workers | `Async` for IO, `CpuBound` for compute |
| **Worker pool** | Data parallelism, batch processing | Async IO, low-latency interactive | `CpuBoundN(n)` over instances |
| **Embedded async runtime** | no_std embedded | Heap-heavy workloads | `Async` with no_std |
| **Dedicated thread** | Hard real-time, CPU affinity | Complex IO multiplexing | `CpuBound` + `CpuAffinity` |

### axiom-runtime: the bundled reference runtime

`axiom-runtime` (in the `runtime/` subdirectory) is the reference implementation
bundled with this repo. Unlike the hypothetical external adapters above, it is
a concrete `Runtime` that materializes a `DynamicTopology` and drives `tick()`:

```rust
use axiom_runtime::{Runtime, RuntimeConfig};

let mut rt = Runtime::new(RuntimeConfig::sequential());
rt.register::<MyMachine>("my_machine");
rt.materialize(&spec)?;                 // materialize the topology
let outputs = rt.tick(inputs)?;         // drive one tick
```

**Execution modes** (selected via `RuntimeConfig::mode`, no code changes):

| Mode | Physics | Use case |
|------|---------|----------|
| `Inline` | Caller-thread, no tick limit | Zero-overhead inline execution |
| `Sequential` | Single-thread BFS, direct move delivery | Deterministic, testable |
| `Parallel(n)` | One OS thread per machine + channel carriers | Multi-core, backpressure |

**Internal subsystem contracts.** `axiom-runtime` is itself a parent
system whose subsystems are **necessary-but-replaceable** modules — structural
consistency applied to the runtime itself (each subsystem has an explicit
interface contract, mirroring the machine/link model of the user space):

| Subsystem | Contract | Replaceable strategies |
|-----------|----------|------------------------|
| **Scheduler** (drives `tick`) | `scheduler::Scheduler` — `tick(&self, rt, inputs)`, selected at construction by `RuntimeConfig::mode`, held as `Box<dyn Scheduler>` | `SequentialScheduler` (BFS + fairness quota), `ParallelScheduler` (thread-per-machine); custom schedulers implement the trait |
| **Carrier factory** | `carrier::channel_for` — `LinkKind → carrier` | per-kind physicalization of all six kinds (channel / sync / slot / overwrite / lock-free ring; `Inline` migrates to an unbounded channel across threads) |
| **Lifecycle** | `materialize`/`shutdown` + `MachineHandle` init/cleanup | runtime-managed; restart/supervision is outside the bundled lifecycle |
| **IO reactor** | `io::IoReactor` trait | platform backends (epoll / kqueue / WSAEventSelect) |
| **Replay** | `replay::` module | snapshot/replay of machine states |

The **Scheduler** contract is the first internal subsystem formalized:
`SequentialScheduler`/`ParallelScheduler` implement `scheduler::Scheduler`, and
`Runtime::tick` delegates through it. This mirrors the external
`RuntimeContract` ([adapters.md](adapters.md)): the runtime as a whole is replaceable
per contract; its internal subsystems are replaceable per their own contracts.

**Capabilities:**

| Capability | What it does |
|------------|--------------|
| **Fused pipeline** | `materialize` auto-detects adjacent `FusedInline` + `Inline` chains and replaces them with a single `FusedPipeline`, eliminating per-hop routing lookups (−2 alloc/hop) |
| **Parallel cyclic topology** | Kahn's algorithm detects cycles; cyclic topologies use a global `stop_signal` + per-thread tick counters instead of channel-disconnect cascade (avoids deadlock) |
| **IO multiplexing** | `IoReactor` trait with platform backends (epoll/kqueue/WSAEventSelect); `register_io` + `run_io` merge reactor readiness events with external inputs |
| **Composite Machine** | `register_composite` encapsulates a sub-topology + port map as one `machine_type`; `materialize` recursively expands (namespaced sub-machines + redirected links) before fusion — `FusedPipeline` can fuse across former composite boundaries |
| **B-tier carriers** | `Overwriting` bounded cover, `Latest`/`SharedState` single-slot, `ReadPolicy::NonBlocking` polling |
| **Shutdown cascade** | `Done` = stop signal: machine stops, backlog dropped, cascades to downstream whose all in-edge sources have stopped |
| **Observation/debugging** | `Observe`-flow monitor (independent thread, `Dropping` carrier — observation never stalls the main path) + `Control`-flow reverse injection (e.g. `DEBUG FLUSH/SET/INFO`); see [philosophy.md](philosophy.md), section "Observation and debugging are first-class modules" |
| **Streaming pull model** | `StreamingMachine::process_stream` — lazy iterator output for pull-driven data flow |
| **Zero-copy input** | `FuncRef::call_ref(&Input)` — borrowed input, no per-call allocation |

**Scope boundaries**: arbitrary compile-time generic DAGs beyond the
`Chain`/`Diamond` series-parallel algebra (the dynamic path still type-erases
via `Box<dyn Any>`), and the Windows IOCP completion model (the bundled
WSAEventSelect readiness model supports ≤64 sources).

### The &mut State constraint

A single Machine cannot process multiple inputs in parallel — `process()` takes
`&mut State`. Parallelism happens at the instance level: multiple independent
instances run on separate worker threads, each owning its own state.

```rust
// Multiple machine instances, each on its own worker thread
let results: Vec<_> = configs.into_iter()
    .map(|cfg| {
        let mut m = MyMachine::init(&ctx).unwrap();
        for input in &inputs { m.process(&mut m, &ctx, input); }
        m
    })
    .collect();
```

`Func` has no such constraint — `Func::call(input)` is stateless and thread-safe.

### Static execution path (general, zero-cost)

> **Positioning.** `axiom_runtime::static_path` provides the **general static
> execution path**: the combinators `pipeline_chain` (arbitrary-depth linear
> chain over `Chain`), `diamond` (fork-join over `Diamond`), and `feedback`
> (single-machine feedback loop). The topology is encoded in type parameters
> and the compiler fuses stages into a single loop — fully monomorphized, no
> `Box<dyn Any>`, no trait dispatch, no heap allocation per message. This
> closes the anti-narrowing gap at the execution layer: the static path is no
> longer limited to linear chains (see [philosophy.md](philosophy.md), section "The
structural scope constraint").
>
> **Structural positioning.** Static is the **main execution layer** for
> *structure-fixed* systems: topology encoded as types, behavioral complexity
> (what happens inside `process`) is a black box and never requires the dynamic
> path. `Chain` and `Diamond` form a recursive algebra that generates exactly
> the **series-parallel DAGs** — pipelines, map-reduce, diamond networks,
> multi-level split-merge trees — **plus composite hierarchies** (any subgraph
> encapsulated as one node, recursively; top level a series-parallel tree).
> The dynamic path serves only topologies *sourced from runtime data*
> (config/plugin assembly, dynamic links, cycles' time drive). Formal
> definitions are given in [structural-model.md](structural-model.md).

The static path lives in `axiom-runtime::static_path` and builds on the
`Straight` contract in `axiom::static_exec` (`StraightMachine`,
`StraightLink`, `StraightSplit`, `StraightMerge`). It uses a **synchronous
batch topological-order** model: inputs are processed machine by machine in
topology order, outputs collected into `Vec`s, and fork/join are handled by
the `StraightSplit`/`StraightMerge` contracts at the type level.

```rust
use axiom_runtime::static_path::{pipeline_chain, diamond, feedback};
use axiom::static_exec::{Chain, Diamond, StraightClone, StraightId};

// Linear: Doubler → Adder → Tripler (Chain nests to arbitrary depth)
type Chain3 = Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>;
let outputs = pipeline_chain::<Chain3>(inputs)?;

// Diamond: Doubler → StraightClone → (Adder, Tripler) → Sum → Adder
type Shape = Diamond<Doubler, Adder, Tripler, Adder, StraightClone, StraightId, StraightId, Sum>;
let outputs = diamond::<Doubler, Adder, Tripler, Adder, StraightClone, StraightId, StraightId, Sum>(inputs)?;

// Feedback: A's output is fed back into A's input with a one-tick delay
let outputs = feedback::<Doubler, Sum>(inputs, 0)?;
```

**0-cost contract:**

- With `#[inline]` on `process_straight` implementations and `--release`, the compiler fuses all stages into one loop, inlining across stage boundaries.
- Allocations: one `Vec` per stage output (the batch model collects outputs; no per-message allocation).
- The "data flow" metaphor dissolves into pure computation — no runtime `Port` or `Link` objects, no trait dispatch, no function-call barriers.

**API:**

| Function | Topology | Input | Output | Use case |
|----------|----------|-------|--------|----------|
| `pipeline_chain<C: StaticChain>` | arbitrary-depth `Chain` | `Vec<C::In>` | `Vec<C::Out>` | linear pipelines |
| `diamond<A, Left, Right, Down, S, LB, LC, M>` | A → split → (Left, Right) → merge → Down | `Vec<A::StraightIn>` | `Vec<Down::Out>` | fan-out / fan-in |
| `feedback<A, M>` | single-machine feedback loop | `Vec<A::StraightIn>` + `initial` | `Vec<A::StraightOut>` | deterministic cycles |

**Type contracts** (in `axiom::static_exec`):

- `StraightMachine`: single-port machine passing raw payloads — `process_straight(state, input) -> output`, no port enum, no label check.
- `StraightLink<S, D>`: converts `S::StraightOut` → `D::StraightIn` (stage connector); `StraightId` applies when `Out: Into<In>`.
- `StraightSplit<T>`: splits one payload into `(Left, Right)` for fan-out. `StraightClone` provides Tee semantics.
- `StraightMerge<A, B>`: combines two upstream payloads into one downstream payload.
- `Chain<Head, Tail, L>` / `Diamond<A, Left, Right, Down, S, LB, LC, M>`: recursive combinators encoding arbitrary series-parallel topologies; `Composite<Inner>` wraps a sub-topology as a single node.

**Limitations:**

- All stages must implement `StraightMachine` (single-input, single-output, raw payload). Multi-output machines (`YieldMulti`, runtime-determined output counts) are rejected at the type level — the static path expresses compile-time-known topologies only. This is a compile-time guarantee, not a runtime check.
- All stages must be known at compile time (static topology).
- Acyclic except the explicit `feedback` loop — the synchronous batch model cannot express time-driven cycles. Use the dynamic path (`Runtime`) for those.
- Truly arbitrary DAGs (non-series-parallel cross edges) are outside the combinator algebra — stable Rust cannot express an arbitrary edge table with type-safe ports — and take the dynamic path.
- `process_straight` implementations **must** be marked `#[inline]` for cross-crate inlining; without it, the compiler cannot fuse the stages.

---

### Hard real-time

Hard real-time is a deployment question, not a runtime question:

1. Deploy as `CpuBound` on a dedicated OS thread
2. Pin to specific cores via `MachinePhysicalSpec::cpu_affinity` —
   `CpuAffinity::Allowed(cores)` (shared) or `CpuAffinity::Exclusive(cores)`
   (no other thread scheduled there, the "exclusive core" scenario)
3. Optionally place the machine's memory on a NUMA node
   (`MachinePhysicalSpec::numa_node`) and require huge pages
   (`MachinePhysicalSpec::huge_pages`: `Size2MiB` / `Size1GiB`)
4. Require SIMD instruction sets for the hot path
   (`MachinePhysicalSpec::simd` → `SimdRequirement`)
5. Pre-allocate all working memory in `init()`
6. Use `CasFreeRing` links (lock-free)

These constraints live in `MachinePhysicalSpec`. No Machine code changes.
Whether the runtime can honor them is a **declared capability**, not an
assumption: the adapter reports its physical budget (`PhysicalBudget`: core
pinning / exclusive cores / NUMA / huge pages / SIMD features) in its
`RuntimeContract::guarantees`, and `check_spec` rejects a blueprint that
demands more than the runtime can physically deliver — *before* deployment
(`runtime-resource-affinity` / `-affinity-exclusive` / `-numa` /
`-hugepages` / `-simd` rule violations).

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

The result of a single `process()` call is expressed through the **associated type**
`type ProcessOutput: MachineOutput<Self::Output>`. Machines return one of three
concrete output types; `ProcessOutput<O>` remains only as the runtime's unified view
(machines do not return it directly):

```rust
// SingleOutput<O> — exactly one output (1:1)
// MultiOutput<O>  — zero or more outputs, count known at runtime (fan-out)
// TupleOutput<O>  — exactly two outputs (1:1:1, no fan-out; FusedInline-safe)

pub enum ProcessOutput<O> {          // runtime unified view
    Yield(O),                        // single output on one port
    YieldMulti(Vec<O>),              // multiple outputs, each on its own port
    Idle,                            // no output this tick
    Done,                            // machine finished, triggers cascade shutdown
}
```

`MultiOutput::YieldMulti` supports multi-port fan-out in a single tick. For example, a Trainer that produces loss + model_delta + stats simultaneously:

```rust
MultiOutput::YieldMulti(vec![
    TrainerOutput::loss(loss),
    TrainerOutput::model_delta(model_delta),
    TrainerOutput::stats(stats),
])
```

Fixed-count multi-port output (e.g. data + observation) uses `TupleOutput::Yield(O, O)`.
The runtime delivers each output variant to its target port based on the deployment topology.

---

## MachineContext

The context provided to every Machine lifecycle method:

| Feature | Method | Purpose |
|---------|--------|---------|
| Snapshots | `snapshot()` | Capture serialized state (optional) |
| Lifecycle | `lifecycle()` / `set_lifecycle()` | Init → Running → Stopping → Stopped |
| Signals | `poll_signal()` | Consumes `Checkpoint`; `Shutdown` is enforced by the runtime via `has_shutdown_signal()` peek (stop is a runtime lifecycle duty, not consumed by machines) |
| Time | `time_tick()` / `time_ns()` | Full-precision nanosecond wall-clock or simulation time |
| Initial value | `initial_value::<T>()` | Type-safe config injection at deploy time |
| Initial value (set) | `set_initial_value::<T>(value)` | Called by deployer before spawn |

> **Observation short-circuit is a runtime duty.** Connection existence is decided by the
deployment topology (`DynamicTopology`/`materialize`), not queried by machines — `MachineContext`
does not expose `observe_is_connected()` / `output_is_connected()` (removed; see
[foundations.md](foundations.md)).

> **Time precision:** `MachineContext` uses nanosecond precision exclusively via
> `TimeTick` and `time_ns()`. There is no millisecond fallback — all time
> operations preserve full precision from the `Clock` source.

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

### Lifecycle typestate (compile-time enforcement)

The runtime `Lifecycle` enum (stored as `AtomicU8`) signals shutdown across
threads, but cannot prevent programming errors like calling `cleanup()`
before `process()` has finished. The **typestate pattern** encodes the
lifecycle phase as a type parameter, making invalid transitions a
compile-time error:

```text
MachineHandle<M, Init>     ──start()──►  MachineHandle<M, Running>
MachineHandle<M, Running>  ──stop()───►  MachineHandle<M, Stopping>
MachineHandle<M, Stopping> ──finish()──► MachineHandle<M, Stopped>
MachineHandle<M, Stopped>  ──cleanup()──► ()
```

Each state exposes only the methods valid in that state:

| State | Available methods |
|-------|-------------------|
| `Init` | `start()`, `state()`, `context()` |
| `Running` | `process()`, `stop()`, `state()`, `context()` |
| `Stopping` | `process()` (draining), `finish()`, `state()`, `context()` |
| `Stopped` | `cleanup()`, `state()`, `context()` |

```rust
use axiom::machine::{MachineHandle, Init};

let ctx = MachineContext::new("acc");
let handle = MachineHandle::<Accumulator, Init>::new(ctx)?;
let mut running = handle.start();
let out = running.process(input);
let stopped = running.stop().finish();
stopped.cleanup()?;
```

The following are **rejected by the compiler**:

```rust,compile_fail
handle.process(input);      // ERROR: no method `process` on Init
stopped.process(input);     // ERROR: no method `process` on Stopped
stopped.cleanup();          // ERROR: use of moved value (already consumed)
```

The state markers (`Init`, `Running`, `Stopping`, `Stopped`) are zero-sized
types sealed by the `LifecycleState` trait — no external code can introduce
new lifecycle states.

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

A deployment topology `DynamicTopology` is a **labeled directed multigraph**. Graph theory provides the vocabulary and algorithms to analyze it statically — before any runtime runs.

### 1. The deployment graph model

**Definition (deployment graph).**
A deployment is a labeled directed multigraph

$$\Sigma = (V, E, \ell)$$

where:

| Symbol | Code | Meaning |
|--------|------|---------|
| $V$ | `DynamicTopology::machines` $\cup$ `DynamicTopology::funcs` | Vertices: computation units |
| $E$ | `DynamicTopology::links` | Directed edges: connections |
| $\ell: E \to \text{LinkKind}$ | `LinkSpec::kind` | Edge label: physical strategy |
| $\text{in}_M$ | `Machine::port_schema().inputs()` | Incoming edges to $M$ |
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
fn inline_topological_order(spec: &DynamicTopology) -> Result<Vec<Vertex>, CycleError> {
    // Build subgraph of Inline edges only.
    // Run Kahn's algorithm or DFS-based topological sort.
    // If a cycle is detected, return the cycle vertices for error reporting.
}
```

**Purpose:** Determine execution order for machines connected via Inline links on the same thread.

**Implementation.** The algorithm is provided by the `analysis` module:
`analysis::inline_cycle()` (Kahn's algorithm) returns the offending cycle,
enforced as a hard error in `DynamicTopology::validate_deep()`;
`analysis::inline_topological_order()` exposes the order directly.

#### 3b. Strongly connected components

```rust
fn feedback_loops(spec: &DynamicTopology) -> Vec<Vec<Vertex>> {
    // Run Kosaraju or Tarjan on the full graph.
    // Return all SCCs with size > 1 — these are feedback loops.
    // For each SCC, verify that no edge within it is Inline.
}
```

**Purpose:** Identify feedback topologies. Every feedback loop must contain at least one BoundedBuf or Channel edge. A loop consisting entirely of Inline edges is a deadlock.

**Engineering rule:** A cycle of Mealy machines connected by BoundedBuf edges is a legal feedback loop (state update lags by one tick). A cycle of Inline edges is illegal.

**Implementation.** Provided by `analysis::feedback_loops()` (Tarjan's SCC,
single-pass iterative). It returns all SCCs of size > 1 as advisory
`FeedbackLoop` entries via `DynamicTopology::analyze()`.

#### 3c. Reachability

```rust
fn reachable_from(spec: &DynamicTopology, source: &str) -> HashSet<&str> {
    // BFS/DFS from source along outgoing edges.
}

fn can_reach(spec: &DynamicTopology, source: &str, target: &str) -> bool {
    // BFS/DFS from source, stop when target found.
}
```

**Purpose:**
- **Observation completeness** (Theorem 7.2): All FlowKind::Observe ports are reachable from a collector vertex, or equivalently, all observe-labeled edges lead to a sink that stores/forwards the data.
- **Control reachability**: A controller machine's control outputs reach all intended target machines.
- **Orphan detection**: Vertices with no inbound edges (except Source) or no outbound edges (except Sink) — may indicate configuration errors.

**Implementation.** Provided by the `analysis` module:
`analysis::reachable_from()` / `analysis::can_reach()` (BFS),
`analysis::observe_completeness()` (port-level BFS with link edges + internal
edges, distinguishing multiple observe ports per machine), and
`analysis::orphans()`.

#### 3d. Dominator analysis

```rust
fn single_point_of_failure(spec: &DynamicTopology) -> Vec<Vertex> {
    // Compute dominators from root(s).
    // Any vertex that dominates all paths to a critical region is a SPOF.
}
```

**Purpose:** Identify vertices whose failure disconnects the graph. A controller that all data flows through is a single point of failure — its redundancy should be considered at the deployment level.

**Implementation.** Provided by `analysis::single_points_of_failure()`
(Cooper-Harvey-Kennedy iterative dominator analysis from all source vertices,
post-processed to exclude sources themselves). It returns advisory SPOF
entries via `DynamicTopology::analyze()`.

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

**Engineering rule.** If you detect a cycle in `DynamicTopology::validate()`:
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
| Inline acyclicity | Kahn's topological sort | `DynamicTopology::validate_deep()` ← `analysis::inline_cycle()` | Implemented |
| Feedback loop detection | SCC (Tarjan) | Advisory — `analysis::feedback_loops()` via `DynamicTopology::analyze()` | Implemented |
| SPOF detection | Cooper-Harvey-Kennedy dominators | Advisory — `analysis::single_points_of_failure()` via `DynamicTopology::analyze()` | Implemented |
| Observability completeness | Port-level BFS | Advisory — `analysis::observe_completeness()` via `DynamicTopology::analyze()` | Implemented |
| Edge degree constraints | Per-port counter | `DynamicTopology::validate_deep()` ← `analysis::degree_violations()` | Implemented |
| Schema version drift | Version diff check | `LinkCompat::check()` | Implemented |
| Topology mutation cycle detection | Kahn's algorithm | `TopologyMutation::detect_cycle()` | Implemented |
| Atomic batch operations | Snapshot + rollback | `TopologyMutation::apply_batch()` | Implemented |

---

## Session Types

axiom supports **session types** to constrain the sequence of operations on
a port. Two ports can be linked only if their session protocols are dual
(see `session::is_dual`).

### Binary session types

A `SessionType` describes the protocol of a single port as a sequence of
`SessionOp`s (`Send`, `Recv`, `Choice`, `End`, etc.). Two ports with dual
session types can be safely connected — the sender's sends match the
receiver's recvs in order and type.

### Multiparty Session Types (MPST)

For protocols involving more than two participants, axiom supports
**Multiparty Session Types**:

| Concept | Type | Description |
|---------|------|-------------|
| **Global type** | `GlobalType` | The choreography — describes all interactions from a global perspective |
| **Global operation** | `GlobalOp` | A single interaction: `Message { from, to, label }`, `Choice`, `End`, `Skip` |
| **Local type** | `LocalType` | A participant's projected view of the global type |
| **Local operation** | `LocalOp` | `Send { to, label }`, `Recv { from, label }`, `End`, `Skip` |
| **Role** | `Role` (type alias for `&'static str`) | A participant identifier |
| **Projection** | `project(global, role)` | Derives a local type from a global type |

```rust
use axiom::session::{GlobalType, GlobalOp, project};

// Global choreography: Buyer → Seller (order), Seller → Shipper (dispatch)
let global = GlobalType::new(vec![
    GlobalOp::Message { from: "Buyer", to: "Seller", label: "order" },
    GlobalOp::Message { from: "Seller", to: "Shipper", label: "dispatch" },
    GlobalOp::End,
]);

// Project onto each role to get local types
let buyer_local = project(&global, "Buyer");   // Send order to Seller
let seller_local = project(&global, "Seller"); // Recv order, Send dispatch
let shipper_local = project(&global, "Shipper"); // Recv dispatch
```

**Properties enforced:**
- **Communication safety**: messages are always sent to a recipient that expects them.
- **Progress**: if all participants follow their local types, the protocol cannot deadlock.
- **Session fidelity**: the runtime can check that each message conforms to the projected local type.

---

## Topology mutation

> **Positioning: optional capability, not the default.** axiom's default
> worldview is a **static topology** — a `DynamicTopology` declared once,
> `validate_deep` checked once, the instance graph never mutated at runtime.
> Static topologies are zero-cost (monomorphized path), fully analyzable
> before deployment, and carry no dynamic tax (Theorem 15.3). The mutation
> form `TopologyMutation` exists for contract completeness (runtime
> reorganization of a pure-data `DynamicTopology`) and for the few systems
> that genuinely need runtime reconfiguration. One invariant bounds it:
>
> > **The instance graph is dynamic; the type space is static.**
>
> `Spawn`/`Link`/`Unlink`/`Retire`/`Replace` mutate *instances* of
> already-registered machine types; axiom core has no notion of loading a
> new `Machine` implementation at runtime (plugin code loading is a
> runtime-adapter concern). Legitimate mutation use cases: elastic scaling
> (replicas of an existing type), hot-swap/self-healing (`Replace`), and
> session/tenant subgraphs — and even these can usually be expressed with a
> **static topology + control/state changes** (pre-allocated replicas toggled
> via control ports, standby instances for hot-swap). Reach for runtime
> mutation only when the topology itself must be decided by the running
> system.

`TopologyMutation` (in `axiom::topology`) applies runtime
Spawn/Link/Unlink/Retire/Replace operations to an existing deployment for
systems that need to reconfigure at runtime (elastic scaling, hot-swap,
session subgraphs). It is derived from a `DynamicTopology` (`from_spec`) and
exposes `snapshot()` to recover the updated pure-data value form.

### Operations

| `TopologyOp` | Effect |
|--------------|--------|
| `Spawn` | Add a new machine instance (of an existing type) |
| `Link` | Connect two ports |
| `Unlink` | Disconnect two ports |
| `Retire` | Gracefully stop and remove a machine — the topology projection of the lifecycle (`Stopping → Stopped → cleanup`) |
| `Replace` | Atomic hot-swap (retire + spawn, links transferred) |

### Cycle detection (Kahn's algorithm)

Every `Link` operation is checked against the full link graph using
**Kahn's algorithm** for topological sorting. If a cycle is detected, the
operation is rejected with `TopologyError::CyclicDependency { cycle }`,
where `cycle` is the path of machine names forming the cycle.

### Atomic batch operations

`apply_batch(ops)` applies multiple `TopologyOp`s atomically — either all
succeed, or none are applied (rollback on first failure via pre-batch
snapshots of machines, links, and history).

```rust
use axiom::deploy::{DynamicTopology, MachineInstance};
use axiom::link::LinkKind;
use axiom::resource::MachinePhysicalSpec;
use axiom::topology::{TopologyMutation, TopologyOp};

let spec = DynamicTopology::new()
    .with_machine(MachineInstance::new("a", "worker", MachinePhysicalSpec::default()))
    .with_machine(MachineInstance::new("b", "worker", MachinePhysicalSpec::default()));
let mut topo = TopologyMutation::from_spec(&spec);
topo.apply_batch(vec![
    TopologyOp::Link { out: ("a", "out"), into: ("b", "in"), kind: LinkKind::Inline },
])?;
// If any operation fails, all are rolled back.
let updated = topo.snapshot();
```

---

## Hybrid systems

The `hybrid` module extends axiom with **continuous dynamics** alongside
the discrete `Machine` model, based on Hybrid Automata theory.

### Unified state model

The hybrid state is the product of continuous and discrete components:

```text
S = S_c × S_d
```

- `S_c` (continuous) — evolves via ODEs between jumps
- `S_d` (discrete) — transitions via instantaneous jumps

| Type | Description |
|------|-------------|
| `HybridState<C, D>` | Unified hybrid state (continuous × discrete) |
| `HybridMachine` | Trait: defines `flow()`, `guard()`, `reset()` |
| `Jump<D>` | Discrete transition: `Transition(D)`, `Reset { new_discrete }`, `Emit(String)` |
| `HybridDriver<H>` | Steps continuous dynamics, queues/applies jumps |
| `ContinuousState` | Marker trait for types usable as continuous state |

### Time integration

`HybridDriver` uses `TimeTick` for full-precision nanosecond time:

```rust
use axiom::hybrid::{HybridMachine, HybridDriver, Jump};
use axiom::prelude_all::*;

struct Thermostat;
impl HybridMachine for Thermostat {
    type Continuous = f64;
    type DiscreteState = bool;
    fn flow(c: &f64, dt: f64, d: &bool) -> f64 { /* ODE */ }
    fn guard(c: &f64, d: &bool) -> Option<Jump<bool>> { /* threshold */ }
}

let mut driver = HybridDriver::<Thermostat>::new(20.0, false);
driver.step_with_context(&ctx);  // reads TimeTick from MachineContext
let jumps = driver.apply_pending_jumps();
```

The driver computes `dt` automatically from the elapsed `TimeTick` since
the last step, preserving full nanosecond precision.
