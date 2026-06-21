/// Tee: fans out one input stream into two identical output streams.
///
/// `I → (I, I)` — each input produces two output copies.
use std::marker::PhantomData;
use crate::prelude_all::*;

pub struct Tee<I>(PhantomData<I>);

impl<I: Clone + Send + Sync + 'static> Machine for Tee<I> {
    type State = ();
    type Input = I;
    type Output = (I, I);


    fn name() -> &'static str { "builtin.Tee" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<I>("in"))
        .with(PortDecl::output::<(I, I)>("out"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, input: I) -> ProcessOutput<(I, I)> {
        ProcessOutput::Yield((input.clone(), input))
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
