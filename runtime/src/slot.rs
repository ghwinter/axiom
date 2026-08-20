//! 装载槽的运行期存在化（∃ 装载，物理侧）。
//!
//! 槽口 `Slot<I,O>` 在 core 由 `Conforms` 编译期验证（T1：任何 `In=I, Out=O` 的占据者
//! 都合规）。本模块把"运行时**选择**一个合规占据者并**驱动**"做成安全的类型擦除：
//! 占据者的状态类型擦除为 `Box<dyn Any + Send>`，`step` 以函数指针保存
//! （针对该占据者单态化）。这是统一模型里"∃ 装载"的**物理侧**——接口固定
//! （T1 编译期验证）、占据者运行期存在化、可换装。

use axiom::cell_core::PortCell;
use core::any::Any;

/// 一个被运行期存在化填充的装载槽实例。
///
/// - 构造：[`install`](Self::install) 把一个**编译期合规**的占据者 `T`
///   （`T: PortCell<In=I, Out=O>` ⟹ 满足 core 的 `Conforms` 判定）安装进槽；`T` 类型被擦除。
/// - 驱动：[`drive`](Self::drive) 把一次输入流经已安装的占据者，返回 `O`。
/// - 换装：[`swap`](Self::swap) 运行期代换另一个合规占据者（存在化）。
#[cfg(feature = "std")]
pub struct SlotDrive<I, O> {
    state: Option<Box<dyn Any + Send>>,
    step: fn(&mut Box<dyn Any + Send>, I) -> O,
}

#[cfg(feature = "std")]
impl<I, O> SlotDrive<I, O> {
    /// 安装一个合规占据者 `T`（`T: PortCell<In=I, Out=O>` ⟹ `Conforms<Slot<I,O>>`）。
    pub fn install<T>(state: T::State) -> Self
    where
        T: PortCell<In = I, Out = O> + Send + 'static,
        T::State: Send + 'static,
    {
        SlotDrive {
            state: Some(Box::new(state)),
            step: |s, input| {
                let st = s
                    .downcast_mut::<T::State>()
                    .expect("occupant state type matches");
                T::step(st, input)
            },
        }
    }

    /// 换装一个不同的合规占据者（运行期代换，存在化）。
    pub fn swap<T>(&mut self, state: T::State)
    where
        T: PortCell<In = I, Out = O> + Send + 'static,
        T::State: Send + 'static,
    {
        *self = Self::install::<T>(state);
    }

    /// 驱动一次：`input` 流经已安装占据者，返回 `O`。
    pub fn drive(&mut self, input: I) -> O {
        let s = self.state.as_mut().expect("occupant installed");
        (self.step)(s, input)
    }
}
