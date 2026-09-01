//! 剖面目录（六元组标准化 C 构件；meta-foundations 命题 7.1 的 F↦C(F) 代码形态）。
//!
//! 每个剖面 = {允许载体集（文档化白名单）、义务类下限、成本预算}。剖面是类型级令牌
//! （模态①）：同一拓扑在不同剖面下装配，即 T6"同一图层换物理"（内核剖面拒绝
//! 每消息分配载体，工具剖面默认内联）。
//!
//! **诚实声明（A5）**：受开放 `Carrier` impl 约束，载体白名单不可在类型层强制
//! （任何实现者都可自行 `impl Carrier`）；类型层可强制的是成本预算与义务下限——
//! 装配点按模态③ 校验（[`assemble_profile`]）。白名单因此是规范文档（S/L 构件），
//! 预算是可执行投影（T 构件）。
//!
//! **义务下限（C10 step 2，已启用）**：`obligation_min()` 已按剖面分化并经
//! [`contract::validate_obligation_min`](crate::checks::contract::validate_obligation_min)
//! 在装配点强制（模态③）——载体的义务声明不得弱于剖面下限：
//! - **资源轴**：声明 ≤ 下限（更省不违规）；Kernel/Embedded 下限零分配，
//!   Service 下限每消息，Tool 无资源下限。
//! - **投递态轴**（强度序 `NotApplicable < MechanizedFullClosed`，见
//!   [`DeliveryKind::is_at_least`](crate::checks::obligation::DeliveryKind::is_at_least)）：
//!   Service 下限机械化 Full/Closed（直通接缝 N/A 不满足——服务投递接缝必须
//!   机械化）；Kernel/Embedded/Tool 无投递态下限（任何声明均满足）。
//! - **引用有效/生命周期轴**：本阶段不参与校验（保留声明、不予判定，A5 诚实；
//!   参与待义务账本第三阶段）。
//!
//! 剖面（meta 命题 7.1 的实例表）：
//! - **[`KernelProfile`]**：内核形式。白名单 InlineCarrier、BoundedCarrier（有门）；
//!   预算 ZeroAllocInline（零分配义务）；无超时义务（`Delivery` 级 Timeout/Cancelled 保持④ 声明）。
//! - **[`ServiceProfile`]**：服务形式。白名单 BoundedCarrier、BoundedMailbox、spawned_flow；
//!   预算 PerMessageAlloc；Full/Closed 机械化下限（delivery.rs）。
//! - **[`ToolProfile`]**：工具形式。默认 InlineCarrier；预算 External；无义务下限。
//! - **[`EmbeddedProfile`]**：嵌入式形式（no_std）。预算 ZeroAllocInline（稳态每消息）；

use axiom::cell_core::PortCell;

use crate::movers::carrier::{Carrier, CarrierCost, SaturationPolicy};
use crate::checks::contract::ContractError;
use crate::drive::flow::{Driver, drive_link};
use crate::checks::obligation::{DeliveryKind, ObligationClass};

/// 剖面令牌（模态①）：声明"本系统按哪个软件形式的分域承诺装配"。
pub trait Profile {
    /// 成本预算（经验-D：界 + 装配校验）。
    fn cost_budget() -> CarrierCost;
    /// 义务类下限（逻辑-D：装配点的义务不弱于该下限；模态③ 于 [`assemble_profile`]
    /// 校验）。已按剖面分化（C10 step 2）：资源轴声明 ≤ 下限、投递态轴声明 ≥ 下限；
    /// 引用/生命周期轴保持声明、不予判定（A5 诚实）。
    fn obligation_min() -> ObligationClass;
    /// 饱和下限（A1；模态③ 于 [`assemble_profile`] 校验）。载体的
    /// [`Carrier::saturation`] 不得弱于该下限（
    /// [`SaturationPolicy::meets_saturation_floor`] 偏序）。默认 `NotApplicable`
    /// ——无饱和义务（外松）；需要"不得静默丢弃"的剖面声名更高档。取值随剖面
    /// 分化，为设计决断，非命题结论。
    fn saturation_floor() -> SaturationPolicy {
        SaturationPolicy::NotApplicable
    }
    /// 是否为注册门剖面（C3）：若为 `true`，装配须经
    /// [`assemble_profile_gated`]（编译期要求 `C: Registered`——白名单升为
    /// 模态①事实；未注册载体编译失败）。
    const GATED: bool = false;
}

/// 内核形式剖面（F = kernel）：注册门（C3）——仅注册载体可装配。
pub struct KernelProfile;
impl Profile for KernelProfile {
    const GATED: bool = true;
    fn cost_budget() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass {
            delivery: DeliveryKind::NotApplicable, // 内核接缝同步直通：无投递态义务
            resource: CarrierCost::ZeroAllocInline, // 零分配为义务下限
            ..ObligationClass::default()
        }
    }
    fn saturation_floor() -> SaturationPolicy {
        SaturationPolicy::NotApplicable // 同步直通内核：无饱和点
    }
}

