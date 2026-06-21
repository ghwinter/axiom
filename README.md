# axiom

Minimal computation primitives. Zero dependencies, zero runtime assumptions.

## What it is

`axiom` defines two fundamental computation types:

| Primitive | Memory | State | Observable | Controllable | Connection |
|-----------|--------|-------|------------|--------------|------------|
| `Func`    | Stack  | None  | No         | No           | Inline call |
| `Machine` | Heap   | `S`   | Yes        | Yes          | Ports (BoundedBuf / Channel / Latest / CasFreeRing / SharedState) |

Plus typed ports, explicit link topology, deployment specs, and resource classification.

## What it is NOT

- Not a runtime (no tokio, no executor, no event loop)
- Not a framework (no Application trait, no main() wrapper)
- Not a trading engine (no betarc semantics)
- Not a pure abstraction layer — it also defines the physical interface

## Usage

```rust
use axiom::prelude_all::*;

// Define a pure function
struct Scale;
impl Func for Scale {
    type Input = f64;
    type Output = f64;
    fn name() -> &'static str { "scale" }
    fn call(x: f64) -> f64 { x * 2.0 }
}

// Define a stateful machine
struct Accumulator;
impl Machine for Accumulator {
    type State = f64;
    type Input = f64;
    type Output = f64;
    type Observe = String;
    fn name() -> &'static str { "accumulator" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<f64>("in"))
        .with(PortDecl::output::<f64>("out"))
        .with(PortDecl::observe::<String>("log")) }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<f64, InitError> { Ok(0.0) }
    fn process(s: &mut f64, _: &MachineContext, x: f64) -> ProcessOutput<f64> {
        *s += x;
        ProcessOutput::Yield(*s)
    }
    fn cleanup(s: f64, _: &MachineContext) -> Result<(), CleanupError> {
        println!("final: {}", s);
        Ok(())
    }
}

// Declare topology
let deploy = DeploySpec::new()
    .with_machine(MachineInstance {
        name: "acc",
        machine_type: "accumulator",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    });

// Hand to a runtime adapter:
//   axiom_tokio::Runtime::from_deploy(deploy)?.run();
```

## Architecture

### Two computational primitives

`Func(I) -> O` — a pure function. Stack frame, instant, unobservable. The same input always produces the same output. Used for parsing, serialization, mathematical transforms — anything that does not retain state between invocations.

`Machine(S, I, O, Observe)` — a state machine. Heap-allocated State lives across repeated process() calls, observable via PortRegistry and a dedicated observe port. Configurable via ConfigCell entries registered during init.

### Ports as the universal connection point

A Machine declares input ports, output ports, and an observation port. Ports carry type metadata and a schema version. Port compatibility is checked at link time: same TypeId, version drift ≤ 1 allows automatic migration.

Three directions:
- `In` — data flows in. Consumed by process().
- `Out` — data flows out. Produced by process().
- `Observe` — structured observation data flows out. Read-only from outside, connected to a Collector sink.

### Links as explicit topology

The connection between two ports is a `LinkSpec`. It specifies the physical connection strategy:

| Kind | Physics | When |
|------|---------|------|
| `Inline` | Function call, zero allocation | Same-thread, Func→Func or Machine→Func |
| `BoundedBuf` | Lock-based ring buffer, configurable backpressure | Cross-thread, producer-consumer |
| `Channel` | MPSC channel | Multiple producers, single consumer |
| `Latest` | Single overwrite slot | Status feed, UI refresh |
| `CasFreeRing` | Lock-free SPSC, fixed address | Interrupt → main-loop, embedded |
| `SharedState` | Arc\<RwLock\<T\>\> | Config distribution, shared metrics |

### Deployment as a separate concern

A `DeploySpec` is a pure data structure: it describes what Machines exist, how they connect, and with what physical resources. It does not execute anything. A runtime adapter interprets the spec.

The same Machine type can be deployed differently:
- **Backtest**: `CpuBound`, deterministic, `Inline` links, zero allocation
- **Production**: `Async` or `ThreadPool`, nondeterministic, `BoundedBuf` links, backpressure
- **Embedded**: `CpuBound`, static allocation, `CasFreeRing` links, no heap

