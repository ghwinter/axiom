//! 激活模型目录·第一期——call/response 关联调度（时间作值）。
//!
//! 痛点（外部审查定点）：手写异步里 `let resp = http_post(url, body).await?` 一行，
//! 编译器替你拆状态机 + 建关联表；cell 模型里是"一个发请求、一个收响应"两个 step，
//! **关联表（哪个响应对应哪个请求）要自己在 State 里管**——这正是"要手写多久 poll
//! 才有人封装好 epoll"这句话里的 poll。本模块把这一块做成类型化的、确定性可测的组件。
//!
//! ## 形态裁定：单槽同步 vs 乱序异步
//!
//! 仓库里的 [`ComposeLine`](composite::ComposeLine) 是**单槽同步**服务：`step(req)`
//! 立即产出 `resp`，同一时刻至多一个在途调用，关联退化为平凡——它**不需要**关联表，
//! 每会话一份 `Svc::State` + 一次 `poll_until(deadline)` 即可。这类形态配套的是
//! 会话模板（每会话 = 状态持有 + 期限轮询），demo 已用 [`Poller`](observe::ObservedPoller)
//! 表达。
//!
//! 本模块服务的是**另一种**形态：**乱序/可变延迟**的异步服务（网络对端、跨机管道——
//! 请求批量发出、响应按各自延时完成且次序不定）。关联表在此不可退化为平凡，必须显式。
//! 它属于"不合格但兼容的邻居"（socket 之于 file）：**不成 `PortCell`**——`PortCell`
//! 单 In/单 Out 语义承载不了"一次在途调用 + 独立期限 + 关联"这段协议，把它折进元组
//! 合法但别扭。故按讨论定论，关联调度器作为**邻居组件**存在：持有服务状态无关的
//! 关联结构，说 `PortCell` 这门货币语言（内部仍以原子值交换），但不假装它是 cell。
//!
//! ## 时间作值（诚实边界）
//!
//! 本调度器**不读环境时钟**：所有期限判定都经显式 `now: Instant` **输入**注入。
//! 这与核心 `step` 纯函数的裁定一致（cell 内无时间，时间经泵喂入）。由此：
//! - 测试**完全确定**（注入假时钟，不依赖真实计时器粒度）；
//! - 真实驱动端以 `Instant::now()`（或 tokio 时间驱动）调用同一入口，语义不变。
//!
//! ## 真实接入的诚实边界（不伪造）
//!
//! 仓库现有服务（`ComposeLine`）是单槽同步，**不消费**关联表；若把本模块硬焊进
//! demo 便是有名无实的接线。故真实接入登记为**二期**：随第一个**携带相关 id 的
//! 乱序异步服务**（第一个真实网络系统）落地而接线。本模块当前是**纯可用、已测**的
//! 目录件，其一期工作是提供可复用的关联与超时语义；接线点（驱动把物理响应路由到
//! `CallId`）是服务契约，不在本模块范围。这正是"组件先于接线、真实需求才接线"的
//! 纪律。

use std::collections::VecDeque;
use std::time::Instant;

/// 调用标识：一次在途请求的唯一句柄（响应按其归位、期限按其判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallId(u64);

impl CallId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// 单调用判定：`complete` / `settle` 产生，由调用方消费。
///
/// `TimedOut` 不携带负载——调用方经 `CallId` 仍持有一份请求引用，可据其重试。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallResult<R> {
    /// 响应已关联归位。
    Ok(R),
    /// 期限耗尽，响应未到（请求可能需要重试）。
    TimedOut,
}

/// 关联插槽的三种状态。
///
/// - `Await`：在途，期限内可被 [`complete`](CallDispatch::complete) 归位；
/// - `Timed`：已在 [`settle`](CallDispatch::settle) 判为超时并出账——**吸收迟到完成**
///   （该调用的响应过期后才到：调用方已收到 `TimedOut`，迟到响应不双出账、count 丢弃）；
/// - `None`（`Option::None`）：空闲，可分配。
#[derive(Clone)]
enum Slot {
    Await { deadline: Instant },
    Timed,
}

/// **call/response 关联调度器**——把"在途调用关联 + 超时清扫"打包成纯组件。
///
/// 持有：单调递增的 `CallId` 分配、按 `CallId` 索引的关联插槽、出账队列。
/// 所有期限判定经注入的 `now`（时间作值），不读环境时钟。
///
/// 生命周期：`submit`（登记调用与期限）→ 驱动把到达的响应经 `complete(call_id, resp)`
/// 路由进来；驱动在节拍上反复 `settle(now)`；出账经
/// [`drain_outcomes`](CallDispatch::drain_outcomes) 消费。每调用**恰好产出一条**判定
/// （不静默丢，见 L1 语境的"值不静默消失"）。
pub struct CallDispatch<R> {
    next: u64,
    slots: Vec<Option<Slot>>,
    settled: VecDeque<(CallId, CallResult<R>)>,
    // 诚实簿记：迟到完成被吸收的次数（superseded 不双出账）。
    late_dropped: u64,
}

