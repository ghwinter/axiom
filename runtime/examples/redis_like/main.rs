//! # Redis-style server — assembly and driving
//!
//! Three run modes:
//!
//! ```text
//! cargo run --manifest-path runtime/Cargo.toml --example redis_like            # TCP server (default)
//! cargo run --manifest-path runtime/Cargo.toml --example redis_like -- --bench  # throughput + allocation benchmark
//! cargo run --manifest-path runtime/Cargo.toml --example redis_like -- --replay # AOF replay determinism check
//! ```
//!
//! ## Physical assembly (below the blueprint)
//!
//! ```text
//! main: TcpListener ──► DefaultReactor(poll) ──► accept → connection table (shared_table)
//!                                                    └─► rt.register_io(token=conn_id → conn_reader.io)
//! loop { listener_reactor.poll → accept; rt.run_io(io_reactor, connection events) → tick whole graph }
//! ```

mod blueprint;
mod machines;


use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};


use axiom::deploy::DynamicTopology;
use axiom::link::WritePolicy;
use axiom_runtime::{
    default_reactor, ProcessResult, Runtime, RuntimeConfig, IoInterest, IoReactor,
    IoToken, RawIo,
};

use machines::*;

// ── Allocation counting (for bench mode, same as psql --bench) ──────────
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

// ── Platform raw IO handle extraction ───────────────────────────────────
#[cfg(unix)]
fn raw_of<T: std::os::unix::io::AsRawFd>(t: &T) -> RawIo {
    t.as_raw_fd()
}
#[cfg(windows)]
fn raw_of<T: std::os::windows::io::AsRawSocket>(t: &T) -> RawIo {
    t.as_raw_socket()
}

// ════════════════════════════════════════════════════════════════════════
// Assembly: register + materialize (9 machine types, see blueprint.rs)
// ════════════════════════════════════════════════════════════════════════

fn build_runtime(cfg: RuntimeConfig, spec: &DynamicTopology) -> Runtime {
    let mut rt = Runtime::new(cfg);
    rt.register::<ConnReader>("conn_reader");
    rt.register::<RespParser>("resp_parser");
    rt.register::<Sharder>("sharder");
    rt.register::<DataStore>("data_store");
    rt.register::<RespEncoder>("resp_encoder");
    rt.register::<ConnWriter>("conn_writer");
    rt.register::<AofWriter>("aof_writer");
    rt.register::<Monitor>("monitor");
    rt.register::<Debugger>("debugger");
    rt.register::<BroadcastTee>("broadcast_tee");
    rt.materialize(spec).expect("materialize blueprint");
    rt
}

// ════════════════════════════════════════════════════════════════════════
// Mode 1: TCP server (real event loop + network IO)
// ════════════════════════════════════════════════════════════════════════

const LISTENER_TOKEN: IoToken = IoToken(0);

fn server() {
    let listener = TcpListener::bind("127.0.0.1:6380").expect("bind 127.0.0.1:6380");
    listener.set_nonblocking(true).expect("nonblocking listener");
    println!("redis_like listening on 127.0.0.1:6380 (axiom blueprint + IoReactor)");

    let mut rt = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let table = shared_table();

    // Two reactors: listener (accept managed by main) + connections (runtime routes events)
    let mut listener_reactor = default_reactor().expect("listener reactor");
    listener_reactor
        .register(raw_of(&listener), IoInterest::READABLE, LISTENER_TOKEN)
        .expect("register listener");

    let mut io_reactor = default_reactor().expect("io reactor");
    let mut next_conn: usize = 1;

    loop {
        // ── 1. accept new connections (listener_reactor READABLE) ────────
        if let Ok(events) = listener_reactor.poll(Some(Duration::from_millis(20))) {
            for ev in events {
                if ev.token == LISTENER_TOKEN {
                    loop {
                        match listener.accept() {
                            Ok((stream, addr)) => {
                                stream.set_nonblocking(true).expect("nonblocking");
                                let id = next_conn;
                                next_conn += 1;
                                // Physical: connection goes into shared table + READABLE event registered to runtime
                                table.lock().unwrap().conns.insert(id, stream);
                                rt.register_io(
                                    &mut io_reactor,
                                    IoToken(id),
                                    "conn_reader",
                                    "io",
                                    raw_of(&table.lock().unwrap().conns[&id]),
                                    IoInterest::READABLE,
                                )
                                .expect("register_io");
                                println!("+ conn #{id} from {addr}");
                            }
                            Err(_) => break, // WouldBlock or error: this batch of accepts is done
                        }
                    }
                }
            }
        }

        // ── 2. connection events → whole-graph tick (io_reactor non-blocking event fetch) ──
        let _results = rt
            .run_io(&mut io_reactor, Vec::new(), Some(Duration::ZERO))
            .expect("run_io");
    }
}