The Machine implementation does not change. Only the DeploySpec changes.

## Philosophical foundation

### Abstraction vs physics

Every abstraction marks the undifferentiated physical process of memory reads and writes with semantic labels: `PortDir::In`, `"control"`, `"data"`. The physics knows none of these — the CPU only loads from addresses and stores to addresses. But labels make code readable, systems reason-able, and errors localizable.

axiom does not try to eliminate labels. axiom ensures labels do not interfere with physical optimization, while keeping topology explicit and verifiable.

### Control is data

At the physical level, "Controller sends command to Sensor" and "Sensor sends reading to Controller" are the same operation: one thread writes to a memory address, another thread reads from it.

The distinction exists only in how the receiving Machine's `process()` interprets the value. A safety stop flag (`AtomicBool`) and a new sampling interval (`mpsc::channel<u64>`) use the same physical mechanism, but one triggers a shutdown and the other changes a configuration parameter.

**There is no "control" in the physical layer. Control is interpreted by the receiving end's process().**

### Module boundaries are conventions, not walls

A boundary between two modules is where one module's code stops having direct access to another module's State fields. In axiom this is enforced by Rust's module system: each Machine's State is a private struct, only accessible by its own `process()`.

But this boundary is a convention, not a physical requirement. At the hardware level, nothing prevents a thread from writing to arbitrary memory addresses. The convention exists to make the system independently reason-able — each Machine can be understood in isolation.

If the convention is consistently followed, the system is maintainable. If it is broken (e.g., via `unsafe` shared state), the system is still physically valid but no longer locally reason-able.

**axiom is designed for reasoning reliability, not physical possibility.**

### Positioning: a mapping layer

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
  DeploySpec             maps abstract → physical
     ▲                    │
     │                    │ implements scheduling, threading, channels
     ▼                    ▼
Runtime adapters  (axiom_tokio, axiom_rayon, axiom_linear)
     ▲
     │
     ▼
OS / Hardware
```

A pure abstraction layer says "what to do" and knows nothing about the physical layer. A framework says "how to do it" and owns the physical layer. axiom says **both in the same type system, but the upper layer does not depend on any specific implementation of the lower layer.**

`Machine::process()` is pure abstract. `MachinePhysicalSpec::execution` is a physical declaration. They live in the same trait. The application author writes `process()` without knowing which runtime will drive it. The deployer writes `DeploySpec` to map each machine to a physical execution strategy, without changing the machine's code.

This is not abstraction for abstraction's sake. It is **co-expression of intent and resource** — the machine declares what it needs, the deployer provides what the machine gets, and the two are checked for consistency at link/deploy time.

### Why "axiom"

An axiom is a self-evident truth that serves as a foundation. `Func` and `Machine` are the axioms of computation organization. Everything else is derived.

## Examples

```bash
# Func + Machine, single-threaded inline
cargo run --example counter

# Two Machines chained sequentially
cargo run --example pipeline

# 8 threads, 9 channels, in-memory persistence, control vs data blur
cargo run --example complex_topology
```

## Runtime adapters

| Crate | Runtime | Use case |
|-------|---------|----------|
| `axiom_tokio` | Tokio multi-thread | Production servers |
| `axiom_replay` | Deterministic single-thread | Backtesting, simulation |
| `axiom_embassy` | Embassy async | Embedded, no_std |
| `axiom_linear` | for loop | Bare-metal, testing |

Each adapter interprets `DeploySpec` to construct machines, connect ports, and drive the `process` loop.

## Runtime comparison

axiom's `process()` is **synchronous by design**. This is not an oversight — it is what makes axiom compatible with every runtime without the runtime leaking into the trait.

```
Tokio:      tokio::task::spawn_blocking(|| machine.process(state, ctx, input))
Rayon:      rayon::scope(|s| s.spawn(|_| machine.process(state, ctx, input)))
Dedicated:  loop { machine.process(state, ctx, input) }
Inline:     machine.process(state, ctx, input)  — zero runtime overhead
```

The same `Machine` implementation, zero modifications.

### When to use which

| Runtime | Good at | Not good at | axiom deployment hint |
|---------|---------|-------------|----------------------|
| **None (inline)** | Single-threaded CPU-bound loops. Zero overhead. | IO, networking, concurrency. | Drop-in Func/Machine with `Inline` links. |
| **Tokio** | Async IO, networking, timers, HTTP, WebSocket. Large ecosystem. | CPU-bound work on async workers (use spawn_blocking). Priority inversion under load. | `Async` for IO machines, `CpuBound` with spawn_blocking for compute. |
| **Rayon** | Data parallelism, work-stealing, CPU-bound batch processing (grid search, parallel backtest). | Async IO, low-latency interactive tasks. Thread pool is designed for throughput, not responsiveness. | `CpuBoundN(n)` with `par_iter()` over Machine instances, not over process() calls. |
| **Embassy** | no_std embedded, async on Cortex-M/RISC-V. Zero-cost async via static allocation. | Heap-heavy workloads, large std dependency. | `Async` with `#![no_std]` — Machine must avoid std types. |
| **Dedicated thread** | Hard real-time, CPU affinity, deterministic latency. | Complex IO multiplexing, large numbers of concurrent connections. | `CpuBound` with core_affinity + pre-allocated scratch. |

