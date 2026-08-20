//! 有界缓冲 / 背压（`foundations.md` §9.1）：一个容量上限 `CAP` 的 FIFO，满时对生产端施加背压。
//!
//! 用 `std::sync::mpsc::sync_channel(CAP)` 实现——这只是一种有界/有背压的原语：
//! - [`push`](BoundedQueue::push)（阻塞）满时**等待**消费端腾出（真正的背压）；
//! - [`try_push`](BoundedQueue::try_push)（非阻塞）满时返回 `Err`（丢弃/上抛的调用侧选择）。
//! 由此把"满时阻塞 / 丢弃 / 上抛"的策略**留给调用侧**，本原语只承载"容量上限 + 背压信号"。

use std::sync::mpsc;

/// 有界 FIFO，容量上限 `CAP`。
pub struct BoundedQueue<T, const CAP: usize> {
    tx: mpsc::SyncSender<T>,
    rx: mpsc::Receiver<T>,
}

impl<T: Send, const CAP: usize> BoundedQueue<T, CAP> {
    /// 新建一个有界队列（容量 `CAP`）。
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(CAP);
        Self { tx, rx }
    }

    /// 阻塞 push：满则等待消费端腾出（**背压**）。
    pub fn push(&self, v: T) {
        let _ = self.tx.send(v);
    }

    /// 非阻塞 push：满则返回 `Err(v)`（**背压信号**，由调用侧决定丢弃/重试/上抛）。
    pub fn try_push(&self, v: T) -> Result<(), T> {
        self.tx.try_send(v).map_err(|e| match e {
            mpsc::TrySendError::Full(v) | mpsc::TrySendError::Disconnected(v) => v,
        })
    }

    /// 阻塞 pop。
    pub fn pop(&self) -> Option<T> {
        self.rx.recv().ok()
    }

    /// 非阻塞 pop。
    pub fn try_pop(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// 容量上界（`Some(CAP)` 即有界）。
    pub fn spare(&self) -> Option<usize> {
        Some(CAP)
    }
}

impl<T: Send, const CAP: usize> Default for BoundedQueue<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}
