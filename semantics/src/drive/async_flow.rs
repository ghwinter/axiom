//! 异步流水线驱动：把 `PortCell` 与其前后两级异步块环接通。
//!
//! 场景：数据形态转换链（块级数据经多段 `PortCell` 变换）以异步块环为交接点
//! 的流水线化。每个 cell 是一个流水线段：
//!
//! ```text
//! Source ──step──► ring ──recv──► Transform ──step──► ring ──recv──► Sink
//!           send(等非满)              send(等非满)              recv(等新块)
//! ```
//!
//! - [`run_source`]：生产侧——把一组输入逐条经 `A::step` 计算，输出 `send` 到环
//!   （满则异步等待空位）；全部投递后 `close()`（消费侧据此收尾）。
//! - [`run_sink`]：消费侧——`recv`（空则异步等待新块）逐条驱动 `B::step`，
//!   环关闭且排空后返回输出序列。
//!
//! `run_source`/`run_sink` 是单任务驱动单元：生产与消费可并发（不同执行上下文）
//! ——实例层（`axiom-instances` tokio feature）用 `tokio::spawn` 装配二者；
//! 单线程串行语义由「先跑完 source、再跑 sink」的调用序获得（学业化对照）。
//!
//! 等待点语义全归块环实现（[`AsyncBlockRing`](crate::movers::async_ring::AsyncBlockRing)）：
//! 本模块只做「step → 投递 / 取块 → step」的接通，不掺唤醒机制。
//!
//! std 门控、零外部依赖（不做 spawn——executor 是生态领域，实例层提供）。

use axiom::cell_core::PortCell;

use crate::movers::async_ring::AsyncBlockRing;

/// 生产侧驱动：`A::step`（`In → Out`）的输出逐块投递到异步块环。
///
/// 返回 `true` = 全部输入已投递并 `close()`；`false` = 环提前关闭（消费侧退出），
/// 剩余输入未投递（不静默延续生产，对应 `bounded_pump` 的拆除语义）。
pub async fn run_source<A, C, R, It>(ring: &R, inputs: It) -> bool
where
    A: PortCell<Out = C>,
    C: Send,
    R: AsyncBlockRing<C>,
    It: IntoIterator<Item = A::In>,
{
    let mut state = A::State::default();
    for input in inputs {
        let out = A::step(&mut state, input);
        if ring.send(out).await.is_err() {
            // 消费侧已关闭：停止生产（值不静默丢——收尾由调用方裁决）。
            ring.close();
            return false;
        }
    }
    ring.close();
    true
}

/// 消费侧驱动：从异步块环取块（等新块）逐条驱动 `B::step`，返回输出序列。
///
/// 环关闭且排空（`recv` 返回 `None`）后结束。`B::In = C` 由接线保证（编译期 T1）。
pub async fn run_sink<C, B, R>(ring: &R) -> Vec<B::Out>
where
    B: PortCell<In = C>,
    C: Send,
    R: AsyncBlockRing<C>,
{
    let mut state = B::State::default();
    let mut out = Vec::new();
    while let Some(item) = ring.recv().await {
        out.push(B::step(&mut state, item));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movers::async_ring::AsyncBlockRing;
    use alloc::collections::VecDeque;
    use alloc::vec;
    use std::sync::Mutex;

    /// 测试用无等待环（容量足够不触发等待；同 async_ring 测试的实现）。
    struct TestRing<C> {
        q: Mutex<VecDeque<C>>,
        cap: usize,
        closed: Mutex<bool>,
    }

    impl<C> TestRing<C> {
        fn new(cap: usize) -> Self {
            TestRing { q: Mutex::new(VecDeque::new()), cap, closed: Mutex::new(false) }
        }
    }

    impl<C: Send> AsyncBlockRing<C> for TestRing<C> {
        async fn send(&self, item: C) -> Result<(), crate::movers::async_ring::Closed<C>> {
            if *self.closed.lock().unwrap() {
                return Err(crate::movers::async_ring::Closed(item));
            }
            let mut q = self.q.lock().unwrap();
            if q.len() >= self.cap {
                return Err(crate::movers::async_ring::Closed(item));
            }
            q.push_back(item);
            Ok(())
        }

        async fn recv(&self) -> Option<C> {
            let mut q = self.q.lock().unwrap();
            let v = q.pop_front();
            if v.is_none() && *self.closed.lock().unwrap() {
                None
            } else {
                v
            }
        }

        fn len(&self) -> usize {
            self.q.lock().unwrap().len()
        }

        fn capacity(&self) -> usize {
            self.cap
        }

        fn close(&self) {
            *self.closed.lock().unwrap() = true;
        }
    }

    fn drive<F: core::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut fut = std::pin::pin!(fut);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

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
    fn source_then_sink_sequential_matches_chain() {
        // 串行调用序：run_source 全部投递并 close → run_sink 排空并驱动 B。
        // 语义应等于 Chain<A,B> 的逐输入 step（Inc(+1) 后 Double(×2)）。
        let ring = TestRing::new(16);
        let ok = drive(run_source::<Inc, i32, _, _>(&ring, vec![1, 2, 3]));
        assert!(ok, "全部投递并 close");
        let outs = drive(run_sink::<i32, Double, _>(&ring));
        assert_eq!(outs, vec![4, 6, 8], "[1,2,3] → (+1,×2) → [4,6,8]");
    }

    #[test]
    fn closed_ring_stops_source_early() {
        // 环提前关闭：run_source 遇到 Err(Closed) 即停止（不静默延续生产）。
        let ring = TestRing::new(1);
        ring.close(); // 生产前已关闭
        let ok = drive(run_source::<Inc, i32, _, _>(&ring, vec![1, 2, 3]));
        assert!(!ok, "环已关闭 → 生产提前停止");
        let outs = drive(run_sink::<i32, Double, _>(&ring));
        assert!(outs.is_empty());
    }
}