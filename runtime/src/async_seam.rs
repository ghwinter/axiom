//! 异步接缝最小原型（D2 executor 契约；std 门控；零依赖）。
//!
//! 忠实于 async-seam.md（D2 已拍板）：**`step` 永不等**——等待点只发生在边界。
//! 本模块实现三类等待点中的两类于同步域探测：
//! - **输入到达**：未决输入 `None` → `Pending`；就绪 → 同步执行 `step` → `Ready`；
//! - **期限**：[`Poller::poll_until`] 带期限轮询，到期 → [`PollResult::TimedOut`]；
//! - **背压（第二层，A1/C7）**：[`SeamPoller`] ——把 `A::Out` 经**真有限通道**
//!   （`mpsc::SyncSender`）投递，饱和时依 [`SaturationPolicy`]（Block=滞留值并期限
//!   轮询 / Fail=值随判回传）；与 `bounded_pump` 生产端同构，但 poll 化、期限化。
//!
//! **诚实边界（A5）**：投递四态中的 `Timeout` 在 delivery.rs 保持模态④ 声明
//! （需要真定时器/请求域机制）；本层的 `TimedOut` 是**同步轮询域内的期限判定**，
//! 不冒充 `Delivery::Timeout`——它把"期限缺位 = 永不 TimedOut"的退化态（第五轴，
//! boundary-ontology 命题 2.7）显式化：期限必须存在才是良态轮询。
//!
//! **第三层（契约落定）**：[`Executor`] 契约 + 线程参考实现 [`ThreadExec`]——
//! 真 executor/waker 生态经实现本契约接入（如 tokio 适配器）；`SeamPoller` 的
//! 固有递延（`thread::sleep`）即 `ThreadExec` 的语义；EX 泛型化与 `SlotDrive`
//! 换装协同（跨异步边界在途语义，C5 协议）随适配器落地。

use axiom::cell_core::PortCell;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

use crate::carrier::SaturationPolicy;

/// **执行器契约**（C7 第三层；D2 executor 约定）：等待点的事件循环步。
///
/// axiom 不提供执行器（生态领域——广为认知锚点如 tokio）；本契约是异步接缝
/// 对外部 executor 的最小面：宿主在轮询间隙调用 [`park`](Executor::park)
/// 递延（供事件循环/线程/yield 语义），期限由调用方持有。
pub trait Executor {
    /// 递延一步（等待输入到达/腾位/期限的事件循环语义）。
    fn park(&mut self, dur: Duration);
}

/// 线程参考执行器：`park` = `thread::sleep`（同步轮询域的递延；本模块现有
/// 轮询的行为即此语义）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadExec;

impl Executor for ThreadExec {
    fn park(&mut self, dur: Duration) {
        std::thread::sleep(dur);
    }
}

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

    /// 带期限轮询，等待点交给调用方提供的 [`Executor`]（EX 泛型化；instance-layer-design §4）。
    ///
    /// 语义与 [`poll_until`](Poller::poll_until) 相同，但 `Pending` 间隙的
    /// 递延经 `ex.park(tick)` 交给执行器（替代硬编码 `thread::sleep`）。
    /// `ThreadExec`（sleep）与第三方 executor（如 `axiom-instances` 的 tokio
    /// 桥接）经此接管等待点。**additive**：不改动既有入口，现有调用语义不变。
    pub fn poll_with<EX: Executor>(
        &mut self,
        ex: &mut EX,
        deadline: Instant,
        tick: Duration,
    ) -> PollResult<A::Out> {
        loop {
            match self.poll() {
                Poll::Ready(o) => return PollResult::Ready(o),
                Poll::Pending => {
                    if Instant::now() >= deadline {
                        return PollResult::TimedOut;
                    }
                    ex.park(tick); // 等待点交给 executor（替代硬编码 thread::sleep）
                }
            }
        }
    }
}

