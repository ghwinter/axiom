//! 域层（B2）：纯 `PortCell` ——库作者暴露的角色。零物理依赖、
//! 零 std 需求；只表达"因果变换 + 状态"。

use axiom::cell_core::PortCell;

/// 限流域单元：State = (额度, 已用)；超额度 = 失败为值（`Out = Result`）。
pub struct RateLimit;
impl PortCell for RateLimit {
    type In = i32;
    type Out = Result<i32, &'static str>;
    type State = RateState;
    #[inline(always)]
    fn step(s: &mut RateState, x: i32) -> Result<i32, &'static str> {
        if s.used >= s.quota {
            Err("quota exhausted")
        } else {
            s.used += 1;
            Ok(x)
        }
    }
}

/// 限流状态（域层显式类型；默认配额 5，供部署层直接 `default()` 装配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateState {
    /// 额度（0 = 立即拒绝——退化态由装配校验拒绝）。
    pub quota: u32,
    /// 已用。
    pub used: u32,
}

impl Default for RateState {
    fn default() -> Self {
        RateState { quota: 5, used: 0 }
    }
}

/// Result 域适配单元：`Ok` 经变换、`Err` 直通（"失败为值"在域内的显式适配）。
pub struct ScaleOk;
impl PortCell for ScaleOk {
    type In = Result<i32, &'static str>;
    type Out = Result<i32, &'static str>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), r: Result<i32, &'static str>) -> Result<i32, &'static str> {
        r.map(|x| x.wrapping_mul(2))
    }
}

/// 观测域单元：把 Result 归一为值（降级政策：Err → 0）。
pub struct Degrade;
impl PortCell for Degrade {
    type In = Result<i32, &'static str>;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), r: Result<i32, &'static str>) -> i32 {
        r.unwrap_or(0)
    }
}

/// 末端直通单元（部署层汇点：同型直通）。
pub struct AsIs;
impl PortCell for AsIs {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x
    }
}