/// 服务形式剖面（F = service/server）：注册门（C3）。
pub struct ServiceProfile;
impl Profile for ServiceProfile {
    const GATED: bool = true;
    fn cost_budget() -> CarrierCost {
        CarrierCost::PerMessageAlloc
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass {
            delivery: DeliveryKind::MechanizedFullClosed, // 服务投递接缝必须机械化 Full/Closed
            resource: CarrierCost::PerMessageAlloc,
            ..ObligationClass::default()
        }
    }
    fn saturation_floor() -> SaturationPolicy {
        // 服务投递接缝必须背压不丢弃（Block）：会丢（Drop*/Fail）的载体在装配点被拒。
        SaturationPolicy::Block
    }
}

/// 工具形式剖面（F = tool/CLI）：无义务下限（外松全集）。
pub struct ToolProfile;
impl Profile for ToolProfile {
    fn cost_budget() -> CarrierCost {
        CarrierCost::External
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass::default() // N/A + External + 无引用/生命周期承诺
    }
}

/// 嵌入式形式剖面（F = embedded/no_std）：预算零分配（稳态每消息），
/// 白名单 InlineCarrier ＋ [`crate::movers::ring::BoundedRing`] 存储原语
/// （构造期一次预留，稳态零分配；跨线程变体待 D4 关键节选型）。
pub struct EmbeddedProfile;
impl Profile for EmbeddedProfile {
    fn cost_budget() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass {
            delivery: DeliveryKind::NotApplicable, // 单线程直通存储：无投递态义务
            resource: CarrierCost::ZeroAllocInline,
            ..ObligationClass::default()
        }
    }
}

/// 游戏/交互体剖面（F = game；命题 7.1 第四行）。预算 External（帧内分配是常态，
/// 不宣称零成本）；帧预算 deadline 与状态一致性下限在异步/时间层机械化前保持
/// 模态④声明（诚实边界见 runtime.md 剖面节）。
pub struct GameProfile;
impl Profile for GameProfile {
    fn cost_budget() -> CarrierCost {
        CarrierCost::External
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass::default()
    }
    fn saturation_floor() -> SaturationPolicy {
        // 帧内丢弃为常态，drop-with-receipt（Fail）档——低于 Block，但不静默丢值。
        SaturationPolicy::Fail
    }
}

/// 按剖面装配（模态③）：载体成本 ≤ 剖面预算 且 载体义务不弱于剖面义务下限
/// 且 载体饱和策略不弱于剖面饱和下限（A1 第三门），越界 = 装配失败；返回
/// [`drive_link`] 函数指针（热路径零税）。同一 `A`/`B` 拓扑换剖面 = 换预算门 +
/// 换义务下限 + 换饱和下限，不改拓扑（T6：义务随剖面变化，语义一致）。
///
/// **开放入口**：不校验注册（C3）——适用于开放剖面（Tool/Embedded，`GATED=false`）。
/// 注册门剖面（Kernel/Service，`GATED=true`）必须使用 [`assemble_profile_gated`]
/// （编译期要求 `C: Registered`）；对门剖面误用本入口 = 声明者责任（诚实边界：
/// 两入口纪律，lint 原型见 C11 待办）。
pub fn assemble_profile<P, A, B, C>() -> Result<Driver<A, B>, ContractError>
where
    P: Profile,
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    crate::checks::contract::validate_cost::<A, B, C>(P::cost_budget())?;
    // C10 step 2：义务下限（模态③）——从占位转为可强制的装配门。
    crate::checks::contract::validate_obligation_min(C::obligation(), P::obligation_min())?;
    // A1：饱和下限（模态③）——载体的饱和策略不得弱于剖面饱和下限。
    crate::checks::contract::validate_saturation::<A, B, C>(P::saturation_floor())?;
    Ok(drive_link::<A, B, C>)
}

