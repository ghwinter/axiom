//! control_seam —— 控制/观测共形示例（宪法 §8；B4）。
//!
//! 控制是值（概念 1 不新增）：
//! - **系统内控制指令流**：`Controller` 的 `Out` = 指令值（`Mode`），经
//!   `State` 写入目标（模式切换）——一个模块控制另一个模块的行为；
//! - **运维控制面**：暂停 = `Opt(None)` 门；换实现 = `SlotPending` →
//!   `SlotDrive::swap`（运行期存在化 → 换装）。
//!
//! 观测面（B1）：每投递经 `ConsoleTelemetry` 输出（输出目的地示例：
//! 控制台；持久化 = 实现者选择）。

use axiom::cell_core::{Opt, PortCell};
use axiom_semantics::prelude_all::{
    BufTelemetry, ConsoleTelemetry, SlotPending, Telemetry, VerdictView,
};

// ── 域：纯 PortCell（库作者暴露的角色）────────────────────────────────────

/// 模式指令（控制编码为值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Inc,
    Double,
}

/// 受控目标：按 `State` 中的模式执行（指令流写入 State = 控制）。
pub struct Target;
impl PortCell for Target {
    type In = i32;
    type Out = i32;
    type State = (Mode, i32); // 模式 + 内部计数（模式切换后计数保留）
    #[inline(always)]
    fn step((mode, acc): &mut (Mode, i32), x: i32) -> i32 {
        match mode {
            Mode::Inc => {
                *acc += x + 1;
            }
            Mode::Double => {
                *acc += x * 2;
            }
        }
        *acc
    }
}

/// 控制器：指令源（把外部指令值键入系统）。
pub struct Controller;
impl PortCell for Controller {
    type In = Mode;
    type Out = Mode;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), m: Mode) -> Mode {
        m
    }
}

// ── 部署：选载体 + 装配 + 驱动 + 观测（deploy 角色）──────────────────────

fn main() {
    // 系统内控制：指令源 cell 直通指令（控制编码为值；指令经 State 写入目标，
    // 不占数据布线——数据流仍经 In/Out 布线，控制与数据分离）。
    let mut ctl = ();
    assert_eq!(Controller::step(&mut ctl, Mode::Inc), Mode::Inc);

    // 观测：投递裁决上报（B1 接线点：裁决处调用）。
    let mut tel = ConsoleTelemetry;

    // 运维控制面（换实现）：型位换装——许可生命周期与代戳契约（陈旧拒绝）
    // 由 slot.rs 测试锁定；此处演示授权后驱动与换装。
    let pending = SlotPending::<Mode, Mode>::install::<Controller>(());
    let mut live = pending.commit();
    {
        let mut seat = live.seat();
        assert_eq!(seat.drive(Mode::Inc), Mode::Inc);
    } // 独占借用结束（陈旧引用由借用检查器排除，模态①）
    live.swap::<Controller>(() /* 换装：换代，新实现接续 */);

    // 指令流驱动：控制目标行为。
    let mut target = (Mode::Inc, 0);
    let r1 = Target::step(&mut target, 5); // Inc: 0+6=6
    assert_eq!(r1, 6);
    target.0 = Mode::Double; // 指令（写 State）→ 行为切换到 Double
    let r2 = Target::step(&mut target, 3); // 6+6=12
    assert_eq!(r2, 12, "指令切换模式，计数保留");

    // 观测：投递裁决上报（B1 接线点：裁决处调用）。
    tel.on_verdict("control-chain", VerdictView::Delivered);

    // 缓冲观测（持久化前的样本面）：收集并核对。
    let mut buf = BufTelemetry::new();
    buf.on_verdict("control-chain-buf", VerdictView::Full);
    buf.on_depth("control-chain-buf", 1);
    assert_eq!(buf.verdicts.len(), 1);
    assert_eq!(buf.depths, vec![("control-chain-buf", 1)]);

    // 运维暂停门：Opt(None) 即停送（门语义）。
    let mut opt: () = <Opt<Controller> as PortCell>::State::default();
    assert_eq!(<Opt<Controller> as PortCell>::step(&mut opt, None), None, "暂停：值不流经");

    println!("control_seam ok: 控制是值（指令流+换装+暂停门），观测接口就绪");
}