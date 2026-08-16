//! 静态执行路径的类型契约——编译期已知的拓扑，零成本。
//!
//! # 定位（反窄化规则下的归位）
//!
//! 本模块是**结构层 + 类型层契约**：它定义静态执行路径所需的契约——
//! [`StraightMachine`]（单端口裸载荷直传）与 [`Chain`]/[`Diamond`] 组合子
//! （递归表达串并联 DAG）。旧契约 [`Link`]/[`Split`]/[`Merge`]（枚举端口
//! 转换）保留给 `axiom-runtime` 的固定 N 便捷函数（`pipelineN`/`fanout2`/
//! `fanin2`）与动态路径内省；**新代码优先用组合子 + Straight 契约**。
//!
//! # 零成本（P0：消除端口标签税）
//!
//! 静态路径用裸载荷执行：`process_straight(state, i) -> o` 无端口枚举、
//! 无 `match`、无 `MachineContext`、无 `ProcessOutput` 分派。来源/去向由
//! 类型系统在编译期固定——物理执行零验证（来源/去向错误是业务逻辑错误，
//! 不是性能开销的正当理由）。对比动态路径（`Box<dyn Any>` 类型擦除，
//! 每跳堆分配 + downcast，~5x）。
//!
//! `StraightIn`/`StraightOut` 是**纯数据载荷**（P3：不要求 `HasPortInfo`/
//! 运行时内省）——单端口机器的端口标签从物理层剥离，抽象层（端口/拓扑/
//! 验证/可观测）保留在 `Machine` 契约中。
//!
//! # 拓扑覆盖
//!
//! | 拓扑 | 契约 | 执行函数（runtime）/ 组合子（core） |
//! |------|-------|---------------------|
//! | 线性 A→B→C | `StraightLink` | `Chain` + `pipeline_chain` |
//! | 任意深度线性链 | `StraightLink` | `Chain` + `pipeline_chain` |
//! | Fan-out A→(B,C) | `StraightSplit` | `Diamond`（臂可任意链） |
//! | Fan-in (A,B)→C | `StraightMerge` | `Diamond` |
//! | 菱形 A→(B,C)→D | `StraightSplit` + `StraightMerge` | `Diamond` |
//! | 串并联 DAG | Straight 契约递归 | `Chain` + `Diamond` 嵌套 |
//!
//! （旧枚举契约 `Link`/`Split`/`Merge` + `pipeline2`/`pipeline3`/`fanout2`/
//! `fanin2` 保留——见 `axiom-runtime::static_path` 的"固定 N 便捷函数"。）
//!
//! # 表达力边界（串并联 DAG，而非任意 DAG）
//!
//! `Chain`（串行）与 `Diamond`（分叉-汇合）构成一个递归代数，其生成
//! 的语言恰是**串并联 DAG**（series-parallel graphs）——串行组合与并行
//! 组合递归封闭。任何串并联拓扑（流水线、map-reduce、菱形网络、多级
//! 分叉-汇合树）都可用这两者的嵌套表达，且全单态化。
//!
//! 真正的**任意 DAG**（含非串并联的交叉边，如 K4 的传递归约）无法用
//! 这个代数表达：稳定 Rust 不能用 const 泛型描述"任意边表"并同时保持
//! 端口类型安全——边表 `(usize, usize)` 是值级信息，端点的端口类型是
//! 类型级信息，二者之间的映射需要 GAT / `generic_const_exprs`。这是类型
//! 系统的边界，不是实现缺陷；非串并联拓扑走动态路径（`Runtime`），与
//! 动态税同理（数学上不可避免）。
//!
//! # 安全性
//!
//! 静态路径仅接受 `FusedInline` 机器（`SingleOutput` 或 `TupleOutput`）。
//! `MultiOutput`（含 `YieldMulti`，运行时输出数量）在类型层被拒绝——
//! 静态路径处理编译期已知的输出数量，不能处理运行时决定的 fan-out。

use crate::machine::Machine;

use alloc::string::{String, ToString};

// ════════════════════════════════════════════════════════════════════════════
// Section 1: Link — 线性类型转换契约
// ════════════════════════════════════════════════════════════════════════════

