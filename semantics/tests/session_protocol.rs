//! C1: session-protocol port — ruling + minimal prototype.
//!
//! Ruling (2026-08): compile-time session duality in the current model is carried
//! by (a) the typed-hole duality of T1 (`Slot`/`Wire` conformance: ports are
//! dual by In/Out exchange) and (b) `Choice` tags for protocol branching; a
//! protocol *state machine* is an explicit state-phase cell (concept-1
//! instance, §8.3) — no new mechanism needed. The v0 `is_dual`/`project`
//! (archived E40–E43) thesis is thus resettled as: duality is a type judgment;
//! protocol progress is a value-level state machine; illegal transitions are
//! typed failures (Out = Result), never silent.

use axiom::cell_core::{PortCell, Slot, assert_conforms};

/// 协议消息（显式形态：握手/数据/关闭）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    Hello,
    Data(i32),
    Bye,
}

/// 协议阶段（状态机显式化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Handshake,
    Open,
    Closed,
}

/// 协议端口：阶段状态机 cell——握手后才接受数据；关闭后一切被拒；
/// 违法转移 = 类型化失败（值，不静默）。
pub struct ProtoPort;
impl PortCell for ProtoPort {
    type In = Msg;
    type Out = Result<i32, &'static str>;
    type State = Phase;
    #[inline(always)]
    fn step(p: &mut Phase, m: Msg) -> Result<i32, &'static str> {
        match (*p, m) {
            (Phase::Handshake, Msg::Hello) => {
                *p = Phase::Open;
                Ok(0)
            }
            (Phase::Handshake, _) => Err("not handshaked"), // 未握手即传值 = 违法转移
            (Phase::Open, Msg::Data(x)) => Ok(x),
            (Phase::Open, Msg::Hello) => Err("invalid transition"),
            (Phase::Open, Msg::Bye) => {
                *p = Phase::Closed;
                Err("closed") // 显式关闭（值语义）
            }
            (Phase::Closed, _) => Err("protocol closed"),
        }
    }
}

#[test]
fn protocol_phases_reject_illegal_transitions_as_values() {
    let mut st = Phase::default();
    // 未握手即传值：拒绝（失败为值，不静默）。
    assert_eq!(
        ProtoPort::step(&mut st, Msg::Data(5)),
        Err("not handshaked"),
        "Handshake 阶段传值 = 违法转移（类型化失败）"
    );
    assert_eq!(ProtoPort::step(&mut st, Msg::Hello), Ok(0), "握手（进入 Open）");
    assert_eq!(ProtoPort::step(&mut st, Msg::Data(7)), Ok(7), "Open 阶段正常传值");
    assert_eq!(
        ProtoPort::step(&mut st, Msg::Bye),
        Err("closed"),
        "显式关闭"
    );
    assert_eq!(
        ProtoPort::step(&mut st, Msg::Data(9)),
        Err("protocol closed"),
        "关闭后一切被拒（无静默回退）"
    );
}

#[test]
fn protocol_port_is_a_dual_inhabitant_of_its_slot() {
    // 对偶判定 = 类型层（T1）：协议端口是 Slot<Msg, Result<i32, &str>> 的合法
    // 居留项（In/Out 交换对偶由 Conforms 承载——v0 is_dual 论题的现行形态）。
    assert_conforms::<Slot<Msg, Result<i32, &'static str>>, ProtoPort>();
}