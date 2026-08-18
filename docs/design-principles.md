# Design Principles

> **Nature.** A formal document in `docs/`, complementing axiom's design philosophy. It records the **meta-problems** — the deeper questions about abstraction, physics, verification, and zero cost that recur across iteration — together with the corresponding design principles. Relationship to [philosophy.md](philosophy.md): it states axiom's worldview (abstraction/physics decoupling, zero cost, static-first); this document records the **meta-problems those claims exposed during iteration** — the evolution of criteria, the classification of violations, the unification of paradigms — and the design principles formed from them.

---

## 0. The Governing Principle: Blueprint–Redemption Isomorphism

### 0.1 The Core Proposition

The meta-question that governs every axiom design decision:

> **The blueprint is the product; execution is its redemption; validation is the isomorphism guarantee.**

A system's structure is declared once as a typed graph (the blueprint, abstraction layer $\mathcal{A}$); running it realizes that blueprint (the redemption, physical layer $\mathcal{P}_h$); validation guarantees the redemption is faithful — the correspondence $\mathcal{A} \leftrightarrow \mathcal{P}_h$ is **verified, not assumed**.

### 0.2 Why Three Value-Poles Are One Relationship

The three value claims axiom makes for complex systems are not three independent ends but three faces of the single correspondence between declaration and physics:

| Value claim | Face of the correspondence |
|---|---|
| Decouplability / maintainability / comprehensibility | the blueprint stays true as the system evolves |
| Verifiability (evidence, contracts) | the redemption is verifiable |
| Zero cost (static path) | the redemption is cheap enough to be the default path |

A feature that serves none of these faces — that neither makes the blueprint more truthful nor the redemption more verifiable — is over-design and is rejected.

### 0.3 The Meta-Question (Rejection Criterion)

> For every candidate feature: **does it make the blueprint more truthful (more accurately predicting runtime behavior), or the redemption more verifiable (more reliably honoring the blueprint)? If neither, it is over-design.**

"Truthful" is grounded, not rhetorical: a blueprint is truthful exactly when it predicts runtime behavior accurately, and accuracy is adjudicated by the evidence (the E-series verification contracts).

### 0.4 Decoupling Is the Means; Isomorphism Is the Promise

- **Decoupling** (the abstraction–physics split) answers: *can the physics evolve independently of the blueprint?* It makes verification possible.
- **Isomorphism** (the verified correspondence) answers: *does the physics actually honor the blueprint?* It keeps decoupling from degenerating into drift.

Decoupling without isomorphism degenerates into two unrelated things; isomorphism without decoupling degenerates into a single object with no separate layer to verify. The two hold together: decoupling enables verification, verification prevents decoupling from becoming a lie.

### 0.5 First Corollaries

1. **Prune morphology exceptions.** A structural vocabulary item must map 1:1 to a blueprint decision. An experimental machine morphology that neither makes the blueprint more truthful nor the redemption more verifiable is moved out of the default export surface.
2. **Wire every declaration.** A declared contract (`RuntimeContract`, `validate_deep` rules, the FlowKind×carrier matrix) that is not executed on a redemption path is an unverified correspondence — declared-but-unexecuted weight — and must be wired or removed.

### 0.6 Relation to the Other Principles

Every principle in this document is a specialization of the governing proposition:

- **execution-shape isomorphism** (§5.1) — zero cost is the *cheap redemption* face;
- **verification criterion** (§3.1) and **source / destination is a business error** (§3.2) — verification is the *faithful correspondence* face;
- **finite set of execution forms** (§1.2) — physics is organized into a finite choice set so the correspondence stays verifiable;
- **explicit over implicit** (§4.2) — the blueprint declares its physical needs so the correspondence is checkable.

---

## 1. Abstraction and Physics: The Precise Boundary of What and How

### 1.1 The Two Layers of Existence (Recap)

The abstraction layer $\mathcal{A}$ (modules, ports, topology, semantic annotations) and the physical layer $\mathcal{P}_h$ (threads, memory, instructions) are disjoint. axiom's responsibility is to keep abstract annotations from invading physical execution.

### 1.2 Meta-Problem: Unbounded Freedom or a Finite Set of Physical Processes?

**Question.** Different tasks (IO-bound, compute-bound, throughput, latency-sensitive, multi-core parallel) have different optimal physical processes. Is there a universal optimum? If not, should the abstraction abandon the physical process entirely?

**Reasoning.** The physical process is **not an unbounded space but a finite set of orthogonal dimension choices**:

