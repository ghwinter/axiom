//! sqlmini 阶段错误统一（多阶段失败管线的单 E 语汇）。
//!
//! 全链（词法→语法→语义→计划→执行）共享一个 [`SqlError`]：各阶段以变体标记，
//! `TryChain` 使整条管线呈单层 `Result`（netpath 同构）。错误带位置、带值，
//! 不静默、不回退。

/// 统一错误：阶段变体 + 定位 + 失败值。
/// 注：`Parse`/`Plan`/`Exec` 变体由阶段 2–4 使用（当前轮未构造成员，故
/// 标注 allow；随阶段落地自然消去）。
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlError {
    /// 词法：非法字符/未闭合字符串。(位置, 字符)。
    Lex(usize, String),
    /// 语法：预期不符。(位置, 期望, 实得)。
    Parse(usize, String, String),
    /// 语义：列不存在/类型不符/聚合误用。(对象, 说明)。
    Plan(String, String),
    /// 执行：运行时错误。(算子, 说明)。
    Exec(String, String),
}

impl SqlError {
    /// 阶段名（驱动/账本日志用；测试路径已用，示例二进制路径暂未调用）。
    #[allow(dead_code)]
    pub fn stage(&self) -> &'static str {
        match self {
            SqlError::Lex(..) => "lex",
            SqlError::Parse(..) => "parse",
            SqlError::Plan(..) => "plan",
            SqlError::Exec(..) => "exec",
        }
    }
}

impl core::fmt::Display for SqlError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SqlError::Lex(pos, ch) => write!(f, "lex error at {pos}: unexpected {ch:?}"),
            SqlError::Parse(pos, expect, got) => {
                write!(f, "parse error at {pos}: expected {expect}, got {got:?}")
            }
            SqlError::Plan(obj, why) => write!(f, "plan error on {obj}: {why}"),
            SqlError::Exec(op, why) => write!(f, "exec error in {op}: {why}"),
        }
    }
}

impl std::error::Error for SqlError {}