//! miniredis —— redis_like 硬化版：一个可信尺度的 axiom 子系统用例。
//!
//! 引擎 = 组合封闭的单一端口体与三类物理驱动（T6 同一蓝图多物理）：
//!
//! ```text
//!  Engine = Chain< TryChain<CmdParse, DataStore>, StoreDemux >
//!   In: String 命令行        Out: RESP 字符串
//!
//!  物理驱动 (同图、可验证语义等价):
//!  ├─ inline : drive_seq（内联/同步/零分配，单线程）
//!  ├─ pump   : bounded_pump_try（跨线程 + 有界通道 + 阻塞背压；解析错误短路计数）
//!  └─ link   : assemble_link（模态③ 部署期校验成本预算 → drive_link 函数指针）
//!
//! 动态内容（概念 4 的 ∃ 侧）：SlotDrive<Cmd, Result<(Reply,AOF),Error>>
//!  ├─ install DataStore      （可写存储居留项）
//!  └─ swap   ReadOnlyProxy   （同型只读代理 → 运行期换装，§5.9 型位填充）
//!
//! 失败模型：Error = Parse|Store 单枚举（TryChain 共享 E）；解析失败短路、存储失败为值；
//! 账本汇总 ok/err；末尾 T6 断言（pump 与 inline 在共享 Ok 通道上逐位等价）。
//!
//! 运行：cargo run --manifest-path runtime/Cargo.toml --example redis_like [--corpus N]
//!       [--max-keys N] [--max-value N] [--help]
//! 测试：cargo test --manifest-path runtime/Cargo.toml --example redis_like

mod cells;
mod server;

use axiom::cell_core::{Chain, PortCell};
use axiom_runtime::prelude_all::{
    CarrierCost, InlineCarrier, QueueCarrier, SlotDrive, SlotPending, TryChain, assemble_link,
    bounded_pump_try, drive_seq,
};

use cells::{Cmd, Config, DataStore, Error, LineSplit, ReadOnlyProxy, Reply, StoreDemux, StoreErr};
use cells::new_store;

/// 引擎 = 组合封闭的单一端口体：解析（短路）→ 存储（失败为值）→ RESP 编解码。
type Pipe = TryChain<cells::CmdParse, DataStore>;
type Engine = Chain<Pipe, StoreDemux>;

/// 具名状态别名（`Pipe`/`Engine` 的 `PortCell::State` 的具体形态）。
type PipeState = ((), cells::StoreState);
type EngineState = (PipeState, ());

// ── CLI / 配置（零依赖，手写解析）───────────────────────────────────

#[derive(Clone)]
struct Cli {
    corpus: usize,
    max_keys: usize,
    max_value: i64,
    tcp_port: Option<u16>,
    selftcp: bool,
}

const HELP: &str = "\
miniredis —— 一个 axiom 子系统用例（redis_like 硬化版）

用法: cargo run --example redis_like [选项]
  --corpus N      生成 N 条命令的确定性语料（默认 400）
  --max-keys N    键数量上限（默认 10000）
  --max-value N   值上限（默认 1000000）
  --tcp PORT      以 TCP 服务器模式运行（§9.3 IO 接缝首用例；纯 std，Ctrl-C 退出）
  --selftcp       运行事件基座自测（临时端口，断言应答序列）
  --help          显示本帮助
";

