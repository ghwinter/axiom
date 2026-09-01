//! # axiom-semantics（语义函子层）
//!
//! **axiom 的语义 / 契约层**：把核心（[axiom::cell_core]）声明形状之后——"这条值怎么从
//! `A.out` 到 `B.in`、以何种时空成本、边界条件（等待/外部输入/失败/观测）如何定义"——表达成
//! 抽象契约与接缝（socket）。
//!
//! 层界注记：core 头部清单是**形状层四构件**；激活（等待点/运行期绑定）属本层，全谱概念为五。
//!
//! 定位（对应 [core 形状范畴 → 行为范畴] 的语义函子 ⟦·⟧）：axiom 核心（cell_core）只声明
//! **因果数据流**（`A.out -> B.in`），不提供任何物理实现、不运行时序、不立法运行策略。
//! 本层回答唯一个问题——形状在被执行基座上跑起来意味着什么：值怎么流动、什么时空成本、
//! 边界如何定义。每个答案（载体）是独立的可替换单元；换载体不改拓扑（多物理实现，T6）。
//!
//! 本层不执行任何东西——等待点、事件源、真实绑定的兑现归 [axiom-instances]（实例层）。
//!
//! ## 源码按语义分层（目录 = 分层）
//!
//! | 目录 | 语义 | 模块 |
//! |---|---|---|
//! | [`checks`](crate::checks) | 接线检查 + 承诺账本（证据面） | contract / profile / obligation / law / delivery |
//! | [`movers`](crate::movers) | 值的搬运器（物理实现） | carrier / buffer / ring / mailbox |
//! | [`seams`](crate::seams) | 接缝（等待 / 事件 / 观测） | async_seam / event / telemetry |
//! | [`drive`](crate::drive) | 流通组合与驱动 | flow / slot / enum_slot / static_path / macros |
//!
//! ## 载体（Carrier）——"值如何流动"的可替换物理方案
//!
//! | 载体 | 物理方案 | 时空成本 | 单线程/跨线程 |
//! |---|---|---|---|
//! | [`InlineCarrier`](crate::movers::carrier::InlineCarrier) | 栈上函数直接传（`B::step(A::step(x))`） | 零分配、内联 | 单线程 |
//! | [`QueueCarrier`](crate::movers::carrier::QueueCarrier)（std） | 堆队列中转（`Box<dyn Any>` 每消息分配） | 每消息分配 | 单线程内 |
//! | [`BoundedCarrier`](crate::movers::carrier::BoundedCarrier)（std） | 有界通道中转（`CAP >= 1` 编译期门） | 每消息分配 | 单线程内 |
//! | [`spawned_flow`](crate::movers::carrier::spawned_flow)（std） | mpsc 通道 + 独立线程，`B::State` 在专用线程（panic 经回执传播） | 每消息分配 + 同步 | 跨线程 |
//!
//! 蓝图声明"这条流用哪个载体"（如 `Static<Chain<A,B>>` 走 `InlineCarrier`/`static_path`），
//! 语义层按声明确立契约，实例层兑现——"部署期物理"。不同时空成本 = 不同载体 = 同一逻辑的不同物理实现。
//!
//! ## 驱动（drive）与静态路径（static_path）
//!
//! - [`drive::flow`](crate::drive::flow)：`drive_link` —— 编译期布线验证（`Conforms<Wire>`）后，
//!   用选定载体驱动一条 A→B 因果流；验证在编译期，运行期零开销。
//! - [`drive::static_path`](crate::drive::static_path)：`run_static`/`run_declared_static` —— 把被
//!   `Static<SUB>` 声明为"要求零成本"的子图在编译期内联展开（零运行时对象）。
//!
//! ## 模块化与可替换（第三方适配器模板）
//!
//! 每个载体独立、可单独引用。第三方物理适配器（未来 `axiom_tokio`、`axiom_io_uring`）
//! 通过实现 [`Carrier`](crate::movers::carrier::Carrier) trait 挂入，不改 cell 拓扑：
//! 例如 `axiom_tokio` 可用 async 通道载体替换队列/通道形态的载体，`axiom_io_uring` 用
//! io_uring 载体替换。语义层提供各载体作契约参考、实例层作绑定用例。
//!
//! ## 宪法头（公理展出；meta-foundations A3/A5 与定义 1.4 的物理层落点）
//!
//! **逻辑-D（结构不变量，少而硬）**：
//! - L1 无静默丢失：每次投递必得一个区分性判定（`Delivered`/`Full(v)`/`Closed(v)`，
//!   被拒值随判定回传，[`checks::delivery`](crate::checks::delivery)）；
//! - L2 单属主：任一 `State` 在同一时刻至多被一个执行流轮询（`&mut` 独占，
//!   借用检查器背书；[`drive::slot`](crate::drive::slot) 的 `Seat` 亦以代戳拒绝陈旧借用）；
//! - L3 容量归位（capacity placement）：容量是随放置而变的属性，非形状属性，按对象类别落位——
//!   仅携带投递保证且位于抽象层纯承诺子集（无堆、无运行时可校验）的有界接缝，其 `CAP` 才要求编译期常量
//!   且 ≥ 1（模态②，[`assert_capacity_nonzero`](crate::checks::contract::assert_capacity_nonzero)）——这
//!   是此类接缝唯一能取得"不静默丢失/有界等待"见证的机制；一切属于"计算图上的未来"的对象（动态/插件/
//!   外部/跨机器/可重启/可扩容），其容量在聚合（bring-up）时刻才可知，归部署期校验（模态③）或由开发者
//!   与使用者声明（模态④），不施加编译期义务。扩容与重启视角等价：同属生命周期聚合操作，容量由聚合的
//!   调用者选择，不刻进蓝图（边界：不得把未来对象的容量写成 ②）。
//! - L4 政策归驱动：cell 无时间语义，拍次/调度只在 driver/carrier（§8.4）。
//!
//! **经验-D（界 + 监测）**：
//! - E1 性能界随 bench 重验：bench 噪声底背书，工具链升级须重跑；
//! - E2 成本按 [`CarrierCost`](crate::movers::carrier::CarrierCost) 序声明：
//!   `ZeroAllocInline < PerMessageAlloc < External`，未声明默认 `External`（fail-closed）。
//!
//! ## 剖面（六元组 C 构件）与律探针（T 构件）
//!
//! [`checks::profile`](crate::checks::profile)：剖面目录——F↦C(F) 的分域承诺（Kernel/Service/Tool），
//! 同一拓扑换剖面即换预算门（T6）。[`checks::law`](crate::checks::law)：运行期律探针（配对/单调/扇出，
//! `debug_assertions` 门控，release 零开销）。
//!
//! `#![forbid(unsafe_code)]`：runtime 核心无 unsafe。

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

