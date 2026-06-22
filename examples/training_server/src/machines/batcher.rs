//! 批处理组装器——将单个 Sample 累积成 Batch。

use axiom::prelude_all::*;
use crate::types::*;

declare_ports! {
    pub struct BatcherPorts {
        input type BatcherInput {
            sample[Data] => Sample,
        }
        output type BatcherOutput {
            batch[Data] => Batch,
            stats[Observe] => ModuleStats,
        }
    }
}

pub struct BatcherState {
    pub buffer: Vec<Sample>,
    pub batch_size: usize,
    pub batch_id: u64,
    pub processed: u64,
    pub errors: u64,
    pub last_latency_us: u64,
}

pub struct Batcher;

impl Machine for Batcher {
    type State = BatcherState;
    type Input = BatcherInput;
    type Output = BatcherOutput;
    type Ports = BatcherPorts;

    fn name() -> &'static str { "batcher" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(ctx: &MachineContext) -> Result<BatcherState, InitError> {
        let config = ctx.initial_value::<crate::config::Config>()
            .expect("Batcher 需要 Config 注入");
        Ok(BatcherState {
            buffer: Vec::with_capacity(config.training.batch_size),
            batch_size: config.training.batch_size,
            batch_id: 0,
            processed: 0,
            errors: 0,
            last_latency_us: 0,
        })
    }

    fn process(state: &mut BatcherState, _ctx: &MachineContext, input: BatcherInput) -> ProcessOutput<BatcherOutput> {
        let start = std::time::Instant::now();

        let sample = match input {
            BatcherInput::sample(s) => s,
        };

        state.buffer.push(sample);
        state.processed += 1;

        if state.buffer.len() >= state.batch_size {
            let features: Vec<Vec<f64>> = state.buffer.iter().map(|s| s.features.clone()).collect();
            let labels: Vec<f64> = state.buffer.iter().map(|s| s.label).collect();
            state.buffer.clear();
            state.batch_id += 1;
            state.last_latency_us = start.elapsed().as_micros() as u64;

            let batch = Batch { features, labels, batch_id: state.batch_id };
            let stats = ModuleStats {
                module_name: "batcher".into(),
                processed_count: state.processed,
                error_count: state.errors,
                last_latency_us: state.last_latency_us,
                timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            };

            ProcessOutput::YieldMulti(vec![
                BatcherOutput::batch(batch),
                BatcherOutput::stats(stats),
            ])
        } else {
            ProcessOutput::Idle
        }
    }

    fn cleanup(_state: BatcherState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
