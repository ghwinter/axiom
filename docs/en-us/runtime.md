> **Language:** English · [中文版](../zh-cn/runtime.md)

# axiom runtime: Physical-Layer Implementation Use Case (the Carrier)

> **Nature**: the **physical-layer architecture specification** of axiom. Answers
> "what axiom's physical layer is": the core `cell_core` only declares
> **causal data flows** (`A.out -> B.in`), and the runtime answers the single question —
> **how the value of this flow gets from `A.out` to `B.in`, at what space–time cost**. This
> volume describes the shape of the runtime, consistent with the converged
> implementation (`runtime/src/{carrier,flow,static_path,macros,lib}.rs`).
>
> **Normativity**: a self-consistent authoritative specification, focused
> on the shape of axiom's physical layer itself.
>
> **Positioning (in one sentence)**: runtime = **Carrier catalog + redemption verification**:
> a physical implementation (how the value moves) for each causal data flow of
> `cell_core`, each embodying a different space–time cost, modular and replaceable. The
> runtime is the core's **physical-layer implementation use case** — axiom has no runtime
> objects, only two phases: "compile time" and "after compilation".

---

## 1. Conceptual Foundation (Derived from cell_core)

- `cell_core`: open systems (`PortCell`: In/Out/State/step) + causal flows (`Wire`/`Chain`/
  `Broadcast`/`Merge`/`Feedback`) + staticness (`Static`) + compile-time verification (unified `Conforms`).
- A blueprint is a type: zero size, zero runtime objects, exhausted at compile time.
- **The runtime does not re-declare the core** — the runtime only answers "for this causal flow, how does the
  value get from A.out to B.in".

---

## 2. The Core Abstraction: `Carrier`

```rust
pub enum CarrierCost { ZeroAllocInline, PerMessageAlloc, External }   // space–time cost declaration

pub trait Carrier<A, B>
where A: PortCell, B: PortCell<In = A::Out>,   // T1: the causal flow itself is legal
{
    fn cost() -> CarrierCost { CarrierCost::ZeroAllocInline }
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out;
}
```

The carrier catalog (`runtime/src/carrier.rs`):

| Carrier | Physical scheme | Space–time cost | Threading | Module |
|---|---|---|---|---|
| `InlineCarrier` | Direct pass on the stack (`B::step(A::step(x))`); compile-time expansion (`Direct` merged into it) | Zero allocation, inlined | Single-threaded | carrier.rs |
| `QueueCarrier` (std) | Heap-queue relay (`Box<dyn Any>`, allocated per message) | Per-message allocation | Within a single thread | carrier.rs |
| `BoundedCarrier<CAP>` (std) | Bounded-channel relay (`CAP ≥ 1` enforced at compile time) | Per-message allocation | Within a single thread | carrier.rs |
| `spawned_flow` (std) | mpsc channel + dedicated thread, `B::State` on the dedicated thread; worker panic propagates via reply channel | Per-message allocation + synchronization | **Cross-thread** | carrier.rs |

Each carrier is **independently selectable and replaceable**: swapping one implementation
does not change the topology (T6, multiple physical implementations).

> **Placement continuum (linking to `foundations.md` §8.6 items 7–8)**: "single-threaded /
> cross-thread" in the table are **not two models, but the two ends of one physical-placement
> decision spectrum** — the same blueprint, via placement, decides where each edge sits on the
> spectrum. The carriers in the table are the physical forms at different positions on the
> spectrum: single-threaded carriers = the native form of "all edges placed on the same thread"
> (family A = 0); cross-thread carriers honestly bear family A (concurrency-maintenance toll).
> The zero-cost promise (family B = 0, see below) holds equally for both.

> **Carrier as attribute (deployment-time physicality)**: the blueprint declares "which
> carrier this flow uses" (e.g. `Static<Chain<A,B>>` goes through
> `InlineCarrier`/`static_path`), and the runtime redeems it per the declaration.
> "Drop/block/synchronize/asynchronize" are all physical-layer choices (linking to
> `foundations.md` §5.8) — swapping the carrier for the same blueprint changes the
> "drop/block/synchronize" behavior.

---

## 3. Driving (flow) and Static Path (static_path)

- **`flow`** (`runtime/src/flow.rs`): `drive_link` — after compile-time
  wiring verification (unified `Conforms<Wire>`), drives one A→B causal flow with the selected carrier;
  **verification happens at compile time, zero runtime overhead**. (`drive_wired` removed as a
  redundant alias of `drive_link`.)
- **`static_path`** (`runtime/src/static_path.rs`): `run_static` / `run_declared_static` —
  inline-expands at **compile time** the subgraph declared by `Static<SUB>` as "requiring
  zero cost" (zero runtime objects).
- **Declaration macros** (`runtime/src/macros.rs`): `wire!` — a macro/compile-time technique
  that completes "wiring + carrier + verification" in one step at compile time.

---

## 3b. Unified-model activation (runtime, `std`)

