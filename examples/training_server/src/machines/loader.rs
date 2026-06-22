//! 数据加载器——生成合成数据并逐条输出 Sample。

use axiom::prelude_all::*;
use crate::types::*;
use crate::config::Config;

// ── 端口定义（宏自动生成 Input/Output enum + PortSet impl + HasPortInfo impl）──

declare_ports! {
    pub struct DataLoaderPorts {
        input type DataLoaderInput {
            ctrl[Control] => ControlSignal,
            tick[Data] => u64,
        }
        output type DataLoaderOutput {
            sample[Data] => Sample,
            stats[Observe] => ModuleStats,
        }
    }
}

// ── 状态 ──────────────────────────────────────────────

pub struct DataLoaderState {
    pub samples: Vec<Sample>,
    pub cursor: usize,
    pub running: bool,
    pub processed: u64,
    pub errors: u64,
    pub last_latency_us: u64,
}

// ── Machine 实现 ─────────────────────────────────────

pub struct DataLoader;

impl Machine for DataLoader {
    type State = DataLoaderState;
    type Input = DataLoaderInput;
    type Output = DataLoaderOutput;
    type Ports = DataLoaderPorts;

    fn name() -> &'static str { "data_loader" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(ctx: &MachineContext) -> Result<DataLoaderState, InitError> {
        let config = ctx.initial_value::<Config>()
            .expect("DataLoader 需要 Config 注入");
        let samples = generate_synthetic_data(config.training.dataset_size);

        Ok(DataLoaderState {
            samples,
            cursor: 0,
            running: false,
            processed: 0,
            errors: 0,
            last_latency_us: 0,
        })
    }

    fn process(state: &mut DataLoaderState, _ctx: &MachineContext, input: DataLoaderInput) -> ProcessOutput<DataLoaderOutput> {
        let start = std::time::Instant::now();

        match input {
            DataLoaderInput::ctrl(sig) => {
                match sig {
                    ControlSignal::Start | ControlSignal::Resume => state.running = true,
                    ControlSignal::Stop | ControlSignal::Pause => state.running = false,
                    _ => {}
                }
                ProcessOutput::Idle
            }
            DataLoaderInput::tick(_) => {
                if !state.running || state.cursor >= state.samples.len() {
                    if state.cursor >= state.samples.len() {
                        return ProcessOutput::Done;
                    }
                    return ProcessOutput::Idle;
                }

                let sample = state.samples[state.cursor].clone();
                state.cursor += 1;
                state.processed += 1;
                state.last_latency_us = start.elapsed().as_micros() as u64;

                ProcessOutput::YieldMulti(vec![
                    DataLoaderOutput::sample(sample),
                    DataLoaderOutput::stats(ModuleStats {
                        module_name: "data_loader".into(),
                        processed_count: state.processed,
                        error_count: state.errors,
                        last_latency_us: state.last_latency_us,
                        timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                    }),
                ])
            }
        }
    }

    fn cleanup(_state: DataLoaderState, _ctx: &MachineContext) -> Result<(), CleanupError> {
        Ok(())
    }

    fn deterministic() -> bool { true }
}

/// 生成合成数据：y = sin(x1) + x2^2 + noise
fn generate_synthetic_data(n: usize) -> Vec<Sample> {
    use std::cell::Cell;
    thread_local! {
        static SEED: Cell<u64> = Cell::new(12345);
    }

    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let x1 = SEED.with(|s| {
            let mut x = s.get();
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            s.set(x);
            (x as f64 / u64::MAX as f64) * 10.0 - 5.0
        });
        let x2 = SEED.with(|s| {
            let mut x = s.get();
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            s.set(x);
            (x as f64 / u64::MAX as f64) * 4.0 - 2.0
        });
        let noise = SEED.with(|s| {
            let mut x = s.get();
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            s.set(x);
            (x as f64 / u64::MAX as f64 - 0.5) * 0.5
        });

        let y = x1.sin() + x2 * x2 + noise;
        samples.push(Sample {
            features: vec![x1, x2],
            label: y,
            seq: i as u64,
        });
    }
    samples
}
