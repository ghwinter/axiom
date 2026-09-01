//! 观测子系统（综合用例侧；三段式：收集 → 提交 → 打印）。
//!
//! 观测面不是通用面（不同软件观测需求不同）——本模块只服务本用例的观测：
//! 轮询裁决直方图（Ready/Pending/TimedOut）、馈入计数、步进数、应答 ok/err 分类、
//! 墙钟时延采样。不扩 runtime `Telemetry` 面（其投递语义保持通用）。
//!
//! 三段式：
//! - **收集**：[`ObservedPoller`] 包装 [`Poller`]——只读转发、不改 poll 语义
//!   （观测透明性：async_demo 的 T6 断言仍成立，即观测不改变行为的证据）；
//! - **提交**：[`ObsSummary`] 按序汇总观测事件；
//! - **打印**：[`print_summary`] 格式化输出（输出目的地）。
//!
//! 诚实边界（M8 熵）：观测到的是裁决与计数，不是 tokio 内部（调度器/任务队列）
//! 状态；时延为墙钟采样（与 sync 同刻度），不称 tokio 内部时延。

use axiom::cell_core::PortCell;
use axiom_semantics::seams::async_seam::{Poll, PollResult, Poller};
use std::time::{Duration, Instant};

/// 观测事件（记录面粒度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObsEvent {
    /// 输入就绪并同步完成一步。
    Step,
    /// 无输入（等待点挂起）。
    Pending,
    /// 期限耗尽。
    TimedOut,
    /// 通道馈入一条输入。
    Fed,
}

/// 观测汇总（提交面：按序聚合的观测记录）。
#[derive(Debug, Default, Clone)]
pub struct ObsSummary {
    /// Step 次数。
    pub steps: usize,
    /// Pending 次数（等待点挂起拍）。
    pub pending: usize,
    /// TimedOut 次数。
    pub timed_out: usize,
    /// 馈入输入条数。
    pub fed: usize,
    /// 应答 ok（非 `-ERR` 前缀）条数。
    pub ok: usize,
    /// 应答 err（`-ERR` 前缀）条数。
    pub err: usize,
    /// 每次 step 的墙钟时延采样（纳秒）。
    pub latency_ns: Vec<u64>,
}

impl ObsSummary {
    fn record(&mut self, ev: ObsEvent) {
        match ev {
            ObsEvent::Step => self.steps += 1,
            ObsEvent::Pending => self.pending += 1,
            ObsEvent::TimedOut => self.timed_out += 1,
            ObsEvent::Fed => self.fed += 1,
        }
    }
}

/// 收集段：观测型 Poller 包装（只读转发，poll 语义不变）。
pub struct ObservedPoller<A: PortCell> {
    inner: Poller<A>,
    summary: ObsSummary,
}

impl<A: PortCell> ObservedPoller<A> {
    /// 新建：与 [`Poller::new`] 同参数，附空观测。
    pub fn new(state: A::State, pending: Option<A::In>) -> Self {
        ObservedPoller {
            inner: Poller::new(state, pending),
            summary: ObsSummary::default(),
        }
    }

    /// 单次轮询：无输入 → 记录 `Pending`；有输入 → 同步 `step`，记录 `Step`。
    pub fn poll(&mut self) -> Poll<A::Out> {
        match self.inner.poll() {
            Poll::Ready(o) => {
                self.summary.record(ObsEvent::Step);
                Poll::Ready(o)
            }
            Poll::Pending => {
                self.summary.record(ObsEvent::Pending);
                Poll::Pending
            }
        }
    }

    /// 馈入一条输入（记录 `Fed` 后注入；经 `Poller::put`）。
    pub fn feed(&mut self, input: A::In) {
        self.summary.record(ObsEvent::Fed);
        self.inner.put(input);
    }

    /// 期限耗尽通知（记录 `TimedOut`）。
    pub fn note_timed_out(&mut self) {
        self.summary.record(ObsEvent::TimedOut);
    }

    /// 就绪应答观察：按前缀分类 ok/err（提交面）；记录本轮时延（墙钟采样）。
    pub fn observe_ready(&mut self, out: &str, t0: Instant) {
        if out.starts_with("-ERR") {
            self.summary.err += 1;
        } else {
            self.summary.ok += 1;
        }
        self.summary.latency_ns.push(t0.elapsed().as_nanos() as u64);
    }

    /// 观测汇总（提交面访问）。
    pub fn summary(&self) -> &ObsSummary {
        &self.summary
    }

