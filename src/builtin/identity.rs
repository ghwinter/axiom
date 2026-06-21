/// Identity: the category-theoretic identity morphism.
///
/// `input → output` — passes input through unchanged.
/// State is `()`, zero overhead.
use std::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

pub struct IdentityPorts<I>(PhantomData<I>);

#[derive(Debug, Clone, PartialEq)]
pub enum IdentityInput<I> {
    Input(I),
}

#[derive(Debug, Clone, PartialEq)]
pub enum IdentityOutput<I> {
    Output(I),
}

impl<I: Send + Clone + 'static> HasPortInfo for IdentityInput<I> {
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

impl<I: Send + Sync + Clone + 'static> HasPortInfo for IdentityOutput<I> {
    fn port_name(&self) -> &'static str {
        match self { Self::Output(_) => "output" }
    }
    fn flow_kind(&self) -> FlowKind {
        match self { Self::Output(_) => FlowKind::Data }
    }
    fn payload_type_id(&self) -> core::any::TypeId {
        match self { Self::Output(_) => core::any::TypeId::of::<I>() }
    }
    fn payload_type_name(&self) -> &'static str {
        match self { Self::Output(_) => core::any::type_name::<I>() }
    }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name {
            "output" => { let v: Box<I> = payload.downcast().ok()?; Some(Self::Output(*v)) }
            _ => None,
        }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> {
        match self { Self::Output(v) => Box::new(v) }
    }
}

impl<I: Send + Sync + Clone + 'static> PortSet for IdentityPorts<I> {
    type Input = IdentityInput<I>;
    type Output = IdentityOutput<I>;

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<I>("input"))
            .with(PortDecl::output::<I>("output"))
    }
}

// ── Machine impl ────────────────────────────────────────────

pub struct Identity<I>(PhantomData<I>);

impl<I: Send + Sync + Clone + 'static> Machine for Identity<I> {
    type State = ();
    type Input = IdentityInput<I>;
    type Output = IdentityOutput<I>;
    type Ports = IdentityPorts<I>;

    fn name() -> &'static str { "builtin.Identity" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, input: IdentityInput<I>) -> ProcessOutput<IdentityOutput<I>> {
        match input {
            IdentityInput::Input(v) => ProcessOutput::Yield(IdentityOutput::Output(v)),
        }
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
