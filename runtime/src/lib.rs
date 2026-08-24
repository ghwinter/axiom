//! # axiom-runtime
//!
//! **axiom 的物理层实现用例（载体 Carrier）**：为四构件编译期核心（[axiom::cell_core]）
//! 提供"值如何跨连接流动"的多种可替换物理方案。
//!
//! 定位：axiom 核心（cell_core）只声明**因果数据流**（`A.out -> B.in`）；不提供任何物理
//! 实现。**runtime 回答唯一一个问题**——这条流的值怎么从 `A.out` 到 `B.in`，以何种
//! 时空成本。每个答案（载体）是独立的可替换单元；换载体不改拓扑（多物理实现，T6）。
//!
//! ## 载体（Carrier）——"值如何流动"的可替换物理方案
//!
//! | 载体 | 物理方案 | 时空成本 | 单线程/跨线程 |
//! |---|---|---|---|
//! | [`InlineCarrier`](crate::carrier::InlineCarrier) | 栈上函数直接传（`B::step(A::step(x))`） | 零分配、内联 | 单线程 |
//! | [`QueueCarrier`](crate::carrier::QueueCarrier)（std） | 堆队列中转（`Box<dyn Any>` 每消息分配） | 每消息分配 | 单线程内 |
//! | [`BoundedCarrier`](crate::carrier::BoundedCarrier)（std） | 有界通道中转（`CAP >= 1` 编译期门） | 每消息分配 | 单线程内 |
//! | [`spawned_flow`](crate::carrier::spawned_flow)（std） | mpsc 通道 + 独立线程，`B::State` 在专用线程（panic 经回执传播） | 每消息分配 + 同步 | **跨线程** |
//!
//! 蓝图声明"这条流用哪个载体"（如 `Static<Chain<A,B>>` 走 `InlineCarrier`/`static_path`），
//! runtime 按声明兑现——"部署期物理"。不同时空成本 = 不同载体 = 同一逻辑的不同物理实现。
//!
//! ## 驱动（flow）与静态路径（static_path）
//!
//! - [`flow`](crate::flow)：`drive_link` —— 编译期布线验证（`Conforms<Wire>`）后，
//!   用选定载体驱动一条 A→B 因果流；验证在编译期，运行期零开销。
//! - [`static_path`](crate::static_path)：`run_static`/`run_declared_static` —— 把被
//!   `Static<SUB>` 声明为"要求零成本"的子图在编译期内联展开（零运行时对象）。
//!
//! ## 模块化与可替换（第三方适配器模板）
//!
//! 每个载体独立、可单独引用。第三方物理适配器（未来 `axiom_tokio`、`axiom_io_uring`）
//! 通过实现 [`Carrier`](crate::carrier::Carrier) trait 挂入，**不改 cell 拓扑**：
//! 例如 `axiom_tokio` 可用 async 通道载体替换队列/通道形态的载体，`axiom_io_uring` 用
//! io_uring 载体替换。runtime 作为参考实现用例，提供各载体作模板。
//!
//! `#![forbid(unsafe_code)]`：runtime 核心无 unsafe。

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

/// 载体：cell_core 因果数据流的物理实现（值如何流动）。Stability: **stable**。
pub mod carrier;

/// 部署期契约校验：Moore 标记 / 成本符合性 / 背压就绪（声明 → 可校验契约）。Stability: **experimental**。
pub mod contract;

/// 义务类类型系统与义务账本（宪法 A3–A6 的机械；meta-foundations 定义 1.6）。Stability: **experimental**。
pub mod obligation;

/// 投递四态税则：Full/Closed 机械化、Timeout/Cancelled 声明（模态④，无伪见证）。Stability: **experimental**。
#[cfg(feature = "std")]
pub mod delivery;

/// 编译期/运行时驱动：将蓝图（cell 拓扑）+ 载体选型兑现为执行。Stability: **stable**。
pub mod flow;

/// 型位的运行期存在化（∃ 绑定，物理侧）。Stability: **experimental**。
#[cfg(feature = "std")]
pub mod slot;

/// 有界缓冲 / 背压原语（§9.1，std）。Stability: **experimental**。
#[cfg(feature = "std")]
pub mod buffer;

/// 静态路径：声明为静态的子图在编译期内联展开（零运行时对象）。Stability: **stable**。
pub mod static_path;

/// `wire!` 声明宏：编译期展开的"连线 + 载体 + 验证"一次完成（宏/编译期技巧）。Stability: **stable**。
#[macro_use]
pub mod macros;

/// 核心 prelude。
pub mod prelude_all {
    pub use crate::carrier::{Carrier, CarrierCost, InlineCarrier};
    #[cfg(feature = "std")]
    pub use crate::buffer::BoundedQueue;
    #[cfg(feature = "std")]
    pub use crate::carrier::{BoundedCarrier, QueueCarrier, spawned_flow};
    pub use crate::contract::{
        ContractError, Moore, assert_capacity_nonzero, declare_inline_loop_moore,
        validate_capacity, validate_cost, validate_seam,
    };
    #[cfg(feature = "std")]
    pub use crate::delivery::{Delivery, Receipt};
    pub use crate::obligation::{
        DeliveryKind, LedgerEntry, LifecycleKind, Modality, ObligationClass, ReferenceKind, LEDGER,
    };
    pub use crate::flow::{
        assemble_link, assemble_seam, drive_feedback_inline, drive_link, drive_try, Driver,
        TryChain,
    };
    #[cfg(feature = "std")]
    pub use crate::flow::{bounded_pump, bounded_pump_try};
    pub use crate::flow::drive_seq;
    #[cfg(feature = "std")]
    pub use crate::slot::{Seat, SlotDrive, SlotPending};
    pub use crate::static_path::{run_declared_static, run_static};
    pub use crate::wire;
}