```text
physical-process choice space = data-flow shape × concurrency shape × resource policy
  data-flow shape:   streaming (direct pass-through per value) | batched (staged collection) | windowed (batch accumulation)
  concurrency shape: single-threaded | parallel (sharded) | async (event loop / IO multiplexing)
  resource policy:   latency-first | throughput-first | IO-efficiency-first
```

**Principle (finite set of execution forms).** The physical process is not an unbounded space in which developers improvise freely; it is a **finite set of standard execution forms**. The abstraction layer declares *what* (topology, ports, semantics); the physical layer provides a finite *how* set (execution forms), chosen at deployment time. The developer declares only the **task type**, and the runtime selects the corresponding implementation from the set.

**Rationale.** This is more correct than both alternatives: "abandoning the physical process" cannot honor performance promises, and "standardizing on a single physical process" cannot adapt to task differences.

**Boundary.** The finite set covers physical processes; structural definitions (topology, composition) are axiom's contract capability. A custom physical process beyond the set belongs to the adapter layer — axiom does not offer unbounded physical freedom, but organizes physics into a finite, verifiable choice set decoupled from abstract declarations (§4.4).

### 1.3 The Full Meaning of Deploy-Time Physics

axiom's physical dimensions are: concurrency shape (`ExecutionHint`), carrier (`LinkKind`), and resources (`MachinePhysicalSpec`). The data-flow shape (streaming / batched / windowed) is the fourth dimension of the choice space; it is realized by the execution model rather than a spec field — the static path executes in streaming form (§2.4), each value flowing through the fused loop individually instead of batched relaying.

---

## 2. Zero-Cost Abstraction: From "No Extra Operations" to Execution-Shape Isomorphism

### 2.1 Meta-Problem: What Is the Criterion for Zero Cost?

**Question.** Is the non-invasion axiom ($t(\alpha) = t(h) + \epsilon$, $\epsilon < 5\%$) actually satisfied by the implementation? Measurement showed the static path (diamond) ~13× slower than hand-written — the criterion was violated. The question is: which class does the violation belong to?

### 2.2 A Precise Classification of Two Kinds of Violations

| Class | Definition | Nature | Fix |
|---|---|---|---|
| **In-shape redundancy** | redundant operations within the abstraction shape (labels, checks, wrapping, context passing) | constant factor, shape identical to hand-written | **code problem, fixable by patch** |
| **Shape difference** | the abstraction shape fundamentally differs from the hand-written shape (batched vs streaming, recursive vs linear) | structural distance (grows with scale / hierarchy) | **paradigm problem, requires execution-model innovation** |

**Evidence.** The static path measured ~13× slower than hand-written. The label tax (port enumeration `match`, `MachineContext`, `ProcessOutput` dispatch, `Option` checks) is **in-shape redundancy** — removing it via `StraightMachine`'s bare payload pass-through recovers to ~5–6×. The remainder is a **shape difference**: batched relay (each input moves through 5+ intermediate `Vec`s) versus hand-written (a single out `Vec`).

### 2.3 Principle: Classify Any Performance Gap First

**Principle (classify performance gaps first).** Any performance gap is classified before being addressed:

- a gap that is a **constant factor** (does not grow with scale / depth) is in-shape redundancy, fixable by patch;
- a gap that is **structural** (grows with scale / depth) is a shape difference that a patch cannot eliminate — the execution model must be innovated.

**Rationale.** The two classes demand different fixes: a patch suffices for in-shape redundancy; only an execution-model innovation removes a shape difference.

**Boundary.** The classification is determined by whether the gap grows with scale or depth, not by its magnitude alone; it applies to the gap between an abstraction path and its structurally equivalent hand-written counterpart.

### 2.4 Experimental Evidence: Streaming Shape Equals Hand-Written

`cargo bench --bench dag` (release, 100k inputs, all three semantics equivalent):

| Execution shape | vs hand-written |
|---|---|
| Batched diamond (batched relay) | ~6× |
| Hand-written loop | 1.0× |
| **Streaming pass-through (no intermediate `Vec`)** | **~1.01×** |

The streaming shape is isomorphic to the hand-written one — the paradigm direction (batched → streaming) is empirically supported.

---

## 3. Meta-Problems of Verification: Is the Type Set at Verification Time Knowable?

### 3.1 The Precise Criterion for Static vs Dynamic

**Question.** What exactly is "static" in static-first? Is a configuration-driven system "static" too?

**Reasoning.** It is not whether assembly happens before or after startup that determines staticness; it is **whether the set of types the system may exhibit at verification time is knowable**. By this criterion, the system divides into three layers:

