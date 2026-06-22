//! 观测采样器——低频采样各模块状态，生成系统快照。

use axiom::prelude_all::*;
use crate::types::*;
use std::collections::HashMap;
use std::io::Write;

declare_ports! {
    pub struct ObserverPorts {
        input type ObserverInput {
            stats[Observe] => ModuleStats,
            loss[Observe] => Loss,
            metrics[Observe] => Metrics,
            ctrl[Control] => ControlSignal,
        }
        output type ObserverOutput {
            snapshot[Observe] => SystemSnapshot,
        }
    }
}

pub struct ObserverState {
    pub module_stats: HashMap<String, ModuleStats>,
    pub train_state: TrainState,
    pub current_epoch: u32,
    pub current_batch: u64,
    pub latest_loss: Option<f64>,
    pub latest_eval_loss: Option<f64>,
    pub snapshots_file: String,
    pub stdout_summary: bool,
    pub processed: u64,
    pub errors: u64,
    pub last_latency_us: u64,
    pub last_snapshot_at: std::time::Instant,
    pub sample_interval_ms: u64,
}

pub struct Observer;

impl Machine for Observer {
    type State = ObserverState;
    type Input = ObserverInput;
    type Output = ObserverOutput;
    type Ports = ObserverPorts;

    fn name() -> &'static str { "observer" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(ctx: &MachineContext) -> Result<ObserverState, InitError> {
        let config = ctx.initial_value::<crate::config::Config>()
            .expect("Observer 需要 Config 注入");

        // 确保输出目录存在
        if let Some(parent) = std::path::Path::new(&config.observe.snapshots_file).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        Ok(ObserverState {
            module_stats: HashMap::new(),
            train_state: TrainState::Idle,
            current_epoch: 0,
            current_batch: 0,
            latest_loss: None,
            latest_eval_loss: None,
            snapshots_file: config.observe.snapshots_file.clone(),
            stdout_summary: config.observe.stdout_summary,
            processed: 0,
            errors: 0,
            last_latency_us: 0,
            last_snapshot_at: std::time::Instant::now(),
            sample_interval_ms: config.observe.sample_interval_ms,
        })
    }

    fn process(state: &mut ObserverState, _ctx: &MachineContext, input: ObserverInput) -> ProcessOutput<ObserverOutput> {
        let start = std::time::Instant::now();

        match input {
            ObserverInput::ctrl(sig) => {
                match sig {
                    ControlSignal::Start => state.train_state = TrainState::Running,
                    ControlSignal::Stop => state.train_state = TrainState::Stopped,
                    ControlSignal::Pause => state.train_state = TrainState::Paused,
                    ControlSignal::Resume => state.train_state = TrainState::Running,
                    _ => {}
                }
            }
            ObserverInput::stats(stats) => {
                let name = stats.module_name.clone();
                state.module_stats.insert(name.clone(), stats.clone());

                // 从 trainer 的 stats 推断训练进度
                if name == "trainer" {
                    state.current_batch = stats.processed_count;
                }
            }
            ObserverInput::loss(l) => {
                state.latest_loss = Some(l.loss);
                state.current_epoch = l.epoch;
            }
            ObserverInput::metrics(m) => {
                state.latest_eval_loss = Some(m.eval_loss);
                state.current_epoch = m.epoch;
            }
        }

        state.processed += 1;
        state.last_latency_us = start.elapsed().as_micros() as u64;

        // 时间间隔采样：仅在距上次快照超过 sample_interval_ms 时才发射
        let elapsed_ms = state.last_snapshot_at.elapsed().as_millis() as u64;
        if elapsed_ms >= state.sample_interval_ms {
            state.last_snapshot_at = std::time::Instant::now();
            let snapshot = state.make_snapshot();
            state.write_snapshot(&snapshot);
            ProcessOutput::Yield(ObserverOutput::snapshot(snapshot))
        } else {
            ProcessOutput::Idle
        }
    }

    fn cleanup(_state: ObserverState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}

impl ObserverState {
    fn make_snapshot(&self) -> SystemSnapshot {
        SystemSnapshot {
            train_state: self.train_state,
            current_epoch: self.current_epoch,
            current_batch: self.current_batch,
            latest_loss: self.latest_loss,
            latest_eval_loss: self.latest_eval_loss,
            modules: self.module_stats.values().cloned().collect(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    fn write_snapshot(&self, snapshot: &SystemSnapshot) {
        // 写入 JSONL 文件
        if let Ok(json) = serde_json::to_string(snapshot) {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.snapshots_file)
            {
                let _ = writeln!(file, "{}", json);
            }
        }

        // stdout 摘要
        if self.stdout_summary {
            let loss_str = self.latest_loss
                .map(|l| format!("{:.6}", l))
                .unwrap_or_else(|| "N/A".to_string());
            let eval_str = self.latest_eval_loss
                .map(|l| format!("{:.6}", l))
                .unwrap_or_else(|| "N/A".to_string());

            println!(
                "[{}] state={:?} batch={} loss={} eval_loss={}",
                chrono::Utc::now().format("%H:%M:%S"),
                self.train_state,
                self.current_batch,
                loss_str,
                eval_str
            );
        }
    }
}