/// 单轮投递裁决（背压接缝，C7 第二层）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeamRoll<X> {
    /// 无待决输入（等待事件源/调用方喂入）。
    Idle,
    /// 已投递到有界通道（消费侧可收）。
    Accepted,
    /// 饱和且策略 Fail/断连：值随判定回传（不消失），由调用方裁决。
    Full(X),
    /// 饱和且策略 Block：值滞留（值保留），需等待消费侧腾位（期限内）。
    Blocked,
}

/// 带背压等待点的接缝轮询器（C7 第二层；A1 饱和策略的机械）。
///
/// `A::Out` 经有限通道（`mpsc::SyncSender`）投递——等待点第三类（背压）的
/// 同步域形态：与 [`bounded_pump`](crate::flow::bounded_pump) 生产端同构，
/// 但 **poll 化 + 期限化**：
/// - 饱和且策略 [`Fail`](SaturationPolicy::Fail)/断连 → 值随 `Full(v)` 回传；
/// - 饱和且策略 [`Block`](SaturationPolicy::Block) → 值滞留（值保留），
///   [`roll_until`](SeamPoller::roll_until) 带期限轮询等待腾位；
/// - `step` 永不等（D2）：等待只发生在边界投递。
pub struct SeamPoller<A>
where
    A: PortCell,
    A::Out: Send,
{
    state: A::State,
    pending: Option<A::In>,
    held: Option<A::Out>, // Block 滞留值（Full(v) 值保留）
    tx: SyncSender<A::Out>,
    policy: SaturationPolicy,
}

