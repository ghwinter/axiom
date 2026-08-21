> **Language:** English · [中文版](../zh-cn/core.md)

# axiom Compile-Time Core: cell_core (What axiom "Should Be" · Core Volume)

> **Nature**: axiom's **core architecture specification**. It answers "what the axiom core
> should be": it turns the axioms and theorems of `foundations.md` into a **compile-time core**
> `src/cell_core.rs`. It describes the form the axiom core should take, consistent with the
> converged implementation (`src/cell_core.rs`, `src/lib.rs`).
>
> **Normative**: This volume is a self-consistent, authoritative specification focused on the
> definition of the axiom core itself.
>
> **In one sentence**: axiom core layer = **compile-time DSL + verifier**: all of its
> "intelligence" (analysis, verification, type constraints, graph construction) is exhausted
> at **compile time**; the product is **ordinary Rust code**. axiom has no "runtime" — only
> the two phases "compile time" and "post-compile". This naturally satisfies the zero-cost
> promise (no axiom objects after compilation).

---

## 1. Core Proposition

> axiom core layer = **compile-time DSL + verifier**: all of its "intelligence" is exhausted
> at **compile time**; the product is **ordinary Rust code**. axiom has no "runtime" — only
> the two phases "compile time" and "post-compile".

**Implication**: a blueprint is no longer a "runtime value", but a **compile-time construct**
(type + const + macro-generated code). Verification shifts from "done on values at runtime" to
"done at compile time by macros / types / const" — violating a blueprint rule = **compile
error** (`compile_error!`) or a failed type constraint, rather than a runtime `Result`.

---

## 2. The Four Artifacts (The Main Axis of cell_core)

`cell_core` carries **four artifacts**, corresponding to the theoretical convergence
(bridging to `foundations.md`):

| Artifact | Content | Rust Correspondence | Compile-Time Nature |
|---|---|---|---|
| **Open system / port body** | Bounded, typed input/output/state, `step` pure and inlinable | `PortCell` trait (`src/cell_core.rs`) | Type-level, no runtime objects |
| **Causal dataflow** | Directed connections: `A.out -> B.in`, dual pairing at the type layer | `Link<A,B>` | Illegal connections fail to compile (T1) |
| **Composition / nesting** | Combinators are still port bodies, at arbitrary depth | `Chain<A,B>` | Operational structure (T2) |
| **Staticness declaration** | Marks which subgraphs require zero cost | `Static<SUB>` / `Blueprint<TOP>` | Monomorphized, no `Box<dyn>` (T7/§5.6) |

**Many-to-many unified as first-class**: `Broadcast` (fan-out), `Merge` (fan-in), `Feedback`
(loop) are expressed at the type layer, with no Tee tree (bridging to `foundations.md`
§5.3/A2).

### 2.1 `PortCell` (Open System / Port Body)

```rust
pub trait PortCell: Sized {
    type In;                      // input port type (the carried value type)
    type Out;                     // output port type
    type State: Default;          // internal state
    fn step(state: &mut Self::State, input: Self::In) -> Self::Out; // pure transition
}
```

- `In`/`Out` are port types (their duality is what pairs them; see `Wire`); `State` is the
  internal state, default-constructible;
- `step` is a pure transition (`#[inline(always)]` makes inlining hold across crates → (b) of
  Z1);
- A purely abstract layer — it does **not** incorporate threading/synchronization/backpressure/
  timing; those are the concern of the physical carrier (T3 / §5.4).

### 2.2 `Wire` (Causal Dataflow)

```rust
pub struct Wire<A, B>(PhantomData<(A, B)>);
impl<A, B> Wire<A, B>
where A: PortCell, B: PortCell<In = A::Out>,   // dual pairing at the type layer
{
    pub fn fire(astate: &mut A::State, bstate: &mut B::State, input: A::In) -> B::Out {
        let mid = A::step(astate, input);
        B::step(bstate, mid)
    }
}
```

