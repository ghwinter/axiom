> **Language:** English · [中文版](../zh-cn/README.md)

# axiom Documentation (English)

A formal, evolvable specification of axiom, defining its basic concepts, axiom system,
derivable conclusions, and shape. A self-contained authoritative spec: nothing outside
the crate — no external project, path, or material — and readable, reviewable, and evolvable
on its own.

## Documents

| Document | Content | Nature |
|---|---|---|
| [`foundations.md`](foundations.md) | Basic definitions, axiom system, derivable theorems (T1–T9), mathematical expressions, boundary statements | **Foundations**: where axiom's "what" comes from |
| [`core.md`](core.md) | The compile-time core `cell_core`: four constituents, blueprint-as-type, staticity declaration, compile-time verification, theory↔Rust correspondence | **Architecture**: what axiom's core is |
| [`semantics.md`](semantics.md) | The physical layer / Carrier: runtime positioning, carrier catalog, multi-physical-implementation equivalence, boundaries and open questions | **Architecture**: what axiom's physical layer is |
| [`unified.md`](unified.md) | The unified model: the substitution calculus (the one perspective), the definition–activation axis, three forms of substitution, the schema expressiveness ladder with (co)inductive proof, precise dynamic tax | **Upgraded view**: axiom's unified design beyond the "static blueprint" |

> **Theory corpus (non-normative):** [`../internal/theory/`](../internal/theory/README.md)
> holds derivation archives and meta-theory ([boundary-ontology](../internal/theory/boundary-ontology.md) ·
> [meta-foundations](../internal/theory/meta-foundations.md)), the unrealized-directions
> registry ([frontier-notes](../internal/theory/frontier-notes.md)), and the historical
> archive ([theory-archive](../internal/theory/theory-archive.md)). Not part of this
> specification; on conflicts the documents above prevail.

## Reading path

1. [`foundations.md`](foundations.md): the core promise, terminology
   (especially the formal redefinition of "static/dynamic"), axioms, and theorems.
2. [`core.md`](core.md): how the theory becomes a compile-time core (the four-constituent
   `cell_core`) realizing "blueprint-as-type, zero runtime object, verification at compile time".
3. [`semantics.md`](semantics.md): the core's physical-layer implementation use-case
   (Carrier), with runtime positioning and boundaries.
4. (Advanced) [`unified.md`](unified.md): the one perspective under which static/dynamic/
   plugins/loading/driver hot-plug are two binding modes of the same substitution, plus the
   definition–activation axis, schemas, the expressiveness ladder, and the (co)inductive
   proof of "future conformance". This upgrades `core.md`'s "static blueprint".

## Authoritativeness

- This directory is the formal normative reference for axiom.
- The documents retain the strict propositions and open questions required of a spec and are
  not simplified; anything "not yet converged / open" is explicitly marked (see the "Open
  Questions" at the end of each document).
- Where an extension direction is concerned (e.g. extensible physical carriers, the unified
  design's core constructors), only axiom's own shape is described, never any external project;
  proposals that are not yet implemented are kept out of this formal set.

## Terminology quick reference (per `foundations.md`)

- **Static / dynamic**: refers only to whether the *structure/type plane* is fixed at compile
  time (Theorem T9), not to activity in the state/instance plane. Resizing a connection pool,
  switching configuration, or elastic scaling is "dynamic instances/states under static
  structure".
- **Wiring / connection instance**: wiring is a structural-plane (shape) relation; a connection
  instance is a value-plane (dynamic channel).
- **Blueprint**: a blueprint = a zero-sized, compile-time-fixed data type (a set of type
  parameters), not a runtime object and with no JSON/value-form intermediate.
- **Carrier**: a replaceable physical realization of "how a value flows from `A.out` to `B.in`",
  each with a different space–time cost.
- **Definition / activation**: definition = a well-formed, validated structure (type plane, zero
  cost, no runtime use); activation = embedding the definition into a run (feeding inputs, values
  flowing along causal edges, state updates). A definition may never be activated (e.g. skipped by
  `if false` → runtime 0) — axiom core is the algebra of definition (potential).
- **Schema / loadable typed hole**: schema = a closed diagram grammar (interface kinds closed,
  provable by (co)induction); a loadable typed hole = interface fixed, inhabitant replaceable at runtime.
  A typed hole constrains kind, not count (unbounded count comes from recursion / the generator).

## Version and Stability Policy (A5)

- `cell_core` (the compile-time core) = stable: semantic changes are constitution-level
  decisions (§8.3 closed-boundary checklist); semantic regressions are rejected. New
  combinators may be added only as instances of concepts 1–5 (collective ruling if in doubt).
- Runtime modules carry per-module `Stability` markers: `stable` (carrier basics, flow
  drivers), `experimental` (obligation/contract/system under active constitution work).
  A `stable` marker is the only bettability promise before 1.0: third parties may pin
  against it (subject to the versioning rule below); `experimental` modules carry no
  bettability promise and may change in any minor release.
- **Versioning**: before 1.0, breaking changes bump the minor version (SemVer); each
  breaking change ships a concept-migration note (which names moved, which semantics
  shifted, where the concept lives now) even without a compatibility layer.
- `forbid(unsafe_code)` persists: if unsafe ever becomes necessary, it is isolated into a
  dedicated feature with a documented obligation proof (modality ④ exhibition), never into
  the stable core.