// ═══════════════════════════════════════════════════════════
// 源码分层（目录 = 语义分层）：
//   checks —— 接线检查 + 承诺账本（证据面）
//   movers —— 值的搬运器（物理实现）
//   seams  —— 接缝（等待 / 事件 / 观测）
//   drive  —— 流通组合与驱动
// ═══════════════════════════════════════════════════════════

/// 接线检查 + 承诺账本（证据面）。
pub mod checks {
    /// 部署期契约校验：Moore 标记 / 成本符合性 / 背压就绪（声明 → 可校验契约）。Stability: experimental。
    pub mod contract;

    /// 义务类类型系统与义务账本（宪法 A3–A6 的机械；meta-foundations 定义 1.6）。Stability: experimental。
    pub mod obligation;

    /// 剖面目录（六元组 C 构件；F↦C(F) 分域承诺，模态① 令牌 + 模态③ 预算门）。Stability: experimental。
    pub mod profile;

    /// 运行期律探针（T 构件；配对/单调/扇出，debug_assertions 门控、release 零开销）。Stability: experimental。
    #[cfg(feature = "std")]
    pub mod law;

    /// 投递四态税则：Full/Closed 机械化、Timeout/Cancelled 声明（模态④，无伪见证）。Stability: experimental。
    #[cfg(feature = "std")]
    pub mod delivery;

    /// 资源幺半群（D11；可组合所有权的知识单元）：交换幺半群律 + frame 真和探针，
    /// 衔接 L2 单属主与 [`crate::movers::carrier::CarrierCost`]。Stability: experimental。
    pub mod resource;
}

/// 值的搬运器（物理实现）。
pub mod movers {
    /// 载体：cell_core 因果数据流的物理实现（值如何流动）。Stability: stable。
    pub mod carrier;

    /// 有界邮箱（反饥饿背压）：CAP + 每生产者保底席位，三投递模式 fire/try/block。Stability: experimental。
    #[cfg(feature = "std")]
    pub mod mailbox;

    /// 有界缓冲 / 背压原语（§9.1，std）。Stability: experimental。
    #[cfg(feature = "std")]
    pub mod buffer;

    /// 环形缓冲（no_std 单线程 FIFO）。Stability: experimental。
    pub mod ring;

    /// 异步事件驱动块环契约（上游等非满 / 下游等新块；tokio 实例在 instances）。
    /// Stability: experimental。门控：`std`。
    #[cfg(feature = "std")]
    pub mod async_ring;
}

/// 接缝（等待 / 事件 / 观测）。
pub mod seams {
    /// 异步接缝（D2 等待点约定）：可轮询单元（Poll/Poller/poll_until 期限探测
    /// ＋SeamPoller 背压等待点）。契约本体 = 三等待点契约（输入就绪/期限/背压）
    /// ＋激活契约；`Executor` 是同步域合并轮询实现（§0.6 解散裁定）。
    /// Stability: experimental。门控：`async-seam` 特性（接缝载体族）。
    #[cfg(feature = "async-seam")]
    pub mod async_seam;

    /// 事件基座载体类（§9.3 接缝）：事件流（EventStream/ChunkSource）+ 泵驱动
    /// （pump_events：变换＋有界投递＋配对计数）。Stability: experimental。
    /// 门控：`event` 特性（接缝载体族，结构收敛 2026-08）。
    #[cfg(feature = "event")]
    pub mod event;

