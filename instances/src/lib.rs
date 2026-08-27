//! # axiom-instances — 实例层
//!
//! axiom 的**实例层**：经 socket（`Executor` / [`Carrier`](axiom_runtime::carrier::Carrier)
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
//! | `tokio` | `async` + 可选依赖 `tokio` | `tokio_exec::TokioExec`——等待点适配 tokio |
//! | `embedded` | `axiom-runtime/std` | 预留嵌入式流 |
//!
//! ## 布局
//!
//! - `async_driver`：**真异步驱动**（`tokio` feature 门控）——把轮询等待点经语言原生
//!   `.await` 挂进 tokio reactor（`tokio_poll_until`/`tokio_roll_until`），不扩
//!   `Executor` 契约。这是 `Executor` 同步插座（`tokio_exec`）之外的**语言原生异步路径**；
//!   同步 `park` 桥已实测判死（no reactor），真接入在此域落地。
//! - `tokio_exec`：tokio 桥接 **同步**执行器（`Executor` 契约实现，占位语义；`tokio`
//!   feature 门控；默认 feature 下无该模块）。
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

/// tokio 桥接执行器：把 axiom 异步接缝的等待点接进 tokio 的 time 语义。
/// 门控：`tokio` feature。
#[cfg(feature = "tokio")]
pub mod tokio_exec;

/// 真异步驱动：把轮询等待点经语言原生 `.await` 挂进 tokio reactor
/// （`tokio_poll_until`/`tokio_roll_until`），不扩 `Executor` 契约。
/// 门控：`tokio` feature。
#[cfg(feature = "tokio")]
pub mod async_driver;

// 空实例面（无 feature）合法：本 crate 此时无实例导出，编译为骨架。