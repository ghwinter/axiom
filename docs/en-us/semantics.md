> **Language:** English · [中文版](../zh-cn/semantics.md)

# axiom-semantics (formerly axiom-runtime): the semantics / contract layer

> **Status**: renamed on the experimental branch `rework/rename-runtime-semantics` as the
> first step of the runtime → **semantics** repositioning (runtime = the semantics functor
> ⟦core shape category⟧ → behavior category). This page's full prose re-framing — moving
> from the now-inaccurate "physical layer" framing to the semantics/契约 layer framing
> (physical/binding realizations belong to `axiom-instances`) — is a follow-up pass, not yet
> written. Mechanical rename (dir/package/crate/doc paths) is complete and test-green.
>
> **Nature** (legacy framing, being revised): the **physical-layer architecture specification**
> of axiom. Answers "what axiom's physical layer is": the core `cell_core` only declares
> **causal data flows** (`A.out -> B.in`), and the runtime answers the single question —
> **how the value of this flow gets from `A.out` to `B.in`, at what space–time cost**. This
> volume describes the shape of the runtime, consistent with the converged
> implementation (layered: `semantics/src/{checks,movers,seams,drive}/*.rs`).
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
    fn cost() -> CarrierCost { CarrierCost::External }
    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out;
}
```

The carrier catalog (`semantics/src/movers/carrier.rs`):

| Carrier | Physical scheme | Space–time cost | Threading | Module |
|---|---|---|---|---|
| `InlineCarrier` | Direct pass on the stack (`B::step(A::step(x))`); compile-time expansion (`Direct` merged into it) | Zero allocation, inlined | Single-threaded | carrier.rs |
| `QueueCarrier` (std) | Heap-queue relay (`Box<dyn Any>`, allocated per message) | Per-message allocation | Within a single thread | carrier.rs |
| `BoundedCarrier<CAP>` (std) | Bounded-channel relay (`CAP ≥ 1` enforced at compile time) | Per-message allocation | Within a single thread | carrier.rs |
| `spawned_flow` (std) | mpsc channel + dedicated thread, `B::State` on the dedicated thread; worker panic propagates via reply channel | Per-message allocation + synchronization | **Cross-thread** | carrier.rs |

Storage primitive (not a `Carrier`; the bounded FIFO beneath pumps/mailboxes):
`ring::BoundedRing<T, CAP>` — no_std+alloc, dual counters (`readable`/`writable`),
O(1) push/pop with **typed** `Full(v)`/`Empty` verdicts (value conservation), one reserve
allocation at construction and zero per-message allocation in steady state. Single-threaded
by contract; a cross-thread variant awaits the critical-section decision. Serves the
`EmbeddedProfile` (zero-alloc steady-state budget).

> **Bounded-FIFO disambiguation (2026-08, external-audit reconciliation).** Three bounded
> FIFOs coexist by design; they are not redundant:
>
> | Primitive | Blocking semantics | Producers | Saturation surface | Home |
> |---|---|---|---|---|
> | `BoundedQueue` | `push` blocks; `try_push` → `Full(v)` | many (std channel) | Block / Fail via caller choice | buffer.rs (std) |
> | `BoundedMailbox` | `send` parks on own guaranteed seat; `try_send`/`fire` nonblocking | many, **anti-starvation (one seat per producer)** | Block / Fail / best-effort (three modes) | mailbox.rs (std) |
> | `BoundedRing` | none (storage only); `push` immediate `Full(v)` | one (single-threaded contract) | attempted push = verdict | ring.rs (no_std+alloc) |
>
> `BoundedCarrier<CAP>` is **carried by `BoundedQueue`** today; a mid-term swap of its
> innards onto `BoundedMailbox` (removing one wrapper layer) and a deprecation track for
> `BoundedQueue` are open; the disambiguation table above answers the "stacked look"
> objection in the meantime. Also noted (docgate blind spot): prose API names in the
> docs are not machine-checkable — only ```rust fences are compiled; known boundary.