/// `complete` 的裁决：归位成功 / 迟到完成被吸收 / 伪造（无此调用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompleteOutcome {
    /// 在途且未超时，响应归位，产出一条 `Ok(resp)`。
    Settled,
    /// 该调用已判超时出账；迟到响应被吸收（不双出账）。
    StaleDropped,
    /// 无此调用（从未 `submit` 或早已归位释放）——协议违例，调用方应纠错。
    Spurious,
}

/// `submit` 的裁决：登记成功 / 期限已过（立即入账超时）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submit {
    Scheduled,
    AlreadyExpired,
}

impl<R> Default for CallDispatch<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> CallDispatch<R> {
    pub fn new() -> Self {
        CallDispatch {
            next: 0,
            slots: Vec::new(),
            settled: VecDeque::new(),
            late_dropped: 0,
        }
    }

    /// 登记一次在途调用，指定其期限。返回唯一 `CallId`。
    ///
    /// `now` 是注入的时间（时间作值）；若期限已过，立即退化为超时路径：
    /// 依旧分配插槽，由首次 [`settle`](CallDispatch::settle) 出账为 `TimedOut`
    /// （不伪造"已排期却没判"）。
    pub fn submit(&mut self, deadline: Instant, _now: Instant) -> (CallId, Submit) {
        let id = CallId(self.next);
        self.next += 1;
        // CallId 单调递增、槽永不复用 → 新 id 恒等于当前槽长，直接 Push（索引 == 值）。
        let expired = _now >= deadline;
        let slot = if expired {
            // 期限已过：判定已可确定，**当场出账**（不延后、不静默丢——每调用恰一条）。
            self.settled.push_back((id, CallResult::TimedOut));
            Slot::Timed
        } else {
            Slot::Await { deadline }
        };
        self.slots.push(Some(slot));
        let submit = if expired { Submit::AlreadyExpired } else { Submit::Scheduled };
        (id, submit)
    }

    /// 一条响应到达，按 `CallId` 归位。
    ///
    /// 路由方向（物理响应 → `CallId`）是调用方/驱动的职责，契约不在本模块——
    /// 本方法只负责"给出一个 `CallId` 与负载，我按关联语义裁决结果"。
    pub fn complete(&mut self, id: CallId, resp: R) -> CompleteOutcome {
        let idx = id.0 as usize;
        match self.slots.get_mut(idx) {
            Some(Some(Slot::Await { .. })) => {
                // 在途且未超时 → 归位出账。
                self.slots[idx] = None; // 释放插槽（无再分配复用；demo 尺度，防误用）
                self.settled.push_back((id, CallResult::Ok(resp)));
                CompleteOutcome::Settled
            }
            Some(Some(Slot::Timed)) => {
                // 已判超时出账 → 迟到完成：吸收，不双出账、申请数记录。
                self.late_dropped += 1;
                CompleteOutcome::StaleDropped
            }
            _ => CompleteOutcome::Spurious, // 空闲或越界：伪造 id
        }
    }

    /// 超时清扫：任一在途调用期限 ≤ `now` → 判超时出账为 `TimedOut`，插槽转 `Timed`
    /// （吸收其后可能的迟到完成）。返回本次结算出的**新增**超时数。
    pub fn settle(&mut self, now: Instant) -> usize {
        // 带索引遍历，转 Timed 时据此出账到对应 CallId。
        let mut fired = 0;
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if let Some(Slot::Await { deadline }) = slot {
                if now >= *deadline {
                    let id = CallId(idx as u64);
                    self.settled.push_back((id, CallResult::TimedOut));
                    *slot = Some(Slot::Timed);
                    fired += 1;
                }
            }
        }
        fired
    }

    /// 出账条目数（待消费）。
    pub fn pending_outcomes(&self) -> usize {
        self.settled.len()
    }

    /// 消费全部出账判定（每调用恰一条；次序为 settle/complete 触发的先后）。
    pub fn drain_outcomes(&mut self) -> impl Iterator<Item = (CallId, CallResult<R>)> {
        self.settled.drain(..)
    }

    /// 在途调用数（尚未判定的 Await）。
    pub fn in_flight(&self) -> usize {
        self.slots.iter().flatten().filter(|s| s.is_await()).count()
    }

    /// 诚实簿记读数：被吸收的迟到完成数（超时出账后响应才到，不双出账）。
    pub fn late_dropped(&self) -> u64 {
        self.late_dropped
    }
}