| Layer | What changes | Type set at verification time | axiom's stance |
|---|---|---|---|
| Configuration-dynamic | data, parameters, topology shape | knowable (finite pattern space) | supported (`validate_deep` is its home) |
| Instance-dynamic | instance creation / removal / replacement | knowable (type space unchanged) | supported (three legal uses of `topology`) |
| Type-loading | new implementations / new port schemas | **unknown** | rejected (an adapter concern) |

**Corollary.** Configuration-dynamic is "static disguised as dynamic" — it is a **projection of data onto a known type space**, and data is an exhaustively analyzable value.

**Principle (verification criterion).** Staticness is judged by whether the set of types the system may exhibit at verification time is knowable. A configuration-dynamic system is static in this sense; type-loading is not, and is rejected.

**Rationale.** A knowable type set keeps verification finite and exhaustive; an unknown type set cannot be verified at compile time.

**Boundary.** Configuration-dynamic and instance-dynamic layers are supported; type-loading (introducing new implementations or new port schemas) is out of scope and belongs to the adapter concern.

### 3.2 The Value Boundary of Verification

**Question.** When data flow is already fixed at compile time, should source and destination still be verified at runtime?

**Reasoning.** No. Source and destination are fixed by the type system at compile time; runtime verification is redundant. **A source / destination error is a business-logic error (developer responsibility), not a legitimate reason for performance overhead.** axiom has no bus concept; there is no need to "verify whether data is marked as required by some module". The static path's `StraightMachine` is the implementation of this principle: the payload is passed bare, with zero verification.

**Principle (source / destination is a business error).** The source and destination of a data flow fixed at compile time are not re-verified at runtime. A source / destination error is a business-logic error, and re-verifying it at runtime is unjustified overhead.

**Rationale.** The type system fixes directions at compile time; runtime verification duplicates a compile-time fact.

**Boundary.** Applies to flow directions fixed at compile time. Where the type system already fixes source and destination, no runtime check is added.

---

## 4. Meta-Problems of Design Paradigms

### 4.1 Naming: Fixed-N Convenience Functions vs Combinators

**Question.** Should the static execution path be exposed as fixed-N
convenience functions (`pipeline2` / `pipeline3` / `fanout2` / `fanin2`,
"word + number") or as combinators?

**Reasoning.** The numeric suffix (stage count / arity) is conventional in the
data-flow domain, but the fixed-N family does not scale: every new arity
requires a hand-written function, and fan-out/fan-in signatures drift apart as
endpoint types multiply. The recursive combinators (`Chain` / `Diamond`)
express the same topologies at arbitrary depth with no per-arity code.

**Decision.** The fixed-N convenience functions were removed in a breaking
refactor (see [migration-0.2.md](migration-0.2.md)). The static entry points are the
combinators `pipeline_chain` / `diamond` / `feedback`, built on the `Straight`
contract (`StraightMachine` / `StraightLink` / `StraightSplit` /
`StraightMerge`) in `axiom::static_exec`. New code should use the combinators.

### 4.2 Explicit over Implicit

**Principle (explicit over implicit).** Physical decisions, initial states, and execution forms are declared explicitly rather than implicitly defaulted.

**Rationale.** Examples: `feedback` takes an explicit `initial` parameter rather than `Default`; the physical decisions in `MachinePhysicalSpec` are explicit (the `default-physical` lint opposes all-defaults); the execution form is declared by task type.

**Boundary.** Explicitness applies to what the abstraction declares — physical decisions, initial state, and execution form. It does not extend into the runtime's internal implementation choices (concrete carriers, schedulers), which remain the runtime's concern.

### 4.3 Single Source of Truth and Rebuildability

**Principle (single source of truth and rebuildability).** The consistency anchor of a system is **verifiable, replayable pure data** (blueprints, event streams); everything else is a derivation of it (projections, persistence, views). The `Projection` contract upgrades "observable ⟺ rebuildable" from a philosophical promise to a type constraint.

**Rationale.** Projections, persistence, and views are derived; a pure-data anchor keeps the system verifiable and replayable.

**Boundary.** The `Projection` contract enforces "observable ⟺ rebuildable" as a type constraint: every derivation is rebuildable from the pure-data anchor.

### 4.4 Finite Standard Practices vs Developer Freedom

The physical process is a finite choice set (§1.2); structural definitions (topology, composition) are axiom's contract capability. A "custom physical process" beyond the choice set belongs to the adapter layer — axiom does not offer "unbounded physical freedom", but **organizes physics into a finite, verifiable choice set**, decoupled from abstract declarations.

---

## 5. Unified Paradigm: Execution-Shape Isomorphism

### 5.1 Formalization

