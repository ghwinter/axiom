/// Latch: holds the last received value and returns it.
///
/// `T → T` — useful for breaking cycles in feedback topologies.
/// State is `Option<T>`. First process() returns the input and stores it.
use std::marker::PhantomData;
use crate::prelude_all::*;

pub struct Latch<T>(PhantomData<T>);

impl<T: Clone + Send + Sync + 'static> Machine for Latch<T> {
    type State = Option<T>;
    type Input = T;
    type Output = T;


    fn name() -> &'static str { "builtin.Latch" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<T>("in"))
        .with(PortDecl::output::<T>("out"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Option<T>, InitError> { Ok(None) }
    fn process(state: &mut Option<T>, _ctx: &MachineContext, input: T) -> ProcessOutput<T> {
        *state = Some(input.clone());
        ProcessOutput::Yield(input)
    }
    fn cleanup(_state: Option<T>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
