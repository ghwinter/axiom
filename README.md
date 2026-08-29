# axiom

**A four-constituent compile-time core: open systems + causal dataflow + composition + staticity declaration.**

Zero-dependency computation primitives for observable, controllable systems.
axiom is a **compile-time model**: blueprints are defined in Rust code/types, and the core's
intelligence is exhausted at compile time for analysis and verification. After compilation it is
equivalent to hand-written plain Rust with zero runtime objects.

## Core constituents (`axiom::cell_core`)

| Constituent | Content | Compile-time property |
|---|---|---|
| **Open system / port cell** `PortCell` | Bounded, typed input/output/state, `step` pure & inline | Type-level, no runtime object |
| **Causal dataflow** `Wire` | `A.out -> B.in`, type-level dual pairing | Illegal wiring fails to compile (T1) |
| **Many-to-many** `Broadcast` (fan-out) / `Merge` (fan-in) | Broadcast, merge, type-level enforced | No Tee tree |
| **Loop** `Feedback` | Causal closure of a loop expressed at type level | Timing belongs to physical carriers (T3) |
| **Composition** `Chain` | A combinator is itself a port cell, nested to any depth | Operad structure |
| **Staticity** `Static` / `Conforms` / `assert_wiring` | Mark zero-cost subgraphs + compile-time wiring verification | Verification at compile time, zero runtime overhead |

**Core promise**:
- Blueprint-as-type: zero-sized, no runtime object (`size_of::<Blueprint<T>>()==0`);
- Verification at compile time, zero runtime overhead;
- After compilation, equivalent to hand-written plain Rust (see `examples/cell_demo.rs`).

**Semantics annotations, not physical mechanisms**: FlowKind (Data/Control/Observe) is an
**optional abstract-layer annotation** describing how the receiver interprets a value — not a
physical-layer property. The physical layer treats all values uniformly as "value-flowing-through-
structure" (shared variable / buffer / channel). Timing/Delay, threading/sync-async, and
value-form/JSON remain physical-layer concerns — see `docs/foundations.md` §5.8.

## runtime (`axiom-semantics`)

runtime is the core's **physical-layer implementation use-case (Carrier)**: for each causal
dataflow it provides replaceable physical options for "how values flow" —
`InlineCarrier` (stack call · zero allocation), `QueueCarrier` / `BoundedCarrier<CAP>`
(heap queue / bounded channel), `spawned_flow` (channel + dedicated thread · cross-thread,
worker panic propagated), and the `wire!` declaration macro. Modular and replaceable:
a new carrier can be plugged in by implementing the `Carrier` trait without changing the topology.

## instances (`axiom-instances` · third constituent)

The **instance layer** plugs replaceable physical/ecosystem implementations into the core
through the seams (`Executor` / `Carrier` / `Telemetry`). Official instances ship as one
fused crate with feature gating, **all off by default**; third parties self-build separate
crates (dual-form boundary — fused standard set vs. open path).

| feature | pulls | provides |
|---|---|---|
| `async` | `axiom-semantics/async-seam` | async seam (the `Executor` contract) |
| `tokio` | `async` + optional `tokio` dep | `TokioExec`: seam wait-point adapter toward tokio |
| `embedded` | `axiom-semantics/std` | reserved embedded flow |

Dependency direction is one-way, enforced by the workspace member table: `axiom ← axiom-semantics ← axiom-instances`. The core and runtime keep their zero-dependency promise; `tokio` lives only as an optional dep of instances.

## Examples

| File | Demonstrates |
|---|---|
| `examples/cell_demo.rs` | A four-constituent blueprint running as a plain Rust program (zero runtime objects) |
| `examples/pipeline.rs` | Composite pipeline: chain + broadcast + feedback + compile-time verification |
| `semantics/examples/carrier_demo.rs` | Same blueprint, multiple replaceable carriers, semantically equivalent, different space–time cost |
| `semantics/examples/threaded_flow.rs` | Same topology, heterogeneous physics: Inline zero-allocation vs cross-thread channel |

## Build & verify

```text
cargo build --workspace                    # core + runtime + instances + 综合用例
cargo test --workspace                     # core + runtime + demos unit/integration
cargo bench -p axiom --bench dag              # diamond zero-cost proof (composite ≈ handwritten, Δ≈±1%) — release-only evidence
cargo build -p axiom-instances --features tokio   # 实例层（tokio feature 门控；默认全关）
cargo test -p axiom-instances --features tokio    # 实例层 + 对照对拍（T6 多物理语义等价）
cargo run -p axiom-demo-sql-over-redis --bin sync_demo          # 综合用例 sync 演示（SQL-over-Redis；零第三方）
cargo run -p axiom-demo-sql-over-redis --features tokio --bin async_demo  # async 变体（真异步馈入驱动 + 观测子系统）
cargo run -p axiom-demo-sql-over-redis --features tokio --bin concurrent_demo  # 并发等待量化（1 线程服务 N 会话）
cargo bench -p axiom-demo-sql-over-redis --features tokio --bench latency     # sync vs async 同口径逐步时延（min-of-N）
cargo run -p axiom --example pipeline              # run an example (core package)
cargo run -p axiom-semantics --example threaded_flow
```

`--workspace` 依根 `Cargo.toml` 的 `[workspace]` 统一解析（收敛旧双 manifest 分治）；单一 `Cargo.lock`/`target`。`no_std` 承诺：`cargo build -p axiom --no-default-features`、`cargo build -p axiom-semantics --no-default-features`（实例层不参与 no_std）。

> Benchmarks are meaningful only in release profiles; under debug builds they
> skip themselves instead of emitting misleading numbers.

## Further reading

- [`docs/`](docs/README.md): formal specification (bilingual, English default) —
  `foundations.md` (definitions/axioms/theorems T1–T9), `core.md` (the compile-time core
  `cell_core`), `semantics.md` (the physical layer / Carrier).
