/// Collector: accumulates inputs in State, exposes via observe port.
///
/// `input → observe(snapshots)` — each input is appended to State,
/// and the full accumulated vector is yielded on the observe port.
///
/// This fixes a previous inconsistency where the observe port was declared
/// in `port_schema()` but `CollectorOutput` was a zero-variant enum,
/// making the observe data unreachable. Now `CollectorOutput::Snapshots`
/// carries the observation, and `process()` yields it on every input.
use std::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

pub struct CollectorPorts<I>(PhantomData<I>);

#[derive(Debug, Clone, PartialEq)]
pub enum CollectorInput<I> {
    Input(I),
}

/// Collector has no data output, only an observe port.
/// The observe variant carries the accumulated snapshots.
#[derive(Debug, Clone, PartialEq)]
pub enum CollectorOutput<I> {
    Snapshots(Vec<I>),
}

impl<I: Send + Clone + 'static> HasPortInfo for CollectorInput<I> {
    fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<I>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<I>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "input" => { let v: Box<I> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
}

impl<I: Send + Sync + Clone + 'static> HasPortInfo for CollectorOutput<I> {
    fn port_name(&self) -> &'static str { match self { Self::Snapshots(_) => "snapshots" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Snapshots(_) => FlowKind::Observe } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Snapshots(_) => core::any::TypeId::of::<Vec<I>>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Snapshots(_) => core::any::type_name::<Vec<I>>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name {
            "snapshots" => { let v: Box<Vec<I>> = payload.downcast().ok()?; Some(Self::Snapshots(*v)) }
            _ => None,
        }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Snapshots(v) => Box::new(v) } }
}

impl<I: Send + Sync + Clone + 'static> PortSet for CollectorPorts<I> {
    type Input = CollectorInput<I>;
    type Output = CollectorOutput<I>;

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<I>("input"))
            .with(PortDecl::observe::<Vec<I>>("snapshots"))
    }
}

// ── Machine impl ────────────────────────────────────────────

pub struct Collector<I>(PhantomData<I>);

impl<I: Send + Sync + Clone + 'static> Machine for Collector<I> {
    type State = Vec<I>;
    type Input = CollectorInput<I>;
    type Output = CollectorOutput<I>;
    type Ports = CollectorPorts<I>;

    fn name() -> &'static str { "builtin.Collector" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Vec<I>, InitError> { Ok(Vec::new()) }
    fn process(state: &mut Vec<I>, _ctx: &MachineContext, input: CollectorInput<I>) -> ProcessOutput<CollectorOutput<I>> {
        match input {
            CollectorInput::Input(v) => {
                state.push(v);
                // Yield the full snapshot on the observe port.
                ProcessOutput::Yield(CollectorOutput::Snapshots(state.clone()))
            }
        }
    }
    fn cleanup(_state: Vec<I>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
