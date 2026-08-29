//! 综合用例 async 演示：SQL-over-Redis 计划经**观测式**真异步馈入驱动（tokio）。
//!
//! 运行：`cargo run -p axiom-demo-sql-over-redis --features tokio --bin async_demo`
//!
//! 演示：① 观测子系统（三段式：`ObservedPoller` 收集 → `ObsSummary` 提交 →
//! `print_summary` 打印；观测面属用例侧，非通用）；② 真异步馈入驱动
//! `observed_fed_run`——命令经 tokio mpsc 在等待窗内异步抵达；③ 期限由**真定时器**
//! 驱动（`timeout` 包裹 `recv`，Timeout 升模态承载）；④ **T6 断言**：观测式 async
//! 输出与 sync inline 全行（含错误行）逐行等价——观测不改变行为（透明性证据）；
//! ⑤ 多线程运行时组合。

#[cfg(feature = "tokio")]
fn run_tokio() {
    use axiom_demo_sql_over_redis::composite::{self, ComposeLine};
    use axiom_demo_sql_over_redis::observe::{ObservedPoller, observed_fed_run, print_summary};
    use axiom_semantics::seams::async_seam::PollResult;
    use axiom_semantics::drive::flow::drive_seq;
    use std::time::{Duration, Instant};

    let lines = composite::build_corpus(60);
    println!("=== SQL-over-Redis async 演示（corpus={}） ===", lines.len());

    // sync 参考：inline 全行。
    let mut sst = composite::new_composite_state();
    let sync_outs: Vec<String> =
        drive_seq::<ComposeLine, String, String, Vec<String>>(&mut sst, lines.clone());

    // 观测式异步馈入：同一命令序列经 tokio mpsc 在等待窗内送达；每拍经观察器记录。
    let rt = tokio::runtime::Runtime::new().expect("multi-thread rt + time driver");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    let to_send = lines.clone();
    rt.spawn(async move {
        for l in to_send {
            let _ = tx.send(l).await;
        }
    });
    let mut obs = ObservedPoller::<ComposeLine>::new(composite::new_composite_state(), None);
    let async_outs: Vec<String> = rt.block_on(async {
        let mut outs = Vec::new();
        for _ in 0..lines.len() {
            let t0 = Instant::now();
            let r = observed_fed_run(
                &mut obs,
                &mut rx,
                Instant::now() + Duration::from_millis(10_000),
                Duration::from_millis(1),
            )
            .await;
            match r {
                PollResult::Ready(o) => {
                    obs.observe_ready(&o, t0);
                    outs.push(o);
                }
                other => outs.push(format!("-INTERNAL {other:?}")),
            }
        }
        outs
    });

    // T6：观测不改行为——全行（含错误行）逐行等价。
    assert_eq!(
        async_outs, sync_outs,
        "T6：观测式 async 馈入与 sync inline 全行等价"
    );
    println!(
        "      T6: async({}) == sync inline({}) 逐行等价（含错误行；观测透明）",
        async_outs.len(),
        sync_outs.len()
    );
    let ok = async_outs.iter().filter(|r| !r.starts_with("-ERR")).count();
    println!(
        "      账本: 总 {} → ok {ok} / err {}",
        async_outs.len(),
        async_outs.len() - ok
    );
    for (line, resp) in lines.iter().zip(async_outs.iter()).take(6) {
        println!("        < {line:>28} => {resp}");
    }

    // 提交 + 打印：观测汇总经打印模块输出（三段式的输出目的地）。
    print_summary("async-fed", obs.summary());
    println!("\nSQL-over-Redis async ok: 观测式真异步馈入 + 真定时器期限 + T6 等价 + 观测子系统");
}

fn main() {
    #[cfg(feature = "tokio")]
    run_tokio();
    #[cfg(not(feature = "tokio"))]
    eprintln!("skip: requires `tokio` feature; run with `--features tokio`");
}