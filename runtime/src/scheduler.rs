//! 调度器契约——runtime 内部子系统的可替换策略。
//!
//! # 结构一致性（runtime 内部也用"模块 + 契约"组织）
//!
//! runtime 是一个父系统，其子系统（调度、载体、生命周期、IO、重放）
//! 各自是**必要但可替换**的模块。外部 `RuntimeContract` 保证 runtime
//! 整体可替换（`docs/architecture.md`）；`Scheduler` 是**内部子系统
//! 的契约化**——调度策略（Sequential / Parallel / 未来自定义）是
//! 有限的执行形态集合，部署时选择（`design-principles.md` D1 在 runtime
//! 内部的落地）。
//!
//! # 可替换性
//!
//! `Runtime` 构造时按 `RuntimeConfig::mode` 选择调度器并持有
//! `Box<dyn Scheduler>`；自定义调度器 = 实现 [`Scheduler`] 并替换。
//! 调度器经 `&mut Runtime` 访问拓扑与配置（`drive_*` 保留为 Runtime
//! 方法——调度逻辑不复制，契约层转发）。

use crate::config::RuntimeConfig;
use crate::error::RuntimeError;
use crate::erasure::ProcessResult;

/// 调度器契约——驱动循环的可替换策略。
///
/// `tick` 注入外部 inputs，按拓扑传播，返回终端输出。实现者持有自己的
/// 策略（顺序 BFS、多线程、优先级…），经 `rt` 访问物化拓扑与配置。
pub trait Scheduler {
    /// 驱动一次 tick。
    fn tick(
        &self,
        rt: &mut crate::runtime::Runtime,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError>;
}

/// 顺序调度器：单线程 BFS 驱动 + 公平性配额（`Runtime::drive_sequential`）。
pub struct SequentialScheduler;

impl Scheduler for SequentialScheduler {
    fn tick(
        &self,
        rt: &mut crate::runtime::Runtime,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        rt.drive_sequential(inputs)
    }
}

/// 并行调度器：每机器一个 OS 线程 + channel 载体（`Runtime::drive_parallel`）。
pub struct ParallelScheduler {
    /// worker 线程数（声明参数；实际驱动读 `RuntimeConfig::mode`）。
    #[allow(dead_code)]
    pub workers: u32,
}

impl Scheduler for ParallelScheduler {
    fn tick(
        &self,
        rt: &mut crate::runtime::Runtime,
        inputs: Vec<(String, String, Box<dyn core::any::Any + Send>)>,
    ) -> Result<Vec<ProcessResult>, RuntimeError> {
        rt.drive_parallel(inputs)
    }
}

/// 构造默认调度器（按执行模式选择）。
pub(crate) fn default_scheduler(config: &RuntimeConfig) -> Box<dyn Scheduler> {
    match config.mode {
        crate::config::ExecMode::Parallel(n) if n >= 1 => {
            Box::new(ParallelScheduler { workers: n })
        }
        _ => Box::new(SequentialScheduler),
    }
}
