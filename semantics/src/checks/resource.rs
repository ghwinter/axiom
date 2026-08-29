//! 资源幺半群（D11；boundary-ontology 注记 7.4 "唯一真新增、未代码化" 的落点）。
//!
//! 资源作为**一等代数对象**：交换幺半群 (M, ·, ε)。这是义务类资源**轴**（D1；
//! [`CarrierCost`] 的类/量级序）之外**新增的可组合所有权维度**——前四行为结构
//! （因果序/成本/容量/投递）描述单条接缝的运行幅度，资源描述**跨拓扑的可组合
//! 所有权关系**（frame 律）。
//!
//! **frame 律**：不相交所有权下的 `P · R` 可**分别验证**、组合 = 各分量之和
//! （"组合=真和"）。其前置"两份预算确实不相交"由 **L2 单属主** 在类型层背书——
//! `&mut` 独占保证同一时刻至多一个执行流触碰一块 `State`，这正是 frame 推理合法
//! 化的地基。本模块**不伪称能证明 `&mut` 不相交**（那是借用检查器的职责），只背书
//! 一阶幺半群律与"组合 = 真和"。
//!
//! 诚实边界（A5）：本模块是知识单元（律 + 探针），**非分配引擎**——语义层不分配；
//! 具体聚合/归还（bring-up 时刻分配、运行期扩容）归实例层。此处确立的是资源**量的
//! 组合律**，不是资源**物理来源**。

use core::cell::Cell;

use crate::movers::carrier::CarrierCost;

/// 可组合所有权的一等对象：交换幺半群 (M, ·, ε)。
///
/// 幺元 [`Resource::empty`] 与顺序无关的可并 [`Resource::merge`]。frame 律依赖
/// "两份不相交预算的合并 = 各自之和"的**真和**——实现者须保证 `merge` 不为近似、
/// 不丢分量。
pub trait Resource: Sized {
    /// 空资源（幺元 ε）。
    fn empty() -> Self;
    /// 顺序无关的可并合并 ·。
    fn merge(self, other: Self) -> Self;
}

/// 资源的一种具体实例：资源**类**（承接 [`CarrierCost`] 量级序）× 可加**单位**。
///
/// 类 = 资源的量级（分划）；单位 = 类内的可加计数。`merge` 取 `units` 之和（frame
/// 律真和）与两类的保守并（`cost.max`——合并不偷工，整体类取两者更高者）。`max`
/// 为**设计决断**（保守并类，非命题结论）：合并后的资源类不得低于任一分量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceAmount {
    /// 资源类（量级序：`ZeroAllocInline < PerMessageAlloc < External`）。
    pub cost: CarrierCost,
    /// 该类下的可加单位数。
    pub units: u64,
}

impl ResourceAmount {
    /// 指定类下的空资源（幺元）。
    pub const fn empty_in(cost: CarrierCost) -> Self {
        ResourceAmount { cost, units: 0 }
    }
}

impl Resource for ResourceAmount {
    fn empty() -> Self {
        ResourceAmount {
            cost: CarrierCost::ZeroAllocInline,
            units: 0,
        }
    }
    fn merge(self, other: Self) -> Self {
        ResourceAmount {
            cost: self.cost.max(other.cost),
            units: self.units + other.units,
        }
    }
}

/// frame 律探针：两份不相交预算独立记录，合并后总量 = 各自之和。
///
/// `debug_assertions` 门控（同 law.rs）；release 零开销。诚实声明：此探针断言
/// **记账侧**的真和（[`FrameProbe::on_combine`] 后 `combined = left + right`）；
/// "两份预算确实不相交"这一前置由 L2 单属主（`&mut`）在类型层给出，不由本探针
/// 证明。
pub struct FrameProbe {
    left: Cell<u64>,
    right: Cell<u64>,
    combined: Cell<u64>,
}

impl FrameProbe {
    /// 新建探针。
    pub const fn new() -> Self {
        FrameProbe {
            left: Cell::new(0),
            right: Cell::new(0),
            combined: Cell::new(0),
        }
    }

    /// 记录左预算的持有增量（只记量；类维度不参与 frame 求和）。
    pub fn on_left(&self, units: u64) {
        self.left.set(self.left.get().wrapping_add(units));
    }

    /// 记录右预算的持有增量。
    pub fn on_right(&self, units: u64) {
        self.right.set(self.right.get().wrapping_add(units));
    }

    /// 记录"已合并"时刻交付的总量（应为左 + 右）。
    pub fn on_combine(&self, combined_total: u64) {
        self.combined.set(combined_total);
    }

    /// 校验 frame 律（debug）：组合总量 = 左 + 右。
    pub fn assert_frame(&self) {
        debug_assert_eq!(
            self.combined.get(),
            self.left.get() + self.right.get(),
            "frame 律违反：组合量 != 左 + 右（合并非真和）"
        );
    }
}

impl Default for FrameProbe {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::movers::carrier::CarrierCost as C;

    fn amt(cost: C, units: u64) -> ResourceAmount {
        ResourceAmount { cost, units }
    }

    #[test]
    fn monoid_identity() {
        let x = amt(C::PerMessageAlloc, 3);
        assert_eq!(ResourceAmount::empty().merge(x), x, "ε · x = x");
        assert_eq!(x.merge(ResourceAmount::empty()), x, "x · ε = x");
    }

    #[test]
    fn monoid_commutative() {
        let a = amt(C::ZeroAllocInline, 2);
        let b = amt(C::PerMessageAlloc, 5);
        assert_eq!(a.merge(b), b.merge(a), "· 顺序无关");
    }

    #[test]
    fn monoid_associative() {
        let a = amt(C::ZeroAllocInline, 1);
        let b = amt(C::PerMessageAlloc, 2);
        let c = amt(C::External, 3);
        assert_eq!(a.merge(b).merge(c), a.merge(b.merge(c)), "· 结合");
    }

    #[test]
    fn merge_is_conservative_and_true_sum() {
        // frame 真和：units 相加；类取 max（保守并类，不丢高量级）。
        let a = amt(C::PerMessageAlloc, 4);
        let b = amt(C::External, 6);
        let whole = a.merge(b);
        assert_eq!(whole.units, 10, "units 真和");
        assert_eq!(whole.cost, C::External, "类取保守 max");
    }

    #[test]
    fn frame_probe_sums_disjoint_budgets() {
        let f = FrameProbe::new();
        let a = amt(C::PerMessageAlloc, 4);
        let b = amt(C::ZeroAllocInline, 6);
        f.on_left(a.units);
        f.on_right(b.units);
        f.on_combine(a.merge(b).units);
        f.assert_frame(); // combined(10) == left(4) + right(6)
    }
}