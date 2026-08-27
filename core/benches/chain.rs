//! 基于新核心（cell_core 四构件）的基准。
//!
//! 目的：作为**构建用例**保留性能基准，衡量新核心的零成本承诺。
//! 用内置最小计时（零外部依赖），对比：
//! - **静态展开**（`Chain<A,B>` 直接 `step`——全类型参数化、单态化、零分配）：
//!   axiom 主张"编译后等价手写普通 Rust"；
//! - **手写等价循环**（基线）；
//! - **类型擦除模拟**（`Box<dyn Any>` 每消息装箱）——体现动态税，证明零成本的价值。
//!
//! 方法学与 `dag.rs` 一致（见 `bench_common.rs`）：预热 → 轮换交错 → min-of-N，
//! 并附手写基线自噪声底——单次计时曾观测 2.7~6.1% 的顺序性波动（测量伪影），
//! 已废弃。
//!
//! 运行：`cargo bench --bench chain`（harness = false；release 下数字才有意义）。
//!
//! Bench 是测量脚本：debug 构建（`--all-targets`）下测量体被 cfg 移除，其引用的
//! 类型/函数在 debug 视角"未使用"属预期，故文件级允许 dead_code/unused_imports。

#![cfg_attr(debug_assertions, allow(dead_code, unused_imports))]

#[path = "bench_common.rs"]
mod bench_common;

use std::hint::black_box;

use axiom::cell_core::{Chain, PortCell};

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

/// A. 静态展开：Chain<Inc, Scaler> 单态化路径。
fn body_static(st: &mut <Chain<Inc, Scaler> as PortCell>::State, n: usize) -> i64 {
    let mut acc: i64 = 0;
    for i in 0..n {
        acc ^= black_box(<Chain<Inc, Scaler> as PortCell>::step(st, black_box(i as i32))) as i64;
    }
    acc
}

/// B. 手写等价循环（基线）。
fn body_hand(a: &mut (), b: &mut (), n: usize) -> i64 {
    let mut acc: i64 = 0;
    for i in 0..n {
        acc ^= black_box(Scaler::step(b, Inc::step(a, black_box(i as i32)))) as i64;
    }
    acc
}

/// C. 类型擦除模拟（每消息装箱）——动态税的下界体现。
fn body_erased(sa: &mut (), sb: &mut (), n: usize) -> i64 {
    let mut acc: i64 = 0;
    for i in 0..n {
        let boxed: Box<dyn core::any::Any + Send> =
            Box::new(Inc::step(sa, black_box(i as i32)));
        let unboxed: Box<i32> = boxed.downcast::<i32>().unwrap();
        acc ^= black_box(Scaler::step(sb, *unboxed)) as i64;
    }
    acc
}

fn main() {
    // Evidence hygiene (see file header): unoptimized numbers are meaningless,
    // so the whole measurement body is cfg'd out under --all-targets/debug.
    #[cfg(debug_assertions)]
    println!("chain: debug build — benchmark skipped; run `cargo bench --bench chain`");
    #[cfg(not(debug_assertions))]
    run();
}

fn run() {
    use bench_common::{ITERS, ROUNDS, WARMUP_PASSES, measure3, noise_floor_pct, pct_over};

    // Correctness gate (and an extra warmup pass): all variants must agree.
    let mut st_static = <Chain<Inc, Scaler> as PortCell>::State::default();
    let (mut ha, mut hb) = ((), ());
    let (mut ea, mut eb) = ((), ());
    let rc = body_static(&mut st_static, ITERS);
    let rh = body_hand(&mut ha, &mut hb, ITERS);
    assert_eq!(rc, rh, "static and handwritten must agree bit-exactly");

    let [ns_s, ns_h, ns_e] = measure3(
        || {
            body_static(&mut st_static, ITERS);
        },
        || {
            body_hand(&mut ha, &mut hb, ITERS);
        },
        || {
            body_erased(&mut ea, &mut eb, ITERS);
        },
    );

    // Self-noise floor: rerun the handwritten baseline in an independent set.
    let floor = noise_floor_pct(ns_h, || {
        body_hand(&mut ha, &mut hb, ITERS);
    });

    let delta = pct_over(ns_s, ns_h);
    let tax_x = ns_e as f64 / ns_h as f64;
    println!(
        "chain methodology: warmup {WARMUP_PASSES}×3, {ROUNDS} interleaved rounds, {ITERS} msgs/pass, statistic = min"
    );
    println!(
        "chain[static]      {:>7.3} ns/msg  (min {} µs/pass)",
        ns_s as f64 / ITERS as f64,
        ns_s / 1000
    );
    println!(
        "chain[handwritten] {:>7.3} ns/msg  (min {} µs/pass)",
        ns_h as f64 / ITERS as f64,
        ns_h / 1000
    );
    println!(
        "chain[type-erase]  {:>7.3} ns/msg  ({tax_x:.1}× dynamic-tax contrast)",
        ns_e as f64 / ITERS as f64
    );
    println!(
        "chain verdict: Δ(static−handwritten) = {delta:+.2}% | self-noise ±{floor:.2}% | gate <5% => {}",
        if delta.abs() <= 5.0 { "PASS" } else { "FAIL" }
    );
}
