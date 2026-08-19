//! **cell_core — 四构件蓝图核心（编译期模型）**
//!
//! 这是 axiometric 重构后的**新主轴线**：围绕 Rust 编译器能力，把核心层的"智能"
//! 在编译期耗尽，产出与手写等价的普通 Rust，无运行时对象。
//!
//! 四构件（corresponding to the theory收敛）：
//! 1. **开放系统/端口体**（[`PortCell`]）：有边界、输入、输出、状态，`step` 纯且可内联。
//! 2. **因果数据流**（[`Link`] 的 `A.out -> B.in`）：类型层对偶配对，非法连接编译失败（T1）。
//! 3. **组合/嵌套**（[`Chain`] 等）：组合子仍是端口体，任意层级（A2 操作类）。
//! 4. **静态性声明**（[`Staticity`]）：标记哪些子图要求零成本（单态化，无 `Box<dyn>`）。
//!
//! 已移出抽象层的旧语义（归物理载体/实例层）：FlowKind 三分、LinkKind 载体/背压/时序、
//! 值形态/JSON、线程/同步异步/时序（见 refactor-plan 与 theorem T3/T9/§4.1/§4.4）。

// ── 1. 开放系统（端口体）──────────────────────────────────────────

/// 开放系统：带类型化输入/输出端口 + 内部状态 + 转移。
///
/// - `In`/`Out` 是端口类型（对偶靠它们配对，见 [`Link`]）；
/// - `State` 是内部状态，默认可构造；
/// - `step` 是纯转移（`#[inline(always)]` 使内联跨 crate 成立 → Z1 的 (b)）。
///
/// 纯抽象层——不掺线程/同步/背压/时序，那些是物理载体的事（T3/§4.4）。
pub trait PortCell: Sized {
    /// 输入端口类型（承载的值类型）。
    type In;
    /// 输出端口类型。
    type Out;
    /// 内部状态。
    type State: Default;
    /// 状态转移：读输入 -> (新状态, 输出)。必须纯、可内联。
    fn step(state: &mut Self::State, input: Self::In) -> Self::Out;
}

// ── 2. 因果数据流（布线）──────────────────────────────────────────

/// 一条带方向的因果数据流：`A.out -> B.in`。
///
/// **布线合法性 = 类型判定（T1）**：要求 `B::In == A::Out`。若类型不匹配，本
/// 类型根本无法实例化 —— 非法连接在编译期被拒绝（不是运行时检查）。
pub struct Link<A, B>(core::marker::PhantomData<(A, B)>);

impl<A, B> Link<A, B>
where
    A: PortCell,
    B: PortCell<In = A::Out>, // 类型层对偶配对
{
    /// 把 A 的当前输出驱动进 B（单次因果步进）。
    pub fn fire(astate: &mut A::State, bstate: &mut B::State, input: A::In) -> B::Out {
        let mid = A::step(astate, input);
        B::step(bstate, mid)
    }
}

// ── 3. 组合/嵌套（操作类）─────────────────────────────────────────

/// 组合 A -> B（A 输出布线到 B 输入）。仍是端口体，可再嵌套（任意层级）。
///
/// 命名 `CellChain` 以避免与旧 `static_exec::Chain` 冲突（分阶段迁移期间共存；
/// 后续收敛后恢复为 `Chain`）。
pub struct CellChain<A, B>(core::marker::PhantomData<(A, B)>);

impl<A, B> PortCell for CellChain<A, B>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
    type In = A::In;
    type Out = B::Out;
    type State = (A::State, B::State);
    #[inline(always)]
    fn step((sa, sb): &mut (A::State, B::State), input: A::In) -> B::Out {
        let mid = A::step(sa, input);
        B::step(sb, mid)
    }
}

/// 别名：新主轴线上的组合子默认名。
pub type Chain<A, B> = CellChain<A, B>;

// ── 3b. 多对多：广播 / 扇出（因果数据流到多个兼容接收者）─────────

/// 广播：把 `SRC` 的输出同时布线到多个接收者（`R1`, `R2`）。
///
/// 这是多对多连接（fan-out）的**编译期静态**表达——在类型层强制所有接收者
/// 输入类型与源输出类型一致；无 `Box<dyn>`、无运行时对象（T1 对偶配对）。
/// fan-out 到多个接收者是**因果数据流的多对多**，无需 Tee 树。
///
/// 源输出 `SRC::Out` 要求 `Clone`：多路分发在物理层本质是复制/分发——这正是
/// "物理载体"的事，抽象层只是在类型层声明"这一个值流向多个接收者"。
pub struct Broadcast<SRC, R1, R2>
where
    SRC: PortCell,
    SRC::Out: Clone,
{
    _src: core::marker::PhantomData<SRC>,
    _r1: core::marker::PhantomData<R1>,
    _r2: core::marker::PhantomData<R2>,
}

impl<SRC, R1, R2> Broadcast<SRC, R1, R2>
where
    SRC: PortCell,
    SRC::Out: Clone,
    R1: PortCell<In = SRC::Out>,
    R2: PortCell<In = SRC::Out>,
{
    /// 单步广播：SRC 产出一个输出，分别喂给两个接收者。
    #[inline(always)]
    pub fn fire(
        ssrc: &mut SRC::State,
        sr1: &mut R1::State,
        sr2: &mut R2::State,
        input: SRC::In,
    ) -> (R1::Out, R2::Out) {
        let mid = SRC::step(ssrc, input);
        let o1 = R1::step(sr1, mid.clone());
        let o2 = R2::step(sr2, mid);
        (o1, o2)
    }
}

// ── 3c. 环：反馈（因果闭合，编译期类型层表达，时序归物理载体）────

