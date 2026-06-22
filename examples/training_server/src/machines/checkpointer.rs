//! 检查点器——将模型和指标持久化到文件。

use axiom::prelude_all::*;
use crate::types::*;
use crate::config::Config;
use std::io::Write;

declare_ports! {
    pub struct CheckpointerPorts {
        input type CheckpointerInput {
            model_delta[Data] => ModelDelta,
            metrics[Data] => Metrics,
        }
        output type CheckpointerOutput {
            stats[Observe] => ModuleStats,
        }
    }
}

pub struct CheckpointerState {
    pub model_file: String,
    pub metrics_file: String,
    pub checkpoint_count: u64,
    pub processed: u64,
    pub errors: u64,
    pub last_latency_us: u64,
}

pub struct Checkpointer;

impl Machine for Checkpointer {
    type State = CheckpointerState;
    type Input = CheckpointerInput;
    type Output = CheckpointerOutput;
    type Ports = CheckpointerPorts;

    fn name() -> &'static str { "checkpointer" }
    fn config_schema() -> ConfigSchema { ConfigSchema::new() }

    fn init(ctx: &MachineContext) -> Result<CheckpointerState, InitError> {
        let config = ctx.initial_value::<Config>()
            .expect("Checkpointer 需要 Config 注入");

        // 确保输出目录存在
        if let Some(parent) = std::path::Path::new(&config.persist.model_file).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Some(parent) = std::path::Path::new(&config.observe.metrics_file).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        Ok(CheckpointerState {
            model_file: config.persist.model_file.clone(),
            metrics_file: config.observe.metrics_file.clone(),
            checkpoint_count: 0,
            processed: 0,
            errors: 0,
            last_latency_us: 0,
        })
    }

    fn process(state: &mut CheckpointerState, _ctx: &MachineContext, input: CheckpointerInput) -> ProcessOutput<CheckpointerOutput> {
        let start = std::time::Instant::now();

        match input {
            CheckpointerInput::model_delta(delta) => {
                // 保存模型到二进制文件
                match bincode::serialize(&delta.weights) {
                    Ok(data) => {
                        if let Err(e) = std::fs::write(&state.model_file, &data) {
                            eprintln!("[checkpointer] 保存模型失败: {}", e);
                            state.errors += 1;
                        } else {
                            state.checkpoint_count += 1;
                        }
                    }
                    Err(e) => {
                        eprintln!("[checkpointer] 序列化失败: {}", e);
                        state.errors += 1;
                    }
                }
                state.processed += 1;
            }
            CheckpointerInput::metrics(metrics) => {
                // 追加指标到 JSONL 文件
                match serde_json::to_string(&metrics) {
                    Ok(json) => {
                        match std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&state.metrics_file)
                        {
                            Ok(mut file) => {
                                if let Err(e) = writeln!(file, "{}", json) {
                                    eprintln!("[checkpointer] 写入指标失败: {}", e);
                                    state.errors += 1;
                                }
                            }
                            Err(e) => {
                                eprintln!("[checkpointer] 打开指标文件失败: {}", e);
                                state.errors += 1;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[checkpointer] JSON 序列化失败: {}", e);
                        state.errors += 1;
                    }
                }
                state.processed += 1;
            }
        }

        state.last_latency_us = start.elapsed().as_micros() as u64;

        let stats = ModuleStats {
            module_name: "checkpointer".into(),
            processed_count: state.processed,
            error_count: state.errors,
            last_latency_us: state.last_latency_us,
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        };

        ProcessOutput::Yield(CheckpointerOutput::stats(stats))
    }

    fn cleanup(_state: CheckpointerState, _ctx: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    fn deterministic() -> bool { true }
}
