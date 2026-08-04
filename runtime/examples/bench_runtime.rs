//! axiom-runtime 性能基准——验证三个核心特性的开销/收益。
//!
//! 运行（release 才有意义）：
//!   cargo run --manifest-path runtime/Cargo.toml --release --example bench_runtime
//!
//! # 基准组
//!
//! 1. **fusion_overhead**：3 级 FusedInline 链，register_fused vs register。
//!    验证融合降低每跳 alloc（R003）：fused 的 ns/tick 应显著低于 non-fused，
//!    且随链长增长差距扩大。
//! 2. **chain_length_scaling**：链长 1/3/6/10，fused vs non-fused。
//!    验证融合收益随链长线性增长（non-fused 每跳 +路由开销，fused 内化）。
//! 3. **io_routing**：ManualReactor + N 个 token，run_io 事件路由吞吐。
//!    验证 IO 事件合并外部 inputs 的开销在合理范围。
//!
//! # 设计说明
//!
//! - 零外部依赖（无 criterion）：用 `std::time::Instant` + 自动迭代计数。
//! - 每次 `tick` 注入一个输入，BFS 传播到终端输出——测单条链的端到端延迟。
//! - Parallel 模式的 tick 每次 spawn 线程，不适合微基准（吞吐由线程创建
//!   开销主导，非路由开销）——故本基准只覆盖 Sequential。

use std::time::{Duration, Instant};

use axiom::declare_ports;
use axiom::deploy::{DeploySpec, MachineInstance};
use axiom::link::{LinkKind, LinkSpec};
use axiom::machine::Machine;
use axiom::port::MachineContext;
use axiom::resource::MachinePhysicalSpec;

use axiom_runtime::{IoEvent, IoInterest, IoToken, ManualReactor, RawIo, Runtime, RuntimeConfig};

// ════════════════════════════════════════════════════════════════════════
// 基准机器：Doubler（×2），满足 FusedInline
// ════════════════════════════════════════════════════════════════════════

declare_ports! {
    pub struct DoublerPorts {
        input type DoublerInput {
            x[Data] => i32,
        }
        output type DoublerOutput {
            y[Data] => i32,
        }
    }
}

pub struct Doubler;
impl Machine for Doubler {
    type State = ();
    type Input = DoublerInput;
    type Output = DoublerOutput;
    type Ports = DoublerPorts;
    type ProcessOutput = axiom::machine::SingleOutput<DoublerOutput>;

    fn name() -> &'static str { "doubler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_ctx: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
    fn process(_s: &mut Self::State, _ctx: &MachineContext, input: DoublerInput) -> Self::ProcessOutput {
        match input {
            DoublerInput::x(n) => axiom::machine::SingleOutput::Yield(DoublerOutput::y(n * 2)),
        }
    }
    fn cleanup(_s: Self::State, _ctx: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}
impl axiom::machine::FusedInline for Doubler {}

// IO 就绪处理机器：收到 IoEvent 输入后产出 readiness 标识值。
declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct IoHandlerPorts {
        input type IoHandlerInput { ready[Data] => IoEvent }
        output type IoHandlerOutput { result[Data] => i64 }
    }
}

pub struct IoHandler;
impl Machine for IoHandler {
    type State = ();
    type Input = IoHandlerInput;
    type Output = IoHandlerOutput;
    type Ports = IoHandlerPorts;
    type ProcessOutput = axiom::machine::SingleOutput<IoHandlerOutput>;

    fn name() -> &'static str { "io_handler" }
    fn config_schema() -> axiom::port::ConfigSchema { axiom::port::ConfigSchema::new() }
    fn init(_ctx: &MachineContext) -> Result<Self::State, axiom::machine::InitError> { Ok(()) }
    fn process(_s: &mut Self::State, _ctx: &MachineContext, input: IoHandlerInput) -> Self::ProcessOutput {
        let IoHandlerInput::ready(event) = input;
        let mut v = 0i64;
        if event.readiness.is_readable() { v += 1; }
        if event.readiness.is_writable() { v += 2; }
        axiom::machine::SingleOutput::Yield(IoHandlerOutput::result(v))
    }
    fn cleanup(_s: Self::State, _ctx: &MachineContext) -> Result<(), axiom::machine::CleanupError> { Ok(()) }
}

// ════════════════════════════════════════════════════════════════════════
// 拓扑构建助手
// ════════════════════════════════════════════════════════════════════════

