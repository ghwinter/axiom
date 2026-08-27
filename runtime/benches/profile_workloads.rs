//! 每剖面代表性工作负载形状（C10）：五个剖面各一个稳态场景，测 ns/op 与分配面。
//!
//! 形状（命题 7.1 表的基准化）：
//! - **kernel**：Inline 微任务爆发（零分配预算下的密集 step 链）；
//! - **embedded**：BoundedRing 稳态推拉（构造期预留后零分配）；
//! - **service**：BoundedMailbox 三模式混合（吞吐＋保底席位）；
//! - **tool**：冷启动单遍（构造＋一次驱动的端到端延迟）；
//! - **game**：帧批处理（一批 step 的每帧成本）。
//!
//! 方法学同 dynamic_tax 的简化形态：预热＋min-of-N；**不含自噪声底计算**
//! （形状基准，如实行：仅报整轮 best；噪声面与阈值语义见 dynamic_tax/
//! benchmark_common）。debug 自跳过。
//! 运行：`cargo bench --manifest-path runtime/Cargo.toml --bench profile_workloads`。

#![cfg(feature = "std")]

use axiom::cell_core::PortCell;
use axiom_runtime::movers::mailbox::BoundedMailbox;
use axiom_runtime::movers::ring::BoundedRing;
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

const ITERS: u64 = 500_000;

fn timed<F: FnMut()>(name: &str, mut f: F) {
    // 预热
    f();
    f();
    let mut best = u64::MAX;
    for _ in 0..5 {
        let t0 = Instant::now();
        f();
        let d = t0.elapsed().as_nanos() as u64;
        if d < best {
            best = d;
        }
    }
    println!("  {name:<30} {:>10} ns", best);
}

fn main() {
    if cfg!(debug_assertions) {
        println!("跳过：debug 构建。请用 cargo bench --release。");
        return;
    }
    println!("== 剖面工作负载形状，ITERS={ITERS} ==");

    // kernel：内联微任务爆发（稳态零分配）。
    timed("kernel: inline burst", || {
        let mut s1 = 0i32;
        let mut s2 = 0i32;
        let mut acc = 0i32;
        for i in 0..ITERS {
            acc ^= Inc::step(&mut s2, Inc::step(&mut s1, i as i32));
        }
        let _ = std::hint::black_box(acc);
    });

    // embedded：BoundedRing 稳态推拉。
    timed("embedded: ring steady-state", || {
        let mut r = BoundedRing::<i32, 64>::new();
        let mut acc = 0i32;
        for i in 0..ITERS {
            let _ = r.push(i as i32);
            if let Ok(v) = r.pop() {
                acc ^= v;
            }
        }
        let _ = std::hint::black_box(acc);
    });

    // service：BoundedMailbox 吞吐（producer.try_send + mailbox.try_recv 交替）。
    timed("service: mailbox throughput", || {
        let mb = BoundedMailbox::<i32, 64>::new();
        let producer = mb.producer();
        let mut acc = 0i32;
        for i in 0..(ITERS / 2) {
            let _ = producer.try_send(i as i32);
            if let axiom_runtime::checks::delivery::Receipt::Item(v) = mb.try_recv() {
                acc ^= v;
            }
        }
        let _ = std::hint::black_box(acc);
    });

    // tool：冷启动单遍（构造＋单次驱动的端到端）。
    timed("tool: cold start x100k", || {
        for _ in 0..100_000u32 {
            let mut s = 0i32;
            let v = Inc::step(&mut s, 41);
            let _ = std::hint::black_box(v);
        }
    });

    // game：帧批处理（一帧 = 4096 次 step；报告每帧成本）。
    const FRAME: u64 = 4096;
    timed("game: frame batch (4k steps)", || {
        let mut s = 0i32;
        for f in 0..(ITERS / FRAME) {
            let mut frame_acc = 0i32;
            for k in 0..FRAME {
                frame_acc ^= Inc::step(&mut s, ((f * FRAME + k) % 127) as i32);
            }
            let _ = std::hint::black_box(frame_acc);
        }
    });
}
