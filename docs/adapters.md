# Adapter Ecosystem Rules and Runtime Contract Certification

This formal document defines the ecosystem rules for third-party runtime
adapters and the capability certification threshold they must meet. `axiom` core
is a pure contract layer; physical execution is provided by adapters. This
document specifies how adapters cooperate with core, how they declare their
capabilities, and how those capabilities are verified.

---

## Adapter Ecosystem Rules

### Positioning

`axiom` core is the contract layer (ports, topology, verification, static path
combinators). Physical execution — threads, channels, IO multiplexing,
scheduling — is provided by a runtime adapter. Core ships with a single
reference adapter (`axiom-runtime`). Third parties may provide adapters that
target different physical worlds, such as asynchronous runtimes, IO
multiplexing, embedded targets, or WASM.

### The reference adapter is a catalogue, not a monolith

`axiom-runtime` is positioned as a **collection of replaceable physical
modules** ("how data flows" designs — stack-passed `Inline` calls, heap
`Channel`/`BoundedBuf` queues, single-slot `Latest`/`SharedState`, bounded FIFO
`CasFreeRing`), each usable standalone. A deployer selects the subset its
blueprint needs (`LinkKind` per edge) rather than accepting a single mandated
execution shape. This mirrors the adapter ecosystem rule below: adapters are
replaceable physical implementations, and the reference runtime is itself the
largest such composition. Its `Guarantees` (via `RuntimeContract`) enumerate
exactly which physical modules it provides, so an application can rely on the
subset it declares and swap the rest. The modularization of the reference
runtime into standalone composable units is an active direction
(`docs/internal/runtime-modularization-design-notes.md`).

### Dependency Direction

> Adapters depend on core contracts, never on each other's providers.

- An adapter's `Cargo.toml` depends on `axiom` (the contract layer) and does not
  depend on other adapters.
- Adapters cooperate through core contracts (`Machine`, `LinkKind`,
  `ExecutionHint`, `RuntimeContract`) and never import one another.
- Rationale: adapters are replaceable physical implementations. If adapter A
  depended on adapter B's provider, A could not be deployed when B is absent,
  and replaceability would be broken.

### Grouping and Naming

```text
axiom/<adapter-name>           # suggested workspace layout
  core/                        # contract layer (the sole dependency target)
  axiom-<adapter>/             # third-party adapter (depends on core, not on each other)
```

Naming: `axiom-<adapter>` (for example, `axiom-tokio`, `axiom-io-uring`,
`axiom-wasi`). Each adapter declares the `Guarantees` it supports in its
`Cargo.toml` (see Runtime Contract Certification).

### Release Expectation Tiers

| Tier | Meaning | Compatibility Commitment |
|---|---|---|
| **Product** | Production-ready, stable API | Semantic versioning; breaking changes go through deprecation |
| **POC** | Prototype that validates feasibility | No stability promise; may be refactored at any time |
| **Support** | Testing / tooling infrastructure | Low compatibility expectations |

Core and the reference adapter are Product. Experimental adapters (such as
specific IO backends) are POC. A new adapter may start as POC and be promoted to
Product once it matures.

---

## Runtime Contract Certification

### Problem Statement

A blueprint (`DynamicTopology`) declares topology and physical requirements
(`LinkKind`, `ExecutionHint`, `MachinePhysicalSpec`). An adapter's physical
capabilities, however, may not be able to honor those declarations. For example,
an adapter that does not support the `Inline` transport may encounter an `Inline`
link, or a zero-latency adapter may encounter a topology that requires a Moore
element to break a cycle. If these mismatches are not detected before
deployment, they surface only at runtime.

### Mechanism: `RuntimeContract` + `Guarantees`

Core defines `RuntimeContract` (in `src/runtime_contract.rs`): an adapter declares
its physical capabilities as `Guarantees` — which `LinkKind` values it
supports, which execution modes, memory ordering, IO capabilities, link latency
model, and physical budget.

Certification flow (before deployment, at `materialize` time or after blueprint
validation):

```text
DynamicTopology (declares what)           Guarantees (adapter's how capabilities)
        │                                  │
        └───────── check_spec ─────────────┘
        RuntimeContract::check_spec(spec, schemas) -> ValidationReport
        │
        ├─ report empty → materialize and execute
        └─ report non-empty → reject before deployment (structured violations, each indicating which declaration cannot be satisfied)
```

A blueprint fails certification (the report is non-empty) when:

- A `LinkKind` referenced by the blueprint is not in the adapter's
  `LinkKindSupport` → violation.
- An execution mode required by the blueprint (for example, `thread_per_machine`
  for `Parallel(n)`) is not in `ExecModeSupport` → violation.
- The physical budget (`PhysicalBudget`) demanded by a machine exceeds the
  adapter's capabilities → violation.
- A cycle has no Moore element and the adapter uses a zero-latency model
  (`LinkDelay::Zero`) → violation (algebraic cycle).

### Reference Adapter Contract

Core ships a reference declaration, `ReferenceRuntime`, whose `Guarantees` mirror
the built-in `axiom-runtime`: all `LinkKind` values, the sequential /
thread-per-machine / async-cooperative execution modes, and the one-tick link
latency model. It serves as the model for third-party adapters — declaring
`Guarantees` the same way is sufficient for `check_spec` to verify them. An
adapter implements `RuntimeContract` itself (or uses `ReferenceRuntime` as its
declaration when it matches the built-in runtime); core cannot depend on the
runtime crate, so `ReferenceRuntime` is a declaration in core, not the runtime
object.

### Requirements for Third-Party Adapters

1. Implement `RuntimeContract` and declare `Guarantees` honestly (do not declare
   capabilities that are not supported).
2. Invoke the audit at `materialize` time and reject before deployment when
   capabilities cannot be satisfied (fail loud).
3. Declared capabilities must be covered by tests (at least smoke tests at the
   POC tier).

---

## Integration with the Existing Structure

- `src/runtime_contract.rs`: the `RuntimeContract` trait, `Guarantees`, and
  `ReferenceRuntime` are in place; third parties implement against this file.
- `axiom-runtime`: the reference adapter. At `materialize` time it invokes
  `DynamicTopology::validate` plus endpoint validation (port existence and
  direction). Certification is the adapter's pre-deployment step: a third-party
  adapter calls `RuntimeContract::check_spec` before `materialize` and rejects
  the deployment when the report is non-empty.
- [design-principles.md](design-principles.md) §1: physics is a finite set of execution forms —
  `Guarantees` is the typed declaration of that set. An adapter declares which
  members of the set it supports, a blueprint declares which members it needs,
  and deployment matches the two; a mismatch means rejection.

---

Together, these two parts implement the engineering goal of deploy-time physics.
The ecosystem rules make the adapter ecosystem replaceable (each adapter depends
only on core contracts, not on other adapters) and tiered (Product / POC /
Support). Certification makes adapter capabilities verifiable (`Guarantees`
declaration plus pre-deployment audit; unsatisfiable capabilities are rejected).
Physical capability is a declared fact. Deployment matches declarations, and a
mismatch is a configuration error rather than a runtime incident.
