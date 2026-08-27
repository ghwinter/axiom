> **Language:** English · [中文版](../zh-cn/unified.md)

# The Unified Model: Substitution, Schemas, and the Definition–Activation Axis

> **Nature**: the **unified-view volume** of axiom's formal specification. Answers "what is the
> single perspective under which static composition, plugin/loading systems, and driver hot-plug
> are all one thing", and upgrades the account of what axiom is, beyond the "static
> blueprint" of `core.md`. Builds on `foundations.md` (definitions/axioms/theorems) and
> `runtime.md` (the physical layer).
>
> **Authoritativeness**: a self-contained normative spec — depends on nothing outside the crate.

---

## 1. One perspective: substitution over a typed interface algebra

> **Software = a diagram over a typed interface algebra.** Its nodes are *interfaces* with
> typed holes; its edges are *T1-legal wirings*; *building/running* = substituting each typed hole with a
> conforming inhabitant. The legality of every substitution is a type judgment (T1).

Under this one perspective the apparent opposition "static graph vs dynamic loading" dissolves:

- **Static** = substitution performed at **compile time**, **universally quantified** over the
  interface (parametric; provable once; zero cost);
- **Dynamic** (plugins, code loading, driver hot-plug, hot reload) = substitution performed at
  **runtime**, **existentially instantiated** (a future inhabitant that conforms to the interface).

They are **two binding modes of the same substitution operation**, not two mechanisms.

**Theoretical correspondence** (this is where the unified view is rigorized):
- **Operads / symmetric monoidal categories** (Fong–Spivak): composition is associative,
  well-typed substitution (already axiomatized as W1/W2).
- **Polynomial functors / containers**: a container F(X) = Σ_{s∈S} X^{P_s} — **shapes S are closed**
  (interface kinds, sealed at compile time) while **positions P_s are filled by X** (instances,
  arbitrary). Substitution is the fundamental operation.
- **Dependent type theory** (D1): binding/instantiation = the judgment "inhabitant a : interface A".

---

## 2. Two levels and an orthogonal axis

### 2.1 Schema (closed) vs instance (open)

- **Schema / type plane**: interface kinds, protocols, wiring rules — **finite, closed, provable at
  compile time**.
- **Instance plane**: **unbounded count** of instances.

A "future system that is legal" is **not a new kind of legality**; it is the *same* T1 legality
applied to instances **through the schema**. Logical closure is about the schema (shape), instance
openness is about count — never conflate them.

### 2.2 Definition ↔ activation (the orthogonal axis)

- **Definition** (potential): exists as a well-formed, validated structure — no runtime use, not in
  time; it lives in the code/type plane. `Blueprint`/`Static`/schemas/typed holes are **definitions**
  (zero-sized, compile-time typed, T1-provable).
- **Activation** (actual): embedding the definition into a run — feeding inputs, making values flow
  along causal edges ("the connection's time-causality" takes effect), updating state. **Only
  activation consumes runtime/objects/time/resources.**

The two are **independent**: a graph can be **defined and validated yet never activated**
(`if param { drive(..) }` skips it → runtime = 0; an engine script never loaded). This makes
"defined-but-bound-and-not-driven" and "validated-but-not-yet-loaded" **the same thing** under this
axis: both are *validated potential, not yet started*.

Every concrete system is a point in the 2D space (binding modality) × (activation), and all four
corners are the **same typed potential graph**:

| | active | inactive |
|---|---|---|
| **static** (∀, compile-time) | compiled running graph | `if false` / not driven — runtime 0 |
| **dynamic** (∃, runtime) | plugin loaded & running | validated but not yet loaded |

> **Consequence**: axiom core is the **algebra of definition (potential)** — a
> well-formed, type-legal, zero-cost-awaiting-activation graph. **Activation is the run/carrier
> side (actual).** "Definition carries no commitment to activation" is exactly why legality can be
> proven at compile time for free.

### 2.3 Expressing "future-exists" content: interface × conformance × existential holder

"Future content" (a module/piece that is *legal now but present only at* runtime) is **not
expressed by naming it** — it is expressed by a triple:

1. **The interface (universal ∀)**: the typed hole's port signature `(In, Out)` and protocol — a
   *closed* contract that **any** future inhabitant must satisfy (`∀ T : Interface`). This is the
   compile-time, parametric part.
2. **Conformance (T1)**: the rule that only a conforming type may fill the typed hole
   (`T : PortCell<In, Out>` ⟹ `Conforms<Slot<I,O>>`) — decided **once at compile time**
   (logical closure). axiom adds what registry/device-tree-style loading lacks: an inhabitant that
   does not conform *cannot even be registered* (compile error / refused at the seam).
