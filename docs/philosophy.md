# axiom Philosophy

> **Reading guide.** This document discusses axiom's design philosophy — what it is not, why it exists, and what problems it tries to solve. If you care more about concrete usage, return to the README.

---

## Abstraction vs physics

Every abstraction marks the undifferentiated physical process of memory reads and writes with semantic labels: `PortDir::In`, `"control"`, `"data"`. The physics knows none of these — the CPU only loads from addresses and stores to addresses. But labels make code readable, systems reason-able, and errors localizable.

axiom does not try to eliminate labels. axiom ensures labels do not interfere with physical optimization, while keeping topology explicit and verifiable.

### Two layers of existence

There are two independent layers of existence in any axiom system:

- **The abstraction layer** $\mathcal{A}$ — modules, ports, data flow, control flow, topology graph. These are mathematical objects that organize human reasoning. They make the system readable, verifiable, and locally analyzable.
- **The physical layer** $\mathcal{P}_h$ — stacks, CPU, instruction set, cache lines, addresses. These are physical entities that execute actual computation.

The two layers are **disjoint** ($\mathcal{A} \cap \mathcal{P}_h = \emptyset$). When we say "module $M$ sends data to module $N$", what physically happens is: a thread writes bytes to an address, another thread reads them. The "module" and "sending" are semantic annotations on top of physical memory operations; the physical layer has no awareness of them.

Consider a receive-and-print system with two threads: $T_1$ receives network data, $T_2$ computes and prints. The abstraction layer describes this as a graph `Receive → Print`, a directed flow of data between two modules. The physical layer describes the same phenomenon as: $T_1$ executes `recv()` writing into buffer $B$, $T_2$ reads from $B$ and executes `write(stdout)`. Adding a persistence module $M_S$ in the abstraction layer extends the graph to a fan-out `Receive → {Print, Persist}`; physically, it just means $T_1$ (or a clone) also writes into a second buffer. The graph structure exists only in $\mathcal{A}$; the physical layer sees only memory operations.

### Zero-cost abstraction: the non-invasion axiom

Rust's zero-cost abstraction principle says: **what you don't use, you don't pay for; what you do use, you couldn't write better by hand**. axiom extends this to the architecture level.

**Axiom (non-invasion):** An abstraction $\alpha \in \mathcal{A}$ is zero-cost if and only if its physical realization $[\![\alpha]\!]$ satisfies:

1. **Runtime existence disappears** — $\alpha$ allocates no data structure in the compiled artifact. There is no runtime object whose type is `Port` or `Link`.
2. **Execution time is unchanged** — $t([\![\alpha]\!]) = t(h_\alpha) + \epsilon$, where $h_\alpha$ is the equivalent hand-written physical implementation and $\epsilon$ is compiler-optimization noise (typically $|\epsilon| / t(h_\alpha) < 0.05$).
3. **Memory footprint is unchanged** — steady-state memory of $[\![\alpha]\!]$ equals that of $h_\alpha$.

When the business logic is correct and the compiler accepts the program, the physical execution is correct and bears no extra burden from the abstraction. This is the core meaning of "axiom does not invade the runtime": the abstraction constrains the design, not the execution.

### The abstraction-physics boundary contract

> **Principle (abstraction-physics separation).** The abstraction layer
> $\mathcal{A}$ defines *what* a module is, *what* its boundary is, and *what*
> flows between modules. It must not encode *how* the physical layer
> $\mathcal{P}_h$ executes that flow — not the thread model, not the
> synchronisation model, not the memory-copy policy. Physical constraints
> belong to the runtime layer, which applies them on demand based on the
> deployment context.

