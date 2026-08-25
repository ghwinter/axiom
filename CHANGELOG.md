# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/), and versioning follows
[Semantic Versioning](https://semver.org/) (before 1.0, breaking changes are
expressed by incrementing the minor version).

## [Unreleased]

### Added — runtime constitution phase: honesty debts (C9/C11)

- **Dynamic-tax bench** (`runtime/benches/dynamic_tax.rs`): same topology
  (`Inc -> Double`) across four channels — hand-written baseline / static
  generic `drive_link<Inline>` / erased `SlotDrive` / generation-checked `Seat`.
  Headline (stable): erased seam ≈ **+2.0 ns/op** (fn-pointer indirect +
  downcast), `Seat` adds ≈ +0.25 ns/op, `swap` ≈ **30 ns** incl. one heap
  allocation. Channel B is layout-sensitive across builds (~0%…+50%) and is
  documented as such instead of being quoted as a single number.
- **Executable obligation ledger**: each `LedgerEntry` now carries a `probe`
  that executes its *witness symbol* — renaming/deleting a witness breaks the
  build (modality ①); a false probe fails tests (modality ③). Std-gated rows
  (`law`, `delivery`) live in `LEDGER_STD_EXTRA`; no_std builds carry the core
  rows only (`ledger_rows()`).
- **no_std bounded ring** (`runtime/src/ring.rs`): `BoundedRing<T, CAP>` —
  dual-counter FIFO (O(1) push/pop, branch wraparound), typed
  `Full(v)`/`Empty` verdicts with value conservation, one reserve allocation at
  construction and zero per-message allocation in steady state. Single-threaded
  by contract; cross-thread variant awaits the critical-section decision (D4).
  Serves `EmbeddedProfile`.
- **Event-substrate carrier class** (`runtime/src/event.rs`, std): `EventStream`
  (item-level input source) / `ChunkSource` (`io::Read` raw source + splitter +
  per-source cross-chunk state, const `N` chunk buffer) / `split_lines` /
  `pump_events` (transform cell → delivery verdict → pair-law accounting) —
  formalizes the §9.3 IO seam: external events formally become the `in` of a
  causal flow. Failures are data (forwarded to the sink, not short-circuited);
  consumer teardown stops the pump (`dropped` counted, no silent continuation);
  chunk capacity N≥1 refuses the degenerate state via the modality ② gate
  (boundary-ontology Prop. 2.7). `redis_like`'s `handle_conn` is now driven by
  the class (selftest byte-identical). Ledger row `event::pump_events` in
  `LEDGER_STD_EXTRA` (modality ③).
- **Async-seam first layer** (C7, `runtime/src/async_seam.rs`, std): the D2
  executor-contract skeleton — `Poll`/`Poller` wrap a synchronous cell
  ("step never awaits"); two of the three waiting points are probed in the
  sync domain: input arrival and deadline (`poll_until` → `TimedOut`). Honest
  boundary: `TimedOut` is a sync-poll-domain deadline verdict, not
  `Delivery::Timeout`'s modality-④ semantics; degenerate deadlines refused
  (Prop. 2.7). Layer 2 (backpressure carrier, EX future/waker, SlotDrive
  co-wiring) pending.
- **Behavioral-equivalence guard** (C8-2, `runtime/tests/behavioral_equivalence.rs`):
  structural isomorphism ⊬ behavioral equivalence — minimal counterexample
  search separates shape-identical cells with different behavior on a sampled
  domain; expected-equivalent composites agree on the same domain (the
  verifiable fragment of T6, modality ③ sampling, A5 scope note). C8-3:
  foundations §4.2 "per-port equivalent" anchored to T5 bisimulation
  (en/zh degradation clause added).
- **Scale evidence** (C12, `runtime/tests/scale.rs`): 64-cell blueprints
  (flat `Rep<64, Inc>`, nested `Rep<8, Rep<8, Inc>>`, and a hybrid
  scheduler-row + deterministic-island + tail-row form) compose, drive to
  closed forms, stay zero-sized, and prove scale recursion is semantic
  identity (nested 8×8 == flat 64; subsystem = same-scale cell, note 9.9).
  T1 wiring legality and typed-hole conformance asserted across the seams.
- **Hot-swap in-flight disposition** (C5, `slot.rs`): `SlotDrive::swap_and_drain`
  forces explicit disposition of the old inhabitant's state (in-flight work),
  making silent discarding type-impossible; `Drainable` declares a state type
  that may hold in-flight work and yields it as a value (`drain_pending`).
  `swap` remains the assert-no-in-flight form (deployer responsibility, A5);
  generations still bump (stale `Seat` rejection). The concurrent shared-variant
  quiesce protocol is deferred to that form's landing. Ledger row
  `slot::SlotDrive::swap_and_drain` (modality ③).
