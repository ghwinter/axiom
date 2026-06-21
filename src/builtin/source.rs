/// Source: produces a constant value on every process() call.
///
/// `∅ → output` — no input, yields `Default::default()`.
use std::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

pub struct SourcePorts<O>(PhantomData<O>);

#[derive(Debug, Clone, PartialEq)]
pub enum SourceInput {
    Tick(()),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceOutput<O> {
    Output(O),
}

impl HasPortInfo for SourceInput {
    fn port_name(&self) -> &'static str { match self { Self::Tick(_) => "tick" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Tick(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Tick(_) => core::any::TypeId::of::<()>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Tick(_) => core::any::type_name::<()>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "tick" => { let _: Box<()> = payload.downcast().ok()?; Some(Self::Tick(())) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Tick(v) => Box::new(v) } }
}

impl<O: Send + Sync + Clone + 'static> HasPortInfo for SourceOutput<O> {
    fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<O>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<O>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "output" => { let v: Box<O> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
}

impl<O: Send + Sync + Clone + 'static> PortSet for SourcePorts<O> {
    type Input = SourceInput;
    type Output = SourceOutput<O>;

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<()>("tick"))
            .with(PortDecl::output::<O>("output"))
    }
}

pub struct SourceState<O> {
    pub output: O,
}

// ── Machine impl ────────────────────────────────────────────

pub struct Source<O>(PhantomData<O>);

impl<O: Clone + Default + Send + Sync + 'static> Machine for Source<O> {
    type State = SourceState<O>;
    type Input = SourceInput;
    type Output = SourceOutput<O>;
    type Ports = SourcePorts<O>;

    fn name() -> &'static str { "builtin.Source" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SourceState<O>, InitError> {
        Ok(SourceState { output: O::default() })
    }
    fn process(state: &mut SourceState<O>, _ctx: &MachineContext, _input: SourceInput) -> ProcessOutput<SourceOutput<O>> {
        ProcessOutput::Yield(SourceOutput::Output(state.output.clone()))
    }
    fn cleanup(_state: SourceState<O>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
