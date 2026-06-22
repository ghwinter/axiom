//! 神经网络训练服务器——基于 axiom 的并发用例。
//!
//! # 架构
//!
//! 6 个 Machine 并发运行，通过 tokio mpsc channels 通信：
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                        数据面 (Data Flow)                         │
//! │                                                                  │
//! │  main ──tick──→ DataLoader ──sample──→ Batcher ──batch──→ Trainer │
//! │                    │                   │              │  │  │     │
//! │                    │                   │              ↓  ↓  ↓     │
//! │                    │                   │         [router]        │
//! │                    │                   │          │     │        │
//! │                    │                   │     loss  │     │        │
//! │                    │                   │     (stdout+obs)        │
//! │                    │                   │          │     │        │
//! │                    │                   │     model_delta        │
//! │                    │                   │          │     │        │
//! │                    │                   │          ↓     ↓        │
//! │                    │                   │     Evaluator  Checkpointer
//! │                    │                   │          │     ↑        │
//! │                    │                   │          ↓ metrics      │
//! │                    │                   │       [router]──┘        │
//! │                    │                   │                          │
//! ├────────────────────┴───────────────────┴──────────────────────────┤
//! │                        观测面 (Observe Flow)                      │
//! │                                                                  │
//! │  所有 Machine .stats ──→ Observer ──.snapshot──→ stdout + file    │
//! │  Trainer.loss / Evaluator.metrics ──→ Observer（跟踪训练指标）    │
//! │  Observer 按 sample_interval_ms 时间间隔采样，避免刷屏            │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! 每个 Machine 运行在独立的 tokio task 上（`TokioRuntime::spawn`）。
//! Output router tasks 负责 fan-out：根据 output enum variant 路由到不同下游。
//! Channel 关闭级联：上游 drop sender → 下游 recv 返回 None → 下游完成 → 级联。

mod types;
mod config;
mod nn;
mod machines;

use clap::{Parser, Subcommand};
use config::Config;
use machines::*;
use types::*;
use tokio::sync::mpsc;
use axiom::port::MachineContext;
use axiom::deploy::DeploySpec;
use axiom_tokio::TokioRuntime;

#[derive(Parser)]
#[command(name = "training_server")]
#[command(about = "基于 axiom 的并发神经网络训练服务器")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动并发训练（6 个 Machine 同时运行）
    Start,
    /// 查询训练状态（读取持久化文件）
    Status,
    /// 交互模式（REPL）
    Interactive,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let config = Config::load_or_default(std::path::Path::new(&cli.config));
    tracing_subscriber::fmt::init();

    match cli.command {
        Commands::Start => run_training_server(config).await,
        Commands::Status => show_status(&config).await,
        Commands::Interactive => run_interactive(config).await,
    }
}