### Carrier catalog entries, six-tuple (S/L/T/C/V/R) — 2026-08

| Carrier | S (interface & observable behavior) | L (normative strength) | T (conformance) | C (profile) | V | R |
|---|---|---|---|---|---|---|
| `InlineCarrier` | direct pass-through; saturated N/A; zero allocation | MUST: pure relay (T1) | topology tests + C9 bench | Kernel/Embedded/Tool | 0.3 | registry (C3) |
| `QueueCarrier` (std) | heap relay; Block (conservative) | MUST: per-message alloc declared | cost gate (`validate_cost`) | Service/Tool | 0.3 | registry (C3) |
| `BoundedCarrier<CAP>` | bounded relay; CAP≥1 gate; Block | MUST: capacity witness (②) | `assert_capacity_nonzero` + bounded tests | Kernel/Service | 0.3 | registry (C3) |
| `spawned_flow` (std) | dedicated thread; panic propagates | MUST: family-A Sync declared (Z1) | cross-thread equivalence (T6) + teardown tests | Service/Tool | 0.3 | registry (C3) |
| `ResultCarrier`/`MaybeCarrier` | X-lane: Ok passes, Err short-circuits (B not run) | MUST: failure-as-value | short-circuit tests (§9.2) | Tool | 0.3 | registry (C3) |
| `event` substrate (`ChunkSource`/`pump_events`) | external events → `A::In`; teardown = stop pulling | MUST: pairing law (N↔N) | pump tests + ledger row | Service/Tool | 0.3 | ledger (C11) |
| `async_seam` (`Poller`/`SeamPoller`) | poll; deadline verdict (sync-domain TimedOut) | MUST: step never awaits (D2) | async-seam tests + ledger row | Service/Tool | 0.3 | ledger (C11) |

### Third-party adapter guide (2026-08)

To attach a physical implementation **without touching the core**: (1) implement
`Carrier<A, B>` for your seam (S: interface + observable behavior; L: declare
`cost()`/`obligation()`/`saturation()` truthfully); (2) test it as an external consumer
(examples/tests style; T6 sampling equivalence where claimed); (3) use open profiles
(`Tool`/`Embedded`, `assemble_profile`) or, for gated profiles, request registration —
`Registered` is sealed (C3), so unregistered adapters are refused at compile time on
gated profiles by design (whitelist = official catalog); (4) async executors implement
the `Executor` contract (C7 layer 3) instead of axiom shipping one (zero-dependency
promise, D6). Each entry point is a declaration + a check, never a silent default.

Each carrier is **independently selectable and replaceable**: swapping one implementation
does not change the topology (T6, multiple physical implementations).

> **Replaceability layers (constitution)**: pluggability is *stratified*, not universal.
> ① **Mechanism layer — open by mandate**: carriers, short-circuit forms, executors,
> telemetry sinks, event sources, profiles are trait sockets; third parties plug in via
> their own crates (`axiom-tokio` pattern). A new socket opens only when a second
> implementer or a real requirement exists (minimal-basis law). ② **Policy layer —
> semi-open**: saturation strategies and profile floors are declared per deployment.
> ③ **Vocabulary/constitution layer — closed**: the five concepts, `Delivery`/`Modality`
> lattices, obligation axes are the language itself; replacing them is another framework,
> extending them requires the closure-checklist procedure (collective adjudication).
> Layer ③ being non-pluggable is what makes layer ① interoperate.

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

- **`flow`** (`semantics/src/drive/flow.rs`): `drive_link` — after compile-time
  wiring verification (unified `Conforms<Wire>`), drives one A→B causal flow with the selected carrier;
  **verification happens at compile time, zero runtime overhead**. (`drive_wired` removed as a
  redundant alias of `drive_link`.)
- **`static_path`** (`semantics/src/drive/static_path.rs`): `run_static` / `run_declared_static` —
  inline-expands at **compile time** the subgraph declared by `Static<SUB>` as "requiring
  zero cost" (zero runtime objects).
