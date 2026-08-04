//! # 网络收包路径 — 装配与驱动
//!
//! 三种运行模式：
//!
//! ```text
//! cargo run --manifest-path runtime/Cargo.toml --example netpath -- --replay  # 确定性重放验证（默认）
//! cargo run --manifest-path runtime/Cargo.toml --example netpath -- --bench   # pcap 重放吞吐
//! ```
//!
//! 数据源：生成的 `packets.pcap`（合成以太/IPv4/TCP 包，确定性重放）。
//! 验证：同一 pcap 重放两次 → 逐包处理结果一致（确定性）；构造已知包
//! → 解析正确性断言。

mod blueprint;
mod machines;

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use axiom::deploy::DeploySpec;
use axiom_runtime::{ProcessResult, Runtime, RuntimeConfig};

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

// ════════════════════════════════════════════════════════════════════════
// pcap 生成：合成以太/IPv4/TCP 包
// ════════════════════════════════════════════════════════════════════════

/// 合成一个以太/IPv4/TCP 包（载荷 = payload；四元组固定 10.0.0.1:1234 → 10.0.0.2:80）。
fn synth_tcp_packet(seq: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(14 + 20 + 20 + payload.len());
    // 以太头（14）
    p.extend_from_slice(&[0xAA; 6]); // dst MAC
    p.extend_from_slice(&[0xBB; 6]); // src MAC
    p.extend_from_slice(&[0x08, 0x00]); // ethertype = IPv4
    // IP 头（20，无选项）
    p.push(0x45); // ver=4, ihl=5
    p.push(0); // DSCP/ECN
    p.extend_from_slice(&((20 + 20 + payload.len()) as u16).to_be_bytes()); // total_len
    p.extend_from_slice(&[0, 0]); // id
    p.extend_from_slice(&[0x40, 0x00]); // flags=DF, frag=0
    p.push(64); // ttl
    p.push(6); // proto = TCP
    p.extend_from_slice(&[0, 0]); // checksum（不校验）
    p.extend_from_slice(&[10, 0, 0, 1]); // src
    p.extend_from_slice(&[10, 0, 0, 2]); // dst
    // TCP 头（20，无选项）
    p.extend_from_slice(&1234u16.to_be_bytes()); // sport
    p.extend_from_slice(&80u16.to_be_bytes()); // dport
    p.extend_from_slice(&seq.to_be_bytes()); // seq
    p.extend_from_slice(&[0; 4]); // ack
    p.push(0x50); // data offset = 5
    p.push(0x18); // flags = PSH|ACK
    p.extend_from_slice(&[0; 2]); // window
    p.extend_from_slice(&[0; 4]); // checksum + urgent
    p.extend_from_slice(payload);
    p
}

/// 把包序列写入 pcap 文件（global header + 每包记录）。
fn write_pcap(path: &str, packets: &[Vec<u8>]) {
    let mut f = std::fs::File::create(path).expect("create pcap");
    f.write_all(&0xa1b2c3d4u32.to_le_bytes()).unwrap(); // magic (little-endian)
    f.write_all(&2u16.to_le_bytes()).unwrap(); // version 2.4
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&[0; 8]).unwrap(); // thiszone, sigfigs
    f.write_all(&65535u32.to_le_bytes()).unwrap(); // snaplen
    f.write_all(&1u32.to_le_bytes()).unwrap(); // linktype = Ethernet
    for p in packets {
        f.write_all(&0u32.to_le_bytes()).unwrap(); // ts_sec
        f.write_all(&0u32.to_le_bytes()).unwrap(); // ts_usec
        f.write_all(&(p.len() as u32).to_le_bytes()).unwrap(); // incl_len
        f.write_all(&(p.len() as u32).to_le_bytes()).unwrap(); // orig_len
        f.write_all(p).unwrap();
    }
}

/// 生成确定性 pcap：`n` 个包，第 0 个载荷为 "hello"，其余 "pkt-<seq>"。
fn gen_pcap(n: usize) -> Vec<Vec<u8>> {
    let mut pkts = Vec::with_capacity(n);
    for i in 0..n {
        let payload = if i == 0 {
            b"hello".to_vec()
        } else {
            format!("pkt-{i}").into_bytes()
        };
        pkts.push(synth_tcp_packet(i as u32, &payload));
    }
    pkts
}

// ════════════════════════════════════════════════════════════════════════
// 装配 + 驱动
// ════════════════════════════════════════════════════════════════════════