3. **The existential holder (runtime ∃)**: the typed hole is **held as an existential** (type-erased
   `Box<dyn …>` / function pointer), so a concrete filler is an **∃-witness bound at runtime** —
   never a compile-time name.

Algebraically this is **substitution into a variable / hole**: an open term `C[x]` with a hole of
type `I`, plus the rule "substitute any term of type `I`"; the **hole's type and the rule are
compile-time**, and the **substitution (which concrete inhabitant) is the runtime-existential**. The
kernel/device-tree mechanism has the same shape (fixed ops/ABI + runtime data-match + registration;
the future driver is never named, only received through the interface); axiom's typed version merely
adds a compile-time conformance verdict.

> **This is the unification of `Wire` / `Slot` / `SlotDrive`**: they are the *three binding states
> of one typed-hole-substitution* — `Slot` (the hole, unbound / definition), `Wire` (the hole filled by a
> compile-time-known inhabitant → zero cost), `SlotDrive` (the hole filled by a runtime-existential
> inhabitant → dynamic tax at the seam). "Future content" = interface(∀) × conformance(T1) ×
> existential holder(∃); the difference is only *when/with-which-modality* the one substitution
> binds, never a different operation.

---

## 3. Three forms of substitution; typed holes constrain kind, not count

| Form | Binding | Meaning | axiom today |
|---|---|---|---|
| **① Static combination** | compile-time (∀, parametric) | known interfaces composed into a fixed topology | **core** (Wire/Chain/Broadcast/Merge/Static) |
| **② Loadable typed hole** (∃) | runtime, one inhabitant | interface fixed, inhabitant replaceable | **core** `Slot`/`Conforms` (definition) · runtime `SlotDrive` (existential binding · ∃ activation) |
| **③ Generative/recursive schema** | schema closed at compile time; instances unbounded | a finite closed schema F yields an unbounded instance net | **core** `Rep<N,C>` (static star, bounded N) · runtime `drive_seq` (unbounded) |

> **Status note**: ② and ③ are embodied. The **definition** side (core `Rep<N,C>` bounded power `Cⁿ`,
> `Choice`/`Opt`, `Slot`/`Conforms`, compile-time T1) and the **activation** side (runtime
> `SlotDrive`, `drive_seq`, `bounded_pump`) are implemented, verified, and green. The *algebraic
> (mutually recursive)* schema layer needs **no new core combinator**: recursive/mutually-recursive
> diagrams are realized by user-defined recursive `PortCell` types composed with the existing
> combinators (all T1-verified and composable); *unbounded generative unrolling* is the
> ∃/physical side (`drive_seq`/bounded pumps) — see §4.1.

- **② and the wall**: runtime "modification" is *filling/content* substitution within a
  **compile-time-closed interface** (T1 dual pairing + T5 behavioral equivalence + A2 shape–content
  separation). You can replace *what fills a typed hole*, never *the interface/shape itself* —
  **because interfaces, addresses, ABI, and protocols are fixed, dynamic loading is possible**
  (§5.9 of `foundations.md`).
- **③ and the "typed hole implies finite" worry**: a typed hole constrains **kind**, not **count**.
  Unbounded count comes from **recursion**: a tree = the fixed point F(F(F(…))) of a finite schema
  F (a polynomial functor). Every position is still a typed hole; unboundedness is the generator's
  reach, not a finite typed-hole budget. So "connect arbitrarily many protocol-conforming devices" is
  expressed by ③, not by multiplying typed holes.

---

## 4. Schema expressiveness ladder, and the algebraic proof of future conformance

### 4.1 The ladder

| Level | What | Decidable | Place |
|---|---|---|---|
| **0 finite** | bounded non-recursive combinations | yes (exhaustive) | pipelines, fixed typed-hole gluing |
| **1 regular / star (Kleene)** | kinds bounded, counts unbounded | yes (induction/automata) | **regular-tree / free monad / algebraic species**; "regex-like" |
| **2 algebraic** | mutually recursive node kinds | yes | context-free / algebraic species (AST-like) |
| **3 general graphs** | arbitrary sharing/loops/dynamic topology | **undecidable (in general)** | graph grammars / transformations |

"Future-conforming module" with unbounded reach lives at **level 1 (regular-tree / Kleene star /
free monad)** — the "regex-like" intuition, made rigorous: kinds bounded like a finite alphabet,
counts unbounded like `a*`. Levels beyond regular (algebraic, general graphs) do exist; axiom's
**compile-time-provable schema classes are capped at regular/algebraic**, and general graphs fall
to the physical/verification boundary (the explicit exception of T9).

