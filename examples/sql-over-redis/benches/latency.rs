//! 同口径逐步时延基准（sync driver vs async fed driver；min-of-N + 自噪音下限）。
//!
//! 方法论（bench_common 纪律，此处内联复刻）：
//! 1. **预热**：每变体先跑数轮不计时（冷缓存/懒惰页错误不得偏向先跑者）；
//! 2. **交错轮转**：R 轮内轮转起始变体（残余位置/漂移均摊）；
//! 3. **min-of-N**：每变体取最小通过时间（噪音只会加时，最小值收敛到稳态成本）；
//! 4. **自噪音下限**：基准变体（sync）独立重测一轮 min-of-N，其 |Δ|% 即本测量
//!    自身不确定度——头条增量必须显著超过此下限方可主张。
//!
//! 口径：每个 pass = 同一语料全行处理。sync 直接调 `ComposeLine::step`；async 经
//! 通道馈入 `observed_fed_run`（含数据物化与 await 往返）。per-line = min / n。
//! 双变体均含每行输入物化（String 克隆），物化成本对称，差异即驱动机制本身。
//!
//! 运行：`cargo bench -p axiom-demo-sql-over-redis --features tokio --bench latency`

use axiom::cell_core::PortCell;
use axiom_demo_sql_over_redis::composite::{self, ComposeLine};
use std::time::Instant;

/// 不计时预热轮数。
const WARMUP: usize = 3;
/// 计时轮数（交错轮转）。
const ROUNDS: usize = 8;

fn time_pass(mut f: impl FnMut() -> usize) -> u128 {
    let t = Instant::now();
    let _ = f();
    t.elapsed().as_nanos()
}

fn main() {
    let lines = composite::build_corpus(600);
    let n = lines.len();
    println!("[bench] corpus lines = {n}");

    // sync 变体：直接调 step（含每行输入物化）。
    let mut sync_pass = || -> usize {
        let mut st = composite::new_composite_state();
        let mut s = 0usize;
        for l in &lines {
            s += ComposeLine::step(&mut st, l.clone()).len();
        }
        s
    };

    #[cfg(feature = "tokio")]
    let rt = tokio::runtime::Runtime::new().expect("multi-thread rt + time driver");

    #[cfg(feature = "tokio")]
    let mut async_pass = {
        use axiom_demo_sql_over_redis::observe::{ObservedPoller, observed_fed_run};
        use axiom_runtime::seams::async_seam::PollResult;
        use std::time::Duration;
        || -> usize {
            rt.block_on(async {
                // 生产者任务：把本语料按行投递（与综合用例 async 变体同构；含数据物化）。
                let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(16);
                let to_send = lines.clone();
                tokio::spawn(async move {
                    for l in to_send {
                        let _ = tx.send(l).await;
                    }
                });
                let mut obs =
                    ObservedPoller::<ComposeLine>::new(composite::new_composite_state(), None);
                let mut s = 0usize;
                for _ in 0..n {
                    let r = observed_fed_run(
                        &mut obs,
                        &mut rx,
                        Instant::now() + Duration::from_millis(10_000),
                        Duration::from_millis(1),
                    )
                    .await;
                    if let PollResult::Ready(o) = r {
                        s += o.len();
                    }
                }
                s
            })
        }
    };

    // 预热（不计时）。
    for _ in 0..WARMUP {
        sync_pass();
    }
    #[cfg(feature = "tokio")]
    for _ in 0..WARMUP {
        async_pass();
    }

    // 交错轮转 min-of-N。
    let mut ms = u128::MAX;
    #[cfg(feature = "tokio")]
    let mut ma = u128::MAX;
    for r in 0..ROUNDS {
        #[cfg(feature = "tokio")]
        {
            // 轮转起始变体：残差位置/漂移均摊。
            if r % 2 == 0 {
                ms = ms.min(time_pass(&mut sync_pass));
                ma = ma.min(time_pass(&mut async_pass));
            } else {
                ma = ma.min(time_pass(&mut async_pass));
                ms = ms.min(time_pass(&mut sync_pass));
            }
        }
        #[cfg(not(feature = "tokio"))]
        {
            let _ = r; // 轮数仅服务 tokio 模式的交错轮转；无 feature 时单变体无轮转
            ms = ms.min(time_pass(&mut sync_pass));
        }
    }

    println!(
        "[bench] sync  : min {ms} ns ({:.2} µs/行, n={n})",
        ms as f64 / n as f64 / 1000.0
    );
    #[cfg(feature = "tokio")]
    {
        println!(
            "[bench] async : min {ma} ns ({:.2} µs/行, n={n})",
            ma as f64 / n as f64 / 1000.0
        );
        println!(
            "[bench] async 相对 sync: +{:.1}%",
            (ma as f64 - ms as f64) / ms as f64 * 100.0
        );
        // 自噪音下限：sync 独立重测一轮 min-of-N。
        let mut ms2 = u128::MAX;
        for _ in 0..ROUNDS {
            ms2 = ms2.min(time_pass(&mut sync_pass));
        }
        let floor = (ms2 as f64 - ms as f64).abs() / ms as f64 * 100.0;
        println!(
            "[bench] 自噪音下限（sync 重测 |Δ|）: {floor:.1}% —— 增量须显著超过此下限方能主张"
        );
    }
    // 防消除：测量后跑一轮引用合计（不计时）。
    let s1 = sync_pass();
    #[cfg(feature = "tokio")]
    let s2 = async_pass();
    #[cfg(feature = "tokio")]
    println!("[bench] 防消除合计 = {s1:#x} / {s2:#x}");
    #[cfg(not(feature = "tokio"))]
    println!("[bench] 防消除合计 = {s1:#x}");
}