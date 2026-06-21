/// Sink: discards all input.
///
/// `I → ∅` — consumes but never produces.
use std::marker::PhantomData;
use crate::prelude_all::*;

pub struct Sink<I>(PhantomData<I>);

impl<I: Send + Sync + 'static> Machine for Sink<I> {
    type State = ();
    type Input = I;
    type Output = ();
    type Observe = ();

    fn name() -> &'static str { "builtin.Sink" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<I>("in"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, _input: I) -> ProcessOutput<()> {
        ProcessOutput::Idle
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