### 4.2 Proving future conformance algebraically

A future module = an **arbitrary derivation** of the schema grammar (whose derivation rules **are**
the interface/type rules, T1). All derivations are proven well-typed — not each one:

- **Finite unrollings**: **structural induction** over the recursive structure (initial algebra /
  least fixed point) — well-founded, so no enumeration; validity is a universal argument over depth/
  length.
- **Potentially infinite / reactive**: **guarded coinduction / bisimulation** (the T5 side).

> **Schema as grammar**: a schema is a grammar whose rules are the interface/type rules; "future
> modules are legal" = the grammar is a well-typed scheme whose derivability is proven by
> (co)induction over structure once — **logical closure, not instance closure.**

---

## 5. Precise dynamic tax

The dynamic tax is the physical cost of a **dynamic seam** (any connection whose value must cross a
compile-time-unknown implementation):

1. **Indirection/erasure** (per touch): erased box / function pointer / vtable + dynamic dispatch,
   or FFI/ABI symbol resolution. Time: indirect call (no inlining, possible branch-predict miss).
   Safe-Rust lower bound **≥ 1 alloc + 1 vtable** (T7).
2. **Load/unload** (one-time amortized): mapping code into the address space (relocations, symbol
   resolution, PLT/GOT), unloading (refcount to zero, quiesce, deregister).
3. **Lifecycle/refcount** (persistent): dynamic ownership needs register/deregister, refcount,
   teardown.
4. **Forgone optimizations** (implicit time+space): cannot inline/monomorphize across the boundary
   or dead-code-eliminate unselected implementations; all candidates stay resident.
5. **Space**: resident candidate implementations + dispatch structures.

> **Measured instance (C9, `benches/dynamic_tax.rs`, stable readings)**: for the
> `SlotDrive` erased seam the per-touch tax decomposes exactly as item 1 —
> **function-pointer indirect call + `downcast_mut` type-id compare ≈ +2.0 ns/op**
> over a hand-written baseline (noise floor 0.1–1%); `Seat` adds ≈ +0.25 ns/op for
> the generation compare; **swap ≈ 30 ns** including one heap allocation. The
> static-generic channel (`drive_link<Inline>` with `#[inline(always)]`) sits at
> the noise band across builds. These numbers are the budgetable form of items
> 1/3: per-touch tax is O(1) and predictable; load/unload dominates only when
> swaps are frequent.

**Neutrality (why axiom stays sound)**: the dynamic tax is a function of the **physical boundary
mechanism**, not of axiom's abstraction. axiom neither creates nor inflates it, and by keeping the
non-dynamic majority static, **localizes it to the seam**. axiom's zero-cost promise
(⟦α⟧ ≡ cost of an equivalent hand-written program using the same mechanism) is unaffected.

---

## 6. What this means for axiom (current statement)

- axiom core is the **algebra of definition (potential)**: it now realizes three fragments —
  the **static fragment** (① compile-time composition), the **typed-hole definition side**
  (② `Slot` + `Conforms`, zero-sized, provable), and the **activation/physical side**
  (runtime carriers and drivers; activation by `drive` stays separate from definition).
- The **unified design is landed** as two kinds of core constructors (all still *definitions* —
  zero-sized, fixed at compile time; activation remains separate):
  - **② loadable typed holes**: core definition side `Slot<I,O>` with the unified judgment
    `Conforms<Slot<I,O>>` (interface fixed and sealed, parametrically verified as
    `∀ T: PortCell<In=I,Out=O>`); runtime activation side `SlotDrive<I,O>`
    (∃ existential fill: install/swap/drive).
  - **③ schema / grammar constructors**: `Rep<N,C>` (exactly-N self-composition, the power
    `Cⁿ`; alias `Repeat<N,C>` — a literal-honesty ruling avoids the Kleene-star name), `Choice<A,B>` (sum), `Opt<C>` (optional). Unbounded count belongs to
    the activation side (runtime `drive_seq`); mutually recursive schemas are expressed by
    user-defined recursive types plus existing combinators — no new constructor needed.
    ② and ③ together make static and dynamic **two binding modes of the same substitution
    inside the core**, rather than static-in-core + dynamic-at-physical.
- **Boundary**: full general dynamic graphs are not compile-time-provable → physical/verification
  boundary (the explicit exception).
- **Characterization**: axiom is not "a system that composes one static graph"; it is **the algebra of
  definition** — a typed-substitution calculus whose objects range from a single static graph up to
  *generative schemas and loadable typed holes*, all provable at compile time and freely activatable or
  left unactivated at runtime.
