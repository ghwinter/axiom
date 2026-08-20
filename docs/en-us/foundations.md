> **Language:** English · [中文版](../zh-cn/foundations.md)

# axiom Compositional Systems Theory Foundations: Definitions → Axioms → Theorems → Mathematics → axiom

> **Nature**: axiom's **formal foundational specification**. It unifies the axioms,
> definitions, and mathematical expressions of the modern theory family known as
> "compositional systems theory" (open systems/operads, session types/π-calculus,
> dependent type theory, coalgebra, sheaf theory/lenses) with axiom's core commitment
> (**zero-cost abstraction: type erasure leaves code equivalent to hand-written code;
> runtime cost equivalent to that of an unconstrained program**) into a single rigorous
> foundation, and derives the form axiom ought to take.
>
> **Normativity**: this volume is a self-consistent, authoritative specification; axiom's
> form is derived directly from the definitions, axioms, and theorems of this volume; it
> does not depend on any project, path, or external material outside of itself.
>
> **Key position**: the theory family provides "the correct shape"; axiom's zero-cost
> commitment is "the metabolic law of shape"—it guarantees that shape does not charge the
> physical layer. Only together do the two form a complete foundation.

---

## 0. Overview: axiom's Core Commitment (Commitment Before All Else)

The core of axiom is not "being able to draw complex diagrams," but rather:

> **Commitment (Zero-Cost Abstraction)**
> For any abstraction layer structure α ∈ A, its physical implementation ⟦α⟧ ∈ P_h satisfies three conditions:
> (1) Runtime existence vanishes—there is no runtime object for α in the compiled product;
> (2) Execution time is unchanged—t(⟦α⟧) = t(h_α) + ε, h_α being the equivalent hand-written implementation, with |ε|/t(h_α) < 0.05;
> (3) Memory usage is unchanged.
>
> That is: **abstraction does not charge the physical layer; axiom's runtime cost ≡ the cost of an unconstrained equivalent program**.
> The only overhead allowed is **compile-time computation** (monomorphization, inlining, type resolution).

This definition is rigorously formalized in §3.3 of this volume (Zero-Cost Conservation Theorem ZT) and in §5 (Mathematical Expressions).
The task of this document is: to place this commitment within the axiomatic framework of "compositional systems theory," making zero-cost the **conservation law** of the entire theory family—any compositional operation, as long as it satisfies structurally-static, pure-function inlining, and no type erasure, adds no physical cost.

> **Terminological preamble (connecting to T9)**: throughout this document, "static/dynamic" always refers to **whether the structural/type plane is fixed at compile time** (see the redefinition in Theorem T9), not to the activity of the state/instance layer. State changes, connection-pool resizing, configuration switching, etc. belong to the state/instance layer; they are "dynamic instances/state under a static structure" and are not called "dynamic," nor do they negate typing.

---

## 1. Basic Definitions

### 1.0 Systems and Open Systems (Open Systems / Operads; Fong & Spivak)

**Definition (Open System)**
An open system is a quadruple S = (X_in, X_out, Y, δ), where:
- X_in: input boundary (a set of ports);
- X_out: output boundary (a set of ports);
- Y: internal state space;
- δ: behavior (state transition/computation).

