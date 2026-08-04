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
    /// 按端口注入路由来的 payload：用 `HasPortInfo::from_port_name`
    /// 构造本机器的输入 variant 并 process。`Idle` 表示端口不匹配。
    fn inject(&mut self, port: &str, payload: Box<dyn core::any::Any + Send>) -> ProcessResult;
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
        Ok(Self {
            handle: Some(handle),
            done: false,
            fused,
            schema: M::port_schema(),
        })
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
        let handle = match self.handle.as_mut() {
            Some(h) => h,
            None => return ProcessResult::Idle,
        };

        let input: M::Input = match input.downcast::<M::Input>() {
            Ok(b) => *b,
            Err(_) => return ProcessResult::Idle,
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

    fn inject(&mut self, port: &str, payload: Box<dyn core::any::Any + Send>) -> ProcessResult {
        // 用端口名 + 类型擦除 payload 构造本机器的输入 variant（路由路径）。
        let input = match <M::Input as HasPortInfo>::from_port_name(port, payload) {
            Some(i) => i,
            None => return ProcessResult::Idle,
        };
        self.process_boxed(Box::new(input))
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
