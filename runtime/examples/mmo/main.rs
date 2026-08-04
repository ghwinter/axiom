//! # MMO 核心子图 — 装配与驱动
//!
//! 三种运行模式：
//!
//! ```text
//! cargo run --manifest-path runtime/Cargo.toml --example mmo             # TCP 服务器（默认）
//! cargo run --manifest-path runtime/Cargo.toml --example mmo -- --replay # 事件溯源确定性验证
//! cargo run --manifest-path runtime/Cargo.toml --example mmo -- --bench  # N 玩家广播吞吐
//! ```
//!
//! 客户端协议（行，`\n` 结尾）：`LOGIN name` / `MOVE x y` / `SAY text` /
//! `LOGOUT`。广播视图发给所有在线玩家，格式：
//! `event: ... | online: [name@(x,y), ...]`。

mod blueprint;
mod machines;

use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axiom_runtime::{
    default_reactor, ProcessResult, Runtime, RuntimeConfig, IoInterest, IoReactor, IoToken,
    RawIo,
};

use machines::*;

// ── 分配计数（bench 用）────────────────────────────────────────────────
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[cfg(unix)]
fn raw_of<T: std::os::unix::io::AsRawFd>(t: &T) -> RawIo {
    t.as_raw_fd()
}
#[cfg(windows)]
fn raw_of<T: std::os::windows::io::AsRawSocket>(t: &T) -> RawIo {
    t.as_raw_socket()
}

// ════════════════════════════════════════════════════════════════════════
// 装配：register + materialize（7 机器 + 7 链接，见 blueprint.rs）
// ════════════════════════════════════════════════════════════════════════

fn build_runtime(cfg: RuntimeConfig) -> Runtime {
    let mut rt = Runtime::new(cfg);
    rt.register::<ConnGateway>("conn_gateway");
    rt.register::<ProtocolParser>("protocol_parser");
    rt.register::<SessionManager>("session_mgr");
    rt.register::<WorldShard>("world_shard");
    rt.register::<PerPlayerView>("per_player_view");
    rt.register::<BroadcastWriter>("broadcast_writer");
    rt.register::<EventLog>("event_log");
    rt.materialize(&blueprint::blueprint()).expect("materialize blueprint");
    rt
}

/// 注入一行协议输入到 protocol_parser，返回终端观察（world_shard.observe）。
fn send_line(rt: &mut Runtime, conn_id: usize, line: &str) -> Vec<String> {
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(b'\n');
    let out = rt
        .tick(vec![(
            "protocol_parser".to_string(),
            "raw".to_string(),
            Box::new(RawBytes(conn_id, bytes)) as Box<dyn std::any::Any + Send>,
        )])
        .expect("tick");
    let mut obs = Vec::new();
    for r in out {
        if let ProcessResult::Yield { value, .. } = r {
            if let Ok(b) = value.downcast::<String>() {
                obs.push(*b);
            }
        }
    }
    obs
}

// ════════════════════════════════════════════════════════════════════════
// 模式 1：TCP 服务器（真实事件循环 + 时钟注入）
// ════════════════════════════════════════════════════════════════════════

const LISTENER_TOKEN: IoToken = IoToken(0);

