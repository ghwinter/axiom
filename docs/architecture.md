# axiom 架构参考

> 本文档描述了 axiom 的架构组件。如果你在找快速入门，回到 README。

---

## Two computational primitives

| Primitive | Memory | State | Observable | Controllable | Connection |
|-----------|--------|-------|------------|--------------|------------|
| `Func`    | Stack  | None  | No         | No           | Inline call |
| `Machine` | Heap   | `S`   | Yes        | Yes          | Ports (BoundedBuf / Channel / Latest / CasFreeRing / SharedState) |

`Func(I) -> O` — a pure function. Stack frame, instant, unobservable. The same input always produces the same output. Used for parsing, serialization, mathematical transforms.

`Machine(S, I, O, δ)` — a state machine. Heap-allocated State lives across repeated process() calls. Observable via ctx.snapshot() and FlowKind::Observe ports. Configurable via ConfigCell entries registered during init.

The IO-Object is exactly `(S, I, O, δ)` — no more, no less. Observe and Control are port annotations (`FlowKind`), not type parameters.

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

## Built-in modules

| Module | Signature | Purpose |
|--------|-----------|---------|
| `Identity<I>` | `I → I` | Category identity morphism, fills gaps |
| `Sink<I>` | `I → ∅` | Discards input, terminates pipelines |
| `Source<O>` | `∅ → O` | Constant output, useful for testing |
| `Tee<I>` | `I → (I, I)` | Fan-out broadcast |
| `Latch<T>` | `T → T` | Holds last received value |
| `Collector<I>` | `I → ∅` | Accumulates in State, exposes via observe port |
| `EntityRoot` | `∅` | System root — exists, does nothing |

```rust
use axiom::builtin::Identity;
```
