/// Identity: the category-theoretic identity morphism.
///
/// `I → I` — passes input through unchanged.
/// State is `()`, zero overhead.
///
/// Satisfies the unit law:
///   Identity ⨟ Machine == Machine ⨟ Identity == Machine
use std::marker::PhantomData;
use crate::prelude_all::*;

pub struct Identity<I>(PhantomData<I>);

impl<I: Send + Sync + 'static> Machine for Identity<I> {
    type State = ();
    type Input = I;
    type Output = I;


    fn name() -> &'static str { "builtin.Identity" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<I>("in"))
        .with(PortDecl::output::<I>("out"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, input: I) -> ProcessOutput<I> {
        ProcessOutput::Yield(input)
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