- **Declaration macros** (`semantics/src/drive/macros.rs`): `wire!` — a macro/compile-time technique
  that completes "wiring + carrier + verification" in one step at compile time.

---

## 3b. Unified-model activation (runtime, `std`)

The runtime gives *activation* to the unified-model constructs (which are **definitions** in
`core.md`; activation stays the run/carrier side):

- **`SlotPending<I,O>` → `SlotDrive<I, O>`** — *existential binding* (`semantics/src/drive/slot.rs`) —
  the ∃ existential fill of a `Slot<I,O>` under a **license lifecycle (typestate, modality ①)**:
  `SlotPending::install` (Adding) installs a compile-time-conforming inhabitant
  (`T: PortCell<In=I,Out=O>` ⟹ core `Conforms`), type-erases its state to `Box<dyn Any + Send>`;
  `commit()` (Ready→Live) authorizes `SlotDrive` — driving before commit is a **type-level
  refusal** (no runtime check, Placement Law A3); `SlotDrive` then `drive`/`swap`s at runtime
  (swap bumps a generation, so a previously created `Seat` is rejected as stale); `retire()`
  terminates the license (Cleaned). Physical side of "dynamic loading": interface fixed &
  T1-verified at compile time, inhabitant existentially chosen at runtime.
- **`drive_seq`** (`semantics/src/drive/flow.rs`, `no_std + alloc`) — the generative/unbounded-count side of
  `Rep<N,C>`: drive a runtime `IntoIterator` sequence of inputs through one cell, collecting
  outputs, with state held across steps (count decided at runtime, not compile time).
- **`drive_feedback_inline<BODY, FEED>`** (`semantics/src/drive/flow.rs`) — the physical activation of a
  `Feedback` cell form: one inline-unbuffered loop (`BODY -> FEED -> BODY`) per input step,
  gated by the **Moore declaration** (`FEED: Moore`, modality ④ — declaration, not proof).
- **`contract` module** (`semantics/src/checks/contract.rs`) — deployment & compile-time seam contracts:
  `Moore` marker (④); `assert_capacity_nonzero` (②); `validate_cost` / `validate_capacity` /
  `validate_seam` (③); `ContractError`.
- **`obligation` module** (`semantics/src/checks/obligation.rs`) — the obligation-class type system
  (delivery × resource × reference × lifecycle) and the **obligation ledger** (`LEDGER`):
  a machine-readable constitution excerpt (seam × obligation × modality × witness ×
  conformance test), enforcing the minimal-basis and honesty rules (A4/A5).
- **`delivery` module** (`semantics/src/checks/delivery.rs`, `std`) — the four-state delivery taxonomy:
  `Full` / `Closed` mechanized from `mpsc` errors with the rejected value preserved (②③);
  `Timeout` / `Cancelled` **declared** as modality ④ (mechanization is a physical choice:
  timer / request-scoped channels), no fabricated witnesses.
- **`mailbox` module** (`semantics/src/movers/mailbox.rs`, `std`) — the anti-starvation bounded mailbox:
  capacity = `CAP` buffer slots **plus one guaranteed slot per producer**;
  three send modes—`try_send` (strict, `Full(v)` with the value returned), `send` (blocking
  backpressure, parks on its own guaranteed slot), `fire` (best-effort: buffer first, then the
  producer's own slot); `recv` is blocking and never returns `Empty` (`try_recv` observes the
  empty state); close drains then reports `Closed` with values returned. Modality ② capacity
  gate (`CAP ≥ 1`). `bounded_pump` stays as the teaching form; the mailbox is the
  anti-starvation instance of the same obligation class (per-producer slot).
