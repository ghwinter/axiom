//! 异步接缝最小原型（D2 executor 契约第一层；std 门控；零依赖）。
//!
//! 忠实于 async-seam.md（D2 已拍板）：**`step` 永不等**——等待点只发生在边界。
//! 本层实现两类等待点的同步域探测：
//! - **输入到达**：未决输入 `None` → `Pending`；就绪 → 同步执行 `step` → `Ready`；
//! - **期限**：[`Poller::poll_until`] 带期限轮询，到期 → [`PollResult::TimedOut`]。
//!
//! **诚实边界（A5）**：投递四态中的 `Timeout` 在 delivery.rs 保持模态④ 声明
//! （需要真定时器/请求域机制）；本层的 `TimedOut` 是**同步轮询域内的期限判定**，
//! 不冒充 `Delivery::Timeout`——它把"期限缺位 = 永不 TimedOut"的退化态（第五轴，
//! boundary-ontology 命题 2.7）显式化：期限必须存在才是良态轮询。
//!
//! **第二层（待办，async-seam.md 开放问题）**：背压载体注入（三类等待点之第三：
//! 有界载体 `recv`）、`EX` 泛型（真 executor / waker 约定）、与 `SlotDrive` 换装的
//! 协同。本层仅交付"同步图可轮询 + 期限探测"的原子形态。

use axiom::cell_core::PortCell;
use std::time::{Duration, Instant};

/// 轮询裁决：未就绪（挂起）或已产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll<O> {
    /// 输入未就绪，等待（调用方再次 poll 或等事件/期限）。
    Pending,
    /// 输入就绪，`step` 已同步执行完毕。
    Ready(O),
}

/// 带期限的轮询裁决：在 `Pending`/`Ready` 之上增加**期限耗尽**判定。
///
/// 同步轮询域内的期限探测（见模块文档：不冒充 `Delivery::Timeout` 的 ④ 语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollResult<O> {
    /// 期限未到且输入未就绪。
    Pending,
    /// 输入就绪并已同步完成。
    Ready(O),
    /// 期限耗尽，输入仍未就绪（轮询域超时判定）。
    TimedOut,
}

/// 可轮询单元：一个同步 cell `A` 的包装（D2："`step` 永不等"）。
pub struct Poller<A: PortCell> {
    state: A::State,
    pending: Option<A::In>,
}

impl<A> Poller<A>
where
    A: PortCell,
{
    /// 新建：持有同步状态与（可空的）待决输入。
    pub fn new(state: A::State, pending: Option<A::In>) -> Self {
        Poller { state, pending }
    }

    /// 单次轮询：无输入 → `Pending`；有输入 → 同步 `step` → `Ready`。
    pub fn poll(&mut self) -> Poll<A::Out> {
        match self.pending.take() {
            Some(input) => Poll::Ready(A::step(&mut self.state, input)),
            None => Poll::Pending,
        }
    }

    /// 带期限轮询：在 `deadline` 前反复轮询；输入就绪 → `Ready`；
    /// 到期仍未就绪 → `TimedOut`（轮询茎让步 `step` 期间休眠 `tick`）。
    ///
    /// 退化态拒绝（命题 2.7）：`deadline` 必须是良态期限——调用方保证其语义
    /// （本原型不伪造定时器；期限由调用方持有）。
    pub fn poll_until(&mut self, deadline: Instant, tick: Duration) -> PollResult<A::Out> {
        loop {
            match self.poll() {
                Poll::Ready(o) => return PollResult::Ready(o),
                Poll::Pending => {
                    if Instant::now() >= deadline {
                        return PollResult::TimedOut;
                    }
                    std::thread::sleep(tick);
                }
            }
        }
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

    #[test]
    fn poll_ready_when_input_available() {
        let mut p = Poller::<Inc>::new((), Some(5));
        assert_eq!(p.poll(), Poll::Ready(6), "输入就绪即同步完成");
        assert_eq!(p.poll(), Poll::Pending, "输入已消费");
    }

    #[test]
    fn poll_pending_without_input() {
        let mut p = Poller::<Inc>::new((), None);
        assert_eq!(p.poll(), Poll::Pending, "无输入 → 挂起");
    }

    #[test]
    fn poll_until_input_arrives_within_deadline() {
        // 伪事件源：先 Pending（期限内未就绪），随后输入到位——期限内 Ready。
        let mut p = Poller::<Inc>::new((), None);
        let deadline = Instant::now() + Duration::from_millis(400);
        assert_eq!(p.poll(), Poll::Pending, "初始无输入");
        p.pending = Some(41); // 模拟事件源在期限内送达输入
        match p.poll_until(deadline, Duration::from_millis(5)) {
            PollResult::Ready(v) => assert_eq!(v, 42, "输入就绪后同步完成"),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn poll_until_deadline_elapses_yields_timedout() {
        // 期限耗尽：输入始终未就绪 → TimedOut（轮询域期限判定）。
        let mut p = Poller::<Inc>::new((), None);
        let result = p.poll_until(
            Instant::now() + Duration::from_millis(20),
            Duration::from_millis(5),
        );
        assert_eq!(result, PollResult::TimedOut, "期限耗尽 → 轮询域超时判定");
    }
}