/// Latch: Moore 型延迟元素——输出上一次的输入，而非当前输入。
///
/// `T → T` — 用于打破反馈拓扑中的代数环（定理 1.2a）。
///
/// 语义：$\delta(s, i) = (s', \lambda(s))$，其中 $s' = i$，$\lambda(s) = s$。
/// 即：状态存当前输入，输出取旧状态。首次调用时 $s_0 = \text{None}$，输出 `Idle`。
use core::marker::PhantomData;use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

// 当 `derive` feature 启用时，用 `#[ports]` 宏自动生成端口样板；
// 否则手写（保持零依赖能力）。
#[cfg(feature = "derive")]
#[crate::ports]
pub struct LatchPorts<T> {
    #[input] input: T,
    #[output] output: T,
}

#[cfg(not(feature = "derive"))]
mod manual_ports {
    #[cfg(not(feature = "std"))]
    use crate::compat::prelude::*;
    use core::marker::PhantomData;
    use crate::prelude_all::*;

    pub struct LatchPorts<T>(PhantomData<T>);

    #[derive(Debug, Clone, PartialEq)]
    pub enum LatchInput<T> {
        Input(T),
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum LatchOutput<T> {
        Output(T),
    }

    impl<T: Send + 'static> HasPortInfo for LatchInput<T> {
        fn port_name(&self) -> &'static str { match self { Self::Input(_) => "input" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Input(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Input(_) => core::any::TypeId::of::<T>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Input(_) => core::any::type_name::<T>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "input" => { let v: Box<T> = payload.downcast().ok()?; Some(Self::Input(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Input(v) => Box::new(v) } }
    }

    impl<T: Send + Sync + 'static> HasPortInfo for LatchOutput<T> {
        fn port_name(&self) -> &'static str { match self { Self::Output(_) => "output" } }
        fn flow_kind(&self) -> FlowKind { match self { Self::Output(_) => FlowKind::Data } }
        fn payload_type_id(&self) -> core::any::TypeId { match self { Self::Output(_) => core::any::TypeId::of::<T>() } }
        fn payload_type_name(&self) -> &'static str { match self { Self::Output(_) => core::any::type_name::<T>() } }
        fn from_port_name(name: &str, payload: Box<dyn core::any::Any + Send>) -> Option<Self> {
            match name { "output" => { let v: Box<T> = payload.downcast().ok()?; Some(Self::Output(*v)) } _ => None }
        }
        fn into_any(self) -> Box<dyn core::any::Any + Send> { match self { Self::Output(v) => Box::new(v) } }
    }

    impl<T: Send + Sync + 'static> PortSet for LatchPorts<T> {
        type Input = LatchInput<T>;
        type Output = LatchOutput<T>;

        fn port_schema() -> PortSchema {
            PortSchema::new()
                .with(PortDecl::input::<T>("input"))
                .with(PortDecl::output::<T>("output"))
        }
    }
}

#[cfg(not(feature = "derive"))]
pub use manual_ports::{LatchPorts, LatchInput, LatchOutput};

// ── Machine impl ────────────────────────────────────────────

pub struct Latch<T>(PhantomData<T>);

impl<T: Clone + Send + Sync + 'static> Machine for Latch<T> {
    type State = Option<T>;
    type Input = LatchInput<T>;
    type Output = LatchOutput<T>;
    type Ports = LatchPorts<T>;
    type ProcessOutput = SingleOutput<Self::Output>;

    fn name() -> &'static str { "builtin.Latch" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Option<T>, InitError> { Ok(None) }

    /// Moore 型延迟：输出旧状态，存入新输入。
    /// 首次调用（state = None）返回 Idle。
    fn process(state: &mut Option<T>, _ctx: &MachineContext, input: LatchInput<T>) -> SingleOutput<LatchOutput<T>> {
        match input {
            LatchInput::Input(v) => {
                let old = state.take();
                *state = Some(v);
                match old {
                    None => SingleOutput::Idle,
                    Some(prev) => SingleOutput::Yield(LatchOutput::Output(prev)),
                }
            }
        }
    }

    fn cleanup(_state: Option<T>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

// Latch 的输出 `λ(s_old)` 仅依赖更新前状态，与当前输入无关——
// 这是定理 1.2a 中打破反馈拓扑代数环的核心机制。实现 `Moore` marker
// trait 使部署层 `is_moore` 声明与类型层一致，cycle-safety 检查可识别。
impl<T: Clone + Send + Sync + 'static> Moore for Latch<T> {}