fn server() {
    let listener = TcpListener::bind("127.0.0.1:6381").expect("bind 127.0.0.1:6381");
    listener.set_nonblocking(true).expect("nonblocking listener");
    println!("mmo shard listening on 127.0.0.1:6381 (axiom blueprint + IoReactor)");
    println!("client protocol: LOGIN name / MOVE x y / SAY text / LOGOUT");

    let mut rt = build_runtime(RuntimeConfig::sequential());
    let table = shared_table();

    let mut listener_reactor = default_reactor().expect("listener reactor");
    listener_reactor
        .register(raw_of(&listener), IoInterest::READABLE, LISTENER_TOKEN)
        .expect("register listener");

    let mut io_reactor = default_reactor().expect("io reactor");
    let mut next_conn: usize = 1;
    let started = Instant::now();
    let mut last_tick: u64 = 0;

    loop {
        // 1. accept 新连接
        if let Ok(events) = listener_reactor.poll(Some(Duration::from_millis(20))) {
            for ev in events {
                if ev.token == LISTENER_TOKEN {
                    loop {
                        match listener.accept() {
                            Ok((stream, addr)) => {
                                stream.set_nonblocking(true).expect("nonblocking");
                                let id = next_conn;
                                next_conn += 1;
                                table.lock().unwrap().conns.insert(id, stream);
                                rt.register_io(
                                    &mut io_reactor,
                                    IoToken(id),
                                    "conn_gateway",
                                    "io",
                                    raw_of(&table.lock().unwrap().conns[&id]),
                                    IoInterest::READABLE,
                                )
                                .expect("register_io");
                                println!("+ conn #{id} from {addr}");
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        }

        // 2. 时钟注入（心跳/超时）：每 100ms 一个 tick
        let now_ms = started.elapsed().as_millis() as u64;
        let mut inputs: Vec<(String, String, Box<dyn std::any::Any + Send>)> = Vec::new();
        if now_ms - last_tick >= 100 {
            inputs.push(("session_mgr".to_string(), "tick".to_string(), Box::new(now_ms)));
            last_tick = now_ms;
        }

        // 3. 连接事件 + 时钟 → 全图 tick
        let _results = rt
            .run_io(&mut io_reactor, inputs, Some(Duration::ZERO))
            .expect("run_io");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 模式 2：事件溯源确定性验证（同输入 → 同世界终态 + 日志；重启重放重建）
// ════════════════════════════════════════════════════════════════════════

fn replay() {
    let _ = std::fs::remove_file("world_events.log");

    let mut rt = build_runtime(RuntimeConfig::sequential());

    // 1. 确定性场景：alice/bob 登录 → alice 移动 → 聊天 → bob 登出 → 心跳超时踢 alice
    let mut observations: Vec<String> = Vec::new();
    // 注意：alice=conn 1，bob=conn 2（复用同一连接会触发"顶号"踢出旧会话）
    for (conn, line) in [
        (1usize, "LOGIN alice"),
        (2usize, "LOGIN bob"),
        (1usize, "MOVE 1.5 2.5"),
        (1usize, "SAY hello world"),
        (2usize, "LOGOUT"),
    ] {
        observations.extend(send_line(&mut rt, conn, line));
    }
    // alice 心跳超时（tick 到 10s 之后）
    let out = rt
        .tick(vec![(
            "session_mgr".to_string(),
            "tick".to_string(),
            Box::new(HEARTBEAT_TIMEOUT_MS + 1u64) as Box<dyn std::any::Any + Send>,
        )])
        .expect("tick");
    for r in out {
        if let ProcessResult::Yield { value, .. } = r {
            if let Ok(b) = value.downcast::<String>() {
                observations.push(*b);
            }
        }
    }

    // 2. 断言世界事件序列（确定性）
    // observe 文本格式：`event: <evt> | players=[...]` —— 取事件前缀
    let evts: Vec<String> = observations
        .iter()
        .map(|s| {
            s.split("event: ")
                .nth(1)
                .unwrap_or("")
                .split(" | ")
                .next()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(
        evts.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec![
            "alice joined",
            "bob joined",
            "alice moved to (1.5,2.5)",
            "alice says: hello world",
            "bob left",
            "alice left", // 心跳超时踢出
        ],
        "world event stream must be deterministic: {evts:?}"
    );
    // 世界终态：无在线玩家
    assert!(
        observations.last().unwrap().contains("players=[]"),
        "final world must be empty: {:?}",
        observations.last()
    );

    // 3. 事件日志内容 = 事件流
    let log = std::fs::read_to_string("world_events.log").expect("read event log");
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        vec![
            "JOIN 1 alice",
            "JOIN 2 bob",
            "MOVE alice 1.5 2.5",
            "SAY alice hello world",
            "LEAVE 2 bob",
            "LEAVE 1 alice",
        ],
        "event log = world event stream (event sourcing)"
    );

    // 4. 重启：从日志重放重建世界（新 runtime，注入 replay 行）
    let mut rt2 = build_runtime(RuntimeConfig::sequential());
    let mut restored: Vec<String> = Vec::new();
    for line in log.lines() {
        let out = rt2
            .tick(vec![(
                "world_shard".to_string(),
                "replay".to_string(),
                Box::new(line.to_string()) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        for r in out {
            if let ProcessResult::Yield { value, .. } = r {
                if let Ok(b) = value.downcast::<String>() {
                    restored.push(*b);
                }
            }
        }
    }
    // 重放后世界终态 = 原终态（空）；重放中间态正确重建（alice+bob 在场）
    assert!(
        restored.last().unwrap().contains("players=[]"),
        "replayed world must match original final state: {:?}",
        restored.last()
    );
    assert!(
        restored[1].contains("players=[(1, \"alice\", 0.0, 0.0), (2, \"bob\", 0.0, 0.0)]")
            || restored[1].contains("players=[(2, \"bob\", 0.0, 0.0), (1, \"alice\", 0.0, 0.0)]"),
        "replay must rebuild the world mid-state: {:?}",
        restored[1]
    );

    println!("=== replay OK ===");
    println!("6 个世界事件按序复现；事件日志 = 事件流；重启重放重建世界（中间态+终态一致）");
}

// ════════════════════════════════════════════════════════════════════════
// 模式 3：基准（N 玩家在线，世界 tick + 广播吞吐）
// ════════════════════════════════════════════════════════════════════════

fn bench() {
    const N: usize = 100; // 在线玩家
    const ITERS: usize = 10_000; // 世界事件数

    let mut rt = build_runtime(RuntimeConfig::sequential());

    // 1. N 玩家登录（世界快照含 N 人）
    for i in 1..=N {
        send_line(&mut rt, i, &format!("LOGIN player{i}"));
    }
    assert!(
        send_line(&mut rt, 1, "MOVE 0.0 0.0")
            .last()
            .unwrap()
            .contains("(1, \"player1\""),
        "N 玩家就绪"
    );
    println!("=== mmo bench (Sequential, 世界层单实例 + N 视图投影) ===");
    println!("online players: {N}");

    // 2. MOVE 事件广播：世界更新 → N 视图（投影 + 写回无 socket）
    ALLOCS.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    for i in 0..ITERS {
        send_line(&mut rt, 1, &format!("MOVE {} {}", i as f32, 0.0));
    }
    let dt = t0.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed);

    println!(
        "MOVE 广播: {:.0} world-tick/s（每次 tick 投影 {N} 个玩家视图）, {:.1} allocs/tick",
        ITERS as f64 / dt.as_secs_f64(),
        allocs as f64 / ITERS as f64
    );
}

// ════════════════════════════════════════════════════════════════════════
// main
// ════════════════════════════════════════════════════════════════════════

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--replay") => replay(),
        Some("--bench") => bench(),
        _ => server(),
    }
}


