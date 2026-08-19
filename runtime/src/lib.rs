//! # axiom-runtime
//!
//! **axiom 的物理层实现用例（载体 Carrier）**：为四构件编译期核心（[axiom::cell_core]）
//! 提供"值如何跨连接流动"的多种可替换物理方案。
//!
//! 定位：axiom 核心（cell_core）只声明**因果数据流**（`A.out -> B.in`）；不提供任何物理
//! 实现。**runtime 回答唯一一个问题**——这条流的值怎么从 `A.out` 到 `B.in`，以何种
//! 时空成本。每个答案（载体）是独立的可替换单元；换载体不改拓扑（多物理实现，T6）。
//!
//! ## 载体（Carrier）
//!
//! | 载体 | 物理方案 | 时空成本 | 模块 |
//! |---|---|---|---|
//! | [`InlineCarrier`](crate::carrier::InlineCarrier) | 栈上函数直接传（`B::step(A::step(x))`） | 零分配、内联、单线程 | carrier/inline.rs |
//! | [`QueueCarrier`](crate::carrier::QueueCarrier) | 堆队列/通道，跨线程传输 | 每消息分配 + 同步 | carrier/queue.rs |
//! | [`DirectCarrier`](crate::carrier::DirectCarrier) | 编译期展开（静态链内联为调用图） | 零运行时对象 | carrier/direct.rs |
//!
//! 蓝图声明"这条流用哪个载体"（如 `Static<Chain<A,B>>` 走 `InlineCarrier`），
//! 运行时按声明兑现——"部署期物理"。
//!
//! ## 模块化与可替换
//!
//! 每个载体独立、可单独引用。第三方物理适配器（未来 `axiom_tokio`、`axiom_io_uring`）通过
//! 实现 [`Carrier`](crate::carrier::Carrier) trait 挂入，不改 cell 拓扑。
//!
//! `#![forbid(unsafe_code)]`：runtime 核心无 unsafe。

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

/// 载体：cell_core 因果数据流的物理实现（值如何流动）。
pub mod carrier;

/// 编译期/运行时驱动：将蓝图（cell 拓扑）+ 载体选型兑现为执行。
pub mod flow;

/// 静态路径：声明为静态的子图在编译期内联展开（零运行时对象）。
pub mod static_path;

/// 核心 prelude。
pub mod prelude_all {
    pub use crate::carrier::{
        Carrier, CarrierCost, DirectCarrier, InlineCarrier,
    };
    #[cfg(feature = "std")]
    pub use crate::carrier::QueueCarrier;
    pub use crate::flow::{drive_link, drive_wired};
    pub use crate::static_path::{run_declared_static, run_static};
}