- **`profile` module** (`semantics/src/checks/profile.rs`) — the **profile catalog** (six-tuple C
  component; F↦C(F)): `KernelProfile` (zero-alloc budget), `ServiceProfile`
  (per-message budget + Full/Closed mechanized), `ToolProfile` (external); a profile is a
  modality-① type token plus a modality-③ budget gate—`assemble_profile<P, A, B, C>()`
  rejects carriers exceeding the profile budget, so the same topology swaps profiles without
  touching the graph (T6). Carrier whitelists remain normative documentation (open `Carrier`
  impls cannot be whitelisted at the type level—honest A5 note).

- **`law` module** (`semantics/src/checks/law.rs`, `std`) — runtime-law probes (T-component
  deepening): pairing law (N sends ↔ N verdicts; received ≤ delivered), sequence monotonicity,
  broadcast fan-out counting; `debug_assertions`-gated, release zero-overhead.
- **`assemble_link` / `assemble_seam`** (`semantics/src/drive/flow.rs`) — the wired **modality ③
  entries**: validate cost (and, for bounded seams, capacity) once at the deployment assembly
  point and return the `drive_link` function pointer (`Driver<A,B>`); a budget violation is an
  **assembly failure**, never a silent runtime cost. (`BoundedCarrier`'s own const gate is
  modality ②; `assemble_seam` backstops ungated carriers at deploy time.)

