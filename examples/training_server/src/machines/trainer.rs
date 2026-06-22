//! 训练器——使用 Rayon 并行处理 batch，前向 + 反向传播 + SGD 更新。

use axiom::prelude_all::*;
use crate::types::*;
use crate::nn::{Network, SgdOptimizer};
use crate::config::Config;
use ndarray::Array1;
use rayon::prelude::*;

declare_ports! {
    pub struct TrainerPorts {
        input type TrainerInput {
            batch[Data] => Batch,
            ctrl[Control] => ControlSignal,
        }
        output type TrainerOutput {
            loss[Data] => Loss,
            model_delta[Data] => ModelDelta,
            stats[Observe] => ModuleStats,
        }
    }
}

pub struct TrainerState {
    pub network: Network,
    pub optimizer: SgdOptimizer,
    pub epoch: u32,
    pub current_batch: u64,
    pub learning_rate: f64,
    pub eval_interval: u64,
    pub running: bool,
    pub processed: u64,
    pub errors: u64,
    pub last_latency_us: u64,
    pub last_loss: f64,
}

pub struct Trainer;

impl Machine for Trainer {
    type State = TrainerState;
    type Input = TrainerInput;
    type Output = TrainerOutput;
    type Ports = TrainerPorts;

    fn name() -> &'static str { "trainer" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(ctx: &MachineContext) -> Result<TrainerState, InitError> {
        let config = ctx.initial_value::<Config>()
            .expect("Trainer 需要 Config 注入");

        let mut network = Network::new(
            config.network.input_size,
            config.network.hidden1_size,
            config.network.hidden2_size,
            config.network.output_size,
        );

        let mut optimizer = SgdOptimizer::new(
            config.training.learning_rate,
            config.training.momentum,
        );
        optimizer.init_buffers(&[
            &network.layer1, &network.layer2, &network.layer3,
        ]);

        Ok(TrainerState {
            network,
            optimizer,
            epoch: 0,
            current_batch: 0,
            learning_rate: config.training.learning_rate,
            eval_interval: config.training.eval_interval,
            running: false,
            processed: 0,
            errors: 0,
            last_latency_us: 0,
            last_loss: 0.0,
        })
    }

    fn process(state: &mut TrainerState, _ctx: &MachineContext, input: TrainerInput) -> ProcessOutput<TrainerOutput> {
        let start = std::time::Instant::now();

        match input {
            TrainerInput::ctrl(sig) => {
                match sig {
                    ControlSignal::Start | ControlSignal::Resume => state.running = true,
                    ControlSignal::Stop | ControlSignal::Pause => state.running = false,
                    _ => {}
                }
                return ProcessOutput::Idle;
            }
            TrainerInput::batch(batch) => {
                if !state.running {
                    return ProcessOutput::Idle;
                }

                // 使用 Rayon 并行计算每个样本的前向传播
                let batch_size = batch.features.len();
                let predictions: Vec<f64> = batch.features.par_iter()
                    .map(|features| {
                        let x = Array1::from_vec(features.clone());
                        // 这里不能直接调用 network.forward（需要 &mut），
                        // 所以我们只做前向计算，不修改网络
                        forward_only(&state.network, &x)
                    })
                    .collect();

                // 计算 MSE 损失
                let total_loss: f64 = predictions.iter()
                    .zip(batch.labels.iter())
                    .map(|(p, t)| { let d = p - t; d * d })
                    .sum::<f64>() / batch_size as f64;

                // 反向传播（串行，因为需要修改网络）
                state.network.zero_grad();
                for (features, label) in batch.features.iter().zip(batch.labels.iter()) {
                    let x = Array1::from_vec(features.clone());
                    let pred = state.network.forward(&x);
                    let target = Array1::from_vec(vec![*label]);
                    state.network.backward(&pred, &target);
                }

                // SGD 更新
                state.optimizer.step_layer(0, &mut state.network.layer1);
                state.optimizer.step_layer(1, &mut state.network.layer2);
                state.optimizer.step_layer(2, &mut state.network.layer3);

                state.current_batch += 1;
                state.processed += 1;
                state.last_loss = total_loss;
                state.last_latency_us = start.elapsed().as_micros() as u64;

                let loss = Loss {
                    batch_id: batch.batch_id,
                    loss: total_loss,
                    epoch: state.epoch,
                };

                let model_delta = ModelDelta {
                    epoch: state.epoch,
                    batch_id: batch.batch_id,
                    loss: total_loss,
                    weights: state.network.weights_flatten(),
                };

                let stats = ModuleStats {
                    module_name: "trainer".into(),
                    processed_count: state.processed,
                    error_count: state.errors,
                    last_latency_us: state.last_latency_us,
                    timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                };

                ProcessOutput::YieldMulti(vec![
                    TrainerOutput::loss(loss),
                    TrainerOutput::model_delta(model_delta),
                    TrainerOutput::stats(stats),
                ])
            }
        }
    }

    fn cleanup(_state: TrainerState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

/// 只做前向传播，不修改网络（用于 Rayon 并行）。
fn forward_only(network: &Network, x: &Array1<f64>) -> f64 {
    // 复制权重做前向计算，避免 &mut
    let h1 = network.layer1.weights.dot(x) + &network.layer1.bias;
    let h1: Array1<f64> = h1.mapv(|v| if v > 0.0 { v } else { 0.0 });
    let h2 = network.layer2.weights.dot(&h1) + &network.layer2.bias;
    let h2: Array1<f64> = h2.mapv(|v| if v > 0.0 { v } else { 0.0 });
    let out = network.layer3.weights.dot(&h2) + &network.layer3.bias;
    out[0]
}
