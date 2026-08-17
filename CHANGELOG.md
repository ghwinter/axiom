# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/), and versioning follows
[Semantic Versioning](https://semver.org/) (before 1.0, breaking changes are
expressed by incrementing the minor version).

Migration guide: [docs/migration-0.2.md](docs/migration-0.2.md).

## [0.2.0] — 2026-08-18 — Consistency integration refactor (breaking)

This release is a breaking refactor: it unifies the blueprint concept, removes
all deprecated compatibility layers, and upgrades "declarations" to "contracts".
No backward-compatible aliases are kept — the goal is a complete, mature, and
robust final product. See the [migration guide](docs/migration-0.2.md).

### Breaking

**Unified blueprint concept (three topology expressions → one concept, two materialization paths)**

- `Topology` is the single topology-declaration blueprint; `StaticTopology`
  (compile-time projection: `Chain`/`Diamond` combinators + the `Straight`
  contract) and `DynamicTopology` (runtime projection, value form) are two
  materialization paths of the same declaration.

**Naming convergence**

- `DeploySpec` is renamed `DynamicTopology` (runtime value form).
- The instance-mutation time form `DynamicTopology` is renamed
  `TopologyMutation` (removing the name collision).

**Removed deprecated compatibility layers**

- `static_exec`: the old enum port contract `Link` / `IdLink` / `Split` /
  `CloneSplit` / `Merge` and their traits are removed.
- `static_path`: the fixed-N convenience functions `pipeline2` / `pipeline3` /
  `fanout2` / `fanin2` are removed.
- `projection`: the `replay` free function is removed; `Projection::replay` is
  the single entry point.
- `lib.rs`: the corresponding deprecated exports are removed.

**Static path single entry point**

- Static execution is entered exclusively through the recursive combinators
  `Chain` (linear) + `Diamond` (split–merge) + `feedback`, together with the
  `Straight` contract (`StraightMachine` / `StraightLink` / `StraightSplit` /
  `StraightMerge`) to express arbitrary-depth series-parallel DAGs.

### Added

- **Declaration → contract** (three previously non-enforced declarations are
  now deploy-time checks):
  - `FlowKind` × `LinkKind` carrier-compatibility matrix
    (`carrier_compatible`), enforced in `validate_deep` / `validate_report`;
  - the `Moore` marker trait: deploy-time consistency check between a machine
    "declared Moore" and its actual `Moore` trait implementation;
  - backpressure policy ↔ carrier correspondence:
    `BackpressurePolicy::required_action` × `BackpressureActionSupport`,
    checked in the runtime contract.
- `FlowKind` becomes an **optional semantic annotation** (`Data` is the
  un-annotated default, no carrier constraint); the physical layer does not
  distinguish the three flows — the annotation only affects carrier-selection
  preference on annotated edges (`Observe` → non-blocking, `Control` →
  droppable), enforced by the `(FlowKind, LinkKind)` compatibility matrix.
- Module maturity tiers (stable / experimental / tool) across 31 public modules.
- Physical-placement continuum: a unified single- and multi-threaded execution
  model; edge cost is determined by `MachinePhysicalSpec` (placement) +
  routing requirements (labels) + carrier.
- `MachinePhysicalSpec` gains a `per_message_latency_us` field
  (`serde(default)`).

### Performance

The refactor introduces no performance regression; the zero-cost promise holds
(`benches/dag.rs`, runtime `dynamic` bench):

| Comparison | Result |
|------------|--------|
| `T: StaticChain` generic path vs direct call (Diamond, 100k) | 87.2µs vs 88.7µs (~1.7% difference, within noise) — generic abstraction and direct call produce the same execution shape |
| Streaming pass-through vs handwritten loop | 77.1µs vs 76.9µs (0.3%) — isomorphic execution shape |
| Allocations per message (3-stage chain) | dynamic 3.0 / fused 1.0 / static 0.000 |

### Documentation

- Added the migration guide `docs/migration-0.2.md`.
- Unified the execution-model description as the "physical-placement
  continuum"; the "downgrade" phrasing is retired.

## [0.1.0] — Early releases

The first 0.1 series: `Func` + `Machine` primitives, typed ports, explicit
topology, deployment physical specs. The 0.1 API is superseded by the 0.2.0
consistency refactor; no compatibility layer is provided.
