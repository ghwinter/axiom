//! # axiom-demo-sql-over-redis — 综合用例（跨层演示）
//!
//! 首个综合用例 = SQL-over-Redis：redis 协议面 × psql 计算面在单一组合核心
//! （组合 `PortCell`）内协同；同一计划由 sync 与 async 两组物理驱动承载，行级等价
//! 交叉验证（T6）。分层：
//!
//! - [`plans`]：计划（`sql_plan` 计算面 + `redis_plan` 协议面）
//! - [`composite`]：组合核心（RouteParse 分派 + ComposeLine 单一组合 `PortCell` + 语料）
//! - [`observe`]：观测模块（用例侧三段式：收集 → 提交 → 打印；与其它模块平级）
//! - [`callresp`]：激活模型目录件（call/response 关联调度，时间作值）
//! - [`gateway`]：批处理网关（综合实例·堆叠演示——事件接缝 × callresp 接线 × 观测）
//! - [`tcp_server`]：TCP 服务器（事件接缝实际网络域：sync/async/仿真三物理域服务）
//!
//! 目录 = 语义分层：`plans/`（计划）· `composite.rs`（组合核心）· `observe.rs`（观测模块）。
//!
//! 双参照标注：绝对尺度 = 演示级组合（数百行，非工程级）；相对尺度 = 仓库最大组合示例。
//!
//! 依赖方向单向：axiom ← axiom-semantics ← axiom-instances ← 综合用例（workspace 成员表强制）。

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

/// 激活模型目录·第一期（call/response 关联调度，时间作值）。
/// 用例侧首个目录件：把"在途关联 + 超时清扫"打包成纯、可确定性测试的组件。
pub mod callresp;

/// 批处理网关（综合实例·堆叠演示）：事件接缝 × call/response 关联 × 观测。
/// 把三个接缝堆叠在同一用例上（"往上堆叠不语义爆炸"的实证）；callresp 二期接线。
pub mod gateway;

/// TCP 服务器（网络异步 I/O）：SQL-over-Redis 组合经真实套接字在
/// sync/async 两物理域服务（每连接事件泵、有界背压、按序回写）；系统级 T6 交叉验证
/// 见 `tests/tcp_t6_crosscheck.rs`，可运行演示见 `bin/tcp_demo.rs`。
pub mod tcp_server;

/// 网络物理切换（确定性仿真载体 ↔ 实际 tokio）：`tokio::net` ↔ `turmoil::net`
/// 编译期切换，使同一 async 服务器蓝图可落实际域/仿真域；仅 async 相关 feature 下编译。
#[cfg(any(feature = "tokio", feature = "turmoil"))]
pub mod net;