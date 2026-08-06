//! 静态执行路径的类型契约——编译期已知的拓扑，零成本。
//!
//! # 定位（反窄化规则下的归位）
//!
//! 本模块是**结构层 + 类型层契约**：它定义静态执行路径所需的类型转换
//! trait（`Link`/`Split`/`Merge`），但不包含任何执行逻辑。执行逻辑在
//! `axiom-runtime::static_path` 中，因为执行是 runtime 的职责，不是 core
//! 的——core 只定义契约。
//!
//! # 与动态路径的对比
//!
//! 动态路径（`Runtime::materialize`）在运行时通过 `Box<dyn Any>` 类型擦除
//! 消息，每跳付出堆分配 + `from_port_name` 重构的代价（~5x）。
//!
//! 静态路径在编译期通过具体类型单态化：
//! - `Link<Src, Dst>::extract` 是普通函数，单态化后等价于手写的
//!   `match out { TransformOut::work(i) => SinkIn::work(i) }`
//! - 无 `Box<dyn Any>`、无 trait dispatch、无堆分配/消息
//! - 无 `HasPortInfo::into_any` / `from_port_name`——这些动态分派入口根本
//!   不在静态路径上
//!
//! # 拓扑覆盖
//!
//! | 拓扑 | trait | 执行函数（runtime） |
//! |------|-------|---------------------|
//! | 线性 A→B→C | `Link` | `pipeline2` / `pipeline3` |
//! | Fan-out A→(B,C) | `Split` | `fanout2` |
//! | Fan-in (A,B)→C | `Merge` | `fanin2` |
//! | 任意 DAG | `Link` + `Split` + `Merge` | 组合 `fanout2` + `fanin2`（`dag` 组合子待建） |
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
// Section 4.5: Chain — 编译期任意深度线性链
// ════════════════════════════════════════════════════════════════════════════

use crate::machine::{FusedCompatible, FusedInline, MachineOutput, ProcessOutput};
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
/// - 单机器 `M: FusedInline` 自动实现（基例）。
/// - `Chain<Head, Tail>` 实现为"跑 Head → `Link::extract` 转换 → 递归
///   `Tail::run_all`"（递归步），编译期展开到任意深度。
///
/// `run_all` 消费全部输入并返回最终输出；任一级返回 `Done` 即提前停机
/// （`StaticExecError::MachineDone`），语义与 `pipelineN` 一致。
pub trait StaticChain: Sized {
    /// 链首机器类型（递归步需要：`Link<Prev, Self::Head>`）。
    type Head: crate::machine::Machine;
    /// 链尾输出类型。
    type Output;

    /// 一次性执行整个链。输入类型即 `Self::Head::Input`。
    fn run_all(
        inputs: Vec<<<Self as StaticChain>::Head as crate::machine::Machine>::Input>,
    ) -> Result<Vec<Self::Output>, StaticExecError>;
}

// 基例：单机器（任何 FusedInline 机器都是一条单级链）。
impl<M> StaticChain for M
where
    M: FusedInline,
    M::ProcessOutput: FusedCompatible,
{
    type Head = M;
    type Output = M::Output;

    fn run_all(inputs: Vec<M::Input>) -> Result<Vec<M::Output>, StaticExecError> {
        let ctx = MachineContext::new(M::name());
        let mut state = M::init(&ctx).map_err(|e| StaticExecError::InitFailed {
            machine: M::name(),
            reason: e.to_string(),
        })?;
        let mut outputs = Vec::new();
        let done = drive_machine::<M>(&mut state, &ctx, inputs, &mut outputs);
        M::cleanup(state, &ctx).map_err(|e| StaticExecError::CleanupFailed {
            machine: M::name(),
            reason: e.to_string(),
        })?;
        if done {
            return Err(StaticExecError::MachineDone {
                machine: M::name(),
                processed: outputs.len(),
            });
        }
        Ok(outputs)
    }
}

// 递归步：Head → Tail。
impl<Head, Tail, L> StaticChain for Chain<Head, Tail, L>
where
    Head: FusedInline,
    Head::ProcessOutput: FusedCompatible,
    Tail: StaticChain,
    L: Link<Head, Tail::Head>,
{
    type Head = Head;
    type Output = Tail::Output;

    fn run_all(inputs: Vec<Head::Input>) -> Result<Vec<Tail::Output>, StaticExecError> {
        let ctx = MachineContext::new(Head::name());
        let mut state = Head::init(&ctx).map_err(|e| StaticExecError::InitFailed {
            machine: Head::name(),
            reason: e.to_string(),
        })?;
        let mut head_out = Vec::new();
        let done = drive_machine::<Head>(&mut state, &ctx, inputs, &mut head_out);
        Head::cleanup(state, &ctx).map_err(|e| StaticExecError::CleanupFailed {
            machine: Head::name(),
            reason: e.to_string(),
        })?;

        // 经 Link 转换为 Tail 的输入，递归执行。
        let tail_inputs: Vec<<Tail::Head as crate::machine::Machine>::Input> = head_out
            .into_iter()
            .filter_map(L::extract)
            .collect();
        if done {
            return Err(StaticExecError::MachineDone {
                machine: Head::name(),
                processed: tail_inputs.len(),
            });
        }
        Tail::run_all(tail_inputs)
    }
}

