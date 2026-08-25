//! 剖面目录（六元组标准化 C 构件；meta-foundations 命题 7.1 的 F↦C(F) 代码形态）。
//!
//! 每个剖面 = {允许载体集（文档化白名单）、义务类下限、成本预算}。剖面是**类型级令牌**
//! （模态①）：同一拓扑在不同剖面下装配，即 T6"同一图层换物理"（内核剖面拒绝
//! 每消息分配载体，工具剖面默认内联）。
//!
//! **诚实声明（A5）**：受开放 `Carrier` impl 约束，载体白名单不可在类型层强制
//! （任何实现者都可自行 `impl Carrier`）；类型层可强制的是**成本预算与义务下限**——
//! 装配点按模态③ 校验（[`assemble_profile`]）。白名单因此是规范文档（S/L 构件），
//! 预算是可执行投影（T 构件）。
//!
//! **待办（义务账本第二阶段）**：`obligation_min()` 目前三剖面同返保守默认，下限轴仅立
//! API 未分化。预期演化：`ServiceProfile` 收紧 `delivery = 机械化 Full/Closed` 下限、
//! `KernelProfile` 收紧 zero-alloc 资源下限、`ToolProfile` 保持外松；届时随义务账本
//! 扩行为各剖面配下限校验（模态③）。此为前提言，非承诺。
//!
//! 剖面（meta 命题 7.1 的实例表）：
//! - **[`KernelProfile`]**：内核形式。白名单 InlineCarrier、BoundedCarrier（有门）；
//!   预算 ZeroAllocInline（零分配义务）；无超时义务（Timeout/Cancelled 保持④ 声明）。
//! - **[`ServiceProfile`]**：服务形式。白名单 BoundedCarrier、BoundedMailbox、spawned_flow；
//!   预算 PerMessageAlloc；Full/Closed 机械化（delivery.rs）。
//! - **[`ToolProfile`]**：工具形式。默认 InlineCarrier；预算 External（fail-closed 默认）。

use axiom::cell_core::PortCell;

use crate::carrier::{Carrier, CarrierCost};
use crate::contract::ContractError;
use crate::flow::{Driver, drive_link};
use crate::obligation::ObligationClass;

/// 剖面令牌（模态①）：声明"本系统按哪个软件形式的分域承诺装配"。
pub trait Profile {
    /// 成本预算（经验-D：界 + 装配校验）。
    fn cost_budget() -> CarrierCost;
    /// 义务类下限（逻辑-D：装配点的义务不弱于该下限）。
    fn obligation_min() -> ObligationClass;
}

/// 内核形式剖面（F = kernel）。
pub struct KernelProfile;
impl Profile for KernelProfile {
    fn cost_budget() -> CarrierCost {
        CarrierCost::ZeroAllocInline
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass::default()
    }
}

/// 服务形式剖面（F = service/server）。
pub struct ServiceProfile;
impl Profile for ServiceProfile {
    fn cost_budget() -> CarrierCost {
        CarrierCost::PerMessageAlloc
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass::default()
    }
}

/// 工具形式剖面（F = tool/CLI）。
pub struct ToolProfile;
impl Profile for ToolProfile {
    fn cost_budget() -> CarrierCost {
        CarrierCost::External
    }
    fn obligation_min() -> ObligationClass {
        ObligationClass::default()
    }
}

/// 按剖面装配（模态③）：载体成本 ≤ 剖面预算，越界 = 装配失败；返回 [`drive_link`]
/// 函数指针（热路径零税）。同一 `A`/`B` 拓扑换剖面 = 换预算门，不改拓扑（T6）。
pub fn assemble_profile<P, A, B, C>() -> Result<Driver<A, B>, ContractError>
where
    P: Profile,
    A: PortCell,
    B: PortCell<In = A::Out>,
    C: Carrier<A, B>,
{
    crate::contract::validate_cost::<A, B, C>(P::cost_budget())?;
    Ok(drive_link::<A, B, C>)
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::carrier::{InlineCarrier, QueueCarrier};

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
        // 内核剖面：零分配预算 → Queue（PerMessageAlloc）装配失败。
        let link = assemble_profile::<KernelProfile, Inc, Double, InlineCarrier>();
        assert!(link.is_ok());
        let rejected =
            assemble_profile::<KernelProfile, Inc, Double, QueueCarrier>();
        assert!(matches!(rejected, Err(ContractError::CostExceeded { .. })));
    }

    #[test]
    fn service_profile_accepts_per_message_carriers() {
        // 服务剖面：PerMessageAlloc 预算 → Queue 通过；Inline 亦通过（预算宽松）。
        assert!(assemble_profile::<ServiceProfile, Inc, Double, QueueCarrier>().is_ok());
        assert!(assemble_profile::<ServiceProfile, Inc, Double, InlineCarrier>().is_ok());
    }

    #[test]
    fn tool_profile_accepts_everything_and_drives() {
        let link = assemble_profile::<ToolProfile, Inc, Double, InlineCarrier>()
            .expect("tool 剖面预算最宽");
        let (mut sa, mut sb) = ((), ());
        assert_eq!(link(&mut sa, &mut sb, 5), 12);
    }
}