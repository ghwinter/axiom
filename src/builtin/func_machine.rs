/// FuncMachine: wraps any `Func` into a `Machine`.
///
/// This bridges the two computation primitives: a pure function becomes
/// a stateful machine (State = ()) that can be connected into any topology.
///
/// In category theory terms, this is embedding the sub-category of Funcs
/// into the larger category of Machines — every Func is also a Machine.
use std::marker::PhantomData;
use crate::prelude_all::*;

pub struct FuncMachine<F>(PhantomData<F>);

impl<F: Func> Machine for FuncMachine<F> {
    type State = ();
    type Input = F::Input;
    type Output = F::Output;

    fn name() -> &'static str { F::name() }
    fn port_schema() -> PortSchema {
        PortSchema::new()
            .with(PortDecl::input::<F::Input>("in"))
            .with(PortDecl::output::<F::Output>("out"))
    }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
    fn process(_state: &mut (), _ctx: &MachineContext, input: F::Input) -> ProcessOutput<F::Output> {
        ProcessOutput::Yield(F::call(input))
    }
    fn cleanup(_state: (), _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { !F::nondeterministic() }
}
