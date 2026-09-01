//! **laws — 律断言（推演 4 表的代码落点）**
//!
//! 本体论审计推演 4 修正了 T2 断言的模态分层：单位律①（类型级已实现，
//! `Id` + 既有测试）、结合律③（行为同构，此前**无书面断言**）、对称律③
//! （此前**无态射级见证**）。本模块把这张表的每一行落成可执行断言：
//!
//! | 律 | 形式 | 断言（本模块测试） |
//! |---|---|---|
//! | 单位（左/右） | `Chain<Id,C> ≡ C`、`Chain<C,Id> ≡ C` | `unit_law_*` |
//! | 结合 | `Chain(Chain(a,b),c) ≡ Chain(a,Chain(b,c))` | `associativity_*` |
//! | 对合 | `Swap ∘ Swap ≡ Id` | `swap_is_involutive` |
//! | 对称自然性 | `Par ∘ Swap ≡ Swap ∘ Par`（换侧） | `symmetry_is_natural` |
//! | 余单位 | `Duplicate` 舍一侧 `≡ Id` | `comonoid_counit_law` |
//! | 相干往返（听证 D） | `α⁻¹∘α ≡ Id`、`λ⁻¹∘λ ≡ Id`、`ρ⁻¹∘ρ ≡ Id`、slide 往返 | `associator_round_trip` 等 |
//! | 张量单位（听证 D） | `λ ∘ (Id⊗C) ≡ C`、`ρ ∘ (C⊗Id) ≡ C` | `tensor_unit_through_lambda` |
//! | 半辫自然性 | `slide ∘ (f⊗id⊗g) ≡ (g⊗id⊗f) ∘ slide` | `slide_is_natural` |
//! | 守卫反馈（正） | `Feedback<C,C> ≡ C³`（有界内联迭代，C2） | `feedback_is_bounded_iteration` |
//! | Yanking（负见证） | `feedback(braid) ≠ braid`（多一拍） | `yanking_fails_under_guarded_ruling` |
//!
//! **Central Sliding 侧条件**（听证 D 声明）：持有 ε/δ 使基范畴非笛卡尔，
//! slide 律按 Benton–Hyland 需中心性侧条件；本词汇的同构格均为纯重排
//! （无效应），slide 律在重排格上成立——侧条件是对基的声明。
//!
//! 判据是**行为等价**（同一输入序列、各自默认初态、逐步比较输出）——不是
//! 类型相等（结合律/张量律在 Rust 类型上不可判等，审计 F2）。这使本模块
//! 属模态③器械：它证明律在**被测实例**上成立，不证明律在任何实例上成立。
//! 全例断言（∀实例）归语义层 `egraph` 商机器与未来的重写系统合流性证明。
//!
//! 零依赖：不引入属性测试框架；输入取固定向量（确定性、可复现）。

use crate::cell_core::PortCell;

// ── 器械：行为等价判定 ────────────────────────────────────────────

/// 判定两个同接口端口体在给定输入序列上行为是否一致（各自默认初态、逐步比较）。
///
/// 要求 `In: Clone`（同一输入喂给双方）与 `Out: PartialEq`。
/// 这是模态③探针：真值只覆盖被测输入与被测实现。
pub fn behaviors_match<C1, C2>(inputs: &[C1::In]) -> bool
where
    C1: PortCell,
    C1::In: Clone,
    C1::Out: PartialEq,
    C2: PortCell<In = C1::In, Out = C1::Out>,
{
    inputs.iter().all(|x| {
        let mut s1 = C1::State::default();
        let mut s2 = C2::State::default();
        C1::step(&mut s1, x.clone()) == C2::step(&mut s2, x.clone())
    })
}

/// 断言版（测试与 CI 用）：行为不一致即 panic，带定位信息。
pub fn assert_same_behavior<C1, C2>(inputs: &[C1::In], what: &str)
where
    C1: PortCell,
    C1::In: Clone,
    C1::Out: PartialEq + core::fmt::Debug,
    C2: PortCell<In = C1::In, Out = C1::Out>,
{
    assert!(
        behaviors_match::<C1, C2>(inputs),
        "law violated: {what}"
    );
}

