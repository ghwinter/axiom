//! 部署期模态③接线测试：`assemble_link` / `assemble_seam` = 装配点**一次**校验，
//! 越界 = 装配失败（返回 [`ContractError`]），不进入驱动；通过后经 `drive_link`
//! 的编译期验证 + 零成本路径运行。

use axiom::cell_core::PortCell;
use axiom_semantics::movers::carrier::Carrier;
use axiom_semantics::checks::contract::ContractError;
use axiom_semantics::prelude_all::{
    BoundedCarrier, CarrierCost, InlineCarrier, QueueCarrier, assemble_link, assemble_seam,
};

// 生产 cell：加一（i32 -> i32）。
struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x + 1
    }
}

// 消费 cell：翻倍（i32 -> i32，In == A::Out）。
struct Double;
impl PortCell for Double {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x * 2
    }
}

// 无编译期门的"有界"载体：模态③ 容量校验的对象——模态② 的 `assert_capacity_nonzero`
// 只覆盖自带门的 `BoundedCarrier` 自身，无门载体由模态③ 在部署期承接校验。
struct NoGateBounded<const CAP: usize>;

impl<A, B, const CAP: usize> Carrier<A, B> for NoGateBounded<CAP>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    A::Out: Send + 'static,
{
    fn cost() -> CarrierCost {
        CarrierCost::PerMessageAlloc
    }

    fn flow(sa: &mut A::State, sb: &mut B::State, input: A::In) -> B::Out {
        let mid = A::step(sa, input);
        let (tx, rx) = std::sync::mpsc::sync_channel::<A::Out>(CAP);
        let _ = tx.send(mid);
        let v = rx.recv().expect("channel alive");
        B::step(sb, v)
    }
}

#[test]
fn assemble_link_rejects_budget_violation() {
    // QueueCarrier 声明 PerMessageAlloc > ZeroAllocInline 预算 → 装配失败。
    let link = assemble_link::<Inc, Double, QueueCarrier>(CarrierCost::ZeroAllocInline);
    assert!(matches!(link, Err(ContractError::CostExceeded { .. })));
}

#[test]
fn assemble_link_accepts_budget_and_drives() {
    let link = assemble_link::<Inc, Double, InlineCarrier>(CarrierCost::ZeroAllocInline)
        .expect("Inline 满足零分配预算");
    let (mut sa, mut sb) = ((), ());
    assert_eq!(link(&mut sa, &mut sb, 5), 12); // Inc(5->6) -> Double(6->12)
    assert_eq!(link(&mut sa, &mut sb, 0), 2);
}

#[test]
fn assemble_seam_combines_cost_and_capacity() {
    // 成本 + 容量一次通过：BoundedCarrier<2> 声明 PerMessageAlloc，预算取 PerMessageAlloc。
    let seam = assemble_seam::<Inc, Double, BoundedCarrier<2>, 2>(CarrierCost::PerMessageAlloc)
        .expect("有界接缝满足预算与容量");
    let (mut sa, mut sb) = ((), ());
    assert_eq!(seam(&mut sa, &mut sb, 5), 12);
}

#[test]
fn assemble_seam_rejects_zero_capacity_at_deploy_time() {
    // 模态③ 部署期拒绝 CAP=0（无门载体不由模态② 覆盖，由本入口承接）。
    let seam = assemble_seam::<Inc, Double, NoGateBounded<0>, 0>(CarrierCost::PerMessageAlloc);
    assert!(matches!(seam, Err(ContractError::ZeroCapacity)));
}

#[test]
fn assemble_seam_rejects_cost_and_capacity() {
    // 成本越界：NoGateBounded 声明 PerMessageAlloc > ZeroAllocInline 预算。
    let seam = assemble_seam::<Inc, Double, NoGateBounded<2>, 2>(CarrierCost::ZeroAllocInline);
    assert!(matches!(seam, Err(ContractError::CostExceeded { .. })));
    // 双失败时按校验次序返回（先成本后容量）。
    let seam = assemble_seam::<Inc, Double, NoGateBounded<0>, 0>(CarrierCost::ZeroAllocInline);
    assert!(matches!(
        seam,
        Err(ContractError::CostExceeded { .. }) | Err(ContractError::ZeroCapacity)
    ));
}