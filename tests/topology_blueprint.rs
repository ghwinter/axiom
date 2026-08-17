//! Blueprint concept integration test (unified topology declaration language) — validates
//! the refactored capabilities from outside the crate.
//!
//! External acceptance of the refactor (unified static entry point + blueprint concept
//! + naming convergence), validating the three core promises of the axiom vision:
//!
//! 1. **One blueprint concept, two materialization paths**: `Topology` unifies the
//!    "three topology languages" — the static projection (`StraightMachine` single
//!    machine, `Chain`/`Diamond`/`Composite` combinators) and the runtime projection
//!    (`DynamicTopology` value form, `TopologyMutation` instance-changes temporal
//!    form, `CompositeSpec` subgraph reuse) all implement the same `Topology`.
//! 2. **Zero-cost abstraction (non-invasive axiom)**: `StaticTopology` is a
//!    **zero-sized marker** — the compile-time projection disappears from the build
//!    artifact, with no runtime topology object; executing through the `T: StaticChain`
//!    generic bound (the contract surface of the unified static entry point) is
//!    **semantically identical** to a direct call — the abstraction does not change
//!    the execution shape.
//! 3. **Determinism**: deterministic machines with the same input reach the same terminal state.

use axiom::prelude_all::*;

// ── Test machines (Machine enum contract + StraightMachine bare-payload contract) ──
// Isomorphic to the static_exec unit tests — re-validates the semantics from an
// external perspective, and plugs into StaticTopology.

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DoublerPorts {
        input type DoublerInput {
            x[Data] => i32,
        }
        output type DoublerOutput {
            y[Data] => i32,
        }
    }
}

pub struct Doubler;
impl Machine for Doubler {
    type State = ();
    type Input = DoublerInput;
    type Output = DoublerOutput;
    type Ports = DoublerPorts;
    type ProcessOutput = SingleOutput<DoublerOutput>;
    fn name() -> &'static str { "doubler" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_: &mut (), _: &MachineContext, input: DoublerInput) -> SingleOutput<DoublerOutput> {
        match input {
            DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
        }
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
impl StraightMachine for Doubler {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(_: &mut (), n: i32) -> i32 { n * 2 }
}

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct AdderPorts {
        input type AdderInput {
            x[Data] => i32,
        }
        output type AdderOutput {
            y[Data] => i32,
        }
    }
}

