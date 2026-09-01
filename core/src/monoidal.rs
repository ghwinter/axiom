//! **monoidal — 张量结构（S₀ 词汇的一次宪法修正）**
//!
//! 来源与地位：本体论审计（第二轮，2026-09）发现 F1——T2 宣称"对称可交换"，
//! 但态射级缺少并行组合的见证：并行只以类型乘积的元数编码存在
//! （[`Broadcast`](crate::cell_core::Broadcast)/[`Merge`](crate::cell_core::Merge)）。
//! 本模块是该裁决选项 (b) 的落地：把**对称张量幺半结构**的生成元补进词汇表，
//! 使 T2 对称律获得态射级见证，trace 形态（Feedback = 迹）的单幺前提齐备。
//!
//! 这是 [`cell_core`](crate::cell_core) 五概念封闭边界之外的**一次显式修正**，
//! 不是对既有词汇的溶解——模块独立成篇，正是为了让"修正"作为修正被看见。
//!
//! 词汇增量（第一修正，四个生成元 + 第二修正，相干同构族四族八格，
//! 全部仍是 [`PortCell`]——组合封闭不破）：
//!
//! | 生成元 | 代数角色 | 见证 |
//! |---|---|---|
//! | [`Par`](crate::monoidal::Par) | 张量积 ⊗（无共享并置） | 单幺结构的积 |
//! | [`Swap`](crate::monoidal::Swap) | 对称 σ（交换子） | T2 对称律的态射级见证 |
//! | [`Discard`](crate::monoidal::Discard) | 余单位 ε（消灭） | comonoid 的 counit |
//! | [`Duplicate`](crate::monoidal::Duplicate) | 余乘 δ（复制，需 `Clone`） | comonoid 的 comultiplier |
//! | [`Assoc`](crate::monoidal::Assoc) / [`AssocInv`](crate::monoidal::AssocInv) | 结合子 α 及其逆 | 张量结合律的同构见证 |
//! | [`UnitL`](crate::monoidal::UnitL) / [`UnitLInv`](crate::monoidal::UnitLInv) | 左单位子 λ 及其逆 | 张量左单位律 |
//! | [`UnitR`](crate::monoidal::UnitR) / [`UnitRInv`](crate::monoidal::UnitRInv) | 右单位子 ρ 及其逆 | 张量右单位律 |
//! | [`Slide`](crate::monoidal::Slide) / [`SlideInv`](crate::monoidal::SlideInv) | 半辫（central slide）及其逆 | premonoidal 现实下的交换 |
//!
//! 幺半单位对象 = `()`，其上的恒等单元即 [`Id<()>`](crate::cell_core::Id)；
//! 第二修正起 λ/ρ 与该单位对象**绑定声明**（听证 D 条件 iii）——单位对象的
//! 选择直接决定 vanishing 律的内容。
//!
//! **第二次宪法修正（2026-09-01，听证 D，用户裁决）**：
//!
//! 1. **Feedback = 守卫反馈（一步延迟）**：[`Feedback`](crate::cell_core::Feedback)
//!    的既有 C2 结构（一次内联无缓冲回环迭代，每外部拍 BODY→FEED→BODY 各一拍，
//!    有界、全函数）批准为宪法语义。不动点 trace 不取——严格求值 Rust 下它
//!    需要运行时迭代（`step` 丧失总称性）、惰性 thunk（零成本/no_std 丧失）或
//!    消息链（拍边界丧失）三选一。**Yanking 律失败**（feedback∘braid ≠ braid，
//!    多一拍）被升格为断言对象（`laws::tests::yanking_fails_under_guarded_ruling`）。
//! 2. **相干同构族入词汇**（G1 闭合）：Haskell 实证（circuits 仓库）表明
//!    feedback/merge/superpose 三个核心定义缺 α 即不可写。律以 **oracle 行为
//!    断言**审计而非类型 discharge（"lawfulness is an audit concern rather
//!    than a discharge concern"——与宪法层听觉面同构）。
//! 3. **Central Sliding 侧条件**（声明）：持有 ε/δ 使基范畴非笛卡尔，
//!    sliding 律按 Benton–Hyland 需中心性侧条件；本词汇的同构格均为纯重排
//!    （无效应），slide 律在重排格上成立——侧条件是对基的声明而非词汇的
//!    限制。
//!
//! **诚实边界**（与审计 F2 一致）：本结构的诸律（结合/单位/对称/相干）在
//! Rust 类型系统里是**行为同构而非类型相等**——`Par<Par<A,B>,C>` 与
//! `Par<A,Par<B,C>>` 的状态元组嵌套形状不同，类型上不等。律的可执行检查在
//! [`crate::laws`]（行为等价测试）与语义层 `egraph`（商机器）；两者都属
//! 模态③器械，不是类型级①判定。