/// 反馈环：`BODY` 的输出回喂到 `BODY` 的输入，形成因果闭合。
///
/// 抽象层**只声明环的存在**（因果闭合，T3）；环是否良定义、是否需要缓冲，
/// 是物理载体的事（Kahn 通道 ⟹ 环安全；内联 ⟹ 需 Moore）。这里在类型层表达
/// 闭合：回喂经过 `FEED`（可改变值），`FEED` 的输入来自 `BODY` 输出、
/// 输出回到 `BODY` 输入 —— 编译期保证这条因果闭合合法。
pub struct Feedback<BODY, FEED>
where
    BODY: PortCell,
    FEED: PortCell<In = BODY::Out, Out = BODY::In>,
{
    _body: core::marker::PhantomData<BODY>,
    _feed: core::marker::PhantomData<FEED>,
}

impl<BODY, FEED> Feedback<BODY, FEED>
where
    BODY: PortCell,
    FEED: PortCell<In = BODY::Out, Out = BODY::In>,
{
    /// 一拍：外部输入经 BODY，输出经 FEED 成为"下一拍输入"的形态。
    ///
    /// 真实循环调度（何时用回喂值、是否需要缓冲）是物理载体职责（T3）；
    /// 本方法仅演示因果闭合在类型层成立——无运行时对象，无 Box<dyn>。
    #[inline(always)]
    pub fn tick(sbody: &mut BODY::State, sfeed: &mut FEED::State, external: BODY::In) -> BODY::Out {
        let out = BODY::step(sbody, external);
        // 回喂：out 作为 FEED 输入，产出下一次的 BODY 输入形态。
        let next_in: BODY::In = FEED::step(sfeed, out);
        // 为演示闭合，再驱动一拍（用回喂值）。真实环由载体驱动；这里类型已闭合。
        BODY::step(sbody, next_in)
    }
}


// ── 4. 静态性声明 ─────────────────────────────────────────────────

/// 静态性声明：标记"这个子图要求零成本"。
///
/// 仅对声明为静态的子图，编译期强制单态化 + 内联，验证零成本（Z ⟹ 展开）；
/// 未声明的走普通 Rust/载体路径（dynamic 税可接受）。
/// 这里用类型参数 `SUB` 承载"子图类型"，编译期确定。
pub struct Static<SUB>(core::marker::PhantomData<SUB>);

impl<SUB> Static<SUB> {
    /// 声明一个子图为静态（零成本）。
    pub fn declare() -> Self {
        Static(core::marker::PhantomData)
    }
}

// ── 驱动辅助 ──────────────────────────────────────────────────────

/// 驱动一个端口体：构造状态并单步，返回输出。
/// 完全类型参数化 → 编译器单态化（T7 静态路径）。
pub fn drive<C: PortCell>(state: &mut C::State, input: C::In) -> C::Out {
    C::step(state, input)
}

// ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct Inc;
    struct Scaler;

    impl PortCell for Inc {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x + 1
        }
    }

    impl PortCell for Scaler {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x * 2
        }
    }

    #[test]
    fn link_fires_causal_flow() {
        let (mut as_, mut bs) = ((), ());
        // Inc.out(i32) -> Scaler.in(i32)：因果数据流，一次 fire。
        let out = Link::<Inc, Scaler>::fire(&mut as_, &mut bs, 5);
        assert_eq!(out, 12); // 5 -> 6 -> 12
    }

    #[test]
    fn chain_nests_arbitrarily() {
        // CellChain<Inc, Scaler> 仍是端口体 (In=i32, Out=i32)。
        // 再链 Scaler：CellChain<CellChain<Inc,Scaler>, Scaler>
        type Three = CellChain<CellChain<Inc, Scaler>, Scaler>;
        let mut st = <Three as PortCell>::State::default();
        let out = drive::<Three>(&mut st, 2);
        // 2 -> Inc 3 -> Scaler 6 -> Scaler 12
        assert_eq!(out, 12);
    }

    #[test]
    fn static_declaration_compiles() {
        // 静态性声明是可编译的、编译期定型的标记。
        let _ = Static::<CellChain<Inc, Scaler>>::declare();
    }

    #[test]
    fn broadcast_fans_out_to_multiple_receivers() {
        // Inc.out(i32) 同时 -> Inc 与 Scaler（多对多 fan-out，无 Tee 树）。
        let (mut ss, mut sr1, mut sr2) = ((), (), ());
        // fire: src Inc(5->6); r1 Inc(6->7); r2 Scaler(6->12)
        let (o1, o2) = Broadcast::<Inc, Inc, Scaler>::fire(&mut ss, &mut sr1, &mut sr2, 5);
        assert_eq!((o1, o2), (7, 12));
    }

    #[test]
    fn feedback_closes_causally_in_types() {
        // 环：BODY=Inc, FEED=Inc，类型闭合（i32->i32 -> i32->i32）。
        // tick(external=8): Inc 8->9; feed Inc 9->10; 再 Inc 10->11
        let (mut sb, mut sf) = ((), ());
        let out = Feedback::<Inc, Inc>::tick(&mut sb, &mut sf, 8);
        assert_eq!(out, 11);
    }

    #[test]
    fn chained_relay_loop_in_types() {
        // 一个更真实的环：BODY 是一个两段链 Chain<Inc,Scaler>(i32->i32)，
        // FEED 是 Inc —— 展示"组合出的端口体仍旧可入环"，任意嵌套 + 环。
        type Body = CellChain<Inc, Scaler>;
        let (mut sb, mut sf) = (<Body as PortCell>::State::default(), ());
        // tick(external=1): Body 1->2->4; feed Inc 4->5; Body 5->6->12
        let out = Feedback::<Body, Inc>::tick(&mut sb, &mut sf, 1);
        assert_eq!(out, 12);
    }
}
