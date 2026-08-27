//! # axiom-demo-sql-over-redis — 综合用例（跨层演示）
//!
//! 首个综合用例 = **SQL-over-Redis**：redis 协议面 × psql 计算面在**单一组合核心**
//! （组合 `PortCell`）内协同；同一计划由 sync 与 async 两组物理驱动承载，行级等价
//! 对拍（T6）。分层：
//!
//! - [`plans`]：计划（`sql_plan` 计算面 + `redis_plan` 协议面）
//! - [`composite`]：组合核心（RouteParse 分派 + ComposeLine 单一组合 `PortCell` + 语料）
//! - [`observe`]：观测模块（用例侧三段式：收集 → 提交 → 打印；与其它模块平级）
//!
//! 目录 = 语义分层：`plans/`（计划）· `composite.rs`（组合核心）· `observe.rs`（观测模块）。
//!
//! 双参照标注：绝对尺度 = 演示级组合（数百行，非工程级）；相对尺度 = 仓库最大组合示例。
//!
//! 依赖方向单向：axiom ← axiom-runtime ← axiom-instances ← 综合用例（workspace 成员表强制）。

#![forbid(unsafe_code)]

/// 计划（计算面 + 协议面）。
pub mod plans {
    /// SQL 计算面（Lexer / Parser / Executor / Database / SqlPipe）。
    pub mod sql_plan;

    /// KV 协议面（LineSplit / CmdParse / DataStore / 编解码）。
    pub mod redis_plan;
}

/// 组合核心（RouteParse 分派 + ComposeLine 单一组合 `PortCell` + 语料）。
pub mod composite;

/// 观测模块（用例侧三段式：收集 → 提交 → 打印；与其它模块平级）。
pub mod observe;