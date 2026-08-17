//! # MMO core subgraph — assembly and driving
//!
//! Three run modes:
//!
//! ```text
//! cargo run --manifest-path runtime/Cargo.toml --example mmo             # TCP server (default)
//! cargo run --manifest-path runtime/Cargo.toml --example mmo -- --replay # event-sourcing determinism check
//! cargo run --manifest-path runtime/Cargo.toml --example mmo -- --bench  # N-player broadcast throughput
//! ```
//!
//! Client protocol (line-based, `\n`-terminated): `LOGIN name` / `MOVE x y` / `SAY text` /
//! `LOGOUT`. The broadcast view is sent to all online players in the format:
//! `event: ... | online: [name@(x,y), ...]`.

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

// ── Allocation counting (for bench mode) ────────────────────────────────
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
// Assembly: register + materialize (7 machines + 7 links, see blueprint.rs)
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

/// Injects one protocol line into protocol_parser and returns the terminal observations (world_shard.observe).
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
// Mode 1: TCP server (real event loop + clock injection)
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
        // 1. accept new connections
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

        // 2. Clock injection (heartbeat/timeout): one tick every 100ms
        let now_ms = started.elapsed().as_millis() as u64;
        let mut inputs: Vec<(String, String, Box<dyn std::any::Any + Send>)> = Vec::new();
        if now_ms - last_tick >= 100 {
            inputs.push(("session_mgr".to_string(), "tick".to_string(), Box::new(now_ms)));
            last_tick = now_ms;
        }

        // 3. connection events + clock → whole-graph tick
        let _results = rt
            .run_io(&mut io_reactor, inputs, Some(Duration::ZERO))
            .expect("run_io");
    }
}

// ════════════════════════════════════════════════════════════════════════
// Mode 2: event-sourcing determinism check (same input → same final world + log; restart + replay rebuilds it)
// ════════════════════════════════════════════════════════════════════════

fn replay() {
    let _ = std::fs::remove_file("world_events.log");

    let mut rt = build_runtime(RuntimeConfig::sequential());

    // 1. Deterministic scenario: alice/bob log in → alice moves → chat → bob logs out → heartbeat timeout kicks alice
    let mut observations: Vec<String> = Vec::new();
    // Note: alice=conn 1, bob=conn 2 (reusing the same connection would trigger a "kick old session" on re-login)
    for (conn, line) in [
        (1usize, "LOGIN alice"),
        (2usize, "LOGIN bob"),
        (1usize, "MOVE 1.5 2.5"),
        (1usize, "SAY hello world"),
        (2usize, "LOGOUT"),
    ] {
        observations.extend(send_line(&mut rt, conn, line));
    }
    // alice heartbeat timeout (tick past 10s)
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

    // 2. Assert the world event sequence (deterministic)
    // observe text format: `event: <evt> | players=[...]` — take the event prefix
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
            "alice left", // kicked by heartbeat timeout
        ],
        "world event stream must be deterministic: {evts:?}"
    );
    // World final state: no online players
    assert!(
        observations.last().unwrap().contains("players=[]"),
        "final world must be empty: {:?}",
        observations.last()
    );

    // 3. Event log content = event stream
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

    // 4. Restart: rebuild the world by replaying the log (new runtime, inject replay lines)
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
    // after replay, the world's final state = original final state (empty); replay correctly rebuilds the mid-state (alice + bob present)
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
    println!("6 world events replayed in order; event log = event stream; restart + replay rebuilds the world (mid-state + final state consistent)");
}

// ════════════════════════════════════════════════════════════════════════
// Mode 3: benchmark (N players online, world tick + broadcast throughput)
// ════════════════════════════════════════════════════════════════════════

fn bench() {
    const N: usize = 100; // online players
    const ITERS: usize = 10_000; // number of world events

    let mut rt = build_runtime(RuntimeConfig::sequential());

    // 1. N players log in (world snapshot contains N people)
    for i in 1..=N {
        send_line(&mut rt, i, &format!("LOGIN player{i}"));
    }
    assert!(
        send_line(&mut rt, 1, "MOVE 0.0 0.0")
            .last()
            .unwrap()
            .contains("(1, \"player1\""),
        "N players ready"
    );
    println!("=== mmo bench (Sequential, single world-layer instance + N view projections) ===");
    println!("online players: {N}");

    // 2. MOVE event broadcast: world update → N views (projection + write-back without sockets)
    ALLOCS.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    for i in 0..ITERS {
        send_line(&mut rt, 1, &format!("MOVE {} {}", i as f32, 0.0));
    }
    let dt = t0.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed);

    println!(
        "MOVE broadcast: {:.0} world-tick/s (each tick projects {N} player views), {:.1} allocs/tick",
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


