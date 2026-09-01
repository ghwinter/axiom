//! 静态路径：把声明为静态的 cell_core 子图在编译期内联展开，零运行时对象。
//!
//! 对应"静态优先 + 编译期展开"（T7 静态路径）。蓝图里用 [`Static<C>`] 声明"这棵
//! 子图 `C` 要求零成本"，此处驱动时强制走编译期展开 / 内联（Direct 已并入 Inline），
//! 语义上等价手写 `C::step` 的直接调用——`Chain`/`Broadcast`/`Feedback` 的
//! `step`/`fire`/`tick` 已 `#[inline(always)]`，编译器把整棵静态子图折叠成一段指令，
//! 无中间对象、零分配。

use axiom::cell_core::{PortCell, Static};

/// 在一个"声明为静态"的子图 `C` 上运行一次，返回输出。
///
/// `C: PortCell` 可为任意四构件拓扑（链/嵌套/广播聚合）。被 `Static<C>` 声明后，
/// 驱动走编译期展开路径（零运行时对象、零分配，语义 = 手写 `C::step`）。
#[inline(always)]
pub fn run_static<C>(state: &mut C::State, input: C::In) -> C::Out
where
    C: PortCell,
{
    C::step(state, input)
}

/// 以"声明为静态"的入口运行一个子图 `SUB`。
///
/// `_declared: &Static<SUB>` 是编译期见证（蓝图声明"SUB 要求零成本"）——仅类型层，
/// 零大小、无运行时对象。驱动即 `SUB::step` 的内联展开。
#[inline(always)]
pub fn run_declared_static<SUB>(
    _declared: &Static<SUB>,
    state: &mut SUB::State,
    input: SUB::In,
) -> SUB::Out
where
    SUB: PortCell,
{
    SUB::step(state, input)
}
