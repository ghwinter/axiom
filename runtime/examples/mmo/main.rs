//! mmo —— 用 cell_core 四构件 + runtime Carrier 构建的多人世界核心子图。
//!
//! 重建旧 mmo 的本质：多玩家事件 → 有状态世界 → 每玩家视图 → 广播。
//! 用 `Broadcast`（多对多 fan-out）向在线玩家扇出视图；用 Carrier 驱动
//! 皮层对（InlineCarrier 单线程零分配）。
//!
//! 运行：`cargo run --manifest-path runtime/Cargo.toml --example mmo`

mod cells;

use axiom::cell_core::{Broadcast, PortCell};
use axiom_runtime::carrier::InlineCarrier;
use axiom_runtime::flow::drive_link;

use cells::{PlayerHandler, WorldState};

fn main() {
    println!("=== mmo: cell_core + Carrier 多人世界核心子图 ===\n");

    // 模拟客户端事件流。
    let events = [
        "LOGIN alice",
        "LOGIN bob",
        "MOVE alice 1 2",
        "SAY alice hello world",
        "MOVE bob 5 5",
        "LOGOUT bob",
    ];

    // 皮层状态。
    let mut handler_state = <PlayerHandler as PortCell>::State::default();
    let mut world_state = <WorldState as PortCell>::State::default();

    for line in events {
        // ① 解析行事件（有状态：在线表）。
        let evt = PlayerHandler::step(&mut handler_state, line.to_string());
        // ② 应用世界状态（有状态：位置表）。
        let applied = WorldState::step(&mut world_state, evt);
        // ③+④ Broadcast 内完成"投影 → 分发给玩家 A/B"（多对多 fan-out）：
        //    SRC=ViewProject 处理 (applied, snapshot) 产出 View，克隆给 R1/R2。
        let pos_snapshot = world_state.clone();
        let (view_a, view_b) =
            Broadcast::<cells::ViewProject, cells::PlayerA, cells::PlayerB>::fire(
                &mut (), &mut (), &mut (), (applied, pos_snapshot));
        println!("  {line:<22} => A: {view_a:?} | B: {view_b:?}");
    }

    // ── 用 Carrier 驱动皮层对：PlayerHandler -> WorldState（InlineCarrier）──
    let mut sh = <PlayerHandler as PortCell>::State::default();
    let mut sw = <WorldState as PortCell>::State::default();
    let e = drive_link::<PlayerHandler, WorldState, InlineCarrier>(
        &mut sh, &mut sw, "MOVE carol 9 9".to_string());
    println!("  Carrier 驱动(Inline) MOVE carol => {e:?}");

    println!("\nmmo ok: 多玩家事件→世界状态→视图投影→广播 基于 cell_core + Carrier");
}