**Wiring legality = type judgment (T1)**: `B::In == A::Out` is required. If the types do not
match, this type cannot even be instantiated — an illegal connection is rejected at **compile
time** (not a runtime check). `Wire<A,B>` is a *typed position* (the unified `Conforms` object)
and the compile-time-bound composition action — the compile-time side of the substitution/binding
concept 4 (`foundations.md` §8).

### 2.3 `Chain` (Composition / Nesting)

```rust
pub struct Chain<A, B>(PhantomData<(A, B)>);
impl<A, B> PortCell for Chain<A, B>
where A: PortCell, B: PortCell<In = A::Out>,
{
    type In = A::In; type Out = B::Out; type State = (A::State, B::State);
    fn step((sa, sb): &mut (A::State, B::State), input: A::In) -> B::Out {
        let mid = A::step(sa, input);
        B::step(sb, mid)
    }
}
```

Composing A -> B (A's output wired to B's input) is **still a port body** and can be nested
again (at arbitrary depth) — the closure of concept 3 (composition closure) in `foundations.md` §8.

### 2.4 `Broadcast` / `Merge` / `Feedback` (Many-to-Many, Loops)

- **`Broadcast<SRC, R1, R2>`**: a source's output is wired simultaneously to multiple
  receivers (fan-out). The type layer enforces that all receivers' inputs match the source's
  output; no `Box<dyn>`, no runtime objects. The source output requires `Clone` —
  multi-way distribution is, at its essence, copying/dispatching at the physical layer, and
  that is precisely the physical carrier's concern; the abstraction layer merely declares at
  the type layer that "this one value flows to multiple receivers".
- **`Merge<S1, S2, DST>`**: multiple compatible sources merge into one receiver (fan-in). The
  "order" of the merge (who arrives first) is the physical carrier's concern
  (T3/Kahn) — the abstraction layer only declares the causal form "multiple sources can wire
  into the same receiver".
- **`Feedback<BODY, FEED>`**: `BODY`'s output is fed back through `FEED` into `BODY`'s input,
  forming a causal closure. The abstraction layer **only declares the existence of the loop**
  (causal closure, T3); whether the loop is well-defined and whether buffering is needed is
  the physical carrier's concern (Kahn channels ⟹ loop safety; inlining ⟹ Moore required).

### 2.5 `Static` / `Blueprint` (Staticness Declaration + Blueprint-as-Type)

```rust
pub struct Static<SUB>(PhantomData<SUB>);          // marks a subgraph requiring zero cost
pub struct Blueprint<TOP>(PhantomData<TOP>);       // blueprint = zero-sized type
pub const fn blueprint_is_zero_sized<TOP>() -> bool {
    core::mem::size_of::<Blueprint<TOP>>() == 0
}
```

- **`Static<SUB>`**: explicitly declares a subgraph as static (zero cost). Only for subgraphs
  declared static does the compiler enforce monomorphization + inlining and verify zero cost
  (Z ⟹ unfolding); undeclared ones take the ordinary Rust/carrier path (dynamic tax is
  acceptable) — "static-first + explicit exceptions" (bridging to `foundations.md` §5.6).
- **`Blueprint<TOP>`**: a blueprint = a **zero-sized, compile-time-fixed type** (a set of type
  parameters). The opposite of "value-form blueprint/JSON" (bridging to `foundations.md`
  §5.5): a blueprint is not a runtime object but a set of type parameters;
  `size_of::<Blueprint<TOP>>() == 0` — there is no blueprint object at runtime.

---

## 3. Blueprint-as-Type, No JSON / Value-Form Intermediate Layer

> **Conclusion (bridging to `foundations.md` §5.5 / 4.1)**: in the mainstream of compiled
> languages (Rust), "modifying code/topology at runtime" has no necessary universal example;
> engineering **clearly leans toward compile time**. Blueprints are defined directly in Rust
> code (types / macro invocations describe the static graph structure); **no JSON/value-form
> is needed as a first-class expression**.

**Argument**:
- Dynamic library loading (dlopen/.so) is a common "plugin" form, but what is loaded is not
  new code — a `.so` is a unit already fixed at compile time; loading is merely "connecting
  existing code + symbol binding", the type plane does not change, and it lands at the
  instance layer.