/// 构建 N 级 Doubler 链：d1 → d2 → ... → dN（全 Inline）。
fn doubler_chain(n: usize) -> DeploySpec {
    let mut spec = DeploySpec::new();
    for i in 1..=n {
        spec = spec.with_machine(MachineInstance::new(
            format!("d{i}"), "doubler", MachinePhysicalSpec::default(),
        ));
    }
    for i in 1..n {
        spec = spec.with_link(LinkSpec::new(
            (format!("d{i}"), "y"),
            (format!("d{}", i + 1), "x"),
            LinkKind::Inline,
        ));
    }
    spec
}

// ════════════════════════════════════════════════════════════════════════
// 基准 harness（极简：自动迭代计数）
// ════════════════════════════════════════════════════════════════════════

struct BenchResult {
    name: String,
    iterations: u64,
    mean_ns: f64,
    p99_ns: f64,
}

impl std::fmt::Display for BenchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ops = if self.mean_ns > 0.0 { 1e9 / self.mean_ns } else { f64::INFINITY };
        write!(
            f,
            "{:<42} {:>7} iters | {:>9.1} ns/iter | p99 {:>9.1} ns | {:>11.0} ops/s",
            self.name, self.iterations, self.mean_ns, self.p99_ns, ops,
        )
    }
}

/// 运行闭包 `f` 若干次（自动选迭代数使总时长 ≥ 200ms），返回统计。
fn bench<F: FnMut()>(name: &str, mut f: F) -> BenchResult {
    // warmup + 迭代数探测
    let target = Duration::from_millis(200);
    let mut iter = 1u64;
    loop {
        let start = Instant::now();
        for _ in 0..iter { f(); }
        let elapsed = start.elapsed();
        if elapsed >= target || iter >= 1_000_000 { break; }
        if elapsed > Duration::from_micros(10) {
            iter = ((target.as_nanos() / elapsed.as_nanos().max(1)) * iter as u128) as u64;
            iter = iter.clamp(1, 1_000_000);
        } else {
            iter *= 10;
        }
    }

    // 采样 5 轮取 mean/p99
    let mut samples: Vec<Duration> = Vec::with_capacity(5);
    for _ in 0..5 {
        let start = Instant::now();
        for _ in 0..iter { f(); }
        samples.push(start.elapsed() / iter as u32);
    }
    samples.sort();
    let mean_ns = samples.iter().map(|d| d.as_nanos() as f64).sum::<f64>() / samples.len() as f64;
    let p99_ns = samples.last().map(|d| d.as_nanos() as f64).unwrap_or(mean_ns);
    let r = BenchResult { name: name.to_string(), iterations: iter, mean_ns, p99_ns };
    println!("{r}");
    r
}

// ════════════════════════════════════════════════════════════════════════
// 基准 1：fusion_overhead —— 融合 vs 非融合（3 级链）
// ════════════════════════════════════════════════════════════════════════

fn bench_fusion_overhead() {
    println!("\n── fusion_overhead: 3 级 Doubler 链，fused vs non-fused ──────");
    let spec = doubler_chain(3);

    // 非融合：register（is_fused_compatible=false，不触发融合）
    let mut rt_plain = Runtime::new(RuntimeConfig::sequential());
    rt_plain.register::<Doubler>("doubler");
    rt_plain.materialize(&spec).expect("materialize plain");

    let plain_result = bench("non-fused (register)", || {
        let out = rt_plain.tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))]).expect("tick");
        debug_assert_eq!(out.len(), 1);
    });

    // 融合：register_fused（3 级 → 1 个 FusedPipeline）
    let mut rt_fused = Runtime::new(RuntimeConfig::sequential());
    rt_fused.register_fused::<Doubler>("doubler");
    rt_fused.materialize(&spec).expect("materialize fused");
    debug_assert_eq!(rt_fused.topology().unwrap().machines.len(), 1, "3 级链应融合为 1");

    let fused_result = bench("fused (register_fused)", || {
        let out = rt_fused.tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))]).expect("tick");
        debug_assert_eq!(out.len(), 1);
    });

    let speedup = plain_result.mean_ns / fused_result.mean_ns;
    println!("  → 融合加速比: {speedup:.2}x (ns/tick: {:.1} → {:.1})",
             plain_result.mean_ns, fused_result.mean_ns);
}

