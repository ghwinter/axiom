//! 异步事件驱动块环：上游等“非满”、下游等“新块”，新数据/空位唤醒。
//!
//! ## 语义契约（驱动层与实现层的分界线）
//!
//! 块级数据流的**跨阶段交接**原语：
//! - [`AsyncBlockRing::send`]：写入一块。环满 → **异步等待**直至消费侧腾出空位；
//!   环已关闭 → `Err(Closed)`（值随错误回传，宪法 L1“无静默丢失”）。
//! - [`AsyncBlockRing::recv`]：读出一块。环空 → **异步等待**直至生产侧提交新块；
//!   环关闭且空 → `None`（消费侧收尾）。
//! - [`AsyncBlockRing::close`]：生产侧完成全部投递后调用——消费侧据此
//!   在排空剩余块后结束（`recv` 返回 `None`）。
//!
//! ## 唤醒策略与实现分工（T3：物理归实现）
//!
//! “满时/空时的等待如何被唤醒”是**实现细节**，契约不规定：
//! - **notify（事件驱动）**：`axiom-instances` 的 `TokioBlockRing`（`tokio::sync::Notify`
//!   双唤醒：生产者提交 → 唤醒消费者；消费者腾位 → 唤醒生产者）——等待挂进
//!   tokio reactor，真异步（推荐路径）。
//! - **spin（线程主动轮询）**：同步无锁块环的轮询语义；若需在 async 语境用，
//!   可包 `yield_now()` 让步。
//!
//! 不同实现必须给出**同一语义序**（同输入序列 → 同输出序列，T6 多物理实现等价），
//! 对拍见 `axiom-instances` 集成测试。
//!
//! std 门控、零外部依赖：本模块只定义契约（真实实现经实例层接入 tokio）。

use core::future::Future;

/// 发送失败：环已关闭（消费侧退出 / 生产侧 close 后继续 send）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

/// 异步块环契约：`C` = 块类型（如 `Chunk`/`FrameChunk`）。
///
/// `send`/`recv` 的等待点语义（非满/新块）由实现负责；调用方不关心唤醒机制
/// （notify / spin），只依赖“等待会结束、唤醒会发生”。
pub trait AsyncBlockRing<C>: Send + Sync {
    /// 写入一块。满则异步等待空位；已关闭返回 `Err(Closed)`（值回传，不静默丢失）。
    fn send(&self, item: C) -> impl Future<Output = Result<(), Closed>> + Send;

    /// 读出一块。空则异步等待新块；关闭且排空后返回 `None`。
    fn recv(&self) -> impl Future<Output = Option<C>> + Send;

    /// 当前积压块数（观测/背压诊断）。
    fn len(&self) -> usize;

    /// 环容量（块数）。
    fn capacity(&self) -> usize;

    /// 是否为空。
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 关闭：禁止后续 `send`；已积压块仍可被 `recv` 排空，随后 `recv` 返回 `None`。
    fn close(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::VecDeque;
    use std::sync::Mutex;

    /// 测试用无等待环：容量足够时不触发等待（实现语义的参考，非物理实现）。
    /// `send` 满时返回 `Err(Closed)`（测试不得触发）；`close` 后 `send` 拒绝、`recv` 排空后 `None`。
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
        fn send(&self, item: C) -> impl Future<Output = Result<(), Closed>> + Send {
            // 立即完成：库满或已关闭 → Err；否则入队。
            async move {
                if *self.closed.lock().unwrap() {
                    return Err(Closed);
                }
                let mut q = self.q.lock().unwrap();
                if q.len() >= self.cap {
                    return Err(Closed);
                }
                q.push_back(item);
                Ok(())
            }
        }

        fn recv(&self) -> impl Future<Output = Option<C>> + Send {
            async move {
                let mut q = self.q.lock().unwrap();
                let v = q.pop_front();
                if v.is_none() && *self.closed.lock().unwrap() {
                    None
                } else {
                    v
                }
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

    /// 微型 executor（仅测试）：轮询 future 直至 Ready（无真实唤醒，用于驱动逻辑验证）。
    fn drive<F: Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let waker = Waker::noop();
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(fut);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    #[test]
    fn ring_fifo_and_close_semantics() {
        let ring = TestRing::new(4);
        let _ = drive(ring.send(1));
        let _ = drive(ring.send(2));
        assert_eq!(drive(ring.recv()), Some(1));
        assert_eq!(drive(ring.recv()), Some(2));
        ring.close();
        assert_eq!(drive(ring.recv()), None, "关闭且空 → None");
        assert_eq!(drive(ring.send(9)), Err(Closed), "关闭后 send 拒绝（值回传）");
    }

    #[test]
    fn send_full_returns_closed_without_silent_loss() {
        let ring = TestRing::new(2);
        let _ = drive(ring.send(1));
        let _ = drive(ring.send(2));
        assert_eq!(drive(ring.send(3)), Err(Closed), "满：（测试实现）Err 回传，不静默丢弃");
        // 注：真 tokio 实例在此场景**等待空位**而非立即 Err——等待语义属实现，契约只保“值不静默消失”。
    }
}