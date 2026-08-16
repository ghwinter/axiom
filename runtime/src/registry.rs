//! 机器类型注册表——`machine_type` 字符串 → `RegisterFn` 构造函数。

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;

use axiom::machine::Machine;
use axiom::port::MachineContext;

// CompositeSpec 现在从 core 引用——复合是结构定义能力，属于 axiom core。
use axiom::composite::CompositeSpec;
use crate::erasure::{MachineWrapper, RunningMachine, ScratchMachine};
use crate::error::RuntimeError;

/// 机器构造函数——从 `MachineContext` 构造 `Box<dyn RunningMachine>`。
pub trait RegisterFn: Send + Sync {
    fn build(&self, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError>;
}

struct TypedRegisterFn<M: Machine>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    fused: bool,
    _phantom: core::marker::PhantomData<M>,
}

impl<M: Machine> RegisterFn for TypedRegisterFn<M>
where
    M::Input: core::any::Any + Send,
    M::Output: core::any::Any + Send,
{
    fn build(&self, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError> {
        let wrapper = MachineWrapper::<M>::new(ctx, self.fused)?;
        Ok(Box::new(wrapper))
    }
}

/// 类型化融合注册器——构建 [`ScratchMachine`]（unsafe 破局后的级间免装箱）。
/// `M::Input: Pack` + `M::Output: Unpack`（FusedInline 单输入单输出机器
/// 由 declare_ports 自动满足）。
struct TypedFusedRegisterFn<M: Machine>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    _phantom: core::marker::PhantomData<M>,
}

impl<M: Machine> RegisterFn for TypedFusedRegisterFn<M>
where
    M::Input: core::any::Any + Send + axiom::portset::Pack,
    M::Output: core::any::Any + Send + axiom::portset::Unpack,
{
    fn build(&self, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError> {
        Ok(Box::new(ScratchMachine::<M>::new(ctx, true)?))
    }
}

/// 机器类型注册表——`machine_type` 字符串 → `RegisterFn` 或复合定义。
pub struct Registry {
    builders: BTreeMap<String, Box<dyn RegisterFn>>,
    /// 复合 machine_type → 子拓扑 + 端口映射。materialize 时展开。
    composites: BTreeMap<String, CompositeSpec>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            builders: BTreeMap::new(),
            composites: BTreeMap::new(),
        }
    }

    pub fn register<M>(&mut self, machine_type: &str)
    where
        M: Machine,
        M::Input: core::any::Any + Send,
        M::Output: core::any::Any + Send,
    {
        self.builders.insert(
            machine_type.to_string(),
            Box::new(TypedRegisterFn::<M> {
                fused: false,
                _phantom: core::marker::PhantomData,
            }),
        );
    }

    /// 注册一个可融合机器——`M: FusedInline` 在类型层保证 `ProcessOutput`
    /// 为 `SingleOutput`/`TupleOutput`（输出数量固定，无 `YieldMulti`），
    /// `M::Input: Pack` + `M::Output: Unpack`（单输入单输出，declare_ports
    /// 自动满足）——`materialize` 可将其纳入 `FusedPipeline` 链，级间经
    /// `ScratchMachine` 类型化单槽免装箱（unsafe 破局，同类型级间 0 分配）。
    pub fn register_fused<M>(&mut self, machine_type: &str)
    where
        M: Machine + axiom::machine::FusedInline,
        M::Input: core::any::Any + Send + axiom::portset::Pack,
        M::Output: core::any::Any + Send + axiom::portset::Unpack,
        M::ProcessOutput: axiom::machine::FusedCompatible,
    {
        self.builders.insert(
            machine_type.to_string(),
            Box::new(TypedFusedRegisterFn::<M> {
                _phantom: core::marker::PhantomData,
            }),
        );
    }

    /// 注册一个复合 Machine——子拓扑 + 端口映射。materialize 时展开为
    /// 名字空间化的子机器 + 重定向的外部链接。
    pub fn register_composite(&mut self, machine_type: &str, spec: CompositeSpec) {
        self.composites.insert(machine_type.to_string(), spec);
    }

    /// 已注册的复合定义（materialize 展开用）。
    pub(crate) fn composites(&self) -> &BTreeMap<String, CompositeSpec> {
        &self.composites
    }

    pub(crate) fn build(&self, machine_type: &str, ctx: MachineContext) -> Result<Box<dyn RunningMachine>, RuntimeError> {
        let builder = self.builders.get(machine_type)
            .ok_or_else(|| RuntimeError::InitFailed {
                machine: machine_type.to_string(),
                error: axiom::machine::InitError::Other(format!("type `{machine_type}` not registered")),
            })?;
        builder.build(ctx)
    }
}

impl Default for Registry {
    fn default() -> Self { Self::new() }
}
