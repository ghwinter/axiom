#![cfg(all(feature = "tokio", not(feature = "turmoil")))]
//! 系统级 T6 交叉验证（事件接缝·实际网络域）：同一组合核心（SQL-over-Redis）经实际 TCP
//! 套接字在 sync/async 两物理域服务，与 inline 组合迹三方零分歧。
//!
//! 判据（每层不变量互不干扰）：
//! - **配对律（线上可观测）**：应答数 = 命令数（每条命令恰一个应答，不静默丢）；
//! - **同命令流 → 同应答流**：每连接应答顺序 = 入队顺序（存储全局 FIFO）；
//! - **跨连接共享组合状态**：前连接的写对后连接可见（单属主存储，与 inline 一致）；
//! - **T6 多物理实现语义等价**：std 线程服务器 / tokio 任务服务器应答流零分歧。
//!
//! 判定独立性：客户端顺序驱动（前一连接读完并关闭，再开后一连接）→ 两域、三方的
//! 判定不依赖调度顺序（存储 FIFO + 每连接 FIFO 是结构性保证，非偶然）。

use axiom_demo_sql_over_redis::composite::{self, ComposeLine};
use axiom_demo_sql_over_redis::tcp_server::{client_run, serve_tcp_async, serve_tcp_sync};
use axiom_semantics::drive::flow::drive_seq;
use std::net::TcpListener as StdListener;

/// inline 参照：同一组合状态上逐连接执行（模拟服务器存储任务的全局 FIFO 次序）。
fn inline_with_shared_state(sets: &[&[String]]) -> Vec<Vec<String>> {
    let mut st = composite::new_composite_state();
    sets.iter()
        .map(|lines| {
            drive_seq::<ComposeLine, String, String, Vec<String>>(&mut st, lines.to_vec())
                .iter()
                .map(|r| r.trim_end_matches("\r\n").to_string())
                .collect()
        })
        .collect()
}

/// 起 sync 服务器（std 线程，监听临时端口）→ 地址。
fn start_sync() -> std::net::SocketAddr {
    let l = StdListener::bind("127.0.0.1:0").expect("bind");
    let addr = l.local_addr().expect("local addr");
    let store = composite::new_composite_state();
    std::thread::spawn(move || serve_tcp_sync(l, store));
    addr
}

/// 起 async 服务器（tokio 多线程运行时）。`_rt` 持有运行时至结构体析构——服务器存活
/// 于整个测试（运行时被丢弃即后台任务停止，故不可只返回地址）。
struct AsyncServer {
    _rt: tokio::runtime::Runtime,
    addr: std::net::SocketAddr,
}

fn start_async() -> AsyncServer {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("rt");
    let addr = rt.block_on(async {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = l.local_addr().expect("local addr");
        let store = composite::new_composite_state();
        tokio::spawn(async move { serve_tcp_async(l, store).await });
        addr
    });
    AsyncServer { _rt: rt, addr }
}

// ── 景一：单连接混合命令流（一次写入突发）三方零分歧 ─────────────────────────

#[test]
fn tcp_trace_matches_inline_and_across_physics() {
    let lines = composite::build_corpus(40); // KV + SQL + 各层错误，一次写入（流水线突发）
    let inline = inline_with_shared_state(&[&lines]);
    let expected = &inline[0];

    let sync_out = client_run(start_sync(), &lines);
    let async_out = client_run(start_async().addr, &lines);

    assert_eq!(sync_out.len(), lines.len(), "sync 配对律：应答数 = 命令数");
    assert_eq!(async_out.len(), lines.len(), "async 配对律：应答数 = 命令数");
    assert_eq!(sync_out, *expected, "sync 线上应答 == inline 组合迹（按序 FIFO）");
    assert_eq!(async_out, *expected, "async 线上应答 == inline 组合迹（按序 FIFO）");
    assert_eq!(sync_out, async_out, "T6：sync/async TCP 服务器应答流零分歧");
}

// ── 景二：跨连接共享组合状态（前连接的写对后连接可见） ────────────────────────

#[test]
fn tcp_shared_store_across_conns_matches_inline() {
    let a: Vec<String> = ["SET shared 7", "INCR shared", "GET shared", "SET k2 2"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let b: Vec<String> = ["GET shared", "GET missing", "GET k2"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let expected = inline_with_shared_state(&[&a, &b]);

    let sync_addr = start_sync();
    let sync = vec![client_run(sync_addr, &a), client_run(sync_addr, &b)];

    let srv = start_async();
    let async_ = vec![client_run(srv.addr, &a), client_run(srv.addr, &b)];

    assert_eq!(sync, expected, "sync 跨连接共享存储 == inline（全局 FIFO 单属主）");
    assert_eq!(async_, expected, "async 跨连接共享存储 == inline（全局 FIFO 单属主）");
    assert_eq!(sync, async_, "T6：跨连接共享状态两物理域零分歧");
}
