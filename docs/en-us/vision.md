> **Language:** English · [中文](../zh-cn/vision.md)

# axiom Vision Charter: The Cross-Domain Constitution Layer and the Consistency Machine

> **Status:** This volume is axiom's charter. It answers two questions — why
> axiom is built, and why its shape is what it is — and anchors the answers in
> the definitions, axioms, and theorems of the formal volumes (foundations /
> core / semantics / unified). It introduces no new axioms, constructors, or
> normative obligations; where it conflicts with a formal volume, the formal
> volume prevails. Claims that are assumptions or not yet verified are marked
> in place.

## 0. Position

axiom is the cross-domain constitution layer for heterogeneous complex
systems. Each term of the position is unfolded below.

- **Constitution layer**: it prescribes only the legality of composition —
  what may connect to what, which obligations must be declared, which
  decisions are made at compile time — and not application logic. By analogy
  with an existing fact: Rust provides memory safety but not applications;
  axiom provides topology safety, explicit obligations, and swappable
  physics, but not systems.
- **Cross-domain**: across different classes of complex systems, the hardest
  part of each converges on one problem set (§1). axiom builds only that
  intersection; it is not tailored to any single domain.
- **Heterogeneous complex systems**: the four target systems (§1) cover
  interaction-heavy, state-heavy, throughput-heavy, and task-heavy workload
  shapes. The charter's test is whether all four decompose losslessly onto
  one concept set (§2).

Non-goals follow: not an all-in-one framework; no inversion of control; no
runtime container; no ownership of the user's lifecycle. The constitution
layer provides legality, not the system itself.

## 1. Four Target Systems and the Convergent Problem Set

The four target systems: MMO game servers, large GUI desktop applications,
high-performance databases, and AI agents.

In contemporary architecture practice, the hardest roughly twenty percent of
each converges on one problem set:

| Problem | Shape |
|---|---|
| Cancellation | Requests propagate in reverse along the composition tree; every layer on the path must respond honestly |
| Backpressure | Upstream rate constraints propagate forward; no intermediate layer may swallow them silently |
| Deadlines | Time constraints enter composition as values, not as callbacks scattered through code |
| Crash recovery | State and side effects are rebuildable from a persistent event stream |
| Deterministic replay | The same input sequence yields the same trace on the same binary |
| Ownership partitioning | Exclusive boundaries of state are drawn at compile time; locks and races are not the default |
| Context propagation | Identity, tracing, and configuration survive async boundaries |

For any single system class, each problem has its own engineering answers;
when the four classes are placed side by side, the answers share one shape:
topology (what connects to what), obligations (who must do what), and physics
(how waiting and I/O are implemented) must be separated, yet declared at the
same place. This is the part covered by axiom's five-concept closure,
obligation ledger, and carrier seams (see the foundations and semantics
volumes).

Four community-fact-level observations support the convergence judgment
(project comparisons and sources live in the internal research directory):

1. Structured concurrency is an established paradigm whose production
   incidents concentrate on "escaping the structured scope": cancellation
   that does not propagate, context lost across async boundaries. Conclusion:
   structural constraints belong in the definition layer, not in runtime
   conventions. axiom's counterpart is the blueprint-as-task-tree; that
   cancellation is deactivation propagating in reverse along blueprint edges
   is a topological operation. (The latter sentence is an assumption pending
   adversarial evaluation; see §6.)
2. Deterministic simulation testing is an established practice in databases,
   finance, and distributed infrastructure. Its three prerequisites — a
   single-threaded deterministic core, all I/O behind an abstraction, explicit
   fault injection — correspond item by item to axiom's architecture
   (synchronous pure step, all physics through carrier seams, the obligation
   ledger); an axiom system is simulation-ready by construction.
3. Durable execution (event logs, crash replay, idempotent side effects) is a
   common shape in AI agent engineering. The axiom mapping: the agent is a
   PortCell; model and tool calls are wait points with deadlines and
   cancellation; the event carrier serves as the log; replay is blueprint
   simulation.
