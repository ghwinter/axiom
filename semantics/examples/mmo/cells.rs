//! mmo —— 多人世界核心子图，用 cell_core + runtime Carrier 重建（阶段 6 硬化）。
//!
//! 多玩家事件 → 有状态世界 → 每玩家视图投影 → 按在线名单数据驱动扇出（N 玩家，
//! 非固定双接收者）。解析失败由类型化错误表达：缺名/坐标非法不得静默成 "?"/0。
//!
//! - `PlayerHandler`：line 事件（LOGIN/MOVE/SAY/LOGOUT）→ `Result<Evt, EventErr>`，
//!   有状态（在线玩家表）；未知命令为 `Evt::Ignored`（值，非错误——协议噪声丢弃属正常）；
//! - `WorldState`：更新玩家位置（有状态、总函数）；
//! - `ViewProject`：世界 → 通用视图（无状态纯变换）；
//! - `ViewFor`：视图 × 玩家名 → 该玩家的视角行（纯；扇出循环的逐份载荷）。

use std::collections::BTreeMap;

use axiom::cell_core::PortCell;

/// 玩家命令事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evt {
    Login { name: String },
    Move { name: String, x: i32, y: i32 },
    Say { name: String, text: String },
    Logout { name: String },
    /// 未知命令（协议噪声，丢弃属正常语义——与"畸形已知命令"区分）。
    Ignored,
}

