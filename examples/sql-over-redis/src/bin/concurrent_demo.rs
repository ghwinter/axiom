//! 并发等待演示：多会话同时等待、单线程服务——"await 不阻塞 runtime 线程"的量化证据。
//!
//! - **async 路径**：`current_thread` 运行时（1 个工作线程），`SESSIONS` 个并发任务
//!   各自按上游节拍（`PACE`/行，模拟慢上游）等待命令到达，到达即被同一线程服务；
//! - **sync 对照**：`SESSIONS` 个 std 线程，各自 `sleep(PACE)` 等待后逐行执行；
//! - **输出**：两路径的墙钟、线程占用、吞吐。
//!
//! **平台计时器事实（M8 熵，实测记录）**：本 Windows 主机上 `tokio::time::sleep(2ms)`
//! 实际生效粒度 ≈ **15.6ms**（`thread::sleep(2ms)` ≈ 2.5ms）。因此默认节拍取 **20ms**
//! （高于两种粒度），使两路径的墙钟对比不被计时器粒度污染；观察 S 从 1→8 墙钟是否
//! 恒定（恒定 = 会话间等待重叠成立）。
//!
//! 环境变量（诊断/复现）：`AXIOM_DEMO_SESSIONS`、`AXIOM_DEMO_PACE_MS`、
//! `AXIOM_DEMO_RT`（current|multi）、`AXIOM_DEMO_MODE`（fed|plain|tmo）。
//!
//! 运行：`cargo run -p axiom-demo-sql-over-redis --features tokio --bin concurrent_demo`

#[cfg(feature = "tokio")]
fn run_tokio() {
    use axiom::cell_core::PortCell;
    use axiom_demo_sql_over_redis::composite::{self, ComposeLine};
    use axiom_demo_sql_over_redis::observe::{ObservedPoller, observed_fed_run};
    use axiom_runtime::async_seam::PollResult;
    use std::time::{Duration, Instant};

    const PER_SESSION: usize = 40;

    // 可调参数（环境变量；默认 8 会话 / 2ms 节拍）。
    let sessions_n: usize = std::env::var("AXIOM_DEMO_SESSIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let pace_ms: u64 = std::env::var("AXIOM_DEMO_PACE_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20); // 默认 20ms：高于本主机两种计时器粒度（~15.6ms / ~2.5ms）
    let pace = Duration::from_millis(pace_ms);

    let corpus = composite::build_corpus(PER_SESSION);
    let per_session = corpus.len(); // 实际每会话行数（语料生成密度决定）
    let sessions: Vec<Vec<String>> = (0..sessions_n).map(|_| corpus.clone()).collect();
    let total = sessions_n * per_session;
    println!(
        "=== 并发等待演示: {sessions_n} 会话 × {per_session} 行, 上游节拍 {:.1}ms/行 ===",
        pace.as_secs_f64() * 1000.0
    );

    // ── async:运行时种类可切换（AXIOM_DEMO_RT=multi → 多线程；默认 current_thread）──
    let rt_kind = std::env::var("AXIOM_DEMO_RT").unwrap_or_else(|_| "current".into());
    let rt = if rt_kind == "multi" {
        tokio::runtime::Runtime::new().expect("multi-thread rt")
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("current-thread rt + time driver")
    };
    let t0 = Instant::now();
    let served_async: usize = rt.block_on(async {
        let mut handles = Vec::new();
        for lines in sessions.clone() {
            handles.push(tokio::spawn(async move {
                // 本会话的上游生产者：按节拍投递命令（先克隆，防 lines 被移走）。
                let to_send = lines.clone();
                let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
                tokio::spawn(async move {
                    for l in to_send {
                        tokio::time::sleep(pace).await;
                        let _ = tx.send(l.clone()).await;
                    }
                });
                // 本会话消费者：命令到达即被同一线程服务（等待期间不占线程）。
                // AXIOM_DEMO_MODE：fed（默认，观测式馈入）/ plain（纯 recv）/ tmo（仅 timeout+recv）。
                let mode = std::env::var("AXIOM_DEMO_MODE").unwrap_or_else(|_| "fed".into());
                let mut served = 0usize;
                let deadline = Instant::now() + Duration::from_secs(10);
                if mode == "plain" {
                    while let Some(_l) = rx.recv().await {
                        served += 1;
                        if served >= lines.len() {
                            break;
                        }
                    }
                } else if mode == "tmo" {
                    for _ in 0..lines.len() {
                        let r = tokio::time::timeout(deadline - Instant::now(), rx.recv()).await;
                        if r.is_ok() {
                            served += 1;
                        }
                    }
                } else {
                    let mut obs = ObservedPoller::<ComposeLine>::new(
                        composite::new_composite_state(),
                        None,
                    );
                    for _ in 0..lines.len() {
                        let r = observed_fed_run(
                            &mut obs,
                            &mut rx,
                            deadline,
                            Duration::from_millis(1),
                        )
                        .await;
                        if let PollResult::Ready(_) = r {
                            served += 1;
                        }
                    }
                }
                served
            }));
        }
        let mut total_served = 0usize;
        for h in handles {
            total_served += h.await.expect("session task");
        }
        total_served
    });
    let wall_async = t0.elapsed();
    assert_eq!(served_async, total, "async 全量服务");

    // ── sync 对照:N 线程,各自 sleep 等待后执行 ──
    let t1 = Instant::now();
    let mut handles = Vec::new();
    for lines in sessions.clone() {
        handles.push(std::thread::spawn(move || {
            let mut st = composite::new_composite_state();
            let mut served = 0usize;
            for l in &lines {
                std::thread::sleep(pace); // 模拟等待（慢上游；阻塞线程）
                let _ = ComposeLine::step(&mut st, l.clone()); // 执行（计数口径 = 行）
                served += 1;
            }
            served
        }));
    }
    let served_sync: usize = handles.into_iter().map(|h| h.join().expect("sync session")).sum();
    let wall_sync = t1.elapsed();
    assert_eq!(served_sync, total, "sync 全量处理（计数口径为行）");

    println!(
        "async : 服务 {} 行, 墙钟 {:.1} ms, 运行时 {rt_kind}({} 线程)",
        served_async,
        wall_async.as_secs_f64() * 1000.0,
        if rt_kind == "multi" { "多" } else { "1" }
    );
    println!(
        "sync  : 处理 {} 行, 墙钟 {:.1} ms, 占用线程 {sessions_n}",
        total,
        wall_sync.as_secs_f64() * 1000.0
    );
    println!(
        "       async 吞吐 {:.0} 行/s；sync 吞吐 {:.0} 行/s",
        served_async as f64 / wall_async.as_secs_f64(),
        total as f64 / wall_sync.as_secs_f64()
    );
    println!(
        "\n解读: S 从 1→8 墙钟是否恒定 ⇒ 会话间等待重叠成立（async 单线程服务 N 会话,\n     sync 需 N 线程）。本主机计时器粒度: tokio ~15.6ms / thread::sleep ~2.5ms\n     （M8 观测缺损已录;节拍须高于粒度对比才不被污染）。本演示只量化服务能力\n     与线程占用;行为等价由 composite_pair 对拍保持。"
    );
}

fn main() {
    #[cfg(feature = "tokio")]
    run_tokio();
    #[cfg(not(feature = "tokio"))]
    eprintln!("skip: requires `tokio` feature; run with `--features tokio`");
}