The criterion for zero-cost abstraction is **not "no extra operations", but "the execution shape generated by the abstraction is isomorphic to the hand-written shape"** — the same data-flow shape (how values flow) and the same control-flow shape (loop / call structure):

```text
t(α) = t(h) + ε, ε < 5%  ⟺  dist(Shape(exec(α)), Shape(exec(h))) → 0
```

**Principle (execution-shape isomorphism).** Zero cost means the abstraction's execution shape is isomorphic to the hand-written shape, not merely the absence of extra operations.

**Rationale.** A shape-isomorphic abstraction dissolves into the same data-flow and control-flow structure as hand-written code; a shape-different abstraction carries a structural distance that no patch removes (§2.3).

**Boundary.** Isomorphism is judged on the data-flow and control-flow shapes jointly. Streaming is not globally optimal for every task (IO-bound / throughput tasks have their own shapes); it is one standard member of the data-flow shape dimension, chosen by task type (§5.3).

### 5.2 The General Judgment Procedure (applies to any performance-gap problem)

1. Measure the gap: abstraction vs hand-written.
2. Classify: is the gap a **constant factor** (in-shape redundancy) or does it **grow with scale / depth** (shape difference)?
3. Fix: in-shape redundancy → eliminate the labels / checks / wrapping inside the abstraction (patch); shape difference → innovate the execution model so the abstraction generates a shape isomorphic to hand-written (paradigm).

### 5.3 Guidance for axiom Iteration

- The static path's execution shape is **linear streaming** (`FlowThrough` on
  `StaticChain`): all machine states are initialized once into a type-level
  tuple, values flow element-by-element through nested calls, and a single
  cleanup runs at the end of the batch — the output `Vec` is the only staging
  structure. It measures **0 allocs/msg** and $\epsilon \approx 1$–5% vs a
  handwritten loop (see [zero-cost-paradigm.md](zero-cost-paradigm.md)).
- Streaming is not globally optimal (IO-bound / throughput tasks have their own shapes); it is one standard member of the data-flow shape dimension, chosen by task type.

### 5.4 The Lower Bound of the Dynamic Path

**Meta-Problem.** Is "the dynamic tax is unavoidable" true?

**Analysis.** The dynamic path's cost is bounded below by safe Rust's
type-erasure mechanism and by the abstraction layer's `forbid(unsafe_code)`:

- Passing values across heterogeneous types requires `Box<dyn Any>`; runtime
  `TypeId` equality cannot be written in a typed way in safe Rust. This sets
  the safe-Rust lower bound at 1 `Box` + 1 virtual call per level (see
  [foundations.md §15.3](foundations.md)).
- The runtime's encapsulated `typed_slot` (`TypeId` check + bit-copy `ptr::read`
  / `copy_nonoverlapping`) removes allocation between levels of the same type:
  the fused chain measures **1.0 allocs/msg** (1 for the external input).
- The static path (`FlowThrough`, linear streaming) measures **0.000 allocs/msg**.

**Conclusion.** The dynamic tax is the mechanism cost of dynamic dispatch +
type-erasure (a virtual call ~70 ns/level, growing linearly with the number of
levels) plus the single external-input allocation of a fused chain. It is not a
per-level allocation tax on pass-through.

**Guidance.** Deep chains / hot paths prefer the static path (0 allocations +
0 dispatch); the dynamic path's "arbitrary topology" value trades against the
per-level virtual call (see [foundations.md §15.3](foundations.md)).

### 5.5 The Unsafe Strategy, Layered

**Principle (unsafe layering).** axiom divides the unsafe boundary along the abstraction / execution layers, following the standard ecosystem pattern: the Rust standard library and other de-facto standard libraries run under a layered scheme in which upper layers `forbid(unsafe_code)` and cores encapsulate unsafe; axiom's core / runtime structure follows the same pattern.

- **core (abstraction layer)**: `#![forbid(unsafe_code)]` (`src/lib.rs`). The abstraction layer's type promises stay pure — source and destination are fixed at compile time by the type system, with no unsafe intrusion. Any demand to "increase expressive power" must not come at the cost of unsafe; if it is ever truly needed, its necessity must be argued in this document and tracked as a separate item.
- **runtime (execution layer)**: **encapsulated unsafe** is allowed. The dynamic path's performance requirements (lock-free carriers, typed value passing) are implemented in the runtime layer, isolated in a single module (`carrier`, the lock-free SPSC carrier; `typed_slot`, the typed value slot), with **documented safety invariants + tests** guaranteeing a safe external interface.

**Judgment criteria.** Unsafe is allowed only at runtime encapsulation points, and only when all three conditions hold:
(i) the external interface is safe (callers write zero `unsafe`);
(ii) safety invariants are documented in the module header (preconditions + violation consequences);
(iii) invariants are covered by tests.

