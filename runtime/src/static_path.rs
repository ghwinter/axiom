//! 静态执行路径——编译期已知拓扑的零成本批量执行。
//!
//! # 定位
//!
//! 本模块是 `axiom::static_exec`（core 类型契约）的执行端。它提供具体
//! 函数，将 `FusedInline` 机器按编译期已知的拓扑驱动执行，全程单态化、
//! 无 `Box<dyn Any>`、无 trait dispatch、无堆分配/消息。
//!
//! # 执行模型：同步批量拓扑序
//!
//! 与动态路径（`Runtime::materialize` → `tick` 的逐消息驱动）不同，静态
//! 路径采用**同步批量**模型：
//!
//! 1. 输入 `Vec<I>` → 按拓扑序逐机器执行
//! 2. 每台机器的输出收集为 `Vec<Output>`
//! 3. Fan-out 时通过 `Split` 拆分
//! 4. Fan-in 时通过 `Merge` 合并
//! 5. 最终输出 `Vec<O>`
//!
//! 这是原线性探针（已删除的 `LinearRuntime::pipeline3`）到任意 DAG 的
//! 自然推广——从 `inputs.iter().map(|x| c(b(a(x)))).collect()` 扩展到
//! 支持分支与汇聚。
//!
//! # 与动态路径的对比
//!
//! | 维度 | 静态路径（本模块） | 动态路径（`Runtime`） |
//! |------|---------------------|----------------------|
//! | 拓扑确定时机 | 编译期 | 运行时（`DeploySpec`） |
//! | 类型擦除 | 无（具体类型单态化） | `Box<dyn Any>` |
//! | 每消息成本 | 零（无堆分配） | ~5x（堆分配 + dispatch） |
//! | 拓扑能力 | 串并联 DAG + 单机器反馈环 | 任意 DAG + 环 |
//! | IO/异步 | 不支持 | 支持（`IoReactor`） |
//! | 适用场景 | 固定管道、热路径 | 配置驱动、插件、动态拓扑 |
//!
//! # 固定 N 便捷函数 vs 组合子
//!
//! 本模块同时提供两类 API：
//!
//! - **组合子**（首选）：`Chain`（任意深度线性链）、`Diamond`（分叉-汇合，
//!   臂与下游可为任意链）、`feedback`（单机器反馈环）。它们递归组合，
//!   表达串并联 DAG 与反馈环，是静态路径的表达力主体。
//! - **固定 N 便捷函数**（`pipeline2`/`pipeline3`/`fanout2`/`fanin2`）：
//!   数字后缀表阶段数（`pipelineN`）或路数（`fanout`/`fanin` 为 2 路）。
//!   它们是 `Chain`/`Diamond` 的特例别名——`pipeline2::<A, B, L>` 等价于
//!   `Chain<A, B, L>`，`fanout2`/`fanin2` 是 `Diamond` 的拆开形态。新代码
//!   优先用组合子；便捷函数保留以服务固定形状的紧凑写法。
//!
//! # 安全性
//!
//! 所有函数要求 `FusedInline` 机器——`SingleOutput` 或 `TupleOutput`，
//! 类型层排除 `YieldMulti`（运行时输出数量）。静态路径处理编译期已知
//! 的输出数量，不能处理运行时决定的 fan-out。

use axiom::machine::{
    CleanupError, FusedCompatible, FusedInline, InitError, MachineOutput, ProcessOutput,
};
use axiom::port::MachineContext;
use axiom::static_exec::{
    Diamond, Link, Merge, Split, StaticChain, StaticExecError,
    StraightLink, StraightMachine, StraightMerge, StraightSplit,
};

use alloc::format;
use alloc::vec::Vec;

// ── 辅助：错误转换 ───────────────────────────────────────────────────────────

fn init_err(machine: &'static str, e: InitError) -> StaticExecError {
    StaticExecError::InitFailed {
        machine,
        reason: format!("{e}"),
    }
}

fn cleanup_err(machine: &'static str, e: CleanupError) -> StaticExecError {
    StaticExecError::CleanupFailed {
        machine,
        reason: format!("{e}"),
    }
}

// ── 辅助：单机器批量执行 ─────────────────────────────────────────────────────

