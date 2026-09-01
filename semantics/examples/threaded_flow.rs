//! 基于 runtime Carrier 的"同拓扑、异构物理" example。
//!
//! 替代旧 `threaded_pipeline`（多线程阶段流水线）的精神，但用新 runtime：
//! 同一张 cell 逻辑，用不同 Carrier（InlineCarrier 单线程零分配 /
//! `spawned_flow` 跨线程通道载体）执行，语义等价、物理不同（T6 多物理实现）。
//!
//! 演示：一个"传感器 → 归一化 → 累加 → 输出"的因果链，分别：
//! - 用 InlineCarrier 在调用线程直接跑（栈上函数传递、零分配）；
//! - 其中一步用 `spawned_flow` 放到独立工作线程（mpsc 通道 + B::State 在线程内）。

use axiom::cell_core::{Chain, PortCell};
use axiom_semantics::movers::carrier::{InlineCarrier, spawned_flow};
use axiom_semantics::drive::flow::drive_link;

// ── 细胞 ─────────────────────────────────────────

/// 传感器：输入采样 u32，输出 f64。
struct Sensor;
impl PortCell for Sensor {
    type In = u32;
    type Out = f64;
    type State = u64;
    fn step(s: &mut u64, x: u32) -> f64 {
        *s += 1;
        x as f64
    }
}

/// 归一化：值 / 100。
struct Normalize;
impl PortCell for Normalize {
    type In = f64;
    type Out = f64;
    type State = ();
    fn step(_: &mut (), x: f64) -> f64 {
        x / 100.0
    }
}

fn main() {
    // ═══ A. 单线程：InlineCarrier 栈上直接传（零分配、内联）═══
    type Pipe = Chain<Sensor, Normalize>; // Sensor(out f64) -> Normalize(in f64)
    let _ = core::marker::PhantomData::<Pipe>; // 类型即蓝图（编译期）

    // 用 InlineCarrier 驱动每一步：Sensor(300) -> 3.0；但 drive_link 是一次 A->B。
    let mut ss: u64 = 0; // Sensor::State
    let mut sn = (); // Normalize::State
    let a = drive_link::<Sensor, Normalize, InlineCarrier>(&mut ss, &mut sn, 300);
    println!("A. InlineCarrier 单线程: Sensor(300)->归一化 = {a} (零分配)");

    // ═══ B. 跨线程：spawned_flow 把 Normalize 放到工作线程 ═══
    // 同一逻辑：Sensor 在调用线程, Normalize 在专用工作线程处理（mpsc 通道）。
    let mut ss2: u64 = 0;
    let b = spawned_flow::<Sensor, Normalize>(&mut ss2, || (), 600);
    println!("B. spawned_flow 跨线程: Sensor(600)->归一化 = {b} (通道+独立线程)");

    // ═══ C. 编译期布线验证（drive_link 内含 Conforms<Wire>）═══
    assert_eq!(a, 3.0);
    assert_eq!(b, 6.0);
    println!("threaded_flow ok: 同一因果链, Inline(零分配) 与 跨线程通道(独立线程) 语义等价");
}
