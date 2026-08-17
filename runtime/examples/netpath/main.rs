//! # Network receive path — assembly and driving
//!
//! Three run modes:
//!
//! ```text
//! cargo run --manifest-path runtime/Cargo.toml --example netpath -- --replay  # deterministic replay check (default)
//! cargo run --manifest-path runtime/Cargo.toml --example netpath -- --bench   # pcap replay throughput
//! ```
//!
//! Data source: a generated `packets.pcap` (synthetic Ethernet/IPv4/TCP packets, deterministic replay).
//! Verification: replaying the same pcap twice → per-packet results are identical (determinism); crafted
//! known packets → parsing correctness assertions.

mod blueprint;
mod machines;

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use axiom::deploy::DynamicTopology;
use axiom_runtime::{ProcessResult, Runtime, RuntimeConfig};

use machines::*;

// ── Allocation counting (for bench mode) ─────────────────────────────────
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
// pcap generation: synthetic Ethernet/IPv4/TCP packets
// ════════════════════════════════════════════════════════════════════════

/// Synthesize one Ethernet/IPv4/TCP packet (payload = payload; fixed 4-tuple 10.0.0.1:1234 → 10.0.0.2:80).
fn synth_tcp_packet(seq: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(14 + 20 + 20 + payload.len());
    // Ethernet header (14)
    p.extend_from_slice(&[0xAA; 6]); // dst MAC
    p.extend_from_slice(&[0xBB; 6]); // src MAC
    p.extend_from_slice(&[0x08, 0x00]); // ethertype = IPv4
    // IP header (20, no options)
    p.push(0x45); // ver=4, ihl=5
    p.push(0); // DSCP/ECN
    p.extend_from_slice(&((20 + 20 + payload.len()) as u16).to_be_bytes()); // total_len
    p.extend_from_slice(&[0, 0]); // id
    p.extend_from_slice(&[0x40, 0x00]); // flags=DF, frag=0
    p.push(64); // ttl
    p.push(6); // proto = TCP
    p.extend_from_slice(&[0, 0]); // checksum (not verified)
    p.extend_from_slice(&[10, 0, 0, 1]); // src
    p.extend_from_slice(&[10, 0, 0, 2]); // dst
    // TCP header (20, no options)
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

/// Write the packet sequence to a pcap file (global header + per-packet records).
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

/// Generate a deterministic pcap: `n` packets, the 0th payload is "hello", the rest are "pkt-<seq>".
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
// Assembly + driving
// ════════════════════════════════════════════════════════════════════════

fn build_runtime(cfg: RuntimeConfig, spec: &DynamicTopology) -> Runtime {
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

/// Replay the entire pcap (injecting next events to drive PcapReader), collecting the AppDeliver.report sequence.
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
// Mode 1: deterministic replay check
// ════════════════════════════════════════════════════════════════════════

fn replay() {
    const N: usize = 1000;

    // generate a deterministic pcap (first packet carries the known "hello" payload)
    let pkts = gen_pcap(N);
    write_pcap("packets.pcap", &pkts);

    // 1. first replay
    let mut rt1 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let reports1 = replay_all(&mut rt1, N);

    // 2. second replay (fresh runtime) — determinism: per-packet results are identical
    let mut rt2 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let reports2 = replay_all(&mut rt2, N);

    assert_eq!(reports1, reports2, "replaying the same pcap twice must match packet by packet (determinism)");
    assert_eq!(reports1.len(), N, "each packet produces one report");

    // 3. parsing correctness: first packet's "hello" payload → stream bytes ≥ 5; stream count = 1 (fixed 4-tuple)
    assert_eq!(reports1[0].flows, 1, "single 4-tuple → single stream");
    assert!(
        reports1[0].bytes >= 5,
        "first packet's hello payload should count toward stream bytes: {:?}",
        reports1[0]
    );
    // Nth packet: N packets accumulated, still 1 stream, bytes = sum of all payloads
    assert_eq!(reports1[N - 1].flows, 1);
    let expected_bytes: u64 = pkts.iter().map(|p| (p.len() - 14 - 20 - 20) as u64).sum();
    assert_eq!(
        reports1[N - 1].bytes, expected_bytes,
        "stream bytes = sum of all TCP payloads"
    );

    // 4. protocol path: all packets are IPv4+TCP → every packet reaches AppDeliver
    assert_eq!(reports1[N - 1].pkt_id, N as u64, "all N packets delivered");

    println!("=== replay OK ===");
    println!("{N} packets replayed twice packet-for-packet identical; Ethernet→IP→TCP parsing all passed; payload aggregation correct ({expected_bytes} bytes / 1 stream)");
}

// ════════════════════════════════════════════════════════════════════════
// Mode 2: benchmark (pcap replay throughput + per-packet allocation)
// ════════════════════════════════════════════════════════════════════════

fn bench() {
    const N: usize = 100_000;

    let pkts = gen_pcap(N);
    write_pcap("packets.pcap", &pkts);
    let total_bytes: u64 = pkts.iter().map(|p| p.len() as u64).sum();

    // warm-up (independent runtime: does not consume the official replay's pcap cursor)
    let mut rt0 = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    let _ = replay_all(&mut rt0, 1000);

    // measure full-path replay (fresh runtime, starting from the pcap header)
    let mut rt = build_runtime(RuntimeConfig::sequential(), &blueprint::blueprint());
    ALLOCS.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let reports = replay_all(&mut rt, N);
    let dt = t0.elapsed();
    let allocs = ALLOCS.load(Ordering::Relaxed);

    assert_eq!(reports.len(), N, "all packets delivered");
    println!("=== netpath bench (Sequential, full path: pcap→eth→ip→tcp→deliver) ===");
    println!(
        "{:.0} pkt/s, {:.2} MB/s, {:.1} allocs/pkt",
        N as f64 / dt.as_secs_f64(),
        total_bytes as f64 / dt.as_secs_f64() / 1e6,
        allocs as f64 / N as f64
    );
    println!("pcap total size: {total_bytes} bytes / {N} packets (each with Ethernet+IP+TCP headers 54B + payload)");
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--bench") => bench(),
        _ => replay(),
    }
}
