//! 有界缓冲 / 背压（`foundations.md` §9.1）：一个容量上限 `CAP` 的 FIFO，满时对生产端施加背压。
//!
//! 用 `std::sync::mpsc::sync_channel(CAP)` 实现——这只是一种有界/有背压的原语：
//! - [`push`](BoundedQueue::push)（阻塞）满时**等待**消费端腾出（真正的背压）；
//! - [`try_push`](BoundedQueue::try_push)（非阻塞）满时返回 `Err`（丢弃/上抛的调用侧选择）。
//! 由此把"满时阻塞 / 丢弃 / 上抛"的策略**留给调用侧**，本原语只承载"容量上限 + 背压信号"。
//!
//! **终止性/诚实性**：`push` 在接收端断连时返回 `Err(被拒值)` **不静默丢值**；
//! `pop`/`try_pop` 用 `Result` **区分"空"与"断连"**（不再以 `None` 混淆）。

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
    ///
    /// 返回 `Err(值)` 当且仅当接收端已断连——**值不被静默丢弃**，由调用侧决定补救。
    pub fn push(&self, v: T) -> Result<(), T> {
        self.tx.send(v).map_err(|e| e.0)
    }

    /// 非阻塞 push：满则返回 `Err(v)`（**背压信号**），由调用侧决定丢弃/重试/上抛。
    pub fn try_push(&self, v: T) -> Result<(), T> {
        self.tx.try_send(v).map_err(|e| match e {
            mpsc::TrySendError::Full(v) | mpsc::TrySendError::Disconnected(v) => v,
        })
    }

    /// 阻塞 pop：`Ok(值)`=有值；`Err(RecvError)`=发送端断连。
    ///
    /// 与 `try_pop` 配合：`try_pop` 的 `Err(Empty)` 表示"此刻为空"（可重试），
    /// `Err(Disconnected)` 表示"发送端断连"——"空"与"断连"不再被 `None` 混淆。
    pub fn pop(&self) -> Result<T, mpsc::RecvError> {
        self.rx.recv()
    }

    /// 非阻塞 pop：`Err(Empty)`=此刻为空（可重试），`Err(Disconnected)`=发送端断连。
    pub fn try_pop(&self) -> Result<T, mpsc::TryRecvError> {
        self.rx.try_recv()
    }

    /// 容量上界（编译期常量 `CAP`）。
    pub fn capacity(&self) -> usize {
        CAP
    }
}

impl<T: Send, const CAP: usize> Default for BoundedQueue<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}