/// Latch: holds the last received value and returns it.
///
/// `T → T` — useful for breaking cycles in feedback topologies.
/// State is `Option<T>`. First process() returns the input and stores it.
use std::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

pub struct LatchPorts<T>(PhantomData<T>);

#[derive(Debug, Clone, PartialEq)]
pub enum LatchInput<T> {
    Input(T),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LatchOutput<T> {
    Output(T),
}

impl<T: Send + Clone + 'static> HasPortInfo for LatchInput<T> {
    fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<T>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<T>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "input" => { let v: Box<T> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
}

impl<T: Send + Sync + Clone + 'static> HasPortInfo for LatchOutput<T> {
    fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<T>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<T>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "output" => { let v: Box<T> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
}

impl<T: Send + Sync + Clone + 'static> PortSet for LatchPorts<T> {
    type Input = LatchInput<T>;
    type Output = LatchOutput<T>;

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<T>("input"))
            .with(PortDecl::output::<T>("output"))
    }
}

// ── Machine impl ────────────────────────────────────────────

pub struct Latch<T>(PhantomData<T>);

impl<T: Clone + Send + Sync + 'static> Machine for Latch<T> {
    type State = Option<T>;
    type Input = LatchInput<T>;
    type Output = LatchOutput<T>;
    type Ports = LatchPorts<T>;

    fn name() -> &'static str { "builtin.Latch" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Option<T>, InitError> { Ok(None) }
    fn process(state: &mut Option<T>, _ctx: &MachineContext, input: LatchInput<T>) -> ProcessOutput<LatchOutput<T>> {
        match input {
            LatchInput::Input(v) => {
                *state = Some(v.clone());
                ProcessOutput::Yield(LatchOutput::Output(v))
            }
        }
    }
    fn cleanup(_state: Option<T>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
