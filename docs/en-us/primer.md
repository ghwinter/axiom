> **Language:** English · [中文版](../zh-cn/primer.md)

# axiom Primer — read this first

> **Nature**: this is a guide volume, not a normative specification. It exists to give a
> developer (human or AI) a true first reading: what axiom is, why it exists, how to
> understand it without hallucinating, and how to think with it before diving into the
> formal volumes. It introduces no axioms, constructors, or normative obligations.
> On any conflict, the formal volumes prevail ([`foundations.md`](foundations.md) /
> [`core.md`](core.md) / [`semantics.md`](semantics.md) / [`unified.md`](unified.md)).

---

## 0. What axiom is, in one breath

**axiom is a constitution layer, not a framework.** It does not give you a system;
it gives you the typed vocabulary, compile-time verification, and replaceable-physics
seams with which to build a system that *stays* legal, explicit, and provably cheap.

The analogy axiom itself uses: Rust gives you memory safety **without** giving you the
application. axiom gives you **topology safety, explicit obligations, and physical
replaceability** without giving you the system. You write the application.

Three sentences you can rely on without reading anything else:

1. You write *what connects to what* (topology) as **types** — illegal wiring fails to compile.
2. You declare *who must do what* (obligations: deadlines, cancellation, backpressure) as
   **declared items**, not comments.
3. You choose *how waiting and I/O are implemented* (physics) by **swapping a carrier** —
   the topology does not change.

And the compile-time promise: after compilation, your blueprint is equivalent to
hand-written plain Rust, **with zero runtime objects**.

## 1. What axiom is NOT (avoid hallucinations early)

| It is not… | Because… |
|---|---|
| an all-in-one framework | no control inversion, no runtime container, no lifecycle ownership |
| an ORM / message queue / scheduler | those are *carriers* — replaceable physical realizations, not the layer |
| a runtime that executes your blueprints | a **definition may never be activated**; the core is an algebra of definitions, not of runs |
| a new programming language | it is Rust types + proc macros (one compile-time-only dependency) |
| a claim that these problems are solved | the evidence discipline (§6 of the vision) explicitly marks what is *landed*, *in flight*, and *not yet started* |

## 2. The vision, in a few lines

Four kinds of complex systems — MMO game servers, large GUI desktop apps, high-performance
databases, AI Agents — have different vocabulary but converge on the *same hard problem set*:
cancellation, backpressure, deadlines, crash recovery, deterministic replay, ownership
partitioning, context propagation.

Viewed side by side, the answer has the same shape for all four: **topology, obligations,
and physics must be separated, and declared in the same place.** axiom builds only this
cross-cutting part. Its thesis for the AI era: when code generation is nearly free, the
scarce resource is *keeping structure consistent* — so axiom's product is not a feature,
it is an invariant; axiom is a *consistency machine* (see [`vision.md`](vision.md) §4).

Honest status, as of the current changelog: the compile-time core, the obligation ledger,
and the T1–T9 code projections are **landed**; the second-implementation cross-checks
(T6, sync/async and backpressure) are **in flight**; the falsifiable-prediction list bound
to an actually-studied real system is **not yet started** — the vision says so explicitly.
Believe the landed part, verify the in-flight part, do not infer the rest.

## 3. The five concepts, explained simply

Everything reduces to five slots. If a system can decompose into these five slots without
losing meaning, it belongs to axiom's scope; if not, that distortion is the bug to study.

| Concept | Simple meaning | What it buys you |
|---|---|---|
| **State** | one cell owns one exclusive state; `step` is a pure function | ownership partition at type level — fewer locks, no default races |
| **step** | a pure, inline function: input → output (+ state) | determinism, replay, zero-cost composition |
| **Wait-point** | the only places time and I/O may enter: input-ready / deadline / backpressure | *waiting only at boundaries*; the sync/async question becomes a binding, not an architecture |
| **Carrier** | a replaceable physical realization of "how a value flows from `A.out` to `B.in`" | performance & sync/async are carrier swaps, not topology rewrites |
| **Partition** | the module boundary is a wiring boundary | directory/package structure follows from the causal graph, not from fashion |

Three more shapes from the core you will meet constantly — also simple:

- **Wire** — the type-level pair `A.out → B.in`. It is what "module A feeds module B"
  means *as a type*, so an illegal connection cannot be written (T1).
- **Chain / Broadcast / Merge / Feedback** — composition is itself a port cell; fan-out,
  fan-in, and loops are expressible at type level. "Modular / component / plugin" are not
  three features: they are three *scales of the same substitution operation* (unified.md).
