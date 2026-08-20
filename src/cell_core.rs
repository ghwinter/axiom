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
//! 值形态/JSON、线程/同步异步/时序（定理 T3/T9 与文档 `docs/foundations.md`）。

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

// ── 3b₂. 多对多：汇合 / fan-in（多个相容源合入一个接收者）─────────

/// 汇合：`S1` 与 `S2` 的输出（类型须一致）合入一个 `DST` 接收者。
///
/// 多对多连接的 fan-in 侧：两个源的类型经类型层强制与接收者输入一致（T1），
/// 无 Box<dyn>/运行时对象。汇合的"顺序"（谁先到）是物理载体的事（T3/Kahn）——
/// 抽象层只声明"多个源可布入同一接收者"这一因果形态。
pub struct Merge<S1, S2, DST>
where
    S1: PortCell,
    S2: PortCell,
    DST: PortCell<In = S1::Out>,
    S2::Out: Into<DST::In>,
{
    _s1: core::marker::PhantomData<S1>,
    _s2: core::marker::PhantomData<S2>,
    _dst: core::marker::PhantomData<DST>,
}

impl<S1, S2, DST> Merge<S1, S2, DST>
where
    S1: PortCell,
    S2: PortCell,
    DST: PortCell<In = S1::Out>,
    S2::Out: Into<DST::In>,
{
    /// 两步汇合：先驱动 S1，再驱动 S2，各输出进同一 DST 接收者（fan-in）。
    /// 因果顺序由调用决定；物理顺序/仲裁归载体（T3）。DST 状态在两 step 间保持。
    #[inline(always)]
    pub fn join(
        ss1: &mut S1::State,
        ss2: &mut S2::State,
        sdst: &mut DST::State,
        in1: S1::In,
        in2: S2::In,
    ) -> DST::Out {
        let o1 = S1::step(ss1, in1);
        DST::step(sdst, o1);
        let o2: DST::In = S2::step(ss2, in2).into();
        DST::step(sdst, o2)
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


// ── 3d. 统一模型：正则 / 星（同一 cell 的 N 次自组合）──────────────

/// 正则 / 星：同一端口体 `C` 的 `N` 次**自组合**（有界计数、编译期定型）。
///
/// `Rep<N, C>` 表示"把 `C` 重复作用 N 次"——Kleene 星 `C*` 的编译期有界片段：
/// 种类（`C` 的接口）在类型平面封闭，计数 N 是类型层面常量（编译期不变）。
///
/// 自组合要求 `C::In` 与 `C::Out` 互相可转换（即 `In == Out` 的同型表达），
/// 否则无法把输出再喂回自身。语义 = 一遍 `C::step` 的 N 次链接；
/// `State = [C::State; N]`，零分配、无运行时对象，编译期单态化 / 展开（零成本静态路径）。
///
/// > **统一模型衔接**：有界计数 N 是"正则/星"的**静片段**；无界计数（任意 N）属
/// > 生成/递归层面的运行期实例网（由 runtime/载体驱动）。本构造子表达"种类封闭、
/// > 计数为类型级常量"的部分，`N=0` 即恒等（`Rep<0,C>` 输出等于输入）。
pub struct Rep<const N: usize, C>(core::marker::PhantomData<C>);

/// Rep 的状态：`N` 个 `C::State` 的定长序列。
///
/// 自定义类型以**手动提供** `Default`（用 `core::array::from_fn`），不依赖原生数组
/// 对泛型 `N` 的 `Default` 实现（避免编译器边界问题）。
pub struct RepState<const N: usize, C: PortCell>(pub [C::State; N]);

impl<const N: usize, C> Default for RepState<N, C>
where
    C: PortCell,
{
    #[inline]
    fn default() -> Self {
        RepState(core::array::from_fn(|_| C::State::default()))
    }
}

impl<const N: usize, C: PortCell> RepState<N, C> {
    /// 定长序列长度（编译期常量 `N`）。
    #[inline(always)]
    pub const fn len(&self) -> usize {
        N
    }

    /// 长度是否为 `0`（编译期常量判定）。
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        N == 0
    }
}

impl<const N: usize, C> Rep<N, C> {
    /// 构造零大小的正则/星声明（运行时无对象）。
    pub fn declare() -> Self {
        Rep(core::marker::PhantomData)
    }
}

impl<const N: usize, C> PortCell for Rep<N, C>
where
    C: PortCell,
    C::In: From<C::Out>,
    C::Out: From<C::In>,
{
    type In = C::In;
    type Out = C::Out;
    type State = RepState<N, C>;

    #[inline(always)]
    fn step(state: &mut RepState<N, C>, input: C::In) -> C::Out {
        let mut acc: C::In = input;
        let n = state.0.len();
        for (i, s) in state.0.iter_mut().enumerate() {
            let mid: C::Out = C::step(s, acc);
            if i + 1 == n {
                // 最后一次：直接返回 Out，避免双移（mid 既作输出又作下一输入）。
                return mid;
            }
            acc = mid.into(); // 非最后一次：喂回自身（C::Out -> C::In）
        }
        // N=0：恒等，输出 = 输入（同型转换）。
        C::Out::from(acc)
    }
}

// ── 3e. 统一模型：装载槽（∃ / 定义侧）──────────────────────────────

/// 装载槽：一个"接口固定、占据者运行时可换"的**定义**（∃ 装载，编译期定型）。
///
/// `Slot<I, O>` 本身不是可运行的 cell，而是声明"这个位置需要一个 `In=I, Out=O`
/// 的端口体占据"。它把"未来占据者"约束在一个类型对偶对（T1）——这是统一模型里
/// ∃（运行时装载）在**定义侧**的锚点；具体的运行期存在化填充由 runtime/载体承担。
/// 本构造子是零大小、编译期定型的**定义**（不占运行时；定义可永不激活）。
pub struct Slot<I, O>(core::marker::PhantomData<(I, O)>);

impl<I, O> Slot<I, O> {
    /// 声明一个装载槽（运行时无对象）。
    pub fn declare() -> Self {
        Slot(core::marker::PhantomData)
    }
}

/// 编译期"合规"判定：`OCC` 能否填入 `Slot<I, O>`。
///
/// 这是 ∃ 装载槽的**参数化 T1 验证**：只要 `OCC: PortCell<In=I, Out=O>`，"未来任何
/// 符合该接口的占据者都合规"这一规则在编译期成立（对占据者的存在量化表达为类型层
/// 判定）。与 [`DoesWire`] 同构——只是把"线两端配对(T1)"表述为"槽与占据者配对(T1)"。
pub trait Conforms<SLOT> {
    /// 编译期证据：`OCC` 可填充该槽（零大小、运行时无对象）。
    const OK: bool = true;
}

impl<I, O, OCC> Conforms<Slot<I, O>> for OCC
where
    OCC: PortCell<In = I, Out = O>,
{
}

/// 断言一个占据者可填充给定装载槽（编译期）；不满足 T1 则该 impl 不存在 → 编译失败。
pub fn assert_conforms<SLOT, OCC>()
where
    OCC: Conforms<SLOT>,
{
    let _: bool = <OCC as Conforms<SLOT>>::OK;
}

// ── 3f. 统一模型：正则算子（并 / 可选）─────────────────────────────

/// 并（|）的输入标号：选 `A` 或 `B`。
pub enum ChoiceIn<IA, IB> {
    /// 派发给 `A` 的内容。
    A(IA),
    /// 派发给 `B` 的内容。
    B(IB),
}

/// 并（|）的输出标号：哪个分支产出的结果。
pub enum ChoiceOut<OA, OB> {
    /// `A` 分支的输出。
    A(OA),
    /// `B` 分支的输出。
    B(OB),
}

/// 并（|）：两个同处一个接口代数的 cell，作为**类型层的和**。
///
/// `Choice<A, B>` 接一个带标号的输入（[`ChoiceIn`]），**由输入标号决定**把内容派发给
/// `A` 或 `B` 的 `step`，产出对应标号的输出（[`ChoiceOut`]）。纯、确定（无运行时模式
/// 选择——是输入携带决定），是正则语言算子的 `|` 的**一等 PortCell** 表达；两个分支
/// 的状态各自独立保存。
pub struct Choice<A, B>(core::marker::PhantomData<(A, B)>);

impl<A, B> PortCell for Choice<A, B>
where
    A: PortCell,
    B: PortCell,
{
    type In = ChoiceIn<A::In, B::In>;
    type Out = ChoiceOut<A::Out, B::Out>;
    type State = (A::State, B::State);

    #[inline(always)]
    fn step(
        (sa, sb): &mut (A::State, B::State),
        input: ChoiceIn<A::In, B::In>,
    ) -> ChoiceOut<A::Out, B::Out> {
        match input {
            ChoiceIn::A(i) => ChoiceOut::A(A::step(sa, i)),
            ChoiceIn::B(i) => ChoiceOut::B(B::step(sb, i)),
        }
    }
}

/// 可选（?）：对可选输入应用或不应用 `C`（正则的 0 或 1 次）。
///
/// `Opt<C>` 把 `Option<C::In>` 变换为 `Option<C::Out>`：`None` 恒等（0 次，状态不变），
/// `Some` 应用一次 `C::step`（1 次）。纯、确定、类型层可组合；正则语言算子的 `?`
/// 的一等 PortCell 表达。
pub struct Opt<C>(core::marker::PhantomData<C>);

impl<C> PortCell for Opt<C>
where
    C: PortCell,
{
    type In = Option<C::In>;
    type Out = Option<C::Out>;
    type State = C::State;

    #[inline(always)]
    fn step(s: &mut C::State, input: Option<C::In>) -> Option<C::Out> {
        input.map(|i| C::step(s, i))
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

// ── 4b. 蓝图即类型（无 JSON / 值形态中间层；无运行时对象）──────

/// 一张蓝图 = 一个**零大小、编译期定型**的类型（类型参数集合）。
///
/// 与"值形态蓝图/JSON"相反：这里蓝图不是一个运行时对象，而是一个类型参数
/// 集合（§4.1）。`TOP` 承载整个拓扑（端口体 + 连接 + 组合 + 静态性），
/// `size_of::<Blueprint<TOP>>() == 0` —— 运行时没有任何蓝图对象，只有编译器
/// 据类型生成的业务代码。
pub struct Blueprint<TOP>(core::marker::PhantomData<TOP>);

impl<TOP> Blueprint<TOP> {
    /// 定义/冻结一张蓝图（编译期定型，运行时零对象）。
    pub fn define() -> Self {
        Blueprint(core::marker::PhantomData)
    }
}

/// 编译期证明：任何蓝图都是零大小，运行时零对象。
/// 这是"无值形态/无 JSON/无运行时蓝图"（§4.1）的常量级证据。
pub const fn blueprint_is_zero_sized<TOP>() -> bool {
    core::mem::size_of::<Blueprint<TOP>>() == 0
}

// ── 驱动辅助 ──────────────────────────────────────────────────────

/// 驱动一个端口体：构造状态并单步，返回输出。
/// 完全类型参数化 → 编译器单态化（T7 静态路径）。
pub fn drive<C: PortCell>(state: &mut C::State, input: C::In) -> C::Out {
    C::step(state, input)
}

// ── 5. 编译期验证（能力到编译期耗尽）────────────────────────────

/// 编译期布线判定：`A` 的输出能否布到 `B` 的输入（因果数据流对偶配对）。
///
/// 这是一个**编译期的验证产物**：若 `DoesWire<A,B>` 可构造（impl 存在），
/// 则这条布线在该类型对偶下合法——与运行时验证无关，纯类型层（T1）。
pub trait DoesWire<A, B> {
    /// 编译期证据：一条合法的因果数据流 A -> B（零大小、运行时无对象）。
    const WIRES: bool = true;
}

impl<A, B> DoesWire<A, B> for ()
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
}

/// 断言一条布线合法：编译期成立则产生零大小证据；若类型不配对则该 impl 不存在 → 编译错误。
/// 这是"用于分析与验证"的入口——验证在编译期完成，运行期零开销。
pub fn assert_wiring<A, B>()
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
    let _: bool = <() as DoesWire<A, B>>::WIRES;
}

/// 将一条编译期验证的布线冻结为一个零大小值（合并"验证产物"与"类型即蓝图"）。
pub fn wired<A, B>()
where
    A: PortCell,
    B: PortCell<In = A::Out>,
{
    let _ = assert_wiring::<A, B>;
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

    #[test]
    fn blueprint_is_zero_sized_zero_runtime_object() {
        // 蓝图是零大小类型 → 运行时无对象（该"蓝图即类型"，无 JSON/值形态中间层）。
        type Top = CellChain<Inc, CellChain<Scaler, Inc>>;
        assert!(blueprint_is_zero_sized::<Top>());
        assert_eq!(core::mem::size_of::<Blueprint<Top>>(), 0);
    }

    #[test]
    fn compile_time_wiring_verification() {
        // 编译期布线判定：Inc.out(i32) 布到 Scaler.in(i32) 合法（DoesWire 可构造）。
        // 这是"能力到编译期耗尽用于验证"的入口——运行期零开销。
        assert_wiring::<Inc, Scaler>();
        assert_wiring::<CellChain<Inc, Scaler>, Scaler>();
        // 一条合法布线的编译期证据存在（关联常量）。
        let _: bool = <() as DoesWire<Inc, Scaler>>::WIRES;
    }

    #[test]
    fn rep_repeats_same_cell_n_times() {
        // Rep<3, Inc>：Inc 3 次自组合，5 -> 6 -> 7 -> 8
        type R = Rep<3, Inc>;
        let mut st = <R as PortCell>::State::default();
        let out = drive::<R>(&mut st, 5);
        assert_eq!(out, 8);
    }

    #[test]
    fn rep_zero_is_identity() {
        // Rep<0, Inc>：恒等，5 -> 5（输出=输入）
        type R = Rep<0, Inc>;
        let mut st = <R as PortCell>::State::default();
        assert_eq!(drive::<R>(&mut st, 5), 5);
        assert_eq!(<R as PortCell>::State::default().len(), 0);
    }

    #[test]
    fn rep_state_is_fixed_array_of_n() {
        // State 是 [C::State; N]：长度为 N，不引入运行时对象/N 计数。
        type R = Rep<4, Inc>;
        assert_eq!(core::mem::size_of::<R>(), 0); // 类型本身零大小
        let st = <R as PortCell>::State::default();
        assert_eq!(st.len(), 4);
    }

    #[test]
    fn rep_wires_into_scaler_compile_time() {
        // Rep<2, Inc>.out(i32) -> Scaler.in(i32)：T1 类型层验证成立。
        assert_wiring::<Rep<2, Inc>, Scaler>();
        // 组合 stella：Rep<2,Inc>(5->6->7) 再 Scaler(7->14)
        type Body = CellChain<Rep<2, Inc>, Scaler>;
        let mut st = <Body as PortCell>::State::default();
        assert_eq!(drive::<Body>(&mut st, 5), 14);
    }

    #[test]
    fn slot_declares_interface_and_conforms() {
        // 装载槽定义是可编译的、零大小的（∃ 装载在定义侧）。
        let _ = Slot::<i32, i32>::declare();
        // 槽的接口（I=O=i32）可被任意符合该对偶的占据者参数化地填入（T1）。
        type S = Slot<i32, i32>;
        assert_conforms::<S, Inc>();
        // Rep 也是 In=Out=i32 → 仍可填充同一槽（未来占据者的存在量化）。
        assert_conforms::<S, Rep<3, Inc>>();
        let _: bool = <Inc as Conforms<S>>::OK;
    }

    #[test]
    fn choice_dispatches_by_input_tag() {
        // Choice<Inc, Scaler>：输入标号决定分支（纯、由输入决定）。
        // Inc(i32): +1；Scaler(i32): 已是标量语义（此处用其 state 宏）。
        type C = Choice<Inc, Scaler>;
        let mut st = <C as PortCell>::State::default();
        // A 分支：Inc(0).step(5) -> 6
        assert!(matches!(
            drive::<C>(&mut st, ChoiceIn::A(5)),
            ChoiceOut::A(6)
        ));
        // B 分支：Scaler 翻倍，Scaler(0).step(2) -> 4
        assert!(matches!(
            drive::<C>(&mut st, ChoiceIn::B(2)),
            ChoiceOut::B(4)
        ));
    }

    #[test]
    fn opt_maps_option_identity_or_apply() {
        // Opt<Inc>：None 恒等；Some(x) 应用一次 Inc（Opt::State=Inc::State=()，无状态）。
        assert_eq!(drive::<Opt<Inc>>(&mut (), None), None);
        assert_eq!(drive::<Opt<Inc>>(&mut (), Some(5)), Some(6));
    }

    #[test]
    fn choice_opt_compose_as_port_cells() {
        // Opt 可链：Opt<Inc>::Out = Option<i32> == Opt<Scaler>::In。
        type Body = Chain<Opt<Inc>, Opt<Scaler>>;
        let mut st = <Body as PortCell>::State::default();
        // None -> 双方均恒等；Some(5) -> Inc(6) -> Scaler(12)
        assert_eq!(drive::<Body>(&mut st, Some(5)), Some(12));
        assert_eq!(drive::<Body>(&mut st, None), None);
    }

    #[test]
    fn recursive_cell_type_composes_with_t1() {
        // 代数（递归）schema 的 running 形态：用户自定义**递归** cell（内部可递归、
        // `step` 仍是全函数），并作为组合子参与既有组合（Chain）与编译期 T1 验证。
        // 结论（K）：递归/互递归图样无需新的核心组合子——由用户递归类型 + 既有组合子表达；
        // 无界的**生成性展开**（任意运行时计数）归 ∃/物理侧（见 runtime 的 drive_seq/泵）。
        struct Sum;
        impl PortCell for Sum {
            type In = Vec<i32>;
            type Out = i32;
            type State = ();
            fn step(_: &mut (), xs: Vec<i32>) -> i32 {
                fn rec(xs: &[i32], i: usize) -> i32 {
                    if i >= xs.len() { 0 } else { xs[i] + rec(xs, i + 1) }
                }
                rec(&xs, 0)
            }
        }
        // 递归 cell 仍是 PortCell：可进 Chain、可做 T1 布线验证。
        assert_wiring::<Sum, Scaler>();
        type S = CellChain<Sum, Scaler>;
        let mut st = <S as PortCell>::State::default();
        // Sum([1,2,3])=6 -> Scaler(×2)=12
        assert_eq!(drive::<S>(&mut st, vec![1, 2, 3]), 12);
    }

    #[test]
    fn merge_fans_in_multiple_sources() {
        // 两个计数器源合入一个加法接收者（fan-in 多对多）。
        // S1=Inc, S2=Scaler, DST=Accumulator-ish: 用 Counter 求和语义。
        struct Accum;
        impl PortCell for Accum {
            type In = i32;
            type Out = i32;
            type State = i32;
            #[inline(always)]
            fn step(s: &mut i32, x: i32) -> i32 {
                *s += x;
                *s
            }
        }
        // S1=Inc(5->6), S2=Scaler(5->10); Accum 累加: 6 -> 16
        let (mut s1, mut s2, mut sdst) = ((), (), 0i32);
        let out = Merge::<Inc, Scaler, Accum>::join(&mut s1, &mut s2, &mut sdst, 5, 5);
        assert_eq!(out, 16);
    }
}