pub struct Adder;
impl Machine for Adder {
    type State = i32;
    type Input = AdderInput;
    type Output = AdderOutput;
    type Ports = AdderPorts;
    type ProcessOutput = SingleOutput<AdderOutput>;
    fn name() -> &'static str { "adder" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<i32, InitError> { Ok(0) }
    fn process(state: &mut i32, _: &MachineContext, input: AdderInput) -> SingleOutput<AdderOutput> {
        match input {
            AdderInput::x(n) => {
                *state += n;
                SingleOutput::Yield(AdderOutput::y(*state))
            }
        }
    }
    fn cleanup(_: i32, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
impl StraightMachine for Adder {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(state: &mut i32, n: i32) -> i32 {
        *state += n;
        *state
    }
}

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct TriplerPorts {
        input type TriplerInput {
            x[Data] => i32,
        }
        output type TriplerOutput {
            y[Data] => i32,
        }
    }
}

pub struct Tripler;
impl Machine for Tripler {
    type State = ();
    type Input = TriplerInput;
    type Output = TriplerOutput;
    type Ports = TriplerPorts;
    type ProcessOutput = SingleOutput<TriplerOutput>;
    fn name() -> &'static str { "tripler" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }
    fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_: &mut (), _: &MachineContext, input: TriplerInput) -> SingleOutput<TriplerOutput> {
        match input {
            TriplerInput::x(n) => SingleOutput::Yield(TriplerOutput::y(n * 3)),
        }
    }
    fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
impl StraightMachine for Tripler {
    type StraightIn = i32;
    type StraightOut = i32;
    #[inline]
    fn process_straight(_: &mut (), n: i32) -> i32 { n * 3 }
}

// ── Bare-payload merge (StraightMerge) ─────────────────────────────────────────

struct Sum;
impl StraightMerge<i32, i32> for Sum {
    type Output = i32;
    #[inline]
    fn merge(a: i32, b: i32) -> i32 { a + b }
}

// ── Combinators (compile-time shapes of the unified static entry point) ─────────

/// 3-stage linear chain: Doubler → Adder → Tripler.
type Chain3 = Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>;

/// Diamond: Doubler → StraightClone → (Adder, Tripler) → Sum → Adder.
type DiamondShape = Diamond<
    Doubler,
    Adder,
    Tripler,
    Adder,
    StraightClone,
    StraightId,
    StraightId,
    Sum,
>;

/// Composite: wraps a chain (subgraph reuse, transparent forwarding).
type Composite3 = Composite<Chain3>;

// ════════════════════════════════════════════════════════════════════════════
// 1. Blueprint unification: three topology languages → one Topology concept
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn blueprint_unifies_three_topology_languages() {
    fn assert_blueprint<T: Topology>() {}

    // Compile-time projection (static path: combinators + StraightMachine machines)
    assert_blueprint::<Doubler>();
    assert_blueprint::<Chain3>();
    assert_blueprint::<DiamondShape>();
    assert_blueprint::<Composite3>();

    // Runtime projection (value form: a declared type-erased topology)
    assert_blueprint::<DynamicTopology>();
    // Runtime projection (temporal form: instance changes)
    assert_blueprint::<TopologyMutation>();
    // Composite (subgraph-reuse mechanism, folded into the blueprint concept)
    assert_blueprint::<CompositeSpec>();
}

// ════════════════════════════════════════════════════════════════════════════
// 2. StaticTopology: the compile-time projection serves as a type-level contract (T: StaticTopology bound)
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn static_projection_is_typed_contract() {
    fn assert_static<T: StaticTopology>() {}

    assert_static::<Doubler>();
    assert_static::<Chain3>();
    assert_static::<DiamondShape>();
    assert_static::<Composite3>();
}

// ════════════════════════════════════════════════════════════════════════════
// 3. Zero cost (non-invasive axiom): StaticTopology is a zero-sized marker with no runtime topology object
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn static_projection_is_zero_sized_marker() {
    assert_eq!(std::mem::size_of::<Doubler>(), 0);
    assert_eq!(std::mem::size_of::<Chain3>(), 0);
    assert_eq!(std::mem::size_of::<DiamondShape>(), 0);
    assert_eq!(std::mem::size_of::<Composite3>(), 0);
}

// ════════════════════════════════════════════════════════════════════════════
// 4. Unified static entry point: executing through the T: StaticChain generic bound is
//    semantically identical to a direct call
//    (the abstraction produces an execution shape equivalent to handwritten code —
//    zero-cost abstraction means execution-shape isomorphism)
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn unified_static_entry_is_semantically_transparent() {
    // The contract surface of the unified static entry point: any T: StaticChain can
    // execute (the single entry to the static path).
    // Sources/sinks inside the generic body are fixed by the type system (the bare input
    // type of T::Head); physical execution needs zero validation.
    fn run_static<T>(inputs: Vec<i32>) -> Vec<T::Out>
    where
        T: StaticChain,
        T::Head: StraightMachine<StraightIn = i32>,
    {
        T::run_all(inputs).expect("static run")
    }

    // Generic-bound path vs direct-call path: semantically identical (the abstraction
    // does not change the execution shape).
    assert_eq!(Chain3::run_all(vec![1, 2]).unwrap(), run_static::<Chain3>(vec![1, 2]));
    assert_eq!(
        DiamondShape::run_all(vec![1, 2]).unwrap(),
        run_static::<DiamondShape>(vec![1, 2])
    );
    assert_eq!(
        Composite3::run_all(vec![1, 2]).unwrap(),
        run_static::<Composite3>(vec![1, 2])
    );

    // Hand-computed semantic anchors (verifying the combinator math):
    // Chain3: D(2) → A(2) → T(6)
    assert_eq!(run_static::<Chain3>(vec![1]), vec![6]);
    // Diamond: D(2,4) → split(2,2),(4,4) → A(2,6), T(6,12) → Sum(8,18) → A(8,26)
    assert_eq!(run_static::<DiamondShape>(vec![1, 2]), vec![8, 26]);
    // Composite is semantically identical to the inner topology (transparent forwarding).
    assert_eq!(run_static::<Composite3>(vec![1, 2]), run_static::<Chain3>(vec![1, 2]));
}

// ════════════════════════════════════════════════════════════════════════════
// 5. Runtime projection: the declaration (value form) ↔ instance (temporal form)
//    can be converted both ways, and the form can keep evolving
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn dynamic_value_form_roundtrips_to_instance_form() {
    let spec = DynamicTopology::new()
        .with_machine(MachineInstance::new(
            "a",
            "doubler",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "b",
            "adder",
            MachinePhysicalSpec::default(),
        ))
        .with_link(LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline));

    // Value form → instance form (from_spec: dynamic evolution starts after deploying
    // the static spec).
    let mut topo = TopologyMutation::from_spec(&spec);
    assert_eq!(topo.machine_count(), 2);
    assert_eq!(topo.link_count(), 1);

    // Instance form → value form (snapshot: checkpoint / migrate back to a static deployment).
    let snap = topo.snapshot();
    assert_eq!(snap.links.len(), 1);
    assert_eq!(
        snap.links[0],
        LinkSpec::new(("a", "out"), ("b", "in"), LinkKind::Inline)
    );
    let names: Vec<String> = snap.machines.iter().map(|m| m.name.to_string()).collect();
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"b".to_string()));

    // The instance form is a "temporal" form: it keeps evolving on top of the declaration (Spawn).
    topo.apply(TopologyOp::Spawn {
        name: "c",
        machine_type: "tripler",
        physical: MachinePhysicalSpec::default(),
        config_overrides: vec![],
    })
    .unwrap();
    assert_eq!(topo.machine_count(), 3);
}

// ════════════════════════════════════════════════════════════════════════════
// 6. Determinism: same input → same terminal state
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn determinism_same_input_same_terminal_state() {
    // All machines declare deterministic; running twice with the same input sequence
    // yields bit-identical output.
    let inputs = vec![1, 2, 3, 4, 5];
    let a = Chain3::run_all(inputs.clone()).unwrap();
    let b = Chain3::run_all(inputs).unwrap();
    assert_eq!(a, b);

    // Determinism is a type-level/metadata declaration that the runtime can rely on
    // (the precondition for replay safety).
    assert!(<Adder as Machine>::deterministic());
    assert!(<Doubler as Machine>::deterministic());
}
