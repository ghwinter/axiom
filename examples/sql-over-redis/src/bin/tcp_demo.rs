//! TCP 服务器演示（网络异步 I/O）——SQL-over-Redis 组合经真实套接字服务。
//!
//! 同一蓝图在 sync/async 两物理域各起一个服务器，同一命令流驱动同一客户端脚本，
//! 断言三方零分歧：**inline 组合迹 == sync 线上应答 == async 线上应答**（系统级 T6），
//! 外加线上配对律（应答数 = 命令数）。
//!
//! 运行：`cargo run -p axiom-demo-sql-over-redis --features tokio --bin tcp_demo [corpus] [port]`
//! - `corpus`：语料行数（默认 48）；
//! - `port`：监听端口（默认 0 = 临时端口，两个服务器各取一个）。

#[cfg(not(feature = "tokio"))]
fn main() {
    // 默认特性下本 bin 无 tokio 可用：提示启用 tokio 特性。
    println!("tcp_demo 需要 --features tokio：cargo run -p axiom-demo-sql-over-redis --features tokio --bin tcp_demo");
}

/// turmoil 仿真域与演示不兼容：演示绑定实际网络监听器，仿真域 net 类型只在
/// 确定性仿真内可构造（`tests/tcp_sim_adversarial.rs`）。同时启用两 feature 时给出提示。
#[cfg(all(feature = "tokio", feature = "turmoil"))]
fn main() {
    println!("tcp_demo 需要实际网络域：仅启用 tokio 特性（--features tokio，不带 turmoil）");
}

#[cfg(all(feature = "tokio", not(feature = "turmoil")))]
fn main() {
    let n = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(48);
    let port: u16 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let lines = axiom_demo_sql_over_redis::composite::build_corpus(n);
    println!("=== TCP 服务器演示（corpus={n}，sync/async 两物理域，T6 交叉验证） ===");

    // ① inline 参照：组合核心直接执行（带终止符 → 剥离同口径）。
    let inline: Vec<String> = {
        use axiom_demo_sql_over_redis::composite::ComposeLine;
        use axiom_semantics::drive::flow::drive_seq;
        let mut st = axiom_demo_sql_over_redis::composite::new_composite_state();
        drive_seq::<ComposeLine, String, String, Vec<String>>(&mut st, lines.clone())
    };
    let inline: Vec<String> = inline
        .iter()
        .map(|r| r.trim_end_matches("\r\n").to_string())
        .collect();

    // ② async 服务器（tokio，每连接事件泵）。
    let rt = tokio::runtime::Runtime::new().expect("rt");
    let async_addr = rt.block_on(async {
        let l = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind");
        let a = l.local_addr().expect("local addr");
        let store = axiom_demo_sql_over_redis::composite::new_composite_state();
        tokio::spawn(async move {
            let _ = axiom_demo_sql_over_redis::tcp_server::serve_tcp_async(l, store).await;
        });
        a
    });

    // ③ sync 服务器（std 线程）。
    let sync_listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind");
    let sync_addr = sync_listener.local_addr().expect("local addr");
    let store = axiom_demo_sql_over_redis::composite::new_composite_state();
    std::thread::spawn(move || {
        axiom_demo_sql_over_redis::tcp_server::serve_tcp_sync(sync_listener, store)
    });

    // ④ 同一命令流驱动同一客户端脚本（物理域无关）。
    let sync_out = axiom_demo_sql_over_redis::tcp_server::client_run(sync_addr, &lines);
    let async_out = axiom_demo_sql_over_redis::tcp_server::client_run(async_addr, &lines);

    // ⑤ 系统级 T6 断言（三方零分歧 + 配对律）。
    assert_eq!(sync_out.len(), lines.len(), "sync 配对律：应答数 = 命令数");
    assert_eq!(async_out.len(), lines.len(), "async 配对律：应答数 = 命令数");
    assert_eq!(sync_out, inline, "sync 线上应答 == inline 组合迹");
    assert_eq!(async_out, inline, "async 线上应答 == inline 组合迹");
    assert_eq!(sync_out, async_out, "T6：sync/async 应答流零分歧");

    // ⑥ 摘要 + 样本。
    let ok_ct = inline.iter().filter(|r| !r.starts_with("-ERR")).count();
    println!(
        "      async 服务器 {async_addr} / sync 服务器 {sync_addr}: 应答 {}/{}（ok {ok_ct}）",
        async_out.len(),
        lines.len()
    );
    for (line, resp) in lines.iter().zip(inline.iter()).take(10) {
        println!("        < {line:>28} => {resp}");
    }
    if lines.len() > 10 {
        println!("        … 其余 {} 条省略", lines.len() - 10);
    }

    println!("\nTCP 服务器 ok: 事件接缝实际网络域 + 系统级 T6 三方交叉验证 + 线上配对律");
}