/// **注册门装配**（C3；模态①）：与 [`assemble_profile`] 同语义，另要求
/// `C: Registered`（官方载体）——未注册（第三方）载体在注册门剖面
/// （Kernel/Service）编译失败：白名单从文档约定升为编译期事实。
pub fn assemble_profile_gated<P, A, B, C>() -> Result<Driver<A, B>, ContractError>
where
    P: Profile,
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B> + crate::movers::carrier::Registered,
{
    assemble_profile::<P, A, B, C>()
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::movers::carrier::{InlineCarrier, QueueCarrier, SaturationPolicy};
    use crate::checks::contract::validate_saturation;
    use crate::checks::obligation::ObligationClass;

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

    #[test]
    fn kernel_profile_rejects_per_message_carriers() {
        // 内核剖面：零分配预算 → Queue（PerMessageAlloc）装配失败（成本违约）。
        let link = assemble_profile::<KernelProfile, Inc, Double, InlineCarrier>();
        assert!(link.is_ok());
        let rejected =
            assemble_profile::<KernelProfile, Inc, Double, QueueCarrier>();
        assert!(matches!(rejected, Err(ContractError::CostExceeded { .. })));
    }

    #[test]
    fn service_profile_accepts_per_message_carriers_but_not_unmechanized_delivery() {
        // 服务剖面：预算 PerMessageAlloc + 投递态机械化下限（C10 step 2）→ Queue 通过；
        // Inline（同步直通、投递态 N/A）不满足机械化下限 → 义务违约拒绝
        // （服务投递接缝必须机械化 Full/Closed，直通属内核式义务）。
        assert!(assemble_profile::<ServiceProfile, Inc, Double, QueueCarrier>().is_ok());
        assert!(matches!(
            assemble_profile::<ServiceProfile, Inc, Double, InlineCarrier>(),
            Err(ContractError::ObligationUnderMet { axis: "delivery", .. })
        ));
    }

    #[test]
    fn obligation_min_splits_profiles() {
        // C10 step 2：义务下限按剖面分化——Service 投递态机械化下限、
        // Kernel/Embedded 零分配资源下限、Tool 外松无下限。
        assert_eq!(
            ServiceProfile::obligation_min().delivery,
            DeliveryKind::MechanizedFullClosed,
            "服务剖面要求投递态机械化"
        );
        assert_eq!(
            KernelProfile::obligation_min().resource,
            CarrierCost::ZeroAllocInline
        );
        assert_eq!(
            EmbeddedProfile::obligation_min().resource,
            CarrierCost::ZeroAllocInline
        );
        assert_eq!(
            ToolProfile::obligation_min().resource,
            CarrierCost::External,
            "工具剖面无资源义务下限"
        );
        // Tool 外松：任何载体义务都满足下限（Inline 与 Queue 均通过）。
        assert!(assemble_profile::<ToolProfile, Inc, Double, InlineCarrier>().is_ok());
        assert!(assemble_profile::<ToolProfile, Inc, Double, QueueCarrier>().is_ok());
    }

    // 探针用"会丢"载体：成本量级通过 Service 预算，投递态机械化，饱和为 DropNewest。
    // 专用于证明饱和门槛（A1）独立于成本/义务门——否则会先被 cost/obligation 拒绝。
    struct DropOnSaturation;
    impl Carrier<Inc, Double> for DropOnSaturation {
        fn cost() -> CarrierCost {
            CarrierCost::PerMessageAlloc
        }
        fn obligation() -> ObligationClass {
            ObligationClass {
                delivery: DeliveryKind::MechanizedFullClosed,
                resource: CarrierCost::PerMessageAlloc,
                ..ObligationClass::default()
            }
        }
        fn saturation() -> SaturationPolicy {
            SaturationPolicy::DropNewest
        }
        fn flow(sa: &mut (), sb: &mut (), x: i32) -> i32 {
            Double::step(sb, Inc::step(sa, x))
        }
    }

    #[test]
    fn service_profile_rejects_dropping_carrier() {
        // A1 第三门：Service 饱和下限 Block——会丢（DropNewest）的载体在装配点被拒
        // （不走 delivery 门：DropOnSaturation 投递态已机械化、成本也过 Service 预算）。
        assert_eq!(
            assemble_profile::<ServiceProfile, Inc, Double, DropOnSaturation>(),
            Err(ContractError::SaturationUnderMet {
                declared: SaturationPolicy::DropNewest,
                floor: SaturationPolicy::Block,
            })
        );
        // 同一载体在 Tool（无饱和下限）下装配通过——门是剖面属性的，非载体固有。
        assert!(assemble_profile::<ToolProfile, Inc, Double, DropOnSaturation>().is_ok());
        // 同一载体在 Game（Fail 档）下仍被拒（DropNewest < Fail，异策略不可比）。
        assert!(matches!(
            assemble_profile::<GameProfile, Inc, Double, DropOnSaturation>(),
            Err(ContractError::SaturationUnderMet { .. })
        ));
    }

    #[test]
    fn game_profile_accepts_fail_tier() {
        // Game 饱和下限 Fail：Fail 档载体满足自身；Block 载体（更强）也满足。
        assert!(validate_saturation::<Inc, Double, QueueCarrier>(SaturationPolicy::Fail)
            .is_ok()); // Block ≥ Fail
    }

    #[test]
    fn gated_assembly_requires_registered_carrier() {
        // C3：注册门剖面经 gated 入口装配官方载体（Registered）成功；
        // 未注册载体在该剖面编译失败（sealed 保证不可外部注册——负测试不可写，
        // 即模态① 的形态：违反在编译期被类型系统拒绝）。
        assert!(assemble_profile_gated::<KernelProfile, Inc, Double, InlineCarrier>().is_ok());
        assert!(assemble_profile_gated::<ServiceProfile, Inc, Double, QueueCarrier>().is_ok());
        // Bounded 族注册覆盖任意 CAP。
        assert!(assemble_profile_gated::<ServiceProfile, Inc, Double, crate::movers::carrier::BoundedCarrier<4>>().is_ok());
    }

    #[test]
    fn tool_profile_accepts_everything_and_drives() {
        let link = assemble_profile::<ToolProfile, Inc, Double, InlineCarrier>()
            .expect("tool 剖面预算最宽");
        let (mut sa, mut sb) = ((), ());
        assert_eq!(link(&mut sa, &mut sb, 5), 12);
    }
}