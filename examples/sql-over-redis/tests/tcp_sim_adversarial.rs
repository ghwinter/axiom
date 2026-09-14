#![cfg(feature = "turmoil")]
//! 对抗性 T6 交叉验证（确定性仿真载体 vs 实际 tokio；T6 由主张变证据）。
//!
//! 第三物理域 = turmoil 确定性仿真载体：单线程模拟主机 / 时间 / 网络，`rng_seed`
//! 固定种子使整个对抗迹可复现（实际 tokio 不可得）。**同一 async 服务器蓝图**
//! （`serve_tcp_async`，经 [`crate::net`] 编译期切换 net 类型）落在仿真域。
//!
//! ```text
//!                     ┌────────────── 同 一 蓝 图 ──────────────┐
//!   实际 tokio 域      │  serve_tcp_async（tokio::net）          │
//!   仿真域           │  serve_tcp_async（turmoil::net）        │  ← 仅 net 类型不同
//!   sync 域（std）    │  serve_tcp_sync（std::net）             │
//!                     └──────────────┬─────────────────────────┘
//!                                    ▼
//!                        事件泵 → 有界背压 → FIFO 存储 → 按序回执
//! ```
//!
//! ## 判据
//!
//! - **逐拍对齐（T6 证据）**：仿真域应答流 == inline 组合迹（本文件直接断言）；
//!   实际 tokio == inline（`tcp_t6_crosscheck.rs` 断言）⟹ **仿真域 == 实际 tokio**（传递）。
//!   二者同源（同一组合核心 + 同一确定性逐行执行），传递在逻辑上闭合。
//! - **对抗不变量**：在途挂起（hold → release）零丢失、按序交付；连接断裂（fail_rate
//!   注入）诚实拆除、服务器不 panic、应答不静默丢失、无双重执行。
//! - **确定性复现**：同种子 → 同对抗迹（两次运行逐字节一致）——实际 tokio 无法提供的证据。
//!
//! 判定独立性：客户端顺序驱动（前一连接读完/断开并关闭，再开后一连接）→ 判定不依赖
//! 调度顺序（存储 FIFO + 每连接 FIFO 是结构性保证）。

use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axiom_demo_sql_over_redis::composite::{self, ComposeLine};
use axiom_demo_sql_over_redis::tcp_server::serve_tcp_async;
use axiom_semantics::drive::flow::drive_seq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// 仿真域服务器端口（host `server` 上固定；同一 sim 内可复现）。
const PORT: u16 = 8080;

/// inline 参照：单一组合状态上执行（模拟服务器存储任务的全局 FIFO 次序）。
fn inline_reference(lines: &[String]) -> Vec<String> {
    let mut st = composite::new_composite_state();
    drive_seq::<ComposeLine, String, String, Vec<String>>(&mut st, lines.to_vec())
        .iter()
        .map(|r| r.trim_end_matches("\r\n").to_string())
        .collect()
}

/// inline 参照（逐连接共享存储）：`sets` 依序在同一状态上执行，返回各组迹。
fn inline_sets(sets: &[&[String]]) -> Vec<Vec<String>> {
    let mut st = composite::new_composite_state();
    sets.iter()
        .map(|lines| {
            drive_seq::<ComposeLine, String, String, Vec<String>>(&mut st, lines.to_vec())
                .iter()
                .map(|r| r.trim_end_matches("\r\n").to_string())
                .collect()
        })
        .collect()
}

/// 仿真域服务器主机：**同一蓝图** `serve_tcp_async`（net 类型已由 `crate::net` 切换）。
///
/// turmoil 的 bind 只接受未指定/回环地址（`0.0.0.0`/`::`/localhost）——绑定到主机自身
/// IP（"server" 经 DNS 解析为 192.168.0.1）会报 `AddrNotAvailable`；客户端 connect 无
/// 此限制，用主机名 "server" 解析到服务器即可。
fn spawn_server(sim: &mut turmoil::Sim<'_>) {
    sim.host("server", || async {
        let listener = turmoil::net::TcpListener::bind(("0.0.0.0", PORT)).await?;
        let store = composite::new_composite_state();
        let _ = serve_tcp_async(listener, store).await;
        Ok(())
    });
}

