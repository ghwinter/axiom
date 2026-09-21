# axiom

**A four-constituent compile-time core: open systems + causal dataflow + composition + staticity declaration.**

[English](README.md) · [简体中文](README.zh-CN.md)

> **New here? Start with the [primer](docs/en-us/primer.md) (bilingual:
> [en](docs/en-us/primer.md) / [zh-cn](docs/zh-cn/primer.md))** — what axiom is, what it is
> *not*, the vision in a few lines, and how to think without a framework. Three sentences:
> (1) you write *what connects to what* as **types** — illegal wiring fails to compile;
> (2) you declare *who must do what* (deadlines / cancellation / backpressure) as **declared
> items**, not comments; (3) you choose *how waiting and I/O are implemented* by **swapping a
> carrier** — the topology does not change. axiom is a **constitution layer, not a framework**:
> topology safety + explicit obligations + physical replaceability, without giving you the system.

axiom is a **constitution layer, not a framework**: it supplies the typed vocabulary
(shape, contracts, obligation modalities), the compile-time verification, and the
replaceable-physics seams — not an application, not an all-in-one runtime, no control
inversion, no lifecycle ownership. Rust gives memory safety without giving you the
application; axiom gives topology safety, explicit obligations, and physical
replaceability without giving you the system.

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
| **Composition** `Chain` | A combinator is itself a port cell, nested to any depth | A compositional structure (geometric characterization) |
| **Staticity** `Static` / `Conforms` / `assert_wiring` | Mark zero-cost subgraphs + compile-time wiring verification | Verification at compile time, zero runtime overhead |

**Core promise**:
- Blueprint-as-type: zero-sized, no runtime object (`size_of::<Blueprint<T>>()==0`);
- Verification at compile time, zero runtime overhead;
- After compilation, equivalent to hand-written plain Rust (see `core/examples/cell_demo.rs`).

**Semantics annotations, not physical mechanisms**: FlowKind (Data/Control/Observe) is an
optional abstract-layer annotation describing how the receiver interprets a value — not a
physical-layer property. The physical layer treats all values uniformly as "value-flowing-through-
structure" (shared variable / buffer / channel). Timing/Delay, threading/sync-async, and
value-form/JSON remain physical-layer concerns — see `docs/en-us/foundations.md` §5.8.

## semantics (`axiom-semantics`)

semantics is the **contract layer** (the semantics functor ⟦core shape category⟧ → behavior
category): for each causal dataflow it declares the behavior and space–time cost contracts —
the three wait-point contracts (input-ready / deadline / backpressure) plus the activation
contract, and the carrier sockets. In-tree carriers: `InlineCarrier` (stack call · zero
allocation), `QueueCarrier` / `BoundedCarrier<CAP>` (heap queue / bounded channel),
`spawned_flow` (channel + dedicated thread · cross-thread, worker panic propagated), and the
`wire!` declaration macro. Modular and replaceable: a new carrier plugs in by implementing
the `Carrier` trait without changing the topology; real bases (tokio/io_uring/std/embedded)
are bound and fulfilled by the instance layer.

## The four seams and the gap ledger

The runtime boundary is a **four-edged seam map**, not a three-way information split.
Data/Control/Observe are classifications of *seam direction*, not of information —
the physical edge proves it: it carries no information at all (an allocator is matter).

| Edge | Seam | Surface |
|---|---|---|
| Data | `Wire` (core) / `Carrier` (semantics) | In/Out/State |
| Control | `Executor` (`async-seam`) | wait-point contracts |
| Observation | `Telemetry` (`telemetry`) | verdicts per seam |
| **Physical** | `seams::physical` (`PhysicalWindow` / `PhysicalSnapshot`) | **no surface** — process-singleton window (allocator / signals / stdio), declared + observed only, never an In/Out/State |

The **crosscut** seam (`seams::crosscut`, the `CrossCut` marker) is the one variety with no
surface by design: it rides the call flow and belongs to no module (contexts, cancellation
tokens). It is not a cell and never enters `assert_wiring`.

The **gap ledger** (`checks::friction`) is a governance artifact, not a vocabulary extension:
the six frictions the algebra cannot express (waiting / physical singleton / crosscutting /
residency / derived semantics / completeness) are typed, indexed to their containment seams,
and left **open** (`#[non_exhaustive]`) — completeness cannot be self-proven inside the law,
so a new gap is *added to the ledger*, not pretended away.

## instances (`axiom-instances` · third constituent)

The **instance layer** plugs replaceable physical/ecosystem implementations into the core
through the seams (`Executor` / `Carrier` / `Telemetry`). Official instances ship as one
fused crate with feature gating, all off by default; third parties self-build separate
crates (dual-form boundary — fused standard set vs. open path).

| feature | pulls | provides |
|---|---|---|
| `async` | `axiom-semantics/async-seam` | async seam (the `Executor` contract) |
| `tokio` | `async` + optional `tokio` dep | `TokioExec`: seam wait-point adapter toward tokio |
| `embedded` | `axiom-semantics/std` | reserved embedded flow |

Dependency direction is one-way, enforced by the workspace member table: `axiom ← axiom-semantics ← axiom-instances`. The core keeps its zero-dependency promise; the semantics layer adds exactly one compile-time-only dependency (the `axiom-macros` proc-macro crate, which links nothing at runtime); `tokio` lives only as an optional dep of instances.