The definition of an open system is "having boundaries, interacting through ports." A composed system remains an open system (recursively)—"the whole again forms a module" is thereby formalized. At which layer a certain fan-out holds is determined by the observation layer (which layer's composition is regarded as a unit).

### 1.2 Connections / Channels (Session Types / π-calculus; Honda, Milner)

**Definition (Channel / Connection)**
A connection (channel) is **a value that carries a type**: c : T ⟷ T⊥, where the two ends are dual protocols.
- Send and receive are the basic actions on a channel;
- **Mobility**: a channel itself can be passed as a value, dynamically created, and constrained (scope restriction).

> 1000 connections = 1 connection type (static) + 1000 connection instances (dynamic). The type plane does not change with the number of connections—this is the rigorous resolution of "modules are static, connection pools can scale."

### 1.3 Type-Term Bipartition (Dependent Type Theory)

**Definition (Type-Term Bipartition)**
- Type: form, universal, static;
- Term (Term a : A): instance, particular, dynamic value.

> Building on the "pattern vs instance" debate: an architecture diagram draws **statements about types**; running is **the flow of terms**. The two correspond through the "type judgment a : A" (typed judgment)—this is the bridge connecting an "architecture diagram" and a "running system."

### 1.4 State Systems (Coalgebra)

**Definition (State System as Coalgebra)**
A state system is an object X together with a structure map c : X → F(X), where F is some functor;
observation = reading observable behavior (transitions) out of X.

### 1.5 Local-Global (Sheaf Theory / Lenses)

**Definition (Local-Global)**
- Sheaf: a global object assembled from local fragments according to the **gluing condition** (sheaf condition);
- Lens: a bidirectional consistent mapping between a view and a base, satisfying the putget/getput laws.

### 1.6 Instantiation Context / Host (New Definition Added by axiom)

**Definition (Instantiation Context / Host)**
Any module instance m : M necessarily has a host context H ⊨ m, such that:
- m is created by H and carried by H, with its lifetime ⊆ H's lifetime (or explicitly managed by H);
- H may be a function stack frame, a thread, a heap allocation, a session scope, or a supervisor.

### 1.7 Wiring / Connection Instances (New Definitions Added by axiom)

**Definition (Wiring) (structural plane / shape plane)**
A wiring w ⊆ P_out × P_in is a **relation** between port bodies (able to fan-out / fan-in / form cycles / any combination).
Wiring is part of the topological shape S(c): it declares "how outputs connect to other inputs," and is a compositional declaration of **arbitrary topology**, presupposing no fixed I/O count.

**Definition (Connection instance) (value plane)**
A connection instance γ : w is a concrete dynamic channel of a wiring w: it carries a type, protocol duality, can be created and destroyed, and can be passed, carried by some host context.

> **Name clarification**: the old `LinkSpec`/`LinkKind` "connection" suggested "a fixed data link," which is misleading.
> The structural "wiring" (an arbitrary topological relation) and the runtime "connection instance" (a dynamic channel) are two different things; the former is a first-class shape, the latter is a first-class value. Henceforth the documents call the structure "wiring" and the value "connection instance."

### 1.8 Formal Redefinition of Dynamic/Static (The Three Layers of Change; Connecting to Theorem T9)

> For a long time the "dynamic/static" dichotomy has been overused: everything "active at runtime" (state changes, connection-pool resizing, configuration switching, routing changes, instance addition/removal) has been vaguely classified as "dynamic." This definition slices "change" into three layers according to **which plane is changing**, and asserts: the only thing that truly determines whether axiom must be typed is the structural/type layer.

**Definition (The Three Layers of Change)**
Let the runtime activity of a system S be divided into three layers by "the plane of change":

| Layer | Plane | What it is | Example | Requires changing code at runtime? |
|---|---|---|---|---|
| **State layer** | Value | The **content** of data is changing (the values taken by the same shape change) | Counters, parameters, connection-pool slots | No |
| **Instance layer** | Value (quantity) | The **number of instances** of the same type changes | 1000→2000 connections | No (type is static) |
| **Structural/type layer** | Type | The **form of the code** changes (acquiring capabilities that did not exist at compile time) | Installing a new plugin, swapping an implementation, dynamic topology | **Yes** |

**Definition (Dynamic/Static Redefinition)**
The "dynamic / static" dichotomy **should refer only to "whether the structural/type plane is fixed at compile time"**:
- **Static** = the structural/type plane is fixed at compile time; at runtime only state/instance-layer activity occurs;
- **Dynamic** = the structural/type plane can change at runtime (requiring an explicit loading mechanism / interpreter / value-form blueprint, paying the dynamic tax).
- Activity in the state/instance layer **must not** be loosely called "dynamic"—it is "dynamic instances/state under a static structure."

---

## 2. The Axiom System

### 2.1 Taking One Axiom from the Theory Family (Source: Part 1)

| Axiom | Conclusion | Source |
|---|---|---|
| **W1 (Composition)** | Systems can be composed via wiring into larger systems; composition is associative, unital, and symmetric-commutative | Open systems/operads |
| **W2 (Shape-Content Separation)** | Topological "shape" and "content" are two independent planes; the same shape can be filled with different content | Open systems/operads |
| **S1 (Connection as First-Class Object)** | A connection is a first-class entity that is typed, can be dynamically created/destroyed, and can be passed as a value | Session types/π-calculus |
| **S2 (Duality)** | The two ends of an established connection must be protocol-dual, otherwise rejected at construction time | Session types/π-calculus |
| **D1 (Two-Plane Separation)** | The form plane is static, verifiable, and can be unfolded at zero cost; the instance plane is dynamic and can be freely created/destroyed | Dependent type theory |
| **C1 (State as Behavior / Substitutability)** | A system's identity is determined by behavioral observation equivalence (bisimulation), not by internal structure | Coalgebra |
| **L1 (Local-Global Verification Consistency)** | If every local verification is correct and the gluing condition is satisfied, then the global is correct; views and bases do not drift | Sheaf theory/lenses |

### 2.2 The Unified Axiom Set (axiom's Compositional Foundation)

Condensing the axioms of Part 1 into axiom's own axiom set (each with its source annotated):

| Axiom | Conclusion | Source |
|---|---|---|
| **A1 Boundary** | Everything interactable = open system (port body); after composition it remains an open system | W1, W2 |
| **A2 Shape-Content Separation** | Topological shape and content are independent; the same shape can be filled with different content | W2 |
| **A3 Wiring-Connection Bipartition** | Wiring (structural plane) is a shape of arbitrary topology; connection instances (value plane) are dynamic channels | S1, S2, B2 |
| **A4 Two-Plane Separation** | The type plane is static; the instance plane is dynamic; the judgment a:A bridges the two | D1 |
| **I1 Every Instance Must Be Carried** | Any module instance exists in some host context H, with lifetime following H | I1 |
| **A5 Behavioral Substitution** | Substitutability = behavioral observation equivalence | C1 |
| **A6 Local-Global Consistency** | Local verifications glue into a global one; views and bases do not drift | L1 |
| **Z1 Zero-Cost Conservation** | Compositions satisfying (structurally static ∧ pure-function inlining ∧ no type erasure) do not charge the physical layer | Axiom 15 formalized |
| **B2 Wiring-Connection Bipartition** | The structural plane has only wiring (permanent shape declarations); only the value plane has connection instances (transient values) | S1, S2, A3 |
| **V1 Inter-Layer Independence** | The state layer and the instance layer are always dynamic and independent of whether axiom is typed; only the structural/type layer requires "changing code at runtime" | T9 |

### 2.3 Supplementary Axioms

**Axiom I1 (Every Instance Must Be Carried)**
There is no running module instance without a host.
- Type plane = {module class M}, static, always existing;
- Instance plane = { m : M, H ⊨ m }, dynamically created/destroyed;
- "Modules are always dynamically created" (physically true: every instance must be created on the stack/heap/thread and disappears with its host)
  and "module types are static" (abstractly true) **do not contradict**—they belong to two separate planes, bridged by the "instantiation judgment m:M ∧ H⊨m."
- Corollary: the instantiation context is a first-class concept declarable in a blueprint (stack/thread/heap/session/supervisor are all hosts).

**Axiom B2 (Wiring-Connection Bipartition)**
- The structural plane has only wiring (the arbitrary topological shape of composition)—permanent shape declarations;
- Only the value plane has connection instances (concrete channels)—transient values, created/destroyed with their host and purpose;
- A single wiring w can be instantiated into 0..N connection instances (the physicalization of fan-out).

**Meta-Axiom M1 (Everything Through Ports)**
Any "free variable" that does not belong to the internal state of some open system and does not interact through ports does not exist at the structural level—it is a physical-layer implementation detail.

---

## 3. Theorems (Conclusions Derivable from the Axioms)

> Notation carries over: system c = (S, Γ_in, Γ_out, δ), δ: S×Γ_in → S'×Γ_out; wiring w ⊆ Γ_out×Γ_in.
> Each theorem is annotated with its **premises** and **necessity**; axiom's form is determined by these derivable conclusions.

### T1. Composition Legality = Type Judgment
**Premises**: δ accepts typed input and produces typed output; wiring connects one system's output to another system's input.
**Derivation**: for legality, the former's output type must fall into the latter's input type (consumable by δ). If the types do not match, the composition is meaningless.
**Formalization**: wiring w : (c₁.p → c₂.q) is legal ⟺ τ(p) ⊑ τ(q) (output type consumable by input type).
**Necessity**: necessary (typed input must be supplied by typed output).
**Corollary**: a system's "type" = its port signature (Γ_in → Γ_out); composition = a partial correspondence between signatures; "protocol duality" follows from this rather than from convention.

> **axiom realization**: `Link<A, B>` requires `B::In == A::Out` (`src/cell_core.rs`)—if the types do not match, the type cannot even be instantiated, and illegal connections are rejected at compile time. See `../en-us/core.md`.

### T2. Composition Forms an Operad Structure
**Premises**: composition proceeds via wiring, and the result after composition is still a system (composable again).
**Derivation**: recursion forces ①associativity, ②identity (empty wiring as id), ③symmetry (order of parallel branches is irrelevant).
**Formalization**: (c₁ ⊗ c₂) ⊗ c₃ ≅ c₁ ⊗ (c₂ ⊗ c₃); c ⊗ id ≅ c; c₁ ⊗ c₂ ≅ c₂ ⊗ c₁ (transposition).
**Necessity**: necessary (a direct consequence of nestable composition).
**Corollary**: composition forms a symmetric closed structure (an operad); any wiring diagram can be rewritten into a canonical form; composability is a decidable formal language.

> **axiom realization**: the combinator `CellChain<A,B>` is still a port body and can be nested at any depth.

### T3. Well-Definedness of Cycles Is Ascribed to the Physical Carrier; the Abstraction Layer Only Declares Causality (Self-Consistency Theorem · Amendment)
**Premises**: δ: S×Γ_in → S'×Γ_out; wiring consists of directed causal edges (A.out → B.in).
**Derivation**: the KPN proof—as long as nodes are deterministic + channels are FIFO + blocking-read, the network is automatically well-defined and deterministic, and cycles are unremarkable; causality manifests through data arrival order (channel FIFO). Cycles require **no Delay declaration whatsoever** at the abstraction layer.
**Formalization**: abstraction layer G = (C, Γ, W), W = directed causal edges (no timing marks). Whether cycles are well-defined is decided by the **physical carrier**, unrelated to the abstraction layer:
- Carrier = channels/queues (Kahn-style, buffered, asynchronous) → cycles are naturally well-defined, zero abstraction constraints;
- Carrier = inline direct call (zero-buffered, synchronous) → cycles are synchronous algebraic feedback loops, requiring a Moore guarantee.
**Necessity**: necessary (the determinacy theorem of KPN).
**Corollary**: the Moore machine and channel buffering are not "Delays of the abstraction layer," but rather **two means by which the physical carrier realizes causality** (state isolation / tick isolation). Timing belongs to the runtime (physical layer) and is declared by the carrier.

> **axiom realization**: `Feedback` expresses causal closure of cycles at the type layer; whether a cycle is well-defined and whether buffering is needed is the carrier's (the runtime's) responsibility. See `../en-us/core.md` and `../en-us/runtime.md`.