### The key constraint

`Machine::process()` takes `&mut State`. This means **a single Machine cannot process multiple inputs in parallel**. Parallelism happens at the Machine-instance level:

```rust
// CORRECT: multiple machines, each on its own thread
let results: Vec<_> = configs.par_iter()
    .map(|cfg| {
        let mut m = MyMachine::init(&ctx).unwrap();
        for input in &inputs { m.process(&mut m, &ctx, input); }
        m
    })
    .collect();

// INCORRECT: same machine, parallel access to state
// my_machine.process(&mut state, &ctx, a)  // &mut is exclusive
// my_machine.process(&mut state, &ctx, b)  // cannot run in parallel
```

`Func` has no such constraint — `Func::call(input)` is a pure function and can be called from any number of threads simultaneously:

```rust
// CORRECT: Func is stateless, safe to parallelize
let results: Vec<_> = inputs.par_iter().map(|x| MyFunc::call(x)).collect();
```

### What about hard real-time?

Hard real-time (deterministic latency, no allocations on the hot path, no lock contention) is **not a runtime question — it is a deployment question**. If a Machine needs hard real-time guarantees:

1. Deploy it as `CpuBound` on a dedicated OS thread
2. Pin that thread to a specific core via `core_affinity`
3. Pre-allocate all working memory in `init()` — `process()` does zero allocations
4. Ensure all channels connected to it use `CasFreeRing` (lock-free) or are pre-sized

These constraints are expressed in `MachinePhysicalSpec` and enforced by the runtime adapter. They do not require changing the Machine implementation.

### What if a Machine is too slow and blocks the pipeline?

That is not an axiom design problem — it is a **topology problem**. If `Machine_A → Machine_B` causes backpressure because `Machine_B` is slow, the fix is in the `LinkSpec`:

```rust
// Option 1: add capacity
LinkKind::BoundedBuf { capacity: 4096, write_policy: Blocking, .. }

// Option 2: decouple with dropping
LinkKind::Channel { capacity: 64, drop_when_full: true }

// Option 3: fan-out to multiple workers
// (deploy two instances of Machine_B behind a load balancer Machine)
```

No Machine code changes. No runtime changes.

### The real-world performance profile

```
betarc 2024 full-year backtest (60M bars):
  Pure synchronous CpuBound:     ~30 seconds (~2M bars/sec)
  With Tokio (spawn_blocking):   ~31 seconds (Tokio adds ~3% overhead)
  With Rayon (parallel months):  ~8 seconds (4 cores, ~4x speedup)
```

Tokio adds 3% overhead when used correctly (spawn_blocking, not async worker). Rayon gives near-linear speedup when you parallelize at the Month level (each month is a separate Machine instance). **Both are transparent to the Machine implementation — only the DeploySpec changes.**

## Tests

```bash
cargo test --lib          # 25 integration tests
```

Tests cover: Func call/composition/generics, Machine lifecycle/three ProcessOutput variants, PortDecl direction/type/schema-version checks, DeploySpec empty/validation, Clock monotonicity/replay, ResourceClass creation.
