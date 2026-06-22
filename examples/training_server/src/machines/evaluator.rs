//! 评估器——在验证集上评估模型性能。

use axiom::prelude_all::*;
use crate::types::*;
use crate::config::Config;
use ndarray::Array1;

declare_ports! {
    pub struct EvaluatorPorts {
        input type EvaluatorInput {
            model_delta[Data] => ModelDelta,
            ctrl[Control] => ControlSignal,
        }
        output type EvaluatorOutput {
            metrics[Data] => Metrics,
            stats[Observe] => ModuleStats,
        }
    }
}

pub struct EvaluatorState {
    pub eval_data: Vec<Sample>,
    pub weights: Vec<f64>,
    pub network_shape: (usize, usize, usize, usize),
    pub processed: u64,
    pub errors: u64,
    pub last_latency_us: u64,
    pub last_eval_loss: f64,
}

pub struct Evaluator;

impl Machine for Evaluator {
    type State = EvaluatorState;
    type Input = EvaluatorInput;
    type Output = EvaluatorOutput;
    type Ports = EvaluatorPorts;

    fn name() -> &'static str { "evaluator" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(ctx: &MachineContext) -> Result<EvaluatorState, InitError> {
        let config = ctx.initial_value::<Config>()
            .expect("Evaluator 需要 Config 注入");

        // 生成验证集（20% 数据）
        let eval_size = (config.training.dataset_size as f64 * (1.0 - config.training.train_ratio)) as usize;
        let eval_data = generate_eval_data(eval_size);

        Ok(EvaluatorState {
            eval_data,
            weights: Vec::new(),
            network_shape: (
                config.network.input_size,
                config.network.hidden1_size,
                config.network.hidden2_size,
                config.network.output_size,
            ),
            processed: 0,
            errors: 0,
            last_latency_us: 0,
            last_eval_loss: 0.0,
        })
    }

    fn process(state: &mut EvaluatorState, _ctx: &MachineContext, input: EvaluatorInput) -> ProcessOutput<EvaluatorOutput> {
        let start = std::time::Instant::now();

        match input {
            EvaluatorInput::ctrl(_) => ProcessOutput::Idle,
            EvaluatorInput::model_delta(delta) => {
                state.weights = delta.weights.clone();

                // 在验证集上评估
                let mut total_loss = 0.0;
                let mut total_abs_err = 0.0;

                for sample in &state.eval_data {
                    let pred = predict_with_weights(
                        &state.weights,
                        &state.network_shape,
                        &sample.features,
                    );
                    let err = pred - sample.label;
                    total_loss += err * err;
                    total_abs_err += err.abs();
                }

                let n = state.eval_data.len() as f64;
                let eval_loss = total_loss / n;
                let mae = total_abs_err / n;

                state.processed += 1;
                state.last_eval_loss = eval_loss;
                state.last_latency_us = start.elapsed().as_micros() as u64;

                let metrics = Metrics {
                    epoch: delta.epoch,
                    batch_id: delta.batch_id,
                    train_loss: delta.loss,
                    eval_loss,
                    mae,
                };

                let stats = ModuleStats {
                    module_name: "evaluator".into(),
                    processed_count: state.processed,
                    error_count: state.errors,
                    last_latency_us: state.last_latency_us,
                    timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                };

                ProcessOutput::YieldMulti(vec![
                    EvaluatorOutput::metrics(metrics),
                    EvaluatorOutput::stats(stats),
                ])
            }
        }
    }

    fn cleanup(_state: EvaluatorState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

/// 用扁平化权重做前向预测（不需要构建 Network 对象）。
fn predict_with_weights(
    weights: &[f64],
    shape: &(usize, usize, usize, usize),
    features: &[f64],
) -> f64 {
    let (input, h1, h2, output) = *shape;
    let mut cursor = 0;

    // Layer 1: [h1, input] weights + [h1] bias
    let l1_w_len = h1 * input;
    let mut hidden1 = vec![0.0; h1];
    for i in 0..h1 {
        for j in 0..input {
            hidden1[i] += weights[cursor + i * input + j] * features[j];
        }
        hidden1[i] += weights[cursor + l1_w_len + i]; // bias
        if hidden1[i] < 0.0 { hidden1[i] = 0.0; } // ReLU
    }
    cursor += l1_w_len + h1;

    // Layer 2: [h2, h1] weights + [h2] bias
    let l2_w_len = h2 * h1;
    let mut hidden2 = vec![0.0; h2];
    for i in 0..h2 {
        for j in 0..h1 {
            hidden2[i] += weights[cursor + i * h1 + j] * hidden1[j];
        }
        hidden2[i] += weights[cursor + l2_w_len + i];
        if hidden2[i] < 0.0 { hidden2[i] = 0.0; }
    }
    cursor += l2_w_len + h2;

    // Layer 3: [output, h2] weights + [output] bias (linear)
    let l3_w_len = output * h2;
    let mut out = vec![0.0; output];
    for i in 0..output {
        for j in 0..h2 {
            out[i] += weights[cursor + i * h2 + j] * hidden2[j];
        }
        out[i] += weights[cursor + l3_w_len + i];
    }

    out[0]
}

fn generate_eval_data(n: usize) -> Vec<Sample> {
    use std::cell::Cell;
    thread_local! {
        static SEED: Cell<u64> = Cell::new(99999);
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
        let y = x1.sin() + x2 * x2;
        samples.push(Sample { features: vec![x1, x2], label: y, seq: i as u64 });
    }
    samples
}
