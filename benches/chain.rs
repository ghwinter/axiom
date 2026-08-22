//! 基于新核心（cell_core 四构件）的基准。
//!
//! 目的：作为**构建用例**保留性能基准，衡量新核心的零成本承诺。
//! 用内置最小计时（零外部依赖），对比：
//! - **静态展开**（`Chain<A,B>` 直接 `step`——全类型参数化、单态化、零分配）：
//!   axiom 主张"编译后等价手写普通 Rust"；
//! - **手写等价循环**（基线）；
//! - **类型擦除模拟**（`Box<dyn Any>` 每消息装箱）——体现动态税，证明零成本的价值。
//!
//! 运行：`cargo bench --bench chain`（`cargo bench` 需要 nightly 或 harness=false + main）。
//!
//! Bench 是测量脚本：debug 构建（`--all-targets`）下测量体被 cfg 移除，其引用的
//! 类型/函数在 debug 视角"未使用"属预期，故文件级允许 dead_code/unused_imports。

#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]

use axiom::cell_core::{Chain, PortCell};

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
    // Evidence hygiene (see file header): unoptimized numbers are meaningless,
    // so the whole measurement body is cfg'd out under --all-targets/debug.
    #[cfg(debug_assertions)]
    println!("chain: debug build — benchmark skipped; run `cargo bench --bench chain`");
    #[cfg(not(debug_assertions))]
    run();
}

#[cfg(not(debug_assertions))]
fn run() {
    const N: usize = 1_000_000;

    // A. 静态展开：Chain<Inc, Scaler>（零分配、内联）
    let mut st = <Chain<Inc, Scaler> as PortCell>::State::default();
    let t0 = now();
    let mut acc = 0i32;
    for i in 0..N {
        acc ^= <Chain<Inc, Scaler> as PortCell>::step(&mut st, i as i32);
    }
    let t_static = now() - t0;
    println!("chain: 静态展开(零分配) {N} = {t_static:?} (acc {acc:#x})");

    // B. 手写等价循环（基线）
    let (mut a, mut b) = ((), ());
    let t1 = now();
    let mut acc = 0i32;
    for i in 0..N {
        acc ^= Scaler::step(&mut b, Inc::step(&mut a, i as i32));
    }
    let t_hand = now() - t1;
    println!("chain: 手写等价循环   {N} = {t_hand:?} (acc {acc:#x})");

    // C. 类型擦除模拟（Box<dyn Any> 每消息装箱）——动态税的下界体现
    let mut sa2 = ();
    let t2 = now();
    let mut acc = 0i32;
    for i in 0..N {
        let boxed: Box<dyn core::any::Any + Send> = Box::new(Inc::step(&mut sa2, i as i32));
        let unboxed: Box<i32> = boxed.downcast::<i32>().unwrap();
        acc ^= Scaler::step(&mut b, *unboxed);
    }
    let t_erase = now() - t2;
    println!("chain: 类型擦除(每消息装箱) {N} = {t_erase:?} (acc {acc:#x})");

    println!("零成本对照: 静态≈手写(差={:?}), 擦除税(差={:?})",
        t_static.saturating_sub(t_hand),
        t_erase.saturating_sub(t_static));
}

fn now() -> std::time::Instant {
    std::time::Instant::now()
}
