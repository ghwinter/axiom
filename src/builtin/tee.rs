/// Tee: fans out one input stream into two identical output streams.
///
/// `input → (output_a, output_b)` — each input produces TWO output copies,
/// one on each port, via `ProcessOutput::YieldMulti`.
use std::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

pub struct TeePorts<I>(PhantomData<I>);

#[derive(Debug, Clone, PartialEq)]
pub enum TeeInput<I> {
    Input(I),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TeeOutput<I> {
    OutputA(I),
    OutputB(I),
}

impl<I: Send + Clone + 'static> HasPortInfo for TeeInput<I> {
    fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<I>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<I>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "input" => { let v: Box<I> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
}

impl<I: Send + Sync + Clone + 'static> HasPortInfo for TeeOutput<I> {
    fn port_name(&self) -> &'static str {
        match self { Self::OutputA(_) => "output_a", Self::OutputB(_) => "output_b" }
    }
    fn flow_kind(&self) -> FlowKind {
        match self { Self::OutputA(_) | Self::OutputB(_) => FlowKind::Data }
    }
    fn payload_type_id(&self) -> core::any::TypeId {
        match self { Self::OutputA(_) | Self::OutputB(_) => core::any::TypeId::of::<I>() }
    }
    fn payload_type_name(&self) -> &'static str {
        match self { Self::OutputA(_) | Self::OutputB(_) => core::any::type_name::<I>() }
    }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name {
            "output_a" => { let v: Box<I> = payload.downcast().ok()?; Some(Self::OutputA(*v)) }
            "output_b" => { let v: Box<I> = payload.downcast().ok()?; Some(Self::OutputB(*v)) }
            _ => None,
        }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> {
        match self { Self::OutputA(v) => Box::new(v), Self::OutputB(v) => Box::new(v) }
    }
}

impl<I: Send + Sync + Clone + 'static> PortSet for TeePorts<I> {
    type Input = TeeInput<I>;
    type Output = TeeOutput<I>;

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<I>("input"))
            .with(PortDecl::output::<I>("output_a"))
            .with(PortDecl::output::<I>("output_b"))
    }
}

// ── Machine impl ────────────────────────────────────────────

pub struct Tee<I>(PhantomData<I>);

impl<I: Clone + Send + Sync + 'static> Machine for Tee<I> {
    type State = ();
    type Input = TeeInput<I>;
    type Output = TeeOutput<I>;
    type Ports = TeePorts<I>;

    fn name() -> &'static str { "builtin.Tee" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, input: TeeInput<I>) -> ProcessOutput<TeeOutput<I>> {
        match input {
            TeeInput::Input(v) => {
                // Fan-out: yield to BOTH output ports in a single step.
                // The runtime delivers output_a first, then output_b.
                ProcessOutput::YieldMulti(vec![
                    TeeOutput::OutputA(v.clone()),
                    TeeOutput::OutputB(v),
                ])
            }
        }
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
