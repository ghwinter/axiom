//! 动态税基准（C9）：同一拓扑「静态单态化 vs 型位擦除」的差值测量。
//!
//! 拓扑恒定：`Inc -> Double`（i32）。四个通道：
//! - **A 手写基线**：`Double::step(&mut (), Inc::step(&mut (), x))`——零成本承诺的参照系；
//! - **B 静态泛型驱动**：`drive_link::<Inc, Double, InlineCarrier>`——单态化，应≈A；
//! - **C 型位擦除驱动**：`SlotDrive::drive`——函数指针间接＋`downcast_mut`
//!   （slot.rs 成本声明：PerInstallAlloc＋每驱动一次间接）；
//! - **D 席位擦除驱动**：`Seat::drive`——C ＋ 代一致性比较。
//!
//! 另测换装税：`swap::<T>` 的分配与指针改写速率（每次一次堆分配）。
//!
//! 已知读数特性：通道 B（静态泛型）对代码布局敏感——跨构建 Δ 在 ~0% 与 ~+50% 间双峰；
//! 通道 C/D（型位/席位擦除）为本基准的稳定结论：每驱动 ≈ +2.0 ns/op（间接＋downcast），
//! Seat 另 +0.25 ns/op（代比较）；swap ≈ 30 ns/次。
//! 方法学（与核心 crate 基准同纪律）：预热 2×3；8 轮取 min-of-N；自带自噪声底
//! （同代码双份测量之差）；debug 构建自跳过。安全代码，无 unsafe。
//! 运行：`cargo bench --manifest-path runtime/Cargo.toml --bench dynamic_tax`。

#![cfg(feature = "std")]

use axiom::cell_core::PortCell;
use axiom_runtime::movers::carrier::InlineCarrier;
use axiom_runtime::drive::flow::drive_link;
use axiom_runtime::drive::slot::{SlotDrive, SlotPending};
use std::time::Instant;

struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = i32;
    #[inline(always)]
    fn step(s: &mut i32, x: i32) -> i32 {
        *s = s.wrapping_add(x);
        x.wrapping_add(1)
    }
}

struct Double;
impl PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = i32;
    #[inline(always)]
    fn step(s: &mut i32, x: i32) -> i32 {
        *s = s.wrapping_mul(3);
        x.wrapping_mul(3)
    }
}

const ITERS: u64 = 1_000_000;
const ROUNDS: u64 = 8;
const WARMUP_ROUNDS: u64 = 2;

fn measure_round(f: &mut Box<dyn FnMut(i32) -> i32 + '_>) -> u64 {
    let t0 = Instant::now();
    let mut acc = 0i32;
    for i in 0..ITERS {
        acc ^= f(std::hint::black_box(i as i32));
    }
    let _ = std::hint::black_box(acc);
    t0.elapsed().as_nanos() as u64
}

/// 轮换交错取 min-of-N：`$mk` 每轮重新求值产出新鲜闭包（构造在计时区外）。
/// 宏展开使每通道闭包借用驻留于各自作用域，避免高阶 trait 对象生命周期。
macro_rules! bench_chan {
    ($name:expr, $baseline:expr, $mk:block) => {{
        for _ in 0..WARMUP_ROUNDS {
            let mut f: Box<dyn FnMut(i32) -> i32> = $mk;
            measure_round(&mut f);
        }
        let mut best = u64::MAX;
        for _ in 0..ROUNDS {
            let mut f: Box<dyn FnMut(i32) -> i32> = $mk;
            let d = measure_round(&mut f);
            if d < best {
                best = d;
            }
        }
        match $baseline {
            Some(b) => println!(
                "  {:<26} {:>9} ns/round ({:>6.2} ns/op)  Δ{:>+8.0} ns ({:>+6.2}%)",
                $name,
                best,
                best as f64 / ITERS as f64,
                best as f64 - b as f64,
                (best as f64 - b as f64) * 100.0 / b.max(1) as f64
            ),
            None => println!(
                "  {:<26} {:>9} ns/round ({:>6.2} ns/op)",
                $name,
                best,
                best as f64 / ITERS as f64
            ),
        }
        best
    }};
}