### T4. Verifiability Decomposes ⟺ Wiring Is Purely Connectional (Composable Verification Criterion)
**Premises**: the property of a composed system = component properties + interactions introduced by wiring.
**Derivation**: if each wiring only moves data (no state, no side effects), then the properties each component holds on its own automatically hold after composition.
**Formalization** (positive): on a purely connectional P, ∧_components c_i satisfying φ(c_i) ⟹ φ(⊗ c_i) (verification can be localized).
(Contrapositive): if verification is not decomposable (locally correct but globally wrong), then ∃ wiring that is not purely connectional (carrying state/side effects).
**Necessity**: necessary (both directions are logical consequences of pure connectionality).
**Corollary**: "pure connectionality" is a necessary-and-sufficient condition for composable verification; designs with stateful wiring must explicitly pay the cost of "verification cannot be localized."

### T5. Substitutability = Behavioral Equivalence (Bisimulation)
**Premises**: systems have state and transitions; the outside observes only through inputs/outputs.
**Derivation**: a system's observable identity = its output behavior on arbitrary input sequences (including state observations). If two systems are behaviorally indistinguishable, their compositional roles are fully substitutable.
**Formalization**: c₁ ≃ c₂ (substitutable) ⟺ ∀ environment E (external wiring), obs-behavior(⟨c₁,E⟩) = obs-behavior(⟨c₂,E⟩) (bisimulation).
**Necessity**: necessary (a direct consequence of the outside being able to distinguish only through ports).
**Corollary**: the substitutability criterion is behavioral equivalence, not structural isomorphism—the theoretical basis of provider/consumer and adapters.

