//! # axiom-demo-sql-over-redis — 综合用例（跨层演示）
//!
//! 首个综合用例 = **SQL-over-Redis**：redis 协议面 × psql 计算面在**单一组合核心**
//! （组合 `PortCell`）内协同；同一计划由 sync 与 async 两组物理驱动承载，行级等价
//! 对拍（T6）。分层：
//!
//! - [`redis_plan`]：KV 协议面（LineSplit / CmdParse / DataStore / 编解码）
//! - [`sql_plan`]：SQL 计算面（Lexer / Parser / Executor / Database）
//! - [`composite`]：组合核心（RouteParse 分派 + ComposeLine 单一组合 `PortCell` + 语料）
//! - [`observe`]：观测子系统（用例侧三段式：收集 → 提交 → 打印；观测面非通用）
//!
//! 双参照标注：绝对尺度 = 演示级组合（数百行，非工程级）；相对尺度 = 仓库最大组合示例。
//!
//! 依赖方向单向：axiom ← axiom-runtime ← axiom-instances ← demos（workspace 成员表强制）。

#![forbid(unsafe_code)]

pub mod composite;
pub mod observe;
pub mod redis_plan;
pub mod sql_plan;