- **Carrier obligation declarations** (C10 step 1): `Carrier::obligation()`
  added — fail-closed default (`External`), truthful overrides for
  `InlineCarrier` (`ZeroAllocInline`) and queue/bounded carriers
  (`PerMessageAlloc` + mechanized Full/Closed delivery). `EmbeddedProfile`
  added (zero-alloc steady-state budget). `Profile::obligation_min` remains an
  inert placeholder by honest declaration until the delivery axis grows an
  N/A variant — no fake enforcement.
- **Obligation floors enforced** (C10 step 2): `DeliveryKind` gains
  `NotApplicable` (sync pass-through seams) with the strength order
  `NotApplicable < MechanizedFullClosed`; `ObligationClass::meets_min` judges
  resource (declared ≤ floor) and delivery (declared ≥ floor) axes; the
  fail-closed default now claims no delivery mechanization. `Profile::obligation_min`
  is differentiated per profile (Kernel/Embedded = zero-alloc + N/A delivery,
  Service = per-message + mechanized Full/Closed, Tool = no floor) and enforced
  at `assemble_profile` via `contract::validate_obligation_min` (modality ③):
  a service seam assembled with a pass-through Inline carrier is now rejected
  (`ObligationUnderMet { axis: "delivery" }`) — obligations follow the profile,
  not just the cost budget (T6). Reference/lifecycle axes stay unjudged (A5).
- **Doc drift gate** (`runtime/tests/docgate.rs`, CI step "Docgate"): every
  ```rust fence in the formal docs is compiled against the current API;
  `rust,ignore` fences and `tmp/docgate-ignore.txt` entries are skipped.
  Current state: 12/12 blocks pass.

### Fixed

- `flow::drive_link`: missing cross-crate `#[inline(always)]` cost a real call
  per drive in some layouts (+45%…+89% on the C9 bench) — zero-cost promise
  restoration.
## [0.3.0] — 2026-08 — Axiometric core (current design baseline)

The crate is the **four-constituent compile-time core** (`cell_core`) plus a
physical-layer crate (`axiom-runtime`) of replaceable carriers. Blueprint =
zero-sized Rust type; verification happens at compile time (`Conforms`);
after compilation there are no axiom objects.

> **项目定位注记**：axiom 已重构为几乎全新的项目。0.1/0.2 的旧核心
> （`Func`/`Machine`/`Link`/`Deploy` 体系）已整体废弃并移出，**不保留迁移指南、
> 不提供兼容层**——旧历史仅在本文件以一行注记存证，不对新读者展开。

### Core

- Four constituents: `PortCell` / `Wire` / combinators (`Chain`, `Broadcast`,
  `Merge`, `Feedback`) / staticity (`Static`, `Blueprint`, `Conforms`,
  `assert_wiring`).
- Unified-model constructors: `Rep<N,C>` (bounded power `Cⁿ`), `Slot<I,O>` +
  `Conforms` (∃ definition side), `Choice<A,B>`, `Opt<C>`.
- `Id<I>` — identity unit; T2's unit law becomes constructible.
- `Diamond<SRC,R1,R2,DST>` — first-class split–merge combinator (serial–parallel
  complement of `Chain`).
- `Repeat<N,C>` — reading-friendly alias of `Rep<N,C>` (exactly-N power `Cⁿ`);
  `Rep::NONEMPTY` opt-in compile-time witness for sites requiring `N >= 1`.
  (A Kleene-plus constructor was deliberately rejected: an unbounded `C⁺` has
  no honest static semantics; see docs/internal/axiom-conventions.md §13.)
- `Blueprint::define` is now `const fn` — definitions can live in `const`
  items, proving the definition↔activation split.

### Runtime

- `axiom-runtime`: physical-layer crate of **replaceable carriers** (template
  for third-party adapters); core has zero runtime objects.
- `contract` module — seam contracts with explicit proof modality:
  `Moore` declaration + `declare_inline_loop_moore` (modality ④),
  `assert_capacity_nonzero` compile-time witness (②),
  `validate_cost` / `validate_capacity` / `validate_seam` deployment checks (③).

### Tests & Benchmarks

- tests/topology_blueprint.rs — six hard assertions (expressiveness, type-level
  contracts on composites, non-invasion ZST/const proof, bit-exact static entry,
  defined-without-activation, determinism R001).
- benches/dag.rs — diamond shape: composite vs handwritten (Δ≈±1%, within self-noise
  floor) vs type erasure (dynamic-tax contrast).

---

## Historical note (archived)

0.1.0 / 0.2.0 属已废弃的旧核心（`Func`+`Machine` primitives、`Topology`/
`DeploySpec` 值形态蓝图、`FlowKind` 三分、`static_exec`/`static_path` 兼容层）。
这些设计已被 0.3 的四构件编译期核心取代；因 axiom 已几乎全新，**不保留迁移指南，
不提供兼容层**。仅存证于此，供理论文档追溯设计演进。
