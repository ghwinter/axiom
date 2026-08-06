//! # Redis 风格服务器 — 装配与驱动
//!
//! 三种运行模式：
//!
//! ```text
//! cargo run --manifest-path runtime/Cargo.toml --example redis_like            # TCP 服务器（默认）
//! cargo run --manifest-path runtime/Cargo.toml --example redis_like -- --bench  # 吞吐 + 分配基准
//! cargo run --manifest-path runtime/Cargo.toml --example redis_like -- --replay # AOF 重放确定性验证
//! ```
//!
//! ## 物理装配（蓝图之下）
//!
//! ```text
//! main: TcpListener ──► DefaultReactor(poll) ──► accept → 连接表(shared_table)
//!                                                    └─► rt.register_io(token=conn_id → conn_reader.io)
//! loop { listener_reactor.poll → accept； rt.run_io(io_reactor, 连接事件) → tick 全图 }
//! ```

mod blueprint;
mod machines;


use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};


use axiom::deploy::DeploySpec;
use axiom::link::WritePolicy;
use axiom_runtime::{
    default_reactor, ProcessResult, Runtime, RuntimeConfig, IoInterest, IoReactor,
    IoToken, RawIo,
};

use machines::*;

// ── 分配计数（bench 用，psql --bench 同款）──────────────────────────────
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

// ── 平台 raw IO 句柄提取 ────────────────────────────────────────────────
#[cfg(unix)]
fn raw_of<T: std::os::unix::io::AsRawFd>(t: &T) -> RawIo {
    t.as_raw_fd()
}
#[cfg(windows)]
fn raw_of<T: std::os::windows::io::AsRawSocket>(t: &T) -> RawIo {
    t.as_raw_socket()
}

// ════════════════════════════════════════════════════════════════════════
// 装配：register + materialize（9 机器类型，见 blueprint.rs）
// ════════════════════════════════════════════════════════════════════════

