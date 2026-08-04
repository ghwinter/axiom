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
// 装配：register + materialize（8 机器 + 7 链接，见 blueprint.rs）
// ════════════════════════════════════════════════════════════════════════

fn build_runtime(cfg: RuntimeConfig, spec: &DeploySpec) -> Runtime {
    let mut rt = Runtime::new(cfg);
    rt.register::<ConnReader>("conn_reader");
    rt.register::<RespParser>("resp_parser");
    rt.register::<DataStore>("data_store");
    rt.register::<RespEncoder>("resp_encoder");
    rt.register::<ConnWriter>("conn_writer");
    rt.register::<AofWriter>("aof_writer");
    rt.register::<Monitor>("monitor");
    rt.register::<Debugger>("debugger");
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

fn bench() {
    const N: usize = 100_000;

    // 构造 RESP 命令字节（conn_id = 0，表中无 socket → ConnWriter 是空操作）
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$5\r\nbench\r\n$5\r\nvalue\r\n".to_vec();
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$5\r\nbench\r\n".to_vec();

    let inject_batch = |rt: &mut Runtime, bytes: &[u8], n: usize| {
        let inputs: Vec<(String, String, Box<dyn std::any::Any + Send>)> = (0..n)
            .map(|_| {
                (
                    "resp_parser".to_string(),
                    "raw".to_string(),
                    Box::new(RawBytes(0, bytes.to_vec())) as Box<dyn std::any::Any + Send>,
                )
            })
            .collect();
        rt.tick(inputs).expect("tick")
    };

    println!("=== redis_like bench: 观测模块对主路径的影响 ===");
    println!("（Parallel(4)：链接走 channel 载体；monitor 模拟低速观测 20µs/事件）\n");

    // 模拟低速观测：真实观测（日志/聚合/磁盘）远慢于主路径
    MONITOR_WORK_NS.store(20_000, Ordering::Relaxed);

    // 三种配置：基线（无观测）/ monitor+Blocking / monitor+Dropping
    let configs: [(&str, DeploySpec); 3] = [
        ("baseline (no monitor)", bench_spec(None)),
        ("monitor + Blocking", bench_spec(Some(WritePolicy::Blocking))),
        ("monitor + Dropping", bench_spec(Some(WritePolicy::Dropping))),
    ];

    for (label, spec) in configs {
        let mut rt = build_runtime(RuntimeConfig::parallel(4), &spec);

        // 预热
        inject_batch(&mut rt, &set_cmd, 1000);

        // SET 基准（一次 tick 批量 N 条：线程只创建一次，测真实吞吐）
        ALLOCS.store(0, Ordering::Relaxed);
        let t0 = Instant::now();
        inject_batch(&mut rt, &set_cmd, N);
        let set_dt = t0.elapsed();
        let set_allocs = ALLOCS.load(Ordering::Relaxed);

        // GET 基准
        ALLOCS.store(0, Ordering::Relaxed);
        let t0 = Instant::now();
        inject_batch(&mut rt, &get_cmd, N);
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

    println!("\n预期：Blocking 载体下观测积压拖慢主路径；Dropping 载体下观测丢弃、主路径接近基线。");
}

// ════════════════════════════════════════════════════════════════════════
// 模式 3：AOF 重放（确定性验证：同命令序列 → 同最终状态）
// ════════════════════════════════════════════════════════════════════════

fn replay() {
    // 每次从干净状态开始（AOF 是 append 模式，二次运行会累积旧日志）
    let _ = std::fs::remove_file("redis_like.aof");
    // 1. 用蓝图 DataStore 生成一组写命令，执行并记录 AOF 行（真实重启路径：
    //    AOF 文件字节 → resp_parser 重新解析 → data_store 重放）
    let mut rt = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());

    let cmds: Vec<&[&[u8]]> = vec![
        &[b"SET", b"k1", b"v1"],
        &[b"INCR", b"k2"],
        &[b"INCR", b"k2"],
        &[b"INCR", b"k2"],
        &[b"LPUSH", b"lst", b"a"],
        &[b"LPUSH", b"lst", b"b"],
        &[b"HSET", b"h", b"f", b"v"],
    ];

    // 2. 逐条注入（RESP 字节 → 完整链路），记录观察输出
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

    // 3. 断言确定性结果
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
        "deterministic replay must reproduce exact command results"
    );

    // 4. 重启（新 runtime，全新状态）→ 用写入的 AOF 文件字节重放恢复
    let aof_bytes = std::fs::read("redis_like.aof").expect("read aof");
    let mut rt2 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let mut restored: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    while cursor < aof_bytes.len() {
        // 一条 RESP 命令 = 一个完整块；这里逐条喂给解析链
        let cmd_end = next_command_end(&aof_bytes, cursor);
        let chunk = aof_bytes[cursor..cmd_end].to_vec();
        cursor = cmd_end;
        let out = rt2
            .tick(vec![(
                "resp_parser".to_string(),
                "raw".to_string(),
                Box::new(RawBytes(0, chunk)) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        for r in out {
            if let ProcessResult::Yield { value, .. } = r {
                if let Ok(boxed) = value.downcast::<(usize, String)>() { let (_, summary) = *boxed;
                    restored.push(summary.clone());
                }
            }
        }
    }

    // 5. 断言：重放后状态 = 原状态（INCR 到 3、list 长度 2、hash 存在）
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
    assert!(get(&mut rt, "k1").contains("Bulk(Some"));
    let k2v = get(&mut rt, "k2");
    assert!(
        k2v.contains("Bulk(Some([51]))"),
        "rt: GET k2 应返回存储值 \"3\" (0x33), got {k2v}"
    );
    let k2v2 = get(&mut rt2, "k2");
    assert!(
        k2v2.contains("Bulk(Some([51]))"),
        "rt2 (AOF 重放后): GET k2 应返回 \"3\", got {k2v2}"
    );
    assert!(get(&mut rt, "lst").contains("WRONGTYPE"), "list 用 GET 应报 WRONGTYPE");
    assert_eq!(restored, observed, "AOF 重放必须逐条复现原执行结果");

    // ── 6. 调试注入（Control 流，经 Debugger → data_store.ctrl）────────
    let mut rt3 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
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
    // DEBUG SET（旁路控制，不记 AOF）→ 正常链路 GET 可见
    let obs = dbg(&mut rt3, DebugCmd::Set("dk".into(), "dv".into()));
    assert!(
        obs.iter().any(|s| s.contains("DEBUG SET dk => dv")),
        "DEBUG SET 应被 monitor 观测到: {obs:?}"
    );
    assert!(
        get(&mut rt3, "dk").contains("Bulk(Some"),
        "DEBUG 注入的键值应可被正常 GET 读取"
    );
    // DEBUG INFO → 键数量
    let obs = dbg(&mut rt3, DebugCmd::Info);
    assert!(
        obs.iter().any(|s| s.contains("keys=")),
        "DEBUG INFO 应返回统计: {obs:?}"
    );
    // DEBUG FLUSH → 清空；GET 返回 nil
    let _ = dbg(&mut rt3, DebugCmd::Flush);
    assert!(
        get(&mut rt3, "dk").contains("Bulk(None)"),
        "DEBUG FLUSH 后 GET 应为 nil"
    );

    println!("=== replay OK ===");
    println!("observed 7 条命令全部按序复现；重启后状态恢复（k1=v1, k2=3, lst 存在, h.f=v）");
    println!("debug: DEBUG SET/INFO/FLUSH 经 Control 流注入，monitor 观测到全部调试事件");
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