fn main() {
    if cfg!(debug_assertions) {
        println!("跳过：debug 构建（优化缺失使结果无意义）。请用 cargo bench --release。");
        return;
    }

    println!("== 动态税基准：Inc->Double，ITERS={ITERS}，ROUNDS={ROUNDS} ==");

    // 自噪声底：同一手写通道测两次，差值即方法噪声量级。
    let base_a = bench_chan!("noise-floor (handwritten)", None::<u64>, {
        { let mut s1 = 0i32; let mut s2 = 0i32; Box::new(move |x| Double::step(&mut s2, Inc::step(&mut s1, x))) }
    });
    let base = bench_chan!("A hand-written baseline", None::<u64>, {
        { let mut s1 = 0i32; let mut s2 = 0i32; Box::new(move |x| Double::step(&mut s2, Inc::step(&mut s1, x))) }
    });
    let noise = base_a.abs_diff(base);
    println!(
        "  noise floor                 {:>9} ns ({:>4.1}%)\n",
        noise,
        noise as f64 * 100.0 / base.max(1) as f64
    );

    // B 静态泛型驱动。
    let b = {
        let mut sa = 0i32;
        let mut sb = 0i32;
        bench_chan!("B drive_link<Inline>", Some(base), {
            let a = &mut sa;
            let s = &mut sb;
            Box::new(move |x| drive_link::<Inc, Double, InlineCarrier>(a, s, x))
        })
    };

    // C 型位擦除驱动。
    let c = {
        let mut live: SlotDrive<i32, i32> = SlotPending::<i32, i32>::install::<Double>(7).commit();
        bench_chan!("C SlotDrive (erased)", Some(base), {
            let l = &mut live;
            Box::new(move |x| l.drive(x))
        })
    };

    // D 席位擦除驱动（含代一致性比较）。
    let d = {
        let mut live: SlotDrive<i32, i32> = SlotPending::<i32, i32>::install::<Double>(7).commit();
        bench_chan!("D Seat (erased + gen)", Some(base), {
            let mut seat = live.seat();
            Box::new(move |x| seat.drive(x))
        })
    };

    let _ = (b, c, d);

    // 换装税：swap 的分配+指针改写速率（独立于每驱动税）。
    const SWAPS: u64 = 20_000;
    let mut live3: SlotDrive<i32, i32> = SlotPending::<i32, i32>::install::<Inc>(0).commit();
    let t0 = Instant::now();
    let mut g = 0u64;
    for i in 0..SWAPS {
        if i % 2 == 0 {
            live3.swap::<Double>(std::hint::black_box(11));
        } else {
            live3.swap::<Inc>(std::hint::black_box(22));
        }
        g = g.wrapping_add(live3.generation());
    }
    let _ = std::hint::black_box(&live3);
    let el = t0.elapsed();
    let _ = std::hint::black_box(g);
    // 阈值断言钩子（C14-A2）：钉扎硬件上设置 DYNAMIC_TAX_MAX_NS_OP（ns/op）即可回归监视擦除缝。
    if let Ok(mx) = std::env::var("DYNAMIC_TAX_MAX_NS_OP") {
        let mx: f64 = mx.parse().expect("DYNAMIC_TAX_MAX_NS_OP 应为数字");
        let per_op = c as f64 / ITERS as f64;
        assert!(per_op <= mx, "erased seam per-touch tax {per_op:.2} ns/op exceeds threshold {mx:.2}");
    }

    println!(
        "-- 换装税 --\n  swap ×{SWAPS}: {:>8.2} ns/次（含 1 次堆分配）",
        el.as_nanos() as f64 / SWAPS as f64
    );
}
