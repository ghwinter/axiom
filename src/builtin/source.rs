/// Source: produces a constant value on every process() call.
///
/// `∅ → O` — no input, yields `Default::default()`.
/// For non-default outputs, set the output via `f` field at init.
use std::marker::PhantomData;
use crate::prelude_all::*;

pub struct Source<O>(PhantomData<O>);

pub struct SourceState<O> {
    pub output: O,
}

impl<O: Clone + Default + Send + Sync + 'static> Source<O> {
    pub fn new() -> Self { Source(PhantomData) }
}

impl<O: Clone + Default + Send + Sync + 'static> Machine for Source<O> {
    type State = SourceState<O>;
    type Input = ();
    type Output = O;


    fn name() -> &'static str { "builtin.Source" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::output::<O>("out"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<SourceState<O>, InitError> {
        Ok(SourceState { output: O::default() })
    }
    fn process(state: &mut SourceState<O>, _ctx: &MachineContext, _input: ()) -> ProcessOutput<O> {
        ProcessOutput::Yield(state.output.clone())
    }
    fn cleanup(_state: SourceState<O>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
