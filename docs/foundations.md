# The Algebraic Foundations of axiom

> This document provides a formal definition of the computational model of axiom and derives its consequences from the perspectives of category theory, type theory, and systems theory. Every definition, axiom, theorem, and corollary corresponds to concrete Rust types and trait implementations in the axiom crate, so that the mapping between mathematical proof and code is verifiable.
>
> **Structure.** Each section begins with axioms (basic assumptions taken as self-evident), then defines the concepts of the domain, and finally derives theorems and corollaries. The arrow P → Q indicates that P is a proof premise of Q.

---

## Contents

0. [Physical Foundation](#0-physical-foundation)
1. [Computational Primitives](#1-computational-primitives)
2. [Ports and Connections](#2-ports-and-connections)
3. [Execution Sequences and Scheduling](#3-execution-sequences-and-scheduling)
4. [Resource Algebra](#4-resource-algebra)
5. [Composition and Categorical Structure](#5-composition-and-categorical-structure)
6. [Deployment Algebra](#6-deployment-algebra)
7. [System-Level Theorems](#7-system-level-theorems)
7.5. [Engineering Patches: The Gap Between the Mathematical Model and Its Implementation](#75-engineering-patches-the-gap-between-the-mathematical-model-and-its-implementation)
8. [Rust Mapping](#8-rust-mapping)
9. [Curry–Howard Correspondence](#9-curryhoward-correspondence)
10. [Theorem Classification and Coverage](#10-theorem-classification-and-coverage)
11. [Session Types](#11-session-types)
12. [Hybrid Systems](#12-hybrid-systems)
13. [Lifecycle Typestates](#13-lifecycle-typestates)
14. [Unified Assessment: Reduction and Coverage of Research Lineages](#14-unified-assessment-reduction-and-coverage-of-research-lineages)
15. [Zero-Cost Abstraction: Decoupling the Abstraction Layer from the Physical Layer](#15-zero-cost-abstraction-decoupling-the-abstraction-layer-from-the-physical-layer)

---

## 0. Physical Foundation

**Axiom 0.1 (Existence of the Set of Memory Locations)**
There exists a set of addressable memory locations L. Each location l ∈ L holds a value v ∈ V at time t. Write mem_t: L → V for the memory state at time t.

**Axiom 0.2 (Existence of Computational Steps)**
There exists a ternary computational step (r, w, φ), where r ⊆ L is the read set, w ⊆ L is the write set, and φ: V^|r| → V^|w| is the transition function.

**Definition 0.1 (Thread)**
A thread T is a sequence of computational steps. At the physical layer, a thread is equivalent to a stack: each step pushes a frame, executes, and pops it.

**Definition 0.2 (Process)**
A process P = {T_1, ..., T_n} is a set of threads sharing a common address space L_P ⊆ L.

---

## 1. Computational Primitives

### 1.1 Pure Functions

**Definition 1.1 (Pure Function)**
A pure function is defined as f = (I, O, f̂), where f̂: I → O is the mapping function.
**Physical implementation:** a computational step (r_f, w_f, φ_f) whose write set w_f is confined to the current stack frame.

**Axiom 1.1 (Stack-Frame Isolation)**
The write set of one stack frame is disjoint from the write sets of all other stack frames.

> **Theorem 1.1 (Physical Isolation of Pure Functions)**
> Axiom 1.1 ⇒ for every pure function f, w_f ∩ L_other = ∅, where L_other denotes the locations owned by all other frames.

> **Corollary 1.1a (Parallelizability)**
> Theorem 1.1 ⇒ any set {f_i} of pure functions can be executed in parallel on any number n of threads with equivalent results.

### 1.2 Machines

**Definition 1.2 (Machine)**
A machine M is defined as M = (S, I, O, δ, ρ) — an IO-Object (S, I, O, δ) augmented with a cleanup function ρ:
- There is no independent Obs component. Observational data is the subset of O emitted through ports of kind `FlowKind::Observe`.
- There is no independent C component. Control data is the subset of I received through ports of kind `FlowKind::Control`.
- S is the state space.
- δ: S × I → S × O is the transition function (a Mealy machine).
- ρ: S → S is the cleanup function.

**Physical implementation:** S is allocated on the heap (L_S ⊂ L_P); each invocation of δ executes one computational step.

> **Theorem 1.2 (State Locality)**
> Definition 1.2 ⇒ for every machine M, the write sets of all invocations of δ are contained in L_S ∪ w_δ.

**Definition 1.2a (Mealy and Moore Semantics)**
The default Machine of axiom is a **Mealy machine**: the output depends on both the current state and the current input, δ: S × I → S × O.

Certain computational primitives require **Moore semantics**: the output depends only on the current state and is independent of the current input. This is formalized as λ: S → O with a state transition δ_S: S × I → S.

**Construction of a Moore Machine:** implement δ as "update the state first, then produce output from the old state":
δ(s, i) = (s', λ(s)) where s' = δ_S(s, i)

That is, the output λ(s) comes from the state s **before** the transition, not from the state s' after it. This realizes a delay-by-one (one-cycle-late) semantics.

> **Theorem 1.2a (Moore Latency Breaks Feedback Cycles)**
> Definition 1.2a ⇒ in a feedback topology M_1 → M_2 → M_1, if M_2 is Moore, then the output of M_2 lags its input by one step, breaking the algebraic cycle within a single clock.
> *Proof: the output of M_1 at time t depends on the output of M_2 at time t, but the output of M_2 at time t derives from its state at time t−1 and therefore does not depend on the output of M_1 at time t.*

> **Engineering Remark 1.2a (Initial Output of Moore Machines)**
> On the first invocation of δ, the state s_0 of a Moore machine is the initial value. If λ(s_0) is meaningless (e.g., `Option::None`), the convention is that the first output is `Idle` rather than `Yield`. This is a boundary condition that the Turing-machine model does not address but that an implementation must handle.

### 1.3 Entities

**Definition 1.3 (Entity)**
An entity E is defined as E = (S, name). An entity has only a state and a name; it has no inputs, no outputs, and no transition function. It is the minimal declaration of existence.

> **Theorem 1.3 (Observability of Entities)**
> Definition 1.3 ⇒ the state S of an entity E can be observed externally (it suffices to read the addresses L_S), but E participates in no computational topology.

---

## 2. Ports and Connections

**Axiom 2.1 (Communication Only Through Shared Addresses)**
Between two threads, communication free of data races can occur only through shared memory addresses (L_1 ∩ L_2 ≠ ∅) or by copying values.

**Definition 2.1 (Port)**
A port p = (T, d, f) consists of a type T, a direction d ∈ {in, out}, and a flow annotation f ∈ {data, control, observe} (a semantic label; `data` is the un-annotated default). The annotation is semantic, not physical — it does not alter how the flow moves; its only effect is a carrier-selection preference on explicitly annotated edges.

**Definition 2.2 (Interface)**
An interface Γ is a finite **set** of ports. That is, ∀p_1, p_2 ∈ Γ: name(p_1) ≠ name(p_2) ∨ p_1 = p_2.

> **Axiom 2.2 (Compile-Time Static Declaration of Interfaces)**
> The input interface Γ_in and the output interface Γ_out of a Machine are fixed at compile time and immutable at run time.
> *Rust mapping: `type Input: HasPortInfo` (an enum with one variant per port) together with `type Ports: PortSet` (which connects the type space and the value space).*

**Definition 2.3 (Connection)**
A connection ℓ = (p_s, p_t) requires dir(p_s) = out, dir(p_t) = in, T_p_s = T_p_t, and f_p_s = f_p_t.

> **Theorem 2.1 (Type Soundness)**
> Definition 2.3 ⇒ a connection whose types match is semantically valid.
> *Rust mapping: guaranteed by the compiler through `TypeId` checks.*

> **Theorem 2.2 (Observation Isolation)**
> Definition 2.1 ⇒ outputs of the observe flow (f = observe) do not participate in the δ input of any Machine.
> *Proof: in the signature δ: S × I → S × O, Obs does not occur among the inputs. `FlowKind::Observe` is a port annotation, not a computational component.*

> **Theorem 2.3 (Type-Value Consistency)**
> Axiom 2.2 ∧ Definition 2.2 ⇒ the set of enum variants of `type Input` is in one-to-one correspondence with the set of `PortDecl` declarations of `port_schema()`.
> *Proof: `port_schema()` of `type Ports: PortSet` is generated by the `PortSet` implementation, and its declarations originate from the same source as the enum variants of `type Input`/`type Output` (guaranteed by the `declare_ports!` macro or a manual `PortSet` impl).*
> *Rust mapping: the `PortSet` trait connects `type Input: HasPortInfo` (type space) with `PortSchema` (value space); `port_schema()` is auto-derived.*

> **Theorem 2.4 (Existence of Multi-Port Fan-Out)**
> Definition 2.2 ⇒ a Machine can emit to multiple output ports in a single δ invocation.
> *Rust mapping: multi-port fan-out is expressed by `MultiOutput::YieldMulti(Vec<O>)` — the number of outputs is determined at run time (a fan-out machine); fixed-count multi-port output uses `TupleOutput::Yield(O, O)`.*

**Definition 2.4 (Connection Graph)**
A system Σ = (M_Σ, L_Σ), with L_Σ ⊆ ⋃_{M ∈ M_Σ} Out_M × ⋃_{M ∈ M_Σ} In_M.

> **Theorem 2.5 (Output Reachability)**
> Definition 2.4 ⇒ the data of an output port p of Machine M is reachable ⟺ ∃ℓ ∈ L_Σ: ℓ = (p, _).
> *Rust mapping: connection existence is determined by the deployment-layer topology (`DynamicTopology` plus `validate_deep`); a Machine does not query output reachability — observation short-circuiting is a runtime responsibility (the `Observe` flow is materialized through links).*

---

## 3. Execution Sequences and Scheduling

**Definition 3.1 (Execution Sequence)**
The sequence of δ applications of a machine M: s_0 --i_1--> (s_1, o_1) --i_2--> (s_2, o_2) ...

**Definition 3.2 (Scheduler)**
A scheduler Π: M_Σ × ℕ → {T_1, ..., T_n} maps each δ invocation to a physical thread.

> **Theorem 3.1 (Equivalence of Function Execution)**
> Corollary 1.1a ⇒ for a set of pure functions, any scheduler Π yields the same result.

**Axiom 3.1 (Sequential Constraint)**
Successive δ invocations of a machine M must execute on the same thread; otherwise the race conditions on S render the result undefined.

> **Theorem 3.2 (Schedulers Must Respect the Sequential Constraint)**
> Axiom 3.1 ∧ Definition 3.2 ⇒ a scheduler Π must map any two invocations of the same Machine to the same thread.

**Axiom 3.2 (Completeness of the Execution-Primitive Classification)**
All physical execution modes can be classified as: zero scheduling overhead (Inline), cooperative scheduling (Async), preemptive scheduling (CpuBound/CpuBoundN/ThreadPool), and process isolation (Subprocess).

| Primitive | Physical counterpart | Isolation level |
|-----------|----------------------|-----------------|
| Inline | same-thread stack-frame call | shared (0) |
| Async | event-driven thread pool | shared (1) |
| CpuBound | dedicated OS thread | exclusive (2) |
| CpuBoundN(n) | N dedicated threads | exclusive (3) |
| ThreadPool | private bounded thread pool | exclusive (3) |
| Subprocess | separate process (IPC) | isolated (4) |

> **Corollary 3.2a (Completeness of Execution Primitives)**
> Axiom 3.2 ⇒ the six primitives above cover all execution modes.

---

## 4. Resource Algebra

**Axiom 4.1 (Acquisition and Release of Resources Are Paired)**
Every resource r has an acquisition point α(r) and a release point ζ(r); α precedes ζ, and r is inaccessible after ζ executes.

**Definition 4.1 (Resource Class)**
R = (τ, α, ζ, γ), where τ ∈ {static, dynamic, os, thread, process}.

> **Theorem 4.1 (Monotonicity of the Resource Lifecycle)**
> Axiom 4.1 + Definition 4.1 ⇒ in the sequence `init → process* → cleanup`: a resource does not exist before init, exists after init, and ceases to exist after cleanup.

**Definition 4.2 (Static Resource)**
A resource r is called static ⟺ γ(r) = permanent ⟺ ζ(r) = ∅.

> **Theorem 4.2 (Non-Reclaimability of Static Resources)**
> Definition 4.2 ⇒ the lifetime of a static resource equals the lifetime of the process.
> *Rust mapping: code segments, type metadata, vtables, and factory registration information are fixed at compile time.*

---

## 5. Composition and Categorical Structure

**Axiom 5.1 (Existence of the Serial Composition Operation)**
Given M_1: I → O and M_2: O → J, there exists a composition M_1 ⨟ M_2: I → J.

**Definition 5.1 (Serial Composition)**
M_1 ⨟ M_2 = (S_1 × S_2, I_1, O_2, δ_12, ρ_12), where δ_12 executes δ_1 first and then δ_2.

> **Theorem 5.1 (Determinism Preservation)**
> Definition 5.1 ⇒ if both M_1 and M_2 are deterministic, then M_1 ⨟ M_2 is deterministic.
> *Proof: the composition of deterministic functions is deterministic.*

**Definition 5.2 (The Machine Category M)**
Objects: types I, O. Morphisms: machines M: I → O. Identity morphism: id_I = (∅, I, I, ∅, δ_id, ρ_id). Composition: ⨟.

> **Theorem 5.2 (M Satisfies the Category Laws)**
> Definition 5.2 ⇒
> 1. Closure: the output types of ⨟ match.
> 2. Associativity: (M_1 ⨟ M_2) ⨟ M_3 = M_1 ⨟ (M_2 ⨟ M_3).
> 3. Identity law: id ⨟ M = M ⨟ id = M.

---

## 6. Deployment Algebra

**Axiom 6.1 (Separability of Abstraction and Physics)**
The semantic behavior δ of a given Machine does not depend on how it is executed at the physical layer.

**Definition 6.1 (Deployment Mapping)**
Δ: M → (Hint × Spec) maps a Machine to an execution primitive and its parameters.

> **Theorem 6.1 (Deployment Invariance)**
> Axiom 6.1 + Definition 6.1 ⇒ any deployment mapping Δ does not alter δ.

> **Theorem 6.2 (Deployment Consistency)**
> Theorem 6.1 ⇒ the same M can use different Hint values under different deployments, while δ remains consistent.

---

## 7. System-Level Theorems

> **Theorem 7.1 (System Closure)**
> Definition 2.4 ⇒ any δ invocation of any M ∈ M_Σ reads only: S_M, the upstream data in L_Σ, and the current input i.

> **Theorem 7.2 (Observability Completeness)**
> Definition 2.4 ⇒ an output of O_M annotated as `FlowKind::Observe` reaches the collector ⟺ L_Σ contains the corresponding connection.

> **Theorem 7.3 (Back-Pressure Propagation Criterion)**
> The `LinkKind` definition ⇒ back-pressure propagates ⟺ the connection uses BoundedBuf_blocking.

---

## 7.5 Engineering Patches: The Gap Between the Mathematical Model and Its Implementation

The following items acknowledge boundary conditions that the formal Turing-machine/Mealy-machine definitions do not cover but that an implementation must handle. Each item records the corresponding Rust remedy.

**Remark 7.5.1 (Connection Counting)**
In the mathematical Definition 2.4, the existence of a connection ℓ is a boolean quantity. Connection existence is determined by the deployment-layer topology: `DynamicTopology` establishes it once at `materialize`; the runtime does not track connection counts; and a Machine does not query output reachability (observation short-circuiting is a runtime responsibility).

**Remark 7.5.2 (State Cleanup on Panic)**
In the mathematical Definition 1.2, δ is a total function (defined for all inputs). In the implementation, δ (i.e., `process()`) may panic. In that case the state S is in an undefined intermediate state, and invoking ρ (`cleanup()`) may be unsafe.

**Remedy:** the linear runtime uses `CleanupGuard` to safely drop the state on panic (skipping `cleanup`); the asynchronous runtime requires an equivalent mechanism. This is a "safe but leaky" trade-off: resources may leak, but no undefined behavior arises.

**Remark 7.5.3 (Type Erasure in Signal Delivery)**
In the mathematical model, system signals σ ∈ {Shutdown, Checkpoint} are discrete events. `send_signal(&self, signal: SystemSignal)` carries the signal type; `poll_signal() -> Option<SystemSignal>` consumes only `Checkpoint` — `Shutdown` is enforced by the runtime by peeking via `has_shutdown_signal()` (shutdown is a runtime lifecycle responsibility and is not consumed by the machine).

**Remark 7.5.4 (Constant Injection for Sources)**
In the mathematical model, a Source is M = (S, ∅, O, δ, ρ) with δ: S × ∅ → S × O. In the implementation, `init()` can obtain information only from `MachineContext` and cannot accept external configuration parameters with which to set the constant values to be produced.

**Remedy:** pass serialized values through the configuration channel of `MachineContext` (e.g., `config_overrides`), or inject them through the `MachineInstance` configuration at deployment time.

**Remark 7.5.5 (Completeness of Deployment Validation)**
The validity of a connection graph Σ in the mathematical Definition 2.4 requires that all port names appearing in connections exist, that types match, and that there are no cyclic dependencies. The implementation splits the requirement across two validation levels. `DynamicTopology::validate()` performs structural integrity: name uniqueness, existence of the machines referenced by links, self-loop rejection, and implicit fan-out rejection. `DynamicTopology::validate_deep(schemas)` adds the checks that need type information: port-name existence and direction, type and flow compatibility via `LinkCompat::can_link_to()`, resource budget, edge-degree constraints, Inline acyclicity, and Moore cycle safety. Cycle detection for runtime mutation is handled by `TopologyMutation::detect_cycle()` (Kahn's algorithm) before every `Link` operation; `apply_batch()` verifies the batch the same way.

**Remark 7.5.6 (Initial Output of Moore Machines)**
See Engineering Remark 1.2a. On its first invocation, the state of a Moore machine is the initial value s_0; if λ(s_0) is meaningless, the convention is to output `Idle`. This is a boundary condition not covered by the formal definition but that an implementation must handle.

---

## 8. Rust Mapping

| Algebraic concept | Rust implementation | Compiler guarantee |
|-------------------|---------------------|--------------------|
| Pure function f = (I, O, f̂) | `trait Func { type I; type O; fn call(I) -> O }` | Send + Sync, no `&mut State` |
| Machine M = (S, I, O, δ, ρ) | `trait Machine { type State; type Input: HasPortInfo; type Output: HasPortInfo; type Ports: PortSet; type ProcessOutput: MachineOutput<Self::Output>; process(); cleanup() }` | Send + Sync, across lifecycle phases |
| Interface set Γ | `type Input`/`type Output` (enum, one variant per port) | `HasPortInfo` guarantees queryable port metadata |
| Port-set connection | `type Ports: PortSet<Input=Self::Input, Output=Self::Output>` | `PortSet` guarantees consistency between the type space and the value space |
| Entity E = (S, name) | `trait Entity { type S; fn name() }` | no process, no ports |
| Port p = (T, d, f) | `PortDecl { type_id, dir: PortDir, flow: FlowKind }` + enum variant | `TypeId` checked on connection |
| Connection ℓ | `LinkSpec { out, into, kind: LinkKind }` | `LinkCompat::check` |
| Connection graph Σ | `DynamicTopology { machines, links }` | `validate()` |
| Deployment Δ | `MachinePhysicalSpec { execution: ExecutionHint }` | trait signature contains no Hint |
| Resource class R | `ResourceClass { Static, DynamicHeap, OsResource, ... }` | documentation marker |
| Identity morphism id | `builtin::Identity<I>` | zero-cost, zero-branch |
| Categorical composition ⨟ | `FuncScratchPipeline<(A,B)>` | compile-time generic composition |
| Multi-port fan-out | `MultiOutput::YieldMulti(Vec<O>)` / `TupleOutput::Yield(O, O)` | Theorem 2.4 |
| Output reachability | deployment-layer topology (`DynamicTopology`/`materialize`) | Theorem 2.5 |
| Time t | `TimeTick { ns: u64 }` / `MachineContext::time_tick()` | nanosecond precision, no millisecond fallback |
| Session type T | `SessionType { ops: Vec<SessionOp> }` | Theorem 11.1 (binary duality) |
| Global type G | `GlobalType { ops: Vec<GlobalOp> }` | Theorem 11.2 (communication safety) |
| Local type L_p | `LocalType { ops: Vec<LocalOp> }` | Theorem 11.3 (progress) |
| Projection project | `project(global, role) -> LocalType` | Definition 11.4 |
| Hybrid state S_c × S_d | `HybridState<C, D>` | Definition 12.1 |
| Continuous evolution f | `HybridMachine::flow(c, dt, d) -> C` | Definition 12.2 |
| Guard g | `HybridMachine::guard(c, d) -> Option<Jump<D>>` | Definition 12.3 |
| Jump j | `Jump<D> { Transition, Reset, Emit }` | Definition 12.4 |
| Lifecycle state l | `struct Init/Running/Stopping/Stopped` (sealed ZST) | Theorem 13.1 (compile-time safety) |
| Typestate handle | `MachineHandle<M, S: LifecycleState>` | Theorem 13.2 (linearity) |

---

## 9. Curry–Howard Correspondence

| Category theory | Type theory | axiom |
|-----------------|-------------|-------|
| Objects I, O | types `I`, `O` | `type Input, type Output` |
| Morphism M: I → O | function I → O | `trait Machine` |
| Identity morphism | `identity` | `builtin::Identity<I>` |
| Composition ⨟ | function composition | `FuncScratchPipeline` |
| Product S_1 × S_2 | tuple `(S1, S2)` | State of a composite Machine |
| Initial object | `!` (empty, never) | `builtin::EntityRoot` (no ports, no process) |

---

## 10. Theorem Classification and Coverage

The results established in the preceding sections fall into two groups: those that correspond to classical results in the formalization of runtime systems, and those that are novel contributions of axiom. The following table summarizes the classification. For a row with a classical counterpart, the third column records the refinement that axiom introduces; where no counterpart exists, the entry is a new theorem or axiom of this framework.

| Reference result | axiom | Refinement |
|------------------|-------|------------|
| Lifecycle monotonicity | Theorem 4.1 | explicit resource classification; non-reclaimable resources marked Static |
| Closure after registration | Theorem 7.1 | the sources of closure are made explicit as L_Σ |
| — | Theorem 1.1 | new: physical isolation of pure functions (parallel safety) |
| — | Theorem 1.3 | new: observability of entities (persistent existence without a process) |
| — | Theorem 2.2 | new: observation isolation (Obs is not among the inputs of δ) |
| — | Theorem 5.2 | new: verification of the category laws (the algebraic structure of composition) |
| — | Theorem 6.1 | new: deployment invariance (separability of abstraction and physics) |
| — | Axioms 1.1–3.2 | explicit axiomatization — every corollary is traceable to its premises |
| — | Identity morphism | concretized as `builtin::Identity<I>` |
| — | Initial object | concretized as `builtin::EntityRoot` |

---

## 11. Session Types

### 11.1 Binary Session Types

**Definition 11.1 (Session Type)**
A session type T is given by the following recursive grammar:
T ::= !ℓ.T | ?ℓ.T | μt.T | t | end

where !ℓ.T means sending label ℓ and then continuing as T, ?ℓ.T means receiving label ℓ and then continuing as T, μt.T is a recursive type, and end is termination.

**Definition 11.2 (Binary Duality)**
Two session types T_1 and T_2 are dual (dual(T_1, T_2)) if and only if:
- dual(!ℓ.T_1, ?ℓ.T_2) ⟺ dual(T_1, T_2)
- dual(end, end)

> **Theorem 11.1 (Safety of Binary Connections)**
> Definition 11.2 ⇒ two ports can be connected safely ⟺ their session types are dual.
> *Rust mapping: `session::is_dual(&T1, &T2)`.*

### 11.2 Multiparty Session Types (MPST)

**Definition 11.3 (Global Type)**
A global type G describes the interaction choreography among all participants:
G ::= p_1 → p_2 : ℓ. G | end | skip

where p_1 → p_2 : ℓ means that role p_1 sends label ℓ to role p_2.

> *Rust mapping: `GlobalType { ops: Vec<GlobalOp> }`, with `GlobalOp::Message { from, to, label }`.*

**Definition 11.4 (Projection)**
The projection project(G, p) of a global type G onto a role p produces the local type L_p:
- For p_1 → p_2 : ℓ. G':
  - if p = p_1: L = !ℓ → p_2. project(G', p)
  - if p = p_2: L = ?ℓ ← p_1. project(G', p)
  - otherwise: L = skip. project(G', p)

> *Rust mapping: `project(global: &GlobalType, role: &str) -> LocalType`.*

> **Theorem 11.2 (Communication Safety)**
> Definition 11.3 ∧ Definition 11.4 ⇒ a message is always sent to the role that expects to receive it.
> *Proof: projection guarantees that the sender's `Send{to}` and the receiver's `Recv{from}` correspond to the same `Message{from, to}` in the global type.*

> **Theorem 11.3 (Progress)**
> Definition 11.3 ∧ Definition 11.4 ⇒ if all participants follow their projected local types, the protocol does not deadlock.
> *Proof: a global type describes a linear sequence of interactions, and projection preserves the ordering constraints.*

---

## 12. Hybrid Systems

### 12.1 Hybrid Automaton Model

**Definition 12.1 (Hybrid State)**
A hybrid state is the product of a continuous state and a discrete state:
S = S_c × S_d

> *Rust mapping: `HybridState<C, D> { continuous: C, discrete: D }`.*

**Definition 12.2 (Continuous Evolution / Flow)**
Between two discrete jumps, the continuous state evolves according to an ODE:
dc/dt = f(c, d)

where d remains constant during the evolution.

> *Rust mapping: `HybridMachine::flow(c: &C, dt: f64, d: &D) -> C`.*

**Definition 12.3 (Guard Condition)**
A guard condition g: S_c × S_d → Option<Jump> detects whether the continuous state has crossed a threshold, thereby triggering a discrete jump.

> *Rust mapping: `HybridMachine::guard(c: &C, d: &D) -> Option<Jump<D>>`.*

**Definition 12.4 (Jump)**
A jump j is an instantaneous state transition:
- Transition(d'): the discrete state becomes d', and `reset()` updates the continuous state.
- Reset{d'}: the discrete state becomes d', and the continuous state is reset via `reset()`.
- Emit(s): emit an output without changing the state.

> *Rust mapping: the `Jump<D>` enum.*

**Definition 12.5 (Hybrid Transition Function)**
The transition function of a hybrid system:
δ_h: (S_c, S_d) × I × Δt → (S_c, S_d) × O

where Δt comes from the runtime's `TimeTick` (nanosecond precision).

> **Theorem 12.1 (Time-Precision Preservation)**
> Definition 12.5 ⇒ `HybridDriver` computes Δt using `TimeTick` (nanoseconds) without loss of precision.
> *Rust mapping: `step_to_tick(tick: TimeTick)` computes `dt` automatically.*

> **Theorem 12.2 (Atomicity of Jumps)**
> Definition 12.4 ⇒ jumps are applied atomically in `apply_pending_jumps()` — the discrete-state update and the `reset()` call complete in the same step.

---

## 13. Lifecycle Typestates

### 13.1 The Typestate Pattern

**Definition 13.1 (Set of Lifecycle States)**
The set of lifecycle states of a machine is L = {Init, Running, Stopping, Stopped}, with the partial order:
Init ≺ Running ≺ Stopping ≺ Stopped

**Definition 13.2 (Typestate Encoding)**
Each lifecycle state l ∈ L corresponds to a zero-sized type (ZST) that serves as the type parameter S of `MachineHandle<M, S>`.

> *Rust mapping: `struct Init; struct Running; struct Stopping; struct Stopped;` (sealed behind the `LifecycleState` trait).*

**Definition 13.3 (State Transition Functions)**
A transition function trans: MachineHandle<M, l_1> → MachineHandle<M, l_2> exists only when l_1 ≺ l_2:
- start: Init → Running
- stop: Running → Stopping
- finish: Stopping → Stopped

> **Theorem 13.1 (Compile-Time Safety)**
> Definition 13.2 ∧ Definition 13.3 ⇒ illegal state transitions are rejected at compile time.
> *Proof: the Rust type system guarantees that `process()` exists only on `MachineHandle<M, Running>` and `MachineHandle<M, Stopping>`, and that `cleanup()` exists only on `MachineHandle<M, Stopped>`. The compiler rejects any invocation of a method in the wrong state.*

> **Theorem 13.2 (Linearity Guarantee)**
> Definition 13.3 ⇒ each transition consumes `self` (received by value) and returns a handle to the new state; the handle to the old state can no longer be used.
> *Rust mapping: `fn start(self) -> MachineHandle<M, Running>`.*

> **Theorem 13.3 (Sealing)**
> Definition 13.2 ⇒ external code cannot introduce new lifecycle states.
> *Rust mapping: `LifecycleState: private::Sealed`, where the `Sealed` trait lives in a private module.*

---

## 14. Unified Assessment: Reduction and Coverage of Research Lineages

This section addresses a meta-question: can the few primitives of axiom absorb the multiple lines of research in concurrency, distributed systems, and control theory accumulated over decades, yielding a unified mathematical formalization? The assessment has two parts: a reduction table of the absorbed lineages, and an evaluation of the uncovered gaps.

### 14.1 Reduction of Absorbed Research Lineages

The following table lists the research lineages that axiom has formally absorbed; each is reduced to axiom's few primitives (Port / Flow / Session / Topology / Lifecycle / Machine). The direction of reduction is that the concept of the original research **disappears** into a composition or specialization within axiom, rather than being added as a parallel concept.

| Research lineage | Representative work | axiom reduction | Mode of absorption |
|------------------|---------------------|-----------------|--------------------|
| Binary session types | Honda 1993, Takeuchi 1994 | `SessionType` + `is_dual` (§11.1) | the protocol becomes an attribute of Port; duality checking becomes one step of `can_link_to` |
| Multiparty session types (MPST) | Honda–Yoshida–Carbone 2008/2016 | `GlobalType` + `project` + `is_consistent` (§11.2) | global choreography is projected onto local types; consistency checking reduces to paired send/receive after projection |
| Interface automata | de Alfaro–Henzinger 2001 | `can_link_to` + `LinkCompat` (§2) | compatibility checking moves from an independent model to the conjunction of the four Port dimensions (direction/flow/type/protocol) |
| Hybrid automata | Alur–Courcoubetis–Henzinger 1995 | `HybridMachine` + `HybridDriver` (§12) | continuous dynamics become an extension trait of Machine; Jump reuses the existing output channels |
| Typestate / linear types | Strom–Yemini 1986, Rust ownership | `MachineHandle<M, S>` + sealed ZST (§13) | lifecycle phases become type parameters; transitions consume self → compile-time safety |
| Dataflow / Kahn networks | Kahn 1974 | `Machine` + `Port` + topology constraints (§1–3) | a dataflow network = an acyclic Port graph; the Kahn fixed point = stable iteration in topological order |
| Pull-model stream processing | iteratees / FRP (Elm, Rx) | `StreamingMachine::process_stream` (supplementary) | lazy iterator output: the first `next()` resets the cursor and the machine produces in internal batches — a dual push/pull model |
| Zero-copy input | zero-copy frameworks (io_uring, DPDK) | `FuncRef::call_ref(&Input)` (supplementary) | borrowing input with zero allocation vs owned input coexist as two paths — zero cost means "no extra cost", not "no cost at all" |
| Topology mutation / hot swap | — | `TopologyMutation` + `apply_batch` + Replace (§6) | an engineering capability (not a research lineage): it exists for contract completeness, as the runtime mirror of a pure-data topology; runtime reorganization reduces to atomic snapshot + rollback over (machines, links). **Static topology is the default world view** (see the positioning note below) |
| Categorical composition | — | `Func` composition + ⨟ (§5) | function composition = categorical morphism composition, zero-cost at compile time via generics |
| Curry–Howard | — | theorem ↔ type mapping (§9) | propositions as types, proofs as programs, in direct correspondence with the Rust type system |

**The mathematical meaning of reduction:** axiom introduces no "parallel universe" for any of the lineages above. Each lineage is expressed as a **constraint** or **extension** of a few primitives:
- session types = protocol constraints on Port (no new connection concept);
- hybrid systems = continuous extension of Machine (no new computational model);
- typestate = type parameter of MachineHandle (no new runtime object);
- dynamic topology = atomic change of (machines, links) (no new graph model).

This is the criterion of unification: **the number of concepts does not grow with the number of research lineages.**

> **Positioning note (topology mutation):** `TopologyMutation` differs from the other entries — it is not a research lineage (it has no representative work) but an engineering capability. Its reason for existence is **contract completeness**: the pure-data `DynamicTopology` is serializable and cannot by itself reorganize the instance graph, so the runtime mirror of a topology — `TopologyMutation` — must have a type. The default world view of axiom is **static topology + deployment-time validation** (the checks of `validate_deep` and `analysis` — degree constraints, acyclicity of Inline, Moore cycle safety — are meaningful only when the topology is fixed). Most seemingly "dynamic" scenarios (elastic scaling, hot swap, session subgraphs) can be expressed with static topology plus control/state changes (e.g., preallocating a maximum replica count at deployment time and starting/stopping via control ports at run time); runtime reorganization is an optional capability, not part of the core algebra. Only a few scenarios genuinely require it (topologies decided at run time; the dynamic lifecycle of session-private subgraphs), and the **type space remains static** — only instances of already-registered types can be added or removed.

### 14.2 Uncovered Gaps and Assessment

The following table lists research lineages that are frequently mentioned but that axiom has **not formalized**, together with an assessment of whether the gap should be filled.

| Research lineage | Representative work | Relation to axiom | Assessment |
|------------------|---------------------|-------------------|------------|
| π-calculus | Milner 1992 | dynamic creation and mobility of channels | **Not filled.** Port names of axiom are `&'static str`; channels are not first-class citizens. Dynamic creation is realized through `TopologyOp::Spawn` applied by `TopologyMutation`, but its semantics is a topology change, not channel mobility. If mobility semantics were ever needed, it could be added as an extension of `TopologyOp` without requiring a new primitive. |
| Petri nets | Petri 1962 | concurrent tokens and places | **Not filled.** The concurrent semantics of Petri nets can be expressed by combining multi-port fan-out with multi-input aggregation (Theorem 2.4, multi-port fan-out). Petri-net token-count constraints have no counterpart; they could be added as an optional capacity constraint on `PortDecl`, although the benefit is limited. |
| CSP / CCS | Hoare 1978, Milner 1980 | process algebra and synchronous communication | **Not filled.** The synchronous rendezvous of CSP differs from the asynchronous ports of axiom. Session types already cover protocol-level synchronization; the algebraic equivalence laws of CSP have no counterpart, but the goal of axiom is an executable engineering system, not a process-algebra calculus. |
| Actor model | Hewitt 1973 | location-transparent message passing | **Partially covered.** A `Machine` is an actor and a Port is its mailbox. What is not covered is location transparency (remote actors). This is a transport-layer problem that could be introduced at the runtime layer via a `RemotePort` without affecting the core algebra. |
| Temporal logic / LTL | Pnueli 1977 | temporal specification of system properties | **Not filled.** The theorems of axiom are structural (safety/progress); verifying temporal properties requires a model checker. This could be done by an external tool (in the style of TLA+) verifying axiom topologies, without embedding model checking in the core algebra. |
| LTL model checking | Clarke 1981 | automated property verification | Same as above. |
| Soft real-time scheduling | Liu–Layland 1973 | deadlines and scheduling | **Partially covered.** `TimeTick` provides nanosecond time and `Lifecycle::Stopping` supports graceful exit. Scheduling algorithms themselves are not covered — this is a runtime-policy concern that the core algebra does not address. |

### 14.3 Unification Conclusions

> **Theorem 14.1 (Primitive Convergence)**
> axiom absorbs 9 research lineages (§14.1) with 6 classes of primitives (Port / Flow / Session / Topology / Lifecycle / Machine), and the expression of each lineage introduces no new parallel concept — each is a constraint or extension of an existing primitive.

> **Theorem 14.2 (Extensibility of Gaps)**
> None of the uncovered lineages listed in §14.2 requires a new core primitive. Their filling paths are either "an optional constraint on an existing primitive" (e.g., Petri-net capacity), "a runtime-layer extension" (e.g., remote actors), or "verification by an external tool" (e.g., temporal logic). The core algebra remains stable.

**Conclusion.** axiom possesses the skeleton of a unified mathematical formalization — few primitives plus composable extensions. Nine lineages have been absorbed, and the seven uncovered ones all have explicit filling paths that do not break the core algebra. The "unification" of the framework manifests as the convergence of the number of concepts: no matter how many research lineages are absorbed, the core primitives remain the 6 classes, and new lineages are reduced as constraints or extensions rather than piled up as parallel concepts.

**Open Questions.** The following items are optional extensions; none of them affects the current unification conclusions:
1. **π-calculus mobility semantics.** If channel mobility were ever needed, the semantics of `TopologyOp::MovePort` would be the object of study.
2. **Temporal property verification.** Provide an exporter from axiom topologies into a form verifiable by external model checkers (in the style of TLA+) for temporal properties.
3. **Petri-net capacity constraints.** Add an optional `capacity: Option<usize>` to `PortDecl` to cover bounded-place semantics.

---

## 15. Zero-Cost Abstraction: Decoupling the Abstraction Layer from the Physical Layer

This section formalizes the core design axiom of axiom: **the abstraction layer and the physical layer are two independent layers of existence; the abstraction provides correctness constraints for the physical layer, and the physical layer pays no cost for the existence of the abstraction.** This is the system-architecture-level counterpart of the Rust zero-cost-abstraction philosophy.

### 15.1 Two Layers of Existence

**Definition 15.1 (Abstraction Layer A)**
The abstraction layer is the algebra of types and topologies of axiom:
A = (M, P, L, T, Σ)
where M is the set of Machines, P the set of Ports, L the set of Links, T the set of types, and Σ ⊆ M × M the connection graph. The elements of A are **mathematical objects** — modules, ports, dataflows, control flows — which organize the readability and reasonability of a system.

**Definition 15.2 (Physical Layer P_h)**
The physical layer is the CPU, the stacks, the instruction set, the cache lines, and the address space:
P_h = (L, T, Φ, mem)
where L is the set of memory locations (Axiom 0.1), T the set of threads, Φ the instruction sequences, and mem: L → V the memory state. The elements of P_h are **physical entities** — addresses, registers, stack frames — which perform the actual computation.

**Axiom 15.1 (Disjointness of the Layers of Existence)**
A ∩ P_h = ∅
The objects of the abstraction layer ("modules", "ports", "control flows") **do not exist** at the physical layer. The physical layer has only memory addresses and instructions. When we say "machine M sends data to machine N", what happens at the physical layer is merely that some thread writes bytes to some address and another thread reads bytes from that address. The labels "machine" and "send" are semantic annotations of the abstraction layer, of which the physical layer is unaware.

**Example 15.1 (Two-Layer Description of a Receive-and-Print System)**
Two threads: T_1 receives network data, T_2 computes and prints.

| Layer | Description |
|-------|-------------|
| Abstraction layer A | receive module M_R → (data flow) → print module M_P; if persistence M_S is added, then M_R fans out to both M_P and M_S, forming a directed graph. |
| Physical layer P_h | T_1 executes `recv()`, writing to buffer B (located at L_B); T_2 reads from B and executes `write(stdout)`. "Modules" and "data flows" do not exist at the physical layer. |

The graph structure M_R → M_P of the abstraction layer and the memory writes of the physical layer are two descriptions of the same phenomenon; the former serves human reasoning, the latter is what the machine actually executes.

### 15.2 The Zero-Intrusion Axiom of Abstraction over Physics

**Axiom 15.2 (Zero-Cost Abstraction)**
For any abstraction-layer structure α ∈ A whose physical implementation ⟦α⟧ ∈ P_h satisfies the following conditions, α is called **zero-cost**:

1. **Disappearance at run time:** α allocates no data structure in the compiled artifact. That is, there exists no runtime object o such that typeof(o) = typeof(α).
2. **Unchanged execution time:** the execution time of ⟦α⟧ equals that of an equivalent handwritten physical implementation h_α: t(⟦α⟧) = t(h_α) + ε, where ε is the noise of compiler optimization (typically |ε| / t(h_α) < 0.05).
3. **Unchanged memory footprint:** the steady-state memory footprint of ⟦α⟧ equals that of h_α.

**Theorem 15.1 (Compile-Time Resolution of Abstractions)**
If an abstraction α satisfies the following conditions, then α is zero-cost:
- (a) all of α's type parameters are known at compile time (**static deployment**);
- (b) all of α's transition functions are pure and inlined by the compiler;
- (c) α introduces no runtime type erasure (no `Box<dyn Any>`, no vtables).

> **Proof:**
> (a) ⇒ the compiler monomorphizes each instance of α, generating code for concrete types and no trait objects;
> (b) ⇒ after inlining, the transition functions disappear into instruction sequences isomorphic to handwritten code;
> (c) ⇒ no heap allocation and no dynamic dispatch, so t(⟦α⟧) = t(h_α).
> By Axiom 15.2, α is zero-cost. ∎

### 15.3 The Two Deployment Paths of axiom

axiom provides **two deployment paths**, corresponding to the two ways of satisfying Axiom 15.1:

| Path | Function | Topology known at | Cost per message | Satisfies Axiom 15.2? | Use cases |
|------|----------|-------------------|------------------|-----------------------|-----------|
| **Static** | `axiom_runtime::static_path::{pipeline_chain, diamond, feedback}` (combinators) | compile time | **zero** | yes | series-parallel pipelines, fan-out/fan-in, hot paths (**default**) |
| **Dynamic** | `Runtime::materialize(spec)` | run time | fused: 1.0 allocs/msg (typed-slot reuse); plain: per-hop alloc + vtable | no | runtime topology mirror (rare scenarios) |

> **Static-first principle.** A topology known at compile/deployment time is the **default**. The dynamic path (`Runtime::materialize`) is used only when the topology must be decided at run time (Corollary 15.3a), and the **type space remains static** — the runtime can only add or remove instances of already-registered `machine_type`s, and cannot load new code (plugin code loading is a concern of the runtime adapter, not a contract of the axiom core). Most "dynamic" scenarios (scaling, hot swap, session subgraphs) can be expressed with static topology plus control/state changes.

> **Structural scope constraint (anti-narrowing rule).** The structural layer of axiom must express **arbitrary topologies** — multiple pipelines, fan-out, fan-in, directed cycles, nested composition — as first-class static definitions. A single linear pipeline A → B → C, a single execution thread, and a single fixed-function path are **narrow subsets** of the design space; they are **not** the default capability, **not** the goal, and **not** an acceptable fallback when full structural capability is hard to achieve. When a feature is "easy for a linear chain but difficult for a general DAG", the correct course is to **solve the general case, or explicitly mark it as out of scope** — rather than silently narrowing axiom to the easy subset. Narrow subsets may serve as minimal verification probes (such as the `pipeline_chain` zero-overhead probe of §15.3), but must not be mistaken for the upper bound of axiom's capabilities.

**Definition 15.3 (Static Deployment deploy_static)**
Static deployment encodes the topology in the type parameters of the
combinator algebra — `Chain<Head, Tail, L>` (arbitrary-depth linear chain),
`Diamond<A, Left, Right, Down, S, LB, LC, M>` (fork-join), and `feedback`
(single-machine feedback loop):

deploy_static: Machine × Machine × Link → StaticDeployed
where `L: StraightLink<S, D>` is a compile-time-known pure function
convert: S::StraightOut → D::StraightIn.

> **Implementation mapping:** deploy_static is implemented by the `axiom_runtime::static_path` module — the combinators `pipeline_chain` (linear), `diamond` (fork-join), and `feedback` (loop). The type contracts (`StraightMachine`/`StraightLink`/`StraightSplit`/`StraightMerge`) are defined in `axiom::static_exec`. In the following formalization, deploy_static denotes that module collectively.

Its physical implementation ⟦deploy_static⟧ satisfies the three conditions of Theorem 15.1:
- (a) the topology ⟨Head, Tail, L⟩ is known at compile time → monomorphization;
- (b) `L: StraightLink`'s convert is a trait method inlined by the compiler into a direct payload move;
- (c) stage boundaries are direct calls on bare payloads — no channel, no type erasure, no per-message allocation.

**Theorem 15.2 (Zero-Cost Nature of Static Deployment)**
deploy_static satisfies Axiom 15.2, i.e.:
t(⟦deploy_static⟨S, D, L⟩⟧) = t(h_{S → D}) + ε
where h_{S → D} is an equivalent handwritten two-task + mpsc-channel implementation.

> **Empirical verification (the L1 benchmark suite, release build in a reference environment, 100k messages):** relative throughput (absolute numbers vary with machine and allocator; the ordering is environment-independent):
> - handwritten (`l1_pipeline`): 1.0× (baseline)
> - static path (`l1_static`): **1.24×** (because inlining `StraightLink::convert` eliminates the adapter task present in the handwritten version)
> - dynamic path (`l1_declarative`): 0.20× (the "dynamic tax")
>
> The static path not only matches the handwritten version but is actually **faster** — because the abstraction layer exposes to the compiler a conversion structure that the handwritten code hides, enabling the elimination of an intermediate task. This positively validates Axiom 15.1: the existence of the abstraction layer adds no burden to the physical layer, and in fact enables physical optimization.

**Definition 15.4 (Dynamic Deployment deploy_dynamic)**
Dynamic deployment accepts a `DynamicTopology` at run time and type-erases every message through the envelope type `Wire { payload: Box<dyn Any> }`:
deploy_dynamic: DynamicTopology × FactoryMap → Deployed

Its physical implementation does not satisfy Theorem 15.1:
- (a) the topology is known only at run time → no monomorphization;
- (b) `from_port_name` reconstructs types via string matching → no inlining;
- (c) `Box<dyn Any>` heap-allocates per message → violates condition (c).

**Theorem 15.3 (The Irreducible Part of the Dynamic Tax)**
An earlier formulation of this theorem claimed that the dynamic tax necessarily consists of one heap allocation, one dynamic dispatch, and one string comparison per message; measurement showed this statement to be inaccurate — the true composition of the dynamic tax is **two `Box` heap allocations per stage** (reduced to about one per stage after the changes described below), and the "unavoidable" part had been conflated with implementation redundancy. The following is the corrected statement.

If the topology can be determined only at run time, then under **safe Rust** every message requires at least, per stage:
- **1 heap allocation:** values must cross heterogeneous types (`Out → Box<dyn Any> → In`); `Box<dyn Any>` is the only type-erasure mechanism in safe Rust (`forbid(unsafe_code)` forbids transmute-style zero-allocation type conversion). `from_port_name` unpacks the input Box and `into_any` re-boxes the output — a net 1 per stage;
- **1 dynamic dispatch:** the machine's `process` is invoked virtually through `dyn RunningMachine`.

That is, deploy_dynamic cannot be zero-cost — but "unavoidable" refers only to the safe-Rust lower bound above (about one allocation plus one virtual call per stage), **not** to the two allocations per stage of the earlier implementation.

> **Measurement (3-stage chain, 100k messages, release build with an allocation counter):**
> - before the change (2 boxings per stage): **6.0 allocs/msg**, dynamic tax ≈ 400×;
> - with ID-ization and `inject` eliminating redundant boxing: **3.0** non-fused / **4.0** fused allocs/msg;
> - with the unsafe resolution (the encapsulated `typed_slot`; see the design-principles document, §5.5): fused chain **1.0 allocs/msg** (one for the external input; zero allocations between stages of the same type), time 36 → 27 ms — the allocation tax is eliminated, leaving dynamic dispatch and the cost of the type-erasure mechanism;
> - static path (`FlowThrough` linear streaming): **0.000 allocs/msg**.
>
> Safe-Rust attempts at "zero allocations between stages" (`ScratchMachine`/`Unpack` typed single-slot) were refuted: after a single-slot `take()` the slot is empty and must be re-boxed; runtime `TypeId` equality cannot perform a typed write. By the layered decision of the design-principles document §5.5 (core `forbid`, runtime permits encapsulated unsafe), `typed_slot` uses a `TypeId` check plus bit copy (`ptr::read`/`copy_nonoverlapping`) to achieve zero allocation between stages of the same type — the dynamic path's allocation tax is reduced to one for the external input, and **fully zero allocation (0.000 allocs/msg) remains achievable only on the static path** (typed at compile time).

> **Corollary 15.3a (Justification of the Dynamic Path)**
> The dynamic tax (about one allocation plus one virtual call per stage) is the **price of run-time topology flexibility**. It is justified if and only if the topology must be decided by configuration, plugins, or run-time decisions; otherwise the static path (zero allocation) should be used. The dynamic tax grows linearly with the number of stages — **for deep chains, prefer the static path.**

### 15.4 The Separation Theorem of Abstraction and Physics

**Theorem 15.4 (Non-Intrusiveness of Abstraction over Physics)**
Let Π be any runtime adapter (such as `axiom_tokio`, `axiom_rayon`) and α ∈ A any axiom abstraction structure. Then:
∀Π, ∀α: the physical behavior of ⟦α⟧_Π equals the physical behavior of ⟦α⟧_Π' (in the sense of semantic equivalence)

That is, the abstraction layer α depends on no specific runtime. Replacing the runtime adapter does not change the semantics of the abstraction layer; it changes only the execution strategy of the physical layer (thread model, scheduler, I/O model).

> *Rust mapping: the axiom core crate (`src/`) depends on neither an async runtime nor a thread-pool library. All execution logic lives in runtime adapters: core ships one reference adapter (`axiom-runtime`); third parties provide further adapters (e.g. `axiom-tokio`, `axiom-rayon`) that depend on core. The core defines only traits and types; it contains no `async fn`, calls no `spawn`, and allocates no threads.*

**Corollary 15.4a (Purity of the core)**
The axiom core satisfies the following invariants:
- no thread or task spawning (no `std::thread::spawn`, no executor spawn);
- no `async fn`, no `Future` implementations;
- no runtime objects (no executor, no reactor).

The core is the formalization of A, and an adapter is an interpreter of A → P_h.

### 15.5 Analogy: debug vs. release

**Axiom 15.3 (Switchability of Abstractions)**
The existence of the abstraction layer does not force the physical layer to be aware of it. This parallels the debug/release dichotomy of Rust:

| Dimension | debug build | release build |
|-----------|-------------|---------------|
| bounds checking | performed at run time | eliminated after compile-time proof |
| call stack | fully preserved | tail-call optimization, inlining |
| abstraction layer | observable (panic information, trait names) | **disappears** into machine code |

In a release build, the abstractions of axiom should likewise **disappear**: the `Port`, `Link`, and `Machine` types do not exist at run time; only concrete types and concrete functions exist. This is precisely the meaning of condition 1 of Axiom 15.2 (disappearance at run time).

> **Theorem 15.5 (Provable Resolvability of Abstractions)**
> The abstractions of axiom can be fully resolved by the compiler in a release build if and only if the three conditions of Theorem 15.1 hold. The static path (`deploy_static`) satisfies them; the dynamic path (`deploy_dynamic`) does not (because condition (c) is violated).

### 15.6 Summary of Design Axioms

> **Axiom 15.4 (The Existential Axiom of axiom)**
> axiom is a formalization of the abstraction layer A, not a runtime framework. Its purpose is:
> 1. to provide **correctness constraints** at the abstraction layer (type soundness, port consistency, legal topology);
> 2. to provide **zero-cost execution** at the physical layer (compile-time resolution, no runtime burden);
> 3. to provide **strict separation** between the two layers (the core contains no runtime; adapters contain no abstraction).
>
> When the business logic is correct and compiles, the physical execution is correct and carries no extra burden. This is the intent of axiom as an "architectural axiom": it constrains design, not execution.