use crate::cell_core::PortCell;

// ── 1. 张量积（Par，⊗）────────────────────────────────────────────

/// 张量积：两个端口体的**无共享并置**——各自独立演化，输入成对、输出成对。
///
/// 与 [`Broadcast`](crate::cell_core::Broadcast) 的分工（防词汇混叠）：
/// - `Par<C1, C2>` 是**积**：一个输入对 `(C1::In, C2::In)` 同时进入两格，
///   无值共享（无 `Clone` 要求）——这是幺半结构的 ⊗；
/// - `Broadcast` 是**扇出**：一个值复制后流向两个同型接收者——那是因果
///   数据流的拓扑形态，不是积。
///
/// 代数角色：⊗ 使 cell 集合成为张量幺半范畴的对象侧；结合律与单位律以
/// 行为同构成立（见模块头"诚实边界"），对称由 [`Swap`] 见证。
pub struct Par<C1, C2>(core::marker::PhantomData<(C1, C2)>);

impl<C1, C2> PortCell for Par<C1, C2>
where
    C1: PortCell,
    C2: PortCell,
{
    type In = (C1::In, C2::In);
    type Out = (C1::Out, C2::Out);
    type State = (C1::State, C2::State);

    #[inline(always)]
    fn step((s1, s2): &mut Self::State, (i1, i2): Self::In) -> Self::Out {
        (C1::step(s1, i1), C2::step(s2, i2))
    }
}

// ── 2. 对称（Swap，σ）─────────────────────────────────────────────

/// 对称：交换输入对的两个分量。这是 T2"对称可交换"的**态射级见证**——
/// 对称律不再只是声明，而是一个可构造、可组合、可测试的单元。
///
/// 对合律：`Chain<Swap<I1,I2>, Swap<I2,I1>>` 与 `Id<(I1, I2)>` 行为相等
/// （律断言测试见 [`crate::laws`]）。自然性：`Chain<Par<A,B>, Swap>` 与
/// `Chain<Swap, Par<B,A>>` 行为相等（同符号两端同型时）。
pub struct Swap<I1, I2>(core::marker::PhantomData<(I1, I2)>);

impl<I1, I2> PortCell for Swap<I1, I2> {
    type In = (I1, I2);
    type Out = (I2, I1);
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), (a, b): (I1, I2)) -> (I2, I1) {
        (b, a)
    }
}

// ── 3. 余单位（Discard，ε）────────────────────────────────────────

/// 余单位：消灭输入，产出幺半单位 `()`。
///
/// comonoid 的 counit 侧。与 [`Duplicate`] 配对后，迹（[`Feedback`](crate::cell_core::Feedback)
/// 形态）的展开方程才具备生成元基础——signal flow calculus 对应（本体论审计
/// 猜想 12.10）所需的余代数结构由此补齐。消灭是**显式声明**：值在此处
/// 终止，无静默丢失（L1 的形状侧等价物——判定由类型 `Out = ()` 承载）。
pub struct Discard<I>(core::marker::PhantomData<I>);

impl<I> PortCell for Discard<I> {
    type In = I;
    type Out = ();
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), _input: I) {}
}

// ── 4. 余乘（Duplicate，δ）────────────────────────────────────────

/// 余乘：复制输入为一个对。要求 `I: Clone`——复制的**机制**与
/// [`Broadcast`](crate::cell_core::Broadcast) 同理归物理载体侧声明
/// （值语义下一个值流到多处，至少要能复制）。
///
/// comonoid 的 comultiplier 侧。余结合律（`Δ` 的两次嵌套经结合子同构相等）
/// 与余单位律（`Δ` 后舍一侧 = 恒等）的行为等价测试见 [`crate::laws`]。
pub struct Duplicate<I>(core::marker::PhantomData<I>);

impl<I: Clone> PortCell for Duplicate<I> {
    type In = I;
    type Out = (I, I);
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), input: I) -> (I, I) {
        (input.clone(), input)
    }
}

// ── 5. 相干同构族（α / λ / ρ / 半辫；第二次修正，听证 D）──────────
//
// 四族**可逆重排格**，各带逆向（Rust 无内建逆，正逆各一格）。全部无状态、
// 纯移动（无 `Clone` 要求）。律以 oracle 行为断言审计（往返律、张量单位律、
// 自然性）——见 [`crate::laws`]。证据来源：circuits（Haskell）实证 α/λ/ρ/
// slide 是 feedback/merge/superpose 定义的必要前件（field-research §2）。

