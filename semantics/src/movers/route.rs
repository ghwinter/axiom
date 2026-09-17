//! 路由/扇出物理载体（fan-out / fan-in 的 mover 侧）。
//!
//! core 形状层已声明扇出/汇合拓扑（[`Broadcast`](axiom::cell_core::Broadcast)/
//! [`Merge`](axiom::cell_core::Merge)），本模块为其补物理载体：
//! - [`FanOut`]：一个源分发到两个接收者的物理能力（`SRC::Out: Clone` 是分发的
//!   必要条件——多路分发在物理层本质是复制/分发）；
//! - [`FanIn`]：两个相容源合入一个接收者的物理能力（`S2::Out: Into<DST::In>`
//!   与 [`Merge::join`](axiom::cell_core::Merge) 同约束）。
//!
//! **为何不并入 [`Carrier`](crate::movers::carrier::Carrier)**：`Carrier<A,B>` 是
//! 单出单入签名（一个 `A::Out` 流入一个 `B::In`）；fan-out 一对二 / fan-in 二对一
//! 无法用单出单入表达，故以独立 trait 落地（先例：
//! [`spawned_flow`](crate::movers::carrier::spawned_flow) 因 `&mut B::State` 不可
//! 跨线程而独立成 free function）。载体只声明物理成本
//! （cost/obligation/saturation），step 语义复用抽象层
//! （`Broadcast::fire`/`Merge::join`）——模态③/① 分离。
//!
//! 内联令牌（[`InlineFanOut`]/[`InlineFanIn`]）：零分配、内联、同步直通（无饱和
//! 点）——与 [`InlineCarrier`](crate::movers::carrier::InlineCarrier) 同剖面资格
//! （`EmbeddedProfile` 预算零分配）。fan-out **拓扑本身**仍可进既有装配：
//! `Broadcast` 组合封闭为 `PortCell`（概念 3），经
//! `Carrier<Broadcast<...>, DST>` 由 `assemble_profile_gated` 装配（见本模块
//! 测试的 profile 演示）。

use axiom::cell_core::{Broadcast, Merge, PortCell};

use crate::checks::obligation::{DeliveryKind, ObligationClass};
use crate::movers::carrier::{CarrierCost, SaturationPolicy};

/// 扇出载体能力（fan-out）：源 `SRC` 的一个输入产出一个输出，分发到两个
/// 接收者 `R1`/`R2`。
///
/// `SRC::Out: Clone` 约束放 trait where（与 [`Broadcast`] 类型层一致）：
/// 多路分发在物理层本质是复制/分发。三声明（cost/obligation/saturation）与
/// [`Carrier`](crate::movers::carrier::Carrier) 同构——默认 fail-closed 保守。
pub trait FanOut<SRC, R1, R2>
where
    SRC: PortCell,
    SRC::Out: Clone,
    R1: PortCell<In = SRC::Out>,
    R2: PortCell<In = SRC::Out>,
{
    /// 本载体的时空成本声明（默认保守 [`External`](CarrierCost::External)；
    /// 实现者应显式声明真实成本）。
    fn cost() -> CarrierCost {
        CarrierCost::External
    }

    /// 本载体接缝的义务类声明（C10 分化）。默认 fail-closed（资源=External）；
    /// 每个实现者应覆写：resource 取 [`Self::cost`] 同值，有投递语义者再补 delivery 轴。
    fn obligation() -> ObligationClass {
        ObligationClass::default()
    }

    /// 本载体的背压饱和策略声明（A1）。默认 [`Block`](SaturationPolicy::Block)
    /// （保守：不静默丢值）；同步直通载体应声明
    /// [`NotApplicable`](SaturationPolicy::NotApplicable)（无缓冲、无饱和点）。
    fn saturation() -> SaturationPolicy {
        SaturationPolicy::Block
    }

    /// 单步扇出：`SRC` 产出一个输出，分别喂给 `R1`/`R2`，返回各接收者的输出。
    ///
    /// 语义上等价于 `R1::step(sr1, SRC::step(ssrc, input).clone())` 与
    /// `R2::step(sr2, SRC::step(ssrc, input))`（因果数据流，逐路 step）。
    fn fan(
        ssrc: &mut SRC::State,
        sr1: &mut R1::State,
        sr2: &mut R2::State,
        input: SRC::In,
    ) -> (R1::Out, R2::Out);
}

