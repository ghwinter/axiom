# axiom

**Func + Machine: typed ports, explicit topology, deploy-time physics.**

Zero-dependency computation primitives for observable, controllable systems.

`Func` (stack, stateless) and `Machine` (heap, stateful) — with typed ports, explicit link
topology, deployment specs, resource classification, and an algebraic foundation.
A companion `axiom-runtime` turns a `DynamicTopology` blueprint into a running system
(single/multi-threaded, fusion, IO multiplexing).

## What it is

```rust
use axiom::declare_ports;
use axiom::func::Func;
use axiom::machine::{CleanupError, InitError, Machine, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::resource::MachinePhysicalSpec;
use axiom::deploy::{DynamicTopology, MachineInstance};

// ── Pure function: stack, stateless, parallel-safe ──
struct Scale;
impl Func for Scale {
    type Input = f64;
    type Output = f64;
    fn name() -> &'static str { "scale" }
    fn call(x: f64) -> f64 { x * 2.0 }
}

// ── Stateful machine: heap, persistent, observable ──
declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct AccumulatorPorts {
        input type AccumulatorInput {
            input [Data] => f64,
        }
        output type AccumulatorOutput {
            output [Data] => f64,
        }
    }
}

struct Accumulator;
impl Machine for Accumulator {
    type State = f64;
    type Input = AccumulatorInput;
    type Output = AccumulatorOutput;
    type Ports = AccumulatorPorts;
    type ProcessOutput = SingleOutput<AccumulatorOutput>;

    fn name() -> &'static str { "accumulator" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<f64, InitError> { Ok(0.0) }
    #[inline]
    fn process(
        s: &mut f64,
        _: &MachineContext,
        input: AccumulatorInput,
    ) -> SingleOutput<AccumulatorOutput> {
        let AccumulatorInput::input(x) = input;
        *s += x;
        SingleOutput::Yield(AccumulatorOutput::output(*s))
    }
    fn cleanup(s: f64, _: &MachineContext) -> Result<(), CleanupError> {
        println!("final: {s}");
        Ok(())
    }
}

// ── Declare topology (DynamicTopology) ──
let spec = DynamicTopology::new()
    .with_machine(MachineInstance::new("acc", "accumulator", MachinePhysicalSpec::default()));

// ── Hand to a runtime: axiom-runtime materializes the blueprint ──
// let mut rt = axiom_runtime::Runtime::new(RuntimeConfig::sequential());
// rt.register::<Accumulator>("accumulator");
// rt.materialize(&spec)?;
// rt.tick(vec![("acc".into(), "input".into(), Box::new(1.0f64))])?;
```

## What it is NOT

- Not a runtime — `axiom` core has no executor, no event loop, no threads (runtime is `axiom-runtime`)
- Not a framework — no Application trait, no main() wrapper
- Not a pure abstraction — it co-defines the physical interface via `MachinePhysicalSpec`

## Zero-cost abstraction: two layers of existence

axiom extends Rust's zero-cost abstraction principle to the architecture level.

There are two independent layers of existence in any axiom system:

- **The abstraction layer** — modules, ports, data flow, control flow, topology graph. Mathematical objects that organize human reasoning.
- **The physical layer** — stacks, CPU, instruction set, addresses. Physical entities that execute actual computation.

The two layers are disjoint. When we say "module $M$ sends data to module $N$", what physically happens is: a thread writes bytes to an address, another thread reads them. The "module" and "sending" are semantic annotations; the physical layer has no awareness of them. axiom's job is to ensure these annotations **do not impose any runtime burden** on the physical layer.

### Two execution paths