/// 编译期已知的类型转换：`Src::Output → Option<Dst::Input>`。
///
/// 这是动态路径 `Wire { payload: Box<dyn Any> }` + `from_port_name` 的静态
/// 对应物。用户提供一个普通函数，将一个具体的输出端口枚举转换为下游的
/// 输入端口枚举。单态化并内联后，等价于手写的 match + rewrap。
///
/// 返回 `None` 表示"此输出不前往 Dst"（例如多端口机器中只有一个端口
/// 连接到 Dst）。静态路径丢弃 `None` 输出。
///
/// # 为什么在 core 而非 adapter
///
/// `Link` 是**类型层契约**：它描述两个具体端口枚举类型如何在类型层
/// 关联。原实现（axiom-tokio）注释说"If Link proves generally useful it
/// may be promoted to core later"——现在执行：它是静态路径的核心契约，
/// 归位到 core 使任何 runtime adapter 都可复用。
///
/// # 零成本
///
/// `extract` 是普通关联函数（无 `&self`），单态化后无 vtable、无间接调用。
/// 在 `--release` + `#[inline]` 下，编译器将其内联到调用点，stage 边界
/// 消融为纯计算。
///
/// # 示例
///
/// ```
/// use axiom::machine::Machine;
/// use axiom::static_exec::Link;
///
/// // 手动实现：将 DoublerOutput::y(n) 转为 TriplerInput::x(n)
/// struct DoublerToTripler;
/// impl<Src: Machine, Dst: Machine> Link<Src, Dst> for DoublerToTripler
/// where Src::Output: Into<Dst::Input>
/// {
///     fn extract(out: Src::Output) -> Option<Dst::Input> {
///         Some(out.into())
///     }
/// }
/// ```
pub trait Link<Src: Machine, Dst: Machine>: Send + 'static {
    /// 将 `Src::Output` 转换为 `Option<Dst::Input>`。
    ///
    /// 返回 `None` 表示此输出不前往 Dst（被丢弃）。
    fn extract(out: Src::Output) -> Option<Dst::Input>;
}

/// 恒等链接——当 `Src::Output` 可 `Into<Dst::Input>` 时的默认实现。
///
/// 适用于两个机器的端口类型相同（或可通过 `From`/`Into` 转换）的情况。
/// 这是最常见的链接方式：上游输出端口类型 == 下游输入端口类型。
pub struct IdLink;

