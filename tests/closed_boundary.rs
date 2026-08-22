//! 封闭边界（`foundations.md` §8）回执测试。
//!
//! 把定稿的封闭核心边界落到代码可验证的形式：
//! - **组合自封闭**：每个构造子（Chain/Rep/Choice/Opt/Broadcast/Merge/Feedback）
//!   仍是 `PortCell`（概念 1/3），并可任意嵌套；
//! - **统一 T1 判定 `Conforms<EXPECT>`**：同时覆盖 `Wire<A,B>`（布线）与 `Slot<I,O>`（装载）；
//! - **复合仍单元、可驱动、蓝图零大小**（零成本静态路径的证据）。

use axiom::cell_core::{
    Broadcast, Chain, Choice, ChoiceIn, ChoiceOut, Conforms, Feedback, Merge, Opt, PortCell, Rep,
    Slot, Wire, assert_conforms, assert_wiring, blueprint_is_zero_sized, drive,
};

struct Inc; // +1
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x + 1
    }
}

struct Double; // *2
impl PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x * 2
    }
}

struct Acc; // 累加
impl PortCell for Acc {
    type In = i32;
    type Out = i32;
    type State = i32;
    #[inline(always)]
    fn step(s: &mut i32, x: i32) -> i32 {
        *s += x;
        *s
    }
}

/// 泛型证明：`C` 是 `PortCell`（组合自封闭，概念 1/3）。
fn assert_pc<C: PortCell>() {}

#[test]
fn every_constructor_is_a_port_cell_closed() {
    assert_pc::<Inc>();
    assert_pc::<Chain<Inc, Double>>();
    assert_pc::<Rep<3, Inc>>();
    assert_pc::<Choice<Inc, Double>>();
    assert_pc::<Opt<Inc>>();
    assert_pc::<Broadcast<Inc, Inc, Double>>();
    assert_pc::<Merge<Inc, Double, Acc>>();
    assert_pc::<Feedback<Chain<Inc, Double>, Inc>>();
    assert_pc::<Chain<Chain<Inc, Double>, Rep<2, Inc>>>();
}

#[test]
fn unified_conforms_t1_covers_wire_and_slot() {
    // 布线合法经统一 Conforms：Wire<A,B>
    assert_wiring::<Inc, Double>();
    let _: bool = <() as Conforms<Wire<Inc, Double>>>::OK;
    // 装载合规经统一 Conforms：Slot<I,O>
    let _: bool = <Inc as Conforms<Slot<i32, i32>>>::OK;
    assert_conforms::<Slot<i32, i32>, Inc>();
    // 复合单元也是合法线源 / 占据者。
    type Body = Chain<Inc, Double>;
    assert_wiring::<Body, Acc>();
}

#[test]
fn closed_composite_drives_and_is_zero_sized() {
    type Main = Chain<Rep<2, Inc>, Double>; // Inc^2 -> Double
    let mut st = <Main as PortCell>::State::default();
    // x=10 -> Inc 11 -> Inc 12 -> Double 24
    assert_eq!(drive::<Main>(&mut st, 10), 24);
    assert!(blueprint_is_zero_sized::<Main>());
}

#[test]
fn choice_dispatch_and_broadcast_fanout_stay_cells() {
    type C = Choice<Inc, Double>;
    let mut st = <C as PortCell>::State::default();
    assert!(matches!(drive::<C>(&mut st, ChoiceIn::A(5)), ChoiceOut::A(6)));
    assert!(matches!(drive::<C>(&mut st, ChoiceIn::B(5)), ChoiceOut::B(10)));
    // 分叉仍是单元（多对多，无 Tee 树）。
    let (mut ss, mut sr1, mut sr2) = ((), (), ());
    let (o1, o2) = Broadcast::<Inc, Inc, Double>::fire(&mut ss, &mut sr1, &mut sr2, 5);
    // SRC Inc 5->6; R1 Inc 6->7; R2 Double 6->12
    assert_eq!((o1, o2), (7, 12));
}

#[test]
fn merge_fans_in_stays_a_cell() {
    type M = Merge<Inc, Double, Acc>; // Inc/Double -> Acc 累加
    let (mut s1, mut s2, mut sdst) = ((), (), 0i32);
    // Inc(5)=6, Double(5)=10；Acc: 6+10=16
    let out = M::join(&mut s1, &mut s2, &mut sdst, 5, 5);
    assert_eq!(out, 16);
}
