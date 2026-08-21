//! redis_like —— 用 cell_core 四构件 + runtime Carrier 构建的 KV 服务器管线。
//!
//! 重建旧 redis_like 的核心逻辑（命令管线：解析→分派→存储→编码 + AOF 日志），
//! 但完全用新核心/runtime。管线 = 一组皮层经 `Chain` 组合、`InlineCarrier`/
//! `spawned_flow` 驱动——体现"构建真实多模块程序" + 驱动 runtime 迭代。
//!
//! 运行：`cargo run --manifest-path runtime/Cargo.toml --example redis_like`

mod cells;

use axiom::cell_core::PortCell;
use axiom_runtime::carrier::{InlineCarrier, spawned_flow};
use axiom_runtime::flow::drive_link;

use std::collections::HashMap;

use cells::{Cmd, Config, DataStore, LineSplit, RespEncode};

fn main() {
    println!("=== redis_like: cell_core + Carrier KV 服务器管线 ===\n");

    // ── A. 单线程：InlineCarrier 驱动管线各步（零分配）──
    let mut split_state = <LineSplit as PortCell>::State::default();
    let mut store_state = <DataStore as PortCell>::State::default();

    // 模拟一段连接输入（带换行，含/含错：协议错误、值过大、未知命令）。
    let input = "SET foo 10\nGET foo\nINCR foo\nSET big 999999999999999999\nSET foo notanumber\nGET\nDEL foo\nNOPE x\n"
        .to_string();
    let lines = LineSplit::step(&mut split_state, input);

    let mut aof_log: Vec<String> = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let cmd = cells::CmdParse::step(&mut (), line.clone());
        let (reply, log) = DataStore::step(&mut store_state, cmd);
        let resp = RespEncode::step(&mut (), (reply, log.clone()));
        if let Some(entry) = &log {
            aof_log.push(entry.clone());
        }
        println!("  < {line:>12}  =>  {resp:?}");
    }
    println!("  AOF 日志: {aof_log:?}");

    // ── 配置/资源边界演示：受限服务器拒绝超限（键数、值大小）──
    let mut bounded: <DataStore as PortCell>::State =
        (HashMap::new(), Vec::new(), Config { max_keys: 1, max_value: 1_000 });
    let (r1, _) = DataStore::step(&mut bounded, Cmd::Set("a".into(), 1));
    let (r2, _) = DataStore::step(&mut bounded, Cmd::Set("b".into(), 2));       // 超键数 → 拒
    let (r3, _) = DataStore::step(&mut bounded, Cmd::Set("c".into(), 999_999)); // 值过大 → 拒
    println!("  受限(max_keys=1,max_val=1000): SET a={r1:?} SET b={r2:?} SET c={r3:?}");

    // ── B. 单个 GET/SET 经 Carrier 载体驱动（drive_link + InlineCarrier）──
    // 展示用 runtime Carrier 驱动"皮层对"（DataStore -> RespEncode）作为独立链路。
    let mut sstore = <DataStore as PortCell>::State::default();
    let r = drive_link::<DataStore, RespEncode, InlineCarrier>(
        &mut sstore, &mut (), Cmd::Get("missing".into()));
    println!("  Carrier GET missing => {r}");

    // ── C. 跨线程（spawned_flow）：把 RespEncode 放到工作线程 ──
    // 体现"同逻辑、异构物理"——DataStore 在调用线程, RespEncode 在专用线程。
    let mut sstore2 = <DataStore as PortCell>::State::default();
    let resp = spawned_flow::<DataStore, RespEncode>(
        &mut sstore2,
        || (),
        cells::Cmd::Get("missing".into()),
    );
    println!("  跨线程 GET missing => {resp}");

    assert!(aof_log.contains(&"SET foo 10".to_string()));
    println!("\nredis_like ok: cell_core 多模块管线构造 + Carrier 单线程/跨线程驱动");
}