impl<A> SeamPoller<A>
where
    A: PortCell,
    A::Out: Send,
{
    /// 新建：同步状态 + 待决输入 + 有界发送端 + 饱和策略。
    pub fn new(
        state: A::State,
        pending: Option<A::In>,
        tx: SyncSender<A::Out>,
        policy: SaturationPolicy,
    ) -> Self {
        SeamPoller {
            state,
            pending,
            held: None,
            tx,
            policy,
        }
    }

    /// 单轮：滞留值（Block 未投递）优先重投；其后消费待决输入 → `step` → 投递。
    pub fn roll(&mut self) -> SeamRoll<A::Out> {
        if let Some(v) = self.held.take() {
            return self.try_deliver(v);
        }
        match self.pending.take() {
            None => SeamRoll::Idle,
            Some(input) => {
                let out = A::step(&mut self.state, input);
                self.try_deliver(out)
            }
        }
    }

    fn try_deliver(&mut self, v: A::Out) -> SeamRoll<A::Out> {
        match self.tx.try_send(v) {
            Ok(()) => SeamRoll::Accepted,
            Err(TrySendError::Full(v)) => match self.policy {
                SaturationPolicy::Fail | SaturationPolicy::DropNewest => SeamRoll::Full(v),
                SaturationPolicy::Block => {
                    self.held = Some(v); // 值保留，待腾位重投
                    SeamRoll::Blocked
                }
                // DropOldest/NotApplicable 在有界直投语义下按 Fail 处理值回传
                // （策略的实现细节归载体层；此处保守：值不静默消失）。
                SaturationPolicy::DropOldest | SaturationPolicy::NotApplicable => {
                    SeamRoll::Full(v)
                }
            },
            Err(TrySendError::Disconnected(v)) => SeamRoll::Full(v), // 断连，值随判定回传
        }
    }

    /// 带期限轮询（Block 语义）：`Idle`/`Blocked` 在期限内反复尝试；
    /// 到期仍未就绪/未腾位 → `TimedOut`。消费侧腾位后滞留值自动重投。
    pub fn roll_until(
        &mut self,
        deadline: Instant,
        tick: Duration,
    ) -> PollResult<SeamRoll<A::Out>> {
        loop {
            match self.roll() {
                SeamRoll::Idle | SeamRoll::Blocked => {
                    if Instant::now() >= deadline {
                        return PollResult::TimedOut;
                    }
                    std::thread::sleep(tick);
                }
                other => return PollResult::Ready(other),
            }
        }
    }

    /// 带期限轮询，等待点交给调用方提供的 [`Executor`]（EX 泛型化；instance-layer-design §4）。
    ///
    /// 同 [`roll_until`](SeamPoller::roll_until)，但 `Idle`/`Blocked` 间隙的
    /// 递延经 `ex.park(tick)` 交给执行器（替代硬编码 `thread::sleep`）——
    /// 背压等待点同样可被第三方 executor（tokio 桥接）接管。**additive**。
    pub fn roll_with<EX: Executor>(
        &mut self,
        ex: &mut EX,
        deadline: Instant,
        tick: Duration,
    ) -> PollResult<SeamRoll<A::Out>> {
        loop {
            match self.roll() {
                SeamRoll::Idle | SeamRoll::Blocked => {
                    if Instant::now() >= deadline {
                        return PollResult::TimedOut;
                    }
                    ex.park(tick); // 等待点交给 executor（替代硬编码 thread::sleep）
                }
                other => return PollResult::Ready(other),
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
    fn thread_exec_parks_for_requested_duration() {
        // 契约参考实现：park 递延语义（线程 sleep）。
        let mut ex = ThreadExec;
        let t0 = Instant::now();
        ex.park(Duration::from_millis(10));
        assert!(t0.elapsed() >= Duration::from_millis(10));
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

    // ── C7 第二层：背压等待点（SeamPoller）─────────────────────────────

    #[test]
    fn seam_roll_accepts_when_channel_has_room() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<i32>(2);
        let mut p = SeamPoller::<Inc>::new((), Some(5), tx, SaturationPolicy::Block);
        assert_eq!(p.roll(), SeamRoll::Accepted, "容量 2 空 → 投递即收");
        assert_eq!(rx.recv(), Ok(6), "消费侧收到 A::step 输出");
    }

    #[test]
    fn seam_roll_fail_policy_returns_value() {
        // 饱和 + Fail 策略：值随 Full(v) 回传（不消失、不静默丢）。
        let (tx, _rx) = std::sync::mpsc::sync_channel::<i32>(1);
        tx.try_send(99).expect("占满容量 1");
        let mut p = SeamPoller::<Inc>::new((), Some(5), tx, SaturationPolicy::Fail);
        match p.roll() {
            SeamRoll::Full(v) => assert_eq!(v, 6, "值随判定回传"),
            other => panic!("expected Full(6), got {other:?}"),
        }
    }

    #[test]
    fn seam_roll_block_retains_value_until_space() {
        // 饱和 + Block 策略：值滞留（held），消费侧腾位后自动重投——值不丢。
        let (tx, rx) = std::sync::mpsc::sync_channel::<i32>(1);
        tx.try_send(99).expect("占满容量 1");
        let mut p = SeamPoller::<Inc>::new((), Some(5), tx, SaturationPolicy::Block);
        assert_eq!(p.roll(), SeamRoll::Blocked, "满 → 滞留（值保留）");
        assert_eq!(p.roll(), SeamRoll::Blocked, "滞留值继续等待，不丢不重算");
        rx.recv().expect("消费侧取走占位"); // 腾位
        assert_eq!(p.roll(), SeamRoll::Accepted, "腾位后滞留值投递成功");
        assert_eq!(rx.recv(), Ok(6), "消费侧收到滞留值（A::step 只跑一次）");
    }

    #[test]
    fn seam_roll_until_waits_for_space_within_deadline() {
        // Block + 期限：占用通道 → roll_until 等待 → 消费侧腾位 → Ready(Accepted)。
        let (tx, rx) = std::sync::mpsc::sync_channel::<i32>(1);
        tx.try_send(99).expect("占满容量 1");
        let mut p = SeamPoller::<Inc>::new((), Some(5), tx, SaturationPolicy::Block);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            let _ = rx.recv(); // 腾位
            // 保持接收端存活（若 drop rx，通道断连 → 后续 try_send 得
            // Disconnected → Full(v)——断连值回传语义，非本测试场景）。
            std::thread::sleep(Duration::from_millis(400));
        });
        match p.roll_until(Instant::now() + Duration::from_millis(400), Duration::from_millis(5)) {
            PollResult::Ready(SeamRoll::Accepted) => {}
            other => panic!("expected Ready(Accepted), got {other:?}"),
        }
    }

    // ── EX 泛型化（instance-layer-design §4）：等待点经 executor.park ──

    /// 计数执行器：park 只计数、绝不真正休眠——证明等待点已接管到 executor，
    /// 而非硬编码 `thread::sleep`（否则本测试会真实阻塞 1ms × 轮数）。
    struct CountingExec {
        parks: u32,
    }
    impl Executor for CountingExec {
        fn park(&mut self, _dur: Duration) {
            self.parks += 1;
        }
    }

    #[test]
    fn poll_with_parks_via_executor_not_hardcoded_sleep() {
        // 输入不预置：poll_with 在期限内反复 Pending，等待点经 executor.park
        // 驱动（计数 executor 永不真实休眠）→ 到期限 TimedOut。
        let mut ex = CountingExec { parks: 0 };
        let mut p = Poller::<Inc>::new((), None); // 输入永不就绪
        let result = p.poll_with(
            &mut ex,
            Instant::now() + Duration::from_millis(30),
            Duration::from_millis(1),
        );
        assert_eq!(result, PollResult::TimedOut, "输入不抵达 → 期限超时");
        assert!(ex.parks >= 1, "等待点必须经 executor.park（而非硬编码休眠）");
    }

    #[test]
    fn poll_with_ready_when_input_available_before_any_park() {
        // 输入即就绪：首拍 Ready，不需等待点 → 不 park（现值语义不变）。
        let mut ex = CountingExec { parks: 0 };
        let mut p = Poller::<Inc>::new((), Some(43));
        let result = p.poll_with(
            &mut ex,
            Instant::now() + Duration::from_millis(200),
            Duration::from_millis(1),
        );
        assert_eq!(result, PollResult::Ready(44), "43 → Inc(+1) = 44");
        assert_eq!(ex.parks, 0, "有输入即 Ready，进不了 Pending 等待点");
    }

    #[test]
    fn poll_with_times_out_before_park_when_deadline_passed() {
        // 期限已过：第一拍 Pending → 立即 TimedOut，不调用 park（偶不需要等待点）。
        let mut ex = CountingExec { parks: 0 };
        let mut p = Poller::<Inc>::new((), None); // 输入永不就绪
        let result = p.poll_with(&mut ex, Instant::now() - Duration::from_millis(1), Duration::from_millis(1));
        assert_eq!(result, PollResult::TimedOut, "期限先于等待点 → 立即超时");
        assert_eq!(ex.parks, 0, "未到等待点 → 不 park");
    }

    #[test]
    fn roll_with_parks_via_executor_and_retains_value_on_block() {
        // 背压 Block 语义复用，但等待点经 executor.park（计数 executor 不真实休眠）。
        let (tx, rx) = std::sync::mpsc::sync_channel::<i32>(1);
        tx.try_send(99).expect("占满容量 1");
        let mut ex = CountingExec { parks: 0 };
        let mut p = SeamPoller::<Inc>::new((), Some(5), tx, SaturationPolicy::Block);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            let _ = rx.recv(); // 腾位
            std::thread::sleep(Duration::from_millis(60)); // 保持 rx 存活防断连
        });
        match p.roll_with(&mut ex, Instant::now() + Duration::from_millis(300), Duration::from_millis(5)) {
            PollResult::Ready(SeamRoll::Accepted) => {}
            other => panic!("expected Ready(Accepted), got {other:?}"),
        }
        assert!(ex.parks >= 1, "背压等待点亦须经 executor.park");
    }
}