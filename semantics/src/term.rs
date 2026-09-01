//! **term — S₁ 项层的可执行化（Term + Reify）**
//!
//! 地位（本体论审计 §12 定义 12.2 的代码落点）：axiom 的项层 S₁ 是全部
//! 良形蓝图的自由项代数。此前它是**纯类型级**的——蓝图即类型
//! （[`Blueprint`](axiom::cell_core::Blueprint) 零大小），组合结构只存在于
//! 编译器眼里。本模块把它**重化**为可遍历、可比较、可重写的值：
//!
//! - [`Term`]：项代数的值级表示（每个 [`PortCell`](axiom::cell_core::PortCell)
//!   组合子对应一个构造子）；
//! - [`Reify`]：类型级 → 值级的提取桥——组合子的 blanket impl 自动展开子项，
//!   叶子格由使用者声明名字。
//!
//! 与"值形态蓝图/JSON"的**区别**（防历史倒退）：Term 不是运行时蓝图对象，
//! 不参与执行——它是**分析器械**（模态③）的输入表示，服务于
//! [`crate::egraph`] 的商判定。执行面仍然只有类型（零成本承诺不动）。
//!
//! **诚实边界**：Term 不携带端口类型（无类型标签）——叶子名字是唯一身份。
//! 无类型化重写规则只对**良形项**可靠（良形性由 Rust 类型系统在 reify 前
//! 已保证）；饱和过程生成的新项不保证良形，故当前规则集只收录不依赖
//! 类型上下文的律（见 egraph 模块头）。带类型标签的 Term 是登记的开放项。

use axiom::cell_core::{
    Broadcast, Chain, Choice, Diamond, Feedback, Id, Merge, Opt, Rep,
};
use axiom::monoidal::{Discard, Duplicate, Par, Swap};
use alloc::boxed::Box;

// ── 1. 项（Term）──────────────────────────────────────────────────

/// 项代数的值级表示（审计 定义 12.2：T(Σ) 的载体）。
///
/// 构造子与 [`axiom::cell_core`] 的组合子一一对应；叶子 [`Term::Cell`]
/// 以名字标识一个具体的原子格。递归经 `Box`（值级表示允许堆分配——
/// 这是分析侧的表示成本，不属于执行面的零成本承诺范围）。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Term {
    /// 原子格（叶子），以名字标识。
    Cell(&'static str),
    /// 恒等单元（[`Id`](axiom::cell_core::Id)）。
    Id,
    /// 串行组合（[`Chain`](axiom::cell_core::Chain)）。
    Chain(Box<Term>, Box<Term>),
    /// 张量积（[`Par`](axiom::monoidal::Par)）。
    Par(Box<Term>, Box<Term>),
    /// 对称（[`Swap`](axiom::monoidal::Swap)）。
    Swap,
    /// 余单位（[`Discard`](axiom::monoidal::Discard)）。
    Discard,
    /// 余乘（[`Duplicate`](axiom::monoidal::Duplicate)）。
    Duplicate,
    /// 扇出（[`Broadcast`](axiom::cell_core::Broadcast)）。
    Broadcast(Box<Term>, Box<Term>, Box<Term>),
    /// 扇入（[`Merge`](axiom::cell_core::Merge)）。
    Merge(Box<Term>, Box<Term>, Box<Term>),
    /// 菱形（[`Diamond`](axiom::cell_core::Diamond)）。
    Diamond(Box<Term>, Box<Term>, Box<Term>, Box<Term>),
    /// 反馈环 / 迹（[`Feedback`](axiom::cell_core::Feedback)）。
    Feedback(Box<Term>, Box<Term>),
    /// 和（[`Choice`](axiom::cell_core::Choice)）。
    Choice(Box<Term>, Box<Term>),
    /// 可选（[`Opt`](axiom::cell_core::Opt)）。
    Opt(Box<Term>),
    /// 有界自组合（[`Rep`](axiom::cell_core::Rep)，N 次幂）。
    Rep(usize, Box<Term>),
}

impl Term {
    /// 叶子格的简写构造。
    pub fn cell(name: &'static str) -> Term {
        Term::Cell(name)
    }

    /// 串行组合的简写构造。
    pub fn chain(a: Term, b: Term) -> Term {
        Term::Chain(Box::new(a), Box::new(b))
    }

    /// 张量积的简写构造。
    pub fn par(a: Term, b: Term) -> Term {
        Term::Par(Box::new(a), Box::new(b))
    }
}

// ── 2. 重化桥（Reify）─────────────────────────────────────────────

/// 类型级 → 值级的提取：一个实现了 [`Reify`] 的类型能在运行期交出自己的项表示。
///
/// 组合子侧是 blanket impl（子项递归展开）；叶子格由使用者实现——
/// 惯用法是 `fn term() -> Term { Term::Cell("名字") }`。
pub trait Reify {
    /// 本类型的项表示（每次调用重新构造；分析侧一次性使用）。
    fn term() -> Term;
}

// 组合子的 blanket impl：类型级嵌套 → 值级递归。

impl<A: Reify, B: Reify> Reify for Chain<A, B> {
    fn term() -> Term {
        Term::chain(A::term(), B::term())
    }
}

impl<A: Reify, B: Reify> Reify for Par<A, B> {
    fn term() -> Term {
        Term::par(A::term(), B::term())
    }
}

