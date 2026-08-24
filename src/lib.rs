//! # axiom
//!
//! **四构件编译期核心：开放系统 + 因果数据流 + 组合 + 静态性声明。**
//!
//! Zero-dependency computation primitives for observable, controllable systems.
//! axiom 是一个**编译期模型**：蓝图用 Rust 代码/类型定义（无 JSON/值形态中间层），
//! 核心能力到编译期耗尽用于分析、验证，编译后等价手写普通 Rust、零运行时对象。
//!
//! - **开放系统（端口体）** [`PortCell`](crate::cell_core::PortCell)：有边界的计算体，
//!   类型化输入/输出/状态，`step` 纯且内联。
//! - **因果数据流** [`Wire`](crate::cell_core::Wire)：`A.out -> B.in`，类型层对偶配对，
//!   非法连接编译失败（T1）。多对多 [`Broadcast`](crate::cell_core::Broadcast)
//!   （fan-out）、[`Merge`](crate::cell_core::Merge)（fan-in）、
//!   环 [`Feedback`](crate::cell_core::Feedback) 亦是类型层表达。
//! - **组合** [`Chain`](crate::cell_core::Chain)：组合子仍是端口体，任意层级嵌套。
//! - **静态性** [`Static`](crate::cell_core::Static) / 编译期验证
//!   [`Conforms`](crate::cell_core::Conforms) / [`assert_wiring`](crate::cell_core::assert_wiring)。
//!
//! > **编译期核心承诺**：
//! > - 蓝图即类型：零大小、运行时无对象（`size_of::<Blueprint<T>>() == 0`）；
//! > - 验证在编译期（类型判定/宏），运行期零开销；
//! > - 编译后等价手写普通 Rust（见 `examples/cell_demo.rs`）。
//!
//! > **移出抽象层的旧语义（归物理载体）**：
//! > FlowKind（Data/Control/Observe）三分、时序/Delay、线程/同步异步、值形态/JSON。
//!
//! # 安全与 no_std
//!
//! - `axiom` 核心无任何 `unsafe`（`#![forbid(unsafe_code)]`——编译期承诺）。
//! - 支持 `no_std + alloc`（`--no-default-features`）：cell_core 只用 `core`/`alloc`。

#![cfg_attr(docsrs, feature(doc_cfg))]
// axiom core contains no `unsafe` — make that a compile-time promise.
#![forbid(unsafe_code)]

// axiom supports a `no_std + alloc` build.
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

// Let `::axiom::` resolve from inside this crate itself (e.g. at `wire!` expansion
// sites inside this crate), matching the macro's hardcoded extern path.
extern crate self as axiom;

// ═══════════════════════════════════════════════════════════════════════════
// 核心主轴线：cell_core（四构件编译期模型）。
//
// 旧核心（v0：machine/port/link/deploy/FlowKind/值形态等）已移出 src；
// 物理实现（载体/宏/编译期展开）由 runtime 承担（axiom-runtime crate）。
//
// Module maturity: `cell_core` is **stable** (closed five-concept boundary,
// foundations.md §8); additive-only evolution via the closure checklist.
// ═══════════════════════════════════════════════════════════════════════════

/// 核心主轴：四构件编译期模型（Stability: **stable**）。
pub mod cell_core;

/// 核心 prelude：四构件主轴线的默认导出面。
pub mod prelude_all {
    pub use crate::cell_core::{
        Blueprint, Broadcast, Chain, Choice, ChoiceIn, ChoiceOut, Conforms, Diamond, Feedback,
        Id, Merge, Opt, PortCell, Rep, Repeat, Slot, Static, Wire, assert_conforms,
        assert_wiring, blueprint_is_zero_sized, drive, wired,
    };
}