The runtime gives *activation* to the unified-model constructs (which are **definitions** in
`core.md`; activation stays the run/carrier side):

- **`SlotDrive<I, O>`** — *existential binding* (`runtime/src/slot.rs`) — the ∃ existential fill of a `Slot<I,O>`:
  install a compile-time-conforming inhabitant (`T: PortCell<In=I,Out=O>` ⟹ core `Conforms`),
  type-erase its state to `Box<dyn Any + Send>`, and `drive`/`swap` it at runtime. This is the
  physical side of "dynamic loading" — interface fixed & T1-verified at compile time, inhabitant
  existentially chosen at runtime.
- **`drive_seq`** (`runtime/src/flow.rs`, `std`) — the generative/unbounded-count side of
  `Rep<N,C>`: drive a runtime `IntoIterator` sequence of inputs through one cell, collecting
  outputs, with state held across steps (count decided at runtime, not compile time).

Both are `std`-gated and safe (`#![forbid(unsafe_code)]`); they localize the dynamic tax to the
seam (see [`unified.md`](unified.md) §5).

## 4. Modularity and Replaceability (Extensible Physical Carriers)

- Each carrier is an **independent unit** that can be a standalone crate.
- A new physical carrier is attached by implementing the `Carrier` trait **without changing
  the cell topology**: for example, replacing the queue/channel-form carrier with a channel
  carrier carrying other scheduling/timing semantics, or replacing the zero-allocation carrier
  with other
  low-level mechanisms.
- As a reference implementation use case, the runtime provides each carrier as a template.

---

## 5. Semantic Equivalence and Verification (the Contract of Replaceability)

- **Semantic equivalence of multiple physical implementations** (T6): the `flow` of any
  carrier is semantically equivalent to `B::step(sb, A::step(sa, x))` — i.e. the same causal
  flow. Swapping carriers does not change the output.
- **Determinism and equivalence acceptance**: the real use cases (see §7) produce bit-identical
  output on carriers such as Inline and Queue, and verify determinism (same input rerun yields
  the same output). This should serve as the **carrier semantic-equivalence regression
  acceptance** — whenever a new carrier is added, it must first assert output consistency on the
  existing use cases, to prevent a carrier from silently breaking semantics (this is an
  engineering convention derived from netpath practice).
- **Compile-time vs runtime verification**: wiring legality and staticness verification happen
  at compile time (T1/unified `Conforms`); the carrier's space–time cost is an optional quantitative
  declaration (`CarrierCost`), not a performance promise.

---

## 6. Build and Acceptance Baseline

```text
cargo build/test --manifest-path runtime/Cargo.toml   # runtime (7 tests)
cargo run --manifest-path runtime/Cargo.toml --example carrier_demo
cargo run --manifest-path runtime/Cargo.toml --example threaded_flow
cargo bench --manifest-path runtime/Cargo.toml --bench carrier
```

**Accomplished (chain of evidence)**:
- The runtime depends only on cell_core (the new core), not on any v0 module.
- Carrier catalog: Inline (stack pass · zero allocation) / Queue (heap-queue relay) / Channel
  (cross-thread mpsc) / Direct+static_path (compile-time expansion) / wire! (declaration macro).
- Modular and replaceable: swapping a carrier does not change the topology (T6), and each
  carrier is independent and can be referenced on its own.
- `#![forbid(unsafe_code)]`, no_std (the `std` feature gates the non-zero-allocation /
  cross-thread carriers).
- bench: Inline 2.7µs vs spawned_flow 6.96s — the space–time cost difference of the same causal
  flow under single-thread inline and cross-thread channel carriers, empirically showing
  "different carriers = different space–time costs".

---

## 7. Real Use Cases (the Runtime as the Core's Implementation Use Case)

| Use case | Type | Runtime capability exercised |
|---|---|---|
| `redis_like` | Multi-module server class | Multi-module pipeline + single-thread/cross-thread (`spawned_flow`) |
| `psql` | Parse/execute pipeline class | Pipeline composition + Inline/cross-thread parsing |
| `mmo` | Multiplayer world/broadcast | Broadcast many-to-many fan-out + world state + view projection |
| `netpath` | Multi-segment network pipeline | Multi-segment parse composition + Queue vs Inline carrier equivalence + determinism |
| `carrier_demo` | Carrier demonstration | Multiple carriers replaceable for the same blueprint, semantic equivalence, differing space–time costs |
| `threaded_flow` | Heterogeneous physicality on the same topology | Inline zero allocation vs cross-thread channels |

These use cases are build use cases for "building real programs on axiom/axiom-runtime", and
also the carriers for runtime iteration and equivalence verification. Legacy counterparts of
these use cases (including TCP-server-shaped ones) can be recovered from git history for
reference (`git show main:runtime/examples/<name>/main.rs`).

---

## 8. Boundaries (Honest Statement)

- This is a **physical-layer implementation use case + template**, not "the most feature-complete
  general-purpose runtime".
