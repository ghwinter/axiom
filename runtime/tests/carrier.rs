//! runtime 载体/驱动测试——验证"不同载体 = 不同物理实现（T6）"且语义等价。

use axiom::cell_core::PortCell;
use axiom_runtime::carrier::{Carrier, InlineCarrier, QueueCarrier};
use axiom_runtime::flow::drive_link;

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
fn inline_carrier_compile_time_unrolled() {
    // Direct 已并入 Inline（两者逐字节等价）：Inline 即编译期展开/内联。
    let (mut sa, mut sb) = ((), ());
    let out = drive_link::<Inc, Scaler, InlineCarrier>(&mut sa, &mut sb, 3);
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
fn drive_link_verifies_wiring_before_running() {
    // drive_link 在驱动前做统一 Conforms<Wire> 编译期布线验证（原 drive_wired 已并入/删除）。
    let (mut sa, mut sb) = ((), ());
    let out = drive_link::<Inc, Scaler, InlineCarrier>(&mut sa, &mut sb, 10);
    assert_eq!(out, 22); // 10 -> 11 -> 22
}

#[test]
fn static_path_unrolls_declared_static_subgraph() {
    use axiom::cell_core::{Chain, Static};
    use axiom_runtime::static_path::{run_declared_static, run_static};

    // 静态子图：链 Inc -> Scaler（被 Static 声明为"要求零成本"）。
    type StaticChain = Chain<Inc, Scaler>;
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
fn spawned_flow_crosses_threads() {
    // spawned_flow 跨线程：A(Inc) 在调用线程产出，B(Scaler) 状态在工作线程，跨线程 causal flow。
    let mut sa = ();
    // 用 `spawned_flow`：Inc 输出 5->6，Scaler 在工作线程 6->12。
    let out = axiom_runtime::carrier::spawned_flow::<Inc, Scaler>(&mut sa, || (), 5);
    assert_eq!(out, 12);
}

#[test]
fn spawned_flow_propagates_worker_panic() {
    // S1：工作线程 B::step panic 必须传播到调用线程（原实现会永久阻塞 recv）。
    struct Boom;
    impl PortCell for Boom {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), _: i32) -> i32 {
            panic!("boom in worker");
        }
    }
    let mut sa = ();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        axiom_runtime::carrier::spawned_flow::<Inc, Boom>(&mut sa, || (), 5)
    }));
    assert!(result.is_err(), "worker panic must propagate to the caller");
}

#[test]
fn wire_macro_compile_time_inline() {
    // wire! 宏：Inc -> Scaler 一条因果流，编译期展开为内联调用（零宏运行时开销）。
    let mut sa = ();
    let mut sb = ();
    let flow = axiom_runtime::wire!(Inc => Scaler);
    let out = flow(&mut sa, &mut sb, 4); // 4 -> 5 -> 10
    assert_eq!(out, 10);
}