/// 解析错误（类型化；取代静默默认 "?"/0）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventErr {
    /// 已知命令缺玩家名。
    MissingName(&'static str),
    /// MOVE 坐标非法（非整型）。
    BadCoord(&'static str),
}

/// 世界视图（在线状态行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub lines: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════
// PlayerHandler —— line 事件 → Result<Evt, EventErr>（有状态：在线玩家表）
// ═══════════════════════════════════════════════════════════════

pub struct PlayerHandler;
impl PortCell for PlayerHandler {
    type In = String;
    type Out = Result<Evt, EventErr>;
    type State = Vec<String>; // 在线玩家
    #[inline(always)]
    fn step(online: &mut Vec<String>, line: String) -> Result<Evt, EventErr> {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("LOGIN") => {
                let name = it.next().ok_or(EventErr::MissingName("LOGIN"))?;
                if !online.contains(&name.to_string()) {
                    online.push(name.to_string());
                }
                Ok(Evt::Login { name: name.to_string() })
            }
            Some("MOVE") => {
                let name = it.next().ok_or(EventErr::MissingName("MOVE"))?;
                let x: i32 = it
                    .next()
                    .ok_or(EventErr::BadCoord("MOVE x"))?
                    .parse()
                    .map_err(|_| EventErr::BadCoord("MOVE x"))?;
                let y: i32 = it
                    .next()
                    .ok_or(EventErr::BadCoord("MOVE y"))?
                    .parse()
                    .map_err(|_| EventErr::BadCoord("MOVE y"))?;
                Ok(Evt::Move { name: name.to_string(), x, y })
            }
            Some("SAY") => {
                let name = it.next().ok_or(EventErr::MissingName("SAY"))?;
                let rest = line.splitn(3, char::is_whitespace).nth(2).unwrap_or("").to_string();
                Ok(Evt::Say { name: name.to_string(), text: rest })
            }
            Some("LOGOUT") => {
                let name = it.next().ok_or(EventErr::MissingName("LOGOUT"))?;
                online.retain(|n| n != name);
                Ok(Evt::Logout { name: name.to_string() })
            }
            _ => Ok(Evt::Ignored),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// WorldState —— 玩家位置（有状态、总函数）
// ═══════════════════════════════════════════════════════════════

pub struct WorldState;
impl PortCell for WorldState {
    type In = Evt;
    type Out = Evt; // 透传已应用的事件
    type State = BTreeMap<String, (i32, i32)>;
    #[inline(always)]
    fn step(pos: &mut BTreeMap<String, (i32, i32)>, evt: Evt) -> Evt {
        match &evt {
            Evt::Login { name } => {
                pos.entry(name.clone()).or_insert((0, 0));
            }
            Evt::Move { name, x, y } => {
                pos.insert(name.clone(), (*x, *y));
            }
            Evt::Logout { name } => {
                pos.remove(name);
            }
            _ => {}
        }
        evt
    }
}

// ═══════════════════════════════════════════════════════════════
// ViewProject —— 把世界投影为通用视图（无状态纯）
// ═══════════════════════════════════════════════════════════════

pub struct ViewProject;
impl PortCell for ViewProject {
    type In = (Evt, BTreeMap<String, (i32, i32)>);
    type Out = View;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), (evt, pos): (Evt, BTreeMap<String, (i32, i32)>)) -> View {
        let mut lines = Vec::new();
        match evt {
            Evt::Login { name } => lines.push(format!("event: {name} joined")),
            Evt::Move { name, x, y } => lines.push(format!("event: {name} -> ({x},{y})")),
            Evt::Say { name, text } => lines.push(format!("event: {name}: {text}")),
            Evt::Logout { name } => lines.push(format!("event: {name} left")),
            Evt::Ignored => {}
        }
        let online: Vec<String> =
            pos.iter().map(|(n, (x, y))| format!("{n}@({x},{y})")).collect();
        lines.push(format!("online: [{}]", online.join(", ")));
        View { lines }
    }
}

// ═══════════════════════════════════════════════════════════════
// ViewFor —— 视图 × 玩家名 → 该玩家的视角行（纯；数据驱动扇出的逐份载荷）
// ═══════════════════════════════════════════════════════════════

pub struct ViewFor;
impl PortCell for ViewFor {
    type In = (String, View);
    type Out = String;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), (name, v): (String, View)) -> String {
        v.lines
            .into_iter()
            .map(|l| format!("{name}: {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_known_commands_are_typed_errors() {
        let mut st = Vec::new();
        assert_eq!(
            PlayerHandler::step(&mut st, "LOGIN".into()),
            Err(EventErr::MissingName("LOGIN"))
        );
        assert_eq!(
            PlayerHandler::step(&mut st, "MOVE alice x 1".into()),
            Err(EventErr::BadCoord("MOVE x"))
        );
        assert_eq!(
            PlayerHandler::step(&mut st, "SAY".into()),
            Err(EventErr::MissingName("SAY"))
        );
        assert_eq!(
            PlayerHandler::step(&mut st, "LOGOUT".into()),
            Err(EventErr::MissingName("LOGOUT"))
        );
        // 畸形输入不产生 "?"/0 污染：状态未被触碰。
        assert!(st.is_empty());
    }

    #[test]
    fn valid_session_and_unknown_command() {
        let mut st = Vec::new();
        assert_eq!(
            PlayerHandler::step(&mut st, "LOGIN alice".into()),
            Ok(Evt::Login { name: "alice".into() })
        );
        assert_eq!(
            PlayerHandler::step(&mut st, "MOVE alice 1 2".into()),
            Ok(Evt::Move { name: "alice".into(), x: 1, y: 2 })
        );
        assert_eq!(st, vec!["alice"]);
        // 未知命令为值，非错误。
        assert_eq!(
            PlayerHandler::step(&mut st, "NOPE xyz".into()),
            Ok(Evt::Ignored)
        );
        assert_eq!(
            PlayerHandler::step(&mut st, "LOGOUT alice".into()),
            Ok(Evt::Logout { name: "alice".into() })
        );
        assert!(st.is_empty(), "LOGOUT 应移出在线表");
    }

    #[test]
    fn world_and_view_are_total_and_projection_works() {
        let mut pos = BTreeMap::new();
        let applied = WorldState::step(
            &mut pos,
            Evt::Move { name: "alice".into(), x: 7, y: 8 },
        );
        let view = ViewProject::step(&mut (), (applied, pos.clone()));
        assert!(view.lines[0].contains("alice -> (7,8)"));
        let out = ViewFor::step(&mut (), ("bob".into(), view));
        assert!(out.contains("bob: event: alice -> (7,8)"));
        assert!(out.contains("bob: online: [alice@(7,8)]"));
    }
}