// ════════════════════════════════════════════════════════════════════════
// Mode 2: benchmark (no network: RESP bytes injected into the parse chain, measures whole-graph throughput + allocations)
// ════════════════════════════════════════════════════════════════════════

/// Bench-specific minimal topology: resp_parser (entry, no inbound edges) → data_store →
/// resp_encoder → conn_writer (+ aof_writer); `obs` controls the observe→monitor
/// carrier (None = no observer module). Parallel-mode entry injection needs machines with no inbound edges.
fn bench_spec(obs: Option<WritePolicy>) -> DynamicTopology {
    use axiom::deploy::MachineInstance;
    use axiom::link::{LinkKind, LinkSpec, ReadPolicy};
    use axiom::resource::MachinePhysicalSpec;

    let mut spec = DynamicTopology::new()
        .with_machine(MachineInstance::new(
            "resp_parser",
            "resp_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "data_store",
            "data_store",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "resp_encoder",
            "resp_encoder",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "conn_writer",
            "conn_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "aof_writer",
            "aof_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_link(LinkSpec::new(
            ("resp_parser", "cmd"),
            ("data_store", "cmd"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("data_store", "reply"),
            ("resp_encoder", "reply"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("resp_encoder", "out"),
            ("conn_writer", "resp"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ))
        .with_link(LinkSpec::new(
            ("data_store", "log"),
            ("aof_writer", "log"),
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        ));
    if let Some(policy) = obs {
        spec = spec
            .with_machine(MachineInstance::new(
                "monitor",
                "monitor",
                MachinePhysicalSpec::default(),
            ))
            .with_link(LinkSpec::new(
                ("data_store", "observe"),
                ("monitor", "log"),
                LinkKind::BoundedBuf {
                    capacity: 16,
                    write_policy: policy,
                    read_policy: ReadPolicy::Blocking,
                },
            ));
    }
    spec
}

/// Sharded-cluster bench topology (**multi-entry**: each shard has an independent parse chain —
/// Redis cluster shape, where connections hit nodes directly; under Parallel the two chains
/// genuinely run in parallel).
fn bench_spec_multi_entry() -> DynamicTopology {
    use axiom::deploy::MachineInstance;
    use axiom::link::{LinkKind, LinkSpec, ReadPolicy};
    use axiom::resource::MachinePhysicalSpec;

    let buf = |a: (&'static str, &'static str), b: (&'static str, &'static str)| {
        LinkSpec::new(
            a,
            b,
            LinkKind::BoundedBuf {
                capacity: 1024,
                write_policy: WritePolicy::Blocking,
                read_policy: ReadPolicy::Blocking,
            },
        )
    };
    DynamicTopology::new()
        .with_machine(MachineInstance::new(
            "resp_parser_0",
            "resp_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "resp_parser_1",
            "resp_parser",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "data_store_0",
            "data_store",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "data_store_1",
            "data_store",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "resp_encoder",
            "resp_encoder",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "conn_writer",
            "conn_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "aof_writer_0",
            "aof_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_machine(MachineInstance::new(
            "aof_writer_1",
            "aof_writer",
            MachinePhysicalSpec::default(),
        ))
        .with_link(buf(("resp_parser_0", "cmd"), ("data_store_0", "cmd")))
        .with_link(buf(("resp_parser_1", "cmd"), ("data_store_1", "cmd")))
        .with_link(buf(("data_store_0", "reply"), ("resp_encoder", "reply")))
        .with_link(buf(("data_store_1", "reply"), ("resp_encoder", "reply")))
        .with_link(buf(("resp_encoder", "out"), ("conn_writer", "resp")))
        .with_link(buf(("data_store_0", "log"), ("aof_writer_0", "log")))
        .with_link(buf(("data_store_1", "log"), ("aof_writer_1", "log")))
}

fn bench() {
    const N: usize = 100_000;

    // Build RESP command bytes (conn_id = 0, no socket in table → ConnWriter is a no-op)
    // key "bench" hashes to a fixed shard; use two keys to balance shard load (bench_a/bench_b)
    let set_a = b"*3\r\n$3\r\nSET\r\n$6\r\nbench_a\r\n$5\r\nvalue\r\n".to_vec();
    let get_a = b"*2\r\n$3\r\nGET\r\n$6\r\nbench_a\r\n".to_vec();
    let set_b = b"*3\r\n$3\r\nSET\r\n$6\r\nbench_b\r\n$5\r\nvalue\r\n".to_vec();

    let inject = |rt: &mut Runtime, inputs: Vec<(String, String, Box<dyn std::any::Any + Send>)>| {
        rt.tick(inputs).expect("tick")
    };
    let batch = |bytes: &[u8], n: usize| {
        (0..n)
            .map(|_| {
                (
                    "resp_parser".to_string(),
                    "raw".to_string(),
                    Box::new(RawBytes(0, bytes.to_vec())) as Box<dyn std::any::Any + Send>,
                )
            })
            .collect()
    };

    // A. Observer-carrier comparison (single DataStore baseline, Parallel(4))
    println!("=== redis_like bench A: impact of observer module on the main path ===");
    println!("(Parallel(4): links use the channel carrier; monitor simulates a slow observer at 20µs/event)\n");
    MONITOR_WORK_NS.store(20_000, Ordering::Relaxed);

    let configs: [(&str, DynamicTopology); 3] = [
        ("baseline (no monitor)", bench_spec(None)),
        ("monitor + Blocking", bench_spec(Some(WritePolicy::Blocking))),
        ("monitor + Dropping", bench_spec(Some(WritePolicy::Dropping))),
    ];

    for (label, spec) in configs {
        let mut rt = build_runtime(RuntimeConfig::parallel(4), &spec);
        inject(&mut rt, batch(&set_a, 1000)); // warm-up

        ALLOCS.store(0, Ordering::Relaxed);
        let t0 = Instant::now();
        inject(&mut rt, batch(&set_a, N));
        let set_dt = t0.elapsed();
        let set_allocs = ALLOCS.load(Ordering::Relaxed);

        ALLOCS.store(0, Ordering::Relaxed);
        let t0 = Instant::now();
        inject(&mut rt, batch(&get_a, N));
        let get_dt = t0.elapsed();
        let get_allocs = ALLOCS.load(Ordering::Relaxed);

        println!(
            "{label:<22}  SET {:>7.0} cmd/s ({:.1} allocs)   GET {:>7.0} cmd/s ({:.1} allocs)",
            N as f64 / set_dt.as_secs_f64(),
            set_allocs as f64 / N as f64,
            N as f64 / get_dt.as_secs_f64(),
            get_allocs as f64 / N as f64,
        );
    }
    MONITOR_WORK_NS.store(0, Ordering::Relaxed);

    // B. Sharded cluster vs single DataStore: real multicore benefit of parallel sharding
    println!("\n=== redis_like bench B: sharded cluster (fan-out + fan-in + parallel shards) ===");
    println!("(two keys balanced across 2 shards; compares Sequential single-core vs Parallel shards)\n");

    // Alternate injection across the two entries (one parse chain per command shard — Redis
    // cluster shape: connections hit nodes directly, commands arrive concurrently at each shard;
    // under Parallel the two chains genuinely run in parallel).
    let mixed_shards = |n: usize| {
        (0..n)
            .flat_map(|i| {
                let (machine, bytes) = if i & 1 == 0 {
                    ("resp_parser_0", &set_a)
                } else {
                    ("resp_parser_1", &set_b)
                };
                vec![(
                    machine.to_string(),
                    "raw".to_string(),
                    Box::new(RawBytes(0, bytes.to_vec())) as Box<dyn std::any::Any + Send>,
                )]
            })
            .collect::<Vec<_>>()
    };

    let mut rt_seq = build_runtime(RuntimeConfig::sequential(), &bench_spec_multi_entry());
    let mut rt_par = build_runtime(RuntimeConfig::parallel(4), &bench_spec_multi_entry());
    let mut rt_single = build_runtime(RuntimeConfig::parallel(4), &bench_spec(None));

    inject(&mut rt_seq, mixed_shards(1000));
    inject(&mut rt_par, mixed_shards(1000));
    inject(&mut rt_single, batch(&set_a, 1000));

    let run = |rt: &mut Runtime, n: usize| -> (f64, f64) {
        ALLOCS.store(0, Ordering::Relaxed);
        let t0 = Instant::now();
        inject(rt, mixed_shards(n));
        let dt = t0.elapsed();
        (n as f64 / dt.as_secs_f64(), ALLOCS.load(Ordering::Relaxed) as f64 / n as f64)
    };

    let (seq_rate, seq_alloc) = run(&mut rt_seq, N);
    let (par_rate, par_alloc) = run(&mut rt_par, N);
    // Single DataStore uses single-entry injection (fair comparison: same 100k commands; multi-entry injection does not apply to it)
    ALLOCS.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    inject(&mut rt_single, batch(&set_a, N));
    let single_dt = t0.elapsed();
    let single_rate = N as f64 / single_dt.as_secs_f64();
    let single_alloc = ALLOCS.load(Ordering::Relaxed) as f64 / N as f64;

    println!(
        "sharded Sequential(1)   {:>7.0} cmd/s ({:.1} allocs)",
        seq_rate, seq_alloc
    );
    println!(
        "sharded Parallel(4)     {:>7.0} cmd/s ({:.1} allocs)   {:.2}x vs Sequential",
        par_rate, par_alloc, par_rate / seq_rate
    );
    println!(
        "single  Parallel(4)     {:>7.0} cmd/s ({:.1} allocs)   {:.2}x vs sharded-Seq",
        single_rate, single_alloc, single_rate / seq_rate
    );

    println!(
        "\nExpected: sharded Parallel processes with two DataStore threads in parallel (fan-in convergence, lossless semantics);\n\
         sharded Sequential is slightly below single DataStore because of the extra sharder hop + dual AOF (complexity is conserved:\n\
         one extra topology hop costs one extra absolute hop; Parallel sharding can offset that)."
    );
}

// ════════════════════════════════════════════════════════════════════════
// Mode 3: AOF replay (determinism check: same command sequence → same final state)
// ════════════════════════════════════════════════════════════════════════

fn replay() {
    // Start from a clean state every time (AOF append mode accumulates; the sharded variant has two AOF files)
    let _ = std::fs::remove_file("redis_like.aof");
    let _ = std::fs::remove_file("redis_like_aof_writer_0.aof");
    let _ = std::fs::remove_file("redis_like_aof_writer_1.aof");

    // 1. Sharded-cluster blueprint: commands are routed by key hash to 2 DataStores (fan-out).
    //    The command sequence is interleaved across shards; the assertions do not assume a
    //    hash distribution, only that each shard is self-consistent (SET/GET for the same key
    //    route to the same shard) and that final values are correct.
    let mut rt = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded());

    let cmds: Vec<&[&[u8]]> = vec![
        &[b"SET", b"k1", b"v1"],
        &[b"INCR", b"k2"],
        &[b"INCR", b"k2"],
        &[b"INCR", b"k2"],
        &[b"LPUSH", b"lst", b"a"],
        &[b"LPUSH", b"lst", b"b"],
        &[b"HSET", b"h", b"f", b"v"],
    ];

    // 2. Inject one by one (RESP bytes → full chain: parse → shard routing → execute → observe)
    let mut observed: Vec<String> = Vec::new();
    for c in &cmds {
        let bytes = encode_resp(*c);
        let out = rt
            .tick(vec![(
                "resp_parser".to_string(),
                "raw".to_string(),
                Box::new(RawBytes(0, bytes)) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        for r in out {
            if let ProcessResult::Yield { value, .. } = r {
                if let Ok(boxed) = value.downcast::<(usize, String)>() { let (_, summary) = *boxed;
                    observed.push(summary.clone());
                }
            }
        }
    }

    // 3. Assert: deterministic results (converged across shards, identical to the single-DataStore variant)
    assert_eq!(
        observed,
        vec![
            "SET => Ok".to_string(),
            "INCR => Int(1)".to_string(),
            "INCR => Int(2)".to_string(),
            "INCR => Int(3)".to_string(),
            "LPUSH => Int(1)".to_string(),
            "LPUSH => Int(2)".to_string(),
            "HSET => Int(1)".to_string(),
        ],
        "sharded cluster must deterministically reproduce command results (fan-out/fan-in does not change semantics)"
    );

    // 4. GET query helper (inject RESP GET → full chain → observe summary)
    let get = |rt: &mut Runtime, key: &str| -> String {
        let out = rt
            .tick(vec![(
                "resp_parser".to_string(),
                "raw".to_string(),
                Box::new(RawBytes(0, encode_resp(&[b"GET", key.as_bytes()])))
                    as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        for r in out {
            if let ProcessResult::Yield { value, .. } = r {
                if let Ok(boxed) = value.downcast::<(usize, String)>() { let (_, summary) = *boxed;
                    return summary;
                }
            }
        }
        unreachable!()
    };

    // 5. Cross-shard routing consistency: SET/GET for the same key → same shard → consistent value
    assert!(get(&mut rt, "k1").contains("Bulk(Some"), "GET k1 should hit the shard that owns it");
    let k2v = get(&mut rt, "k2");
    assert!(
        k2v.contains("Bulk(Some([51]))"),
        "GET k2 should return the stored value \"3\" (consistent cross-shard routing), got {k2v}"
    );
    assert!(get(&mut rt, "lst").contains("WRONGTYPE"), "GET on a list should report WRONGTYPE");

    // 6. Restart (new runtime, fresh state) → replay both AOF files in order (one log per shard)
    //    → sharder re-hashes → each command returns to the correct shard → state is rebuilt
    let aof0 = std::fs::read("redis_like_aof_writer_0.aof").unwrap_or_default();
    let aof1 = std::fs::read("redis_like_aof_writer_1.aof").unwrap_or_default();
    assert!(!aof0.is_empty() || !aof1.is_empty(), "at least one of the two shard AOF files must be non-empty");
    let mut rt2 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded());
    for aof in [&aof0, &aof1] {
        let mut cursor = 0usize;
        while cursor < aof.len() {
            let cmd_end = next_command_end(aof, cursor);
            let chunk = aof[cursor..cmd_end].to_vec();
            cursor = cmd_end;
            rt2.tick(vec![(
                "resp_parser".to_string(),
                "raw".to_string(),
                Box::new(RawBytes(0, chunk)) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        }
    }
    // Replayed state == original state (write commands have no cross-shard dependencies → dual-log replay order is safe)
    let k2v2 = get(&mut rt2, "k2");
    assert!(
        k2v2.contains("Bulk(Some([51]))"),
        "rt2 (after dual-AOF replay): GET k2 should return \"3\", got {k2v2}"
    );
    assert!(get(&mut rt2, "k1").contains("Bulk(Some"));
    assert!(get(&mut rt2, "lst").contains("WRONGTYPE"), "lst should exist after replay (WRONGTYPE)");

    // 7. Debug injection (Control flow broadcast to both shards: debugger → data_store_0.ctrl + data_store_1.ctrl)
    let mut rt3 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded());
    let dbg = |rt: &mut Runtime, cmd: DebugCmd| -> Vec<String> {
        let out = rt
            .tick(vec![(
                "debugger".to_string(),
                "cmd".to_string(),
                Box::new(cmd) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        let mut obs = Vec::new();
        for r in out {
            if let ProcessResult::Yield { value, .. } = r {
                if let Ok(boxed) = value.downcast::<(usize, String)>() {
                    let (_, s) = *boxed;
                    obs.push(s);
                }
            }
        }
        obs
    };
    // DEBUG SET (out-of-band control, not logged to AOF) → injected into both shards → GET hits either shard
    let obs = dbg(&mut rt3, DebugCmd::Set("dk".into(), "dv".into()));
    assert!(
        obs.iter().any(|s| s.contains("DEBUG SET dk => dv")),
        "DEBUG SET should be observed by monitor: {obs:?}"
    );
    assert!(
        get(&mut rt3, "dk").contains("Bulk(Some"),
        "keys injected via DEBUG should be readable by normal GET (broadcast to both shards; either shard hits)"
    );
    // DEBUG INFO → key count (reported once per shard)
    let obs = dbg(&mut rt3, DebugCmd::Info);
    assert!(
        obs.iter().any(|s| s.contains("keys=")),
        "DEBUG INFO should return statistics: {obs:?}"
    );
    // DEBUG FLUSH → both shards cleared; GET returns nil
    let _ = dbg(&mut rt3, DebugCmd::Flush);
    assert!(
        get(&mut rt3, "dk").contains("Bulk(None)"),
        "GET should be nil after DEBUG FLUSH"
    );

    println!("=== replay OK (sharded cluster: fan-out + fan-in + dual AOF + Control broadcast) ===");
    println!("7 commands deterministically reproduced across shards; SET/GET same-key routing consistent; dual-AOF replay rebuilds state; DEBUG broadcast across both shards");
}

/// Encodes `[cmd, args...]` as RESP command bytes (same format as machines::encode_command).
fn encode_resp(args: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(format!("*{}\r\n", args.len()).as_bytes());
    for a in args {
        out.extend_from_slice(format!("${}\r\n", a.len()).as_bytes());
        out.extend_from_slice(a);
        out.extend_from_slice(b"\r\n");
    }
    out
}

/// Locates the end of the next RESP command in an AOF byte stream (scans by `*N\r\n` + N×`$len\r\n...`).
fn next_command_end(buf: &[u8], from: usize) -> usize {
    let rest = &buf[from..];
    // Simplification: in this showcase each AOF command is a complete RESP block with no
    // inter-block separators — so just skip N arguments per RESP syntax (same logic as try_parse_command).
    let (n_line, mut rest) = split_crlf_static(rest).expect("cmd header");
    let n: usize = std::str::from_utf8(&n_line[1..]).expect("n").parse().expect("n");
    for _ in 0..n {
        let (len_line, after) = split_crlf_static(rest).expect("arg header");
        let len: usize = std::str::from_utf8(&len_line[1..]).expect("len").parse().expect("len");
        rest = &after[len + 2..];
    }
    buf.len() - rest.len()
}

fn split_crlf_static(b: &[u8]) -> Option<(&[u8], &[u8])> {
    let pos = b.windows(2).position(|w| w == b"\r\n")?;
    Some((&b[..pos], &b[pos + 2..]))
}

// ════════════════════════════════════════════════════════════════════════
// main
// ════════════════════════════════════════════════════════════════════════

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--bench") => bench(),
        Some("--replay") => replay(),
        Some("--timetravel") => timetravel(),
        _ => server(),
    }
}

// ════════════════════════════════════════════════════════════════════════
// Mode 4: time-travel debugging (event-sourcing replay showcase)
// ════════════════════════════════════════════════════════════════════════
//
// While executing the command sequence, record the input event stream (journal); afterwards,
// replay from a **clean state** to any point in time and query the state — "after a failure,
// replay to just before the crash and inspect the state" (causal reasoning).
// This works under the sharded-cluster blueprint (fan-out/fan-in) too — replay is a whole-graph replay.

fn timetravel() {
    use axiom_runtime::{ReplayJournal, Replayer};

    // 1. Execute the command sequence (sharded cluster) while recording the journal (RESP byte inputs, Clone).
    let mut rt = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded());
    let mut journal = ReplayJournal::new();

    let cmds: Vec<&[&[u8]]> = vec![
        &[b"SET", b"k1", b"v1"],
        &[b"INCR", b"k2"],
        &[b"INCR", b"k2"],
        &[b"INCR", b"k2"],
        &[b"LPUSH", b"lst", b"a"],
        &[b"LPUSH", b"lst", b"b"],
        &[b"HSET", b"h", b"f", b"v"],
    ];
    for c in &cmds {
        let bytes = encode_resp(*c);
        journal.end_batch();
        let _ = rt
            .tick(vec![(
                "resp_parser".to_string(),
                "raw".to_string(),
                Box::new(RawBytes(0, bytes.clone())) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        journal.record("resp_parser", "raw", &RawBytes(0, bytes));
    }
    assert_eq!(journal.len(), 7, "all 7 batches of commands must be recorded");

    // 2. Time travel: replay to any point in time (each time from a clean state).
    let replayer = Replayer::new(&journal);
    let get = |rt: &mut Runtime, key: &str| -> String {
        let out = rt
            .tick(vec![(
                "resp_parser".to_string(),
                "raw".to_string(),
                Box::new(RawBytes(0, encode_resp(&[b"GET", key.as_bytes()])))
                    as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        for r in out {
            if let ProcessResult::Yield { value, .. } = r {
                if let Ok(boxed) = value.downcast::<(usize, String)>() { let (_, summary) = *boxed;
                    return summary;
                }
            }
        }
        unreachable!()
    };

    // Time point 3: after SET + INCR×2 — k2 should = "2", lst does not exist yet.
    let (mut rt3, _) = replayer.forward_to(3, || {
        build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded())
    }).expect("replay to 3");
    let k2_at_3 = get(&mut rt3, "k2");
    assert!(
        k2_at_3.contains("Bulk(Some([50]))"),
        "time point 3: k2 should = \"2\" (INCR×2 executed), got {k2_at_3}"
    );
    let lst_at_3 = get(&mut rt3, "lst");
    assert!(
        lst_at_3.contains("Bulk(None)"),
        "time point 3: lst not yet created (LPUSH not executed), got {lst_at_3}"
    );

    // Time point 4: after INCR×3 — k2 = "3" (time travel to any intermediate point).
    let (mut rt4, _) = replayer.forward_to(4, || {
        build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded())
    }).expect("replay to 4");
    let k2_at_4 = get(&mut rt4, "k2");
    assert!(
        k2_at_4.contains("Bulk(Some([51]))"),
        "time point 4: k2 should = \"3\" (INCR×3 executed), got {k2_at_4}"
    );

    // Time point 5: after LPUSH×2 — lst exists (WRONGTYPE is the reply to GET on a list).
    let (mut rt5, _) = replayer.forward_to(5, || {
        build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded())
    }).expect("replay to 5");
    let lst_at_5 = get(&mut rt5, "lst");
    assert!(
        lst_at_5.contains("WRONGTYPE"),
        "time point 5: lst created (LPUSH×2 executed), got {lst_at_5}"
    );

    // Time point 7: after all commands — h.f = v.
    let (mut rt7, _) = replayer.forward_to(7, || {
        build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint_sharded())
    }).expect("replay to 7");
    let h_at_7 = get(&mut rt7, "h");
    assert!(
        h_at_7.contains("WRONGTYPE"),
        "time point 7: h created (HSET executed), got {h_at_7}"
    );

    println!("=== timetravel OK (event-sourcing replay) ===");
    println!("7 batches recorded into the journal; replay to time points 3/4/5/7 yields causal states:");
    println!("  t=3: k2=\"2\" (after INCR×2), lst absent (before LPUSH)");
    println!("  t=4: k2=\"3\" (after INCR×3)");
    println!("  t=5: lst created (after LPUSH×2)");
    println!("  t=7: h created (after HSET)");
    println!("time travel also works under the sharded cluster (fan-out/fan-in) — replay is a whole-graph replay");
}


