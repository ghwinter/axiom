//! # axiom-instances — 实例层
//!
//! axiom 的**实例层**：经 socket（`Executor` / [`Carrier`](axiom_runtime::movers::carrier::Carrier)
//! / `Telemetry`，见 runtime 模块 async-seam / carrier / telemetry）接入可替换的
//! 物理/生态实现。官方标准集 = 融合单 crate + feature 门控，**默认全关**（空实例面
//! 合法）；第三方实例经自建独立 crate 走开放路径（双形态边界，internal-design §3 / §5）。
//!
//! > **socket 的 feature 依赖（L1 审计修订）**：`Executor` 需 `async`
//! > （`axiom-runtime/async-seam`）、`Telemetry` 需 runtime `telemetry`；默认 feature
//! > 下仅 `Carrier`（核心族，无条件）可解析。故上述 socket 以**纯文本**引用、不设
//! > intra-doc 链接——防破链（docs.rs 以默认 features 构建）。
//!
//! ## Feature 门控
//!
//! | feature | 拉起 | 提供 |
//! |---|---|---|
//! | `async` | `axiom-runtime/async-seam` | 异步接缝（`Executor` 契约的前提） |
//! | `tokio` | `async` + 可选依赖 `tokio` | [`backend`] 的 tokio 引擎（真异步驱动 + 占位执行器） |
//! | `embedded` | `axiom-runtime/std` | 预留嵌入式流 |
//!
//! ## 布局（目录 = 语义分层）
//!
//! - [`backend`]（`tokio` feature 门控）：异步后端——`async_driver`（**真异步驱动**：
//!   把轮询等待点经语言原生 `.await` 挂进 tokio reactor，`tokio_poll_until`/
//!   `tokio_roll_until`/`tokio_poll_fed`，不扩 `Executor` 契约；同步 `park` 桥已实测
//!   判死，真接入在此域落地）+ `tokio_exec`（同步 `Executor` 契约的占位实现）。
//!
//! **no_std**：本 crate **不参与** no_std 承诺——实例层依赖 `std`（tokio/embedded
//! 实例均需）。默认 feature 下无 std 使用路径（空实例面），保持最小。
//!
//! 依赖方向单向：`axiom ← axiom-runtime ← axiom-instances`；实例层不得被 core/runtime
//! 反向依赖（workspace 成员表 + 依赖图强制）。
//!
//! `#![forbid(unsafe_code)]`：实例层无 unsafe。

#![forbid(unsafe_code)]
// no_std 说明（H1 审计修订）：本 crate 有意**不参与** no_std 承诺——实例层依赖
// std（tokio/embedded 实例均需 std）。不声明独立 std feature：无此门控需要；
// 默认 feature 下为空实例面（无 std 使用路径），保持最小。

/// 异步后端（`tokio` feature 门控）：真异步驱动 + 同步占位执行器。
pub mod backend {
    /// 真异步驱动：把轮询等待点经语言原生 `.await` 挂进 tokio reactor
    /// （`tokio_poll_until`/`tokio_roll_until`/`tokio_poll_fed`），不扩 `Executor` 契约。
    /// 门控：`tokio` feature。
    #[cfg(feature = "tokio")]
    pub mod async_driver;

    /// tokio 桥接执行器：把 axiom 异步接缝的等待点接进 tokio 的 time 语义
    /// （同步 `Executor` 契约的诚实占位实现；真接入在 `async_driver` 的异步路径）。
    /// 门控：`tokio` feature。
    #[cfg(feature = "tokio")]
    pub mod tokio_exec;

    /// tokio 事件驱动异步块环：实现 runtime `AsyncBlockRing` 契约（send 等非满 /
    /// recv 等新块，双 Notify 唤醒挂 tokio reactor）。块级流水线的交接原语。
    /// 门控：`tokio` feature。
    #[cfg(feature = "tokio")]
    pub mod async_ring;
}

// 空实例面（无 feature）合法：本 crate 此时无实例导出，编译为骨架。