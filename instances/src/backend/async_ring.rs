//! # TokioBlockRing — 事件驱动异步块环（tokio 实例）
//!
//! 实现语义层 [`AsyncBlockRing`](axiom_semantics::movers::async_ring::AsyncBlockRing)
//! 契约的 **notify（事件驱动）** 路径：等待点挂进 tokio reactor，非忙轮询。
//!
//! 唤醒协议（双 `tokio::sync::Notify`）：
//! - 生产者 [`send`](TokioBlockRing::send) 因满而等待时，挂在 `not_full`；
//!   消费者腾位（`recv` 弹出）后 `not_full.notify_waiters()` 唤醒一个生产者；
//! - 消费者 [`recv`](TokioBlockRing::recv) 因空而等待时，挂在 `not_empty`；
//!   生产者提交（`send` 入队）后 `not_empty.notify_waiters()` 唤醒一个消费者。
//!
//! 语义（T6 与同步路径同序）：
//! - `send` = 等**非满**；`Closed` 拒绝后续投递（值随错误回传，L1 无静默丢失）；
//! - `recv` = 等**新块**；关闭且排空后返回 `None`。
//!
//! 取消安全：等待期间任务被 cancel 不破坏不变量——`send` 未入队时值随任务
//! 丢弃（调用方持有所有权，可重建）；`recv` 未取块时队列原样。等待循环
//! 重新进入时以锁内最新状态判定（不依赖唤醒计数）。
//!
//! 门控：`tokio` feature（`axiom-instances`）。安全：无 unsafe。
//!
//! ## 观测钩子（`telemetry-tracing` feature 门控）
//!
//! [`set_telemetry`](TokioBlockRing::set_telemetry) 给环挂一个语义层
//! `Telemetry` 接收面（`tracing` 适配见 crate 根的 `telemetry_tracing`）：
//! - 每次 `send` 成功入队 → `on_depth`（生产时刻的水位）+ `on_latency`
//!   （`send` 调用到入队成功的全程耗时——含等非满的背压窗口；背压真实发生
//!   时此值可见地变大，是"性能取决于最慢步骤"的直接读数）；
//! - `send` 因关闭被拒 → `on_verdict(Dropped)`（值随错误回传，观测同步登记）。
//!
//! 钩子是**实例侧扩展**，不改 `AsyncBlockRing` 契约（契约级遥测是后续裁定）；
//! 未挂载时零分支开销（`Option::None` 快路径）。

use axiom_semantics::movers::async_ring::Closed; // 内部实现用（re-export 的对外契约见下）
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

/// re-export：异步块环契约（语义层定义，实例层对外直接可用——消费端只需依赖
/// 实例层即可拿到契约，无需直接依赖语义层）。
pub use axiom_semantics::movers::async_ring::{AsyncBlockRing, Closed as RingClosed};

/// 观测钩子槽位（`telemetry-tracing` 门控）：接缝名 + 共享接收面。
#[cfg(feature = "telemetry-tracing")]
type TelHook = (
    &'static str,
    Arc<std::sync::Mutex<dyn axiom_semantics::seams::telemetry::Telemetry + Send>>,
);

/// 事件驱动异步块环（tokio 实现）。
///
/// `C` = 块类型（`Send`）；容量 = 环可积压的块数（背压点：满则生产者等待）。
/// 多生产/多消费**不承诺**（SPSC 语义：单写单读——但经
/// Mutex 允许多消费者竞争，语义退化为 FIFO 队列，仍正确；效率建议 SPSC）。
pub struct TokioBlockRing<C> {
    inner: Mutex<VecDeque<C>>,
    cap: usize,
    closed: AtomicBool,
    count: AtomicUsize,
    not_full: Notify,
    not_empty: Notify,
    /// 观测钩子（`telemetry-tracing` 门控；未挂载 = None，零分支开销）。
    #[cfg(feature = "telemetry-tracing")]
    tel: std::sync::Mutex<Option<TelHook>>,
}