> **Static-first: the default is a static topology; the dynamic path is the structural-dynamism adapter.**
> axiom's default model is a **static topology** — a `DynamicTopology` declared
> once, validated by `validate_deep`, and executed by the runtime or fused into
> `static_path`. `static_path` is the **principal execution layer for
> structure-fixed systems**: it covers topologies whose shape is known at
> compile time (linear / fan / diamond / composite hierarchies), dissolving
> overhead via type expansion (monomorphization). Topologies *sourced from
> runtime data* (config/plugin assembly, dynamic links, cycles' time drive)
> cannot be monomorphized and take the dynamic path. The static/dynamic split
> is a **structural criterion, not a behavioral one** (see
> `docs/structural-model.md`).

| Path | Topology shape | Topology known at | Per-message cost | Zero-cost? | Role |
|------|---------------|-------------------|------------------|------------|------|
| **static_path** (main model) | structure-fixed systems: series-parallel + composite hierarchies (chains, fans, diamonds, nested subsystems) | compile time | **zero** (0 allocs/msg) | yes | primary execution: any structure-fixed system, regardless of behavioral complexity (see `docs/structural-model.md` §2–4) |
| **DynamicTopology + Runtime** (structure-dynamic adapter) | topology sourced from runtime data (config/plugin assembly, dynamic links, cycles' time drive) | runtime | fused: 1 alloc/msg (typed-slot reuse); plain: per-hop alloc + vtable | no (dynamic tax: dispatch + type-erasure) | structural-dynamic systems: topology not known until runtime |

The static path monomorphizes over concrete machine types and inlines
`StraightLink::convert` / `StraightSplit::split` / `StraightMerge::merge` — in
release the compiled code is equivalent to hand-writing the streaming loop
directly. **`StaticChain` executes via `FlowThrough`** (see `axiom::static_exec`):
all machine states are initialized once into a type-tuple, values flow through
the whole chain element-by-element (nested calls, no intermediate `Vec`
staging), and cleanup runs once per batch — the execution shape is isomorphic
to a handwritten loop (bench: `ε ≈ 1–5%` vs handwritten, within noise). The
dynamic path must type-erase via `Box<dyn Any>` because topology
is not known until runtime; this "dynamic tax" is the dispatch + type-erasure
cost of the dynamic path — measured 1.0 allocs/msg for fused chains (typed-slot
reuse, `runtime/src/typed_slot.rs`) vs 0.000 for the static path (see
`docs/foundations.md` §15.3). **The static/dynamic split is a structural
criterion, not a behavioral one** (see `docs/structural-model.md`): axiom's
domain is the structure layer (module set + links); behavioral complexity
(what happens inside a `process`) is a black box and never requires the
dynamic path. Static covers any structure-fixed system — series-parallel
plus composite hierarchies — regardless of how complex the behavior is;
the dynamic path serves only topologies *sourced from runtime data*.

> **Scope note (anti-narrowing rule).** The static execution path
> (`axiom_runtime::static_path`) is entered exclusively through the combinators —
> `pipeline_chain` (arbitrary-depth linear chain), `diamond` (split–merge with
> arbitrary chain arms and downstream), and `feedback` (single-machine feedback
> loop). It is acyclic except for the explicit `feedback` loop (synchronous batch
> model). The combinators execute on bare payloads via `StraightMachine` — no port
> enum tags, no runtime validation of data origin/destination: origin/destination
> is fixed by the type system at compile time, so a routing mistake is a business-logic
> error, not a per-message performance tax. `Chain` (serial) and `Diamond`
> (split-merge) form a recursive algebra that generates exactly the **series-parallel
> DAGs** — pipelines, map-reduce, diamond networks, multi-level split-merge trees —
> all monomorphized. Truly arbitrary DAGs (with non-series-parallel cross edges) are
> outside this algebra: stable Rust cannot express an arbitrary edge table while
> keeping port types type-safe (the edge table is value-level, the port types are
> type-level). Such topologies take the dynamic path (`Runtime`); like the dynamic
> tax, this is a type-system boundary, not an implementation gap. See
> `docs/philosophy.md` §"The structural scope constraint" and `docs/architecture.md`
> §"Static execution path" for details.

**Empirical validation** (100k-message Transform → Sink pipeline, release build, single reference environment):

| Implementation | Relative throughput | vs hand-written |
|----------------|-------------------:|----------------:|
| Hand-written (adapter task) | 1.0× | baseline |
| Static path (monomorphized) | **1.24×** | faster |
| Dynamic path (type-erased) | 0.20× | slower |

*Relative ratios (not absolute throughput): absolute numbers vary by machine/allocator; the ordering static > hand-written > dynamic is environment-independent.*

The static path not only matches but **exceeds** hand-written — the abstraction
lets the compiler see structure that hand-written code hides, enabling it to
eliminate an intermediate task. See [`docs/philosophy.md`](docs/philosophy.md)
and [`docs/foundations.md` §15](docs/foundations.md) for the formal treatment.

## Flow semantics: Data / Control / Observe

Every port carries one of three flow kinds (`axiom::flow::FlowKind`), and link
FlowKind must match (`validate_deep` rejects mismatches):

| Flow | Semantics | Loss | Side effects |
|------|-----------|------|--------------|
| `Data` | information processed by the module, changing state content | loss = error | changes state |
| `Control` | instruction that changes behavior/configuration | droppable (latest wins) | changes behavior |
| `Observe` | state snapshot for external consumption | droppable (best-effort) | **must not** affect the source |

The three-way split is not arbitrary: the semantics differ in *loss tolerance*,
*idempotency*, and *side-effect direction*. `Observe` flows are guaranteed not to
react on their source — which is what makes a slow observation module safe to run
on its own thread with a `Dropping` carrier without stalling the main path
(empirically validated, see Showcase below).

The flow kind is a semantic annotation, not a physical property: the physical
layer moves all flows identically. The annotation implies a carrier-selection
preference (`Observe` → non-blocking, `Control` → droppable), enforced by the
`(FlowKind, LinkKind)` compatibility matrix in `validate_deep`.

## Built-in modules

`Identity<I>`, `Sink<I>`, `Source<O>`, `Tee<I>`, `Latch<T>`, `Collector<I>`, `EntityRoot`, `FuncMachine`

## Advanced features

| Feature | Module | Description |
|---------|--------|-------------|
| **Session Types** | `axiom::session` | Binary + Multiparty (MPST) protocols with `GlobalType`/`LocalType` projection, `is_dual`, `is_consistent` |
| **Streaming** | `axiom::stream` | `StreamingMachine`: pull-model iterator output (first `next()` resets cursor) |
| **Borrowed Input** | `axiom::func` | `FuncRef::call_ref`: zero-copy input (no per-call allocation) |
| **Static Execution** | `axiom::static_exec` | `Chain`/`Diamond` combinators + `StraightMachine` bare-payload pass-through (FusedInline-gated) |
| **Topology Mutation** | `axiom::topology::TopologyMutation` | Optional runtime mutation of the *instance* graph (elastic scaling, hot-swap, session subgraphs) |
| **Hybrid Systems** | `axiom::hybrid` | Continuous dynamics via `HybridMachine` (`flow`/`guard`/`reset`) with `TimeTick` integration |
| **Lifecycle Typestate** | `axiom::machine` | Compile-time enforcement of `Init → Running → Stopping → Stopped` via `MachineHandle<M, S>` |
| **Composite Machines** | `axiom::composite` | `CompositeSpec` + `expand_composites`: subsystem nesting (recursive, depth-limited) |
| **AI Blueprint** | `axiom::blueprint` *(serialize)* | JSON Schema export of `DynamicTopology` + strict reverse parser: an AI writes JSON, gets structured errors, iterates |
| **Structured Validation** | `axiom::deploy` | `validate_report`: collects **all** violations as `RuleViolation {rule_id, path, expected, actual}` (not fail-fast) |
| **Architecture Lint** | `axiom::lint` | Anti-narrowing axioms as executable rules: `no-observation`, `default-physical`, `uniform-link-kind`, … |
| **Runtime Contract** | `axiom::runtime_contract` | `RuntimeContract` trait + `Guarantees` (link carriers / exec modes / memory order / IO / delay / **physical budget**) — audit a blueprint against an adapter's physical capabilities: link carriers, exec modes, and deep physical budget (CPU affinity / exclusive cores / NUMA / huge pages / SIMD) are checked *before* deployment |

### Static-first worldview

axiom's default is a **static topology**: the `DynamicTopology` is declared once,
validated once (`validate_deep`), and never changes while the system runs.
Static topologies are zero-cost (the monomorphized `static_path` functions) and
fully analyzable before deployment (feedback loops, SPOF, degree constraints,
Inline acyclicity). Runtime topology mutation is an *optional* capability for
the few systems that need it — elastic scaling (replicas of an existing
machine), hot-swap (upgrade a machine in place), and session subgraphs
(protocols whose shape is fixed at compile time but whose instances are
created/destroyed at runtime). The instance graph may move; the type space is
static. See `docs/philosophy.md` §"Static-first worldview" for the full
rationale and the three legitimate dynamic-topology use cases.

## axiom-runtime

`Runtime` executes a `DynamicTopology` with explicit physics.

**Positioning: a catalogue of physical data-flow designs, not a mandated executor.**
Axiom's core is a constraint system: it declares *what* flows between modules and
exposes each choice for verification, but does not dictate *how* the physical
layer moves data. `axiom-runtime` is the seed of that physical layer — its
carriers are a **catalogue of replaceable physical designs** for "how data
flows": stack-passed direct calls (`Inline`), heap queues (`Channel`, `BoundedBuf`),
single-slot overwrite (`Latest`/`SharedState`), bounded FIFO (`CasFreeRing`). How
a developer composes them is a blueprint decision (`LinkKind` per edge); the
runtime's job is to provide each design and to **verify it can honor the
blueprint** (`check_spec` at `materialize`) rather than to impose one execution
shape. The bundled `Runtime`'s unified drive loop is **one way to use these
modules** (the reference configuration for deterministic single-process
systems), not the only form — modularization so that each carrier/driver is a
standalone composable unit is the current direction (see
`docs/internal/runtime-modularization-design-notes.md`).

Capabilities of the current reference runtime:

- **Execution modes**: `Inline` / `Sequential` (BFS direct delivery) / `Parallel(n)` (thread-per-machine, channel carriers)
- **Carrier matrix**: `Blocking` (backpressure) / `Dropping` (drop new) / `Overwriting` (ring) / `Latest`-`SharedState` (single slot) — the *physical realization* of a `LinkKind`
- **Lifecycle**: `Done` is a stop signal — propagates downstream (cascade shutdown), backlog dropped; parallel threads exit
- **Fusion**: chain fusion over `FusedInline` links (allocations per hop reduced)
- **Parallel cycles**: Kahn cycle detection + `stop_signal` termination
- **IO multiplexing**: `IoReactor` trait — epoll / kqueue / WSAEventSelect backends + `ManualReactor`, `default_reactor()`
- **Observation/debugging**: `Observe`-flow monitor (independent thread, `Dropping` carrier) + `Control`-flow reverse injection
- **Determinism**: same input sequence → same output (verified across Sequential/Parallel, replay)

## Examples

Core (`examples/`):

| Example | Demonstrates |
|---------|-------------|
| `http_tutorial` | Beginner path: Receiver → Calculator → Persister + ASCII topology |
| `threaded_pipeline` | Source → Tee → 2×Worker → Collector, multi-thread contract stress |
| `psql` | SQL REPL pipeline (lexer/parser/executor as `Func`/`FuncRef`), `--bench` alloc accounting |
| `declarative_dag` | Composite + multi-LinkKind declarative acceptance |
| `graph_validation` | Complex-graph validation & analysis: kernel-style graph (syscall fan-out + dual path + 3 feedback loops + observation) passes `validate_deep`; itemized detection of flow mismatch / Inline cycle / non-Moore cycle; SPOF / cycle / degree / reachability report |

Runtime (`runtime/examples/`):

| Example | Demonstrates | Verify |
|---------|-------------|--------|
| `http_declarative` | Same topology, declarative `register → materialize → tick` | Sequential == Parallel |
| `redis_like` | 6-module server blueprint: gateway → RESP → KV/List/Hash → encoder → writer + AOF; **monitor (Observe) + debugger (Control)** | `--replay` deterministic; `--bench` carrier effect |
| `mmo` | MMO core subgraph: sessions (lifecycle + heartbeat timeout), world shard, per-player view projection, event sourcing | `--replay` event-stream determinism |
| `netpath` | Kernel network RX path: pcap → Ethernet → IP → TCP → deliver + stats observer | `--replay` byte-identical double replay |
| `composite_machine` | CompositeSpec expansion at runtime | — |
| `bench_runtime` | Runtime overhead accounting | — |

**Showcase: observation must not slow the main path** (`redis_like --bench`, Parallel(4), monitor simulates 20µs/event):

| Config | Main-path throughput (relative) |
|--------|-------------------------------:|
| baseline (no monitor) | 1.0× |
| monitor + **Blocking** | **0.2× (-80%)** — observation stalls the main path |
| monitor + **Dropping** | **≈1.0×** — observation dropped, main path clean |

*Relative ratios from a single reference environment; absolute cmd/s vary by machine. The pattern (Blocking stalls, Dropping does not) is environment-independent.*

The slow observation module running on its own thread with a `Dropping` carrier
does not stall the main path; `Blocking` would. This is the empirical statement
of "low-speed behaviors live on independent threads; the blueprint does not
dictate physics, the carrier choice does".

## Tests

- `axiom` core: **358 tests** (238 src unit + 120 integration, incl. source audit) + 19 doctests — all green
- `axiom-runtime`: **84 tests** — all green
- Verification philosophy: evidence corpus `evidence/` (E-contracts + R-benchmarks, local-only, not in git)

## Further reading

| Document | What it covers |
|----------|---------------|
| [`docs/foundations.md`](docs/foundations.md) | Algebraic foundation — axioms, theorems, proofs |
| [`docs/philosophy.md`](docs/philosophy.md) | Design philosophy — abstraction vs physics, control/data blur |
| [`docs/design-principles.md`](docs/design-principles.md) | Meta-problems & design principles — zero-cost as shape isomorphism, verification judgement, physical process as a finite choice set |
| [`docs/doc-governance.md`](docs/doc-governance.md) | Documentation standards & decision records — tier taxonomy, word budgets, decision log |
| [`docs/adapters.md`](docs/adapters.md) | Adapter ecosystem rules & runtime-contract certification — Guarantees audit, release tiers |
| [`docs/architecture.md`](docs/architecture.md) | Architecture details — ports, links, deployment, runtime comparison |
| [`docs/structural-model.md`](docs/structural-model.md) | Formal structural model — system as typed graph, structure vs behavior layer, static/dynamic criterion (set/graph/category theory) |
| [`docs/architecture_diagrams.md`](docs/architecture_diagrams.md) | Diagrams — system layers, link strategies, deployment, analysis |
| [`docs/migration-0.2.md`](docs/migration-0.2.md) | Migration guide — 0.1.x → 0.2.0 breaking changes and replacements |
| [`CHANGELOG.md`](CHANGELOG.md) | Release notes — 0.2.0 breaking refactor, bench regression results |

## Why "axiom"

An axiom is a self-evident truth that serves as a foundation. `Func` and `Machine` are the axioms of computation organization. Everything else is derived.