### T6. The Same Abstract Composition Can Have Multiple Physical Implementations (Multiple Physical Implementations Theorem)
**Premises**: the abstraction layer and the physical layer are separated (axiom's core commitment); composition is declared at the type layer.
**Derivation**: an abstract composition does not "exist" before the physical layer, so the same logical wiring diagram can be realized by different carriers/physical compositions with unchanged semantics.
**Formalization**: ⟦α⟧_Π₁ and ⟦α⟧_Π₂ are semantically equivalent (for any physical implementation Π, the physical behavior of α is consistent).
**Necessity**: necessary (logical consequence of two-plane separation).
**Corollary**: adapters/translators are a necessity, not an option; multiple physical implementations = a direct consequence of two-plane separation.

> **axiom realization**: the same `cell_core` blueprint can be implemented by carriers such as `InlineCarrier`/`QueueCarrier`/`ChannelCarrier` at different spatiotemporal costs while remaining semantically equivalent (`../en-us/runtime.md`).

### T7. Static Composition Is Zero-Cost; Dynamic Composition Must Pay a Tax
**Premises**: the type plane (composition fixed at compile time) and the instance plane (composition instantiated at runtime) are separated; zero-cost conservation Z1.
**Derivation**: determinable at compile time → monomorphization/inlining → zero cost; determined only at runtime → type erasure, and under safe-Rust at least one allocation + one dispatch/stage.
**Formalization**: Z(α) ⟹ t(⟦α⟧)=t(h_α)+ε; ¬Z(α) ⟹ t(⟦α⟧) ≥ t(h_α) + (1 alloc + 1 vtable)/stage.
**Necessity**: necessary (including the lower bound; a direct consequence of the two planes).
**Corollary**: static free, dynamic taxed is not a preference but a necessity; the lower bound of the dynamic tax is computable.

### T8 (A Structural Choice, Not Necessary)
**Instantiation count N (connection-pool scale / wiring instance count)** being a static declaration or free runtime generation is not decided by T1–T7; it must be separately committed.
- If N is static: the blueprint hard-codes the instance count, belonging to the type plane;
- If N is dynamic: an evolution of the host context's internal state (what expands is the instance count within the host, not the type plane).
**Property**: a choice, not a theorem; it decides onto which plane "dynamically expanding the connection pool" falls.

### T9 (The Three Layers of Change + Paths to and Scarcity of Structural Self-Modification)
**Theorem T9 (Paths to and Scarcity of Structural Self-Modification)**
A system "modifying itself" does not proceed through "modifying running instructions," but through:
1. **The data-selection path** (most common): the code contains all paths, and at runtime data selects one of them—not self-modification, but self-selection;
2. **A loading mechanism** (dylib/WASM): acquiring new capabilities that did not exist at compile time—genuine structural modification, requiring an explicit mechanism;
3. **An interpretive virtual machine** (JIT/DSL evaluation): executing "code" as data—requiring an embedded interpretive loop;
4. **Self-rewriting instructions**: theoretically possible (Turing machine), almost never used in engineering (risk of inconsistency).
Observation and necessity: the vast majority of "dynamic systems" are of the first kind (self-selection); genuine kinds 2/3 (structural modification) in Rust (AOT, statically typed) must be explicitly introduced and are **rare**.

**Corollary T9a (axiom's Default Coverage)**
The coverage within which axiom's typing holds = "state/instance dynamic + structural static"—this exactly covers connection pools, configuration, routing, elastic scaling, and the **vast majority** of real complex systems. The few loading-based systems that genuinely need "structural dynamism" (changing code at runtime) are handled separately as **explicit exceptions** (plugins/loading mechanisms), without downgrading all blueprints.

---

## 4. Mathematical Expressions

### 4.1 Graph Model of a System

Let system G = (C, Γ, W):
- C: the set {c_i}, where each c_i is an open system (port body);
- Γ: the full port set, each port p ∈ Γ belonging to a unique c_i, with type τ(p);
- W: the set of connections (lines)—**directed causal edges**, a many-to-many relation w : P_out → P_in, w ⊆ Γ×Γ, paired by duality.
- Note: G contains **no timing component**—the abstraction layer only declares causality (arrows with heads); timing/delay are physical-carrier attributes (see T3).

**Formula (many-to-many is not a tree)**
fan-out(c, p) = { p' ∈ Γ : ∃w, (p,p')∈w }, |fan-out| = N requiring not N Tees.
fan-in(c, p) = { p' ∈ Γ : ∃w, (p',p)∈w } likewise.