    /// 观测型带期限轮询（sync 对拍侧；与 `poll_until` 同语义，每拍经观测）。
    pub fn observed_poll_until(&mut self, deadline: Instant, tick: Duration) -> PollResult<A::Out> {
        loop {
            if let Poll::Ready(o) = self.poll() {
                return PollResult::Ready(o);
            }
            if Instant::now() >= deadline {
                self.note_timed_out();
                return PollResult::TimedOut;
            }
            std::thread::sleep(tick);
        }
    }
}

/// 观测型馈入驱动（async 侧；与 `tokio_poll_fed` 同语义，每拍经观测）。
///
/// 期限由真定时器驱动（`timeout` 包裹 `recv`）；`tokio` feature 门控。
#[cfg(feature = "tokio")]
pub async fn observed_fed_run<A: PortCell>(
    obs: &mut ObservedPoller<A>,
    rx: &mut tokio::sync::mpsc::Receiver<A::In>,
    deadline: Instant,
    tick: Duration,
) -> PollResult<A::Out>
where
    A::In: Send,
{
    loop {
        if let Poll::Ready(o) = obs.poll() {
            return PollResult::Ready(o);
        }
        let now = Instant::now();
        if now >= deadline {
            obs.note_timed_out();
            return PollResult::TimedOut;
        }
        match tokio::time::timeout(deadline - now, rx.recv()).await {
            Ok(Some(input)) => obs.feed(input),
            Ok(None) => tokio::time::sleep(tick).await, // 通道关闭：让步至期限
            Err(_elapsed) => {
                obs.note_timed_out();
                return PollResult::TimedOut;
            }
        }
    }
}

/// 打印模块（输出目的地）：格式化观测汇总。
pub fn print_summary(tag: &str, s: &ObsSummary) {
    println!(
        "[observe] {tag}: steps {} / pending {} / fed {} / timed_out {} / ok {} / err {}",
        s.steps, s.pending, s.fed, s.timed_out, s.ok, s.err
    );
    if !s.latency_ns.is_empty() {
        let min = s.latency_ns.iter().copied().min().unwrap_or(0);
        let max = s.latency_ns.iter().copied().max().unwrap_or(0);
        let sum: u64 = s.latency_ns.iter().sum();
        let avg = sum / s.latency_ns.len() as u64;
        println!(
            "[observe] {tag}: wall-step latency min {min} ns / avg {avg} ns / max {max} ns (n={})",
            s.latency_ns.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::cell_core::PortCell;

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
    fn observed_poll_records_step_and_pending() {
        let mut o = ObservedPoller::<Inc>::new((), Some(5));
        assert_eq!(o.poll(), Poll::Ready(6));
        assert_eq!(o.poll(), Poll::Pending);
        let s = o.summary();
        assert_eq!(s.steps, 1);
        assert_eq!(s.pending, 1);
    }

    #[test]
    fn observed_poll_until_times_out_and_records() {
        let mut o = ObservedPoller::<Inc>::new((), None);
        let r = o
            .observed_poll_until(
                Instant::now() + Duration::from_millis(15),
                Duration::from_millis(2),
            );
        assert_eq!(r, PollResult::TimedOut);
        assert!(o.summary().timed_out >= 1, "期限耗尽须记录");
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn fed_run_records_and_matches_unobserved_semantics() {
        // 观测透明性：同输入序列下 observed_fed_run 的裁决与无观测路径一致；
        // 且 fed/steps 计数吻合。
        let lines = [1, 2, 3];
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(4);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("rt");
        rt.spawn(async move {
            for x in lines {
                let _ = tx.send(x).await;
            }
        });
        let mut obs = ObservedPoller::<Inc>::new((), None);
        let outs: Vec<PollResult<i32>> = rt.block_on(async {
            let mut v = Vec::new();
            for _ in 0..lines.len() {
                v.push(
                    observed_fed_run(
                        &mut obs,
                        &mut rx,
                        Instant::now() + Duration::from_millis(500),
                        Duration::from_millis(1),
                    )
                    .await,
                );
            }
            v
        });
        assert_eq!(
            outs,
            vec![PollResult::Ready(2), PollResult::Ready(3), PollResult::Ready(4)]
        );
        assert_eq!(obs.summary().fed, 3, "馈入计数一致");
        assert_eq!(obs.summary().steps, 3, "步进计数一致");
    }
}