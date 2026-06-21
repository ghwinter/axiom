# axiom

**Func + Machine: typed ports, explicit topology, deploy-time physics.**
**函数与状态机：类型化端口、显式拓扑、部署时物理决策。**

Zero-dependency computation primitives for observable, controllable systems.
零依赖计算原语，构建可观测、可控制的软件系统。

`Func` (stack, stateless) and `Machine` (heap, stateful) — with typed ports, explicit link
topology, deployment specs, resource classification, and an algebraic foundation.

## What it is

```rust
use axiom::prelude_all::*;

// ── Pure function: stack, stateless, parallel-safe ──
struct Scale;
impl Func for Scale {
    type Input = f64;
    type Output = f64;
    fn name() -> &'static str { "scale" }
    fn call(x: f64) -> f64 { x * 2.0 }
}

// ── Stateful machine: heap, persistent, observable ──
struct Accumulator;
impl Machine for Accumulator {
    type State = f64;
    type Input = f64;
    type Output = f64;
    fn name() -> &'static str { "accumulator" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<f64>("in"))
        .with(PortDecl::output::<f64>("out"))
    }
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

// ── Declare topology ──
let spec = DeploySpec::new()
    .with_machine(MachineInstance {
        name: "acc",
        machine_type: "accumulator",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    });

// Hand to a runtime adapter: axiom_tokio::Runtime::from_deploy(spec)?.run()
```

## What it is NOT

- Not a runtime (no tokio, no executor, no event loop)
- Not a framework (no Application trait, no main() wrapper)
- Not a pure abstraction — it co-defines the physical interface via `MachinePhysicalSpec`

## Built-in modules

`Identity<I>`, `Sink<I>`, `Source<O>`, `Tee<I>`, `Latch<T>`, `Collector<I>`, `EntityRoot`

## Examples

```bash
cargo run --example counter          # Func + Machine, single-threaded
cargo run --example pipeline         # Two Machines chained sequentially
cargo run --example complex_topology # 8 threads, 9 channels, memory persistence
```

## Runtime adapters

| Crate | Runtime | Use case |
|-------|---------|----------|
| `axiom_tokio` | Tokio multi-thread | Production servers |
| `axiom_replay` | Deterministic single-thread | Backtesting, simulation |
| `axiom_embassy` | Embassy async | Embedded, no_std |
| `axiom_linear` | for loop | Bare-metal, testing |

Each adapter interprets `DeploySpec` to construct machines, connect ports, and drive `process()`.

## Tests

```bash
cargo test --lib          # 41 integration tests
```

## Further reading

| Document | What it covers |
|----------|---------------|
| [`docs/foundations.md`](docs/foundations.md) | Algebraic foundation — axioms, theorems, proofs |
| [`docs/philosophy.md`](docs/philosophy.md) | Design philosophy — abstraction vs physics, control/data blur |
| [`docs/architecture.md`](docs/architecture.md) | Architecture details — ports, links, deployment, runtime comparison |

## Why "axiom"

An axiom is a self-evident truth that serves as a foundation. `Func` and `Machine` are the axioms of computation organization. Everything else is derived.
