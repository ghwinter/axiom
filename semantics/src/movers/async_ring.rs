//! 异步事件驱动块环：上游等“非满”、下游等“新块”，新数据/空位唤醒。
//!
//! ## 语义契约（驱动层与实现层的分界线）
//!
//! 块级数据流的跨阶段交接原语：
//! - [`AsyncBlockRing::send`]：写入一块。环满 → 异步等待直至消费侧腾出空位；
//!   环已关闭 → `Err(Closed(被拒块))`（值随错误回传，宪法 L1“无静默丢失”；对齐
//!   [`crate::checks::delivery::Delivery::Closed`]）。
//! - [`AsyncBlockRing::recv`]：读出一块。环空 → 异步等待直至生产侧提交新块；
//!   环关闭且空 → `None`（消费侧收尾）。
//! - [`AsyncBlockRing::close`]：生产侧完成全部投递后调用——消费侧据此
//!   在排空剩余块后结束（`recv` 返回 `None`）。
//!
//! ## 序保持与重复（N1 回溯审计落点：基声明，非惯例）
//!
//! **序保持**：同一生产者的块按 `send` 序交付 `recv`（FIFO）——这是载体
//! 声明面义务（I1 三通道之“基声明”），不是 ④ 惯例：乱序违反的后果是下游
//! 序敏感状态机被污染（不可逆：因果不可重放），按 N1 判据不得住惯例面；
//! 而 H2 封顶下任意实现者的序保持不可编译期强制（②不可达），故落 ③——
//! 序等价由对拍见证（`axiom-instances` 背压对拍断言同输入同输出序）。
//!
//! **重复**：无需声明——单消费者环 `recv` 即移除（结构上无重复，模态①）；
//! 多路复制须经显式 `Clone`（`Broadcast`/`Diamond` 的 `SRC::Out: Clone`，
//! 复制在类型面可见）。
//!
//! ## 唤醒策略与实现分工（T3：物理归实现）
//!
//! “满时/空时的等待如何被唤醒”是实现细节，契约不规定：
//! - **notify（事件驱动）**：`axiom-instances` 的 `TokioBlockRing`（`tokio::sync::Notify`
//!   双唤醒：生产者提交 → 唤醒消费者；消费者腾位 → 唤醒生产者）——等待挂进
//!   tokio reactor，真异步（推荐路径）。
//! - **spin（线程主动轮询）**：同步无锁块环的轮询语义；若需在 async 语境用，
//!   可包 `yield_now()` 让步。
//!
//! 不同实现必须给出同一语义序（同输入序列 → 同输出序列，T6 多物理实现等价），
//! 对拍见 `axiom-instances` 集成测试。
//!
//! std 门控、零外部依赖：本模块只定义契约（真实实现经实例层接入 tokio）。

use core::future::Future;

/// 发送失败：环已关闭（消费侧退出 / 生产侧 close 后继续 send）。
///
/// **携带被拒块**（T2 值守恒修正）：`Closed(C)` 与
/// [`Delivery::Closed(T)`](crate::checks::delivery::Delivery::Closed) 同形——
/// 值随错误回传调用方，不静默丢弃（宪法 L1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed<C>(pub C);

/// 异步块环契约：`C` = 块类型（如 `Chunk`/`FrameChunk`）。
///
/// `send`/`recv` 的等待点语义（非满/新块）由实现负责；调用方不关心唤醒机制
/// （notify / spin），只依赖“等待会结束、唤醒会发生”。
pub trait AsyncBlockRing<C>: Send + Sync {
    /// 写入一块。满则异步等待空位；已关闭返回 `Err(Closed(item))`
    /// （被拒值回传调用方，不静默丢失）。
    fn send(&self, item: C) -> impl Future<Output = Result<(), Closed<C>>> + Send;

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
    /// `close` 后 `send` 拒绝（`Err(Closed(被拒块))`）、`recv` 排空后 `None`。
    ///
    /// **测试面注记**：本实现无法等待，环满时也以 `Err(Closed(v))` 拒绝——与真实
    /// 实现“满 → 异步等待空位”是不同语义（T2 一并区分；T6 对拍在实例层）。
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
        fn send(&self, item: C) -> impl Future<Output = Result<(), Closed<C>>> + Send {
            // 立即完成：库满或已关闭 → Err（携带被拒块）；否则入队。
            async move {
                if *self.closed.lock().unwrap() {
                    return Err(Closed(item));
                }
                let mut q = self.q.lock().unwrap();
                if q.len() >= self.cap {
                    return Err(Closed(item));
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
        assert_eq!(drive(ring.send(9)), Err(Closed(9)), "关闭后 send 拒绝（值随错误回传）");
    }

    #[test]
    fn rejection_carries_value_without_silent_loss() {
        let ring = TestRing::new(2);
        let _ = drive(ring.send(1));
        let _ = drive(ring.send(2));
        // 满被拒（测试实现）与关闭被拒同形，但值都随错误回传（L1 值守恒的机械化见证）。
        assert_eq!(drive(ring.send(3)), Err(Closed(3)), "满：（测试实现）Err(Closed(3)) 回传，不静默丢弃");
        ring.close();
        assert_eq!(drive(ring.send(4)), Err(Closed(4)), "关闭：Err(Closed(4)) 回传（对齐 Delivery::Closed）");
        // 注：真 tokio 实例在“满”场景等待空位而非立即 Err——等待语义属实现，契约只保“值不静默消失”。
    }
}