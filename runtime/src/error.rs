//! runtime 物化或驱动过程中的错误类型。

/// runtime 物化或驱动过程中的错误。
#[derive(Debug)]
pub enum RuntimeError {
    /// `MachineInstance::name` 包含非 `'static` 数据（owned String），
    /// 而 `MachineContext::new` 要求 `&'static str`。
    NonStaticName { instance: String },

    /// 指定的 link kind 在当前 runtime 实现中不支持。
    UnsupportedLinkKind { kind: String, hint: String },

    /// 拓扑中引用了不存在的 machine 或 port。
    DanglingRef { machine: String, port: String },

    /// 拓扑在 Parallel 模式下无法物化（如 Source 类无输入端口机器）。
    UnsupportedTopology { machine: String, reason: String },

    /// Machine init 失败。
    InitFailed { machine: String, error: axiom::machine::InitError },

    /// 驱动循环达到 max_ticks 仍未完成。
    TickLimitExceeded { ticks: u64 },

    /// cleanup 失败。
    CleanupFailed { machine: String },

    /// IO 多路复用操作失败（reactor register / poll 错误）。
    IoFailed { error: crate::io::IoError },

    /// 复合 Machine 嵌套深度超过上限（可能为复合自引用导致无限展开）。
    CompositeTooDeep { depth: usize, hint: String },
}

impl core::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NonStaticName { instance } => write!(
                f, "machine instance `{instance}` has non-'static name"
            ),
            Self::UnsupportedLinkKind { kind, hint } => write!(
                f, "link kind `{kind}` not supported: {hint}"
            ),
            Self::DanglingRef { machine, port } => write!(
                f, "topology references non-existent endpoint ({machine}, {port})"
            ),
            Self::UnsupportedTopology { machine, reason } => write!(
                f, "topology `{machine}` not supported in this mode: {reason}"
            ),
            Self::InitFailed { machine, error } => write!(
                f, "machine `{machine}` init failed: {error:?}"
            ),
            Self::TickLimitExceeded { ticks } => write!(
                f, "driver loop exceeded {ticks} ticks"
            ),
            Self::CleanupFailed { machine } => write!(
                f, "machine `{machine}` cleanup failed"
            ),
            Self::IoFailed { error } => write!(f, "io reactor error: {error}"),
            Self::CompositeTooDeep { depth, hint } => write!(
                f, "composite expansion exceeded depth {depth}: {hint}"
            ),
        }
    }
}

impl std::error::Error for RuntimeError {}
