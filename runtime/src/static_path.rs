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
//! | 拓扑能力 | 任意 DAG（不含环） | 任意 DAG + 环 |
//! | IO/异步 | 不支持 | 支持（`IoReactor`） |
//! | 适用场景 | 固定管道、热路径 | 配置驱动、插件、动态拓扑 |
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
use axiom::static_exec::{Link, Merge, Split, StaticExecError};

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
// 单元测试
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::declare_ports;
    use axiom::machine::{FusedInline, Machine, SingleOutput};
    use axiom::port::MachineContext;
    use axiom::static_exec::{CloneSplit, Link, Merge};

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
}
