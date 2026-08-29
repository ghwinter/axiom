# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/), and versioning follows
[Semantic Versioning](https://semver.org/) (before 1.0, breaking changes are
expressed by incrementing the minor version).

## [Unreleased]

### Added — telemetry-tracing instance (Telemetry second implementer)

- **`TracingTelemetry`** (`instances/src/telemetry_tracing.rs`, feature
  `telemetry-tracing`): the semantic `Telemetry` contract bound to `tracing` —
  the socket's **second implementer** (first: in-tree `ConsoleTelemetry`/
  `BufTelemetry`), pulled by a real consumer need (redacted-project observation
  module) per the minimal-basis rule. Declared level contract: `Delivered`
  → `trace`, `Full/Failed/Dropped` → `warn`, depth → `debug`, latency →
  `trace`; output destination is the subscriber's decision (print-vs-observe
  boundary mechanized). Zero-sized adapter; capture-subscriber test without
  extra deps.
- **`TokioBlockRing::with_telemetry`** (`backend/async_ring.rs`, gated
  `telemetry-tracing`): instance-side observation hook — `on_depth` (water
  level at production time) + `on_latency` (send-to-enqueue elapsed,
  including the wait-for-room backpressure window) on every successful send;
  `on_verdict(Dropped)` on closed rejection. Contract unchanged
  (`AsyncBlockRing` untouched); no-hook fast path unchanged. Hook test
  included.

### Added — adoption evidence & activation-model catalog, first slice (2026-08-29)

- **Compile-cost probe** (`core/examples/deep_blueprint.rs`): reproducible
  depth × compile-cost measurement (rustc 1.98, LTO+opt-3): depth ×8 ⟹ marginal
  compile ≈ ×8 (0.25 s → 2.05 s), binary size flat under LTO folding.
  Method walkthrough + truth table in-file; honest boundary noted
  (single-machine measurement, drifts with rustc).
- **Wiring-diagnostic note** (`core/src/cell_core.rs` `assert_wiring`):
  measured diagnostic shape — a mismatch is an atomic E0271 inequality + call
  site; deep `Chain` tails do not bury the error in generic stacks. Noted as
  rustc-drifting, not promised.
- **Documented-but-unimplemented gaps landed** (`semantics/src/checks/*`,
  `movers/carrier.rs`): D11 resource monoid (`resource.rs`), A1 saturation
  gate in profile assembly, A3 NoPanic declaration discipline.
- **Activation-model catalog, phase 1** (`examples/sql-over-redis/src/callresp.rs`):
  call/response correlation dispatcher — in-flight association + timeout sweep
  as a pure, deterministically testable component. **Time-as-value**: deadline
  verdicts take an injected `now` (no ambient clock); tests are fully
  deterministic. Deliberately **not** a `PortCell` (single-In/Out cannot carry
  "in-flight call + independent deadline + association"); it exists as a
  compatible neighbor, admitted through the closed-boundary checklist. Real
  wiring is deferred to the first real out-of-order async service (see
  `frontier-notes.md` item 8). **Uncommitted at audit time.**
- **Audit & convergence plan** (`docs/internal/theory/audit-2026-08.md`):
  evidence-backed full audit (builds/tests/benches/stability markers),
  necessity-filtered gap register (P0/P1/P2), and dignified non-goal
  registrations.
- **Known most-likely amendment point registered** (`core.md` §8, en+zh):
  `PortCell`'s single-In/Out signature — the alphabet's most fragile seam,
  named before it is hit.
- **Bettability policy made explicit** (`docs/README.md` A5, en+zh): a
  `stable` marker is the only pre-1.0 bettability promise; `experimental`
  carries none.

### Changed — subtraction discipline (2026-08-29)

- Frontier item 4 (behavior-verification tooling incl. deterministic
  simulation replay) downgraded from "extension/undone" to **registered
  deferral** — no promised claim, no forcing scenario; shape to be defined by
  the first real consumer.
- Boundary-ontology note 7.5 (boundary criteria & handover checklist)
  withdrawn: it was review-dialogue residue growing new governance vocabulary
  — the 极小基律 applies to documentation too (constructors are closed;
  annotations must not snowball).

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
- **Event-substrate carrier class** (`runtime/src/seams/event.rs`, std): `EventStream`
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
- **Cost semantics formalized** (Z1, semantics.md en/zh §10): edge cost = f(carrier,
  placement, types) as a formal grammar (per-class cost map, composition = max per C4,
  modality-③ budgets) plus the declarative proof skeleton of family-A cross-thread
  irreducibility (relative equality + contradiction; statement at the Rice boundary,
  modality ④).

### Added — instance layer, async path & layered structure (2026-08)

- **Workspace restructure**: virtual root manifest; the core package lives at `core/`;
  source directories mirror semantics — `runtime/src/{checks,movers,seams,drive}`,
  `instances/src/backend`, `examples/sql-over-redis/src/plans`.
- **Async path** (`instances/src/backend/async_driver.rs`): waits suspend on the tokio
  reactor with deadlines from tokio's timer (`tokio::time::timeout` around input waits);
  channel feeding delivers commands while waiting (`Poller::put`, additive); output equals
  the sync path line by line (composite use case, 195/195 rows).
- **Comprehensive use case** (`examples/sql-over-redis`): SQL-over-Redis composite plan;
  sync and async drivers; three-stage observation as an ordinary module; latency bench
  (min-of-N; async ≈ +90% per step); concurrency demo (single thread serves N sessions,
  wall time independent of N). tokio timed waits quantize at ≈ 15.6 ms on this host.