/// 结合子 α：`((A, B), C) → (A, (B, C))`（左嵌套重联为右嵌套）。
///
/// 张量结合律的同构见证：`Par<Par<A,B>,C>` 与 `Par<A,Par<B,C>>` 类型上不等
/// （状态元组嵌套形状不同，见模块头"诚实边界"），α 是两者之间的**可执行桥**。
/// 往返律 `Chain<Assoc, AssocInv> ≡ Id` 见 [`crate::laws`]。
pub struct Assoc<A, B, C>(core::marker::PhantomData<(A, B, C)>);

impl<A, B, C> PortCell for Assoc<A, B, C> {
    type In = ((A, B), C);
    type Out = (A, (B, C));
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), ((a, b), c): ((A, B), C)) -> (A, (B, C)) {
        (a, (b, c))
    }
}

/// 结合子逆 α⁻¹：`(A, (B, C)) → ((A, B), C)`。[`Assoc`] 的逆向。
pub struct AssocInv<A, B, C>(core::marker::PhantomData<(A, B, C)>);

impl<A, B, C> PortCell for AssocInv<A, B, C> {
    type In = (A, (B, C));
    type Out = ((A, B), C);
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), (a, (b, c)): (A, (B, C))) -> ((A, B), C) {
        ((a, b), c)
    }
}

/// 左单位子 λ：`((), A) → A`——幺半单位从左侧消去。
///
/// 与单位对象 `()` **绑定声明**（听证 D 条件 iii）。张量左单位律的可执行
/// 见证：`Chain<Par<Id<()>, C>, UnitL> ≡ C`。
pub struct UnitL<I>(core::marker::PhantomData<I>);

impl<I> PortCell for UnitL<I> {
    type In = ((), I);
    type Out = I;
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), (_unit, a): ((), I)) -> I {
        a
    }
}

/// 左单位子逆 λ⁻¹：`A → ((), A)`。[`UnitL`] 的逆向（引入单位）。
pub struct UnitLInv<I>(core::marker::PhantomData<I>);

impl<I> PortCell for UnitLInv<I> {
    type In = I;
    type Out = ((), I);
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), a: I) -> ((), I) {
        ((), a)
    }
}

/// 右单位子 ρ：`(A, ()) → A`——幺半单位从右侧消去。
pub struct UnitR<I>(core::marker::PhantomData<I>);

impl<I> PortCell for UnitR<I> {
    type In = (I, ());
    type Out = I;
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), (a, _unit): (I, ())) -> I {
        a
    }
}

/// 右单位子逆 ρ⁻¹：`A → (A, ())`。[`UnitR`] 的逆向。
pub struct UnitRInv<I>(core::marker::PhantomData<I>);

impl<I> PortCell for UnitRInv<I> {
    type In = I;
    type Out = (I, ());
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), a: I) -> (I, ()) {
        (a, ())
    }
}

/// 半辫（central slide）：`(A, (S, B)) → (B, (S, A))`——把两侧因子**绕过**
/// 中心 `S` 交换，中心原位不动。
///
/// premonoidal 现实下的交换（听证 D 条件 ii）：持有 ε/δ 的基范畴非笛卡尔，
/// 全对称 σ 的自由滑动需中心性侧条件（Benton–Hyland）；slide 只对**环绕
/// 中心的重排**成立，是融合/反馈定义（`assoc ∘ slide ∘ strength ∘ assoc'`
/// 链）实际使用的形状。纯移动、无 `Clone`。侧条件见模块头"Central Sliding"。
pub struct Slide<S, A, B>(core::marker::PhantomData<(S, A, B)>);

impl<S, A, B> PortCell for Slide<S, A, B> {
    type In = (A, (S, B));
    type Out = (B, (S, A));
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), (a, (s, b)): (A, (S, B))) -> (B, (S, A)) {
        (b, (s, a))
    }
}

/// 半辫逆向：`(B, (S, A)) → (A, (S, B))`。[`Slide`] 的逆向。
pub struct SlideInv<S, A, B>(core::marker::PhantomData<(S, A, B)>);

impl<S, A, B> PortCell for SlideInv<S, A, B> {
    type In = (B, (S, A));
    type Out = (A, (S, B));
    type State = ();

    #[inline(always)]
    fn step(_state: &mut (), (b, (s, a)): (B, (S, A))) -> (A, (S, B)) {
        (a, (s, b))
    }
}
