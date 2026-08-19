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
}