// ════════════════════════════════════════════════════════════════════════
// 基准 2：chain_length_scaling —— 链长 1/3/6/10，fused vs non-fused
// ════════════════════════════════════════════════════════════════════════

fn bench_chain_length_scaling() {
    println!("\n── chain_length_scaling: 链长 1/3/6/10 ──────────────────────");
    println!("{:<42} {:>7}        {:>9}     {:>11}", "chain (fused?)", "iters", "ns/iter", "ops/s");

    for &n in &[1usize, 3, 6, 10] {
        let spec = doubler_chain(n);

        // non-fused
        let mut rt_p = Runtime::new(RuntimeConfig::sequential());
        rt_p.register::<Doubler>("doubler");
        rt_p.materialize(&spec).expect("materialize");
        let rp = bench(&format!("chain={n:>2} non-fused"), || {
            rt_p.tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))]).expect("tick");
        });

        // fused
        let mut rt_f = Runtime::new(RuntimeConfig::sequential());
        rt_f.register_fused::<Doubler>("doubler");
        rt_f.materialize(&spec).expect("materialize");
        let rf = bench(&format!("chain={n:>2} fused"), || {
            rt_f.tick(vec![("d1".to_string(), "x".to_string(), Box::new(3i32))]).expect("tick");
        });

        let speedup = rp.mean_ns / rf.mean_ns;
        println!("  → chain={n:>2}: 融合加速比 {speedup:.2}x (路由开销/跳 ≈ {:.1} ns)",
                 (rp.mean_ns - rf.mean_ns) / n as f64);
    }
}

// ════════════════════════════════════════════════════════════════════════
// 基准 3：io_routing —— ManualReactor 多事件路由吞吐
// ════════════════════════════════════════════════════════════════════════

fn bench_io_routing() {
    println!("\n── io_routing: ManualReactor 事件路由 ────────────────────────");

    // 8 个 IoHandler 机器，各注册一个 token。每轮 poll 产出 8 个事件，
    // run_io 合并外部 inputs 驱动 tick。
    let n_handlers = 8usize;
    let mut spec = DeploySpec::new();
    for i in 0..n_handlers {
        spec = spec.with_machine(MachineInstance::new(
            format!("h{i}"), "io_handler", MachinePhysicalSpec::default(),
        ));
    }

    let mut rt = Runtime::new(RuntimeConfig::sequential());
    rt.register::<IoHandler>("io_handler");
    rt.materialize(&spec).expect("materialize");

    let mut reactor = ManualReactor::new();
    for i in 0..n_handlers {
        rt.register_io(&mut reactor, IoToken(i), &format!("h{i}"), "ready", 0 as RawIo, IoInterest::READABLE)
            .expect("register_io");
    }

    // 预装载事件（每轮 poll 弹出全部）。注意 ManualReactor poll 后 pending 清空，
    // 需要每轮重新 push——所以 bench 闭包包含 push + run_io。
    bench("8-event run_io (push+poll+tick)", || {
        for i in 0..n_handlers {
            reactor.push_event(IoEvent { token: IoToken(i), readiness: IoInterest::READABLE });
        }
        let out = rt.run_io(&mut reactor, Vec::new(), Some(Duration::from_millis(0))).expect("run_io");
        debug_assert_eq!(out.len(), n_handlers, "每个 handler 产出一个输出");
    });

    // 对照：纯外部 inputs（无 IO 事件），同样 8 个机器。Box<dyn Any> 不可
    // Clone，故每轮在闭包内重建 inputs。
    bench("8-external tick (no reactor)", || {
        let external: Vec<(String, String, Box<dyn core::any::Any + Send>)> = (0..n_handlers)
            .map(|i| (format!("h{i}"), "ready".to_string(), Box::new(IoEvent {
                token: IoToken(i), readiness: IoInterest::READABLE,
            }) as Box<dyn core::any::Any + Send>))
            .collect();
        let out = rt.tick(external).expect("tick");
        debug_assert_eq!(out.len(), n_handlers);
    });
}

// ════════════════════════════════════════════════════════════════════════
// main
// ════════════════════════════════════════════════════════════════════════

fn main() {
    println!("axiom-runtime 性能基准");
    println!("═══════════════════════════════════════════════════════════════");

    bench_fusion_overhead();
    bench_chain_length_scaling();
    bench_io_routing();

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("完成。fused 应显著快于 non-fused，且差距随链长增长。");
}
