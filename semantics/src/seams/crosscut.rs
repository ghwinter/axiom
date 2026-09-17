//! 横切面契约（品种判据的代码落点）——唯一**没有表面**的品种。
//!
//! ## 概念归属（§8.3 封闭判据）
//!
//! 品种分类学：功能面 / 模块 / 契约 trait / 词汇枚举 / 策略 / 载体 / **横切面**。
//! 横切面是唯一故意不实现 [`axiom::cell_core::PortCell`] 的品种——它**没有
//! In/Out/State 表面**：不回答请求、不产出应答、不持有可查询状态，只"跟着调用
//! 流跑"。所以它不是 cell，不进 `assert_wiring`；这正是横切面品种的判据：
//! 问"它跟谁走"，答案是"跟所有模块走，但不属于任何模块"。
//!
//! ## 契约
//!
//! [`CrossCut`] 是**姿态声明**（marker）：把"横切面不是 cell"变成类型层可判定的
//! 事实。约束即品种判据：
//! - **无表面**：不实现 [`PortCell`]（即无 `In`/`Out`/`State` 关联类型）；
//! - **可传播**：`Clone + Send + Sync`——整体穿过模块边界与线程边界；
//! - **派生语义从它长出**：取消令牌等派生句柄（依赖链 1：取消 ← 上下文传播）。
//!
//! ## 参考实现
//!
//! axiom-exp 实验 11 的 ctx 模块（`CtxKey` 类型化键 + 不可变帧链 + 派生取消令牌）。
//! 生态参考：opentelemetry 的 Context（不可变 span + baggage 携带）、golang 的
//! `context.Context`、surrealdb 的 Context / Options / Session 三件套——各系统自研，
//! 无社区默认统一管理器 → 本接缝只立契约，不提供实现（实现是横切面的"载体"：
//! 代码总得有物理形态，但"住在哪"不等于"语义上属于哪"）。

/// 横切面契约：随调用流传播、**不属于任何模块**的载荷。
///
/// 有意没有 In/Out/State 表面（不是 [`axiom::cell_core::PortCell`]）——实现者
/// 无需任何方法；本 trait 是姿态声明，把"横切面不是 cell"变成类型层事实。
pub trait CrossCut: Clone + Send + Sync + 'static {}

/// 编译期判定：`C` 满足横切面契约（无表面 + 可跨线程传播）。
pub fn assert_crosscut<C: CrossCut>() {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参考形态：上下文载荷（ctx 实验的类型化键 + 值表的最小形态）。
    #[derive(Clone)]
    struct Ctx {
        txn: Option<u64>,
    }

    impl CrossCut for Ctx {}

    #[test]
    fn marker_declares_crosscut() {
        // 编译期判定：契约可满足（无表面 + Send + Sync + Clone）。
        assert_crosscut::<Ctx>();
        let a = Ctx { txn: Some(1) };
        let b = a.clone();
        assert_eq!(a.txn, b.txn);
    }
}
