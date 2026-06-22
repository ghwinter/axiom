//! 配置解析——从 config.toml 加载训练配置。

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub training: TrainingConfig,
    pub network: NetworkConfig,
    pub observe: ObserveConfig,
    pub persist: PersistConfig,
    pub runtime: RuntimeConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrainingConfig {
    pub dataset_size: usize,
    pub train_ratio: f64,
    pub batch_size: usize,
    pub learning_rate: f64,
    pub momentum: f64,
    pub epochs: u32,
    pub eval_interval: u64,
    pub checkpoint_interval: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    pub input_size: usize,
    pub hidden1_size: usize,
    pub hidden2_size: usize,
    pub output_size: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObserveConfig {
    pub sample_interval_ms: u64,
    pub metrics_file: String,
    pub snapshots_file: String,
    pub stdout_summary: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersistConfig {
    pub model_file: String,
    pub data_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub cpu_threads: usize,
    pub io_threads: usize,
}

impl Config {
    /// 从文件加载配置，如果文件不存在则用默认配置。
    pub fn load_or_default(path: &Path) -> Self {
        if path.exists() {
            let content = std::fs::read_to_string(path).expect("读取配置文件失败");
            toml::from_str(&content).expect("解析配置文件失败")
        } else {
            Self::default()
        }
    }

    pub fn default() -> Self {
        Self {
            training: TrainingConfig {
                dataset_size: 10000,
                train_ratio: 0.8,
                batch_size: 32,
                learning_rate: 0.01,
                momentum: 0.9,
                epochs: 10,
                eval_interval: 100,
                checkpoint_interval: 500,
            },
            network: NetworkConfig {
                input_size: 2,
                hidden1_size: 16,
                hidden2_size: 8,
                output_size: 1,
            },
            observe: ObserveConfig {
                sample_interval_ms: 1000,
                metrics_file: "output/metrics.jsonl".into(),
                snapshots_file: "output/snapshots.jsonl".into(),
                stdout_summary: true,
            },
            persist: PersistConfig {
                model_file: "output/model.bin".into(),
                data_file: "output/synthetic_data.csv".into(),
            },
            runtime: RuntimeConfig {
                cpu_threads: 0,
                io_threads: 2,
            },
        }
    }
}