/// 汇合载体能力（fan-in）：`S1` 与 `S2` 的输出合入一个 `DST` 接收者。
///
/// `S2::Out: Into<DST::In>` 约束与 [`Merge::join`](axiom::cell_core::Merge::join)
/// 同构（相容源经 `Into` 合流）。汇合的"顺序"（谁先到）是物理载体的事
/// （T3/Kahn）——抽象层只声明形态。
pub trait FanIn<S1, S2, DST>
where
    S1: PortCell,
    S2: PortCell,
    DST: PortCell<In = S1::Out>,
    S2::Out: Into<DST::In>,
{
    /// 本载体的时空成本声明（默认保守 [`External`](CarrierCost::External)；
    /// 实现者应显式声明真实成本）。
    fn cost() -> CarrierCost {
        CarrierCost::External
    }

    /// 本载体接缝的义务类声明（C10 分化）。默认 fail-closed（资源=External）。
    fn obligation() -> ObligationClass {
        ObligationClass::default()
    }

    /// 本载体的背压饱和策略声明（A1）。默认 [`Block`](SaturationPolicy::Block)。
    fn saturation() -> SaturationPolicy {
        SaturationPolicy::Block
    }

    /// 两步汇合：先驱动 `S1`，再驱动 `S2`，各输出进同一 `DST` 接收者（fan-in）。
    /// 因果顺序由调用决定；物理顺序/仲裁归载体（T3）。`DST` 状态在两 step 间保持。
    fn join(
        ss1: &mut S1::State,
        ss2: &mut S2::State,
        sdst: &mut DST::State,
        in1: S1::In,
        in2: S2::In,
    ) -> DST::Out;
}

/// 内联扇出令牌：零分配、内联、同步直通（无饱和点）。
///
/// 物理形态 = 栈上函数直接传（`Broadcast::fire` 内联展开）；`Clone` 开销即
/// 复制分发本身（分发在物理层本质是复制，诚实声明）。纯 core，no_std 可用。
pub struct InlineFanOut;

impl<SRC, R1, R2> FanOut<SRC, R1, R2> for InlineFanOut
where
    SRC: PortCell,
    SRC::Out: Clone,
    R1: PortCell<In = SRC::Out>,
    R2: PortCell<In = SRC::Out>,
{
    fn cost() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }

    fn obligation() -> ObligationClass {
        ObligationClass {
            delivery: DeliveryKind::NotApplicable, // 同步直通：无投递态义务
            resource: CarrierCost::ZeroAllocInline,
            ..ObligationClass::default()
        }
    }

    fn saturation() -> SaturationPolicy {
        SaturationPolicy::NotApplicable // 同步直通：无缓冲、无饱和点
    }

    #[inline(always)]
    fn fan(
        ssrc: &mut SRC::State,
        sr1: &mut R1::State,
        sr2: &mut R2::State,
        input: SRC::In,
    ) -> (R1::Out, R2::Out) {
        // step 语义复用抽象层（Broadcast::fire）——载体只声明物理成本。
        Broadcast::<SRC, R1, R2>::fire(ssrc, sr1, sr2, input)
    }
}

/// 内联合流令牌：零分配、内联、同步直通（无饱和点）。
///
/// 物理形态 = 栈上函数直接传（`Merge::join` 内联展开）。纯 core，no_std 可用。
pub struct InlineFanIn;

