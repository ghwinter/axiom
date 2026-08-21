//! cell_core 四构件蓝图——作为普通 Rust 程序运行（编译后等价手写、无运行时对象）。
//!
//! 演示 axiometric 重构后的新核心：用"开放系统 + 因果数据流 + 组合 + 静态性声明"
//! 定义一个含状态、广播、反馈的小系统，**直接作为普通 Rust 调用运行**。
//! 没有 Box<dyn>、没有 JSON 蓝图、没有运行时模块对象——蓝图即类型，编译期耗尽。

use axiom::cell_core::{Broadcast, Chain, Feedback};

// ── 有状态的开放系统（端口体）────────────────────────────

/// 计数器：输入加 `n`，状态累加，输出累加和。有真实 State（非 ()）。
struct Counter;

impl axiom::cell_core::PortCell for Counter {
    type In = i32;
    type Out = i32;
    type State = i32; // 累加器
    fn step(s: &mut i32, x: i32) -> i32 {
        *s += x;
        *s
    }
}

/// 乘法器：固定放大系数（演示不同端口体）。
struct Double;
impl axiom::cell_core::PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = ();
    fn step(_: &mut (), x: i32) -> i32 {
        x * 2
    }
}

fn main() {
    // 蓝图（类型层次）：Counter -> (Double 与 Counter' 广播) -> Double，再经反馈闭合。
    // —— 全部是类型，无运行时对象。

    // 1. 状态化链：Counter -> Double（Counter 输出放大）。
    type Stage = Chain<Counter, Double>;
    let mut st_stage = <Stage as axiom::cell_core::PortCell>::State::default();

    // 2. 广播：一个内联细胞（Inc-like）的输出同时给两个接收者。
    type Fan = Broadcast<Counter, Counter, Double>;
    let (mut ssrc, mut sr1, mut sr2) = (0i32, 0i32, ());

    // 3. 反馈：Body=Stage, Feed=Double（类型闭合）。
    type Loop = Feedback<Stage, Double>;
    let (mut sbody, mut sfeed) = (
        <Stage as axiom::cell_core::PortCell>::State::default(),
        (),
    );

    // 作为普通 Rust 调用运行（无任何运行时"axiom 对象"）。
    let stage_out = axiom::cell_core::drive::<Stage>(&mut st_stage, 5);
    println!("Stage(Counter,Double)(5) = {stage_out}"); // Counter 5 = 5, Double 5*2 = 10

    let fan_out = Fan::fire(&mut ssrc, &mut sr1, &mut sr2, 7);
    // Counter 7 -> 7; r1 Counter(7) -> 7; r2 Double(7) -> 14
    println!("Broadcast(Counter,Counter,Double)(7) = {fan_out:?}");

    let loop_out = Loop::tick(&mut sbody, &mut sfeed, 1);
    println!("Feedback(Stage,Double)(1) = {loop_out}");

    // 断言编译期布线验证可调用（验证在编译期完成，运行期零开销）。
    axiom::cell_core::assert_wiring::<Counter, Double>();
    println!("cell_demo ok: 四构件蓝图直接作为普通 Rust 程序运行（无运行时对象）");
}