// ── 测试 ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_core::{Chain, Feedback, Id};
    use crate::monoidal::{
        Assoc, AssocInv, Discard, Duplicate, Par, Slide, SlideInv, Swap, UnitL, UnitLInv, UnitR,
        UnitRInv,
    };

    // 被测实例：确定性纯函数格（wrapping 算术——律测试覆盖全值域含极值）。
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
    // 状态格：验证律断言对有状态实例同样可表达。
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

    const INPUTS: [i32; 8] = [0, 1, -1, 5, 100, -100, i32::MAX / 2, i32::MIN / 2];
    const PAIRS: [(i32, i32); 4] = [(0, 0), (1, -1), (5, 100), (-3, 7)];

    // ── 单位律（推演4 表：模态①，已有类型级实现，此处补行为面）────

    #[test]
    fn unit_law_left() {
        type L = Chain<Id<i32>, Scaler>;
        assert_same_behavior::<L, Scaler>(&INPUTS, "Chain<Id,C> ≡ C (left unit)");
        // 有状态实例同断言。
        type LS = Chain<Id<i32>, Acc>;
        assert_same_behavior::<LS, Acc>(&INPUTS, "Chain<Id,C> ≡ C (left unit, stateful)");
    }

    #[test]
    fn unit_law_right() {
        type R = Chain<Scaler, Id<i32>>;
        assert_same_behavior::<R, Scaler>(&INPUTS, "Chain<C,Id> ≡ C (right unit)");
        type RS = Chain<Acc, Id<i32>>;
        assert_same_behavior::<RS, Acc>(&INPUTS, "Chain<C,Id> ≡ C (right unit, stateful)");
    }

    // ── 结合律（推演4 表：模态③，此前无书面断言——本行为面）──────

    #[test]
    fn associativity_pure() {
        type L = Chain<Chain<Inc, Scaler>, Inc>;
        type R = Chain<Inc, Chain<Scaler, Inc>>;
        assert_same_behavior::<L, R>(&INPUTS, "Chain associativity (pure cells)");
    }

    #[test]
    fn associativity_stateful() {
        type L = Chain<Chain<Acc, Scaler>, Acc>;
        type R = Chain<Acc, Chain<Scaler, Acc>>;
        // 单步、各自默认初态下的行为一致（多步游程等价归属性测试/商机器）。
        assert_same_behavior::<L, R>(&INPUTS, "Chain associativity (stateful cells)");
    }

    // ── 对称律（推演4 表：F1 裁决 (b) 后——Swap/Par 提供态射级见证）─

    #[test]
    fn swap_is_involutive() {
        // 对合：Swap ∘ Swap ≡ Id。
        type Twice = Chain<Swap<i32, i64>, Swap<i64, i32>>;
        type Identity = Id<(i32, i64)>;
        let inputs: [(i32, i64); 3] = [(1, 2), (0, -7), (100, 200)];
        assert_same_behavior::<Twice, Identity>(&inputs, "Swap ∘ Swap ≡ Id (involution)");
    }

    #[test]
    fn symmetry_is_natural() {
        // 自然性：Chain<Par<A,B>, Swap> ≡ Chain<Swap, Par<B,A>>。
        // 左：(A,B)入 → Par 产出 (A'，B') → Swap → (B', A')。
        type L = Chain<Par<Inc, Scaler>, Swap<i32, i32>>;
        // 右：(A,B)入 → Swap → (B,A) → Par<Scaler,Inc> 产出 (B', A')。
        type R = Chain<Swap<i32, i32>, Par<Scaler, Inc>>;
        assert_same_behavior::<L, R>(
            &PAIRS,
            "Chain<Par<A,B>, Swap> ≡ Chain<Swap, Par<B,A>> (symmetry naturality)",
        );
    }

    // ── 张量积基本性（Par 自身可组合、可布线）────────────────────

    #[test]
    fn par_composes_and_wires() {
        // Par<Inc, Scaler>：成对输入、成对输出、两格独立演化。
        type P = Par<Inc, Scaler>;
        let mut st = <P as PortCell>::State::default();
        assert_eq!(P::step(&mut st, (3, 4)), (4, 8));
        // 组合封闭：Par 嵌 Par 仍是 PortCell。
        type Nested = Par<P, Inc>;
        let mut st2 = <Nested as PortCell>::State::default();
        assert_eq!(Nested::step(&mut st2, ((3, 4), 10)), ((4, 8), 11));
        // Par 零大小（组合封闭的运行时面）。
        assert_eq!(core::mem::size_of::<P>(), 0);
    }

    #[test]
    fn par_is_disjoint_from_broadcast() {
        // 词汇不混叠的对照：Par 无共享（无 Clone 要求路径），
        // Broadcast 扇出复制——两者输出对在数值上可一致但结构角色不同。
        // 此测试仅固化 Par 的独立性：一侧状态演化不影响另一侧。
        type P = Par<Acc, Inc>;
        let mut st = <P as PortCell>::State::default();
        let (a1, b1) = P::step(&mut st, (5, 5));
        let (a2, b2) = P::step(&mut st, (5, 5));
        assert_eq!((a1, b1), (5, 6)); // Acc(0)+5=5; Inc 5->6
        assert_eq!((a2, b2), (10, 6)); // Acc(5)+5=10; Inc 5->6（Acc 累积，Inc 不受影响）
    }

    // ── 余单位律（comonoid：counit/comultiplier 的行为面）──────────

    #[test]
    fn comonoid_counit_law() {
        // Duplicate 后舍一侧 ≡ Id：需要"取对的第一/第二分量"的辅助格
        // （投影不属 S₀ 生成元——它是余代数方程的检验装置，不进词汇表）。
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
        type Fst = Chain<Duplicate<i32>, KeepFst<i32>>;
        type Snd = Chain<Duplicate<i32>, KeepSnd<i32>>;
        assert_same_behavior::<Fst, Id<i32>>(&INPUTS, "Δ;⟨id,ε⟩ ≡ Id (counit, first)");
        assert_same_behavior::<Snd, Id<i32>>(&INPUTS, "Δ;⟨ε,id⟩ ≡ Id (counit, second)");
    }

    #[test]
    fn discard_announces_termination() {
        // 余单位：Out = () 类型层声明"值在此终止"。
        type D = Discard<i32>;
        assert_eq!(D::step(&mut (), 42), ());
        // 组合封闭：任何格输出可布入 Discard。
        type C = Chain<Inc, Discard<i32>>;
        let mut st2 = <C as PortCell>::State::default();
        assert_eq!(C::step(&mut st2, 1), ());
    }

    // ── 相干同构律（听证 D 2026-09-01：α/λ/ρ/slide，oracle 行为审计）──

    const NESTED: [(i32, (i64, i32)); 3] = [(0, (1, -2)), (-5, (-9, 0)), (7, (100, -3))];

    #[test]
    fn associator_round_trip() {
        // α⁻¹ ∘ α ≡ Id（双向往返）。
        type Fwd = Chain<Assoc<i32, i64, bool>, AssocInv<i32, i64, bool>>;
        type Tri = ((i32, i64), bool);
        let inputs: [Tri; 3] = [((0, 1), true), ((-5, -9), false), ((7, 100), true)];
        assert_same_behavior::<Fwd, Id<Tri>>(&inputs, "α⁻¹∘α ≡ Id (associator round trip)");
        type Back = Chain<AssocInv<i32, i64, bool>, Assoc<i32, i64, bool>>;
        type Duo = (i32, (i64, bool));
        let inputs2: [Duo; 3] = [(1, (2, true)), (0, (-7, false)), (5, (100, true))];
        assert_same_behavior::<Back, Id<Duo>>(&inputs2, "α∘α⁻¹ ≡ Id (associator inverse round trip)");
    }

    #[test]
    fn unitor_round_trips() {
        // λ⁻¹ ∘ λ ≡ Id 与 ρ⁻¹ ∘ ρ ≡ Id。
        type Lam = Chain<UnitL<i32>, UnitLInv<i32>>;
        assert_same_behavior::<Lam, Id<((), i32)>>(
            &[((), 1), ((), -5), ((), 100)],
            "λ⁻¹∘λ ≡ Id (left unitor round trip)",
        );
        type LamInv = Chain<UnitLInv<i32>, UnitL<i32>>;
        assert_same_behavior::<LamInv, Id<i32>>(&INPUTS, "λ∘λ⁻¹ ≡ Id (left unitor inverse)");
        type Rho = Chain<UnitR<i32>, UnitRInv<i32>>;
        assert_same_behavior::<Rho, Id<(i32, ())>>(
            &[(1, ()), (-5, ()), (100, ())],
            "ρ⁻¹∘ρ ≡ Id (right unitor round trip)",
        );
        type RhoInv = Chain<UnitRInv<i32>, UnitR<i32>>;
        assert_same_behavior::<RhoInv, Id<i32>>(&INPUTS, "ρ∘ρ⁻¹ ≡ Id (right unitor inverse)");
    }

    #[test]
    fn tensor_unit_through_lambda() {
        // 张量单位律经 λ/ρ 见证：λ ∘ (Id⊗C) ≡ λ ∘ C（行为等价要求同接口，
        // C 侧经同构 λ' 接到 ((),I)——λ/ρ 可逆，故与 (Id⊗C) ≡ C 等价）。
        type L = Chain<Par<Id<()>, Inc>, UnitL<i32>>;
        type Rhs = Chain<UnitL<i32>, Inc>;
        let left: [((), i32); 8] = INPUTS.map(|x| ((), x));
        assert_same_behavior::<L, Rhs>(&left, "λ ∘ (Id⊗C) ≡ C (tensor left unit)");
        type R = Chain<Par<Inc, Id<()>>, UnitR<i32>>;
        type RhsR = Chain<UnitR<i32>, Inc>;
        let right: [(i32, ()); 8] = INPUTS.map(|x| (x, ()));
        assert_same_behavior::<R, RhsR>(&right, "ρ ∘ (C⊗Id) ≡ C (tensor right unit)");
    }

    #[test]
    fn slide_round_trip() {
        // slide ∘ slide⁻¹ ≡ Id（半辫往返；S=i64 中心，A=B=i32 两翼）。
        type S = Slide<i64, i32, i32>;
        type Fwd = Chain<S, SlideInv<i64, i32, i32>>;
        type Wing = (i32, (i64, i32));
        assert_same_behavior::<Fwd, Id<Wing>>(&NESTED, "slide⁻¹∘slide ≡ Id (half-braid round trip)");
        type Back = Chain<SlideInv<i64, i32, i32>, S>;
        assert_same_behavior::<Back, Id<Wing>>(&NESTED, "slide∘slide⁻¹ ≡ Id (half-braid inverse)");
    }

    #[test]
    fn slide_is_natural() {
        // 半辫自然性：slide ∘ (f⊗id_S⊗g) ≡ (g⊗id_S⊗f) ∘ slide。
        // 左：先作用 f=Inc、g=Scaler，再绕中心滑。
        type L = Chain<Par<Inc, Par<Id<i64>, Scaler>>, Slide<i64, i32, i32>>;
        // 右：先滑，再作用 g、f（两侧已换位）。
        type R = Chain<Slide<i64, i32, i32>, Par<Scaler, Par<Id<i64>, Inc>>>;
        assert_same_behavior::<L, R>(&NESTED, "slide naturality");
    }

    // ── 守卫反馈律（听证 D 裁决：Feedback = 守卫反馈，一步延迟）──────

    #[test]
    fn feedback_is_bounded_iteration() {
        // 正断言：每外部拍 = 恰两次 BODY + 一次 FEED（C2 一次内联无缓冲回环
        // 迭代——有界、全函数；不动点 trace 的"拍内求解"不取）。
        type F = Feedback<Inc, Inc>;
        type Thrice = Chain<Inc, Chain<Inc, Inc>>;
        assert_same_behavior::<F, Thrice>(&INPUTS, "Feedback<C,C> ≡ C³ (bounded inline iteration)");
    }

    #[test]
    fn yanking_fails_under_guarded_ruling() {
        // 负见证：若 Yanking 成立，feedback(braid) 应即时为辫（一步 Swap）。
        // 守卫语义下反馈环多走一拍：(a,b) → (b+2, a+2)，而非 (b+1, a+1)。
        // 律失败本身是断言对象（circuits axioma yankingFailsOk 先例）。
        type Body = Par<Inc, Inc>;
        type Braid = Swap<i32, i32>;
        let inputs: [(i32, i32); 4] = [(0, 0), (1, -1), (5, 100), (-3, 7)];
        for &(a, b) in &inputs {
            let mut sg = <Feedback<Body, Braid> as PortCell>::State::default();
            let guarded = Feedback::<Body, Braid>::step(&mut sg, (a, b));
            let instant = Braid::step(&mut (), (a, b));
            assert_ne!(
                guarded, instant,
                "yanking must fail under guarded feedback ruling (input {a},{b})"
            );
            assert_eq!(guarded, (b.wrapping_add(2), a.wrapping_add(2)));
        }
    }
}
