//! runtime 载体/驱动测试——验证"不同载体 = 不同物理实现（T6）"且语义等价。

use axiom::cell_core::PortCell;
use axiom_runtime::carrier::{Carrier, DirectCarrier, InlineCarrier, QueueCarrier};
use axiom_runtime::flow::{drive_link, drive_wired};

struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x + 1
    }
}

struct Scaler;
impl PortCell for Scaler {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x * 2
    }
}

#[test]
fn inline_carrier_zero_alloc_semantics() {
    let (mut sa, mut sb) = ((), ());
    // Inc(5->6) -> Scaler(6->12)
    let out = <InlineCarrier as Carrier<Inc, Scaler>>::flow(&mut sa, &mut sb, 5);
    assert_eq!(out, 12);
    // 成本声明：零分配、内联。
    assert_eq!(<InlineCarrier as Carrier<Inc, Scaler>>::cost(), axiom_runtime::carrier::CarrierCost::ZeroAllocInline);
}

#[test]
fn direct_carrier_compile_time_unrolled() {
    let (mut sa, mut sb) = ((), ());
    let out = drive_link::<Inc, Scaler, DirectCarrier>(&mut sa, &mut sb, 3);
    assert_eq!(out, 8); // 3 -> 4 -> 8
}

#[test]
fn queue_carrier_per_message_alloc_semantics_equal() {
    let (mut sa, mut sb) = ((), ());
    // Queue 载体语义等价（Inc->Scaler），但每消息堆分配。
    let out = drive_link::<Inc, Scaler, QueueCarrier>(&mut sa, &mut sb, 2);
    assert_eq!(out, 6); // 2 -> 3 -> 6
    assert_eq!(
        <QueueCarrier as Carrier<Inc, Scaler>>::cost(),
        axiom_runtime::carrier::CarrierCost::PerMessageAlloc
    );
}

#[test]
fn drive_wired_verifies_before_running() {
    // 显式布线见证（LINK: DoesWire<Inc,Scaler> 用 () 满足）。
    let (mut sa, mut sb) = ((), ());
    let out = drive_wired::<Inc, Scaler, (), InlineCarrier>(&mut sa, &mut sb, 10);
    assert_eq!(out, 22); // 10 -> 11 -> 22
}

#[test]
fn static_path_unrolls_declared_static_subgraph() {
    use axiom::cell_core::{CellChain, Static};
    use axiom_runtime::static_path::{run_declared_static, run_static};

    // 静态子图：链 Inc -> Scaler（被 Static 声明为"要求零成本"）。
    type StaticChain = CellChain<Inc, Scaler>;
    let declared = Static::<StaticChain>::declare();

    let mut st = <StaticChain as PortCell>::State::default();
    // run_static：直接编译期展开。
    let a = run_static::<StaticChain>(&mut st, 1); // 1 -> 2 -> 4
    assert_eq!(a, 4);
    // run_declared_static：以 Static 声明为入口（零大小证人），同样展开。
    let mut st2 = <StaticChain as PortCell>::State::default();
    let b = run_declared_static::<StaticChain>(&declared, &mut st2, 5); // 5 -> 6 -> 12
    assert_eq!(b, 12);
}

#[test]
fn channel_carrier_crosses_threads() {
    // 跨线程通道载体：A(Inc) 在调用线程产出，B(Scaler) 状态在工作线程，跨线程 causal flow。
    let mut sa = ();
    // 用 `spawned_flow`：Inc 输出 5->6，Scaler 在工作线程 6->12。
    let out = axiom_runtime::carrier::spawned_flow::<Inc, Scaler>(&mut sa, || (), 5);
    assert_eq!(out, 12);
}