## Examples

> Index of the contract-layer examples: [`semantics/examples/README.md`](semantics/examples/README.md)
> (roles, scales, and the relationship to the composite use case). All semantics examples
> run with `--manifest-path semantics/Cargo.toml`.

| File | Demonstrates |
|---|---|
| `core/examples/cell_demo.rs` | A four-constituent blueprint running as a plain Rust program (zero runtime objects) |
| `core/examples/pipeline.rs` | Composite pipeline: chain + broadcast + feedback + compile-time verification |
| `core/examples/deep_blueprint.rs` | Blueprint depth × compile cost honest probe (adoption evidence: compile time grows with depth, runtime stays zero) |
| `semantics/examples/closed.rs` | Closed-boundary integration (foundations §8): composition self-closure, failure-as-value, T6, runtime slot swapping |
| `semantics/examples/carrier_demo.rs` | Same blueprint, multiple replaceable carriers, semantically equivalent, different space–time cost |
| `semantics/examples/threaded_flow.rs` | Same topology, heterogeneous physics: Inline zero-allocation vs cross-thread channel |
| `semantics/examples/psql/` | SQL REPL: single-layer `TryChain` short-circuit (lexer/parser/exec errors as values) |
| `semantics/examples/netpath/` | Network receive path: Eth→IP→TCP parsing, dual-carrier short-circuit equivalence (T6) |
| `semantics/examples/mmo/` | Multi-player world core subgraph: events→world→view projection→data-driven fan-out |
| `semantics/examples/redis_like/` | miniredis hardening: closed-composition single port body + three physical drivers + runtime swapping |
| `semantics/examples/sqlmini/` | SQL subset compile chain (lex→parse→plan→exec) + Inline/partition-parallel dual-physics equivalence (largest example) |
| `semantics/examples/layered/` | Three-role layering: library author / topology integrator / deployer (contract = `PortCell` types) |
| `semantics/examples/control_seam/` | Control & observation conformance: control as values + ops control surface + observation triad |
| `instances/examples/tokio_timeout_async.rs` | True-async timeout with tokio: await-driven polling on a multi-thread reactor (requires `--features tokio`) |
| `examples/sql-over-redis/` | Composite use case: redis protocol face × psql compute face in one composite core (sync/async concurrent demos) |

## Benchmarks

Benches are the **empirical evidence** layer (zero-cost proof / regression gates); examples
are the teaching/paradigm-evidence layer. All benches are release-only (they skip
themselves under debug builds).

| Bench | Answers |
|---|---|
| `core/benches/chain.rs` | Composition zero-cost: generic drive ≈ handwritten `step` chain (bit-exact) |
| `core/benches/dag.rs` | Diamond (serial-then-parallel) generic vs handwritten vs type-erased |
| `semantics/benches/carrier.rs` | Carrier space–time cost: Inline zero-allocation vs spawned cross-thread channel |
| `semantics/benches/dynamic_tax.rs` | Dynamic tax: type-slot / seat-erasure cost per drive, swap cost |
| `semantics/benches/profile_workloads.rs` | Shape workload profiles (min-of-N methodology) |
| `examples/sql-over-redis/benches/latency.rs` | Composite use-case latency: sync vs async step-by-step, same protocol (requires `--features tokio`) |

## Build & verify

```text
cargo build --workspace                    # core + semantics + instances + use-case crates
cargo test --workspace                     # core + semantics + demos unit/integration
cargo bench -p axiom --bench dag              # diamond zero-cost proof (composite ≈ handwritten, Δ≈±1%) — release-only evidence
cargo build -p axiom-instances --features tokio   # instance layer (tokio feature-gated; all off by default)
cargo test -p axiom-instances --features tokio    # instance layer + equivalence cross-check (T6 multi-physics semantic equivalence)
cargo run -p axiom-demo-sql-over-redis --bin sync_demo          # comprehensive use case, sync demo (SQL-over-Redis; zero third-party)
cargo run -p axiom-demo-sql-over-redis --features tokio --bin async_demo  # async variant (true async feeding driver + observation subsystem)
cargo run -p axiom-demo-sql-over-redis --features tokio --bin concurrent_demo  # concurrent waiting quantified (1 thread serving N sessions)
cargo bench -p axiom-demo-sql-over-redis --features tokio --bench latency     # sync vs async step-by-step latency, same protocol (min-of-N)
cargo run -p axiom --example pipeline              # run an example (core package)
cargo run -p axiom-semantics --example threaded_flow
```

`--workspace` resolves uniformly via the root `Cargo.toml` `[workspace]` (consolidating the old dual-manifest split); one `Cargo.lock`/`target`. The `no_std` promise: `cargo build -p axiom --no-default-features`, `cargo build -p axiom-semantics --no-default-features` (the instance layer does not participate in no_std).

> Benchmarks are meaningful only in release profiles; under debug builds they
> skip themselves instead of emitting misleading numbers.

## Further reading

- [`docs/`](docs/README.md): formal specification (bilingual, English default) —
  `foundations.md` (definitions/axioms/theorems T1–T9), `core.md` (the compile-time core
  `cell_core`), `semantics.md` (the physical layer / Carrier).