### 4.2 Composition (Operads)

The composition operator:
> c₁ ⊗w c₂ = (c₁ and c₂ composed via wiring w)
satisfying associativity, identity, and symmetry:
> (c₁ ⊗ c₂) ⊗ c₃ ≅ c₁ ⊗ (c₂ ⊗ c₃);  c ⊗ id ≅ c;  c₁ ⊗ c₂ ≅ c₂ ⊗ c₁

**Shape-Content**
S(c) = the shape projection (port-connection structure), V(c) = the content projection (the concrete system of each port body).
> c₁ ≅ c₂ (structural isomorphism) ⟺ S(c₁) ≅ S(c₂) ∧ V is equivalent port by port

### 4.3 Type-Term and Zero-Cost

**Static monomorphizable condition Z**
> Z(α) ⟺ (the full set of type parameters is known at compile time) ∧ (the transformation function is pure and inlineable) ∧ (no runtime type erasure)

**Zero-Cost Conservation Theorem ZT**
If α = c₁ ⊗w c₂ and Z(c₁), Z(c₂), Z(w), then Z(α) (composition preserves zero-cost):
> t(⟦α⟧) = t(⟦c₁⟧) + t(⟦c₂⟧) + ε, ε/n < 0.05
> Proof: monomorphization still occurs at the composition site (the composite of type parameters is known); inlining can still be performed across the composition boundary; with no type erasure there is no additional allocation. ∎

