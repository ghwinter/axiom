/// Sink: discards all input.
///
/// `input → ∅` — consumes but never produces.
use std::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

pub struct SinkPorts<I>(PhantomData<I>);

#[derive(Debug, Clone, PartialEq)]
pub enum SinkInput<I> {
    Input(I),
}

/// Zero-variant enum: Sink has no output ports, so `Machine::Output = SinkOutput` is
/// uninhabited. `ProcessOutput::Yield` can never be constructed for a Sink.
#[derive(Debug, Clone, PartialEq)]
pub enum SinkOutput {}

impl<I: Send + Clone + 'static> HasPortInfo for SinkInput<I> {
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

impl<I: Send + Sync + Clone + 'static> PortSet for SinkPorts<I> {
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

    fn name() -> &'static str { "builtin.Sink" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, _input: SinkInput<I>) -> ProcessOutput<SinkOutput> {
        ProcessOutput::Idle
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
