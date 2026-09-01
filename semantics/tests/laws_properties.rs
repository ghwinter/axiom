//! 律断言的属性测试面（听证 B：推演 4 表格项并入 T6 对拍器械）。
//!
//! `core/src/laws.rs` 以固定向量做 oracle 行为审计（src 内零依赖纪律）；
//! 本测试面以属性测试补齐该表标注归属性测试的三项（dev-dep，不入 src）：
//!
//! 1. **结合律行为级属性测试**：多步游程等价（laws.rs 注记"多步游程等价
//!    归属性测试/商机器"）——`Chain(Chain(a,b),c) ≡ Chain(a,Chain(b,c))`
//!    在随机输入序列、有状态实例上逐步比较全迹。
//! 2. **迹展开等价**：组合迹 = 逐段展开——`trace(Chain<A,B>, xs) ≡
//!    trace(B, trace(A, xs))`；张量面 `trace(Par<A,B>, pairs) ≡
//!    (trace(A), trace(B))` 分量展开。
//! 3. **Broadcast 复制语义**：复制忠实（`Δ(x) = (x,x)`，随机值域含极值）
//!    与余单位律的随机游程面（`Δ;⟨id,ε⟩ ≡ Id`、`Δ;⟨ε,id⟩ ≡ Id`）。
//! 4. **相容性（interchange，I5 缺口补齐）**：`(f⊗g)∘(h⊗k) ≡ (f∘h)⊗(g∘k)`
//!    的有状态实例多步游程面。
//!
//! 判据同 laws.rs：行为等价（模态③器械——证明律在被测实例上成立）。

use axiom::cell_core::{Chain, Id, PortCell};
use axiom::monoidal::{Duplicate, Par};
use proptest::prelude::*;

/// 多步游程：同一实例状态上依次喂入序列，收集全迹。
fn run_trace<A: PortCell>(xs: &[A::In]) -> Vec<A::Out>
where
    A::In: Clone,
{
    let mut s = A::State::default();
    xs.iter().map(|x| A::step(&mut s, x.clone())).collect()
}

// 被测实例（与 laws.rs 同族：wrapping 算术覆盖全值域）。
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

struct Acc;
impl PortCell for Acc {
    type In = i32;
    type Out = i32;
    type State = i32;
    #[inline(always)]
    fn step(s: &mut i32, x: i32) -> i32 {
        *s = s.wrapping_add(x);
        *s
    }
}

// 余单位检验装置（同 laws.rs：投影不属 S₀ 生成元，不进词汇表）。
struct KeepFst<I>(core::marker::PhantomData<I>);
impl<I> PortCell for KeepFst<I> {
    type In = (I, I);
    type Out = I;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), (a, _): (I, I)) -> I {
        a
    }
}

struct KeepSnd<I>(core::marker::PhantomData<I>);
impl<I> PortCell for KeepSnd<I> {
    type In = (I, I);
    type Out = I;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), (_, b): (I, I)) -> I {
        b
    }
}

proptest! {
    // ── 1. 结合律行为级属性测试（多步游程，有状态实例）────────────────

    #[test]
    fn associativity_run_equivalence(xs in proptest::collection::vec(any::<i32>(), 0..64)) {
        type L = Chain<Chain<Acc, Scaler>, Acc>;
        type R = Chain<Acc, Chain<Scaler, Acc>>;
        prop_assert_eq!(run_trace::<L>(&xs), run_trace::<R>(&xs));
    }

    // ── 2. 迹展开等价 ────────────────────────────────────────────────

    // ── 1.5 相容性（interchange，I5 缺口补齐）────────────────────────

    #[test]
    fn interchange_run_equivalence(
        pairs in proptest::collection::vec((any::<i32>(), any::<i32>()), 0..64),
    ) {
        // (f⊗g)∘(h⊗k) ≡ (f∘h)⊗(g∘k)：有状态实例多步游程面。
        type L = Chain<Par<Acc, Scaler>, Par<Acc, Scaler>>;
        type R = Par<Chain<Acc, Acc>, Chain<Scaler, Scaler>>;
        prop_assert_eq!(run_trace::<L>(&pairs), run_trace::<R>(&pairs));
    }

    #[test]
    fn trace_expansion_chain(xs in proptest::collection::vec(any::<i32>(), 0..64)) {
        // trace(Chain<A,B>, xs) ≡ trace(B, trace(A, xs))：A=Acc（状态）、B=Scaler。
        let via_a = run_trace::<Acc>(&xs);
        let via_b = run_trace::<Scaler>(&via_a);
        type C = Chain<Acc, Scaler>;
        prop_assert_eq!(run_trace::<C>(&xs), via_b);
    }

    #[test]
    fn trace_expansion_par(pairs in proptest::collection::vec((any::<i32>(), any::<i32>()), 0..64)) {
        // trace(Par<A,B>, pairs) ≡ (trace(A), trace(B))：张量面分量展开，
        // 两翼状态独立演化（Acc 累积不串扰 Scaler）。
        let left: Vec<i32> = pairs.iter().map(|p| p.0).collect();
        let right: Vec<i32> = pairs.iter().map(|p| p.1).collect();
        let ta = run_trace::<Acc>(&left);
        let tb = run_trace::<Scaler>(&right);
        type P = Par<Acc, Scaler>;
        let composite = run_trace::<P>(&pairs);
        let expanded: Vec<(i32, i32)> = ta.into_iter().zip(tb).collect();
        prop_assert_eq!(composite, expanded);
    }

    // ── 3. Broadcast 复制语义 ────────────────────────────────────────

    #[test]
    fn broadcast_copies_faithfully(x in any::<i32>()) {
        // 复制忠实：Δ(x) = (x, x)（含极值；wrapping 语义下无特殊值）。
        prop_assert_eq!(Duplicate::<i32>::step(&mut (), x), (x, x));
    }

    #[test]
    fn broadcast_counit_first_runs(xs in proptest::collection::vec(any::<i32>(), 0..64)) {
        // Δ;⟨id,ε⟩ ≡ Id（左舍）：随机游程面。
        type Fst = Chain<Duplicate<i32>, KeepFst<i32>>;
        prop_assert_eq!(run_trace::<Fst>(&xs), run_trace::<Id<i32>>(&xs));
    }

    #[test]
    fn broadcast_counit_second_runs(xs in proptest::collection::vec(any::<i32>(), 0..64)) {
        // Δ;⟨ε,id⟩ ≡ Id（右舍）：随机游程面。
        type Snd = Chain<Duplicate<i32>, KeepSnd<i32>>;
        prop_assert_eq!(run_trace::<Snd>(&xs), run_trace::<Id<i32>>(&xs));
    }

    #[test]
    fn broadcast_into_par_disjoint(xs in proptest::collection::vec(any::<i32>(), 0..64)) {
        // 复制后张量：trace(Chain<Δ, Par<Acc, Scaler>>, xs) 的两翼
        // = (trace(Acc, xs), trace(Scaler, xs))——复制语义与张量组合相容
        // （两侧各收一份忠本，独立演化）。
        let ta = run_trace::<Acc>(&xs);
        let tb = run_trace::<Scaler>(&xs);
        type W = Chain<Duplicate<i32>, Par<Acc, Scaler>>;
        let composite = run_trace::<W>(&xs);
        let expanded: Vec<(i32, i32)> = ta.into_iter().zip(tb).collect();
        prop_assert_eq!(composite, expanded);
    }
}
