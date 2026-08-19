//! mmo —— 多人世界核心子图，用 cell_core + runtime Carrier 重建。
//!
//! 对应旧 mmo（LOGIN/MOVE/SAY/LOGOUT 协议、每玩家视图投影、广播给在线玩家），
//! 但用新核心表达、无需 TCP（模拟客户端事件驱动）：
//! - `PlayerHandler`：解析 line 事件（LOGIN/MOVE/SAY/LOGOUT），有状态（在线玩家表）；
//! - `WorldState`：更新玩家位置，有状态（名字→坐标）；
//! - `ViewProject`：为每个在线玩家投影"世界视图"（无状态纯变换）；
//! - 用 `Broadcast`（多对多）向在线玩家扇出视图。
//!
//! 驱动 runtime 能力：跨玩家广播（Broadcast fan-out）+ 有状态世界 + 每玩家视图投影。

use std::collections::BTreeMap;

use axiom::cell_core::PortCell;

/// 玩家命令事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evt {
    Login { name: String },
    Move { name: String, x: i32, y: i32 },
    Say { name: String, text: String },
    Logout { name: String },
    Ignored,
}

/// 世界视图（每个在线玩家一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub lines: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════
// PlayerHandler —— line 事件 → Evt（有状态：在线玩家集合）
// ═══════════════════════════════════════════════════════════════

pub struct PlayerHandler;
impl PortCell for PlayerHandler {
    type In = String;
    type Out = Evt;
    type State = Vec<String>; // 在线玩家
    fn step(online: &mut Vec<String>, line: String) -> Evt {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("LOGIN") => {
                let name = it.next().unwrap_or("?").to_string();
                if !online.contains(&name) {
                    online.push(name.clone());
                }
                Evt::Login { name }
            }
            Some("MOVE") => {
                let name = it.next().unwrap_or("?").to_string();
                let x = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                let y = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                Evt::Move { name, x, y }
            }
            Some("SAY") => {
                let name = it.next().unwrap_or("?").to_string();
                let rest = line.splitn(3, char::is_whitespace).nth(2).unwrap_or("").to_string();
                Evt::Say { name, text: rest }
            }
            Some("LOGOUT") => {
                let name = it.next().unwrap_or("?").to_string();
                online.retain(|n| n != &name);
                Evt::Logout { name }
            }
            _ => Evt::Ignored,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// WorldState —— 玩家位置（有状态）
// ═══════════════════════════════════════════════════════════════

pub struct WorldState;
impl PortCell for WorldState {
    type In = Evt;
    type Out = Evt; // 透传已应用的事件
    type State = BTreeMap<String, (i32, i32)>;
    fn step(pos: &mut BTreeMap<String, (i32, i32)>, evt: Evt) -> Evt {
        match &evt {
            Evt::Login { name } => { pos.entry(name.clone()).or_insert((0, 0)); }
            Evt::Move { name, x, y } => { pos.insert(name.clone(), (*x, *y)); }
            Evt::Logout { name } => { pos.remove(name); }
            _ => {}
        }
        evt
    }
}

// ═══════════════════════════════════════════════════════════════
// ViewProject —— 把世界投影为每个在线玩家的视图（无状态纯）
// ═══════════════════════════════════════════════════════════════

pub struct ViewProject;
impl PortCell for ViewProject {
    type In = (Evt, BTreeMap<String, (i32, i32)>);
    type Out = View;
    type State = ();
    fn step(_: &mut (), (evt, pos): (Evt, BTreeMap<String, (i32, i32)>)) -> View {
        let mut lines = Vec::new();
        match evt {
            Evt::Login { name } => lines.push(format!("event: {name} joined")),
            Evt::Move { name, x, y } => lines.push(format!("event: {name} -> ({x},{y})")),
            Evt::Say { name, text } => lines.push(format!("event: {name}: {text}")),
            Evt::Logout { name } => lines.push(format!("event: {name} left")),
            Evt::Ignored => {}
        }
        // 广播在线玩家位置（世界视图）。
        let online: Vec<String> = pos.iter().map(|(n, (x, y))| format!("{n}@({x},{y})")).collect();
        lines.push(format!("online: [{}]", online.join(", ")));
        View { lines }
    }
}

// ═══════════════════════════════════════════════════════════════
// 旁路细胞：把 View 转成"为玩家 A / 玩家 B"的广播载荷（多对多）
// ═══════════════════════════════════════════════════════════════

/// 转发一个 View（作为 Broadcast 的 R1 接收者：玩家视角）。
pub struct PlayerA;
impl PortCell for PlayerA {
    type In = View;
    type Out = String;
    type State = ();
    fn step(_: &mut (), v: View) -> String {
        v.lines.into_iter().map(|l| format!("A: {l}")).collect::<Vec<_>>().join("\n")
    }
}

pub struct PlayerB;
impl PortCell for PlayerB {
    type In = View;
    type Out = String;
    type State = ();
    fn step(_: &mut (), v: View) -> String {
        v.lines.into_iter().map(|l| format!("B: {l}")).collect::<Vec<_>>().join("\n")
    }
}
