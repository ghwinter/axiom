//! mmo —— 用 cell_core 四构件 + runtime Carrier 构建的多人世界核心子图（阶段 6 硬化）。
//!
//! 多玩家事件 → 有状态世界 → 通用视图投影 → **按在线名单数据驱动扇出**（N 玩家循环，
//! 非固定双接收者）；解析失败（缺名/坐标非法）为类型化错误计入台账，不静默成 "?"/0。
//! Carrier 演示：`PlayerHandler → WorldState`（Result 车道）经**短路载体**驱动
//! （`ResultCarrier`，Inline 零分配）。
//!
//! 运行：`cargo run --manifest-path runtime/Cargo.toml --example mmo`

mod cells;

use axiom::cell_core::PortCell;
use axiom_runtime::prelude_all::{ResultCarrier, drive_try_carrier};

use cells::{EventErr, PlayerHandler, ViewFor, ViewProject, WorldState};

fn main() {
    println!("=== mmo: cell_core + Carrier 多人世界核心子图 ===\n");

    // 模拟客户端事件流（含畸形命令——须类型化拒绝）。
    let events = [
        "LOGIN alice",
        "LOGIN bob",
        "MOVE alice 1 2",
        "SAY alice hello world",
        "MOVE bob 5 5",
        "LOGIN",              // 畸形：缺名
        "MOVE carol x 1",     // 畸形：坐标非法
        "NOPE xyz",           // 未知命令（值，非错误）
        "LOGOUT bob",
    ];

    let mut handler_state = <PlayerHandler as PortCell>::State::default();
    let mut world_state = <WorldState as PortCell>::State::default();
    let mut err_ledger: Vec<EventErr> = Vec::new();
    let mut discarded = 0usize;

    for line in events {
        match PlayerHandler::step(&mut handler_state, line.to_string()) {
            Err(e) => {
                err_ledger.push(e);
                continue;
            }
            Ok(evt) => {
                let applied = WorldState::step(&mut world_state, evt);
                // 通用视图 → 按在线名单数据驱动扇出（N 玩家）。
                let view = ViewProject::step(&mut (), (applied, world_state.clone()));
                let roster: Vec<String> = world_state.keys().cloned().collect();
                if roster.is_empty() {
                    // 无在线玩家：视图丢弃（正常语义）。
                    discarded += 1;
                } else {
                    for name in &roster {
                        let out = ViewFor::step(&mut (), (name.clone(), view.clone()));
                        println!("  {line:<22} => {out}");
                    }
                }
            }
        }
    }

    // 重放一次正确性：NOPE 为值（Ignored → WorldState 透传，仍算"已处理"）。
    let re = PlayerHandler::step(&mut handler_state, "NOPE again".to_string());
    assert!(matches!(re, Ok(cells::Evt::Ignored)));
    let applied = WorldState::step(&mut world_state, re.unwrap());
    let _ = ViewProject::step(&mut (), (applied, world_state.clone()));

    println!("\n  错误台账（类型化）: {err_ledger:?}  无在线玩家丢弃视图: {discarded}");
    assert_eq!(err_ledger.len(), 2, "两条畸形命令须全部被类型化拒绝");

    // ── Carrier 演示：PlayerHandler -> WorldState（Result 车道经短路载体）──
    let mut sh = <PlayerHandler as PortCell>::State::default();
    let mut sw = <WorldState as PortCell>::State::default();
    let e = drive_try_carrier::<ResultCarrier, PlayerHandler, WorldState, _, _>(
        &mut sh, &mut sw, "MOVE carol 9 9".to_string(),
    );
    println!("  短路载体(ResultCarrier) MOVE carol => {e:?}");

    println!("\nmmo ok: 多玩家事件→世界状态→视图→数据驱动扇出 + 畸形 2 条类型化拒绝（阶段 6 硬化）");
}