**Dynamic tax lower bound** (necessity)
If the structure can only be determined at runtime (non-Z), each stage under safe-Rust costs at least:
> 1 heap allocation (Box<dyn Any> type erasure) + 1 dynamic dispatch (dyn virtual call)
Reference measurement (3-stage chain): static 0.000 allocs/msg vs dynamic lock-free fused 1.0 allocs/msg.

**Dynamic tax (precise decomposition)**
The dynamic tax is the physical-boundary cost of a **dynamic seam** (any connection whose value must cross a compile-time-unknown implementation):
1. **Indirection / erasure (per touch)**: erased box / function pointer / vtable + dynamic dispatch; or FFI/ABI symbol resolution. Time = indirect call (no inlining, possible branch-predict miss); safe-Rust lower bound ≥ 1 alloc + 1 vtable (above).
2. **Load / unload (one-time amortized)**: mapping code into the address space (relocations, symbol resolution, PLT/GOT); unloading (refcount to zero, quiesce, deregister) — the cost of "loading/unloading a driver".
3. **Lifecycle / refcount (persistent)**: dynamic ownership needs register/deregister, refcount, teardown.
4. **Forgone optimizations (implicit time+space)**: cannot inline/monomorphize across the boundary or dead-code-eliminate unselected implementations; all candidates stay resident.
5. **Space**: resident candidate implementations + dispatch structures.
**Neutrality (why axiom stays sound)**: the dynamic tax is a function of the **physical boundary mechanism**, not of axiom's abstraction — axiom neither creates nor inflates it, and by keeping the non-dynamic majority static, **localizes it to the seam**; hence the zero-cost promise (⟦α⟧ ≡ cost of an equivalent hand-written program using the same mechanism) is unaffected. See [`unified.md`](unified.md) §5.

### 4.4 Wiring Algebra and Connection Instances

**Wiring (shape, structural plane)**
Let wiring w ⊆ P_out × P_in be a **relation** from output ports to input ports (many-to-many). Protocol-duality judgment:
> dual(τ(p), τ(q)) = true ⟹ w is legal; otherwise rejected at construction time.
> τ(p) ⊥ = τ(q): the protocol dual of a send port is a receive port.
The fan-out/fan-in of a wiring are **relational** properties, not a fixed I/O:
> fan-out(w, p) = { p' : (p,p') ∈ w }, |·| = N requiring not N Tees.

**Connection instance (value, instance plane)**
A connection instance γ : w is a transient channel of wiring w, carrying a type, duality, and host:
> γ : w ⟹ τ(γ) = the type of w, the dual end is legal, and ∃H, H ⊨ γ.
> A wiring w can be instantiated into {γ₁, ..., γ_N} (N decided by host and purpose, dynamic in the value plane).

**Instantiation (bridging the two planes)**
> m : M ∧ H ⊨ m — module instance m belongs to class M, carried by host H.
> Type plane = {M, w} (static, verifiable, zero-cost unfolding)
> Instance plane = {m:M, γ:w, H⊨m, H⊨γ} (dynamically created/destroyed)

---

## 5. What axiom Ought to Be (the Form Derived from §1–§4)

> This section deduces the conclusions of the axioms and theorems into the form axiom as a system ought to take. This is the formal answer to "what axiom should be."

### 5.1 Two-Plane Separation Is the First Structure
From A4 / D1 / I1 / B2: axiom is naturally a two-layer structure of **type plane + instance plane**. The type plane is static, verifiable, and can be unfolded at zero cost; the instance plane is dynamic, created/destroyed with its host. Any "module/connection/instance" simultaneously has both a "class (static)" and an "instance (dynamic)" side, bridged by the instantiation judgment m:M ∧ H⊨m.

