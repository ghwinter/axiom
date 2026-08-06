/// Source: produces a constant value on every process() call.
///
/// `∅ → output` — yields the injected initial value, or `Default::default()` if none.
#[cfg(not(feature = "std"))]
use crate::compat::prelude::*;
use core::marker::PhantomData;
use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

// 注：本文件未采用 `#[crate::ports]` 宏。Source 的端口签名是不对称泛型：
// `SourceInput` 无泛型（tick 端口承载 `()`），而 `SourceOutput<O>` 带泛型 `O`。
// `#[ports]` 宏会把 struct 的全部泛型统一传播到 Input/Output 两个枚举，
// 这将产生 `SourceInput<O>`——给一个会被实际构造（`Tick(())`）的输入枚举
// 强加无意义的幻影泛型 `O`，破坏构造人机工程学与既有公开 API。因此保留手写。
//
// 其余对称泛型的 builtin（Identity/Tee/Latch/Collector/Sink）均已迁移到宏。

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

impl<O: Send + Sync + 'static> HasPortInfo for SourceOutput<O> {
    fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
    fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
    fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<O>() } }
    fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<O>() } }
    fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
        match name { "output" => { let v: Box<O> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
    }
    fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
}

impl<O: Send + Sync + 'static> PortSet for SourcePorts<O> {
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
    type ProcessOutput = SingleOutput<Self::Output>;

    fn name() -> &'static str { "builtin.Source" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(ctx: &MachineContext) -> Result<SourceState<O>, InitError> {
        let output = ctx.initial_value::<O>().cloned().unwrap_or_default();
        Ok(SourceState { output })
    }
    fn process(state: &mut SourceState<O>, _ctx: &MachineContext, _input: SourceInput) -> SingleOutput<SourceOutput<O>> {
        SingleOutput::Yield(SourceOutput::Output(state.output.clone()))
    }
    fn cleanup(_state: SourceState<O>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
