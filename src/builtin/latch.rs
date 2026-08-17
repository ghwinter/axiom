/// Latch: a Moore-type delay element — outputs the previous input rather
/// than the current one.
///
/// `T → T` — used to break algebraic cycles in feedback topologies
/// (Theorem 1.2a).
///
/// Semantics: $\delta(s, i) = (s', \lambda(s))$, where $s' = i$ and
/// $\lambda(s) = s$. That is, the state holds the current input while the
/// output takes the old state. On the first call $s_0 = \text{None}$, yielding
/// `Idle`.
use core::marker::PhantomData;use crate::prelude_all::*;

// ── Port types ──────────────────────────────────────────────

// When the `derive` feature is enabled, the `#[ports]` macro generates the
// port boilerplate automatically; otherwise it is written by hand
// (preserving the zero-dependency capability).
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

    /// Moore-type delay: outputs the old state and stores the new input.
    /// Returns `Idle` on the first call (state = None).
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

// Latch's output `λ(s_old)` depends only on the pre-update state, not on the
// current input — this is the core mechanism for breaking algebraic cycles in
// feedback topologies (Theorem 1.2a). Implementing the `Moore` marker trait
// keeps the deployment layer's `is_moore` declaration consistent with the type
// layer, so that cycle-safety checks can recognize it.
impl<T: Clone + Send + Sync + 'static> Moore for Latch<T> {}


