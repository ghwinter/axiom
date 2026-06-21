/// Latch: Moore 型延迟元素——输出上一次的输入，而非当前输入。
///
/// `T → T` — 用于打破反馈拓扑中的代数环（定理 1.2a）。
///
/// 语义：$\delta(s, i) = (s', \lambda(s))$，其中 $s' = i$，$\lambda(s) = s$。
/// 即：状态存当前输入，输出取旧状态。首次调用时 $s_0 = \text{None}$，输出 `Idle`（工程修补 1.2a）。
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

    /// Moore 型延迟：输出旧状态，存入新输入。
    /// 首次调用（state = None）返回 Idle（工程修补 1.2a）。
    fn process(state: &mut Option<T>, _ctx: &MachineContext, input: LatchInput<T>) -> ProcessOutput<LatchOutput<T>> {
        match input {
            LatchInput::Input(v) => {
                let old = state.take();
                *state = Some(v);
                match old {
                    None => ProcessOutput::Idle,
                    Some(prev) => ProcessOutput::Yield(LatchOutput::Output(prev)),
                }
            }
        }
    }

    fn cleanup(_state: Option<T>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