fn parse_args() -> Cli {
    let mut cli = Cli {
        corpus: 400,
        max_keys: 10_000,
        max_value: 1_000_000,
        tcp_port: None,
        selftcp: false,
    };
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "--corpus" => {
                i += 1;
                cli.corpus = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(400);
            }
            "--max-keys" => {
                i += 1;
                cli.max_keys = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(10_000);
            }
            "--max-value" => {
                i += 1;
                cli.max_value = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
            }
            "--tcp" => {
                i += 1;
                cli.tcp_port = args.get(i).and_then(|s| s.parse().ok());
            }
            "--selftcp" => cli.selftcp = true,
            other => {
                eprintln!("未知选项: {other}\n{HELP}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    cli
}

// ── 确定性语料（混合有效/解析错误/存储错误/未知命令）──────────────────

fn build_corpus(n: usize) -> String {
    let mut seed: u64 = 0x243F_6A88_85A3_08D3;
    let mut lines: Vec<String> = Vec::new();
    for i in 0..n {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(144_269_504_088_963_407);
        let v = (seed % 21) as i64;
        lines.push(format!("SET k{} {}", i % 23, v));
        lines.push(format!("GET k{}", i % 23));
        if i % 5 == 0 {
            lines.push(format!("INCR k{}", i % 23));
        }
        if i % 7 == 0 {
            lines.push(format!("DEL k{}", i % 23));
        }
        if i % 3 == 0 {
            lines.push(format!("GET k{}", i % 23));
        }
        if i % 4 == 0 {
            lines.push("GET".into()); // 解析错误：缺键
        }
        if i % 9 == 0 {
            lines.push(format!("SET k{} notanumber", i % 23)); // 解析错误：非法值
        }
        if i % 11 == 0 {
            lines.push(format!("NOPE {i}")); // 解析错误：未知命令
        }
        if i % 17 == 0 {
            lines.push(format!("SET big{} {}", i, 999_999_999)); // 存储错误：值越界
        }
    }
    lines.join("\n") + "\n"
}

fn main() {
    let cfg = parse_args();
    println!("=== miniredis: 一个 axiom 子系统（redis_like 硬化版） ===");
    println!("配置: corpus={} max_keys={} max_value={}\n", cfg.corpus, cfg.max_keys, cfg.max_value);

    // ── 0b. TCP IO 接缝（§9.3 事件基座首个实现用例）──
    if cfg.selftcp {
        let replies = server::selftest();
        println!(
            "--selftcp：事件基座自测，{} 条应答（含解析短路/背压/RESP 回写/半闭）",
            replies.len()
        );
        for r in &replies {
            println!("   {r:?}");
        }
        return;
    }
    if let Some(port) = cfg.tcp_port {
        println!("--tcp {port}：启动 TCP 服务器（Ctrl-C 退出）\n");
        server::run_server(&format!("127.0.0.1:{port}"), new_store(cfg_to(&cfg)))
            .expect("server bind");
        return;
    }

    // ── 1. 语料经 LineSplit（有状态跨块缓冲，中途切断一次）──
    let text = build_corpus(cfg.corpus);
    let cut = text.len() / 10 * 7; // 70% 处切断（可能在行中间 → 缓冲）
    let (a, b) = text.split_at(cut);
    let mut buf = String::new();
    let mut lines: Vec<String> = Vec::new();
    lines.extend(LineSplit::step(&mut buf, a.to_string()));
    lines.extend(LineSplit::step(&mut buf, b.to_string()));
    lines.extend(LineSplit::step(&mut buf, String::new()));
    println!("      LineSplit: {} 条命令行（含跨块缓冲）\n", lines.len());

    // ── 2. inline 驱动：Engine 经 drive_seq（内联/同步/零分配）──
    let mut engine: EngineState = (((), new_store(cfg_to(&cfg))), ());
    let replies_inline: Vec<String> =
        drive_seq::<Engine, String, String, Vec<String>>(&mut engine, lines.clone());

    // ── 3. 模态③ 装配：成本预算在装配点校验 → drive_link 函数指针（热路径零税）──
    let link = assemble_link::<Pipe, StoreDemux, InlineCarrier>(CarrierCost::ZeroAllocInline)
        .expect("Inline 满足零分配预算");
    let mut pipe: PipeState = ((), new_store(cfg_to(&cfg)));
    let probe_set = link(&mut pipe, &mut (), "SET probe 1".to_string());
    let probe_get = link(&mut pipe, &mut (), "GET probe".to_string());
    let rejected = assemble_link::<Pipe, StoreDemux, QueueCarrier>(CarrierCost::ZeroAllocInline);
    println!("      模态③: link SET probe=>{probe_set:?}  GET probe=>{probe_get:?}");
    println!("             Queue 超零分配预算 → 装配失败（CostExceeded）={}", rejected.is_err());

    // ── 4. pump 驱动：bounded_pump_try（跨线程 + 有界通道 CAP=16 + 阻塞背压）──
    let pump_cfg = cfg.clone(); // 供跨线程闭包按值捕获（'static）
    let (pump_outs, parse_errs) = bounded_pump_try::<cells::CmdParse, DataStore, Cmd, Error, Vec<String>, 16>(
        || (),
        move || new_store(cfg_to(&pump_cfg)),
        lines.clone(),
    );
    let replies_pump: Vec<String> =
        pump_outs.into_iter().map(|r| StoreDemux::step(&mut (), r)).collect();
    println!(
        "      pump   : 跨线程/背压输出 {} 条（解析短路计数 {}，不污染队列）\n",
        replies_pump.len(),
        parse_errs
    );

    // ── 5. 动态内容（∃）：SlotPending 安装 DataStore → commit 授权 → 换装 ReadOnlyProxy ──
    let mut slot: SlotDrive<Cmd, Result<(Reply, Option<String>), Error>> =
        SlotPending::install::<DataStore>(new_store(cfg_to(&cfg))).commit();
    let r_set = slot.drive(Cmd::Set("slotkey".into(), 42));
    let r_get = slot.drive(Cmd::Get("slotkey".into()));
    slot.swap::<ReadOnlyProxy>(());
    let r_ro = slot.drive(Cmd::Set("slotkey".into(), 1)); // Err(Store(ReadOnly))
    assert_eq!(r_ro, Err(Error::Store(StoreErr::ReadOnly)));
    let aof_captured = match &r_set {
        Ok((_, Some(line))) => line.clone(),
        _ => String::new(),
    };
    println!(
        "      slot   : install DataStore SET=>{r_set:?} GET=>{r_get:?}；swap ReadOnlyProxy SET=>{r_ro:?}"
    );

    // ── 6. 账本与摘要（热路径无断言；断言仅在验收处）──
    let ok_ct = replies_inline.iter().filter(|r| !r.starts_with("-ERR")).count();
    let err_ct = replies_inline.len() - ok_ct;
    println!("\n      账本  : 总 {n} 条命令 → ok {ok_ct} / err {err_ct}（解析+存储错误为值）", n = replies_inline.len());
    println!("             AOF 钩子（Slot SET）: {aof_captured}");
    for (line, resp) in lines.iter().zip(replies_inline.iter()).take(6) {
        println!("             < {line:>10} => {resp:?}");
    }
    if lines.len() > 6 {
        println!("             … 其余 {} 条省略", lines.len() - 6);
    }

    // ── 7. T6 验收断言：pump 与 inline 在共享 Ok 通道上逐位等价 ──
    let mut pstate: PipeState = ((), new_store(cfg_to(&cfg)));
    let mut pump_ref: Vec<String> = Vec::new();
    for line in &lines {
        match <Pipe as PortCell>::step(&mut pstate, line.clone()) {
            Err(Error::Parse(_)) => continue, // 与 pump 的解析短路一致：不进队列
            r => pump_ref.push(StoreDemux::step(&mut (), r)),
        }
    }
    assert_eq!(replies_pump, pump_ref, "T6：有界泵与内联线路在共享 Ok 通道上语义等价");
    let parse_ref = lines
        .iter()
        .filter(|l| matches!(cells::CmdParse::step(&mut (), (*l).clone()), Err(Error::Parse(_))))
        .count();
    assert_eq!(parse_errs, parse_ref, "解析短路计数一致");
    assert!(aof_captured.contains("SET slotkey 42"), "AOF 钩子须记录 SET");

    println!("\nminiredis ok: 组合封闭引擎 + 三物理驱动（T6 等价）+ 模态③装配 + ∃ 换装");
}

fn cfg_to(cli: &Cli) -> Config {
    Config { max_keys: cli.max_keys, max_value: cli.max_value }
}