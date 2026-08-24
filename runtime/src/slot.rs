//! 型位的运行期存在化（∃ 绑定，物理侧）。
//!
//! 型位 `Slot<I,O>` 在 core 由 `Conforms` 编译期验证（T1：任何 `In=I, Out=O` 的居留项
//! 都合规）。本模块把"运行时**选择**一个合规居留项并**驱动**"做成安全的类型擦除：
//! 居留项的状态类型擦除为 `Box<dyn Any + Send>`，`step` 以函数指针保存
//! （针对该居留项单态化）。这是统一模型里"∃ 绑定"的**物理侧**——接口固定
//! （T1 编译期验证）、居留项运行期存在化、可换装。
//!
//! **成本声明（模态 ③）**：每次安装一次堆分配（`Box`）+ 函数指针间接调用——本接缝是
//! runtime 的动态税位置之一，成本在部署期显式声明（PerInstallAlloc 类）。

use axiom::cell_core::PortCell;
use core::any::Any;

/// 一个被运行期存在化填充的型位实例。
///
/// - 构造：[`install`](Self::install) 把一个**编译期合规**的居留项 `T`
///   （`T: PortCell<In=I, Out=O>` ⟹ 满足 core 的 `Conforms` 判定）安装进型位；`T` 类型被擦除。
/// - 驱动：[`drive`](Self::drive) 把一次输入流经已安装的居留项，返回 `O`。
/// - 换装：[`swap`](Self::swap) 运行期代换另一个合规居留项（存在化）。
#[cfg(feature = "std")]
pub struct SlotDrive<I, O> {
    state: Option<Box<dyn Any + Send>>,
    step: fn(&mut Box<dyn Any + Send>, I) -> O,
}

#[cfg(feature = "std")]
impl<I, O> SlotDrive<I, O> {
    /// 安装一个合规居留项 `T`（`T: PortCell<In=I, Out=O>` ⟹ `Conforms<Slot<I,O>>`）。
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
                    .expect("inhabitant state type matches");
                T::step(st, input)
            },
        }
    }

    /// 换装一个不同的合规居留项（运行期代换，存在化）。
    pub fn swap<T>(&mut self, state: T::State)
    where
        T: PortCell<In = I, Out = O> + Send + 'static,
        T::State: Send + 'static,
    {
        *self = Self::install::<T>(state);
    }

    /// 驱动一次：`input` 流经已安装居留项，返回 `O`。
    ///
    /// **前置条件**：须先 `install` 或 `swap`；未安装时调用将 panic（`inhabitant installed`
    /// 断言）——声明性前置条件，非未定义行为。**成本**：每次安装 1 次堆分配（`Box`）+
    /// 函数指针间接调用（每安装分配，模态 ③，部署期声明）。
    pub fn drive(&mut self, input: I) -> O {
        let s = self.state.as_mut().expect("inhabitant installed");
        (self.step)(s, input)
    }
}
