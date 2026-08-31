//! # axiom-instances — 实例层
//!
//! axiom 的**实例层**：经 socket（`Executor` / [`Carrier`](axiom_semantics::movers::carrier::Carrier)
//! / `Telemetry`，见语义层模块 async-seam / carrier / telemetry）接入可替换的
//! 物理/生态实现。官方标准集 = 融合单 crate + feature 门控，**默认全关**（空实例面
//! 合法）；第三方实例经自建独立 crate 走开放路径（双形态边界，internal-design §3 / §5）。
//!
//! > **socket 的 feature 依赖（L1 审计修订）**：`Executor` 需 `async`
//! > （`axiom-semantics/async-seam`）、`Telemetry` 需语义层 `telemetry`；默认 feature
//! > 下仅 `Carrier`（核心族，无条件）可解析。故上述 socket 以**纯文本**引用、不设
//! > intra-doc 链接——防破链（docs.rs 以默认 features 构建）。
//!
//! ## Feature 门控
//!
//! | feature | 拉起 | 提供 |
//! |---|---|---|
//! | `async` | `axiom-semantics/async-seam` | 异步接缝（`Executor` 契约的前提） |
//! | `tokio` | `async` + 可选依赖 `tokio` | [`backend`] 的 tokio 引擎（真异步驱动 + 占位执行器） |
//! | `embedded` | `axiom-semantics/std` | `backend::embedded` 同步块环流水线（BoundedRing 背压，单线程基座；feature 门控模块，故以纯文本引用，见本条纪律） |
//!
//! ## 为什么存在 / Why an instance layer exists
//!
//! **"不替用户选物理" = 没有静默默认，不是"无所提供"**。提供实现 ≠ 强加：违背的情形是
//! 用户未显式引入时实现被默认启用。本层经三个**架构级**机制（结构性保证）通过此检验——
//! (1) feature **默认全关**，不经用户显式开启不会进入编译；(2) **单向依赖**（`axiom ← semantics ← instances`，workspace
//! 强制），core 在结构上不可能感知任何具体物理，只有采纳能发生；(3) 物理只经**命名的缝**进入
//! （`Carrier`/`Executor`/`Telemetry` 契约 + 如实声明的 `cost()`/`saturation()`/`obligation()`）。
//! 删除性判据：删掉本 crate，core 与语义层照常 `no_std` 编译、蓝图仍在 `InlineCarrier` 上运行——
//! 可删、可加、可平行替换三者同时成立，边界在架构上成立。
//!
//! **那为何必须有实例层**？为**存在性证明（witness）**：(1) T6（同抽象组合、多物理实现、语义等价）
//! 是定理宣称，若一律不实现载体，它便是空洞形式化——真异步驱动（`async_driver`）证明"抽象层可对
//! async 全然无知、经契约接入执行器"，只能由真写出来测过的实现证明，不能由文档宣称；(2) 接缝契约的
//! 缺陷（隐含假设、泄漏）只由实现者暴露，本层由项目自己任**第一实现者**验证插座设计（故文档称
//! "实现用例"——use-case，身份是**证据**非权威）；(3) 无实例层等于挖护城河——连适配器都要用户自写，
//! 不是自由是门槛。
//!
//! **身份：树内参考实现，真实但不特权**（同 Linux 树内驱动 / std 的 `HashMap`）。所谓"特权"——
//! 门剖面（Kernel/Service）的注册白名单——本身就是**部署侧**选的；开放剖面（Tool/Embedded）里未注册
//! 第三方照常工作，选择权仍在用户，axiom 只提供"按剖面声明准入"的机制。**生态引力风险**（"原则上
//! 可替换、实际上无人替换"，systemd 之于 init）的防御 = 两条既有纪律：载体目录呈现为**菜单**而非
//! 技术栈，且极小基律把新插座门槛压在"第二实现者"上。用户替换它的那一刻，恰好完成它自己存在的目的
//! ——T6 的第二次兑现。
//!
//! ## 布局（目录 = 语义分层）
//!
//! - [`backend`]（`tokio` feature 门控）：异步后端——`async_driver`（**真异步驱动**：
//!   把轮询等待点经语言原生 `.await` 挂进 tokio reactor，`tokio_poll_until`/
//!   `tokio_roll_until`/`tokio_poll_fed`，不扩 `Executor` 契约；同步 `park` 桥实测
//!   接入失败，真接入在此域落地）+ `tokio_exec`（同步 `Executor` 契约的占位实现）。
//!
//! **no_std**：本 crate **不参与** no_std 承诺——实例层依赖 `std`（tokio/embedded
//! 实例均需）。默认 feature 下无 std 使用路径（空实例面），保持最小。
//!
//! 依赖方向单向：`axiom ← axiom-semantics ← axiom-instances`；实例层不得被 core/语义层
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

    /// tokio 事件驱动异步块环：实现语义层 `AsyncBlockRing` 契约（send 等非满 /
    /// recv 等新块，双 Notify 唤醒挂 tokio reactor）。块级流水线的交接原语。
    /// 门控：`tokio` feature。
    #[cfg(feature = "tokio")]
    pub mod async_ring;

    /// 同步块环流水线（embedded 基座）：BoundedRing 背压的单线程块泵（`async_flow` 的
    /// 退化极限，稳态零分配；`EmbeddedProfile` 白名单存储原语）。门控：`embedded` feature。
    #[cfg(feature = "embedded")]
    pub mod embedded;
}

/// tracing 观测汇：语义层 `Telemetry` 契约的 tracing 实现（Telemetry 插座的
/// **第二实现者**——第一实现为语义层自带 Console/Buf；极小基律下的首个生态绑定，
/// 源于真实观测需求）。级别契约见模块文档（例外 warn / 热路径 trace）。
/// 门控：`telemetry-tracing` feature。
#[cfg(feature = "telemetry-tracing")]
pub mod telemetry_tracing;

// 空实例面（无 feature）合法：本 crate 此时无实例导出，编译为骨架。