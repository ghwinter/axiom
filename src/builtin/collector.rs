/// Collector: accumulates inputs in State, exposes via observe port.
///
/// `I → ∅` — useful for debugging pipelines and verifying upstream output.
use std::marker::PhantomData;
use crate::prelude_all::*;

pub struct Collector<I>(PhantomData<I>);

impl<I: Send + Sync + 'static> Machine for Collector<I> {
    type State = Vec<I>;
    type Input = I;
    type Output = ();
    type Observe = Vec<I>;

    fn name() -> &'static str { "builtin.Collector" }
    fn port_schema() -> PortSchema { PortSchema::new()
        .with(PortDecl::input::<I>("in"))
        .with(PortDecl::observe::<Vec<I>>("snapshots"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<Vec<I>, InitError> { Ok(Vec::new()) }
    fn process(state: &mut Vec<I>, _ctx: &MachineContext, input: I) -> ProcessOutput<()> {
        state.push(input);
        ProcessOutput::Idle
    }
    fn cleanup(_state: Vec<I>, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