Both families are safe (`#![forbid(unsafe_code)]`); `SlotDrive` is `std`-gated, `drive_seq` and
`drive_feedback_inline` are not — they localize the dynamic tax to the
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
cargo build/test --manifest-path semantics/Cargo.toml   # runtime (25 integration + 5 contract unit tests)
cargo run --manifest-path semantics/Cargo.toml --example carrier_demo
cargo run --manifest-path semantics/Cargo.toml --example threaded_flow
cargo run --manifest-path semantics/Cargo.toml --example redis_like -- --corpus 500   # Redis-like subsystem use-case (in-repo example)
cargo test --manifest-path semantics/Cargo.toml --example redis_like                 # 6 cell 单元测试
cargo bench --manifest-path semantics/Cargo.toml --bench carrier
```

**Accomplished (chain of evidence)**:
- The runtime depends only on cell_core (the new core), not on any v0 module.
- Carrier catalog: Inline (stack pass · zero allocation) / Queue (heap-queue relay) / Bounded
  (bounded channel, `CAP ≥ 1` at compile time) / spawned_flow (cross-thread mpsc) / static_path /
  wire! (declaration macro).
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

These use cases are build use cases for "building real programs on axiom/axiom-semantics", and
also the carriers for runtime iteration and equivalence verification. Legacy counterparts of
these use cases (including TCP-server-shaped ones) can be recovered from git history for
reference (`git show main:semantics/examples/<name>/main.rs`).

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
- **No-panic convention (A3; 2026-08)**: engineering convention — a cell's `step` must not
  panic; failure must be a value (`Out = Result`); a violator is the declarer's responsibility.
  Cross-trust-boundary defense uses `flow::drive_catch` (`catch_unwind`; **External-class
  high cost** — after a panic the states may be half-updated and are the seam's
  responsibility); hot paths do not pay this tax. `spawned_flow` / `bounded_pump*` already
  propagate cross-thread panics (teardown is explicit).
- **Topology-level resource budget (C4 feasible subset; 2026-08)**: thread count is
  countable (`spawned_flow` acquires exactly one thread per flow; assembly-time
  arithmetic); allocation is summable (chain per-message class = max over segments,
  per the `CarrierCost` order; `validate_cost` enforces per-seam budgets); stack depth is
  generally undecidable — compile-time stack-depth derivation is NOT promised (honest
  boundary; no fake derivation). Mechanical subset pinned at
  `semantics/tests/resource_budget.rs`.

---

## 9. Known Open Boundaries

> The following are **thin edges** within the runtime's positioning, exposed by the real use
> cases, currently **unresolved but acknowledged**. They belong to "engineering
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
- Resolved: first-class short-circuit carriers — `ShortCircuit` /
  `ResultCarrier` / `MaybeCarrier` (`carrier.rs`) with the `drive_try_carrier` entry:
  `Ok` passes through to `B`, `Err` short-circuits **without executing `B`**; the standard
  `Carrier` bound (`B::In = A::Out`) cannot express the X-lane, so they are implemented as a
  first-class capability, leaving the `Carrier` trait unchanged (T6 unaffected).
  The combined failure × backpressure semantics is provided by `bounded_pump_try`
  (`flow.rs`): `Ok` enters the bounded queue (full = blocking backpressure); `Err`
  short-circuits (not queued) and is counted.

> **Error algebra policy (C15-T2; 2026-08 supplement).** "Failure as a value" (§8.2
> concept-1 instance) needs its propagation rules written down. The status:
> - **Type level (already forced)**: `E` is part of `Out = Result<X, E>`; a seam whose
>   `B::In ≠ A::Out` fails to compile — cross-segment error-type compatibility is enforced
>   by T1, not by convention (netpath's single shared `NetErr` is the type fact, not sugar).
> - **Policy level (free, decided by the driver/seam, documented here)**:
>   - *Fail-fast* (first error aborts): `TryChain` / `drive_try_carrier` — E is not merged;
>   - *Collect* (aggregate E): requires an explicit adapter cell (E stays a value; no hidden
>     collection in cores);
>   - *Union/lifting* (join different E per segment): requires an adapter cell;
>   - *Degrade* (fallback value on Err): `MaybeCarrier` — `None` replaces the failure.
>   The policy is a **driver-side placement** (L4), never a core notion; it can be verified
>   by sampling on a bounded domain (C8-2 counterexample search), never by structure alone
>   (T5).

### 9.3 The Seam for Attaching External Input Sources (IO Events ↔ flow)
The documentation has declared "IO is physical/carrier-replaceable", but the grounding
interface for "how the external world (socket events, etc.) formally becomes the `in` of a
causal flow" has not been formalized. **First realization (landed)**: `redis_like`
(`semantics/examples/redis_like`, `--tcp PORT` / `--selftcp`) — a std-only TCP server:
per-connection stateful `LineSplit` (cross-chunk buffering) → `CmdParse` (typed errors,
short-circuit) → **bounded channel (backpressure)** → a store worker thread owning
`StoreState` (`DataStore` total, no panic path) → RESP reply routing with per-connection
FIFO order and write-half close on EOF.
- Layer: **runtime** (an event substrate is just a class of carrier/driver).
- **Carrier class formalized and landed** (`semantics/src/seams/event.rs`): an event stream
  (`EventStream`, item-level input source) + chunk-source adapter (`ChunkSource`:
  `io::Read` raw source + splitter + per-source cross-chunk state, with the general
  line splitter `split_lines`) + pump driver (`pump_events`: transform cell →
  delivery verdict `PushVerdict` → pair-law accounting `EventPumpStats`).
  `redis_like --tcp` is the reference implementation of this seam; `server.rs::handle_conn`
  is now driven by the class (behavior unchanged, selftest byte-identical). Failures are
  data (parse errors are forwarded through the channel as `-ERR` replies, not swallowed
  by the pump); a consumer teardown stops the pump (`dropped` counted, no silent
  continuation); chunk capacity N≥1 refuses the degenerate state via the modality ② gate
  (boundary-ontology Prop. 2.7). Ledger row: `event::pump_events` (LEDGER_STD_EXTRA).

### 9.4 Failure × Backpressure Occurring Simultaneously
Resolved by `bounded_pump_try`: when the buffer is full and a step fails at the same time,
the failing value short-circuits (not queued, counted) while successful values keep blocking
on the full queue — failure and backpressure are orthogonal and each is explicit.

---

## 10. Cost Semantics (Z1; the formalized core of the zero-cost promise)

The runtime's cost claims as a formal grammar — **edge cost = f(carrier, placement, types)**:

```
edge_cost(seam) := class(f):
  class(Inline, static, ZST)        → ZeroAllocInline
  class(Queue|Bounded, static, …)   → PerMessageAlloc            (per-message heap)
  class(spawned_flow, static, …)    → PerMessageAlloc + Sync     (family A)
  class(Slot existential, …, erase) → PerInstallAlloc + indirect (dynamic tax, C9)
  class(any, …, dyn-erased)         → + downcast per drive