- **N2N belongs to the physical layer**: many-to-many parallel scheduling, queue arbitration,
  borrowing, caching, and threading belong to the physical-implementation domain — axiom does
  not reinvent them; axiom provides the "carrier contract" (declaring preferences) and
  "redemption verification", so that many-to-many implementations can be attached replaceably.
- **The dynamic tax is unavoidable and legitimate**: the tax is paid if and only if the structure
  must be determined at runtime (configuration/plugins/dynamic topology) (linking to T7/T9);
  otherwise the static path must be taken.
- **Scope of the zero-cost promise**: what is promised is **family B = zero** (the abstraction
  does not charge for distinction demands); the **family A** of cross-thread edges
  (synchronization/wakeup/visibility) is a physical toll that an equivalent hand-written
  multi-threaded program pays as well, and is not an "abstraction tax" (linking to
  `foundations.md` §8.6 item 8).

---

## 9. Known Open Boundaries (Honest Record)

> The following are **thin edges** within the runtime's positioning, exposed by the real use
> cases, currently **unresolved but faithfully recorded**. They belong to "engineering
> accretion/optimization + a handful of theoretical boundaries" and do not change the existing
> composition of the core (`cell_core`).

### 9.1 Backpressure / Bounded Buffering
The unbounded mpsc form (queue/thread transport) is covered by `QueueCarrier`/`spawned_flow`
(the latter is **unbounded**); real systems need bounded + backpressure semantics for
"producer fast, consumer slow".
- Layer: **purely runtime** (`foundations.md` has already placed "backpressure/timing" under
  physical carriers).
- **Provided**:
  - `BoundedQueue<T, CAP>` (`buffer.rs`, `std`) — a bounded FIFO built on `sync_channel(CAP)`:
    `push` (blocking = backpressure), `try_push` (returns `Err` when full = capacity/backpressure
    signal; the drop/propagate policy is left to the caller);
  - `BoundedCarrier<CAP>` (`carrier.rs`) — a `Carrier` whose physical form is a bounded channel
    (`CAP` as a compile-time constant; `PerMessageAlloc` cost);
  - `bounded_pump<A, B, It, CAP>` (`flow.rs`) — **real blocking backpressure**: the producer
    pushes `A`'s output into a capacity-`CAP` bounded queue and *blocks when full* until the
    consumer thread drains; returns the `B::step` output sequence.

### 9.2 Error / Failure Pathways (Theory–Practice Divergence)
Real cells (e.g. parsers) need "can fail" semantics; `PortCell::step` is assumed to be a
**total transition** (the `foundations.md` boundary has already faithfully noted this: the
total-function assumption). Currently patched together with the `Out = Result` convention plus
short-circuit carriers.
- Layer: **attributable to the runtime** (using the `Result` convention + short-circuit),
  isomorphic to "drop/block is physical"; if "cells that can fail" were axiomatized
  (partial functions/error output ports), it would be a theoretical boundary (`foundations.md`
  §7, open question 5).
- **Provided (short-circuit side)**:
  - `drive_try<A, B, X, E>` — connects a `Result`-producing `A` to a `B` consuming its `Ok`,
    short-circuiting immediately on `Err` (`no_std`-safe);
  - `TryChain<A, B>` — a composable short-circuit chain of two fallible cells: the whole fallible
    pipeline is a single-level-`Result` `PortCell` (cleaner than `drive_try`'s nested `Out`;
    psql expresses its full REPL as `TryChain<TryChain<Lexer, Parser>, Executor>`).
- Remaining: first-class short-circuit carriers (e.g. `MaybeCarrier`/`ResultCarrier`), and the
  combined failure × backpressure semantics.

### 9.3 The Seam for Attaching External Input Sources (IO Events ↔ flow)
The documentation has declared "IO is physical/carrier-replaceable", but the grounding
interface for "how the external world (socket events, etc.) formally becomes the `in` of a
causal flow" has not been formalized. Upgrading redis_like to real TCP (listen/accept/
frame-based parsing) is the first implementation use case for this seam.
- Layer: **runtime** (an event substrate is just a class of carrier/driver).
- Direction: distill an event substrate (event-substrate), so that external events become a
  replaceable class of input carrier/driver.

### 9.4 Failure × Backpressure Occurring Simultaneously
The semantics when the buffer is full and processing fails at the same time are not specified —
left to converge together once the two above are settled.

---

## 10. Conclusion

> runtime = the `cell_core` **physical-layer implementation use case**: a carrier catalog
> (Inline/Queue/Channel/Direct/static_path/wire!) + redemption verification, modular and
> replaceable, explaining and verifying that "the same static graph can be plugged into
> multiple physical executions (inline/queue/cross-thread), each with verifiable semantic
> equivalence". Carriers can be attached by implementing the `Carrier` trait without changing
> the topology, giving the physical layer extensibility; the open questions within its
> boundaries (backpressure, error pathways, IO seams) have been faithfully recorded as driver
> input for subsequent iteration.
