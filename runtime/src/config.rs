//! runtime 配置——执行模式与物理参数集中于此。

/// runtime 的执行模式——单线程与多线程不是两个类型，而是配置参数。
///
/// - `Inline`：调用方线程直接执行，零线程开销
/// - `Sequential`：单线程顺序循环
/// - `Parallel(n)`：N 个 worker 线程并行调度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    Inline,
    Sequential,
    Parallel(u32),
}

impl Default for ExecMode {
    fn default() -> Self {
        ExecMode::Sequential
    }
}

/// runtime 配置——所有物理执行参数集中在此。
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// 执行模式（线程数策略）。
    pub mode: ExecMode,
    /// 驱动循环每 tick 的最大 process 次数（防止无限循环）。
    /// `None` = 无限制。
    pub max_ticks: Option<u64>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            mode: ExecMode::default(),
            max_ticks: Some(1_000_000),
        }
    }
}

impl RuntimeConfig {
    pub fn inline() -> Self { Self { mode: ExecMode::Inline, max_ticks: None } }
    pub fn sequential() -> Self { Self { mode: ExecMode::Sequential, max_ticks: Some(1_000_000) } }
    pub fn parallel(n: u32) -> Self { Self { mode: ExecMode::Parallel(n), max_ticks: Some(10_000_000) } }
}
