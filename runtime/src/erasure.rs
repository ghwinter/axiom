//! 类型擦除层——`RunningMachine` trait + `MachineWrapper` 适配器。
//!
//! runtime 持有 `Box<dyn RunningMachine>`，不需要知道具体 `M` 类型。
//! `M::Input` 通过 `Box<dyn Any>` downcast 还原；`M::Output` 通过
//! `HasPortInfo::port_name()` 提取端口名，`into_any()` 类型擦除值。

use alloc::boxed::Box;
use alloc::vec::Vec;

use axiom::machine::{Machine, MachineHandle, Init, Running};
use axiom::port::MachineContext;
use axiom::portset::HasPortInfo;

use crate::error::RuntimeError;

/// 类型擦除后的活跃 Machine——runtime 持有 `Box<dyn RunningMachine>`，
/// 不需要知道具体 `M` 类型。
///
/// `process_boxed` 接收 `Box<dyn Any + Send>`（类型擦除的 input），
/// 返回 `ProcessResult`（包含端口名和擦除后的输出值）。
/// 端口名由 `HasPortInfo::port_name()` 从输出值提取，runtime 用它匹配
/// `LinkSpec` 的源端口，找到目标机器和端口。
pub trait RunningMachine: Send {
    fn name(&self) -> &str;
    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult;
    /// 按端口 ID 注入路由来的 payload：ID 经 `in_port_names` 还原端口名，
    /// 用 `HasPortInfo::from_port_name` 构造本机器的输入 variant 并 process。
    /// `Idle` 表示端口不匹配。ID 化消除了热路径的字符串匹配与装箱。
    fn inject(&mut self, port_id: u16, payload: Box<dyn core::any::Any + Send>) -> ProcessResult;
    /// 类型化单槽处理（unsafe 破局后的级间免装箱协议）：从 `slot` 取裸值
    /// （[`take_input`] 位拷贝零分配）、`Pack` 构造输入、process、`Unpack`
    /// 裸值、经 [`put_output`] 写回同一槽（同类型零分配 / 跨类型重装箱）。
    /// 仅 [`ScratchMachine`]（`M::Input: Pack` + `M::Output: Unpack` 单输入
    /// 单输出机器）覆盖；其余机器返回 `Idle`。
    fn process_scratch(
        &mut self,
        _port_id: u16,
        _slot: &mut Option<Box<dyn core::any::Any + Send>>,
    ) -> ScratchResult {
        ScratchResult::Idle
    }
    fn is_done(&self) -> bool;
    fn port_schema(&self) -> &axiom::port::PortSchema;
    /// 是否可进入融合流水线（`register_fused::<M: FusedInline>()` 注册的
    /// 机器返回 `true`）。`materialize` 用此标记识别可融合的 Inline 链，
    /// 替换为 `FusedPipeline`——消除每跳的路由查找与队列开销。
    fn is_fused_compatible(&self) -> bool;
    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError>;
}

