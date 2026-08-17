# Migration Guide: 0.1.x to 0.2.0

Version 0.2.0 is a breaking refactor (consistency integration). It provides no
backward-compatibility layer and no deprecated aliases. This guide lists every
breaking change and its replacement so that 0.1.x users can migrate in one
step. Release notes are in [CHANGELOG.md](../CHANGELOG.md).

## 1. Breaking changes at a glance

| 0.1.x | 0.2.0 |
|-------|-------|
| `DeploySpec` | `DynamicTopology` (runtime value form) |
| `DynamicTopology` (instance-mutation time form) | `TopologyMutation` |
| `static_path::pipeline2/3`, `fanout2`, `fanin2` | `Chain` + `pipeline_chain`; `Diamond` + `diamond` |
| `static_exec::Link`/`IdLink`/`Split`/`CloneSplit`/`Merge` (enum port contract) | `StraightMachine` + `StraightLink`/`StraightSplit`/`StraightMerge`/`StraightClone` contract |
| `projection::replay(...)` free function | `Projection::replay(...)` method |
| `FlowKind` (port annotation; connection requires matching kinds) | `FlowKind` is additionally a **materialization annotation** (`Data` = un-annotated default): `Observe`/`Control` imply a carrier preference enforced by the `(FlowKind, LinkKind)` compatibility matrix |
| `MachinePhysicalSpec` (no `per_message_latency_us`) | new field `per_message_latency_us: u64` (`serde(default)`, optional) |

## 2. Naming convergence

**`DeploySpec` → `DynamicTopology`**

`Topology` is the single topology-declaration language; `DynamicTopology` is
its runtime value form (the compile-time form is `StaticTopology`).

```rust
// 0.1.x
let spec = DeploySpec::new().with_machine(...);

// 0.2.0
let spec = DynamicTopology::new().with_machine(...);
```

**Instance-mutation time form → `TopologyMutation`**

The former `DynamicTopology` (instance mutation in `core::topology`) is renamed
`TopologyMutation`, removing the name collision with the runtime value form.

## 3. Static path: fixed-N functions to combinators

`pipeline2/3`, `fanout2`, and `fanin2` are removed. The only static execution
entry points are the recursive combinators:

- **Linear chain**: `Chain` (arbitrary depth) + `pipeline_chain`;
- **Split–merge**: `Diamond` (arbitrary series-parallel DAG) + `diamond`;
- **Feedback loop**: `feedback`.

```rust
// 0.1.x
let out = pipeline2(step_a, step_b, src);

// 0.2.0 — Chain recursive composition (arbitrary depth)
type StepChain = Chain<StepA, Chain<StepB, StepC, StraightId>, StraightId>;
let out = StepChain::run_all(inputs).expect("pipeline");
```

Every machine on the static path must implement `StraightMachine` (bare-payload
pass-through, no port enum / no type erasure):

```rust
impl StraightMachine for Step {
    type StraightIn = i32;
    type StraightOut = i32;
    fn process_straight(_: &mut Self::State, n: i32) -> i32 { n + 1 }
}
```

## 4. Enum port contract to Straight contract

`static_exec::Link` / `Split` / `Merge` and their `IdLink` / `CloneSplit`
variants are removed. Type conversion uses `StraightLink`, forking uses
`StraightSplit` (with `StraightClone`), merging uses `StraightMerge` — all
explicit zero-cost contracts with no implicit enum tags.

## 5. Projection: free function to method

The `projection::replay` free function is removed; the `Projection::replay`
method is the single entry point.

## 6. FlowKind: annotation with a materialization preference

`FlowKind` remains a per-port semantic annotation, and connections still
require matching flow kinds. In 0.2.0 the annotation is additionally a
**materialization preference**: `Data` is the un-annotated default (no carrier
constraint), while `Observe` (→ non-blocking) and `Control` (→ droppable /
latest-wins) imply a carrier preference. The new `(FlowKind, LinkKind)`
compatibility matrix (`carrier_compatible`) is enforced in `validate_deep` /
`validate_report`: an annotated edge on a contradicting carrier is rejected.
The physical layer does not distinguish the three flows.

## 7. Declaration to contract (new mandatory checks)

Version 0.2.0 upgrades three declarations that were previously not enforced
into deploy-time checks; violations now report `ValidationError`:

1. **FlowKind × LinkKind carrier-compatibility matrix**: annotated edges must
   use a compatible carrier;
2. **Moore marker trait**: a machine declared as Moore must actually implement
   the `Moore` trait;
3. **Backpressure policy ↔ carrier correspondence**: the carrier used by a link
   must support the actions the policy requires (`BackpressureActionSupport`).

## 8. Verification commands (full regression after migration)

```powershell
# core: all features + tests + examples
cargo test --all-targets --features serialize,derive
cargo test --doc --features serialize,derive
# no_std build
cargo build --no-default-features
# derive / runtime
(cd derive  ; cargo test --all-targets)
(cd runtime; cargo test --all-targets)
# bench regression
cargo bench --bench dag
(cd runtime; cargo bench --bench dynamic)
```
