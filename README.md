# axiom

**Func + Machine: typed ports, explicit topology, deploy-time physics.**
**函数与状态机：类型化端口、显式拓扑、部署时物理决策。**

Zero-dependency computation primitives for observable, controllable systems.
零依赖计算原语，构建可观测、可控制的软件系统。

`Func` (stack, stateless) and `Machine` (heap, stateful) — with typed ports, explicit link
topology, deployment specs, resource classification, and an algebraic foundation.
配套 `axiom-runtime`：把 `DeploySpec` 蓝图施工为可运行系统（单线程/多线程、融合、IO 多路复用）。

## What it is

```rust
use axiom::declare_ports;
use axiom::func::Func;
use axiom::machine::{CleanupError, InitError, Machine, SingleOutput};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::resource::MachinePhysicalSpec;
use axiom::deploy::{DeploySpec, MachineInstance};

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

// ── Declare topology (DeploySpec) ──
let spec = DeploySpec::new()
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

> **模型优先：DeploySpec 是任意图；static_path 是固定形状优化子集。**
> axiom 的默认模型是 **任意有向图**（多进多出、fan-in、fan-out、环、复合嵌套）——
> 用 `DeploySpec` 声明、`validate_deep` 验证、runtime 执行。`static_path` 是
> **性能优化子集**：只覆盖编译期形状已知的拓扑（线性/扇形），因为它靠类型
> 展开（单态化）消解开销——任意图（尤其环）无法单态化，必须走动态路径。
> 线性不是 axiom 的场景假设；它是"类型展开"这一优化手段的固有边界。

| Path | Topology shape | Topology known at | Per-message cost | Zero-cost? | Role |
|------|---------------|-------------------|------------------|------------|------|
| **DeploySpec + Runtime**（主模型） | **任意图**（环、fan-in/out、复合） | runtime | bounded (heap alloc + dispatch) | no（动态税不可避免） | 通用执行：复杂图系统 |
| **static_path**（优化子集） | 固定形状（线性/扇形/菱形） | compile time | **zero** | yes | 热路径：编译期已知形状 |

The static path monomorphizes over concrete machine types and inlines `Link::extract` / `Split::split` / `Merge::merge` — in release the compiled code is equivalent to hand-writing the batch loop directly. The dynamic path must type-erase via `Box<dyn Any>` because topology is not known until runtime; this "dynamic tax" is mathematically unavoidable, not an implementation defect. **Neither path imposes a linear assumption on the model** — an arbitrary graph runs on the dynamic path; only the optimization (monomorphization) is shape-restricted.

> **Scope note (anti-narrowing rule).** The static execution path
> (`axiom_runtime::static_path`) supports linear pipelines (`pipeline2`/`pipeline3`),
> fan-out (`fanout2` via `Split`), and fan-in (`fanin2` via `Merge`). It is acyclic
> (synchronous batch model); diamond topologies require composing `fanout2` +
> `fanin2` manually. A `dag` combinator for arbitrary DAGs is future work. See
> `docs/philosophy.md` §"The structural scope constraint" and `docs/architecture.md`
> §"Static execution path" for details.

**Empirical validation** (100k-message Transform → Sink pipeline, release build, single reference environment):

| Implementation | Relative throughput | vs hand-written |
|----------------|-------------------:|----------------:|
| Hand-written (adapter task) | 1.0× | baseline |
| Static path (monomorphized) | **1.24×** | faster |
| Dynamic path (type-erased) | 0.20× | slower |

*Relative ratios (not absolute throughput): absolute numbers vary by machine/allocator; the ordering static > hand-written > dynamic is environment-independent.*

The static path not only matches but **exceeds** hand-written — the abstraction lets the compiler see structure that hand-written code hides, enabling it to eliminate an intermediate task. See [`docs/philosophy.md`](docs/philosophy.md) and [`docs/foundations.md` §15](docs/foundations.md#15-零成本抽象抽象层与物理层的解耦) for the formal treatment.

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

## Built-in modules

`Identity<I>`, `Sink<I>`, `Source<O>`, `Tee<I>`, `Latch<T>`, `Collector<I>`, `EntityRoot`

## Advanced features

| Feature | Module | Description |
|---------|--------|-------------|
| **Session Types** | `axiom::session` | Binary + Multiparty (MPST) protocols with `GlobalType`/`LocalType` projection, `is_dual`, `is_consistent` |
| **Streaming** | `axiom::stream` | `StreamingMachine`: pull-model iterator output (first `next()` resets cursor) |
| **Borrowed Input** | `axiom::func` | `FuncRef::call_ref`: zero-copy input (no per-call allocation) |
| **Static Execution** | `axiom::static_exec` | `Link`/`Split`/`Merge` type contracts (FusedInline-gated) |
| **Dynamic Topology** | `axiom::topology` | Optional runtime mutation of the *instance* graph (elastic scaling, hot-swap, session subgraphs) |
| **Hybrid Systems** | `axiom::hybrid` | Continuous dynamics via `HybridMachine` (`flow`/`guard`/`reset`) with `TimeTick` integration |
| **Lifecycle Typestate** | `axiom::machine` | Compile-time enforcement of `Init → Running → Stopping → Stopped` via `MachineHandle<M, S>` |
| **Composite Machines** | `axiom::composite` | `CompositeSpec` + `expand_composites`: subsystem nesting (recursive, depth-limited) |

### Static-first worldview

axiom's default is a **static topology**: the `DeploySpec` is declared once,
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

`Runtime` executes a `DeploySpec` with explicit physics:

- **Execution modes**: `Inline` / `Sequential` (BFS direct delivery) / `Parallel(n)` (thread-per-machine, channel carriers)
- **Carrier matrix**: `Blocking` (backpressure) / `Dropping` (drop new) / `Overwriting` (ring) / `Latest`-`SharedState` (single slot) — the *physical realization* of a `LinkKind`
- **Lifecycle**: `Done` is a stop signal — propagates downstream (cascade shutdown), backlog dropped; parallel threads exit
- **Fusion**: pipelineN fusion over `FusedInline` chains (allocations per hop reduced)
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
| `graph_validation` | **复杂图验证与分析**：内核风格图（syscall 扇出 + 双路径 + 3 反馈环 + 观测）通过 `validate_deep`；逐项检出流类型不匹配 / Inline 环 / 全非 Moore 环；SPOF / 环 / 度 / 可达性分析报告 |

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

- `axiom` core: **291 tests** (174 src unit + 117 integration, 10 suites) — all green
- `axiom-runtime`: **64 tests** — all green
- Verification philosophy: evidence corpus `evidence/` (E-contracts + R-benchmarks, local-only, not in git)

## Further reading

| Document | What it covers |
|----------|---------------|
| [`docs/foundations.md`](docs/foundations.md) | Algebraic foundation — axioms, theorems, proofs |
| [`docs/philosophy.md`](docs/philosophy.md) | Design philosophy — abstraction vs physics, control/data blur |
| [`docs/architecture.md`](docs/architecture.md) | Architecture details — ports, links, deployment, runtime comparison |
| [`docs/architecture_diagrams.md`](docs/architecture_diagrams.md) | Diagrams — system layers, link strategies, deployment, roadmap |

## Why "axiom"

An axiom is a self-evident truth that serves as a foundation. `Func` and `Machine` are the axioms of computation organization. Everything else is derived.