    /// 观测面接口（B1）：每接缝遥测（投递/深度/延迟），默认 no-op 零成本。
    /// Stability: experimental。门控：`telemetry` 特性（接缝载体族）。
    #[cfg(feature = "telemetry")]
    pub mod telemetry;
}

/// S₁ 项层可执行化（审计 定义 12.2 的代码落点）：[`term::Term`] 值级项表示 +
/// [`term::Reify`] 类型级→值级提取桥。分析器械的输入面，不参与执行。
/// Stability: experimental。
pub mod term;

/// ≡_eg 商机器（审计 F2 的代码落点）：e-graph（hashcons + 并查集 + 同余闭包），
/// 组合律即重写规则，预算有界饱和（合流性未证明——假阴性方向保守）。
/// Stability: experimental。
pub mod egraph;

/// 流通组合与驱动。
pub mod drive {
    /// 编译期/运行时驱动：将蓝图（cell 拓扑）+ 载体选型兑现为执行。Stability: stable。
    pub mod flow;

    /// 型位的运行期存在化（∃ 绑定，物理侧）。Stability: experimental。
    #[cfg(feature = "std")]
    pub mod slot;

    /// 枚举式型位（A4）：编译期已知候选集的零擦除存在化（vs `Slot` 的 dyn 擦除）。
    /// Stability: experimental。
    pub mod enum_slot;

    /// 静态路径：声明为静态的子图在编译期内联展开（零运行时对象）。Stability: stable。
    pub mod static_path;

    /// `wire!` 声明宏：编译期展开的"连线 + 载体 + 验证"一次完成（宏/编译期技巧）。Stability: stable。
    #[macro_use]
    pub mod macros;

    /// 异步流水线驱动：把 `PortCell` 与其前后两级异步块环接通
    /// （`run_source` 生产：step → send 等非满；`run_sink` 消费：recv 等新块 → step）。
    /// 生产/消费任务由实例层（tokio spawn）组合；等待点语义归块环实现。
    /// Stability: experimental。门控：`std`。
    #[cfg(feature = "std")]
    pub mod async_flow;
}

/// 核心 prelude。
pub mod prelude_all {
    pub use crate::movers::carrier::{
        Carrier, CarrierCost, InlineCarrier, MaybeCarrier, ResultCarrier, Registered,
        SaturationPolicy, ShortCircuit, drive_try_carrier,
    };
    #[cfg(feature = "std")]
    pub use crate::movers::buffer::BoundedQueue;
    #[cfg(feature = "std")]
    pub use crate::movers::carrier::{BoundedCarrier, QueueCarrier, spawned_flow};
    pub use crate::checks::contract::{
        ContractError, Moore, NoPanic, assert_capacity_nonzero, declare_inline_loop_moore,
        validate_capacity, validate_cost, validate_saturation, validate_seam,
    };
    pub use crate::checks::resource::{FrameProbe, Resource, ResourceAmount};
    #[cfg(feature = "std")]
    pub use crate::checks::delivery::{Delivery, Receipt};
    #[cfg(feature = "std")]
    pub use crate::movers::mailbox::{BoundedMailbox, Producer};
    #[cfg(all(feature = "std", feature = "event"))]
    pub use crate::seams::event::{
        ChunkSource, EventPumpStats, EventStream, PushVerdict, pump_events, split_lines,
    };
    #[cfg(all(feature = "std", feature = "async-seam"))]
    pub use crate::seams::async_seam::{Executor, Poll, PollResult, Poller, SeamPoller, ThreadExec};
    #[cfg(feature = "telemetry")]
    pub use crate::seams::telemetry::{
        BufTelemetry, MeteredPush, NoOpTelemetry, Telemetry, VerdictView,
    };
    #[cfg(all(feature = "std", feature = "telemetry"))]
    pub use crate::seams::telemetry::ConsoleTelemetry;
    pub use crate::checks::obligation::{
        DeliveryKind, LedgerEntry, LifecycleKind, Modality, ObligationClass, ReferenceKind, LEDGER,
    };
    pub use crate::checks::profile::{
        GameProfile, KernelProfile, Profile, ServiceProfile, ToolProfile, assemble_profile,
        assemble_profile_gated,
    };
    #[cfg(feature = "std")]
    pub use crate::checks::law::{PairLaw, assert_fanout, assert_monotonic};
    pub use crate::drive::flow::{
        assemble_link, assemble_seam, drive_feedback_inline, drive_link, drive_try, Driver,
        TryChain,
    };
    #[cfg(feature = "std")]
    pub use crate::drive::flow::{bounded_pump, bounded_pump_try};
    pub use crate::drive::flow::drive_seq;
    #[cfg(feature = "std")]
    pub use crate::drive::slot::{Seat, SlotDrive, SlotPending};
    pub use crate::drive::enum_slot::EnumSlot;
    pub use crate::drive::static_path::{run_declared_static, run_static};
    pub use crate::egraph::EGraph;
    pub use crate::term::{Reify, Term};
    pub use crate::wire;
}