fn build_runtime(cfg: RuntimeConfig, spec: &DeploySpec) -> Runtime {
    let mut rt = Runtime::new(cfg);
    rt.register::<PcapReader>("pcap_reader");
    rt.register::<EthParser>("eth_parser");
    rt.register::<IpParser>("ip_parser");
    rt.register::<TcpParser>("tcp_parser");
    rt.register::<AppDeliver>("app_deliver");
    rt.register::<PktStats>("pkt_stats");
    rt.materialize(spec).expect("materialize blueprint");
    rt
}

/// 重放整个 pcap（注入 next 事件驱动 PcapReader），收集 AppDeliver.report 序列。
fn replay_all(rt: &mut Runtime, n_packets: usize) -> Vec<AppReport> {
    let mut reports = Vec::with_capacity(n_packets);
    for _ in 0..n_packets {
        let out = rt
            .tick(vec![(
                "pcap_reader".to_string(),
                "next".to_string(),
                Box::new(()) as Box<dyn std::any::Any + Send>,
            )])
            .expect("tick");
        for r in out {
            if let ProcessResult::Yield { value, .. } = r {
                if let Ok(b) = value.downcast::<AppReport>() {
                    reports.push(*b);
                }
            }
        }
    }
    reports
}

// ════════════════════════════════════════════════════════════════════════
// 模式 1：确定性重放验证
// ════════════════════════════════════════════════════════════════════════

fn replay() {
    const N: usize = 1000;

    // 生成确定性 pcap（含已知载荷 "hello" 的首包）
    let pkts = gen_pcap(N);
    write_pcap("packets.pcap", &pkts);

    // 1. 第一次重放
    let mut rt1 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let reports1 = replay_all(&mut rt1, N);

    // 2. 第二次重放（全新 runtime）——确定性：逐包结果一致
    let mut rt2 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let reports2 = replay_all(&mut rt2, N);

    assert_eq!(reports1, reports2, "同 pcap 重放两次必须逐包一致（确定性）");
    assert_eq!(reports1.len(), N, "每包产生一条 report");

    // 3. 解析正确性：首包载荷 "hello" → 流字节数 ≥ 5；流数 = 1（固定四元组）
    assert_eq!(reports1[0].flows, 1, "单四元组 → 单流");
    assert!(
        reports1[0].bytes >= 5,
        "首包载荷 hello 应计入流字节: {:?}",
        reports1[0]
    );
    // 第 N 包：累计 N 包，流仍为 1，字节 = 全部载荷和
    assert_eq!(reports1[N - 1].flows, 1);
    let expected_bytes: u64 = pkts.iter().map(|p| (p.len() - 14 - 20 - 20) as u64).sum();
    assert_eq!(
        reports1[N - 1].bytes, expected_bytes,
        "流字节 = 所有 TCP 载荷之和"
    );

    // 4. 协议路径：所有包都是 IPv4+TCP → 每包都到达 AppDeliver
    assert_eq!(reports1[N - 1].pkt_id, N as u64, "全部 N 包被交付");

    println!("=== replay OK ===");
    println!("{N} 包重放两次逐包一致；以太→IP→TCP 解析全部通过；载荷聚合正确（{expected_bytes} 字节 / 1 流）");
}

// ════════════════════════════════════════════════════════════════════════
// 模式 2：基准（pcap 重放吞吐 + 每包分配）
// ════════════════════════════════════════════════════════════════════════

fn bench() {
    const N: usize = 100_000;

    let pkts = gen_pcap(N);
    write_pcap("packets.pcap", &pkts);
    let total_bytes: u64 = pkts.iter().map(|p| p.len() as u64).sum();

    // 预热（独立 runtime：不消耗正式重放的 pcap 游标）
    let mut rt0 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let _ = replay_all(&mut rt0, 1000);

    // 测量全链路重放（全新 runtime，从 pcap 头部开始）
    let mut rt = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    ALLOCS.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let reports = replay_all(&mut rt, N);
    let dt = t0.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed);

    assert_eq!(reports.len(), N, "全部包交付");
    println!("=== netpath bench (Sequential, 全链路: pcap→eth→ip→tcp→deliver) ===");
    println!(
        "{:.0} pkt/s, {:.2} MB/s, {:.1} allocs/pkt",
        N as f64 / dt.as_secs_f64(),
        total_bytes as f64 / dt.as_secs_f64() / 1e6,
        allocs as f64 / N as f64
    );
    println!("pcap 总大小: {total_bytes} 字节 / {N} 包（每包含以太+IP+TCP 头 54B + 载荷）");
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--bench") => bench(),
        _ => replay(),
    }
}