/// 构建声明式 DeploySpec（用于拓扑校验，不直接执行）。
///
/// axiom 的 `DeploySpec` 定义了 "what" 而非 "how"：它描述哪些 Machine 存在、
/// 如何连接，但不执行。`validate()` 用 Kahn 拓扑排序检测循环依赖。
/// 实际执行由 `run_training_server` 用 typed channels 手动编排。
fn build_deploy_spec() -> DeploySpec {
    use axiom::deploy::*;
    use axiom::link::*;
    use axiom::resource::*;

    let mut spec = DeploySpec::new()
        .with_machine(MachineInstance {
            name: "data_loader", machine_type: "DataLoader",
            physical: MachinePhysicalSpec::default(), config_overrides: vec![],
        })
        .with_machine(MachineInstance {
            name: "batcher", machine_type: "Batcher",
            physical: MachinePhysicalSpec::default(), config_overrides: vec![],
        })
        .with_machine(MachineInstance {
            name: "trainer", machine_type: "Trainer",
            physical: MachinePhysicalSpec::default(), config_overrides: vec![],
        })
        .with_machine(MachineInstance {
            name: "evaluator", machine_type: "Evaluator",
            physical: MachinePhysicalSpec::default(), config_overrides: vec![],
        })
        .with_machine(MachineInstance {
            name: "checkpointer", machine_type: "Checkpointer",
            physical: MachinePhysicalSpec::default(), config_overrides: vec![],
        })
        .with_machine(MachineInstance {
            name: "observer", machine_type: "Observer",
            physical: MachinePhysicalSpec::default(), config_overrides: vec![],
        })
        // 数据流链接
        .with_link(LinkSpec::new(
            ("data_loader", "sample"), ("batcher", "sample"),
            LinkKind::Channel { capacity: 128, drop_when_full: false },
        ))
        .with_link(LinkSpec::new(
            ("batcher", "batch"), ("trainer", "batch"),
            LinkKind::Channel { capacity: 128, drop_when_full: false },
        ))
        .with_link(LinkSpec::new(
            ("trainer", "model_delta"), ("evaluator", "model_delta"),
            LinkKind::Channel { capacity: 128, drop_when_full: false },
        ))
        .with_link(LinkSpec::new(
            ("trainer", "model_delta"), ("checkpointer", "model_delta"),
            LinkKind::Channel { capacity: 128, drop_when_full: false },
        ))
        .with_link(LinkSpec::new(
            ("evaluator", "metrics"), ("checkpointer", "metrics"),
            LinkKind::Channel { capacity: 128, drop_when_full: false },
        ))
        // 观测流链接（所有 stats → observer）
        .with_link(LinkSpec::new(
            ("data_loader", "stats"), ("observer", "stats"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ))
        .with_link(LinkSpec::new(
            ("batcher", "stats"), ("observer", "stats"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ))
        .with_link(LinkSpec::new(
            ("trainer", "stats"), ("observer", "stats"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ))
        .with_link(LinkSpec::new(
            ("evaluator", "stats"), ("observer", "stats"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ))
        .with_link(LinkSpec::new(
            ("checkpointer", "stats"), ("observer", "stats"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ))
        // 观测流链接（loss/metrics → observer，用于跟踪训练指标）
        .with_link(LinkSpec::new(
            ("trainer", "loss"), ("observer", "loss"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ))
        .with_link(LinkSpec::new(
            ("evaluator", "metrics"), ("observer", "metrics"),
            LinkKind::Channel { capacity: 256, drop_when_full: true },
        ));
    spec.settings = DeploySettings { cpu_threads: 4, io_threads: 2 };
    spec
}

/// 并发训练服务器——7 个 Machine 同时运行。
///
/// 每个 Machine 用 `TokioRuntime::spawn` 启动为独立的 tokio task，
/// 通过 mpsc channels 通信。Channel 关闭自然级联：上游完成 →
/// drop sender → 下游 recv 返回 None → 下游完成。
async fn run_training_server(config: Config) {
    println!("=== 并发训练服务器启动 ===");
    println!("网络结构: {} → {} → {} → {}",
        config.network.input_size, config.network.hidden1_size,
        config.network.hidden2_size, config.network.output_size);
    println!("数据集大小: {}", config.training.dataset_size);
    println!("并发 Machine 数: 6 (DataLoader/Batcher/Trainer/Evaluator/Checkpointer/Observer)");
    println!();

    // ── 1. 拓扑校验 ──────────────────────────────────────────────
    let spec = build_deploy_spec();
    spec.validate().expect("DeploySpec 拓扑校验失败");
    println!("[topology] DeploySpec 校验通过: {} machines, {} links",
        spec.machines.len(), spec.links.len());

    // ── 2. 配置 Rayon 线程池 ─────────────────────────────────────
    if config.runtime.cpu_threads > 0 {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(config.runtime.cpu_threads)
            .build_global();
    }

    // ── 3. 创建所有 input channels ───────────────────────────────
    let (loader_in_tx, loader_in_rx) = mpsc::channel::<DataLoaderInput>(128);
    let (batcher_in_tx, batcher_in_rx) = mpsc::channel::<BatcherInput>(128);
    let (trainer_in_tx, trainer_in_rx) = mpsc::channel::<TrainerInput>(128);
    let (evaluator_in_tx, evaluator_in_rx) = mpsc::channel::<EvaluatorInput>(128);
    let (checkpointer_in_tx, checkpointer_in_rx) = mpsc::channel::<CheckpointerInput>(128);
    let (observer_in_tx, observer_in_rx) = mpsc::channel::<ObserverInput>(256);

    // ── 4. 创建所有 output channels ──────────────────────────────
    let (loader_out_tx, mut loader_out_rx) = mpsc::channel::<DataLoaderOutput>(128);
    let (batcher_out_tx, mut batcher_out_rx) = mpsc::channel::<BatcherOutput>(128);
    let (trainer_out_tx, mut trainer_out_rx) = mpsc::channel::<TrainerOutput>(128);
    let (evaluator_out_tx, mut evaluator_out_rx) = mpsc::channel::<EvaluatorOutput>(128);
    let (checkpointer_out_tx, mut checkpointer_out_rx) = mpsc::channel::<CheckpointerOutput>(128);
    let (observer_out_tx, mut observer_out_rx) = mpsc::channel::<ObserverOutput>(64);

    // ── 5. spawn output routers (fan-out) ────────────────────────
    // 每个 router 接收 Machine 的 output enum，根据 variant 路由到不同下游。
    // router 在上游 output channel 关闭时自然结束，drop 持有的下游 sender。

    // DataLoader router: sample → batcher, stats → observer
    {
        let batcher_tx = batcher_in_tx.clone();
        let observer_tx = observer_in_tx.clone();
        tokio::spawn(async move {
            while let Some(out) = loader_out_rx.recv().await {
                match out {
                    DataLoaderOutput::sample(s) => { let _ = batcher_tx.send(BatcherInput::sample(s)).await; }
                    DataLoaderOutput::stats(st) => { let _ = observer_tx.send(ObserverInput::stats(st)).await; }
                }
            }
        });
    }

    // Batcher router: batch → trainer, stats → observer
    {
        let trainer_tx = trainer_in_tx.clone();
        let observer_tx = observer_in_tx.clone();
        tokio::spawn(async move {
            while let Some(out) = batcher_out_rx.recv().await {
                match out {
                    BatcherOutput::batch(b) => { let _ = trainer_tx.send(TrainerInput::batch(b)).await; }
                    BatcherOutput::stats(st) => { let _ = observer_tx.send(ObserverInput::stats(st)).await; }
                }
            }
        });
    }

    // Trainer router: loss → stdout + observer, model_delta → evaluator + checkpointer, stats → observer
    {
        let evaluator_tx = evaluator_in_tx.clone();
        let checkpointer_tx = checkpointer_in_tx.clone();
        let observer_tx = observer_in_tx.clone();
        tokio::spawn(async move {
            while let Some(out) = trainer_out_rx.recv().await {
                match out {
                    TrainerOutput::loss(l) => {
                        // 低频打印训练 loss
                        if l.batch_id % 50 == 0 || l.batch_id <= 2 {
                            println!("[trainer] batch={:4} epoch={} loss={:.6}", l.batch_id, l.epoch, l.loss);
                        }
                        // loss 也发送到 observer，用于跟踪训练进度
                        let _ = observer_tx.send(ObserverInput::loss(l)).await;
                    }
                    TrainerOutput::model_delta(d) => {
                        let _ = evaluator_tx.send(EvaluatorInput::model_delta(d.clone())).await;
                        let _ = checkpointer_tx.send(CheckpointerInput::model_delta(d)).await;
                    }
                    TrainerOutput::stats(st) => {
                        let _ = observer_tx.send(ObserverInput::stats(st)).await;
                    }
                }
            }
        });
    }

    // Evaluator router: metrics → checkpointer + observer, stats → observer
    {
        let checkpointer_tx = checkpointer_in_tx.clone();
        let observer_tx = observer_in_tx.clone();
        tokio::spawn(async move {
            while let Some(out) = evaluator_out_rx.recv().await {
                match out {
                    EvaluatorOutput::metrics(m) => {
                        let _ = checkpointer_tx.send(CheckpointerInput::metrics(m.clone())).await;
                        // metrics 也发送到 observer，用于跟踪 eval loss
                        let _ = observer_tx.send(ObserverInput::metrics(m)).await;
                    }
                    EvaluatorOutput::stats(st) => {
                        let _ = observer_tx.send(ObserverInput::stats(st)).await;
                    }
                }
            }
        });
    }

    // Checkpointer router: stats → observer
    {
        let observer_tx = observer_in_tx.clone();
        tokio::spawn(async move {
            while let Some(out) = checkpointer_out_rx.recv().await {
                match out {
                    CheckpointerOutput::stats(st) => { let _ = observer_tx.send(ObserverInput::stats(st)).await; }
                }
            }
        });
    }

    // Observer router: snapshot → stdout
    tokio::spawn(async move {
        while let Some(out) = observer_out_rx.recv().await {
            match out {
                ObserverOutput::snapshot(s) => {
                    let loss_str = s.latest_loss
                        .map(|l| format!("{:.4}", l))
                        .unwrap_or_else(|| "N/A".into());
                    println!("[observe] state={:?} batch={} modules={} loss={}",
                        s.train_state, s.current_batch, s.modules.len(), loss_str);
                }
            }
        }
    });

    // ── 6. spawn 所有 Machine ────────────────────────────────────
    let make_ctx = |name: &'static str| {
        let mut ctx = MachineContext::new(name);
        ctx.set_initial_value(config.clone());
        ctx
    };

    let loader_handle = TokioRuntime::spawn::<DataLoader>(
        make_ctx("data_loader"), loader_in_rx, loader_out_tx);
    let batcher_handle = TokioRuntime::spawn::<Batcher>(
        make_ctx("batcher"), batcher_in_rx, batcher_out_tx);
    let trainer_handle = TokioRuntime::spawn::<Trainer>(
        make_ctx("trainer"), trainer_in_rx, trainer_out_tx);
    let evaluator_handle = TokioRuntime::spawn::<Evaluator>(
        make_ctx("evaluator"), evaluator_in_rx, evaluator_out_tx);
    let checkpointer_handle = TokioRuntime::spawn::<Checkpointer>(
        make_ctx("checkpointer"), checkpointer_in_rx, checkpointer_out_tx);
    let observer_handle = TokioRuntime::spawn::<Observer>(
        make_ctx("observer"), observer_in_rx, observer_out_tx);

    println!("[runtime] 所有 Machine 已 spawn，开始训练...");
    println!();

    // ── 7. 启动训练：发送 Start 信号 + 喂数据 ────────────────────
    // 发送 Start 到有 ctrl 端口的 Machine
    let _ = trainer_in_tx.send(TrainerInput::ctrl(ControlSignal::Start)).await;
    let _ = evaluator_in_tx.send(EvaluatorInput::ctrl(ControlSignal::Start)).await;
    let _ = observer_in_tx.send(ObserverInput::ctrl(ControlSignal::Start)).await;
    // drop 非数据流的 input sender（router 持有 clone）
    drop(trainer_in_tx);
    drop(evaluator_in_tx);
    drop(observer_in_tx);
    drop(batcher_in_tx);      // Batcher 无 ctrl 端口，main 不需要
    drop(checkpointer_in_tx); // Checkpointer 无 ctrl 端口，main 不需要

    // 发送 Start 到 DataLoader + 喂 tick 数据
    let _ = loader_in_tx.send(DataLoaderInput::ctrl(ControlSignal::Start)).await;
    for i in 0..config.training.dataset_size {
        let _ = loader_in_tx.send(DataLoaderInput::tick(i as u64)).await;
    }
    drop(loader_in_tx); // 喂完数据 → DataLoader 最终返回 Done → 级联关闭

    // ── 8. 等待所有 Machine 完成（channel 级联关闭）──────────────
    // 顺序: DataLoader → Batcher → Trainer → Evaluator → Checkpointer → Observer
    loader_handle.await
        .expect("DataLoader task panicked")
        .expect("DataLoader 运行失败");
    println!("[done] DataLoader 完成");

    batcher_handle.await
        .expect("Batcher task panicked")
        .expect("Batcher 运行失败");
    println!("[done] Batcher 完成");

    trainer_handle.await
        .expect("Trainer task panicked")
        .expect("Trainer 运行失败");
    println!("[done] Trainer 完成");

    evaluator_handle.await
        .expect("Evaluator task panicked")
        .expect("Evaluator 运行失败");
    println!("[done] Evaluator 完成");

    checkpointer_handle.await
        .expect("Checkpointer task panicked")
        .expect("Checkpointer 运行失败");
    println!("[done] Checkpointer 完成");

    observer_handle.await
        .expect("Observer task panicked")
        .expect("Observer 运行失败");
    println!("[done] Observer 完成");

    // ── 9. 打印最终结果 ──────────────────────────────────────────
    println!();
    println!("=== 训练完成 ===");
    println!("模型已保存到: {}", config.persist.model_file);
    println!("指标已保存到: {} (Checkpointer)", config.observe.metrics_file);
    println!("快照已保存到: {} (Observer)", config.observe.snapshots_file);
    println!();
    println!("用 `cargo run -p training_server --release -- status` 查看训练指标");
}

/// 查询训练状态——读取持久化的 metrics.jsonl 和 snapshots.jsonl 文件。
async fn show_status(config: &Config) {
    println!("=== 训练状态 ===");

    // 训练指标（Checkpointer 写入）
    if std::path::Path::new(&config.observe.metrics_file).exists() {
        let content = std::fs::read_to_string(&config.observe.metrics_file).unwrap_or_default();
        let lines: Vec<&str> = content.lines().collect();
        let last_n = lines.len().min(10);
        println!("--- 最近 {} 条训练指标 ---", last_n);
        for line in lines[lines.len() - last_n..].iter() {
            if let Ok(metrics) = serde_json::from_str::<Metrics>(line) {
                println!(
                    "[epoch={} batch={:4}] train_loss={:.6} eval_loss={:.6} mae={:.6}",
                    metrics.epoch, metrics.batch_id,
                    metrics.train_loss, metrics.eval_loss, metrics.mae
                );
            }
        }
        println!("指标记录总数: {}", lines.len());
    } else {
        println!("无指标文件，服务器可能未运行过");
    }

    // 系统快照（Observer 写入）
    if std::path::Path::new(&config.observe.snapshots_file).exists() {
        let content = std::fs::read_to_string(&config.observe.snapshots_file).unwrap_or_default();
        let lines: Vec<&str> = content.lines().collect();
        let last_n = lines.len().min(5);
        println!();
        println!("--- 最近 {} 条系统快照 ---", last_n);
        for line in lines[lines.len() - last_n..].iter() {
            if let Ok(snap) = serde_json::from_str::<SystemSnapshot>(line) {
                let loss_str = snap.latest_loss
                    .map(|l| format!("{:.6}", l))
                    .unwrap_or_else(|| "N/A".into());
                let eval_str = snap.latest_eval_loss
                    .map(|l| format!("{:.6}", l))
                    .unwrap_or_else(|| "N/A".into());
                println!(
                    "[{}] state={:?} epoch={} batch={} loss={} eval_loss={} modules={}",
                    snap.timestamp_ms, snap.train_state, snap.current_epoch,
                    snap.current_batch, loss_str, eval_str, snap.modules.len()
                );
            }
        }
        println!("快照记录总数: {}", lines.len());
    }

    // 模型文件
    if std::path::Path::new(&config.persist.model_file).exists() {
        let size = std::fs::metadata(&config.persist.model_file)
            .map(|m| m.len()).unwrap_or(0);
        println!();
        println!("模型文件: {} ({} 字节)", config.persist.model_file, size);
    }
}

/// 交互模式——REPL 循环读取用户命令。
async fn run_interactive(config: Config) {
    use std::io::{self, BufRead, Write};

    println!("=== 交互模式 ===");
    println!("可用命令: status, quit");
    println!("  status  — 显示最近训练指标");
    println!("  quit    — 退出");
    println!();

    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush().unwrap();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap() == 0 {
            break;
        }

        // 去除 UTF-8 BOM（PowerShell 管道可能添加）
        if line.starts_with('\u{FEFF}') {
            line = line[3..].to_string();
        }

        match line.trim().to_lowercase().as_str() {
            "status" => show_status(&config).await,
            "quit" | "exit" | "q" => break,
            "" => continue,
            other => println!("未知命令: {}（可用: status, quit）", other),
        }
    }
    println!("退出交互模式");
}