impl<Src: Machine, Dst: Machine> Link<Src, Dst> for IdLink
where
    Src::Output: Into<Dst::Input>,
{
    #[inline]
    fn extract(out: Src::Output) -> Option<Dst::Input> {
        Some(out.into())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 2: Split — Fan-out 契约
// ════════════════════════════════════════════════════════════════════════════

/// Fan-out 契约：将一个输出拆分为两个下游输入。
///
/// 静态路径的 fan-out **不是**通过 `MultiOutput`（运行时数量）实现的——
/// 那是动态路径。静态 fan-out 通过 `Split` 在编译期将一个输出值拆分为
/// 两个（或通过链式拆分为 N 个），每个送入不同的下游机器。
///
/// # 为什么需要 Split 而非直接 Clone
///
/// 直接 `Clone` 是最简单的 fan-out（`CloneSplit`），但并非所有 fan-out
/// 都是等量复制：
/// - **等量复制**：Tee 语义，`CloneSplit` 适用
/// - **路由拆分**：按内容分发到不同下游（如偶数→A，奇数→B），需自定义 `Split`
/// - **解构拆分**：输出是元组 `(A, B)`，拆分为 `A` 和 `B` 分别送下游
///
/// `Split` trait 统一这三种情况，编译期单态化。
///
/// # N-way fan-out
///
/// `Split` 是 2-way（最常见）。N-way fan-out 通过链式 `Split` 实现：
/// `A → Split → (B, remainder) → Split → (C, remainder) → ...`
pub trait Split<T> {
    /// 拆分后的左侧输出类型（送往第一个下游）。
    type Left;
    /// 拆分后的右侧输出类型（送往第二个下游）。
    type Right;

    /// 将 `input` 拆分为 `(Left, Right)`。
    fn split(input: T) -> (Self::Left, Self::Right);
}

/// 等量复制拆分——通过 `Clone` 复制两份。
///
/// 适用于 Tee 语义：同一输出送入多个下游，每个下游收到完整副本。
/// 这是最常见的 fan-out 方式。
pub struct CloneSplit;

impl<T: Clone> Split<T> for CloneSplit {
    type Left = T;
    type Right = T;

    #[inline]
    fn split(input: T) -> (T, T) {
        (input.clone(), input.clone())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 3: Merge — Fan-in 契约
// ════════════════════════════════════════════════════════════════════════════

/// Fan-in 契约：将两个上游输出合并为一个下游输入。
///
/// 静态路径的 fan-in 将两条流汇聚到一台机器。与动态路径不同，静态
/// fan-in 在编译期就知道两个上游的类型，合并函数被单态化。
///
/// # 合并语义
///
/// `Merge` 不是简单的 `zip`——合并方式取决于具体场景：
/// - **交错合并**：按顺序交替（如 round-robin）
/// - **聚合合并**：两值合成一个（如 `a + b`）
/// - **选择合并**：按优先级选一个（如 `a.or(b)`）
///
/// `Merge` trait 让用户定义具体语义，编译期单态化。
///
/// # N-way fan-in
///
/// `Merge` 是 2-way。N-way fan-in 通过链式 `Merge` 实现：
/// `(A, B) → Merge → AB, C → Merge → ABC, ...`
pub trait Merge<A, B> {
    /// 合并后的输出类型（送往下游输入）。
    type Output;

    /// 将 `a` 和 `b` 合并为一个值。
    fn merge(a: A, b: B) -> Self::Output;
}

// ════════════════════════════════════════════════════════════════════════════
// Section 4: 错误类型
// ════════════════════════════════════════════════════════════════════════════

/// 静态执行路径的错误。
#[derive(Debug)]
pub enum StaticExecError {
    /// 机器 `init()` 失败。
    InitFailed { machine: &'static str, reason: String },
    /// 机器 `cleanup()` 失败。
    CleanupFailed { machine: &'static str, reason: String },
    /// 机器在执行中途返回 `Done`，提前终止。
    ///
    /// 这不是错误——但对于期望处理所有输入的批量执行，`Done` 意味着
    /// 机器无法继续消费剩余输入。调用者可选择忽略或处理。
    MachineDone { machine: &'static str, processed: usize },
}

impl core::fmt::Display for StaticExecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InitFailed { machine, reason } => {
                write!(f, "init failed for '{}': {}", machine, reason)
            }
            Self::CleanupFailed { machine, reason } => {
                write!(f, "cleanup failed for '{}': {}", machine, reason)
            }
            Self::MachineDone { machine, processed } => {
                write!(
                    f,
                    "machine '{}' returned Done after processing {} inputs",
                    machine, processed
                )
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for StaticExecError {}

// ════════════════════════════════════════════════════════════════════════════
// Section 4.4: Straight — 裸载荷直传契约（消除端口标签税）
// ════════════════════════════════════════════════════════════════════════════
//
// 静态路径的零成本修复（P0）：编译期类型已固定"数据从哪来、到哪去"——
// 来源/去向的验证是业务逻辑错误（开发者责任），不是物理执行的开销。本
// 契约让单端口机器以**裸载荷**直传：无端口枚举、无 match、无标签检查。
// 多端口机器与动态路径保留枚举/内省（拓扑运行时才已知，标签必要）。

/// 单端口机器的免标签直传契约。
///
/// 静态路径（`Chain`/`Diamond`/`feedback`）要求机器实现此契约，用裸载荷
/// 执行：`process_straight(state, input) -> output` 无枚举包装/解包、无
/// `MachineContext`、无 `ProcessOutput` match。载荷类型 [`StraightIn`]/
/// [`StraightOut`] 是纯数据（不要求 `HasPortInfo`）——数据去向由类型系统
/// 在编译期固定，运行时零验证。
///
/// 与 `Machine` 的关系：`Machine` 的端口/拓扑/验证/可观测契约保留在抽象
/// 层（`process_straight` 之外）；`process_straight` 是物理层的直传通道。
/// 多端口机器（fan-out/多输入）走动态路径（`Runtime`），其标签是必要的。
pub trait StraightMachine: Machine {
    /// 单输入端口的载荷类型（去标签，纯数据）。
    type StraightIn: Send + 'static;
    /// 单输出端口的载荷类型（去标签，纯数据）。
    type StraightOut: Send + 'static;

    /// 裸载荷 process：无枚举包装/解包、无 ctx、无标签检查。
    ///
    /// 实现必须 `#[inline]`——这是跨 crate 融合（`StaticChain` 单态化）
    /// 的前提。
    fn process_straight(state: &mut Self::State, input: Self::StraightIn) -> Self::StraightOut;
}

/// 裸载荷链接：`fn(StraightOut) -> StraightIn`。
///
/// 编译期类型已固定"S 的输出必前往 D 的输入"，无枚举 match、无
/// `Option` 检查（对比 [`Link`] 的 `extract -> Option`）。
pub trait StraightLink<S: StraightMachine, D: StraightMachine> {
    /// 将 `S::StraightOut` 转换为 `D::StraightIn`。
    fn convert(out: S::StraightOut) -> D::StraightIn;
}

/// 恒等裸链接——当 `S::StraightOut` 可 `Into<D::StraightIn>` 时（通常
/// 两台机器载荷类型相同）。
pub struct StraightId;

impl<S: StraightMachine, D: StraightMachine> StraightLink<S, D> for StraightId
where
    S::StraightOut: Into<D::StraightIn>,
{
    #[inline]
    fn convert(out: S::StraightOut) -> D::StraightIn {
        out.into()
    }
}

/// 裸载荷分叉：`fn(T) -> (Left, Right)`。
///
/// 无枚举标签——按内容路由/复制是业务逻辑（分发），不是验证。
pub trait StraightSplit<T> {
    /// 左侧载荷类型（送往第一个下游）。
    type Left;
    /// 右侧载荷类型（送往第二个下游）。
    type Right;

    /// 将 `input` 拆分为 `(Left, Right)`。
    fn split(input: T) -> (Self::Left, Self::Right);
}

/// 复制分叉（Tee 语义）：同一载荷复制两份。
pub struct StraightClone;

impl<T: Clone> StraightSplit<T> for StraightClone {
    type Left = T;
    type Right = T;

    #[inline]
    fn split(input: T) -> (T, T) {
        (input.clone(), input.clone())
    }
}

/// 裸载荷汇合：`fn(A, B) -> Output`。
pub trait StraightMerge<A, B> {
    /// 合并后的载荷类型。
    type Output;

    /// 将 `a` 和 `b` 合并为一个载荷。
    fn merge(a: A, b: B) -> Self::Output;
}

// ════════════════════════════════════════════════════════════════════════════
// Section 4.5: Chain — 编译期任意深度线性链
// ════════════════════════════════════════════════════════════════════════════

use crate::port::MachineContext;
use alloc::vec::Vec;
use core::marker::PhantomData;

/// 编译期线性链组合子：`Chain<A, B>` 表达 `A → B`，可嵌套任意深度。
///
/// `Chain<A, Chain<B, C>>` 即 3 阶段流水线。链的深度由类型嵌套决定，
/// 由 [`StaticChain`] 在编译期递归展开——无需为每个 N 手写 `pipelineN`。
///
/// # 为什么不是 const 泛型 DAG
///
/// 稳定 Rust 无法用 const 泛型表达"任意边表"并保持端口类型安全：边表
/// `(usize, usize)` 是值级信息，而端点的端口类型是类型级信息——两者之间
/// 的映射需要 GAT / `generic_const_exprs`。`Chain` 用递归嵌套类型达到同一
/// 目标（任意深度链），fan-out/fan-in 由 `Split`/`Merge` 组合，构成任意
/// DAG 的编译期表达。
///
/// # 零成本
///
/// `StaticChain::run_all` 全单态化：每级 `process` 与 `Link::extract` 都是
/// 具体函数，`--release` + `#[inline]` 下融合为单一循环。无 `Box<dyn Any>`、
/// 无 trait dispatch。与 `pipeline2`/`pipeline3` 相同的保证，深度任意。
pub struct Chain<Head, Tail, L> {
    _marker: PhantomData<(Head, Tail, L)>,
}

impl<Head, Tail, L> Chain<Head, Tail, L> {
    /// 构造一个链类型值（纯类型标记，无运行时表示）。
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<Head, Tail, L> Default for Chain<Head, Tail, L> {
    fn default() -> Self {
        Self::new()
    }
}

/// 编译期线性链的递归执行契约。
///
/// - 单机器 `M: StraightMachine` 自动实现（基例）。
/// - `Chain<Head, Tail>` 实现为"跑 Head → `StraightLink::convert` 转换 →
///   递归 `Tail::run_all`"（递归步），编译期展开到任意深度。
///
/// `run_all` 消费全部输入并返回最终输出。载荷是**裸数据**（`StraightIn`/
/// `StraightOut`），无端口枚举、无标签检查——来源/去向由类型系统在编译期
/// 固定，物理执行零验证（P0：消除端口标签税）。
pub trait StaticChain: Sized {
    /// 链首机器类型（递归步需要：`StraightLink<Prev, Self::Head>`）。
    type Head: StraightMachine;
    /// 链尾裸输出类型。
    type Output: Send + 'static;

    /// 一次性执行整个链。输入类型即 `Self::Head::StraightIn`。
    fn run_all(
        inputs: Vec<<Self::Head as StraightMachine>::StraightIn>,
    ) -> Result<Vec<Self::Output>, StaticExecError>;
}

// 基例：单机器（任何 StraightMachine 机器都是一条单级链）。
impl<M> StaticChain for M
where
    M: StraightMachine,
{
    type Head = M;
    type Output = M::StraightOut;

    fn run_all(inputs: Vec<M::StraightIn>) -> Result<Vec<M::StraightOut>, StaticExecError> {
        let ctx = MachineContext::new(M::name());
        let mut state = M::init(&ctx).map_err(|e| StaticExecError::InitFailed {
            machine: M::name(),
            reason: e.to_string(),
        })?;
        // 预分配（P1）：输入长度已知，避免逐输入 realloc。
        let mut outputs = Vec::with_capacity(inputs.len());
        for input in inputs {
            // 裸载荷直传：无枚举 match、无 ctx、无 ProcessOutput 分派。
            outputs.push(M::process_straight(&mut state, input));
        }
        M::cleanup(state, &ctx).map_err(|e| StaticExecError::CleanupFailed {
            machine: M::name(),
            reason: e.to_string(),
        })?;
        Ok(outputs)
    }
}

// 递归步：Head → Tail。
impl<Head, Tail, L> StaticChain for Chain<Head, Tail, L>
where
    Head: StraightMachine,
    Tail: StaticChain,
    L: StraightLink<Head, Tail::Head>,
{
    type Head = Head;
    type Output = Tail::Output;

    fn run_all(inputs: Vec<Head::StraightIn>) -> Result<Vec<Tail::Output>, StaticExecError> {
        let ctx = MachineContext::new(Head::name());
        let mut state = Head::init(&ctx).map_err(|e| StaticExecError::InitFailed {
            machine: Head::name(),
            reason: e.to_string(),
        })?;
        let mut head_out = Vec::with_capacity(inputs.len());
        for input in inputs {
            head_out.push(Head::process_straight(&mut state, input));
        }
        Head::cleanup(state, &ctx).map_err(|e| StaticExecError::CleanupFailed {
            machine: Head::name(),
            reason: e.to_string(),
        })?;

        // 经 StraightLink 裸转换（无枚举 match、无 Option 检查），递归执行。
        let tail_inputs: Vec<<Tail::Head as StraightMachine>::StraightIn> = head_out
            .into_iter()
            .map(L::convert)
            .collect();
        Tail::run_all(tail_inputs)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 4.6: Diamond — 编译期菱形组合子（分叉 → 两路 → 汇合）
// ════════════════════════════════════════════════════════════════════════════

/// 编译期菱形组合子：`A → Split → (Left, Right) → Merge → Down`。
///
/// 这是静态路径从"线性 + 独立分叉/汇合"迈向"任意 DAG"的核心积木：一个
/// 上游 `A` 经 [`Split`] 分叉为两条**任意深度的链**（[`StaticChain`]），
/// 再经 [`Merge`] zip 配对汇合为一条下游链。左右臂与下游都可是单机器
/// （`FusedInline` 自动实现 `StaticChain`），也可是任意嵌套的 [`Chain`]。
///
/// 菱形是 fan-out + fan-in 的最小完整组合。此前这一形状需要用户手动
/// 衔接 `fanout2`（产出 `Vec<Output>`）与 `fanin2`（接受 `Vec<Input>`），
/// 两端的类型不匹配使衔接成为摩擦点；`Diamond` 在编译期把上游 + 两臂 +
/// 下游的拓扑一次展开，消除了中间衔接的类型摩擦。
///
/// # 组合性
///
/// `Diamond` 实现 [`StaticChain`]，因此与单机器同级：可作为 [`Chain`]
/// 的一节嵌入任意深度的链——
///
/// ```text
/// Chain<X, Diamond<A, Left, Right, Down, S, LB, LC, M>, LX>   // X → 菱形
/// Chain<Diamond<A, Left, Right, Down, S, LB, LC, M>, Y, LD>   // 菱形 → Y
/// ```
///
/// 而菱形的臂本身又可以是 `Chain`（甚至是另一个 `Diamond`），因此
/// "分叉 → 两路链 → 汇合 → 下游链"可以递归嵌套，逼近任意 DAG。
///
/// # 零成本
///
/// `run_all` 全单态化：`A::process_straight`、臂内各机器
/// `process_straight`、`S::split`、`LB/LC::convert`、`M::merge` 都是具体
/// 裸函数，`--release` + `#[inline]` 下融合为单一循环。无 `Box<dyn Any>`、
/// 无 trait dispatch、无端口枚举标签（P0）。
///
/// # 类型参数
///
/// - `A` 上游（单机器 `StraightMachine`），`Left`/`Right` 两条臂
///   （`StaticChain`），`Down` 下游（`StaticChain`）
/// - `S: StraightSplit<A::StraightOut, Left = A::StraightOut, Right = A::StraightOut>`：
///   分叉（裸载荷，无枚举标签）
/// - `LB: StraightLink<A, Left::Head>`、`LC: StraightLink<A, Right::Head>`：
///   分叉后的裸载荷转换（目标分别是两臂的首机器）
/// - `M: StraightMerge<Left::Output, Right::Output, Output = Down::Head::StraightIn>`：
///   汇合（两臂尾裸输出 zip 配对，合并为下游首机器裸输入）
pub struct Diamond<A, Left, Right, Down, S, LB, LC, M> {
    _marker: PhantomData<(A, Left, Right, Down, S, LB, LC, M)>,
}

impl<A, Left, Right, Down, S, LB, LC, M> Diamond<A, Left, Right, Down, S, LB, LC, M> {
    /// 构造一个菱形类型值（纯类型标记，无运行时表示）。
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<A, Left, Right, Down, S, LB, LC, M> Default for Diamond<A, Left, Right, Down, S, LB, LC, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A, Left, Right, Down, S, LB, LC, M> StaticChain for Diamond<A, Left, Right, Down, S, LB, LC, M>
where
    A: StraightMachine,
    Left: StaticChain,
    Right: StaticChain,
    Down: StaticChain,
    S: StraightSplit<A::StraightOut, Left = A::StraightOut, Right = A::StraightOut>,
    LB: StraightLink<A, Left::Head>,
    LC: StraightLink<A, Right::Head>,
    M: StraightMerge<
        Left::Output,
        Right::Output,
        Output = <Down::Head as StraightMachine>::StraightIn,
    >,
{
    type Head = A;
    type Output = Down::Output;

    fn run_all(inputs: Vec<A::StraightIn>) -> Result<Vec<Down::Output>, StaticExecError> {
        let ctx_a = MachineContext::new(A::name());
        let mut state_a = A::init(&ctx_a).map_err(|e| StaticExecError::InitFailed {
            machine: A::name(),
            reason: e.to_string(),
        })?;

        // Stage A：上游（裸载荷直传）。
        let mut a_outputs = Vec::with_capacity(inputs.len());
        for input in inputs {
            a_outputs.push(A::process_straight(&mut state_a, input));
        }

        // Split + Link：分叉后裸转换为两臂首机器的输入。
        let mut left_inputs: Vec<<Left::Head as StraightMachine>::StraightIn> =
            Vec::with_capacity(a_outputs.len());
        let mut right_inputs: Vec<<Right::Head as StraightMachine>::StraightIn> =
            Vec::with_capacity(a_outputs.len());
        for o in a_outputs {
            let (left, right) = S::split(o);
            left_inputs.push(LB::convert(left));
            right_inputs.push(LC::convert(right));
        }

        // 左右臂：各自是任意深度的 `StaticChain`，独立执行（其机器的
        // init/cleanup 由各自的 `run_all` 保证）。
        let left_result = Left::run_all(left_inputs);
        let right_result = Right::run_all(right_inputs);

        // A 已完成使命，无论左右臂结果如何都 cleanup。
        A::cleanup(state_a, &ctx_a).map_err(|e| StaticExecError::CleanupFailed {
            machine: A::name(),
            reason: e.to_string(),
        })?;

        let left_outputs = left_result?;
        let right_outputs = right_result?;

        // Merge：两臂尾裸输出 zip 配对后合并为下游首机器裸输入。
        let down_inputs: Vec<<Down::Head as StraightMachine>::StraightIn> = left_outputs
            .into_iter()
            .zip(right_outputs.into_iter())
            .map(|(l, r)| M::merge(l, r))
            .collect();

        // 下游链。
        let down_outputs = Down::run_all(down_inputs)?;

        Ok(down_outputs)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Section 5: 单元测试
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declare_ports;
    use crate::machine::{CleanupError, InitError, Machine, SingleOutput};
    use crate::port::MachineContext;

    // ── 测试机器（Machine 枚举契约 + StraightMachine 裸载荷契约）────────

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct DoublerPorts {
            input type DoublerInput {
                x[Data] => i32,
            }
            output type DoublerOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct Doubler;
    impl Machine for Doubler {
        type State = ();
        type Input = DoublerInput;
        type Output = DoublerOutput;
        type Ports = DoublerPorts;
        type ProcessOutput = SingleOutput<DoublerOutput>;
        fn name() -> &'static str { "doubler" }
        fn config_schema() -> crate::port::ConfigSchema { crate::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
        fn process(_: &mut (), _: &MachineContext, input: DoublerInput) -> SingleOutput<DoublerOutput> {
            match input {
                DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
            }
        }
        fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl StraightMachine for Doubler {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n * 2 }
    }

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct AdderPorts {
            input type AdderInput {
                x[Data] => i32,
            }
            output type AdderOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct Adder;
    impl Machine for Adder {
        type State = i32;
        type Input = AdderInput;
        type Output = AdderOutput;
        type Ports = AdderPorts;
        type ProcessOutput = SingleOutput<AdderOutput>;
        fn name() -> &'static str { "adder" }
        fn config_schema() -> crate::port::ConfigSchema { crate::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<i32, InitError> { Ok(0) }
        fn process(state: &mut i32, _: &MachineContext, input: AdderInput) -> SingleOutput<AdderOutput> {
            match input {
                AdderInput::x(n) => {
                    *state += n;
                    SingleOutput::Yield(AdderOutput::y(*state))
                }
            }
        }
        fn cleanup(_: i32, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl StraightMachine for Adder {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(state: &mut i32, n: i32) -> i32 {
            *state += n;
            *state
        }
    }

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct TriplerPorts {
            input type TriplerInput {
                x[Data] => i32,
            }
            output type TriplerOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct Tripler;
    impl Machine for Tripler {
        type State = ();
        type Input = TriplerInput;
        type Output = TriplerOutput;
        type Ports = TriplerPorts;
        type ProcessOutput = SingleOutput<TriplerOutput>;
        fn name() -> &'static str { "tripler" }
        fn config_schema() -> crate::port::ConfigSchema { crate::port::ConfigSchema::new() }
        fn init(_: &MachineContext) -> Result<(), InitError> { Ok(()) }
        fn process(_: &mut (), _: &MachineContext, input: TriplerInput) -> SingleOutput<TriplerOutput> {
            match input {
                TriplerInput::x(n) => SingleOutput::Yield(TriplerOutput::y(n * 3)),
            }
        }
        fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl StraightMachine for Tripler {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n * 3 }
    }

    // ── 裸载荷汇合（StraightMerge）──────────────────────────────────────

    struct Sum;
    impl StraightMerge<i32, i32> for Sum {
        type Output = i32;
        #[inline]
        fn merge(a: i32, b: i32) -> i32 { a + b }
    }

    // ══ Straight 契约单元测试 ═══════════════════════════════════════════

    #[test]
    fn straight_machine_single() {
        // 单机器直传：裸载荷，无枚举。
        let outputs = Doubler::run_all(vec![1, 2, 3]).expect("doubler");
        assert_eq!(outputs, vec![2, 4, 6]);
    }

    #[test]
    fn straight_machine_empty() {
        let outputs = Doubler::run_all(vec![]).expect("empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn straight_id_convert() {
        // StraightId：载荷类型相同（i32 Into i32）时恒等转换。
        let x: i32 = <StraightId as StraightLink<Doubler, Adder>>::convert(7);
        assert_eq!(x, 7);
    }

    #[test]
    fn straight_clone_split_duplicates() {
        let (a, b) = StraightClone::split(42i32);
        assert_eq!(a, 42);
        assert_eq!(b, 42);
    }

    #[test]
    fn straight_merge_sums() {
        assert_eq!(Sum::merge(3, 4), 7);
    }

    // ══ StaticChain：Chain 测试 ═════════════════════════════════════════

    #[test]
    fn chain_three_stage_recursive() {
        // Doubler → Adder → Tripler（3 级递归链，StraightId 链接）
        // 输入 [1]: D(2) → A(2) → T(6)
        type Chain3 = Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>;
        let outputs = Chain3::run_all(vec![1]).expect("chain3");
        assert_eq!(outputs, vec![6]);
    }

    #[test]
    fn chain_recursive_multi_input() {
        // Doubler → Adder（Adder 跨输入累加）
        // 输入 [1,2,3]: D(2,4,6) → A(2,6,12)
        type Chain2 = Chain<Doubler, Adder, StraightId>;
        let outputs = Chain2::run_all(vec![1, 2, 3]).expect("chain2");
        assert_eq!(outputs, vec![2, 6, 12]);
    }

    #[test]
    fn chain_empty_inputs() {
        type Chain3 = Chain<Doubler, Chain<Adder, Tripler, StraightId>, StraightId>;
        let outputs = Chain3::run_all(vec![]).expect("chain3 empty");
        assert!(outputs.is_empty());
    }

    // ══ Diamond 测试 ════════════════════════════════════════════════════

    type DiamondShape = Diamond<
        Doubler,
        Adder,
        Tripler,
        Adder,
        StraightClone,
        StraightId,
        StraightId,
        Sum,
    >;

    #[test]
    fn diamond_runs_split_then_merge() {
        // Doubler → StraightClone → (Adder, Tripler) → Sum → Adder
        // 输入 [1, 2]: D(2,4) → split(2,2),(4,4) → A(2,6), T(6,12) → Sum(8,18) → A(8,26)
        let outputs = DiamondShape::run_all(vec![1, 2]).expect("diamond");
        assert_eq!(outputs, vec![8, 26]);
    }

    #[test]
    fn diamond_empty_inputs() {
        let outputs = DiamondShape::run_all(vec![]).expect("diamond empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn diamond_embeds_as_chain_tail() {
        // Diamond 实现 StaticChain，可作为 Chain 的 Tail 嵌入。
        // Chain<Doubler, DiamondShape, StraightId>：外层 Doubler → 菱形
        type ChainWithDiamond = Chain<Doubler, DiamondShape, StraightId>;
        // 输入 [1]: 外层 D(2) → 菱形 D(4) → split(4,4) → A(4), T(12) → Sum(16) → A(16)
        let outputs = ChainWithDiamond::run_all(vec![1]).expect("chain+diamond");
        assert_eq!(outputs, vec![16]);
    }

    #[test]
    fn diamond_arms_are_chains() {
        // 菱形两臂是任意深度链（各 2 级）：左臂 Adder→Doubler，右臂 Tripler→Doubler。
        type LeftArm = Chain<Adder, Doubler, StraightId>;
        type RightArm = Chain<Tripler, Doubler, StraightId>;
        type DChainArms = Diamond<
            Doubler,
            LeftArm,
            RightArm,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >;
        // 输入 [1]: D(2) → split(2,2)
        //   左臂 A→D: 2 → A(2) → D(4)
        //   右臂 T→D: 2 → T(6) → D(12)
        //   Sum(16) → 下游 A(16)
        let outputs = DChainArms::run_all(vec![1]).expect("diamond chain arms");
        assert_eq!(outputs, vec![16]);
    }

    #[test]
    fn diamond_arm_is_diamond() {
        // 菱形套菱形：外层的左臂本身是一个完整菱形——递归完备性。
        type InnerDiamond = Diamond<
            Doubler,
            Adder,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >;
        type OuterDiamond = Diamond<
            Doubler,
            InnerDiamond,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >;
        // 输入 [1]: 外层 D(2) → split(2,2)
        //   左臂 InnerDiamond(2): D(4) → split(4,4) → A(4), T(12) → Sum(16) → A(16)
        //   右臂 Tripler(2): 6
        //   Sum(16+6=22) → 下游 A(22)
        let outputs = OuterDiamond::run_all(vec![1]).expect("diamond in diamond");
        assert_eq!(outputs, vec![22]);
    }

    // ══ 旧契约测试（Link/Split/Merge——保留给动态路径便捷函数）══════════

    #[test]
    fn link_extract_converts_output_to_input() {
        // 旧 Link：多端口机器的枚举转换（动态路径便捷函数用）。
        struct OldDToA;
        impl Link<Doubler, Adder> for OldDToA {
            fn extract(out: DoublerOutput) -> Option<AdderInput> {
                match out {
                    DoublerOutput::y(n) => Some(AdderInput::x(n)),
                }
            }
        }
        let out = DoublerOutput::y(42);
        let input = OldDToA::extract(out).expect("should convert");
        match input {
            AdderInput::x(n) => assert_eq!(n, 42),
        }
    }

    #[test]
    fn clone_split_duplicates_value() {
        let (a, b) = CloneSplit::split(42i32);
        assert_eq!(a, 42);
        assert_eq!(b, 42);
    }

    #[test]
    fn merge_combines_two_values() {
        struct SumMerge;
        impl Merge<i32, i32> for SumMerge {
            type Output = i32;
            fn merge(a: i32, b: i32) -> i32 { a + b }
        }
        assert_eq!(SumMerge::merge(3, 4), 7);
    }

    #[test]
    fn static_exec_error_display() {
        let e = StaticExecError::InitFailed {
            machine: "doubler",
            reason: "out of memory".into(),
        };
        let s = alloc::format!("{e}");
        assert!(s.contains("doubler"));
        assert!(s.contains("out of memory"));

        let e = StaticExecError::MachineDone {
            machine: "source",
            processed: 100,
        };
        let s = alloc::format!("{e}");
        assert!(s.contains("source"));
        assert!(s.contains("100"));
    }
}