composition (C4, pinned):  chain class = max over segments.
budget (modality ③):       declared ≤ budget at each seam (validate_cost);
                           obligation floor per profile (C10).
```

**Family-A irreducibility (declarative proof skeleton; modality ④).** Claim: the
cross-thread additive (`Sync` + per-message synchronization) **cannot be eliminated by
abstraction** — only transferred equitably. Skeleton: (1) causal flow across threads
requires shared memory plus synchronization (wakeup/visibility) at the seam; (2) the
zero-cost promise is **relative equality** (`foundations.md` §0: runtime cost ≡ cost of an
equivalent hand-written program) — a hand-written multithreaded program pays the same
toll; (3) were the family-A tax eliminable, a zero-synchronization cross-thread value
transfer would exist, contradicting causal ordering/observability of the flow; hence
elimination leads to contradiction. The skeleton is a statement, not a machine proof
(Rice boundary, modality ④). Measurement-corpus witnesses: `dynamic_tax.rs` (C9) and
`bench_common.rs`'s noise-floor method; recognized layout sensitivity is acknowledged as
such, never as a single number.

## 11. Conclusion

> runtime = the `cell_core` **physical-layer implementation use case**: a carrier catalog
> (Inline/Queue/Bounded/spawned_flow/static_path/wire!) + redemption verification, modular and
> replaceable, explaining and verifying that "the same static graph can be plugged into
> multiple physical executions (inline/queue/cross-thread), each with verifiable semantic
> equivalence". Carriers can be attached by implementing the `Carrier` trait without changing
> the topology, giving the physical layer extensibility; the resolved items (backpressure,
> failure × backpressure) and the open questions within its boundaries (IO seams,
> first-class short-circuit carriers) serve as driver
> input for subsequent iteration.

---

## Appendix: Source Layout and Async Path

The source is grouped by layer: `checks/` (hookup checks and promise book: contract, profile,
obligation, law, delivery), `movers/` (value movers: carrier, buffer, ring, mailbox), `seams/`
(wait, event, observation: async_seam, event, telemetry), `drive/` (composition and drivers:
flow, slot, enum_slot, static_path, macros). `instances/src` has `backend/` (async_driver and
tokio_exec); `examples/sql-over-redis/src` has `plans/` (sql_plan, redis_plan).

The async path: the runtime declares the `Executor` contract (`seams::async_seam`). The real
async path lives in `axiom-instances` (`backend::async_driver`): waits suspend on the tokio
reactor, deadlines come from tokio's timer (`tokio::time::timeout` around the input wait), and
commands can arrive while waiting (channel feeding). Output equals the sync path line by line
(T6; the composite use case checks 195/195 rows). `backend::tokio_exec` is a placeholder. Observation is an ordinary module (collect → summary → print in the
example), disabled by default. The concurrency demo serves N sessions on one thread with wall
time independent of N; per-step calibration (release, min-of-N with self-noise floor): sync
≈ 0.5 µs/line, async ≈ 0.9 µs/line. On this host, tokio timed waits quantize at ≈ 15.6 ms.

`tokio` is the default async backend (the feature adds the engine behind the `async` door).
Third-party physical adapters (an async-runtime replacement layer, a second backend) are
postponed; the adapter protocol is defined when a second implementer appears
(seam-before-socket rule).

Open items: multi-core parallelism under load is unmeasured; the ledger row for Timeout
modality ②③ awaits an authority change; real network async I/O (tokio `net`) is open — the
current feed is channel-based.