- **Static / Conforms** — marking zero-cost subgraphs and verifying wiring at compile time.
  This is the source of "verification at compile time, zero overhead at runtime."

### The "algebra depth" parts, without the math

Readers sometimes bounce off the algebraic language. The translation:

- **"Blueprint as type"** — a blueprint is literally a zero-sized Rust type (a set of type
  parameters). No runtime object, no JSON intermediate. The term *sounds* abstract; the
  thing is a `struct`.
- **"Composition to any depth"** — depth is compile-time type recursion; at runtime it is
  an ordinary chain of function calls. The benchmarks show generic composition ≈ handwritten
  chains bit-for-bit (that is what "zero-cost" means here).
- **"The substitution calculus"** (unified.md) — the claim that static/dynamic, plugins,
  loading, and driver hot-plug are *two binding modes of one substitution*. You do not need
  this to use axiom; read it as a one-table idea once you are comfortable.
- **Theorems T1–T9** — these are the *why-you-can-trust-it* foundation. Each has a code
  projection. You do not need the math to **use** axiom; you need it only if you propose to
**change the constitution** (that is a constitution-level decision by policy).

## 3b. The traditional words: module / component / subsystem

Before axiom, what do these familiar words mean — and how do they map?

**The traditional meaning.** A *module* is a unit of related functionality (functions plus
state plus internal representation) exposing a limited interface; a *component* is an
independently deliverable, replaceable aggregation of modules with a clear interface
contract (interface–implementation separation); a *subsystem* is a larger functional whole
formed by several components cooperating, exposing a higher-level interface. In the
traditional reading these are all *boundary* notions — function boundary, module boundary,
component boundary, system boundary — differing only in size and rigidity.

Three things the traditional semantics assumes:

- **Collaboration** between different modules = cross-boundary calls / messages / dependencies;
- **API layers** = the interfaces each boundary exposes, ordered by dependency direction;
- **Small modules compose into large modules; base modules compose into subsystems** =
  composition — but composition is a *build-time dependency graph plus runtime call
  relationships*, and boundaries are convention and documentation, not types.

**The axiom mapping** (a concept correspondence to help you translate, not axiom's official
vocabulary):

| Traditional word | In axiom | What changes |
|---|---|---|
| module | a **port cell** (bounded In / Out / exclusive State / pure step) | the boundary is "whose value may flow into whose input", enforced by type (T1) — not "who may call whom" by convention |
| component | a port cell whose **interface–implementation split is the topology-vs-carrier split** | the same component shape swaps physics (carrier) without rewriting topology; its interface contract is the declared obligations (deadline / cancellation / backpressure) |
| subsystem | a **blueprint / composition tree** (Chain · Broadcast · Merge · Feedback) | a combinator is itself a port cell and nests to any depth — small modules into large modules, base modules into subsystems, is *composition nesting*, and the composite is still one port cell |
| collaboration between modules | **wiring (`Wire`) + a carrier** | cooperation is causal value flow `A.out → B.in`; how it flows (direct call / queue / channel / thread) is the carrier |
| API layers | **layer discipline** (core: shape → semantics: behavior/cost contracts → instances: real bindings) | each layer answers one question; wrong-layer composition is a **compile error**, not a review comment |

**The same example, two vocabularies.** Traditional: a Logging module, a Storage module, and
a Query module; Logging calls into Storage, Query depends on Storage; a design doc states who
may call whom and at which layer. axiom: Logging, Storage, and Query are each port cells;
`Logging.out → Storage.in` is a `Wire` (type-level pairing — the illegal connection cannot be
written); the Query–Storage collaboration runs through a *carrier* (choose sync direct call,
bounded queue, or tokio without touching the topology); and the whole (`Query ∘ Storage ∘
Logging`) is a `Chain` — itself a port cell — that can be embedded upward into a larger
subsystem.

**One-sentence difference**: the traditional semantics treats boundaries as *convention*;
axiom treats boundaries as *types* — the illegal connection is not prevented, it does not
exist in the language. The old disciplines (layering, dependency direction, interface
contracts) are not abolished; they are promoted from convention and documentation to
compile-time verification and declared obligations.

## 4. "I want to build a highly modular complex system, with no framework" — how to think

Suppose you must design a large, module-heavy, component/plugin-heavy system — high
performance, both sync and async, many heterogeneous features, and you want the code and
the directory tree to stay readable and maintainable. Without a framework, in what order
do you think? axiom's answer is a thinking sequence, not an API:

1. **Decompose into port cells.** For every unit, answer four questions: what bounded input?
   what bounded output? which exclusive state? is one `step` a pure function? The ownership
   partition (who may touch what, without locks) is decided here, at decomposition time.
2. **Draw the causal graph.** Whose output feeds whose input? Write it as wiring; let the
   compiler reject illegal connections. The answer to "where should my modules and directories
   break" *falls out of this graph* — a module boundary is a wiring boundary.
3. **Declare obligations.** Which edges have deadlines? Which request can be cancelled, and
   how does cancellation propagate? What happens when the upstream is faster than the
   downstream (backpressure)? Write time, failure, and rate constraints as declared items.
4. **Choose the physics last.** Same topology: inline zero-allocation? bounded channel?
   cross-thread spawned flow? tokio? Swapping the carrier changes the space–time cost, not
   the meaning. This is where performance and sync/async live.
5. **Sync and async are two bindings of the same blueprint.** Synchronous = values flow
   directly along the step chain; asynchronous = the wait-points (input-ready / deadline /
   backpressure) are contracted at the boundary and bound to a real base (e.g. tokio) by the
   instance layer. The same blueprint runs both ways — the `sql-over-redis` use case ships
   sync, async, and concurrent demos to prove it.

Where the *elegant, uniform, readable* feelings come from, mechanically:

- **Readable code**: topology is types, types are documentation. Reading the composition
  tree *is* reading the system structure.
- **Readable directories**: layering is semantics — `core` (shape) → `semantics`
  (behavior/cost contracts) → `instances` (real bindings). Each layer answers exactly one
  question, and complexity that grows into the wrong layer is a **compile error**, not a
  code smell.
- **Maintainable**: wrong-layer composition cannot be written; the structure rots only where
  the language lets it.

## 5. What axiom provides — the meta-framework, in one table

| Your ask | axiom's answer |
|---|---|
| high modularity | port cells + combinators; composition is a type |
| component / plugin / hot-load | carriers & seams are replaceable sockets; load/hot-plug = binding modes of one substitution |
| high performance | zero-cost conservation axiom + benchmark evidence; compile-time verification, zero runtime objects |
| sync **and** async | one blueprint, two bindings; wait-point contracts + instance-layer adapters |
| many heterogeneous features | five-concept closure: different systems reduce to the same slot set |
| readable code & directory tree | topology-as-type; layering-as-semantics; wrong-layer wiring is a compile error |
| no hallucinated understanding | honest boundaries: the gap ledger, open items, and "not yet started" markers |

The meta-abstract in one sentence: **axiom is an algebra of the legality of combination.**
Decades of complex-software experience are distilled into:

- a **typed topological vocabulary** (five concepts) that any system decomposes into;
- an **obligation ledger** — the six frictions the algebra cannot express (waiting, physical
  singletons, cross-cutting, residency, derived semantics, completeness) are *indexed,
  contained, and left open*, not pretended away;
- a **four-edged seam map** (Data / Control / Observe / Physical) for the runtime boundary —
  plus one surface-less crosscut seam (contexts, cancellation tokens) by design;
- a **falsifiable evidence discipline** — claims are bound to code projections, cross-checks,
  and explicit honesty markers.

## 6. Rules for reading axiom without hallucinating

1. Trust only claims that have compile-time verification or code projection. Marks like
   *hypothesis / pending cross-check* (vision §1 observation 1, §6) are real boundaries.
2. **Static / dynamic** means only whether the *structure plane* is fixed at compile time —
   never the activity of states or instances.
3. A **definition may never be activated** (e.g. skipped by `if false`). axiom core is the
   algebra of potential, not of runs.
4. See **carrier** → think "replaceable physics". See **obligation** → think "must be
   declared, then ledgered". See **seam** → think "the one place waiting/I/O may enter".
5. On conflicts, the formal volumes prevail; this primer is a guide, not a law.

## 7. Reading path

**This primer → [`vision.md`](vision.md) → [`foundations.md`](foundations.md) →
[`core.md`](core.md) → [`semantics.md`](semantics.md) → [`unified.md`](unified.md).**

- Want *what it is*: this primer, then vision (why) and foundations (precisely).
- Want *how it maps to Rust code*: `core.md`, then the crate-level README and examples.
- Want *the physical layer / carriers / sync-async*: `semantics.md` and the instance layer.
- Want *the one unified view* (plugins, loading, hot-plug): `unified.md` (advanced).
