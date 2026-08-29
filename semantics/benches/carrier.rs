//! runtime 载体基准（构建用例）：衡量不同 Carrier 的时空成本。
//!
//! 对比：
//! - **InlineCarrier**（栈上函数传，零分配、单线程）；
//! - **spawned_flow**（跨线程通道载体，mpsc + 独立线程——每消息同步 + 装箱）。
//!
//! 运行：`cargo bench --manifest-path runtime/Cargo.toml --bench carrier`。

use axiom::cell_core::PortCell;
use axiom_semantics::movers::carrier::{InlineCarrier, spawned_flow};
use axiom_semantics::drive::flow::drive_link;

struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 { x + 1 }
}

struct Scaler;
impl PortCell for Scaler {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 { x * 2 }
}

fn main() {
    const N: usize = 50_000;

    // A. InlineCarrier：零分配、单线程、内联
    let mut sa = ();
    let mut sb = ();
    let t0 = now();
    let mut acc = 0i32;
    for i in 0..N {
        acc ^= drive_link::<Inc, Scaler, InlineCarrier>(&mut sa, &mut sb, i as i32);
    }
    println!("carrier: Inline(零分配) {N} = {:?} (acc {acc:#x})", now() - t0);

    // B. spawned_flow：跨线程通道（每调用起一个 worker + mpsc 往返）
    let mut sa2 = ();
    let t1 = now();
    let mut acc = 0i32;
    for i in 0..N {
        acc ^= spawned_flow::<Inc, Scaler>(&mut sa2, || (), i as i32);
    }
    let t_worker = now() - t1;
    println!("carrier: spawned_flow(跨线程通道) {N} = {t_worker:?} (acc {acc:#x})");
    let _ = acc;

    // A 的 acc 未打印，避免影响计时；这里只对比时间。
}

fn now() -> std::time::Instant {
    std::time::Instant::now()
}