impl<S1, S2, DST> FanIn<S1, S2, DST> for InlineFanIn
where
    S1: PortCell,
    S2: PortCell,
    DST: PortCell<In = S1::Out>,
    S2::Out: Into<DST::In>,
{
    fn cost() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }

    fn obligation() -> ObligationClass {
        ObligationClass {
            delivery: DeliveryKind::NotApplicable, // 同步直通：无投递态义务
            resource: CarrierCost::ZeroAllocInline,
            ..ObligationClass::default()
        }
    }

    fn saturation() -> SaturationPolicy {
        SaturationPolicy::NotApplicable // 同步直通：无缓冲、无饱和点
    }

    #[inline(always)]
    fn join(
        ss1: &mut S1::State,
        ss2: &mut S2::State,
        sdst: &mut DST::State,
        in1: S1::In,
        in2: S2::In,
    ) -> DST::Out {
        // step 语义复用抽象层（Merge::join）——载体只声明物理成本。
        Merge::<S1, S2, DST>::join(ss1, ss2, sdst, in1, in2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Inc;
    impl PortCell for Inc {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x.wrapping_add(1)
        }
    }

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

    struct Sink;
    impl PortCell for Sink {
        type In = (i32, i32);
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), (a, b): (i32, i32)) -> i32 {
            a + b
        }
    }

    #[test]
    fn inline_fan_out_matches_per_branch_step() {
        // fan 等价于逐路 step：对同输入，两分支结果 == 各分支独立 step 同源值。
        let (mut sa, mut sr1, mut sr2) = ((), (), ());
        let (o1, o2) =
            <InlineFanOut as FanOut<Inc, Inc, Double>>::fan(&mut sa, &mut sr1, &mut sr2, 5);
        // mid = Inc(5) = 6；R1 = Inc(6) = 7；R2 = Double(6) = 12。
        assert_eq!(o1, 7);
        assert_eq!(o2, 12);
        // 逐路等价（独立状态顺序执行同一因果链）。
        let mut indep = ();
        let mid = Inc::step(&mut indep, 5);
        assert_eq!(o1, Inc::step(&mut indep, mid.clone()));
        assert_eq!(o2, Double::step(&mut indep, mid));
    }

    #[test]
    fn inline_fan_in_matches_merge_join() {
        // 两源 S1=Inc、S2=Double，各输出经 Into（i32→i32 恒等）合入 DST=Inc。
        let (mut ss1, mut ss2, mut sdst) = ((), (), ());
        let out =
            <InlineFanIn as FanIn<Inc, Double, Inc>>::join(&mut ss1, &mut ss2, &mut sdst, 3, 10);
        // S1: Inc(3)=4 → DST Inc(4)=5；S2: Double(10)=20 → DST Inc(20)=21。
        assert_eq!(out, 21);
        // 与 Merge::join 直接等价（同一实现路径）。
        let (mut ms1, mut ms2, mut mdst) = ((), (), ());
        let expect = Merge::<Inc, Double, Inc>::join(&mut ms1, &mut ms2, &mut mdst, 3, 10);
        assert_eq!(out, expect);
    }

    #[test]
    fn inline_tokens_declare_honestly() {
        // A1/诚实：同步直通无饱和点（N/A）；零分配内联。
        assert_eq!(
            <InlineFanOut as FanOut<Inc, Inc, Double>>::cost(),
            CarrierCost::ZeroAllocInline
        );
        assert_eq!(
            <InlineFanOut as FanOut<Inc, Inc, Double>>::saturation(),
            SaturationPolicy::NotApplicable
        );
        assert_eq!(
            <InlineFanIn as FanIn<Inc, Double, Inc>>::cost(),
            CarrierCost::ZeroAllocInline
        );
        assert_eq!(
            <InlineFanIn as FanIn<Inc, Double, Inc>>::saturation(),
            SaturationPolicy::NotApplicable
        );
    }

    #[test]
    fn broadcast_topology_assembles_under_kernel_profile() {
        // 装配演示：Broadcast 组合封闭为 PortCell（概念 3），扇出拓扑本身可经
        // 既有 Carrier 装配进入注册门剖面——A = Broadcast<SRC,R1,R2>（PortCell，
        // In=SRC::In，Out=(R1::Out,R2::Out)），B = Sink（In=(i32,i32)），
        // C = InlineCarrier（Registered，Kernel 零分配预算）。
        type Fanned = Broadcast<Inc, Inc, Double>;
        let link = crate::checks::profile::assemble_profile_gated::<
            crate::checks::profile::KernelProfile,
            Fanned,
            Sink,
            crate::movers::carrier::InlineCarrier,
        >();
        assert!(link.is_ok(), "fan-out 拓扑应可进 Kernel 装配（零分配内联）");

        // 驱动：src 一个输入 → 两分支（Inc/Double）→ Sink 求和。
        let driver = link.expect("kernel link");
        let mut fan_state = ((), (), ()); // (SRC::State, R1::State, R2::State)
        let mut sdst = ();
        let out = driver(&mut fan_state, &mut sdst, 5);
        // mid = Inc(5) = 6；R1 = Inc(6) = 7；R2 = Double(6) = 12；Sink = 7+12 = 19。
        assert_eq!(out, 19);
    }
}
