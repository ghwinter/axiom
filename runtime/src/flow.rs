//! 驱动：把蓝图（cell 拓扑）+ 载体选型兑现为执行。
//!
//! 这里"兑现"沿 cell_core 的"蓝图即类型"——给定一个 `PortCell` 拓扑和载体，
//! 提供便捷的驱动入口。不同载体 = 不同物理实现（T6）。

use axiom::cell_core::{DoesWire, PortCell};
use crate::carrier::Carrier;

/// 用载体 `C` 驱动一条 A→B 因果流，返回 B 的输出。
///
/// 载体 `C` 决定物理实现（Inline=零分配内联 / Queue=队列中转 / Direct=编译期展开）。
/// 在驱动前做编译期布线验证（`DoesWire`，失败即编译错误）——验证在编译期，运行期零开销。
pub fn drive_link<A, B, C>(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    // 编译期布线判定：A.out 可布到 B.in（DoesWire 对 () 实现）。
    let _: bool = <() as DoesWire<A, B>>::WIRES;
    C::flow(sa, sb, input)
}

/// 驱动一条已验证布线的 A→B 流（显式以 `LINK` 作为布线持证者）。
///
/// `LINK` 须满足 `DoesWire<A, B>`，作为"这条因果流合法"的编译期见证。
pub fn drive_wired<A, B, LINK, C>(
    sa: &mut A::State,
    sb: &mut B::State,
    input: A::In,
) -> B::Out
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    LINK: DoesWire<A, B>,
    C: Carrier<A, B>,
{
    let _ = <() as DoesWire<A, B>>::WIRES;
    let _ = core::marker::PhantomData::<LINK>;
    C::flow(sa, sb, input)
}
