/// FuncMachine: wraps any `Func` into a `Machine`.
///
/// This bridges the two computation primitives: a pure function becomes
/// a stateful machine (State = ()) that can be connected into any topology.
#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use core::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

// Note: this file does not use the `#[crate::ports]` macro. FuncMachine's port
// enums are based on the associated types `F::Input` / `F::Output`, and the
// hand-written `Clone`/`PartialEq` impls deliberately require only
// `F::Input: Clone` / `F::Output: Clone` rather than `F: Clone` (see the impl
// comments below). The `#[ports]` macro applies
// `#[derive(Debug, Clone, PartialEq)]` uniformly to the generated enums,
// which would impose `Clone`/`PartialEq` bounds on the generic parameter `F`
// (the conservative behavior of std derive), tightening the bounds permitted
// by the `Func` trait (which requires only `Send + Sync + 'static`) — a
// semantic regression. The hand-written version is therefore kept.

pub struct FuncMachinePorts<F>(PhantomData<F>);

#[derive(Debug)]
pub enum FuncMachineInput<F: Func> {
    Input(F::Input),
}

// Manual Clone impl: only requires F::Input: Clone, not F: Clone.
impl<F: Func> Clone for FuncMachineInput<F>
where
    F::Input: Clone,
{
    fn clone(&self) -> Self {
        match self { Self::Input(v) => Self::Input(v.clone()) }
    }
}

impl<F: Func> PartialEq for FuncMachineInput<F>
where
    F::Input: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) { (Self::Input(a), Self::Input(b)) => a == b }
    }
}

#[derive(Debug)]
pub enum FuncMachineOutput<F: Func> {
    Output(F::Output),
}

impl<F: Func> Clone for FuncMachineOutput<F>
where
    F::Output: Clone,
{
    fn clone(&self) -> Self {
        match self { Self::Output(v) => Self::Output(v.clone()) }
    }
}

impl<F: Func> PartialEq for FuncMachineOutput<F>
where
    F::Output: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) { (Self::Output(a), Self::Output(b)) => a == b }
    }
}

impl<F: Func> HasPortInfo for FuncMachineInput<F>
where
    F::Input: Clone,
{
    fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<F::Input>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<F::Input>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "input" => { let v: Box<F::Input> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
}

impl<F: Func> HasPortInfo for FuncMachineOutput<F>
where
    F::Output: Clone,
{
    fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<F::Output>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<F::Output>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "output" => { let v: Box<F::Output> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
}

impl<F: Func + Send + Sync> PortSet for FuncMachinePorts<F>
where
    F::Input: Clone,
    F::Output: Clone,
{
    type Input = FuncMachineInput<F>;
    type Output = FuncMachineOutput<F>;

    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<F::Input>("input"))
            .with(PortDecl::output::<F::Output>("output"))
    }
}

// ── Machine impl ────────────────────────────────────────────

pub struct FuncMachine<F>(PhantomData<F>);

impl<F: Func> Machine for FuncMachine<F>
where
    F::Input: Clone,
    F::Output: Clone,
{
    type State = ();
    type Input = FuncMachineInput<F>;
    type Output = FuncMachineOutput<F>;
    type Ports = FuncMachinePorts<F>;
    type ProcessOutput = SingleOutput<Self::Output>;

    fn name() -> &'static str { F::name() }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, input: FuncMachineInput<F>) -> SingleOutput<FuncMachineOutput<F>> {
        match input {
            FuncMachineInput::Input(v) => SingleOutput::Yield(FuncMachineOutput::Output(F::call(v))),
        }
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { !F::nondeterministic() }
}

// ── Straight contract (unified static entry point) ──────────
//
// FuncMachine is the bridge that connects a pure function into the static
// path's combinators: `F::Input → F::Output`, calling `F::call` directly on
// the raw payload without enum wrapping/unwrapping.

impl<F: Func> StraightMachine for FuncMachine<F>
where
    F::Input: Clone,
    F::Output: Clone,
{
    type StraightIn = F::Input;
    type StraightOut = F::Output;

    #[inline]
    fn process_straight(_state: &mut (), input: F::Input) -> F::Output {
        F::call(input)
    }
}