impl Slot {
    fn is_await(&self) -> bool {
        matches!(self, Slot::Await { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 假时钟：从 `t0` 起，`at(ms)` 给出该时刻。
    fn t0() -> Instant {
        Instant::now() - Duration::from_secs(1000)
    }
    fn at(epoch: Instant, ms: u64) -> Instant {
        epoch + Duration::from_millis(ms)
    }

    #[test]
    fn out_of_order_completion_associates_payloads() {
        // 两个在途调用，响应乱序到达 → 各自按 CallId 归位为正确负载。
        let epoch = t0();
        let mut d = CallDispatch::new();
        let (id_a, _) = d.submit(at(epoch, 100), at(epoch, 0));
        let (id_b, _) = d.submit(at(epoch, 200), at(epoch, 0));

        // B 先于 A 完成（乱序）。
        assert_eq!(d.complete(id_b, "resp-b"), CompleteOutcome::Settled);
        assert_eq!(d.complete(id_a, "resp-a"), CompleteOutcome::Settled);

        let out: Vec<_> = d.drain_outcomes().collect();
        assert_eq!(
            out,
            vec![
                (id_b, CallResult::Ok("resp-b")),
                (id_a, CallResult::Ok("resp-a")),
            ],
            "乱序完成按 CallId 归位，负载不错配"
        );
    }

    #[test]
    fn deadline_sweep_times_out_and_absorbs_late_completion() {
        // 期限内不完成 → settle 判超时；迟到的 complete 被吸收（不双出账）。
        let epoch = t0();
        let mut d = CallDispatch::new();
        let (id, _) = d.submit(at(epoch, 50), at(epoch, 0));

        assert_eq!(d.settle(at(epoch, 40)), 0, "期限内不判超时");
        assert_eq!(d.settle(at(epoch, 60)), 1, "期限过后判超时");
        assert_eq!(
            d.drain_outcomes().collect::<Vec<_>>(),
            vec![(id, CallResult::TimedOut)],
        );

        // 迟到完成：不双出账，进 late_dropped 簿记。
        assert_eq!(d.complete(id, "晚到"), CompleteOutcome::StaleDropped);
        assert_eq!(d.drain_outcomes().count(), 0, "不再产出判定");
        assert_eq!(d.late_dropped(), 1, "迟到完成被吸收计数");
    }

    #[test]
    fn immediate_expired_submit_settles_timed_out() {
        // 期限已过才 submit → AlreadyExpired，判定当场出账（不静默丢）。
        let epoch = t0();
        let mut d = CallDispatch::<&'static str>::new();
        let (id, submit) = d.submit(at(epoch, 10), at(epoch, 20));
        assert_eq!(submit, Submit::AlreadyExpired);
        assert_eq!(
            d.drain_outcomes().collect::<Vec<_>>(),
            vec![(id, CallResult::<&'static str>::TimedOut)],
            "过期即出账，无需等 settle"
        );
        assert_eq!(d.in_flight(), 0, "过期调用不入在途");
    }

    #[test]
    fn spurious_completion_rejected() {
        // 从未 submit（或已归位）的 CallId → Spurious（协议违例，语义为纠错）。
        let mut d = CallDispatch::new();
        assert_eq!(d.complete(CallId(99), "幽灵"), CompleteOutcome::Spurious);
    }

    #[test]
    fn every_submit_yields_exactly_one_outcome_no_silent_loss() {
        // 多个在途、部分超时部分完成 → 出账总数 == 提交数（值不静默消失）。
        let epoch = t0();
        let mut d = CallDispatch::new();
        let done1 = d.submit(at(epoch, 10), at(epoch, 0)).0;
        let done2 = d.submit(at(epoch, 10), at(epoch, 0)).0;
        let tmo1 = d.submit(at(epoch, 5), at(epoch, 0)).0;
        let tmo2 = d.submit(at(epoch, 5), at(epoch, 0)).0;

        d.complete(done1, "d1");
        d.complete(done2, "d2");
        d.settle(at(epoch, 6)); // 仅 tmo1/tmo2 超时

        let out: Vec<_> = d.drain_outcomes().collect();
        assert_eq!(out.len(), 4, "4 提交 → 4 出账");
        for (id, r) in out {
            match id {
                x if x == done1 => assert_eq!(r, CallResult::Ok("d1")),
                x if x == done2 => assert_eq!(r, CallResult::Ok("d2")),
                x if x == tmo1 => assert_eq!(r, CallResult::TimedOut),
                x if x == tmo2 => assert_eq!(r, CallResult::TimedOut),
                _ => panic!("未知 CallId"),
            }
        }
    }
}