/// 在 `state` 上逐条处理 `inputs`，收集所有 `Yield` 输出。
///
/// 返回 `(outputs, done)`：`done = true` 表示机器中途返回 `Done`。
///
/// 零成本：对 `Yield`（最常见路径）无 Vec 分配——直接 push 到外部
/// `outputs`。`YieldMulti` 的 Vec 来自机器本身（非本函数分配）。
#[inline]
fn run_machine<M: FusedInline>(
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
// 线性流水线
// ════════════════════════════════════════════════════════════════════════════

/// 2 阶段线性流水线：`A → B`。
///
/// 输入 `A::Input` 的迭代器，A 的输出经 `L::extract` 转换为 B 的输入，
/// B 的输出收集返回。
///
/// # 零成本
///
/// 全程单态化：`A::process`、`L::extract`、`B::process` 都是具体类型的
/// 具体函数，在 `--release` + `#[inline]` 下编译器可融合为单一循环。
/// 无 `Box<dyn Any>`、无 trait dispatch、无中间 Vec（除最终输出）。
///
/// # 示例
///
/// ```ignore
/// use axiom_runtime::static_path::pipeline2;
///
/// let outputs = pipeline2::<Doubler, Adder, DoublerToAdder>(
///     vec![DoublerInput::x(1), DoublerInput::x(2)],
/// )?;
/// ```
pub fn pipeline2<A, B, L>(
    inputs: impl IntoIterator<Item = A::Input>,
) -> Result<Vec<B::Output>, StaticExecError>
where
    A: FusedInline,
    B: FusedInline,
    L: Link<A, B>,
    A::ProcessOutput: FusedCompatible,
    B::ProcessOutput: FusedCompatible,
{
    let ctx_a = MachineContext::new(A::name());
    let ctx_b = MachineContext::new(B::name());

    let mut state_a = A::init(&ctx_a).map_err(|e| init_err(A::name(), e))?;
    let mut state_b = B::init(&ctx_b).map_err(|e| init_err(B::name(), e))?;

    let mut a_outputs = Vec::new();
    let a_done = run_machine::<A>(&mut state_a, &ctx_a, inputs, &mut a_outputs);

    // 经 Link 转换为 B 的输入
    let b_inputs: Vec<B::Input> = a_outputs
        .into_iter()
        .filter_map(|o| L::extract(o))
        .collect();

    let mut b_outputs = Vec::new();
    let _b_done = run_machine::<B>(&mut state_b, &ctx_b, b_inputs, &mut b_outputs);

    A::cleanup(state_a, &ctx_a).map_err(|e| cleanup_err(A::name(), e))?;
    B::cleanup(state_b, &ctx_b).map_err(|e| cleanup_err(B::name(), e))?;

    // 如果 A 提前 Done，报告已处理数量
    if a_done {
        return Err(StaticExecError::MachineDone {
            machine: A::name(),
            processed: b_outputs.len(),
        });
    }

    Ok(b_outputs)
}

/// 任意深度线性流水线（编译期递归链）。
///
/// 与 `pipeline2`/`pipeline3` 语义一致，但深度由类型决定而非手写函数：
/// 用 `Chain` 组合子嵌套即可得到任意 N 阶段链（roadmap C1）：
///
/// ```ignore
/// use axiom::static_exec::Chain;
/// use axiom_runtime::static_path::pipeline_chain;
///
/// // 4 阶段链：Doubler → Tripler → Adder → Negater
/// type MyChain = Chain<Doubler, Chain<Tripler, Chain<Adder, Negater>>, DToT>;
/// let outputs = pipeline_chain::<MyChain>(vec![/* inputs */])?;
/// ```
///
/// 由 [`StaticChain`] 在编译期递归展开——无 `Box<dyn Any>`、无 trait
/// dispatch，与固定 `pipelineN` 相同的零成本保证。
pub fn pipeline_chain<C: axiom::static_exec::StaticChain>(
    inputs: Vec<
        <<C as axiom::static_exec::FlowThrough>::Head as axiom::static_exec::StraightMachine>::StraightIn,
    >,
) -> Result<Vec<C::Out>, StaticExecError> {
    C::run_all(inputs)
}

/// 3 阶段线性流水线：`A → B → C`。
///
/// A 的输出经 `L1` 转换为 B 的输入，B 的输出经 `L2` 转换为 C 的输入。
pub fn pipeline3<A, B, C, L1, L2>(
    inputs: impl IntoIterator<Item = A::Input>,
) -> Result<Vec<C::Output>, StaticExecError>
where
    A: FusedInline,
    B: FusedInline,
    C: FusedInline,
    L1: Link<A, B>,
    L2: Link<B, C>,
    A::ProcessOutput: FusedCompatible,
    B::ProcessOutput: FusedCompatible,
    C::ProcessOutput: FusedCompatible,
{
    let ctx_a = MachineContext::new(A::name());
    let ctx_b = MachineContext::new(B::name());
    let ctx_c = MachineContext::new(C::name());

    let mut state_a = A::init(&ctx_a).map_err(|e| init_err(A::name(), e))?;
    let mut state_b = B::init(&ctx_b).map_err(|e| init_err(B::name(), e))?;
    let mut state_c = C::init(&ctx_c).map_err(|e| init_err(C::name(), e))?;

    // Stage A
    let mut a_outputs = Vec::new();
    let a_done = run_machine::<A>(&mut state_a, &ctx_a, inputs, &mut a_outputs);

    // A → B
    let b_inputs: Vec<B::Input> = a_outputs
        .into_iter()
        .filter_map(|o| L1::extract(o))
        .collect();
    let mut b_outputs = Vec::new();
    let b_done = run_machine::<B>(&mut state_b, &ctx_b, b_inputs, &mut b_outputs);

    // B → C
    let c_inputs: Vec<C::Input> = b_outputs
        .into_iter()
        .filter_map(|o| L2::extract(o))
        .collect();
    let mut c_outputs = Vec::new();
    let _c_done = run_machine::<C>(&mut state_c, &ctx_c, c_inputs, &mut c_outputs);

    A::cleanup(state_a, &ctx_a).map_err(|e| cleanup_err(A::name(), e))?;
    B::cleanup(state_b, &ctx_b).map_err(|e| cleanup_err(B::name(), e))?;
    C::cleanup(state_c, &ctx_c).map_err(|e| cleanup_err(C::name(), e))?;

    if a_done {
        return Err(StaticExecError::MachineDone {
            machine: A::name(),
            processed: c_outputs.len(),
        });
    }
    if b_done {
        return Err(StaticExecError::MachineDone {
            machine: B::name(),
            processed: c_outputs.len(),
        });
    }

    Ok(c_outputs)
}

// ════════════════════════════════════════════════════════════════════════════
// Fan-out: A → (B, C)
// ════════════════════════════════════════════════════════════════════════════

/// 2 路扇出：`A → (B, C)`。
///
/// A 的输出经 `S::split` 拆分为两路，左路经 `LB` 转换为 B 的输入，
/// 右路经 `LC` 转换为 C 的输入。返回 `(B 的输出, C 的输出)`。
///
/// # Split 语义
///
/// `CloneSplit` 等量复制（Tee 语义）；自定义 `Split` 可按内容路由或
/// 解构元组。参见 `axiom::static_exec::Split`。
pub fn fanout2<A, B, C, S, LB, LC>(
    inputs: impl IntoIterator<Item = A::Input>,
) -> Result<(Vec<B::Output>, Vec<C::Output>), StaticExecError>
where
    A: FusedInline,
    B: FusedInline,
    C: FusedInline,
    S: Split<A::Output, Left = A::Output, Right = A::Output>,
    LB: Link<A, B>,
    LC: Link<A, C>,
    A::ProcessOutput: FusedCompatible,
    B::ProcessOutput: FusedCompatible,
    C::ProcessOutput: FusedCompatible,
{
    let ctx_a = MachineContext::new(A::name());
    let ctx_b = MachineContext::new(B::name());
    let ctx_c = MachineContext::new(C::name());

    let mut state_a = A::init(&ctx_a).map_err(|e| init_err(A::name(), e))?;
    let mut state_b = B::init(&ctx_b).map_err(|e| init_err(B::name(), e))?;
    let mut state_c = C::init(&ctx_c).map_err(|e| init_err(C::name(), e))?;

    // Stage A
    let mut a_outputs = Vec::new();
    let a_done = run_machine::<A>(&mut state_a, &ctx_a, inputs, &mut a_outputs);

    // Split + Link
    let mut b_inputs = Vec::new();
    let mut c_inputs = Vec::new();
    for o in a_outputs {
        let (left, right) = S::split(o);
        if let Some(bi) = LB::extract(left) {
            b_inputs.push(bi);
        }
        if let Some(ci) = LC::extract(right) {
            c_inputs.push(ci);
        }
    }

    // Stage B
    let mut b_outputs = Vec::new();
    let _b_done = run_machine::<B>(&mut state_b, &ctx_b, b_inputs, &mut b_outputs);

    // Stage C
    let mut c_outputs = Vec::new();
    let _c_done = run_machine::<C>(&mut state_c, &ctx_c, c_inputs, &mut c_outputs);

    A::cleanup(state_a, &ctx_a).map_err(|e| cleanup_err(A::name(), e))?;
    B::cleanup(state_b, &ctx_b).map_err(|e| cleanup_err(B::name(), e))?;
    C::cleanup(state_c, &ctx_c).map_err(|e| cleanup_err(C::name(), e))?;

    if a_done {
        return Err(StaticExecError::MachineDone {
            machine: A::name(),
            processed: b_outputs.len() + c_outputs.len(),
        });
    }

    Ok((b_outputs, c_outputs))
}

// ════════════════════════════════════════════════════════════════════════════
// Fan-in: (A, B) → C
// ════════════════════════════════════════════════════════════════════════════

/// 2 路汇聚：`(A, B) → C`。
///
/// A 和 B 分别执行，输出按 `M::merge` 合并为 C 的输入。A 和 B 的输出
/// 按**配对**合并（zip 语义）：第 i 个 A 输出与第 i 个 B 输出合并。
/// 不等长时，多余输出被丢弃。
///
/// # Merge 语义
///
/// `Merge<A::Output, B::Output, Output = C::Input>` 定义如何将两个上游
/// 输出合为 C 的输入。用户实现 `Merge` 来表达具体的合并语义（求和、
/// 交错、选择等）。参见 `axiom::static_exec::Merge`。
pub fn fanin2<A, B, C, M>(
    inputs_a: impl IntoIterator<Item = A::Input>,
    inputs_b: impl IntoIterator<Item = B::Input>,
) -> Result<Vec<C::Output>, StaticExecError>
where
    A: FusedInline,
    B: FusedInline,
    C: FusedInline,
    M: Merge<A::Output, B::Output, Output = C::Input>,
    A::ProcessOutput: FusedCompatible,
    B::ProcessOutput: FusedCompatible,
    C::ProcessOutput: FusedCompatible,
{
    let ctx_a = MachineContext::new(A::name());
    let ctx_b = MachineContext::new(B::name());
    let ctx_c = MachineContext::new(C::name());

    let mut state_a = A::init(&ctx_a).map_err(|e| init_err(A::name(), e))?;
    let mut state_b = B::init(&ctx_b).map_err(|e| init_err(B::name(), e))?;
    let mut state_c = C::init(&ctx_c).map_err(|e| init_err(C::name(), e))?;

    // Stage A
    let mut a_outputs = Vec::new();
    let _a_done = run_machine::<A>(&mut state_a, &ctx_a, inputs_a, &mut a_outputs);

    // Stage B
    let mut b_outputs = Vec::new();
    let _b_done = run_machine::<B>(&mut state_b, &ctx_b, inputs_b, &mut b_outputs);

    // Merge: zip a_outputs and b_outputs, merge into C::Input
    let c_inputs: Vec<C::Input> = a_outputs
        .into_iter()
        .zip(b_outputs.into_iter())
        .map(|(a_out, b_out)| M::merge(a_out, b_out))
        .collect();

    // Stage C
    let mut c_outputs = Vec::new();
    let _c_done = run_machine::<C>(&mut state_c, &ctx_c, c_inputs, &mut c_outputs);

    A::cleanup(state_a, &ctx_a).map_err(|e| cleanup_err(A::name(), e))?;
    B::cleanup(state_b, &ctx_b).map_err(|e| cleanup_err(B::name(), e))?;
    C::cleanup(state_c, &ctx_c).map_err(|e| cleanup_err(C::name(), e))?;

    Ok(c_outputs)
}

// ════════════════════════════════════════════════════════════════════════════
// 菱形：A → Split → (Left, Right) → Merge → Down
// ════════════════════════════════════════════════════════════════════════════

/// 菱形执行：`A → Split → (Left, Right) → Merge → Down`。
///
/// [`Diamond`] 组合子的便捷入口，等价于
/// `<Diamond<A, Left, Right, Down, S, LB, LC, M> as StaticChain>::run_all(inputs)`。
///
/// 一个上游经 `S::split` 分叉为两条任意深度的链（`Left`/`Right`，可为
/// 单机器或 [`Chain`]），再经 `M::merge` zip 配对汇合为一条下游链
/// （`Down`）。此前该形状需要手动衔接 [`fanout2`]（产出 `Vec<Output>`）
/// 与 [`fanin2`]（接受 `Vec<Input>`），两端类型不匹配；`diamond` 一次
/// 展开上游 + 两臂 + 下游的拓扑，消除中间衔接。
///
/// # 零成本
///
/// 全单态化：`A::process`、两臂与下游链内各机器 `process`、`S::split`、
/// `LB/LC::extract`、`M::merge` 都是具体函数，`--release` + `#[inline]`
/// 下融合。无 `Box<dyn Any>`、无 trait dispatch。参见
/// `axiom::static_exec::Diamond`。
pub fn diamond<A, Left, Right, Down, S, LB, LC, M>(
    inputs: Vec<A::StraightIn>,
) -> Result<Vec<Down::Out>, StaticExecError>
where
    A: StraightMachine,
    Left: StaticChain,
    Right: StaticChain,
    Down: StaticChain,
    S: StraightSplit<A::StraightOut, Left = A::StraightOut, Right = A::StraightOut>,
    LB: StraightLink<A, Left::Head>,
    LC: StraightLink<A, Right::Head>,
    M: StraightMerge<
        Left::Out,
        Right::Out,
        Output = <Down::Head as StraightMachine>::StraightIn,
    >,
{
    <Diamond<A, Left, Right, Down, S, LB, LC, M> as StaticChain>::run_all(inputs)
}

// ════════════════════════════════════════════════════════════════════════════
// 反馈环：A 输出经一个 tick 延迟反馈回 A 输入
// ════════════════════════════════════════════════════════════════════════════

/// 反馈环：`A` 的输出经一个 tick 延迟反馈回 `A` 的输入。
///
/// 静态路径从无环 DAG 迈向**确定性有环**的第一步：单机器自反馈环。
/// 每 tick，`A` 消费"外部输入 + 上一 tick 的输出"，产出新输出——它既是
/// 本轮结果，也经隐式延迟（等价于 [`Latch`](axiom::builtin::Latch) 的
/// 一个 tick 延迟）反馈为下一 tick 的输入。
///
/// 环的语义（`t` 为 tick 序号）：
///
/// ```text
/// output[0] = A(merge(input[0], initial))
/// output[t] = A(merge(input[t], output[t-1]))
/// ```
///
/// 环被拆为"无环主体 `A` + 延迟回边"：延迟由内部状态模拟，第一次 tick
/// 的反馈是调用者显式提供的 `initial`。这是把有环拓扑在无环的同步批量
/// 模型上表达的关键——显式延迟使环可静态单态化，而无需运行时 channel
/// 的隐式延迟。
///
/// # 零成本
///
/// 每 tick 的 `M::merge`、`A::process_straight` 都是裸函数，单态化；无
/// `Box<dyn Any>`、无 trait dispatch、无端口枚举标签（P0）。与
/// `Chain`/`Diamond` 的批量模型不同，`feedback` 是逐 tick 交错（每个
/// tick 的输出立即反馈），这正是环的执行语义。
///
/// # 类型参数
///
/// - `A`：环上的机器（`StraightMachine`，裸载荷）
/// - `M: StraightMerge<A::StraightIn, A::StraightOut, Output = A::StraightIn>`：
///   合并外部输入（`A::StraightIn`）与反馈（`A::StraightOut`）为 `A` 的
///   新输入
/// - `initial`：第一次 tick 的反馈值（显式，避免隐式 `Default`）
pub fn feedback<A, M>(
    inputs: Vec<A::StraightIn>,
    initial: A::StraightOut,
) -> Result<Vec<A::StraightOut>, StaticExecError>
where
    A: StraightMachine,
    A::StraightOut: Clone,
    M: StraightMerge<A::StraightIn, A::StraightOut, Output = A::StraightIn>,
{
    let ctx = MachineContext::new(A::name());
    let mut state = A::init(&ctx).map_err(|e| init_err(A::name(), e))?;

    let mut prev: A::StraightOut = initial;
    let mut outputs = Vec::with_capacity(inputs.len());

    for input in inputs {
        // 裸载荷直传：merge 消费反馈（move），process 无枚举。结果与反馈
        // 是双消费者——一次 Clone（业务分发，非标签税）。
        let merged = M::merge(input, prev);
        let out = A::process_straight(&mut state, merged);
        outputs.push(out.clone());
        prev = out;
    }

    A::cleanup(state, &ctx).map_err(|e| cleanup_err(A::name(), e))?;
    Ok(outputs)
}

// ════════════════════════════════════════════════════════════════════════════
// 单元测试
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::declare_ports;
    use axiom::machine::{FusedInline, Machine, SingleOutput};
    use axiom::port::MachineContext;
    use axiom::static_exec::{
        Chain, CloneSplit, Link, Merge,
        StraightClone, StraightId, StraightMachine, StraightMerge,
    };

    // ── 测试机器 ──────────────────────────────────────────────────────────

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
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
        fn process(
            _: &mut (),
            _: &MachineContext,
            input: DoublerInput,
        ) -> SingleOutput<DoublerOutput> {
            match input {
                DoublerInput::x(n) => SingleOutput::Yield(DoublerOutput::y(n * 2)),
            }
        }
        fn cleanup(
            _: (),
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for Doubler {}
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
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<i32, axiom::machine::InitError> { Ok(0) }
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
        fn cleanup(
            _: i32,
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for Adder {}
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
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
        fn process(
            _: &mut (),
            _: &MachineContext,
            input: TriplerInput,
        ) -> SingleOutput<TriplerOutput> {
            match input {
                TriplerInput::x(n) => SingleOutput::Yield(TriplerOutput::y(n * 3)),
            }
        }
        fn cleanup(
            _: (),
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for Tripler {}
    impl StraightMachine for Tripler {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n * 3 }
    }

    // ── Link 实现 ────────────────────────────────────────────────────────

    struct DoublerToAdder;
    impl Link<Doubler, Adder> for DoublerToAdder {
        fn extract(out: DoublerOutput) -> Option<AdderInput> {
            match out {
                DoublerOutput::y(n) => Some(AdderInput::x(n)),
            }
        }
    }

    struct DoublerToTripler;
    impl Link<Doubler, Tripler> for DoublerToTripler {
        fn extract(out: DoublerOutput) -> Option<TriplerInput> {
            match out {
                DoublerOutput::y(n) => Some(TriplerInput::x(n)),
            }
        }
    }

    struct TriplerToAdder;
    impl Link<Tripler, Adder> for TriplerToAdder {
        fn extract(out: TriplerOutput) -> Option<AdderInput> {
            match out {
                TriplerOutput::y(n) => Some(AdderInput::x(n)),
            }
        }
    }

    // ── pipeline_chain 测试（编译期递归链，Straight 裸载荷）─────────────

    #[test]
    fn pipeline_chain_4_stage_recursive() {
        // Doubler → Adder → Doubler → Adder（4 级递归链，任意深度）
        type Chain4 = Chain<
            Doubler,
            Chain<Adder, Chain<Doubler, Adder, StraightId>, StraightId>,
            StraightId,
        >;
        // 输入 [1, 2]: 1→D(2)→A1(2)→D(4)→A2(4); 2→D(4)→A1(6)→D(12)→A2(16)
        let outputs = pipeline_chain::<Chain4>(vec![1, 2]).expect("chain4");
        assert_eq!(outputs, vec![4, 16]);
    }

    #[test]
    fn pipeline_chain_3_stage_recursive() {
        // Doubler → Tripler → Adder（3 级，StraightId 裸链接）
        type Chain3 = Chain<Doubler, Chain<Tripler, Adder, StraightId>, StraightId>;
        // 输入 [2]: 2→D(4)→T(12)→A(12)
        let outputs = pipeline_chain::<Chain3>(vec![2]).expect("chain3");
        assert_eq!(outputs, vec![12]);
    }

    #[test]
    fn pipeline_chain_empty_inputs() {
        type Chain4 = Chain<
            Doubler,
            Chain<Adder, Chain<Doubler, Adder, StraightId>, StraightId>,
            StraightId,
        >;
        let outputs = pipeline_chain::<Chain4>(vec![]).expect("chain4 empty");
        assert!(outputs.is_empty());
    }

    // ── pipeline2 测试 ───────────────────────────────────────────────────

    #[test]
    fn pipeline2_chains_two_machines() {
        // Doubler(×2) → Adder(累加)
        // 输入 [1, 2, 3] → Doubler → [2, 4, 6] → Adder → [2, 6, 12]
        let inputs = vec![DoublerInput::x(1), DoublerInput::x(2), DoublerInput::x(3)];
        let outputs = pipeline2::<Doubler, Adder, DoublerToAdder>(inputs).expect("pipeline2");

        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], AdderOutput::y(2));
        assert_eq!(outputs[1], AdderOutput::y(6));
        assert_eq!(outputs[2], AdderOutput::y(12));
    }

    #[test]
    fn pipeline2_empty_inputs() {
        let outputs = pipeline2::<Doubler, Adder, DoublerToAdder>(vec![]).expect("pipeline2");
        assert!(outputs.is_empty());
    }

    #[test]
    fn pipeline2_single_input() {
        let outputs =
            pipeline2::<Doubler, Adder, DoublerToAdder>(vec![DoublerInput::x(5)]).expect("pipeline2");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0], AdderOutput::y(10));
    }

    // ── pipeline3 测试 ───────────────────────────────────────────────────

    #[test]
    fn pipeline3_chains_three_machines() {
        // Doubler(×2) → Tripler(×3) → Adder(累加)
        // 输入 [1, 2] → Doubler → [2, 4] → Tripler → [6, 12] → Adder → [6, 18]
        let inputs = vec![DoublerInput::x(1), DoublerInput::x(2)];
        let outputs =
            pipeline3::<Doubler, Tripler, Adder, DoublerToTripler, TriplerToAdder>(inputs)
                .expect("pipeline3");

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0], AdderOutput::y(6));
        assert_eq!(outputs[1], AdderOutput::y(18));
    }

    #[test]
    fn pipeline3_empty_inputs() {
        let outputs =
            pipeline3::<Doubler, Tripler, Adder, DoublerToTripler, TriplerToAdder>(vec![])
                .expect("pipeline3");
        assert!(outputs.is_empty());
    }

    // ── fanout2 测试 ─────────────────────────────────────────────────────

    #[test]
    fn fanout2_splits_to_two_downstreams() {
        // Doubler → CloneSplit → (Adder, Tripler)
        // 输入 [1, 2] → Doubler → [2, 4]
        // CloneSplit → ([2,4], [2,4])
        // Adder → [2, 6], Tripler → [6, 12]

        // CloneSplit 对 DoublerOutput 需要 Clone
        // DoublerOutput derives Clone via declare_ports!

        struct DoublerToAdderL;
        impl Link<Doubler, Adder> for DoublerToAdderL {
            fn extract(out: DoublerOutput) -> Option<AdderInput> {
                match out {
                    DoublerOutput::y(n) => Some(AdderInput::x(n)),
                }
            }
        }

        struct DoublerToTriplerL;
        impl Link<Doubler, Tripler> for DoublerToTriplerL {
            fn extract(out: DoublerOutput) -> Option<TriplerInput> {
                match out {
                    DoublerOutput::y(n) => Some(TriplerInput::x(n)),
                }
            }
        }

        let inputs = vec![DoublerInput::x(1), DoublerInput::x(2)];
        let (b_out, c_out) =
            fanout2::<Doubler, Adder, Tripler, CloneSplit, DoublerToAdderL, DoublerToTriplerL>(
                inputs,
            )
            .expect("fanout2");

        // Adder: 2 → 2, 4 → 6
        assert_eq!(b_out.len(), 2);
        assert_eq!(b_out[0], AdderOutput::y(2));
        assert_eq!(b_out[1], AdderOutput::y(6));

        // Tripler: 2 → 6, 4 → 12
        assert_eq!(c_out.len(), 2);
        assert_eq!(c_out[0], TriplerOutput::y(6));
        assert_eq!(c_out[1], TriplerOutput::y(12));
    }

    #[test]
    fn fanout2_empty_inputs() {
        struct DoublerToAdderL;
        impl Link<Doubler, Adder> for DoublerToAdderL {
            fn extract(out: DoublerOutput) -> Option<AdderInput> {
                match out {
                    DoublerOutput::y(n) => Some(AdderInput::x(n)),
                }
            }
        }

        struct DoublerToTriplerL;
        impl Link<Doubler, Tripler> for DoublerToTriplerL {
            fn extract(out: DoublerOutput) -> Option<TriplerInput> {
                match out {
                    DoublerOutput::y(n) => Some(TriplerInput::x(n)),
                }
            }
        }

        let (b_out, c_out) =
            fanout2::<Doubler, Adder, Tripler, CloneSplit, DoublerToAdderL, DoublerToTriplerL>(
                vec![],
            )
            .expect("fanout2");
        assert!(b_out.is_empty());
        assert!(c_out.is_empty());
    }

    // ── fanin2 测试 ──────────────────────────────────────────────────────

    #[test]
    fn fanin2_merges_two_upstreams() {
        // (Doubler, Tripler) → Merge(求和) → Adder(累加)
        // Doubler([1,2]) → [2,4], Tripler([1,2]) → [3,6]
        // Merge(求和): (2,3)→5, (4,6)→10
        // Adder: 5 → 5, 10 → 15

        struct SumMerge;
        impl Merge<DoublerOutput, TriplerOutput> for SumMerge {
            type Output = AdderInput;
            fn merge(a: DoublerOutput, b: TriplerOutput) -> AdderInput {
                match (a, b) {
                    (DoublerOutput::y(av), TriplerOutput::y(bv)) => AdderInput::x(av + bv),
                }
            }
        }

        let inputs_a = vec![DoublerInput::x(1), DoublerInput::x(2)];
        let inputs_b = vec![TriplerInput::x(1), TriplerInput::x(2)];
        let outputs = fanin2::<Doubler, Tripler, Adder, SumMerge>(inputs_a, inputs_b)
            .expect("fanin2");

        // Merge: (2,3)→5, (4,6)→10
        // Adder: 5→5, 10→15
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0], AdderOutput::y(5));
        assert_eq!(outputs[1], AdderOutput::y(15));
    }

    #[test]
    fn fanin2_unequal_lengths_drops_excess() {
        // A 有 2 个输入，B 有 3 个输入 → zip 只配对 2 个
        struct SumMerge;
        impl Merge<DoublerOutput, TriplerOutput> for SumMerge {
            type Output = AdderInput;
            fn merge(a: DoublerOutput, b: TriplerOutput) -> AdderInput {
                match (a, b) {
                    (DoublerOutput::y(av), TriplerOutput::y(bv)) => AdderInput::x(av + bv),
                }
            }
        }

        let inputs_a = vec![DoublerInput::x(1), DoublerInput::x(2)];
        let inputs_b = vec![
            TriplerInput::x(1),
            TriplerInput::x(2),
            TriplerInput::x(3), // 这个被丢弃
        ];
        let outputs = fanin2::<Doubler, Tripler, Adder, SumMerge>(inputs_a, inputs_b)
            .expect("fanin2");

        assert_eq!(outputs.len(), 2, "excess inputs dropped");
    }

    // ── diamond 测试（Straight 裸载荷）──────────────────────────────────

    /// 裸汇合：求和。
    struct Sum;
    impl StraightMerge<i32, i32> for Sum {
        type Output = i32;
        #[inline]
        fn merge(a: i32, b: i32) -> i32 {
            a + b
        }
    }

    #[test]
    fn diamond_runs_split_then_merge() {
        // 菱形：Doubler → StraightClone → (Adder, Tripler) → Sum → Adder
        // 输入 [1, 2]: D(2,4) → split(2,2),(4,4) → A(2,6), T(6,12) → Sum(8,18) → A(8,26)
        let outputs = diamond::<
            Doubler,
            Adder,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >(vec![1, 2])
        .expect("diamond");
        assert_eq!(outputs, vec![8, 26]);
    }

    #[test]
    fn diamond_empty_inputs() {
        let outputs = diamond::<
            Doubler,
            Adder,
            Tripler,
            Adder,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >(vec![])
        .expect("diamond empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn diamond_downstream_is_chain() {
        // 菱形下游是 2 级链：Chain<Adder, Doubler, StraightId>。
        type DownChain = Chain<Adder, Doubler, StraightId>;
        // 输入 [1]: D(2) → split(2,2) → A(2), T(6) → Sum(8) → DownChain: A(8)→D(16)
        let outputs = diamond::<
            Doubler,
            Adder,
            Tripler,
            DownChain,
            StraightClone,
            StraightId,
            StraightId,
            Sum,
        >(vec![1])
        .expect("diamond downstream chain");
        assert_eq!(outputs, vec![16]);
    }

    // ── feedback 测试（Straight 裸载荷）─────────────────────────────────

    declare_ports! {
        #[derive(Debug, Clone, PartialEq)]
        pub struct PassPorts {
            input type PassInput {
                x[Data] => i32,
            }
            output type PassOutput {
                y[Data] => i32,
            }
        }
    }

    pub struct PassThrough;
    impl Machine for PassThrough {
        type State = ();
        type Input = PassInput;
        type Output = PassOutput;
        type Ports = PassPorts;
        type ProcessOutput = SingleOutput<PassOutput>;
        fn name() -> &'static str { "pass" }
        fn config_schema() -> axiom::port::ConfigSchema {
            axiom::port::ConfigSchema::new()
        }
        fn init(_ctx: &MachineContext) -> Result<(), axiom::machine::InitError> { Ok(()) }
        fn process(
            _: &mut (),
            _: &MachineContext,
            input: PassInput,
        ) -> SingleOutput<PassOutput> {
            match input {
                PassInput::x(n) => SingleOutput::Yield(PassOutput::y(n)),
            }
        }
        fn cleanup(
            _: (),
            _: &MachineContext,
        ) -> Result<(), axiom::machine::CleanupError> {
            Ok(())
        }
    }
    impl FusedInline for PassThrough {}
    impl StraightMachine for PassThrough {
        type StraightIn = i32;
        type StraightOut = i32;
        #[inline]
        fn process_straight(_: &mut (), n: i32) -> i32 { n }
    }

    #[test]
    fn feedback_prefix_sum() {
        // 前缀和：output[t] = input[t] + output[t-1]
        // A = PassThrough（透传），M = Sum（StraightMerge），initial = 0
        // input [1,2,3] → output [1, 3, 6]
        let outputs = feedback::<PassThrough, Sum>(vec![1, 2, 3], 0).expect("feedback");
        assert_eq!(outputs, vec![1, 3, 6]);
    }

    #[test]
    fn feedback_empty_inputs() {
        let outputs = feedback::<PassThrough, Sum>(vec![], 0).expect("feedback empty");
        assert!(outputs.is_empty());
    }

    #[test]
    fn feedback_nonzero_initial() {
        // 非零初始反馈：output[0] = input[0] + initial
        let outputs = feedback::<PassThrough, Sum>(vec![5], 100).expect("feedback initial");
        assert_eq!(outputs, vec![105]);
    }
}