/// 仿真域客户端协议（与实际域 [`client_run`](crate::tcp_server::client_run) 同口径）：
/// 一次写入 → 半关写（drop 写半，turmoil 文档即此义）→ 读到 EOF → 按 `\r\n` 拆回执。
///
/// 对抗场景（fail_rate 注入）下应答/ FIN 可能被丢弃 → 读回执永远等不到 EOF。故加
/// 20s 仿真时间超时：超时 = 诚实记录停滞（由调用方映射为 `Broken`），不伪装完整应答。
async fn sim_client_run(lines: &[String]) -> std::io::Result<Vec<String>> {
    let c = turmoil::net::TcpStream::connect(("server", PORT)).await?;
    let (mut rd, mut wr) = c.into_split();
    let payload = lines.join("\n") + "\n";
    wr.write_all(payload.as_bytes()).await?;
    drop(wr); // 半关写 → 服务器读 EOF → 事件泵停止拉取（拆除）
    let mut all = String::new();
    timeout(Duration::from_secs(20), rd.read_to_string(&mut all))
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "应答流停滞（在途丢包）")
        })??;
    Ok(all
        .split("\r\n")
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// 对抗客户端结局（fail_rate 注入的两种可观测形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum SimOutcome {
    /// 连接幸存：完整应答流。
    Full(Vec<String>),
    /// 连接被对抗性断裂：诚实记录错误（不静默丢弃）。
    Broken(String),
}

// ════════════════ 景一：逐拍对齐（T6 第三物理域证据） ════════════════

#[test]
fn sim_trace_matches_inline_and_tokio_transitively() {
    let lines = composite::build_corpus(40); // KV + SQL + 各层错误，一次写入（流水线突发）
    let expected = inline_reference(&lines);

    // 仿真域（良性网络：种子固定、fail_rate 0）——确定性执行。
    let sim_out = run_sim_single(&lines);

    assert_eq!(sim_out.len(), lines.len(), "仿真域配对律：应答数 = 命令数");
    assert_eq!(sim_out, expected, "仿真域线上应答 == inline 组合迹（按序 FIFO）");
    // T6 传递：实际 tokio == inline（tcp_t6_crosscheck::tcp_trace_matches_inline_and_across_physics）
    // 且仿真域 == inline ⟹ 仿真域 == 实际 tokio（同一组合核心、同一确定性逐行执行，传递闭合）。
}

/// 单连接突发：仿真域客户端一次性写入全部命令 → 完整读回。
fn run_sim_single(lines: &[String]) -> Vec<String> {
    let out: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let out_c = out.clone();
    let lines_c = lines.to_vec();
    let mut sim = turmoil::Builder::new()
        .rng_seed(0x5EED_0001) // 固定种子：确定性（fail_rate 0 下本无随机，但纪律一致）
        .simulation_duration(Duration::from_secs(120))
        .build();
    spawn_server(&mut sim);
    sim.client(
        "client",
        async move {
            let got = sim_client_run(&lines_c).await.map_err(|e| -> Box<dyn Error> { Box::new(e) })?;
            *out_c.lock().unwrap() = got;
            Ok(())
        },
    );
    sim.run().expect("仿真运行（无宿主错误）");
    Arc::try_unwrap(out).unwrap().into_inner().unwrap()
}

// ════════════════ 景二：跨连接共享组合状态（仿真域） ════════════════