impl<I> Reify for Id<I> {
    fn term() -> Term {
        Term::Id
    }
}

impl<I1, I2> Reify for Swap<I1, I2> {
    fn term() -> Term {
        Term::Swap
    }
}

impl<I> Reify for Discard<I> {
    fn term() -> Term {
        Term::Discard
    }
}

impl<I> Reify for Duplicate<I> {
    fn term() -> Term {
        Term::Duplicate
    }
}

impl<SRC: Reify, R1: Reify, R2: Reify> Reify for Broadcast<SRC, R1, R2>
where
    SRC: axiom::cell_core::PortCell,
    SRC::Out: Clone,
{
    fn term() -> Term {
        Term::Broadcast(Box::new(SRC::term()), Box::new(R1::term()), Box::new(R2::term()))
    }
}

impl<S1: Reify, S2: Reify, DST: Reify> Reify for Merge<S1, S2, DST>
where
    S1: axiom::cell_core::PortCell,
    S2: axiom::cell_core::PortCell,
    DST: axiom::cell_core::PortCell<In = S1::Out>,
    S2::Out: Into<DST::In>,
{
    fn term() -> Term {
        Term::Merge(Box::new(S1::term()), Box::new(S2::term()), Box::new(DST::term()))
    }
}

impl<SRC: Reify, R1: Reify, R2: Reify, DST: Reify> Reify for Diamond<SRC, R1, R2, DST>
where
    SRC: axiom::cell_core::PortCell,
    SRC::Out: Clone,
    R1: axiom::cell_core::PortCell<In = SRC::Out>,
    R2: axiom::cell_core::PortCell<In = SRC::Out>,
    DST: axiom::cell_core::PortCell<In = (R1::Out, R2::Out)>,
{
    fn term() -> Term {
        Term::Diamond(
            Box::new(SRC::term()),
            Box::new(R1::term()),
            Box::new(R2::term()),
            Box::new(DST::term()),
        )
    }
}

impl<BODY: Reify, FEED: Reify> Reify for Feedback<BODY, FEED>
where
    BODY: axiom::cell_core::PortCell,
    FEED: axiom::cell_core::PortCell<In = BODY::Out, Out = BODY::In>,
{
    fn term() -> Term {
        Term::Feedback(Box::new(BODY::term()), Box::new(FEED::term()))
    }
}

impl<A: Reify, B: Reify> Reify for Choice<A, B> {
    fn term() -> Term {
        Term::Choice(Box::new(A::term()), Box::new(B::term()))
    }
}

impl<C: Reify> Reify for Opt<C> {
    fn term() -> Term {
        Term::Opt(Box::new(C::term()))
    }
}

impl<const N: usize, C: Reify> Reify for Rep<N, C> {
    fn term() -> Term {
        Term::Rep(N, Box::new(C::term()))
    }
}

// ── 测试 ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::cell_core::PortCell;

    // 叶子格：既是 PortCell（真实可驱动），又声明项名字。
    struct Inc;
    impl PortCell for Inc {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x.wrapping_add(1)
        }
    }
    impl Reify for Inc {
        fn term() -> Term {
            Term::Cell("Inc")
        }
    }

    struct Scaler;
    impl PortCell for Scaler {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x.wrapping_mul(2)
        }
    }
    impl Reify for Scaler {
        fn term() -> Term {
            Term::Cell("Scaler")
        }
    }

    #[test]
    fn reify_flattens_type_nesting() {
        // 类型级嵌套 Chain<Chain<Inc,Scaler>,Inc> → 值级递归项。
        type T = Chain<Chain<Inc, Scaler>, Inc>;
        let t = <T as Reify>::term();
        assert_eq!(
            t,
            Term::chain(
                Term::chain(Term::cell("Inc"), Term::cell("Scaler")),
                Term::cell("Inc")
            )
        );
    }

    #[test]
    fn reify_covers_core_generators() {
        // 张量、对称、恒等、有界重复——修正后词汇的项表示。
        type T = Par<Chain<Id<i32>, Inc>, Rep<3, Scaler>>;
        let t = <T as Reify>::term();
        assert_eq!(
            t,
            Term::par(
                Term::chain(Term::Id, Term::cell("Inc")),
                Term::Rep(3, Box::new(Term::cell("Scaler")))
            )
        );
    }

    #[test]
    fn reify_trace_form() {
        // 迹形态：Feedback 的项表示。
        type T = Feedback<Inc, Inc>;
        let t = <T as Reify>::term();
        assert_eq!(t, Term::Feedback(Box::new(Term::cell("Inc")), Box::new(Term::cell("Inc"))));
    }

    #[test]
    fn term_is_comparable_and_hashable() {
        // 商判定的前提：项可判等、可排序、可散列（derive 完备性检查）。
        let a = Term::chain(Term::cell("Inc"), Term::cell("Scaler"));
        let b = Term::chain(Term::cell("Inc"), Term::cell("Scaler"));
        let c = Term::chain(Term::cell("Scaler"), Term::cell("Inc"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = alloc::collections::BTreeSet::new();
        set.insert(a);
        set.insert(b);
        set.insert(c);
        assert_eq!(set.len(), 2); // a==b 合并，c 独立
    }
}