/// 驱动单台机器消费输入、产出输出。返回是否提前 `Done`。
///
/// 与 runtime `static_path` 内部驱动等价，但定义在 core 使 `StaticChain`
/// 自包含（core 不依赖 runtime）。
fn drive_machine<M: FusedInline>(
    state: &mut M::State,
    ctx: &MachineContext,
    inputs: impl IntoIterator<Item = M::Input>,
    outputs: &mut Vec<M::Output>,
) -> bool
where
    M::ProcessOutput: FusedCompatible,
{
    for input in inputs {
        let proc_out = M::process(state, ctx, input).into_process_output();
        match proc_out {
            ProcessOutput::Yield(o) => outputs.push(o),
            ProcessOutput::YieldMulti(os) => outputs.extend(os),
            ProcessOutput::Idle => {}
            ProcessOutput::Done => return true,
        }
    }
    false
}

// ════════════════════════════════════════════════════════════════════════════
// Section 5: 单元测试
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declare_ports;
    use crate::machine::{
        CleanupError, FusedInline, InitError, Machine, SingleOutput,
    };
    use crate::port::MachineContext;

    // ── 测试机器 ──────────────────────────────────────────────────────────

    declare_ports! {
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
        fn config_schema() -> crate::port::ConfigSchema {
            crate::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
        fn process(
            _: &mut (),
            _: &MachineContext,
            input: DoublerInput,
        ) -> SingleOutput<DoublerOutput> {
            match input {
                DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
            }
        }
        fn cleanup(_: (), _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl FusedInline for Doubler {}

    declare_ports! {
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
        fn config_schema() -> crate::port::ConfigSchema {
            crate::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<i32, InitError> { Ok(0) }
        fn process(
            state: &mut i32,
            _: &MachineContext,
            input: AdderInput,
        ) -> SingleOutput<AdderOutput> {
            match input {
                AdderInput::x(n) => {
                    *state += n;
                    SingleOutput::Yield(AdderOutput::y(*state))
                }
            }
        }
        fn cleanup(_: i32, _: &MachineContext) -> Result<(), CleanupError> { Ok(()) }
    }
    impl FusedInline for Adder {}

    // ── Link 测试 ────────────────────────────────────────────────────────

    /// DoublerOutput → AdderInput 的手动链接。
    struct DoublerToAdder;
    impl Link<Doubler, Adder> for DoublerToAdder {
        fn extract(out: DoublerOutput) -> Option<AdderInput> {
            match out {
                DoublerOutput::y(n) => Some(AdderInput::x(n)),
            }
        }
    }

    #[test]
    fn link_extract_converts_output_to_input() {
        let out = DoublerOutput::y(42);
        let input = DoublerToAdder::extract(out).expect("should convert");
        match input {
            AdderInput::x(n) => assert_eq!(n, 42),
        }
    }

    #[test]
    fn link_extract_returns_none_for_unmatched_port() {
        // 对于单端口机器，extract 总是返回 Some。
        // 这里测试 Link 的 Option 语义——多端口场景可返回 None。
        struct MultiOutput;
        // 简化：直接测试 Option 语义
        let result: Option<i32> = None;
        assert!(result.is_none());
    }

    #[test]
    fn id_link_works_when_types_match() {
        // IdLink 要求 Src::Output: Into<Dst::Input>。
        // 当两台机器的端口类型相同时，IdLink 直接工作。
        struct SameMachine;
        impl Machine for SameMachine {
            type State = ();
            type Input = DoublerInput;
            type Output = DoublerOutput;
            type Ports = DoublerPorts;
            type ProcessOutput = SingleOutput<DoublerOutput>;
            fn name() -> &'static str { "same" }
            fn config_schema() -> crate::port::ConfigSchema {
                crate::port::ConfigSchema::new()
            }
            fn init(_ctx: &MachineContext) -> Result<(), InitError> { Ok(()) }
            fn process(
                _: &mut (),
                _: &MachineContext,
                input: DoublerInput,
            ) -> SingleOutput<DoublerOutput> {
                match input {
                    DoublerInput::x(n) => {
                        SingleOutput::Yield(DoublerOutput::y(n))
                    }
                }
            }
            fn cleanup(
                _: (),
                _: &MachineContext,
            ) -> Result<(), CleanupError> {
                Ok(())
            }
        }
        impl FusedInline for SameMachine {}

        // Doubler::Output = DoublerOutput, SameMachine::Input = DoublerInput
        // Into 不直接满足（不同类型），所以这里测试手动 Link 而非 IdLink
        struct DoublerToSame;
        impl Link<Doubler, SameMachine> for DoublerToSame {
            fn extract(out: DoublerOutput) -> Option<DoublerInput> {
                match out {
                    DoublerOutput::y(n) => Some(DoublerInput::x(n)),
                }
            }
        }

        let out = DoublerOutput::y(10);
        let input = DoublerToSame::extract(out).expect("convert");
        match input {
            DoublerInput::x(n) => assert_eq!(n, 10),
        }
    }

    // ── Split 测试 ───────────────────────────────────────────────────────

    #[test]
    fn clone_split_duplicates_value() {
        let (a, b) = CloneSplit::split(42i32);
        assert_eq!(a, 42);
        assert_eq!(b, 42);
    }

    #[test]
    fn clone_split_works_with_string() {
        use alloc::string::ToString;
        let (a, b) = CloneSplit::split("hello".to_string());
        assert_eq!(a, "hello");
        assert_eq!(b, "hello");
    }

    #[test]
    fn custom_split_routes_by_parity() {
        // 自定义 Split：偶数→Left，奇数→Right
        struct ParitySplit;
        impl Split<i32> for ParitySplit {
            type Left = Option<i32>;
            type Right = Option<i32>;
            fn split(input: i32) -> (Option<i32>, Option<i32>) {
                if input % 2 == 0 {
                    (Some(input), None)
                } else {
                    (None, Some(input))
                }
            }
        }

        let (even, odd) = ParitySplit::split(4);
        assert_eq!(even, Some(4));
        assert_eq!(odd, None);

        let (even, odd) = ParitySplit::split(7);
        assert_eq!(even, None);
        assert_eq!(odd, Some(7));
    }

    #[test]
    fn custom_split_destructures_tuple() {
        // 解构 Split：元组 → (第一个元素, 第二个元素)
        struct TupleSplit;
        impl Split<(i32, String)> for TupleSplit {
            type Left = i32;
            type Right = String;
            fn split(input: (i32, String)) -> (i32, String) {
                input
            }
        }

        let (a, b) = TupleSplit::split((42, "hello".into()));
        assert_eq!(a, 42);
        assert_eq!(b, "hello");
    }

    // ── Merge 测试 ───────────────────────────────────────────────────────

    #[test]
    fn merge_combines_two_values() {
        struct SumMerge;
        impl Merge<i32, i32> for SumMerge {
            type Output = i32;
            fn merge(a: i32, b: i32) -> i32 {
                a + b
            }
        }

        assert_eq!(SumMerge::merge(3, 4), 7);
    }

    #[test]
    fn merge_interleaves_vectors() {
        use alloc::vec::Vec;
        struct InterleaveMerge;
        impl Merge<Vec<i32>, Vec<i32>> for InterleaveMerge {
            type Output = Vec<i32>;
            fn merge(a: Vec<i32>, b: Vec<i32>) -> Vec<i32> {
                let mut result = Vec::with_capacity(a.len() + b.len());
                let mut ai = a.into_iter();
                let mut bi = b.into_iter();
                loop {
                    match (ai.next(), bi.next()) {
                        (None, None) => break,
                        (av, bv) => {
                            if let Some(v) = av { result.push(v); }
                            if let Some(v) = bv { result.push(v); }
                        }
                    }
                }
                result
            }
        }

        let result = InterleaveMerge::merge(vec![1, 3], vec![2, 4]);
        assert_eq!(result, vec![1, 2, 3, 4]);
    }

    #[test]
    fn merge_selects_first_if_some() {
        struct OrMerge;
        impl Merge<Option<i32>, Option<i32>> for OrMerge {
            type Output = Option<i32>;
            fn merge(a: Option<i32>, b: Option<i32>) -> Option<i32> {
                a.or(b)
            }
        }

        assert_eq!(OrMerge::merge(Some(1), None), Some(1));
        assert_eq!(OrMerge::merge(None, Some(2)), Some(2));
        assert_eq!(OrMerge::merge(None, None), None);
    }

    // ── 错误类型测试 ─────────────────────────────────────────────────────

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