impl<C: Send> TokioBlockRing<C> {
    /// 新建：容量 `cap`（块数）。
    ///
    /// 模态② 精神：`cap == 0` 在构造点**拒绝**（rendezvous 形态不属于有界队列语域），
    /// 与语义层 [`BoundedRing`](axiom_semantics::movers::ring::BoundedRing) 同门。
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "TokioBlockRing 容量必须 >= 1");
        TokioBlockRing {
            inner: Mutex::new(VecDeque::with_capacity(cap)),
            cap,
            closed: AtomicBool::new(false),
            count: AtomicUsize::new(0),
            not_full: Notify::new(),
            not_empty: Notify::new(),
            #[cfg(feature = "telemetry-tracing")]
            tel: std::sync::Mutex::new(None),
        }
    }

    /// 挂载观测钩子（`telemetry-tracing` 门控）：`seam` 是本环在观测面上的
    /// 接缝名（如 `"blocks→archive"`）；共享接收面（如 `TracingTelemetry`）由调用方
    /// 构造。须在 `into_shared` 之前调用。
    #[cfg(feature = "telemetry-tracing")]
    pub fn with_telemetry(
        self,
        seam: &'static str,
        tel: Arc<std::sync::Mutex<dyn axiom_semantics::seams::telemetry::Telemetry + Send>>,
    ) -> Self {
        *self.tel.lock().unwrap() = Some((seam, tel));
        self
    }

    /// `send` 成功入队后的上报：水位 + 生产侧全程耗时（含背压等待窗口）。
    #[cfg(feature = "telemetry-tracing")]
    fn report_send(&self, waited: std::time::Duration) {
        if let Some((seam, tel)) = self.tel.lock().unwrap().as_ref() {
            let mut t = tel.lock().unwrap();
            t.on_depth(seam, self.count.load(Ordering::Acquire));
            t.on_latency(seam, waited.as_nanos() as u64);
        }
    }

    /// `send` 因关闭被拒的上报：`Dropped`（值随错误回传，观测同步登记）。
    #[cfg(feature = "telemetry-tracing")]
    fn report_dropped(&self) {
        if let Some((seam, tel)) = self.tel.lock().unwrap().as_ref() {
            use axiom_semantics::seams::telemetry::VerdictView;
            tel.lock().unwrap().on_verdict(seam, VerdictView::Dropped);
        }
    }

    /// 共享句柄（驱动两侧各持 `Arc`）。
    pub fn into_shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl<C: Send> AsyncBlockRing<C> for TokioBlockRing<C> {
    async fn send(&self, item: C) -> Result<(), Closed> {
        #[cfg(feature = "telemetry-tracing")]
        let t0 = std::time::Instant::now();
        loop {
            {
                let mut q = self.inner.lock().await;
                if self.closed.load(Ordering::Acquire) {
                    #[cfg(feature = "telemetry-tracing")]
                    self.report_dropped();
                    return Err(Closed); // 值随错误回传，不静默丢
                }
                if self.count.load(Ordering::Relaxed) < self.cap {
                    q.push_back(item);
                    self.count.fetch_add(1, Ordering::Relaxed);
                    drop(q);
                    self.not_empty.notify_one(); // 唤醒一个消费者（新块已就绪）
                    #[cfg(feature = "telemetry-tracing")]
                    self.report_send(t0.elapsed());
                    return Ok(());
                }
            } // 释放锁后再等待（不持锁 await）
            self.not_full.notified().await; // 等空位（消费者腾位后唤醒）
        }
    }

    async fn recv(&self) -> Option<C> {
        loop {
            {
                let mut q = self.inner.lock().await;
                if let Some(v) = q.pop_front() {
                    self.count.fetch_sub(1, Ordering::Relaxed);
                    drop(q);
                    self.not_full.notify_one(); // 唤醒一个生产者（空位已腾出）
                    return Some(v);
                }
                if self.closed.load(Ordering::Acquire) {
                    return None; // 关闭且排空：消费侧收尾
                }
            }
            self.not_empty.notified().await; // 等新块（生产者提交后唤醒）
        }
    }

    fn len(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    fn capacity(&self) -> usize {
        self.cap
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        // 唤醒所有等待者：send 重新判定见 closed → Err；recv 排空后见 closed → None。
        self.not_full.notify_waiters();
        self.not_empty.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::cell_core::PortCell;
    use axiom_semantics::drive::async_flow::{run_sink, run_source};
    use axiom_semantics::drive::flow::bounded_pump;
    use std::time::Duration;

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

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Runtime::new()
            .expect("multi-thread rt")
            .block_on(f)
    }

    /// T6 对拍：同输入序列 → 同步泵（bounded_pump）与异步块环流水线同输出序。
    #[test]
    fn async_ring_matches_sync_pump() {
        let inputs = vec![1, 2, 3, 4, 5];
        // 同步路径：有界泵（Inc → 有界队列 → Double）
        let sync_outs = bounded_pump::<Inc, Double, _, 4>(|| (), || (), inputs.clone());
        assert_eq!(sync_outs, vec![4, 6, 8, 10, 12]);

        // 异步路径：TokioBlockRing（小容量触发背压）+ run_source/run_sink 并行
        let ring: TokioBlockRing<i32> = TokioBlockRing::new(2); // cap 2 < 5 块 → 触发背压
        let ring = ring.into_shared();
        let ring_src = ring.clone();
        let out = block_on(async move {
            // 并行任务：生产与消费互为驱动（等新块/等非满唤醒）。
            // `Arc` move 进任务（'static），内部 `&*arc` 解引用——驱动接受 `&R`。
            let r_sink = ring.clone();
            let sink_task = tokio::spawn(async move { run_sink::<i32, Double, _>(&*r_sink).await });
            let r_src = ring_src.clone();
            let src_task = tokio::spawn(async move { run_source::<Inc, i32, _, _>(&*r_src, inputs.clone()).await });
            let (outs, pushed) = (sink_task.await.expect("sink task"), src_task.await.expect("source task"));
            assert!(pushed, "全部投递并 close");
            outs
        });
        assert_eq!(out, sync_outs, "异步块环与同步泵同输入同输出（T6 多物理实现等价）");
    }

    /// 背压语义：容量 1 时生产者等待空位（等非满），慢消费者腾位节奏驱动生产。
    /// 输出序与无背压路径一致（T6）；等待真实发生（消费间隔即生产等待窗口）。
    #[test]
    fn producer_waits_for_room() {
        let ring: TokioBlockRing<i32> = TokioBlockRing::new(1); // 容量 1：强制背压
        let ring = ring.into_shared();
        let out = block_on(async {
            let ring_src = ring.clone();
            let producer = tokio::spawn(async move {
                run_source::<Inc, i32, _, _>(&*ring_src, vec![1, 2, 3]).await
            });
            let ring_sink = ring.clone();
            let consumer = tokio::spawn(async move {
                let mut state = ();
                let mut outs = Vec::new();
                while let Some(v) = ring_sink.recv().await {
                    outs.push(Double::step(&mut state, v));
                    tokio::time::sleep(Duration::from_millis(5)).await; // 慢消费：制造腾位节奏
                }
                outs
            });
            let (pushed, outs) = (producer.await.expect("producer task"), consumer.await.expect("consumer task"));
            assert!(pushed, "全部投递并 close");
            outs
        });
        assert_eq!(out, vec![4, 6, 8], "背压下同输入同输出（等非满等待真实发生）");
    }

    /// 观测钩子（telemetry-tracing）：send 上报水位+耗时、关闭拒绝上报 Dropped。
    /// 语义（T6/投递）不受钩子影响——同输入同输出，仅旁路观测。
    #[cfg(feature = "telemetry-tracing")]
    #[test]
    fn telemetry_hook_reports_depth_latency_and_dropped() {
        use axiom_semantics::seams::telemetry::{BufTelemetry, VerdictView};
        use std::sync::Mutex as StdMutex;

        let tel = Arc::new(StdMutex::new(BufTelemetry::new()));
        let ring: TokioBlockRing<i32> =
            TokioBlockRing::new(2).with_telemetry("ring-a", tel.clone());
        let ring = ring.into_shared();
        block_on(async {
            assert_eq!(ring.send(1).await, Ok(()));
            assert_eq!(ring.send(2).await, Ok(()));
            assert_eq!(ring.recv().await, Some(1));
            ring.close();
            assert_eq!(ring.send(9).await, Err(Closed), "关闭后 send 拒绝");
        });

        let t = tel.lock().unwrap();
        assert_eq!(t.depths, vec![("ring-a", 1), ("ring-a", 2)], "两次成功入队的水位读数");
        assert_eq!(t.latencies.len(), 2, "每次成功 send 一条耗时");
        assert_eq!(
            t.verdicts,
            vec![("ring-a", VerdictView::Dropped)],
            "关闭拒绝登记为 Dropped（值随错误回传）"
        );
    }

    /// 关闭语义：close 后 send 拒绝（值回传）、recv 排空后 None。
    #[test]
    fn close_semantics_on_tokio_ring() {
        let ring: TokioBlockRing<i32> = TokioBlockRing::new(4);
        let ring = ring.into_shared();
        block_on(async {
            let _ = ring.send(1).await;
            let _ = ring.send(2).await;
            assert_eq!(ring.recv().await, Some(1));
            ring.close();
            assert_eq!(ring.recv().await, Some(2), "关闭后仍排空积压");
            assert_eq!(ring.recv().await, None, "关闭且空 → None");
            assert_eq!(ring.send(9).await, Err(Closed), "关闭后 send 拒绝（值回传）");
        });
    }
}