fn build_runtime(cfg: RuntimeConfig, spec: &DeploySpec) -> Runtime {
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
// 模式 1：TCP 服务器（真实事件循环 + 网络 IO）
// ════════════════════════════════════════════════════════════════════════

const LISTENER_TOKEN: IoToken = IoToken(0);

fn server() {
    let listener = TcpListener::bind("127.0.0.1:6380").expect("bind 127.0.0.1:6380");
    listener.set_nonblocking(true).expect("nonblocking listener");
    println!("redis_like listening on 127.0.0.1:6380 (axiom blueprint + IoReactor)");

    let mut rt = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let table = shared_table();

    // 两个 reactor：listener（main 管理 accept）+ 连接（runtime 路由事件）
    let mut listener_reactor = default_reactor().expect("listener reactor");
    listener_reactor
        .register(raw_of(&listener), IoInterest::READABLE, LISTENER_TOKEN)
        .expect("register listener");

    let mut io_reactor = default_reactor().expect("io reactor");
    let mut next_conn: usize = 1;

    loop {
        // ── 1. accept 新连接（listener_reactor 的 READABLE）────────────
        if let Ok(events) = listener_reactor.poll(Some(Duration::from_millis(20))) {
            for ev in events {
                if ev.token == LISTENER_TOKEN {
                    loop {
                        match listener.accept() {
                            Ok((stream, addr)) => {
                                stream.set_nonblocking(true).expect("nonblocking");
                                let id = next_conn;
                                next_conn += 1;
                                // 物理：连接进共享表 + 注册 READABLE 事件到 runtime
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
                            Err(_) => break, // WouldBlock 或错误：本批 accept 完
                        }
                    }
                }
            }
        }

        // ── 2. 连接事件 → 全图 tick（io_reactor 非阻塞取事件）──────────
        let _results = rt
            .run_io(&mut io_reactor, Vec::new(), Some(Duration::ZERO))
            .expect("run_io");
    }
}

// ════════════════════════════════════════════════════════════════════════
// 模式 2：基准（无网络：RESP 字节注入解析链，测全图吞吐 + 分配）
// ════════════════════════════════════════════════════════════════════════

/// bench 专用最小拓扑：resp_parser（入口，无入边）→ data_store →
/// resp_encoder → conn_writer（+ aof_writer）；`obs` 控制 observe→monitor
/// 载体（None = 无观测模块）。Parallel 模式入口注入需要无入边机器。
fn bench_spec(obs: Option<WritePolicy>) -> DeploySpec {
    use axiom::deploy::MachineInstance;
    use axiom::link::{LinkKind, LinkSpec, ReadPolicy};
    use axiom::resource::MachinePhysicalSpec;

    let mut spec = DeploySpec::new()
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

/// 分片集群 bench 拓扑（**多入口**：每分片独立解析链——Redis 集群形态，
/// 连接直连节点；Parallel 下两链真并行）。
fn bench_spec_multi_entry() -> DeploySpec {
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
    DeploySpec::new()
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

    // 构造 RESP 命令字节（conn_id = 0，表中无 socket → ConnWriter 是空操作）
    // key "bench" 固定哈希到一个分片；用两把 key 让分片负载均衡（bench_a/bench_b）
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

    // 一、观测载体对比（单 DataStore 基线，Parallel(4)）
    println!("=== redis_like bench A: 观测模块对主路径的影响 ===");
    println!("（Parallel(4)：链接走 channel 载体；monitor 模拟低速观测 20µs/事件）\n");
    MONITOR_WORK_NS.store(20_000, Ordering::Relaxed);

    let configs: [(&str, DeploySpec); 3] = [
        ("baseline (no monitor)", bench_spec(None)),
        ("monitor + Blocking", bench_spec(Some(WritePolicy::Blocking))),
        ("monitor + Dropping", bench_spec(Some(WritePolicy::Dropping))),
    ];

    for (label, spec) in configs {
        let mut rt = build_runtime(RuntimeConfig::parallel(4), &spec);
        inject(&mut rt, batch(&set_a, 1000)); // 预热

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

    // 二、分片集群 vs 单 DataStore：并行分片的真实多核收益
    println!("\n=== redis_like bench B: 分片集群（fan-out + fan-in + 并行分片）===");
    println!("（两把 key 均衡分布到 2 分片；对比 Sequential 单核 vs Parallel 并行分片）\n");

    // 双入口交替注入（每命令一个分片解析链——Redis 集群形态：
    // 连接直连节点，命令并发到达各分片；Parallel 下两链真并行）。
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
    // 单 DataStore 用单入口注入（公平对比：同样 100k 命令；多入口注入对它不适用）
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
        "\n预期：分片 Parallel 用两个 DataStore 线程并行处理（fan-in 汇聚无损语义）；\n\
         sharded Sequential 因多一跳 sharder + 双 AOF 略低于单 DataStore（复杂度守恒：\n\
         拓扑多一跳，绝对成本多一跳；Parallel 并行分片可抵消）。"
    );
}

// ════════════════════════════════════════════════════════════════════════
// 模式 3：AOF 重放（确定性验证：同命令序列 → 同最终状态）
// ════════════════════════════════════════════════════════════════════════

fn replay() {
    // 每次从干净状态开始（AOF append 模式会累积；分片版有两个 AOF 文件）
    let _ = std::fs::remove_file("redis_like.aof");
    let _ = std::fs::remove_file("redis_like_aof_writer_0.aof");
    let _ = std::fs::remove_file("redis_like_aof_writer_1.aof");

    // 1. 分片集群蓝图：命令按 key 哈希路由到 2 个 DataStore（fan-out）。
    //    命令序列跨分片交错；断言不预设哈希分布，只验证分片自洽
    //    （SET/GET 同 key 路由同分片）与终值正确。
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

    // 2. 逐条注入（RESP 字节 → 完整链路：解析 → 分片路由 → 执行 → 观测）
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

    // 3. 断言：确定性结果（跨分片汇聚，值与单 DataStore 版完全一致）
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
        "分片集群必须确定性复现命令结果（fan-out/fan-in 不改变语义）"
    );

    // 4. GET 查询 helper（注入 RESP GET → 全链路 → observe 摘要）
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

    // 5. 跨分片路由自洽：SET/GET 同 key → 同分片 → 值一致
    assert!(get(&mut rt, "k1").contains("Bulk(Some"), "GET k1 应命中其所在分片");
    let k2v = get(&mut rt, "k2");
    assert!(
        k2v.contains("Bulk(Some([51]))"),
        "GET k2 应返回存储值 \"3\"（跨分片路由一致）, got {k2v}"
    );
    assert!(get(&mut rt, "lst").contains("WRONGTYPE"), "list 用 GET 应报 WRONGTYPE");

    // 6. 重启（新 runtime，全新状态）→ 顺序重放双 AOF（每分片独立日志）
    //    → sharder 重新哈希 → 各命令回到正确分片 → 状态重建
    let aof0 = std::fs::read("redis_like_aof_writer_0.aof").unwrap_or_default();
    let aof1 = std::fs::read("redis_like_aof_writer_1.aof").unwrap_or_default();
    assert!(!aof0.is_empty() || !aof1.is_empty(), "两个分片 AOF 至少一个非空");
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
    // 重放后状态 = 原状态（写命令无跨分片依赖 → 双日志重放顺序安全）
    let k2v2 = get(&mut rt2, "k2");
    assert!(
        k2v2.contains("Bulk(Some([51]))"),
        "rt2 (双 AOF 重放后): GET k2 应返回 \"3\", got {k2v2}"
    );
    assert!(get(&mut rt2, "k1").contains("Bulk(Some"));
    assert!(get(&mut rt2, "lst").contains("WRONGTYPE"), "重放后 lst 应存在（WRONGTYPE）");

    // 7. 调试注入（Control 流广播到两分片：debugger → data_store_0.ctrl + data_store_1.ctrl）
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
    // DEBUG SET（旁路控制，不记 AOF）→ 两分片都注入 → GET 任意分片命中
    let obs = dbg(&mut rt3, DebugCmd::Set("dk".into(), "dv".into()));
    assert!(
        obs.iter().any(|s| s.contains("DEBUG SET dk => dv")),
        "DEBUG SET 应被 monitor 观测到: {obs:?}"
    );
    assert!(
        get(&mut rt3, "dk").contains("Bulk(Some"),
        "DEBUG 注入的键值应可被正常 GET 读取（广播两分片，任一分片命中）"
    );
    // DEBUG INFO → 键数量（两分片各报一次）
    let obs = dbg(&mut rt3, DebugCmd::Info);
    assert!(
        obs.iter().any(|s| s.contains("keys=")),
        "DEBUG INFO 应返回统计: {obs:?}"
    );
    // DEBUG FLUSH → 两分片都清空；GET 返回 nil
    let _ = dbg(&mut rt3, DebugCmd::Flush);
    assert!(
        get(&mut rt3, "dk").contains("Bulk(None)"),
        "DEBUG FLUSH 后 GET 应为 nil"
    );

    println!("=== replay OK（分片集群：fan-out + fan-in + 双 AOF + Control 广播）===");
    println!("7 条命令跨分片确定性复现；SET/GET 同 key 路由自洽；双 AOF 重放重建状态；DEBUG 广播两分片");
}

/// 把 `[cmd, args...]` 编码为 RESP 命令字节（与 machines::encode_command 同格式）。
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

/// 从 AOF 字节流定位下一条 RESP 命令的结束位置（按 `*N\r\n` + N×`$len\r\n...` 扫描）。
fn next_command_end(buf: &[u8], from: usize) -> usize {
    let rest = &buf[from..];
    // 简化：本 showcase 的 AOF 每条命令是一个完整 RESP 块，块间无分隔——
    // 直接按 RESP 语法跳过 N 个参数（与 try_parse_command 相同逻辑）。
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
        _ => server(),
    }
}