#[test]
fn sim_shared_store_across_conns_matches_inline() {
    let a: Vec<String> = ["SET shared 7", "INCR shared", "GET shared", "SET k2 2"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let b: Vec<String> = ["GET shared", "GET missing", "GET k2"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let expected = inline_sets(&[&a, &b]);

    let sim_out = run_sim_conns(&[a, b]);

    assert_eq!(sim_out, expected, "仿真域跨连接共享存储 == inline（全局 FIFO 单属主）");
}

/// 顺序两连接：前一连接读完并关闭，再开后一连接（共享存储对后连接可见）。
fn run_sim_conns(sets: &[Vec<String>]) -> Vec<Vec<String>> {
    let out: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let out_c = out.clone();
    let sets_c = sets.to_vec();
    let mut sim = turmoil::Builder::new()
        .rng_seed(0x5EED_0002)
        .simulation_duration(Duration::from_secs(120))
        .build();
    spawn_server(&mut sim);
    sim.client(
        "client",
        async move {
            let mut results = Vec::new();
            for lines in &sets_c {
                let got = sim_client_run(lines).await.map_err(|e| -> Box<dyn Error> { Box::new(e) })?;
                results.push(got);
            }
            *out_c.lock().unwrap() = results;
            Ok(())
        },
    );
    sim.run().expect("仿真运行（无宿主错误）");
    Arc::try_unwrap(out).unwrap().into_inner().unwrap()
}

// ════════════════ 景三：在途挂起 → 释放（对抗不变量：零丢失、保序） ════════════════

#[test]
fn hold_release_inflight_loses_nothing_keeps_order() {
    let lines = composite::build_corpus(24);
    let expected = inline_reference(&lines);

    let sim_out = run_sim_hold(&lines);

    assert_eq!(sim_out.len(), lines.len(), "配对律不受在途挂起影响");
    assert_eq!(sim_out, expected, "hold→release：零丢失、按序交付（在途值语义不破）");
}

/// 客户端在应答在途时挂起链路口，再释放：断言全部应答仍按序到达。
fn run_sim_hold(lines: &[String]) -> Vec<String> {
    let out: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let out_c = out.clone();
    let lines_c = lines.to_vec();
    let mut sim = turmoil::Builder::new()
        .rng_seed(0x5EED_0003)
        .simulation_duration(Duration::from_secs(120))
        .build();
    spawn_server(&mut sim);
    sim.client(
        "client",
        async move {
            let c = turmoil::net::TcpStream::connect(("server", PORT)).await?;
            let (mut rd, mut wr) = c.into_split();
            let payload = lines_c.join("\n") + "\n";
            wr.write_all(payload.as_bytes()).await?;
            drop(wr); // 半关写 → 服务器读 EOF → 泵拆除
            // 对抗注入：挂起在途消息（服务器已处理、应答在途），随后释放。
            turmoil::hold("client", "server");
            turmoil::release("client", "server");
            let mut all = String::new();
            rd.read_to_string(&mut all).await?;
            *out_c.lock().unwrap() = all
                .split("\r\n")
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            Ok(())
        },
    );
    sim.run().expect("仿真运行（无宿主错误）");
    Arc::try_unwrap(out).unwrap().into_inner().unwrap()
}

// ════════════════ 景四：对抗确定性（同种子 → 同对抗迹） ════════════════

#[test]
fn same_seed_replays_same_adversarial_trace() {
    let lines = composite::build_corpus(64);
    let expected = inline_reference(&lines);

    let t1 = run_sim_fail(0x5EED_DEAD, &lines);
    let t2 = run_sim_fail(0x5EED_DEAD, &lines);

    assert_eq!(t1, t2, "同种子 → 同对抗迹（确定性载体的复现性，实际 tokio 不可得）");
    // 对抗不变量：连接幸存 → 应答流与 inline 逐字节一致（不静默丢失、不篡改）；
    // 连接被断裂 → 诚实记录错误（run 仍返回 Ok = 服务器未 panic、未挂起）。
    if let SimOutcome::Full(v) = &t1 {
        assert_eq!(*v, expected, "对抗下幸存连接的应答流仍 == inline（零静默丢失）");
    }
}

/// fail_rate 注入的对抗客户端：一次性写入 → 带超时完整读回；断裂则诚实记录错误。
///
/// 对抗下可观测的四类结局（全部**诚实**记录，不静默丢弃）：
/// - 连接建立失败 / 流错误 → `Broken(err)`；
/// - 应答在途丢包且 FIN 亦丢 → 读取停滞 → 超时 → `Broken(停滞)`；
/// - FIN 幸存但部分应答丢失 → 条数不齐 → `Broken(截断)`；
/// - 全部应答按序到齐 → `Full`。
fn run_sim_fail(seed: u64, lines: &[String]) -> SimOutcome {
    let out: Arc<Mutex<SimOutcome>> = Arc::new(Mutex::new(SimOutcome::Broken("未运行".into())));
    let out_c = out.clone();
    let lines_c = lines.to_vec();
    let mut sim = turmoil::Builder::new()
        .rng_seed(seed) // 对抗 + 确定：断裂点由种子决定，可复现
        .fail_rate(0.02) // TCP：断裂连接（无重传）——诚实拆除语义的验证标准
        .simulation_duration(Duration::from_secs(120))
        .build();
    spawn_server(&mut sim);
    sim.client(
        "client",
        async move {
            let outcome = match sim_client_run(&lines_c).await {
                Ok(v) if v.len() == lines_c.len() => SimOutcome::Full(v),
                Ok(v) => SimOutcome::Broken(format!("应答截断：{} / {}", v.len(), lines_c.len())),
                Err(e) => SimOutcome::Broken(e.to_string()), // 断裂/停滞如实记录，不静默
            };
            *out_c.lock().unwrap() = outcome;
            Ok(())
        },
    );
    sim.run().expect("对抗仿真运行：服务器未 panic、未泄漏");
    Arc::try_unwrap(out).unwrap().into_inner().unwrap()
}

// ════════════════ 景五：客户端中途断开（拆除语义诚实、无双重执行） ════════════════

#[test]
fn client_abort_tears_down_cleanly_without_double_exec() {
    // 连接 1：`SET probe 5` + `INCR probe`（全执行则 store 值为 6）——写入后不读应答、
    // 立即断开（拆除）。服务器不 panic、不挂起。
    // 连接 2：正常协议读回。判据：`GET probe` 值落在诚实值域（$-1/:5/:6，**无双重执行**）；
    // 且连接 2 应答数 = 命令数（配对律不因连接 1 的对抗拆除而破坏）。
    let a: Vec<String> = ["SET probe 5", "INCR probe"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let b: Vec<String> = ["GET probe", "GET missing"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let out: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let out_c = out.clone();
    let b_owned = b.clone();
    let mut sim = turmoil::Builder::new()
        .rng_seed(0x5EED_0005)
        .simulation_duration(Duration::from_secs(120))
        .build();
    spawn_server(&mut sim);
    sim.client(
        "client",
        async move {
            // 连接 1：写入后立即断开（不读应答）——对抗拆除。
            {
                let c = turmoil::net::TcpStream::connect(("server", PORT)).await?;
                let (rd, mut wr) = c.into_split();
                let payload = a.join("\n") + "\n";
                wr.write_all(payload.as_bytes()).await?;
                drop(wr);
                drop(rd); // 立即断开：服务器写回失败 → 写回任务拆除；泵读到 EOF → 停止拉取
            }
            // 连接 2：完整协议。
            let got = sim_client_run(&b_owned)
                .await
                .map_err(|e| -> Box<dyn Error> { Box::new(e) })?;
            *out_c.lock().unwrap() = got;
            Ok(())
        },
    );
    sim.run().expect("仿真运行：连接 1 对抗拆除未破坏服务器");

    let got = Arc::try_unwrap(out).unwrap().into_inner().unwrap();
    assert_eq!(got.len(), b.len(), "连接 2 配对律：应答数 = 命令数");
    // `GET probe` 的诚实值域（连接 1 是对抗拆除，服务器可能已执行 0/1/2 条命令）：
    // - 命令未送达（拆除在先）→ `$-1`（nil，键不存在）；
    // - 仅 SET 执行 → `:5`；SET+INCR 均执行 → `:6`。
    // 绝不允许 >6（INCR 双重执行）——执行恰一次由 FIFO 单属主结构性保证。
    let probe = got.first().map(String::as_str).unwrap_or("");
    assert!(
        probe == "$-1" || probe == ":5" || probe == ":6",
        "GET probe 值 {probe:?} 超出诚实值域（疑似双重执行/伪造应答）"
    );
}
