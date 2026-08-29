//! runtime 载体用例：用 cell_core 蓝图 + Carrier 物理载体驱动一个二阶拓扑。
//!
//! 演示 runtime 的定位——"cell_core 的物理层实现用例"：同一张四构件蓝图
//! （链 + 广播），用不同载体（Inline / Queue / Bounded）驱动，**语义等价，
//! 但时空成本不同**（T6 多物理实现）。换载体不改拓扑。

use axiom::cell_core::{Broadcast, Chain, PortCell};
use axiom_semantics::movers::carrier::{Carrier, CarrierCost, InlineCarrier, QueueCarrier};
use axiom_semantics::drive::flow::drive_link;

// ── 有状态细胞 ─────────────────────────────────────────

/// 计数器：状态累加，输出累加和。
struct Counter;
impl PortCell for Counter {
    type In = i32;
    type Out = i32;
    type State = i32;
    fn step(s: &mut i32, x: i32) -> i32 {
        *s += x;
        *s
    }
}

/// 加法器：恒等（作为接收者演示，直接透传 +0 语义用 Counter 即可）。
struct Double;
impl PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = ();
    fn step(_: &mut (), x: i32) -> i32 {
        x * 2
    }
}

fn main() {
    // === A. 链：Counter -> Double（用 InlineCarrier 驱动，零分配） ===
    type Chain2 = Chain<Counter, Double>;
    let mut sc = <Chain2 as PortCell>::State::default();
    // drive_link 显式用 InlineCarrier：等价手写，零分配。
    let a1 = drive_link::<Counter, Double, InlineCarrier>(&mut sc.0, &mut sc.1, 5);
    let a2 = drive_link::<Counter, Double, InlineCarrier>(&mut sc.0, &mut sc.1, 3);
    println!(
        "A. Inline 驱动 Counter->Double: 一次(5)= {a1}, 二次累加(3)= {a2} (零分配)"
    );

    // === B. 同一链再跑一次：Direct 已并入 Inline（编译期展开 = 内联直传） ===
    let mut sb = <Chain2 as PortCell>::State::default();
    let b = drive_link::<Counter, Double, InlineCarrier>(&mut sb.0, &mut sb.1, 7);
    println!("B. Inline 驱动同上链（Direct 已并入 Inline）: (7) = {b} (零分配)");

    // === C. 广播：Counter -> (Counter, Double) 用 Broadcast 类型层 fan-out ===
    let (mut ssrc, mut sr1, mut sr2) = (0i32, 0i32, ());
    let (co, do_) = Broadcast::<Counter, Counter, Double>::fire(&mut ssrc, &mut sr1, &mut sr2, 4);
    println!("C. Broadcast 广播: C出={co}, D出={do_} (多对多, 无 Tee 树)");

    // === D. QueueCarrier：同一 Inline 语义，但每消息堆分配 ===
    let (mut scq, mut sdq) = (0i32, ());
    let d = drive_link::<Counter, Double, QueueCarrier>(&mut scq, &mut sdq, 2);
    println!(
        "D. Queue 驱动同上: (2) = {d} ({} , 每消息堆分配)",
        cost_str::<QueueCarrier, Counter, Double>()
    );

    // 说明：A/B/D 语义等价（因果流相同），仅物理载体不同。
    // A: Inline 零分配 / B: Direct 编译期展开 / D: Queue 每消息分配。
    println!("carrier_demo ok: 同一张蓝图多载体可替换、语义等价、时空成本不同");
}

fn cost_str<C, A, B>() -> &'static str
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    match C::cost() {
        CarrierCost::ZeroAllocInline => "零分配内联",
        CarrierCost::PerMessageAlloc => "每消息分配",
        CarrierCost::External => "外部",
    }
}
