//! Output-type contracts — A1 (Clone-free `HasPortInfo`) + A2 (`TupleOutput` /
//! `FusedCompatible`) from the psql pressure-point audit.
//!
//! These tests pin the type-system guarantees:
//! 1. A payload that does **not** implement `Clone` can cross ports
//!    (`HasPortInfo` no longer carries a `Clone` bound) — both via the
//!    `In`/`Out` wrappers and via `declare_ports!`-generated enums.
//! 2. `TupleOutput` is a fixed two-output type (no data fan-out) and is
//!    `FusedCompatible`, so a data+observe machine can implement
//!    `FusedInline`; `SingleOutput` remains fused-compatible too.
//! 3. `TupleOutput` maps correctly onto the unified `ProcessOutput` for
//!    generic runtimes.

use axiom::declare_ports;
use axiom::flow::FlowKind;
use axiom::machine::{
    CleanupError, FusedCompatible, FusedInline, InitError, Machine, MachineOutput,
    ProcessOutput, SingleOutput, TupleOutput,
};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::portset::{HasPortInfo, Out};
use core::any::TypeId;

// ════════════════════════════════════════════════════════════════════════════
// A1 — non-Clone payloads cross ports
// ════════════════════════════════════════════════════════════════════════════

/// A payload type that deliberately does **not** implement `Clone`.
#[derive(Debug, PartialEq)]
struct NoClone {
    fd: i32,
}

fn assert_has_port_info<T: HasPortInfo>() {}

#[test]
fn non_clone_payload_satisfies_has_port_info_wrapper() {
    // `Out<NoClone>` must implement `HasPortInfo` without `NoClone: Clone`.
    assert_has_port_info::<Out<NoClone>>();

    let out = Out(NoClone { fd: 7 });
    assert_eq!(out.port_name(), "output");
    assert_eq!(out.flow_kind(), FlowKind::Data);
    assert_eq!(out.payload_type_id(), TypeId::of::<NoClone>());
    assert_eq!(out.payload_type_name(), core::any::type_name::<NoClone>());

    // Type-erased transport: move semantics, no clone involved.
    let erased = out.into_any();
    let back: Out<NoClone> = HasPortInfo::from_port_name("output", erased).unwrap();
    assert_eq!(back.0, NoClone { fd: 7 });
}

declare_ports! {
    #[derive(Debug, PartialEq)]
    pub struct NoClonePorts {
        input type NoCloneInput {
            req [Data] => NoClone,
        }
        output type NoCloneOutput {
            resp [Data] => NoClone,
        }
    }
}

#[test]
fn non_clone_payload_works_with_declare_ports_macro() {
    // The macro-generated enums carry a non-Clone payload yet still implement
    // `HasPortInfo` (compilation is the assertion).
    assert_has_port_info::<NoCloneInput>();
    assert_has_port_info::<NoCloneOutput>();

    let inp = NoCloneInput::req(NoClone { fd: 1 });
    assert_eq!(inp.port_name(), "req");
    let erased = inp.into_any();
    let back: NoCloneInput = HasPortInfo::from_port_name("req", erased).unwrap();
    assert_eq!(back, NoCloneInput::req(NoClone { fd: 1 }));
}

// ════════════════════════════════════════════════════════════════════════════
// A2 — TupleOutput: fixed two-output machine, FusedCompatible
// ════════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct DemoPorts {
        input type DemoInput {
            sql [Data] => String,
        }
        output type DemoOutput {
            rows   [Data]    => u32,
            status [Observe] => String,
        }
    }
}

/// A data+observe machine using the fixed two-output type.
struct Demo;

impl Machine for Demo {
    type State = u32;
    type Input = DemoInput;
    type Output = DemoOutput;
    type Ports = DemoPorts;
    type ProcessOutput = TupleOutput<DemoOutput>;

    fn name() -> &'static str {
        "demo"
    }
    fn config_schema() -> ConfigSchema {
        ConfigSchema::new()
    }
    fn init(_: &MachineContext) -> Result<u32, InitError> {
        Ok(0)
    }
    #[inline]
    fn process(s: &mut u32, _: &MachineContext, _: DemoInput) -> TupleOutput<DemoOutput> {
        *s += 1;
        TupleOutput::Yield(
            DemoOutput::rows(*s),
            DemoOutput::status(format!("n={}", *s)),
        )
    }
    fn cleanup(_: u32, _: &MachineContext) -> Result<(), CleanupError> {
        Ok(())
    }
}

// A `TupleOutput` machine CAN enter the fused pipeline path — the key A2 win.
impl FusedInline for Demo {}

#[test]
fn tuple_output_machine_is_fused_inline() {
    // `impl FusedInline for Demo` above compiles — the type-system assertion.
    // Re-assert on the concrete type (a generic `M: FusedInline` helper would
    // fail to resolve `FusedInline`'s where-clause at definition time).
    fn assert_demo_fused()
    where
        Demo: FusedInline,
    {
    }
    assert_demo_fused();
}

#[test]
fn single_and_tuple_outputs_are_fused_compatible() {
    fn assert_single_fc()
    where
        SingleOutput<DemoOutput>: FusedCompatible,
    {
    }
    fn assert_tuple_fc()
    where
        TupleOutput<DemoOutput>: FusedCompatible,
    {
    }
    assert_single_fc();
    assert_tuple_fc();
}

#[test]
fn tuple_output_semantics() {
    // into_process_output → unified runtime type (variant-tag remap).
    let out = TupleOutput::Yield(DemoOutput::rows(42), DemoOutput::status("ok".into()));
    match out.into_process_output() {
        ProcessOutput::YieldMulti(v) => {
            assert_eq!(v.len(), 2);
            assert_eq!(v[0], DemoOutput::rows(42));
            assert_eq!(v[1], DemoOutput::status("ok".into()));
        }
        other => panic!("expected YieldMulti, got {:?}", other),
    }

    // into_outputs → (values, done).
    let out2 = TupleOutput::Yield(DemoOutput::rows(1), DemoOutput::status("s".into()));
    let (vals, done) = out2.into_outputs();
    assert!(!done);
    assert_eq!(vals.len(), 2);

    let (vals, done) = TupleOutput::<DemoOutput>::Idle.into_outputs();
    assert!(!done);
    assert!(vals.is_empty());

    let (vals, done) = TupleOutput::<DemoOutput>::Done.into_outputs();
    assert!(done);
    assert!(vals.is_empty());
}
