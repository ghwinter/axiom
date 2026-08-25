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
  LiteOS-style dual-counter FIFO (O(1) push/pop, branch wraparound), typed
  `Full(v)`/`Empty` verdicts with value conservation, one reserve allocation at
  construction and zero per-message allocation in steady state. Single-threaded
  by contract; cross-thread variant awaits the critical-section decision (D4).
  Serves `EmbeddedProfile`.
- **Carrier obligation declarations** (C10 step 1): `Carrier::obligation()`
  added — fail-closed default (`External`), truthful overrides for
  `InlineCarrier` (`ZeroAllocInline`) and queue/bounded carriers
  (`PerMessageAlloc` + mechanized Full/Closed delivery). `EmbeddedProfile`
  added (zero-alloc steady-state budget). `Profile::obligation_min` remains an
  inert placeholder by honest declaration until the delivery axis grows an
  N/A variant — no fake enforcement.
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
