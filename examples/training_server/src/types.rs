//! 共享类型——所有 Machine 之间的数据载体。

use serde::{Deserialize, Serialize};

/// 合成数据样本：(x1, x2) → y，其中 y = sin(x1) + x2^2 + noise
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub features: Vec<f64>,
    pub label: f64,
    pub seq: u64,
}

/// 训练批次
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub features: Vec<Vec<f64>>,
    pub labels: Vec<f64>,
    pub batch_id: u64,
}

/// 训练损失
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loss {
    pub batch_id: u64,
    pub loss: f64,
    pub epoch: u32,
}

/// 模型增量（训练后的权重更新）
#[derive(Debug, Clone, PartialEq)]
pub struct ModelDelta {
    pub epoch: u32,
    pub batch_id: u64,
    pub loss: f64,
    pub weights: Vec<f64>,
}

/// 评估指标
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub epoch: u32,
    pub batch_id: u64,
    pub train_loss: f64,
    pub eval_loss: f64,
    pub mae: f64,
}

/// 控制信号
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlSignal {
    Start,
    Stop,
    Pause,
    Resume,
    Eval,
    Status,
}

/// 训练状态
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TrainState {
    Idle,
    Running,
    Paused,
    Stopped,
    Finished,
}

/// 各模块的观测统计——module_name 用 String 避免 'static 生命周期约束
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleStats {
    pub module_name: String,
    pub processed_count: u64,
    pub error_count: u64,
    pub last_latency_us: u64,
    pub timestamp_ms: u64,
}

/// 系统快照——Observer 采样所有模块状态后生成
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub train_state: TrainState,
    pub current_epoch: u32,
    pub current_batch: u64,
    pub latest_loss: Option<f64>,
    pub latest_eval_loss: Option<f64>,
    pub modules: Vec<ModuleStats>,
    pub timestamp_ms: u64,
}

impl SystemSnapshot {
    pub fn empty() -> Self {
        Self {
            train_state: TrainState::Idle,
            current_epoch: 0,
            current_batch: 0,
            latest_loss: None,
            latest_eval_loss: None,
            modules: Vec::new(),
            timestamp_ms: 0,
        }
    }
}

/// CLI 命令
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CliCommand {
    Start,
    Stop,
    Pause,
    Resume,
    Status,
    Eval,
    Quit,
}

impl CliCommand {
    pub fn to_control_signal(self) -> Option<ControlSignal> {
        match self {
            CliCommand::Start => Some(ControlSignal::Start),
            CliCommand::Stop => Some(ControlSignal::Stop),
            CliCommand::Pause => Some(ControlSignal::Pause),
            CliCommand::Resume => Some(ControlSignal::Resume),
            CliCommand::Status => Some(ControlSignal::Status),
            CliCommand::Eval => Some(ControlSignal::Eval),
            CliCommand::Quit => None,
        }
    }
}
