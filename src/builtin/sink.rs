/// Sink: discards all input.
///
/// `input → ∅` — consumes but never produces.
#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use core::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

// Note: this file does not use the `#[crate::ports]` macro. Sink's port
// signature is an asymmetric generic: `SinkInput<I>` carries the generic
// parameter `I`, while `SinkOutput` has no generic parameter (a
// zero-variant, uninhabited enum). The `#[ports]` macro propagates all of
// the struct's generic parameters uniformly to both the Input and Output
// enums, which would produce a `SinkOutput<I>` — a zero-variant enum that
// nonetheless carries a generic parameter `I`. Since no variant of a
// zero-variant enum references `I`, rustc reports E0392 (type parameter `I`
// is never used). The hand-written version is therefore kept, and
// `SinkOutput` stays non-generic.
//
// The remaining builtins with symmetric generic parameters and a non-empty
// Output (Identity/Tee/Latch/Collector) have been migrated to the macro.

pub struct SinkPorts<I>(PhantomData<I>);

#[derive(Debug, Clone, PartialEq)]
pub enum SinkInput<I> {
    Input(I),
}

/// Zero-variant enum: Sink has no output ports, so `Machine::Output = SinkOutput` is
/// uninhabited. `ProcessOutput::Yield` can never be constructed for a Sink.
#[derive(Debug, Clone, PartialEq)]
pub enum SinkOutput {}

impl<I: Send + 'static> HasPortInfo for SinkInput<I> {
    fn port_name(&self) -> &'static str {
        match self { Self::Input(_) => "input" }
    }
    fn flow_kind(&self) -> FlowKind {
        match self { Self::Input(_) => FlowKind::Data }
    }
    fn payload_type_id(&self) -> core::any::TypeId {
        match self { Self::Input(_) => core::any::TypeId::of::<I>() }
    }
    fn payload_type_name(&self) -> &'static str {
        match self { Self::Input(_) => core::any::type_name::<I>() }
    }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name {
            "input" => { let v: Box<I> = payload.downcast().ok()?; Some(Self::Input(*v)) }
            _ => None,
        }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> {
        match self { Self::Input(v) => Box::new(v) }
    }
}

impl HasPortInfo for SinkOutput {
    fn port_name(&self) -> &'static str { match *self {} }
    fn flow_kind(&self) -> FlowKind { match *self {} }
    fn payload_type_id(&self) -> core::any::TypeId { match *self {} }
    fn payload_type_name(&self) -> &'static str { match *self {} }
    fn from_port_name(_name: &str, _payload: Box<dyn core::any::Any + Send>) -> Option<Self> { None }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self {} }
}

impl<I: Send + Sync + 'static> PortSet for SinkPorts<I> {
    type Input = SinkInput<I>;
    type Output = SinkOutput;

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<I>("input"))
    }
}

// ── Machine impl ────────────────────────────────────────────

pub struct Sink<I>(PhantomData<I>);

impl<I: Send + Sync + Clone + 'static> Machine for Sink<I> {
    type State = ();
    type Input = SinkInput<I>;
    type Output = SinkOutput;
    type Ports = SinkPorts<I>;
    type ProcessOutput = SingleOutput<Self::Output>;

    fn name() -> &'static str { "builtin.Sink" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, _input: SinkInput<I>) -> SingleOutput<SinkOutput> {
        SingleOutput::Idle
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

// ── Straight contract (unified static entry point) ──────────
//
// Sink acts as the "discard terminal" of the static path: `I → ()`.
// `StraightOut = ()` means there is no downstream; when used as the tail
// machine of a `Chain`/`Diamond` it naturally swallows the payload.

impl<I: Send + Sync + Clone + 'static> StraightMachine for Sink<I> {
    type StraightIn = I;
    type StraightOut = ();

    #[inline]
    fn process_straight(_state: &mut (), _input: I) {}
}