4. Language-level sync/async unification (async drop, effect generics) remains
   in multi-year exploration; the readiness-based and completion-based I/O
   models are split at the kernel-interface layer. Conclusion: sync/async
   unification must be built at the seam level — this is moving first, not
   against the current; the two I/O models are two carrier members of the same
   seam, and platform differences are absorbed by the instance layer (see the
   the three wait-point contracts and the synchronous-domain polling face in
   the semantics volume).

## 2. The Four-System Decomposition Table: The Losslessness Test

The charter's completeness test is this table: whether each target system's
core structure decomposes, without semantic loss, onto the same set of axiom
concepts (State / step / wait point / carrier / partition).

| System | State and step | Wait points | Carrier physics | Partitioning |
|---|---|---|---|---|
| MMO server | Entity state and fixed-step tick | Client messages, persistence write-back | Socket buffers, kernel I/O interfaces | Spatial sharding, interest management |
| GUI desktop | Widget-tree state and event stepping | Input queues, compositor frame receipts | Framebuffers, input event channels | Window/widget boundaries |
| Database | Indexes and buffer pools, query-execution steps | Disk and network I/O | Log-structured storage, page cache | Tables/shards/replicas |
| AI agent | Session state and reasoning steps | Model calls, tool calls | Event logs, message queues | Task/subtask boundaries |

The four rows use different physical vocabularies, but the structural slots
correspond one to one: exclusive State, pure-function step, waiting only at
boundaries, physics swappable through seams, partitioning as ownership. Four
rows coexisting without distortion is the minimum evidence for the charter;
any distortion in one row enters the amendment process (see §6).

## 3. The Niche: Four Properties, One Seam

axiom's niche is defined by the conjunction of four properties; lacking any
one of them, the ground is ceded to existing shapes:

| Property | Content | Common shape when it exists alone |
|---|---|---|
| Typed topology | Composition structure is a type; illegal wiring fails to compile | Combinator ecosystems: composition without typed topology |
| Explicit obligations | Deadlines, cancellation, backpressure are declared items, not comments | Durable execution: replay without typed topology |
| Swappable physics | Waiting and I/O are replaced through contract seams | Deterministic-simulation systems: closed inside their own runtimes |
| Determinism-ready | Synchronous pure core + physics through seams; simulation-ready by construction | Structured-concurrency runtimes: no compile-time topology proofs |

Each property appears alone in many mature shapes; all four coexisting on one
seam is the vacancy axiom claims. Whether the vacancy is real and wanted is
answered by the evidence discipline of §6.

## 4. The AI-Era Thesis: The Consistency Machine

Economic premise: as the cost of generating code approaches zero, the scarce
resource shifts from "writing code" to "keeping structure consistent". Under
this premise axiom's product is not features but invariants; its shape is a
consistency machine.

- **Mechanism 1: structure moves from convention to type.** Models generate
  code faster than humans can review; human review does not scale with
  generation volume, while compiler checks scale with zero marginal cost.
  In axiom's composition grammar there is no corresponding way to write an
  architecture violation — illegal wiring does not compile, just as an
  out-of-bounds reference does not exist in safe Rust: it is not that
  circumvention was blocked; that way of writing is not in the language.
- **Mechanism 2: the blueprint is the surface for structural reasoning.**
  Wording discipline: the constraint is the way of writing, and the way of
  writing is the thinking. Compile errors are the last line of defense, not
  the first mechanism. Just as writing in Rust implicitly involves thinking
  in ownership and lifetimes, building with axiom involves the decomposition
  "which modules, composed how, converging into what system" — the
  composition tree is the system; the blueprint is not paperwork preceding
  the code. Three consequences:
  - Trial and error happens at the algebra layer: composition errors surface
    at compile time, and architecture experiments take seconds. When
    recombination is cheap and accretion expensive, the search process
    converges on compositional architecture.
  - Thinking is externalized into the artifact: once hidden semantics are
    absorbed by vocabulary (exclusive State, declared panic boundaries, time
    as a value, waiting only at boundaries), what remains is the meaning of
    each line itself. The artifact encodes thinking structurally in types
    and topology; context loss is the AI era's entropy increase, and
    externalized thinking is the neg-entropy mechanism. Successors — human
    or model — inherit readable thinking, not code to reverse-engineer.
  - Layer discipline steers where complexity flows: structural reasoning
    targets the definition-layer blueprint; physical adjustment targets the
    instance layer. Structural rot corresponds to complexity growing in the
    wrong layer, and wrong-layer composition is a compile error in axiom.
    Long-term maintenance does not fix one thing by breaking another.
