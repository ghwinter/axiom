//! 控制接口——接收 CLI 命令，分发控制信号到各模块。

use axiom::prelude_all::*;
use crate::types::*;

declare_ports! {
    pub struct ControllerPorts {
        input type ControllerInput {
            command[Control] => CliCommand,
        }
        output type ControllerOutput {
            ctrl_signal[Control] => ControlSignal,
            stats[Observe] => ModuleStats,
        }
    }
}

pub struct ControllerState {
    pub processed: u64,
    pub errors: u64,
    pub last_latency_us: u64,
}

pub struct Controller;

impl Machine for Controller {
    type State = ControllerState;
    type Input = ControllerInput;
    type Output = ControllerOutput;
    type Ports = ControllerPorts;

    fn name() -> &'static str { "controller" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(_ctx: &MachineContext) -> Result<ControllerState, InitError> {
        Ok(ControllerState {
            processed: 0,
            errors: 0,
            last_latency_us: 0,
        })
    }

    fn process(state: &mut ControllerState, _ctx: &MachineContext, input: ControllerInput) -> ProcessOutput<ControllerOutput> {
        let start = std::time::Instant::now();

        let cmd = match input {
            ControllerInput::command(cmd) => cmd,
        };

        state.processed += 1;
        state.last_latency_us = start.elapsed().as_micros() as u64;

        match cmd.to_control_signal() {
            Some(sig) => {
                let stats = ModuleStats {
                    module_name: "controller".into(),
                    processed_count: state.processed,
                    error_count: state.errors,
                    last_latency_us: state.last_latency_us,
                    timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                };
                ProcessOutput::YieldMulti(vec![
                    ControllerOutput::ctrl_signal(sig),
                    ControllerOutput::stats(stats),
                ])
            }
            None => {
                // Quit 命令，无控制信号
                ProcessOutput::Done
            }
        }
    }

    fn cleanup(_state: ControllerState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