### 5.2 Zero-Cost = Compile-Time Folding, and a Conservation Law
From Z1 / ZT / T7: axiom's runtime cost can (and must) only equal the cost of a hand-written equivalent program, the difference arising only from compiler-optimization noise. The only extra overhead allowed is **compilation time** (monomorphization, inlining). This requires axiom's core to be a **compile-time model**: "intelligence" is exhausted at compile time, the compiled product is ordinary Rust, with no runtime axiom objects.

### 5.3 Wiring Is an Arbitrary Topological Relation; Many-to-Many Is the Norm
From A2 / T2 / §4.1: wiring is a **relation** (fan-out/fan-in/cycles/any combination), not a tree of fixed I/O. Many-to-many requires no Tee tree; the physicalization of fan-out/fan-in (copying/distributing/arbitrating) is the physical carrier's business.

### 5.4 The Abstraction Layer Only Declares Causality; Timing/Backpressure/Threading Belong to the Physical
From T3 / §4.4: the wiring of the abstraction layer is only directed causal edges, **containing no timing marks**. Delay, buffering, blocking, dropping, synchronous/asynchronous, and threading are all replaceable attributes of the physical carrier (runtime). A blueprint only declares "there is a typed causal data flow."

### 5.5 Blueprint-as-Code; No JSON / Value-Form Intermediate Layer
From T9 / §5.4: within the mainstream of compiled languages (Rust), "modifying code/topology at runtime" has no necessary universal example; engineering clearly leans toward compile-time (T9's first kind, self-selection, is dominant; genuine structural modification is rare and requires an explicit loading mechanism). Further: since blueprints are static, **there is no reason to define software using non-.rs files**—blueprints are defined directly in Rust code (types/macro invocations characterize the static graph structure), **with no need for JSON/value-forms as a first-class expression**. JSON is at most "a tool input that generates this Rust code," not a first-class form.

### 5.6 Staticity Requires Explicit Declaration
From Z1 / T7 / §4.3: not every composition must/can/ought to be monomorphized; there must be an **explicit staticity declaration**.
- The blueprint/macro explicitly marks "which subgraphs require zero cost";
- Only for declared subgraphs is monomorphization + inlining enforced and zero-cost verified (Z ⟹ unfolding);
- Undeclared subgraphs follow the ordinary Rust / carrier path (dynamic tax acceptable).
Rationale: all-or-nothing would cause compilation explosion; flexibility is necessary ("static-first + explicit exceptions").

### 5.7 The Runtime Is a Physical-Layer Implementation Use Case and Is Replaceable
From T6 / §5.4: the runtime intrudes on **instance-layer** details and does not touch the abstraction-layer topology; it is a **replaceable solution library (carrier API) + the realization of physical timing/causality** for "how values flow across connections," and is itself replaceable. A carrier can be plugged in by implementing the `Carrier` trait without changing the topology, giving the physical layer extensibility. See `../en-us/runtime.md`.

### 5.8 Semantic Downgrade: Blueprints Only Declare Abstract Data Flow (FlowKind Moved Out of the Blueprint)
From §5.4 / T4: the old Data/Control/Observe three-way semantics are **removed from blueprints**—they are not semantics of the abstraction layer but attributes of the physical-layer carrier:
- There is no "dropping" in memory/CPU: Observe is merely the physical layer deciding how much to look at and whether to look at all; Data and Control are both **data changes at an address**;
- Dropping/blocking/synchronous/asynchronous **are all physical-layer choices**; they are not blueprint semantics.
Swapping the carrier for the same blueprint changes the "dropping/blocking/synchronous" behavior; this is "deployment-time physics"—the blueprint declares structure, the carrier declares behavior, and the two are matched through the carrier contract.

### 5.9 What "runtime modification" acts on, and its boundary: loadable slots
Split "topology / dynamic" into three objects that must **not be conflated**:
1. **Shape** (interface pairs / edge types, T1 dual pairing): fixed at compile time; **cannot** change at runtime;
2. **Occupancy / content** (which implementation fills a slot, which edge is activated): **can** change at runtime — this is exactly what plugin loading, driver hot-plug, `dlopen`, and WASM loading do;
3. **Instance cardinality** (how many instances, which devices exist now): **can** change at runtime — hot-plug, connection pools, elasticity.

**The device tree / driver model is the archetype**: the tree's **schema** (kinds of nodes, interfaces, ABI) is compile-time fixed; the tree's **instance** (current device nodes, which drivers are bound) changes at runtime. A new driver = a node filling an **already-existing interface slot**. The driver must obey fixed calling conventions / ABI / interfaces — **because interfaces, addresses, and protocols are fixed, "dynamic" loading is possible**.

Therefore, the **precise meaning** of "runtime topology modification" in axiom is:

> **Replacement and activation of content / instances under a closed interface (the dual-paired types of T1 + the behavioral contract of T5).**

Runtime freedom over structure is **parameterized** by "the target interface must already be compile-time fixed"; it cannot cross that interface — that is the wall. Consequently, software with plugins / loading still has a **static host graph** — it merely declares several **loadable slots**: the mouth (ABI / protocol) is fixed, and the occupant is changeable at runtime. The dynamic boundary (type erasure / FFI / WASM / interpreter) is **localized** to where compile-time-unknown content enters, and the dynamic tax is paid only at that seam, without dragging the whole graph into dynamism (connecting to T9 / T7 and §5.5).

> **One sentence**: runtime "modification" is not in tension with static typing — it is precisely the process of "substituting content inside a compile-time-fixed interface / ABI envelope"; **because the interface is fixed, the dynamism is possible**.

---

## 6. Boundaries (Honest Declarations)

- "Absolutely free" for arbitrary programs is impossible (every computation has a physical cost); what axiom commits to is **abstraction adds no extra charge** = the cost of an equivalent hand-written program.
- Only structures determinable **only at runtime at the structural/type layer** must pay the dynamic tax (the safe-Rust lower bound is explicit; connecting to T7/T9); activity in the state/instance layer never pays a structural tax (connecting to T9).
- Behavioral equivalence (A5/E5) is the hardest item—if it is not implemented, the documentation is downgraded rather than claimed.
- **Total-function assumption (known open boundary)**: in the definition of an open system, the transition δ implicitly assumes a **total function**—an input must have an output transition. Correspondingly, axiom's `PortCell::step` is assumed to be a total transition. **"What shape a failing cell (partial function) has, and how failure propagates through composition, is not covered by any axiom or theorem"**—this is a known deviation between the theory and real programs (such as parsing errors), and by current positioning is attributed to the physical `Result` convention (see the open question in `../en-us/runtime.md`).
- **The access seam for external input sources (known open boundary)**: the documentation declares "IO is physically/carrier-replaceable," but the landing interface whereby "the external world (socket events, etc.) formally becomes the in of a causal flow" has not been formalized—see the open question in `../en-us/runtime.md`.

---

## 7. Open Questions (Awaiting Convergence)

1. Is a connection a "first-class object" or an "association of a port body"?—the engineering form decided by E already leans toward the latter (`Link` as an associated/dual type), but the two-dimensional semantics can still be re-argued.
2. Zero-cost of wiring composition: when the static path expands from a chain to multiple arbitrary subgraphs, the acceptable upper bound of monomorphization volume (compile time / code size).
3. The criterion for behavioral equivalence (A5/E5): whether it is worth implementing, or whether to first document "structural equivalence + declared behavioral consistency."
4. The relationship between sheaf-theoretic gluing (A6/E6) and existing compile-time verification: progressive composite verification vs full-expansion verification.
5. **Failure / partial function landing layer (resolved)**: **a failing cell is NOT axiomatized
   (the core's total-function assumption is untouched)**. The crux — a cell can "fail" *without
   violating totality*: `step` remains a total function `(State, In) -> Out`, where `Out` happens
   to be `Result`; "failure" is a **value** in `Out`, not divergence/undefinedness. Hence:
   - **No** need to introduce "partial function / error output port" as a new shape in the core;
   - "can fail" is expressed as `Out = Result` (a type-level convention), and **how failure
     crosses composition** is carried by **combinators**: `TryChain` (single-level short-circuit)
     and `drive_try` (runtime) — pure composition that leaves `step` total.
   - **Landing layer**: entirely "core's type layer (`Out = Result`) + runtime combinators
     (short-circuit)"; `step` stays `# Total`; see [`runtime.md`](runtime.md) §9.2. This closes
     the "total-function assumption" concern in §6 boundaries.

---

> **In one sentence**: axiom's goal = use the axioms of "compositional systems theory" (boundary, shape-content, connection-as-first-class, two planes, behavioral substitution, local-global) to build the correct shape, and use the "zero-cost conservation law" (structurally static ∧ pure inlining ∧ no erasure) to ensure shape does not charge the physical layer. The theory provides the shape, the conservation law provides the metabolism; per T9, "static/dynamic" refers only to whether the structural/type plane is fixed at compile time, while activity in the state/instance layer (connection pools, configuration, elasticity) is "dynamic instances/state under a static structure" and never pays a structural tax—honestly distinguishing "abstraction is free" from "structural dynamism must pay a legitimate tax."