- Only true runtime generation of new code (JIT) creates new types — not the mainstream of
  non-compiled languages, and in engineering it is almost never used for reliability systems.
- The only value of "config/JSON/serialization import" is "modifying program behavior" — and
  precisely because the runtime does not change structure (T9), this value does not exist.
  JSON is at most "tool input for generating this Rust code", not a first-class form.

**Formalization (blueprint form)**: blueprint G = a typed graph defined by Rust code (open
systems + causal dataflow + port types), expanded directly at compile time; no runtime
blueprint object, no intermediate JSON form. The earlier reservation of "values as
pre-compile source" is withdrawn — the value form is not even necessary as a pre-compile
intermediate state.

---

## 4. Compile-Time Verification (Capability Exhausted at Compile Time)

```rust
// The unified T1 duality judgment: `EXPECT` is a typed position/interface —
// - `Wire<A, B>`: a wire (expects flow A.out -> B.in, i.e. `B::In == A::Out`);
// - `Slot<I, O>`: a load slot (expects an occupant with `In=I, Out=O`).
pub trait Conforms<EXPECT> { const OK: bool = true; }
impl<A, B> Conforms<Wire<A, B>> for () where A: PortCell, B: PortCell<In = A::Out> {}

pub fn assert_wiring<A, B>() where A: PortCell, B: PortCell<In = A::Out> {
    assert_conforms::<Wire<A, B>, ()>();
}
```

- **Compile-time duality judgment (unified)**: if `Conforms<Wire<A,B>>` is constructible (the
  impl exists), then that wiring is legal under the type duality — purely type-level (T1). The
  same one judgment covers slot conformance (`Conforms<Slot<I,O>>`).
- **Asserting a wiring is legal**: if it holds at compile time, a zero-sized witness is produced;
  if the types do not pair, that impl does not exist → **compile error**. The entry point "for
  analysis and verification" — verification is completed at compile time, zero runtime overhead.

---

## 5. Theory ↔ Rust Correspondence

| Theoretical Object | Rust Correspondence | Cost |
|---|---|---|
| Open system / port body | `trait` (with associated input/output port types) | compile time |
| Shape-content separation | generics (shape = type parameters, content = concrete implementation) | compile-time monomorphization |
| Connection as first-class object | connection type + session type (protocol duality) | compile time; value at runtime |
| Type-item dichotomy | `Type` (static) vs `Box<dyn ...>` or instances (dynamic) | static zero / dynamic tax |
| Composition | combinator / nested generics + recursion | compile-time expansion |
| Zero-cost conservation | generic monomorphization (monomorphization) | compile time (size for speed) |
| no_std | no runtime dependency | — |

**Key correspondence: generic monomorphization = the mechanism of zero cost**
Rust's generics generate specialized code for every concrete type at compile time
(monomorphization) — this is precisely the mechanism of zero-cost conservation. When the
topology is encoded in type parameters (combinators, nested generics), the compiler expands
it into an instruction sequence equivalent to hand-written code; one pays only when type
erasure is triggered (`Box<dyn Any>`).

---

## 6. Legacy Semantics Moved Out of the Abstraction Layer (Assigned to the Physical Carrier / Instance Layer)

To keep the core "clean", the following legacy semantics are moved out of the abstraction
layer (bridging to `foundations.md` §5.4/5.8):

- **FlowKind (Data/Control/Observe trichotomy)**: moved out of the blueprint; it is a
  physical-carrier attribute.
- **LinkKind's carrier/backpressure/timing semantics**: the concern of the physical carrier,
  not the abstraction layer.
- **Value-form blueprint / JSON / runtime value verification**: blueprint-as-code, with no
  JSON/value-form intermediate layer.
- **Threading/sync-async/timing**: the instance physical layer (T9/T3).

4. Verification is at compile time (`Conforms`/`assert_wiring`; illegal wiring fails to
   compile).
5. **Legacy semantics moved out of the abstraction layer** (§2 table + §6) do not enter the
   core types — after `cell_core` is cleaned up, only the `PortCell` family + driving +
   compile-time verification remain, zero-dependency, `#![forbid(unsafe_code)]`,
   `#![no_std]`.