- **Mechanism 3: the deterministic-simulation loop.** Mechanisms 1 and 2 make
  agent engineering expressible as an automated loop of generate, simulate,
  verify, iterate: the synchronous pure step and all-physics-through-seams
  make simulation identical to deployment semantics, with no human
  line-by-line review in the loop.

The health law (the charter's definition of "built"): a complex system whose
first line of code, on day one, grows on the right abstraction, and which
does not rot thereafter. Health has two components — runtime zero-cost
conservation (a foundations axiom) and the readability and maintainability
of the writing itself (Mechanism 2).

## 5. Substitution at Six Scales

The unified volume argues that static/dynamic, plugins, loading, and driver
hot-plug are two binding modes of one substitution operation. The charter
lists the operation's full range — six scales, one operation:

| Scale | Form of the operation | Decided at |
|---|---|---|
| Type level | Monomorphization (∀ elimination) | Compile time |
| Module level | Wiring (compile-time binding of residents) | Compile time |
| System level | Slots and plugins (∃ introduction) | Run time |
| Time level | Wait points (async = run-time binding of completion) | Run time |
| Topology level | Sharding (spatial substitution under the frame law) | Run time |
| Organizational level | Whole-instance replacement (the second-implementer discipline) | Build time or run time |

## 6. Evidence Discipline and Honest Boundaries

Each mechanism claim of the charter is bound to falsifiable predictions and
staged evidence; the current status is stated as it stands:

- Landed: the core's compile-time closure, zero-cost probes (benches), the
  obligation ledger, and the code projection of theorems T1–T9 (core volume
  §5).
- In progress: the second-implementer cross-check (the second fulfillment of
  T6); the adversarial evaluation of cancellation-as-topology.
- Site not yet reached: the falsifiable-prediction list (bound to the next
  of the four target systems actually studied).

The honest balance — three counterweights, stated alongside the mechanism
claims:

1. Adoption is a coordination problem. A constitution layer needs ratifiers;
   the first ratifying community is the actual builders of the four target
   systems. The evidence chain accumulates by stage; no single demo
   substitutes for it.
2. Base-layer timescales are measured in decades. A comparable precedent — a
   language-level async ecosystem from inception to ecosystem convergence —
   took close to a decade; scheduling follows the decade, not the quarter.
3. It competes with existing runtime conventions. The positioning is not
   another parallel convention but a compile target for the AI engineering
   toolchain: topology, obligations, and seam contracts are machine-
   consumable formal artifacts, read directly by generators and verifiers.

Open items (registered, not overlooked): the adversarial evaluation of the
cancellation-topology assumption; State inspectability, worst-case execution
time bounds, interrupt-driven activation, and the duplicate/out-of-order
delivery dimensions — all registered candidates for the constitutional
amendment process (see frontier-notes and the internal ledgers).

## 7. Reading Paths

- For "what it is": foundations (definitions, axioms, theorems) → core (the
  compile-time core) → semantics (the physical layer and seams) → unified
  (the unified view of substitution).
- For "why": this volume, §1–§4; the exposition through six scientific lenses
  is in the internal theory volume `positioning`.
- For "how it is built": the semantics constitution design is in the internal
  volume `semantics-constitution`; progress is in the CHANGELOG and the
  internal ledgers.