A module's definition (`Machine` trait, `PortSet`, `HasPortInfo`) should only
express the abstract structure: "this module has state $S$, accepts input on
ports $\Gamma_{in}$, produces output on ports $\Gamma_{out}$, and computes
$\delta: (S, I) \to (S', O)$". Physical execution details — which thread
runs the module, which channel carries the data, whether stages are fused
into one loop — are runtime concerns, not abstract definitions.

#### What is an algebraic property (NOT a physical leak)

Some trait bounds look like physical constraints but are in fact **algebraic
properties of the computation** — they belong to the abstraction layer and
must not be moved out:

1. **Output arity (`SingleOutput` / `TupleOutput` / `MultiOutput`).** The
   number of outputs a machine produces per `process()` call — exactly 1,
   exactly 2, or runtime-variable N — is a structural property of
   $\delta: S \times I \to S' \times O$. It is as algebraic as `Vec<T>`
   vs `(A, B)` is a type-level distinction. This is **not** a physical
   constraint.

2. **Fusion eligibility (`FusedCompatible` sealed marker).** Whether a
   machine is safe to fuse into a single-loop pipeline is determined by its
   output arity (fixed-arity machines can be fused; variable-arity
   `YieldMulti` machines cannot, because the fusion loop would drop
   fan-out outputs). The `FusedInline` trait encodes this as a
   **compile-time proof** via the sealed `FusedCompatible` marker — no
   `unsafe`, no runtime check, the compiler refuses to fuse an
   incompatible machine. Replacing this with a proc-macro check would
   **downgrade a complete proof to a syntactic heuristic** (proc macros
   cannot see trait implementations). This is the correct design and is
   not a leak.

3. **`Clone` is NOT required by `HasPortInfo`.** A non-`Clone` payload (an
   owned buffer, a handle, a `Box<dyn Fn>`) can be transported via
   `into_any`/`from_port_name` (type-erased move). Clone is needed only by
   specific link kinds (`Channel`, `BoundedBuf` when multicasting), which
   require it at the link level, not at the port-trait level. This is the
   correct separation and is not a leak.

#### What is a deployability commitment (deliberately on the trait)

`Machine: Send + Sync + 'static`, `PortSet: Send + Sync + 'static`, and
`HasPortInfo: Send + Sized + 'static` are **deployability commitments**,
not leaks. axiom's design target is systems that *can* be deployed across
threads — `DynamicTopology` declares the topology, the runtime materialises it
across threads, and `Send + Sync` is the type-level contract that makes
multi-threaded deployment sound. Removing these bounds would force every
consumer (`MachineHandle`, `StreamingMachine`, runtime materialiser) to
repeat the bound at each use site, raising API complexity for a near-zero
benefit (supporting `Rc`-internal single-threaded machines). The bounds
stay on the core traits.

#### What genuinely belongs to the runtime layer

The physical execution details that *should* live in the runtime, not in
core, are:

- **Link carrier selection** — `LinkKind` declares the abstract link
  property (`Inline` / `BoundedBuf` / `Channel` / ...); the runtime chooses
  the concrete carrier (`mpsc::Sender`, `Arc<RwLock<T>>`, lock-free ring,
  stack-passed value). The carrier is a physical object; `LinkKind` is an
  abstract declaration.
- **Execution backend** — `ExecutionHint` declares the abstract execution
  class (`Async` / `CpuBound` / `CpuBoundN` / `ThreadPool`); the runtime
  maps it to a concrete executor (an async task, a dedicated thread, a
  thread-pool scope, an inline call).
- **Backpressure execution** — `BackpressurePolicy` declares the abstract
  policy (`Block` / `Drop` / `Overwrite` / `Credit`); the runtime enforces
  it on the physical carrier.
- **Resource accounting** — `MachinePhysicalSpec` declares the abstract
  resource budget (heap bytes, cache-line alignment, cleanup latency); the
  runtime allocates and accounts against it.

These are the physical half of each abstraction. They exist in core as
*declarations* (so the topology is verifiable), but their *execution*
belongs to the runtime.

### How axiom realizes zero-cost

The compiler can fully dissolve an abstraction at compile time when three conditions hold (formal proof in [foundations.md §15.2](foundations.md)):

- (a) All type parameters are known at compile time (**static deployment**);
- (b) All conversion functions are pure and inlined;
- (c) No runtime type erasure (no `Box<dyn Any>`, no vtable).

axiom offers **two deployment paths** corresponding to whether these conditions can be met:

| Path | Function | Topology known at | Per-message cost | Zero-cost? | Use case |
|------|----------|-------------------|------------------|------------|----------|
| **Static** | `axiom_runtime::static_path::{pipeline_chain, diamond, feedback}` (combinators) | compile time | **zero** | yes | series-parallel pipelines, hot paths (**default**) |
| **Dynamic** | `Runtime::materialize(spec)` | runtime | fused: 1.0 allocs/msg (typed-slot reuse); plain: per-hop alloc + vtable | no | runtime topology mirror (rare) |

The static path encodes topology in type parameters; the compiler monomorphizes over concrete machine types, inlines `StraightLink::convert` / `StraightSplit::split` / `StraightMerge::merge`, and emits code equivalent to hand-writing the batch loop directly. The dynamic path must type-erase via `Wire { payload: Box<dyn Any> }` because topology is not known until runtime — this "dynamic tax" is the dispatch + type-erasure cost of the dynamic path (see [foundations.md §15.3](foundations.md)).

> **Static path entry points.** The static execution path
> (`axiom_runtime::static_path`) is entered exclusively through the combinators:
> `pipeline_chain` (arbitrary-depth linear chain), `diamond` (split–merge with
> arbitrary chain arms and downstream), and `feedback` (single-machine feedback
> loop). `Chain` (serial) and `Diamond` (split-merge) form a recursive algebra
> that generates exactly the **series-parallel DAGs**, all monomorphized. Truly
> arbitrary DAGs (non-series-parallel cross edges) are outside this algebra —
> stable Rust cannot express an arbitrary edge table while keeping port types
> type-safe — so they take the dynamic path; like the dynamic tax, this is a
> type-system boundary, not an implementation gap. See [architecture.md](architecture.md)
§"Static execution path" for the current API.

> **Static-first principle.** Static topology is the **default** worldview: a
> `DynamicTopology` declared once, `validate_deep` checked once, the instance
> graph never mutated at runtime. The mutation path exists for **contract
> completeness** (`TopologyMutation`, the runtime mirror of the pure-data
> `DynamicTopology`, must exist as a type) and for the rare case where the
> topology itself must be decided by the running system. One invariant bounds it: **the instance graph is dynamic; the type
> space is static** — the dynamic path can create/remove *instances* of
> already-registered machine types, but cannot load new `Machine`
> implementations. Most scenarios casually called "dynamic" (auto-scaling,
> hot-swap, elastic routing, plugin hosting) are expressible as **static
> topology + state mutation** (pre-allocated pools toggled via control ports,
> routing tables held in machine state, `Box<dyn Trait>` plugins as machine
> state data). The dynamic tax is justified only when this reduction is
> impossible.

### The structural scope constraint (anti-narrowing rule)

**Axiom (structural scope):** axiom's structural layer must express
**arbitrary topology** — multi-pipeline, fan-out, fan-in, directed cycles,
nested composition — as first-class static definitions. A single linear
pipeline $A \to B \to C$, a single thread of execution, or a single
fixed-function path are **narrow subsets** of the design space. They are
**not** the default, **not** the target, and **not** an acceptable
fallback when full structural power proves difficult to implement.

> **Formal basis.** The strict definitions live in
> [structural-model.md](structural-model.md): system = typed graph (§1); axiom's domain is
> the **structure layer** (module set + links), behavior is a black box
> (§2 — behavioral complexity never requires the dynamic path); static =
> topology as types, dynamic = topology as values materialized at runtime
> (§3); static covers series-parallel + composite hierarchies (§4).

> **Forbidden as axiom's default capability:**
>
> 1. **Single-pipeline bias** — designing the type system, examples, or
>    validation around the linear chain $A \to B \to C$ and treating
>    fan-out / fan-in / DAG / cycles as edge cases or "future work".
> 2. **Single-thread assumption** — any structural definition that is only
>    sound under sequential execution, offloading real concurrency to
>    "runtime magic" instead of expressing it in the blueprint.
> 3. **Single-function narrowing** — any API surface whose ergonomics
>    collapse when the topology grows beyond one machine type or one
>    transformation stage.
>
> The blueprint (see "Two layers of existence") describes systems of
> arbitrary complexity: many modules, many connections, many threads,
> nested sub-systems. axiom's value is making **that** complexity
> compilable and verifiable — not making the trivial case fast. A narrow
> subset may serve as a minimal validation probe (e.g. the `pipeline_chain`
> zero-cost probe in the next section), but must never be mistaken for
> axiom's capability ceiling.

This rule governs design decisions: when a feature is "easy for linear
chains but hard for general DAGs", the answer is **solve the general case
or mark it explicitly out-of-scope** — not silently narrow axiom to the
subset where it is easy.

### Empirical validation

On a 100,000-message Transform → Sink pipeline, release build (single reference environment):

| Implementation | Relative throughput | vs hand-written | Latency (relative) |
|----------------|-------------------:|----------------:|-------------------:|
| Hand-written (adapter task) | 1.0× | baseline | 1.0× |
| Static path (monomorphized) | **1.24×** | faster | 0.53× |
| Dynamic path (type-erased) | 0.20× | slower | n/a* |

*Relative values; absolute throughput/latency vary by machine and allocator — the ordering static > hand-written > dynamic is environment-independent.*

The static path not only matches but **exceeds** hand-written performance — because the abstraction lets the compiler see the conversion structure that hand-written code hides, enabling it to eliminate an intermediate task. This positively validates the non-invasion axiom: the abstraction's existence adds no burden to the physical layer; it can even inspire physical optimization.

### Fused pipeline: dissolving the data-flow metaphor

The non-invasion axiom has a second, stricter validation path: the **static execution path** (`axiom_runtime::static_path`). A pipeline of `Add → Mul → Sub` machines, run via `pipeline_chain`, is compiled into a single loop equivalent to a hand-written `for` loop — no intermediate `Vec`, no trait dispatch, no function-call boundaries between stages.

On a 1,000,000-element pipeline, release build (min of 5 runs):

| Implementation | Time | Allocations | vs hand-written loop |
|----------------|-----:|------------:|---------------------:|
| Hand-written `for` loop | 1.999 ms | 1 | baseline |
| `pipeline_chain` (fused, static path) | 1.904 ms | 1 | **0.95x (4.8% faster)** |
| Hand-written `iter().map().collect()` | 1.570 ms | 1 | 0.79x (auto-vectorized) |
| Chained per-machine `run` (unfused) | 19.255 ms | 67 | 9.63x slower (documented Vec tax) |

The static path matches the hand-written loop within compiler-optimization noise ($|\epsilon| / t(h_\alpha) < 0.05$), with **zero** extra allocations. The Machine/Port/Link abstraction dissolved into pure computation — the "data flow" metaphor has no physical cost at this layer.

The iterator chain (`iter().map().collect()`) runs faster due to compiler auto-vectorization of contiguous iterator adapters — a compiler optimization of the *pattern*, not a tax from axiom's abstraction. A hand-written `for` loop has the same gap vs the chain. The static path uses a `for` loop (it must, to support `Idle`/`Done` control flow), so the fair comparison is against the loop.

The chained per-machine `run` path (N independent loops + N-1 intermediate `Vec`) documents the cost of *not* fusing — this is the "Vec tax" that `pipeline_chain` eliminates.

### Codifying compiler knowledge: from guessing to verifying

The fused pipeline validation above was achieved by understanding *why* the compiler fails to fuse stages, not by guessing. This methodology — reasoning about compiler behavior from first principles and verifying with measurements — is itself part of axiom's design contract. The principle:

1. **Type contract** — `Machine::process` implementations marked `#[inline]` enable cross-crate inlining; without it, the static path cannot dissolve stage boundaries.
2. **Combinator monomorphization** — the static path is entered through the combinators `pipeline_chain`/`diamond`/`feedback` over `StraightMachine`s; the topology is encoded in types (`Chain`/`Diamond`), so the compiler monomorphizes the entire shape and fuses stage boundaries into a single loop (no per-stage function-call barrier).
3. **Batch collection** — the static path collects outputs into `Vec` per stage; the batch model avoids per-message allocation while keeping `Idle`/`Done` control flow expressible.
4. **Fair baseline** — compare against the structurally equivalent hand-written `for` loop, not the auto-vectorized iterator chain.

This is the empirical form of the non-invasion axiom: every abstraction path in axiom should be validated against its hand-written equivalent, and the validation should be reproducible as a benchmark and automatable as a CI gate.

### Separation of layers in the codebase

The separation theorem ([foundations.md §15.4](foundations.md)) has a concrete codebase invariant:

- **axiom core** (`src/`) is the formalization of $\mathcal{A}$. It contains no task spawning, no `std::thread::spawn`, no `async fn`, no `Future` implementation, no runtime objects. It defines traits and types only.
- **axiom adapters** (the reference `axiom-runtime`, or third-party adapters
  such as `axiom-tokio`) are interpreters $\mathcal{A} \to \mathcal{P}_h$. They
  implement the execution on specific runtimes.

The abstraction layer never depends on a specific runtime. Swapping the adapter changes the physical execution strategy (threading, scheduling, IO model) without changing the abstraction's semantics.

---

## Control is data

At the physical level, "Controller sends command to Sensor" and "Sensor sends reading to Controller" are the same operation: one thread writes to a memory address, another thread reads from it.

The distinction exists only in how the receiving Machine's `process()` interprets the value. A safety stop flag (`AtomicBool`) and a new sampling interval (`mpsc::channel<u64>`) use the same physical mechanism, but one triggers a shutdown and the other changes a configuration parameter.

**There is no "control" in the physical layer. Control is interpreted by the receiving end's process().**

Consequently, the IO-Object model is minimal:

```
IO-Object = (S, I, O, δ)
```

There is no separate `Observe` type. Observation data is just `Output` flowing through ports labelled `FlowKind::Observe`. There is no separate `Control` type. Control signals are just `Input` flowing through ports labelled `FlowKind::Control`.

Both are **port annotations**, not **type parameters**.

### Observation and debugging are first-class modules, not meta-tools

The consequence of "Control is data" and "Observe is data" is that **observation
and debugging are ordinary modules in the topology** — not external tooling bolted
onto the side:

- **Observing is acquiring data.** A `Monitor` machine subscribes to an `Observe`
  output via an ordinary link. The observation stream is verified the same way
  any data stream is (determinism, event sourcing).
- **Debugging is injecting data/control.** A `Debugger` machine feeds a `Control`
  input (e.g. `DEBUG FLUSH/SET/INFO`) through an ordinary reverse link. Reverse
  injection is not meta-layer magic; it is an edge in the graph.
- **Slow observation must not slow the main path.** Because `Observe` flows are
  guaranteed not to react on their source, a slow observation module can run on
  its own thread with a `Dropping` carrier (observation overruns are dropped,
  the main path is untouched). Empirical validation (`redis_like --bench`,
  Parallel(4), monitor simulating 20µs/event):

  | carrier | main-path throughput |
  |---------|---------------------|
  | none (baseline) | 100% |
  | `Blocking` | **-80%** (observation stalls the main path) |
  | `Dropping` | **≈baseline** (observation dropped, main path clean) |

  The carrier choice — not the blueprint — is what decides whether slow
  observation perturbs the system. This is "deploy-time physics" applied to
  observability itself.

### Module boundaries are physical parallelism boundaries

`RuntimeConfig::Parallel(n)` runs thread-per-machine: each module occupies its
own OS thread, links become channel carriers. This makes the blueprint's graph
the parallel schedule directly — a fan-out node fans work across threads, a
fan-in node aggregates them. The abstraction layer does not encode this model
(it is one configuration among `Inline`/`Sequential`/`Parallel`); but when
thread-switch costs are non-trivial, partitioning modules across threads (core
logic / network / cache / persistence / observation each on its own thread)
pays off because total throughput is bounded by the slowest module, not by the
serial sum. The graph's complexity (fan-in, fan-out, cycles, acyclic chains) is
precisely why the topology is verified (`validate_deep` + `analysis`) before
it is deployed onto threads.

---

## Port annotations are labels; LinkKind is physics

The same physical-layer argument applies to **all** port annotations — not
just FlowKind, but `PortDir` (In/Out) and the port name itself. None of them
have physical force:

- An `In` port and an `Out` port are, at the physical layer, the same thing:
  a slot in a buffer, a variable, or (for `Inline`) nothing at all. The
  distinction is *algebraic* (which end of a directed edge produces, which
  consumes) and *verificational* (`can_link_to` requires out → in;
  `validate_deep` uses direction to check the topology).
- The port name is *identity*: the coordinate of the data in the topology
  graph. It is what makes "the same u64" two different things when it sits
  on two different ports (Yoneda view: an object is its relations).

Physical differences are carried **entirely** by `LinkKind`. The same
`PortDecl` can be connected via `BoundedBuf` (a real buffer: writes and
reads have physical existence), `Latest` (a single variable: overwrite), or
`Inline` (a function call: no buffer, no variable, no allocation). `Inline`
is the extreme of abstraction-dissolution: the topology structure **disappears
in the physical layer** — values move through registers/stack, no data flows,
no memory is allocated — yet the semantic-layer topology remains expressible
and verifiable (`validate_deep` still enforces Inline acyclicity and degree
constraints).

So: **what is classified is the annotation, not the physics.** Physical
symmetry ("ports are essentially no different") and algebraic asymmetry
(direction and flow kind constrain validation) coexist. This is the
abstraction/physics decoupling of §15 applied at the port level — and it is
why axiom's zero-cost claim holds for `Inline`: the abstraction imposes no
runtime burden precisely because the physical layer has nothing to maintain.

---

## Module boundaries are conventions, not walls

A boundary between two modules is where one module's code stops having direct access to another module's State fields. In axiom this is enforced by Rust's module system: each Machine's State is a private struct, only accessible by its own `process()`.

But this boundary is a convention, not a physical requirement. At the hardware level, nothing prevents a thread from writing to arbitrary memory addresses. The convention exists to make the system independently reason-able — each Machine can be understood in isolation.

If the convention is consistently followed, the system is maintainable. If it is broken (e.g., via `unsafe` shared state), the system is still physically valid but no longer locally reason-able.

**axiom is designed for reasoning reliability, not physical possibility.**

This principle extends to lifecycle management: the **typestate pattern**
(`MachineHandle<M, S>`) encodes lifecycle phases as type parameters, so that
calling `process()` on a stopped machine or `cleanup()` before `stop()` is
not just a runtime error — it is a **compile-time impossibility**. The
type system becomes a proof system: if the code compiles, the lifecycle
ordering is correct. This is reasoning reliability pushed to its limit:
the compiler enforces what would otherwise be a runtime convention.

## Modules are ordinary: persistence is not special

The defining property of a Machine is **ordinariness**: any
"input → state transition → output" is a Machine. There is no privileged
category. A module that reads a zip is a Machine; a module that reads a
persisted pack from disk is *the same kind of thing*. This triviality
dissolves a whole class of traditional machinery:

- **Persistence is not a mechanism.** Restoring state after a restart is
  not a lifecycle hook or a serialization framework — it is an ordinary
  module whose function happens to read from disk. In `redacted-project`,
  `pack_loader` (reads the last persisted pack) and `zip_reader` (reads a
  zip) are indistinguishable at the module-definition level.
- **Restart is ordinary.** Re-materializing the same blueprint is the only
  "recovery" operation. State continuity is data-flow continuity: the
  loader emits the restored state as the first hop of the data flow. The
  graph never changes shape.
- **"Where data comes from" is not a special question.** zip, disk,
  network, keyboard, clock — all are sources, all are ordinary Machines.
  There is nothing to name, nothing to pattern-match. Naming "patterns"
  (persistence pattern, recovery pattern) would be an admission that the
  module is *not* ordinary — which it is.

This is the strongest form of the triviality claim: even state recovery —
traditionally the most framework-heavy concern — collapses into an ordinary
module. axiom does not add concepts for it; it proves none are needed.

---

## Positioning: a mapping layer

axiom is not a pure abstraction layer and not an implementation framework. It is a **mapping layer** — it sits between the two.

```
Application         (betarc, server, firmware)
     ▲                    │
     │                    │ writes Machine / Func
     ▼                    ▼
axiom            ←  mapping layer
  Func / Machine         defines computation units
  PortSchema / LinkSpec  defines topology
  ExecutionHint /        defines physical resource interfaces
    PhysicalSpec
  DynamicTopology             maps abstract → physical
     ▲                    │
     │                    │ implements scheduling, threading, channels
     ▼                    ▼
Runtime adapters  (reference: axiom-runtime; third-party: axiom_tokio, axiom_rayon)
     ▲
     │
     ▼
OS / Hardware
```

A pure abstraction layer says "what to do" and knows nothing about the physical layer. A framework says "how to do it" and owns the physical layer. axiom says **both in the same type system, but the upper layer does not depend on any specific implementation of the lower layer.**

`Machine::process()` is pure abstract. `MachinePhysicalSpec::execution` is a physical declaration. They live in the same trait. The application author writes `process()` without knowing which runtime will drive it. The deployer writes `DynamicTopology` to map each machine to a physical execution strategy, without changing the machine's code.

This is not abstraction for abstraction's sake. It is **co-expression of intent and resource** — the machine declares what it needs, the deployer provides what the machine gets, and the two are checked for consistency at link/deploy time.

---

## What axiom is not

- Not a runtime (no executor, no event loop)
- Not a framework (no Application trait, no main() wrapper)
- Not a trading engine (no betarc semantics)
- Not a pure abstraction layer — it also defines the physical interface
- Not a replacement for business logic — it only guarantees structural correctness

---

## Why "axiom"

An axiom is a self-evident truth that serves as a foundation. `Func` and `Machine` are the axioms of computation organization. Everything else is derived.