---

## 6b. Unified-model Constructors (Additive)

Beyond the four constituents, `cell_core` adds, **additively** (no rewrite of existing types),
the unified-model constructors:

- **`Rep<N, C>`** — regular/star: `N`-fold self-composition of a cell `C` (Kleene `C*` with
  bounded count as a compile-time constant). `State = RepState<N,C>` (manual `Default`) zeros
  dependency on the built-in array `Default`; zero-cost, monomorphized; `N=0` is identity.
  Unbounded count (runtime) is the generative/physical side — see `runtime.md`, `drive_seq`.
- **`Slot<I, O>` + `Conforms` / `assert_conforms`** — ∃ loadable-slot **definition**: a
  compile-time-fixed interface (dual pair, T1) with a compile-time parametrically-quantified
  conformity verdict for any future occupant (`∀ T: PortCell<In=I, Out=O>` ⟹ `Conforms<Slot<I,O>>`,
  the same shape as `Conforms`). The runtime existential fill is `SlotDrive` — see `runtime.md`.
- **`Choice<A, B>` + `Opt<C>`** — the *regular* operators `|` and `?` as first-class pure
  `PortCell`s. `Choice` (input-tagged [sum]) dispatches by the input's label to `A` or `B`;
  `Opt<C>` maps `Option<C::In>` to `Option<C::Out>` (identity on `None`, one `C::step` on `Some`).
  Both are deterministic and composable like any cell (the `∃` branch-selection side remains the
  runtime `SlotDrive`).

These are **definitions** (zero-sized, no runtime object) and reuse the same `PortCell` +
`Conforms`-style compile-time verification — the elegant, additive realization of the unified
model's static fragment (see [`unified.md`](unified.md)).

---

## 7. Acceptance Criterion (Core)

```text
cargo build --lib        # zero dependency, no_std support (--no-default-features)
cargo test --lib         # 9 tests
cargo bench --bench chain   # static ≈ hand-written (zero-cost proof)
```

**Achieved (evidence chain)**:
- The four artifacts are complete, compilable, and have tests; complex topologies
  (loops/broadcast/merge) are expressed at the type layer, with no
  `Box<dyn>`/JSON/threading/FlowKind.
- Blueprint-as-type: `size_of::<Blueprint<TOP>>()==0`, zero objects at runtime (const proof).
- Verification at compile time (`Conforms` type judgment; illegal wiring fails to compile).
- After compilation, equivalent to hand-written ordinary Rust (proven by
  `examples/cell_demo.rs`).
- bench: static 51µs ≈ hand-written 49µs (zero cost), type-erasure 1.3ms (~26x dynamic
  tax) — empirically proving "static is free, dynamic must pay a tax" from T7 of
  `foundations.md`.
- `#![forbid(unsafe_code)]`, `#![no_std]` (`default=["std"]`), the core is zero-dependency.

---

## 8. Boundaries and Open Questions

- **The core is a compile-time model**: capability is exhausted at compile time, with no
  axiom objects after compilation; "intelligence" beyond compile time (e.g., linear temporal /
  graph analysis) is not a default capability of the core and must be designed separately.
- **Total-function assumption**: `PortCell::step` is assumed to be a total transition;
  "cells that can fail" are not axiomatized and are currently handled according to the
  physical `Result` convention (see the open questions in [`runtime.md`](runtime.md)).
- **Coverage of the staticness declaration**: at present `Static` marks static subgraphs via
  a type parameter; the tolerable upper bound on monomorphization size after "expanding the
  static path from a chain to any number of subgraphs" is an open question
  (`foundations.md` §7).

> **Conclusion**: the axiom core = the compile-time model of the four artifacts of
> `cell_core` (open system, causal dataflow, composition, staticness declaration) —
> blueprint-as-type, verification at compile time, and after compilation equivalent to
> hand-written ordinary Rust. Physical implementation is borne by the runtime (carrier); see
> [`runtime.md`](runtime.md).