**Motivation.** Extreme-performance critical paths almost necessarily require unsafe (an ecosystem fact); "strict" means **clear boundaries + documented invariants + sufficient verification**, not zero unsafe. axiom's promise is **zero unsafe in the abstraction layer** (trustworthy expressiveness), and encapsulated unsafe in the execution layer in exchange for per-level zero-allocation pass-through on the dynamic path (when `TypeId` equality holds, the bit copy is an identity, hence memory-safe).

---

## Appendix: Design Principle Index

The principles in this document, unnumbered:

- **Blueprint–redemption isomorphism** — the blueprint is the product, execution is its redemption, validation is the isomorphism guarantee; reject any feature that serves neither the blueprint's truthfulness nor the redemption's verifiability. (§0)

- **Finite set of execution forms** — the physical process is a finite set of standard execution forms (data-flow × concurrency × resource), chosen at deployment time. (§1.2)
- **Classify performance gaps first** — in-shape redundancy (patch) vs shape difference (execution-model innovation). (§2.3)
- **Explicit over implicit** — physical decisions / initial state / execution form are declared explicitly. (§4.2)
- **Single source of truth and rebuildability** — the consistency anchor is verifiable, replayable pure data; everything else is derived; the `Projection` contract upgrades "observable ⟺ rebuildable" to a type constraint. (§4.3)
- **Verification criterion** — staticness is judged by whether the type set at verification time is knowable. (§3.1)
- **Source / destination is a business error** — not a reason for runtime verification. (§3.2)
- **Execution-shape isomorphism** — zero cost = the abstraction's execution shape is isomorphic to the hand-written shape. (§5.1)
- **Scheduling verifiability** — multiple writers to a shared slot are exclusive (serialized or explicitly ordered); concurrency conflicts surface at deployment time. (below)
- **Unsafe layering** — core: zero unsafe (abstraction purity); runtime: encapsulated unsafe (single points + documented invariants + tests). (§5.5)

### Scheduling Verifiability

`validate_deep`'s `analysis::shared_slot_conflicts` detects conflicts where multiple sources write the same `SharedState` / `Latest` slot (parallel write order is indeterminate) — the deployment-time form of scheduling ambiguity: typical ECS / scheduler systems warn at run time; axiom discovers it at deployment time. Conflicts can be reported by `TopologyReport`; multiple writers must explicitly serialize or declare an order.

### The Unsafe Strategy, Layered

See §5.5 — core (`src/lib.rs`) `#![forbid(unsafe_code)]` keeps the abstraction layer's type promises pure; the runtime allows **encapsulated unsafe** (`carrier`, the lock-free carrier; `typed_slot`, the typed value slot), all satisfying the three conditions: safe external interface, safety invariants documented in the module header, test coverage. Judgment: no unsafe may intrude into core; the execution layer's unsafe trades "zero unsafe in the abstraction layer" for zero-allocation pass-through on the dynamic path.

### Macro Diagnostics Are Contract

`declare_ports!`'s documentation locks erroneous usages (misspelled flow types, duplicate port names) to compile failure via `compile_fail` doctests — the macro's diagnostic quality is part of the contract and must not drift with refactoring (compile-time tests lock the macro expansion, in the same spirit as UI / macro tests in the ecosystem).

### Async-Readiness Declaration

`Machine::is_ready` (default `true`) lets a machine that needs asynchronous initialization declare when it is ready — the driver (async runtime / adapter) polls `is_ready` and does not drive before readiness (the machine-level form of declaring a lifecycle-ready phase). `axiom-runtime` (synchronous) does not wait; async adapters use this declaration.

### Controlled Shared Data

`SharedResource<T>` (`Arc<RwLock<T>>`) is a middle-ground primitive of "encapsulation + composition": default machine state has a single owner (encapsulation); data that must be shared across machines is carried **explicitly** by `SharedResource` (composition). Reads and writes go through `RwLock`: multiple readers in parallel, writers exclusive (corresponding to Scheduling Verifiability). std-only; machines that need no sharing keep zero-cost encapsulation.

---

## Conclusion

axiom's meta-problems are not "how to optimize performance", but "where exactly the boundary between abstraction and physics lies, what zero cost actually promises, and what verification should actually do". The unified answers: **physics is a finite choice set (decoupled, chosen at deployment time); zero cost is execution-shape isomorphism (not the absence of extra operations); verification is the implementation of compile-time facts (not repeated runtime checks)**. These three form the meta-criteria that guide every judgment about "performance gap / abstraction boundary / verification cost" during iteration.
