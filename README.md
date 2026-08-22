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
| **Causal dataflow** `Link` | `A.out -> B.in`, type-level dual pairing | Illegal wiring fails to compile (T1) |
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

## runtime (`axiom-runtime`)

runtime is the core's **physical-layer implementation use-case (Carrier)**: for each causal
dataflow it provides replaceable physical options for "how values flow" —
`InlineCarrier` (stack call · zero allocation), `QueueCarrier` / `ChannelCarrier` /
`spawned_flow` (heap queue/channel · cross-thread), `DirectCarrier` / `static_path`
(compile-time expansion), and the `wire!` declaration macro. Modular and replaceable:
a new carrier can be plugged in by implementing the `Carrier` trait without changing the topology.

## Examples

| File | Demonstrates |
|---|---|
| `examples/cell_demo.rs` | A four-constituent blueprint running as a plain Rust program (zero runtime objects) |
| `examples/pipeline.rs` | Composite pipeline: chain + broadcast + feedback + compile-time verification |
| `runtime/examples/carrier_demo.rs` | Same blueprint, multiple replaceable carriers, semantically equivalent, different space–time cost |
| `runtime/examples/threaded_flow.rs` | Same topology, heterogeneous physics: Inline zero-allocation vs cross-thread channel |

## Build & verify

```text
cargo build --lib        # core (zero dependency; no_std via --no-default-features)
cargo test --lib         # 18 tests
cargo build/test --manifest-path runtime/Cargo.toml   # runtime (17 tests)
cargo run --example pipeline          # run an example
cargo run --manifest-path runtime/Cargo.toml --example threaded_flow
```

## Further reading

- [`docs/`](docs/README.md): formal specification (bilingual, English default) —
  `foundations.md` (definitions/axioms/theorems T1–T9), `core.md` (the compile-time core
  `cell_core`), `runtime.md` (the physical layer / Carrier).