- Third-party physical adapters (an async replacement layer, a second backend) are
  postponed; tokio is the default async backend; the adapter protocol is defined by the
  second implementer (seam-before-socket rule).
- **Session-protocol port** (C1, `runtime/tests/session_protocol.rs`): ruling — session
  duality is the T1 typed-hole duality (In/Out exchange) plus `Choice` tags; protocol
  progress is an explicit state-phase cell (concept-1 instance); illegal transitions are
  typed failures (values, never silent). The v0 `is_dual`/`project` thesis is resettled.
- **Effect-annotation direction** (C2, frontier-notes #7): Alloc/Block/Async/Fail as
  demand-side signing (the obligation lattice's reverse side) — no new concept, but
  annotation burden requires fail-closed defaults and inferability; design document
  first.
- **Scalable-carrier registry** (C3, `carrier.rs` + `profile.rs`): `Registered` is a
  sealed family (crate-internal impl only — an external carrier cannot register, a
  modality-① fact); `Profile::GATED` marks Kernel/Service; `assemble_profile_gated`
  requires `C: Registered` at compile time (unregistered third-party carriers fail to
  build on gated profiles; the Bounded family is registered for any CAP). Whitelists
  move from documentation to compile-time facts without sealing `Carrier` itself.
- **Resource-budget subset** (C4, semantics.md §8 en/zh + `runtime/tests/resource_budget.rs`):
  thread count countable (one thread per spawned flow), allocation summable
  (chain class = max over segments per the `CarrierCost` order), stack depth honestly
  unbounded (no fake derivation).
- **Executor contract** (C7 layer 3, `async_seam.rs`): `Executor` trait (park step,
  the minimal surface for external executors; axiom ships no executor) + `ThreadExec`
  reference implementation; EX-generic wiring and SlotDrive co-evolution follow with
  adapters.
- **Observation interfaces** (B1, `runtime/src/seams/telemetry.rs`): `Telemetry` trait with
  `on_verdict/on_depth/on_latency` (empty defaults = no-op, zero cost at compile
  time), `NoOp`/`Buf`/`Console` implementations (console = one output destination),
  and `MeteredPush` as the wiring point; module is no_std ready.
- **Role-layered example** (B2, `examples/layered`): cells/topology/main three-file
  organization — domain authors expose `PortCell`, integrators write blueprints,
  deployers choose carriers with the modality-③ cost gate and T6 cross-physical
  equivalence; a multi-crate workspace is this structure modularized.
- **Embedded-shape evidence** (B3, `runtime/tests/embedded_shape.rs`): compositions
  using only cell_core (Id/Diamond/Rep) drive to closed forms with stack-tuple
  states; CI no_std builds are the second witness.
- **Control/observation co-form example** (B4, `examples/control_seam`): control is
  a value — instruction-source cell + State write (mode switch keeps counts),
  `Opt` pause gate, `SlotPending → SlotDrive` swap (ops surface); observation wired
  via Console/Buf (constitution §8 semantics).
- **Saturation policies** (A1, `carrier.rs`): `SaturationPolicy{Block, DropNewest,
  DropOldest, Fail, NotApplicable}` declared per carrier via `Carrier::saturation()`
  (conservative `Block` default; sync pass-through = `NotApplicable`); tests pin the
  declaration to the behavior (Block retains the value until space, Fail returns
  `Full(v)`, disconnect returns the value).
- **Backpressure waiting point** (C7 layer 2, `async_seam.rs`): `SeamPoller<A>`
  delivers `A::Out` through a real bounded channel with deadline polling
  (`roll_until`): Block = retained value re-delivered on space (no loss, no recompute),
  Fail/disconnect = value returned with the verdict. Theory supplements: T1 activation
  obligation (foundations §8.1, authorization ≠ acquisition) and T2 error algebra
  (semantics.md §9.2, E policy tiers; type-level E∈Out already forced).
- **Panic boundary carrier** (A3, `flow::drive_catch`): `catch_unwind` guards a causal
  drive; no-panic convention documented (semantics.md §8, en/zh) — failure must be a value,
  violators own the responsibility; External-class high cost at trust boundaries only.
- **Enumerated slot** (A4, `runtime/src/enum_slot.rs`): `EnumSlot<A,B>` — zero-erasure
  existential with a compile-time-known candidate set (index match, no downcast/boxing;
  both candidate states resident). The two cost curves vs `SlotDrive` are documented;
  a concept-4 instance (§8.3).
- **Version/stability policy** (A5, docs/README.md en/zh): `cell_core` stable at
  constitution level, per-module runtime Stability, 0.x minor breakage with
  concept-migration notes, unsafe isolated behind a dedicated feature.
- **Degenerate-state assembly** (C13, `runtime/tests/degenerate_states.rs`): the
  fifth-axis (admissibility, Prop. 2.7) mechanical landing point — capacity-0
  gates, fake mechanized delivery (fail-closed N/A), uncommitted-slot typestate
  refusal, anti-starvation, and zero-capacity chunk sources are assembled into
  one checkable surface; ledger row `degenerate-assembly (C13)` (probe: gate
  symbol + fail-closed default).
- **Async-seam first layer** (C7, `runtime/src/seams/async_seam.rs`, std): the D2
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
physical-layer crate (`axiom-semantics`) of replaceable carriers. Blueprint =
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

- `axiom-semantics`: physical-layer crate of **replaceable carriers** (template
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