/// `process` 调用的结果——简化版 `ProcessOutput`，用于类型擦除后的路由。
#[derive(Debug)]
pub enum ProcessResult {
    Idle,
    Done,
    Yield { port: &'static str, value: Box<dyn core::any::Any + Send> },
    YieldMulti { outputs: Vec<(&'static str, Box<dyn core::any::Any + Send>)> },
}

/// 类型化单槽处理结果（unsafe 破局后的级间免装箱协议）。
///
/// [`RunningMachine::process_scratch`] 从调用方传入的 [`TypedSlot`] 取裸值、
/// process、经 [`recycle`] 写回同一槽（同类型零分配 / 跨类型重装箱）。
/// `Yield` 的值**隐含在槽中**（未装箱），调用方直接 move。
#[derive(Debug)]
pub enum ScratchResult {
    Idle,
    Done,
    /// 单输出：输出值已在槽中，`port` 是输出端口名。
    Yield(&'static str),
}

/// 把具体 `MachineHandle<M, Running>` 包装成 `Box<dyn RunningMachine>`。
///
/// `M::Input` 通过 `Box<dyn Any>` downcast 还原。
/// `M::Output` 通过 `HasPortInfo::port_name()` 提取端口名，
/// `HasPortInfo::into_any()` 类型擦除值。
pub(crate) struct MachineWrapper<M: Machine> {
    handle: Option<MachineHandle<M, Running>>,
    done: bool,
    fused: bool,
    schema: axiom::port::PortSchema,
    /// 输入端口名表（schema.inputs() 序）——inject(port_id) 还原端口名用。
    in_names: Vec<&'static str>,
}

impl<M: Machine> MachineWrapper<M>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    pub(crate) fn new(ctx: MachineContext, fused: bool) -> Result<Self, RuntimeError> {
        let handle = MachineHandle::<M, Init>::new(ctx)
            .map_err(|e| RuntimeError::InitFailed {
                machine: M::name().to_string(),
                error: e,
            })?
            .start();
        let schema = M::port_schema();
        let in_names: Vec<&'static str> = schema.inputs().map(|p| p.name).collect();
        Ok(Self {
            handle: Some(handle),
            done: false,
            fused,
            schema,
            in_names,
        })
    }

    /// 统一尾部：process 具体输入 → 类型擦除输出。inject 与 process_boxed
    /// 共用——inject 免去装箱+downcast（P0：消除动态路径的冗余堆分配）。
    fn process_input(&mut self, input: M::Input) -> ProcessResult {
        let handle = match self.handle.as_mut() {
            Some(h) => h,
            None => return ProcessResult::Idle,
        };

        let output = handle.process(input);
        let unified = <M::ProcessOutput as axiom::machine::MachineOutput<M::Output>>::into_process_output(output);
        match unified {
            axiom::machine::ProcessOutput::Yield(o) => {
                let port = HasPortInfo::port_name(&o);
                let value = HasPortInfo::into_any(o);
                ProcessResult::Yield { port, value }
            }
            axiom::machine::ProcessOutput::YieldMulti(outs) => {
                let mapped = outs.into_iter().map(|o| {
                    let port = HasPortInfo::port_name(&o);
                    let value = HasPortInfo::into_any(o);
                    (port, value)
                }).collect();
                ProcessResult::YieldMulti { outputs: mapped }
            }
            axiom::machine::ProcessOutput::Idle => ProcessResult::Idle,
            axiom::machine::ProcessOutput::Done => {
                self.done = true;
                ProcessResult::Done
            }
        }
    }
}

impl<M: Machine> RunningMachine for MachineWrapper<M>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    fn name(&self) -> &str {
        self.handle.as_ref().map(|h| h.context().name()).unwrap_or(M::name())
    }

    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult {
        let input: M::Input = match input.downcast::<M::Input>() {
            Ok(b) => *b,
            Err(_) => return ProcessResult::Idle,
        };
        self.process_input(input)
    }

    fn inject(&mut self, port_id: u16, payload: Box<dyn core::any::Any + Send>) -> ProcessResult {
        // ID → 端口名（&'static str，schema.inputs() 序），构造输入 variant。
        let Some(port) = self.in_names.get(port_id as usize).copied() else {
            return ProcessResult::Idle;
        };
        let Some(input) = <M::Input as HasPortInfo>::from_port_name(port, payload) else {
            return ProcessResult::Idle;
        };
        self.process_input(input)
    }

    fn is_done(&self) -> bool { self.done }

    fn is_fused_compatible(&self) -> bool { self.fused }

    fn port_schema(&self) -> &axiom::port::PortSchema { &self.schema }

    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError> {
        let inner = *self;
        if let Some(handle) = inner.handle {
            let stopped = handle.stop().finish();
            stopped.cleanup().map_err(|_e| RuntimeError::CleanupFailed {
                machine: M::name().to_string(),
            })?;
        }
        Ok(())
    }
}

/// 类型化单槽阶段（unsafe 破局后的级间免装箱：`FusedPipeline` 的级间机器）。
///
/// 包装 [`MachineWrapper`]，实现 [`RunningMachine`] 全转发 + `process_scratch`
/// **类型化覆盖**：输入经 `Pack::pack(裸值)` 构造（零分配，免
/// `from_port_name` 的 Box 消费），输出经 `Unpack::unpack` 提取裸值
/// （零分配），再经 [`recycle`] 写回同一槽——**同类型级间（如
/// `Step: i32→i32`）全程 0 分配**，仅外部输入 1 次。
///
/// 仅 `register_fused::<M>`（`M::Input: Pack` + `M::Output: Unpack`，
/// FusedInline 单输入单输出机器）构建——多输入/多输出机器不进入融合链。
pub(crate) struct ScratchMachine<M: Machine>
where
    M::Input: axiom::portset::Pack,
    M::Output: axiom::portset::Unpack,
{
    inner: MachineWrapper<M>,
}

impl<M: Machine> ScratchMachine<M>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    pub(crate) fn new(ctx: MachineContext, fused: bool) -> Result<Self, RuntimeError> {
        Ok(Self { inner: MachineWrapper::<M>::new(ctx, fused)? })
    }

    /// 类型化单槽处理：裸值 Box → `Pack` 构造 Input → process → `Unpack`
    /// 裸值 → 写回（同类型零分配 / 跨类型重装箱）。
    fn process_scratch_typed(
        &mut self,
        port_id: u16,
        slot: &mut Option<Box<dyn core::any::Any + Send>>,
    ) -> ScratchResult {
        // 端口存在性校验（级间协议仅服务单输入机器）。
        let _port = match self.inner.in_names.get(port_id as usize) {
            Some(p) => *p,
            None => return ScratchResult::Idle,
        };
        let Some(b) = slot.take() else {
            return ScratchResult::Idle;
        };
        // 取输入裸值（unsafe 封装点：位拷贝 + 保留分配）。
        let (raw_in, raw_ptr) = match crate::typed_slot::take_input::<<M::Input as axiom::portset::Pack>::Raw>(b) {
            Ok(pair) => pair,
            Err(b) => {
                *slot = Some(b);
                return ScratchResult::Idle;
            }
        };
        let input = <M::Input as axiom::portset::Pack>::pack(raw_in);
        let Some(handle) = self.inner.handle.as_mut() else {
            return ScratchResult::Idle;
        };
        let output = handle.process(input);
        let unified = <M::ProcessOutput as axiom::machine::MachineOutput<M::Output>>::into_process_output(output);
        match unified {
            axiom::machine::ProcessOutput::Yield(o) => {
                let port = HasPortInfo::port_name(&o);
                let raw_out = <M::Output as axiom::portset::Unpack>::unpack(o);
                // 写回：同类型（TypeId 相等）→ 分配复用（0 分配）；
                // 跨类型 → 释放旧分配 + 重装箱（转换点 1 次）。
                let boxed = crate::typed_slot::put_output::<
                    <M::Input as axiom::portset::Pack>::Raw,
                    <M::Output as axiom::portset::Unpack>::Raw,
                >(raw_ptr, raw_out);
                *slot = Some(boxed);
                ScratchResult::Yield(port)
            }
            // FusedInline 单输出机器：多输出不可能（SingleOutput）。
            axiom::machine::ProcessOutput::YieldMulti(_) => ScratchResult::Idle,
            axiom::machine::ProcessOutput::Idle => ScratchResult::Idle,
            axiom::machine::ProcessOutput::Done => {
                self.inner.done = true;
                ScratchResult::Done
            }
        }
    }
}

impl<M: Machine> RunningMachine for ScratchMachine<M>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn process_boxed(&mut self, input: Box<dyn core::any::Any + Send>) -> ProcessResult {
        self.inner.process_boxed(input)
    }

    fn inject(&mut self, port_id: u16, payload: Box<dyn core::any::Any + Send>) -> ProcessResult {
        self.inner.inject(port_id, payload)
    }

    fn process_scratch(&mut self, port_id: u16, slot: &mut Option<Box<dyn core::any::Any + Send>>) -> ScratchResult {
        self.process_scratch_typed(port_id, slot)
    }

    fn is_done(&self) -> bool {
        self.inner.is_done()
    }

    fn port_schema(&self) -> &axiom::port::PortSchema {
        self.inner.port_schema()
    }

    fn is_fused_compatible(&self) -> bool {
        true
    }

    fn cleanup(self: Box<Self>) -> Result<(), RuntimeError> {
        let inner = self.inner;
        Box::new(inner).cleanup()
    }
}
