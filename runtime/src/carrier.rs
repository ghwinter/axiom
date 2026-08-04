//! Parallel 模式的链路载体——按 `LinkKind` 选择物理 channel 实现。
//!
//! 统一 `mpsc::Sender`（无界）、`mpsc::SyncSender`（有界阻塞/丢弃）、
//! 自定义有界覆盖载体（`Overwriting`）、单槽覆盖载体（`Latest`/`SharedState`）
//! 四种物理载体，使路由表能持有异构 sender。

use alloc::boxed::Box;
use alloc::string::String;

/// Parallel 模式 channel 的消息类型：(port_name, payload)。
/// port 名随消息走——下游线程用它 inject，而非固定端口。
pub(crate) type RoutedMsg = (String, Box<dyn core::any::Any + Send>);

/// Parallel 模式的链路 carrier——统一异构 sender，使路由表能持有
/// `mpsc::Sender` / `SyncSender` / 自定义载体。
///
/// `send` 按 carrier 语义投递：
/// - `Unbounded`        → `Sender::send`（永不阻塞）；
/// - `BoundedBlocking`  → `SyncSender::send`（满则阻塞，承载 `WritePolicy::Blocking`）；
/// - `BoundedDropping`  → `SyncSender::try_send`（满则丢弃新消息，承载
///   `WritePolicy::Dropping` / `Channel { drop_when_full }`）；
/// - `Overwriting`      → 自定义有界覆盖（满时覆盖最老，承载
///   `WritePolicy::Overwriting` 原生语义）；
/// - `Slot`             → 单槽覆盖（`Latest` / `SharedState`，读者见最新值）。
///
/// 发送失败（下游断开）静默丢弃——级联停机由 receiver 侧检测，错误不向上传播。
pub(crate) enum ChanSender {
    Unbounded(std::sync::mpsc::Sender<RoutedMsg>),
    BoundedBlocking(std::sync::mpsc::SyncSender<RoutedMsg>),
    BoundedDropping(std::sync::mpsc::SyncSender<RoutedMsg>),
    Overwriting(OverwriteSender<RoutedMsg>),
    Slot(SlotSender<RoutedMsg>),
}

impl ChanSender {
    pub(crate) fn send(&self, msg: RoutedMsg) {
        match self {
            ChanSender::Unbounded(s) => { let _ = s.send(msg); }
            ChanSender::BoundedBlocking(s) => { let _ = s.send(msg); }
            ChanSender::BoundedDropping(s) => { let _ = s.try_send(msg); }
            ChanSender::Overwriting(s) => s.send(msg),
            ChanSender::Slot(s) => s.send(msg),
        }
    }
}

/// Parallel 模式的统一接收端——机器线程 / forward 线程用它 recv。
/// `recv`（阻塞）返回 `None` 表示载体断开（级联停机）；`try_recv`
/// （非阻塞）返回 `None` 表示暂时无消息（`ReadPolicy::NonBlocking`
/// 轮询用）。
pub(crate) enum ChanReceiver {
    Mpsc(std::sync::mpsc::Receiver<RoutedMsg>),
    Overwriting(OverwriteReceiver<RoutedMsg>),
    Slot(SlotReceiver<RoutedMsg>),
}

impl ChanReceiver {
    pub(crate) fn recv(&self) -> Option<RoutedMsg> {
        match self {
            ChanReceiver::Mpsc(r) => r.recv().ok(),
            ChanReceiver::Overwriting(r) => r.recv(),
            ChanReceiver::Slot(r) => r.recv(),
        }
    }
    /// 非阻塞取消息。`Ok(Some)` 有消息；`Ok(None)` 暂无（轮询）；
    /// `Err(())` 载体断开（级联停机）——`NonBlocking` 轮询靠它退出。
    pub(crate) fn try_recv(&self) -> Result<Option<RoutedMsg>, ()> {
        match self {
            ChanReceiver::Mpsc(r) => match r.try_recv() {
                Ok(m) => Ok(Some(m)),
                Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(()),
            },
            ChanReceiver::Overwriting(r) => r.try_recv(),
            ChanReceiver::Slot(r) => r.try_recv(),
        }
    }
}

// ── 有界覆盖载体 ────────────────────────────────────────────────────────────

/// 有界覆盖载体：满时覆盖最老元素（`WritePolicy::Overwriting` 的原生
/// 语义）。所有 sender 释放后 closed → `recv` 返回 `None`（级联停机）。
struct OverwriteSharedInner<T> {
    buf: std::sync::Mutex<std::collections::VecDeque<T>>,
    cap: usize,
    cv: std::sync::Condvar,
    senders: std::sync::atomic::AtomicUsize,
}

pub(crate) struct OverwriteSender<T>(std::sync::Arc<OverwriteSharedInner<T>>);
pub(crate) struct OverwriteReceiver<T>(std::sync::Arc<OverwriteSharedInner<T>>);

impl<T> OverwriteSender<T> {
    fn new(cap: usize) -> (Self, OverwriteReceiver<T>) {
        let inner = std::sync::Arc::new(OverwriteSharedInner {
            buf: std::sync::Mutex::new(std::collections::VecDeque::new()),
            cap,
            cv: std::sync::Condvar::new(),
            senders: std::sync::atomic::AtomicUsize::new(1),
        });
        (OverwriteSender(inner.clone()), OverwriteReceiver(inner))
    }
    fn send(&self, msg: T) {
        let mut b = self.0.buf.lock().unwrap();
        if b.len() == self.0.cap {
            b.pop_front(); // 覆盖最老
        }
        b.push_back(msg);
        self.0.cv.notify_one();
    }
}
impl<T> Drop for OverwriteSender<T> {
    fn drop(&mut self) {
        if self.0.senders.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            self.0.cv.notify_all(); // 最后一个 sender：唤醒 recv 检查 closed
        }
    }
}
impl<T> Clone for OverwriteSender<T> {
    fn clone(&self) -> Self {
        self.0.senders.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        OverwriteSender(self.0.clone())
    }
}
impl<T> OverwriteReceiver<T> {
    fn recv(&self) -> Option<T> {
        let mut b = self.0.buf.lock().unwrap();
        loop {
            if let Some(m) = b.pop_front() {
                return Some(m);
            }
            if self.0.senders.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                return None; // 断开：级联停机
            }
            b = self.0.cv.wait(b).unwrap();
        }
    }
    fn try_recv(&self) -> Result<Option<T>, ()> {
        let mut b = self.0.buf.lock().unwrap();
        if let Some(m) = b.pop_front() {
            Ok(Some(m))
        } else if self.0.senders.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            Err(())
        } else {
            Ok(None)
        }
    }
}

// ── 单槽覆盖载体 ────────────────────────────────────────────────────────────

/// 单槽覆盖载体：`Latest`（单槽，覆盖写，读者见最新）与 `SharedState`
/// （共享状态，单消费者近似）。所有 sender 释放后 closed → `recv` 返回
/// `None`。
struct SlotSharedInner<T> {
    slot: std::sync::Mutex<Option<T>>,
    cv: std::sync::Condvar,
    senders: std::sync::atomic::AtomicUsize,
}

pub(crate) struct SlotSender<T>(std::sync::Arc<SlotSharedInner<T>>);
pub(crate) struct SlotReceiver<T>(std::sync::Arc<SlotSharedInner<T>>);

impl<T> SlotSender<T> {
    fn new() -> (Self, SlotReceiver<T>) {
        let inner = std::sync::Arc::new(SlotSharedInner {
            slot: std::sync::Mutex::new(None),
            cv: std::sync::Condvar::new(),
            senders: std::sync::atomic::AtomicUsize::new(1),
        });
        (SlotSender(inner.clone()), SlotReceiver(inner))
    }
    fn send(&self, msg: T) {
        let mut s = self.0.slot.lock().unwrap();
        *s = Some(msg); // 覆盖旧值：读者只见最新
        self.0.cv.notify_one();
    }
}
impl<T> Drop for SlotSender<T> {
    fn drop(&mut self) {
        if self.0.senders.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            self.0.cv.notify_all();
        }
    }
}
impl<T> Clone for SlotSender<T> {
    fn clone(&self) -> Self {
        self.0.senders.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        SlotSender(self.0.clone())
    }
}
impl<T> SlotReceiver<T> {
    fn recv(&self) -> Option<T> {
        let mut s = self.0.slot.lock().unwrap();
        loop {
            if let Some(m) = s.take() {
                return Some(m);
            }
            if self.0.senders.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                return None;
            }
            s = self.0.cv.wait(s).unwrap();
        }
    }
    fn try_recv(&self) -> Result<Option<T>, ()> {
        let mut s = self.0.slot.lock().unwrap();
        if let Some(m) = s.take() {
            Ok(Some(m))
        } else if self.0.senders.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            Err(())
        } else {
            Ok(None)
        }
    }
}

// ── 载体选择 ─────────────────────────────────────────────────────────────────

/// 按 `LinkKind` 选择 Parallel carrier：
/// - `BoundedBuf { write_policy: Blocking }` / `Channel { !drop_when_full }`
///   → `sync_channel(capacity)`（满则阻塞，自然背压）；
/// - `BoundedBuf { write_policy: Dropping }` / `Channel { drop_when_full }`
///   → `sync_channel(capacity)` + `try_send`（满则丢弃新消息）；
/// - `BoundedBuf { write_policy: Overwriting }` → **自定义有界覆盖载体**
///   （满时覆盖最老——`Overwriting` 的原生语义）；
/// - `Latest` / `SharedState` → **单槽覆盖载体**（读者见最新值）；
/// - `Inline` / `CasFreeRing` → 无界 channel（`CasFreeRing` 的无锁固定地址
///   载体是嵌入式场景，runtime 迁移为无界 channel——文档标注）。
pub(crate) fn channel_for(kind: &axiom::link::LinkKind) -> (ChanSender, ChanReceiver) {
    use axiom::link::{LinkKind, WritePolicy};
    use std::sync::mpsc;
    match kind {
        LinkKind::BoundedBuf { capacity, write_policy, .. } => {
            match write_policy {
                WritePolicy::Overwriting => {
                    let (tx, rx) = OverwriteSender::<RoutedMsg>::new(*capacity.max(&1));
                    (ChanSender::Overwriting(tx), ChanReceiver::Overwriting(rx))
                }
                _ => {
                    let (tx, rx) = mpsc::sync_channel::<RoutedMsg>(*capacity);
                    let sender = match write_policy {
                        WritePolicy::Blocking => ChanSender::BoundedBlocking(tx),
                        _ => ChanSender::BoundedDropping(tx),
                    };
                    (sender, ChanReceiver::Mpsc(rx))
                }
            }
        }
        LinkKind::Channel { capacity, drop_when_full } => {
            let (tx, rx) = mpsc::sync_channel::<RoutedMsg>(*capacity);
            let sender = if *drop_when_full {
                ChanSender::BoundedDropping(tx)
            } else {
                ChanSender::BoundedBlocking(tx)
            };
            (sender, ChanReceiver::Mpsc(rx))
        }
        // Latest / SharedState：单槽覆盖（最新值语义）。
        LinkKind::Latest { .. } | LinkKind::SharedState => {
            let (tx, rx) = SlotSender::<RoutedMsg>::new();
            (ChanSender::Slot(tx), ChanReceiver::Slot(rx))
        }
        // Inline / CasFreeRing：迁移为无界 channel（跨线程 Inline 即函数
        // 调用→channel 的语义迁移；CasFreeRing 的无锁载体属嵌入式场景）。
        _ => {
            let (tx, rx) = mpsc::channel::<RoutedMsg>();
            (ChanSender::Unbounded(tx), ChanReceiver::Mpsc(rx))
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// carrier 覆盖语义单元测试
//
// 端到端测试（tests.rs 的 runtime_*）无法可靠触发覆盖——Sequential 逐跳
// 路由让 d2 在 d1 下一个输出前消费，buffer 不积累；Parallel 下是否积累依赖
// 线程调度（Windows 碰巧积累，Linux 抢先消费）。覆盖语义是 carrier 自身
// 的确定性属性，在此直接验证，不经过 runtime。
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_sender_covers_oldest_when_full() {
        // cap=2：写 1,2,3 → 满时覆盖最老（1）→ recv 得到 2,3。
        let (tx, rx) = OverwriteSender::<i32>::new(2);
        tx.send(1);
        tx.send(2);
        tx.send(3); // 满，覆盖 1
        drop(tx);   // 释放 sender → recv 不阻塞
        assert_eq!(rx.recv(), Some(2));
        assert_eq!(rx.recv(), Some(3));
        assert_eq!(rx.recv(), None); // 断开
    }

    #[test]
    fn overwrite_sender_keeps_all_when_not_full() {
        // cap=4：写 1,2,3 → 未满，无覆盖 → recv 得到 1,2,3。
        let (tx, rx) = OverwriteSender::<i32>::new(4);
        tx.send(1);
        tx.send(2);
        tx.send(3);
        drop(tx);
        assert_eq!(rx.recv(), Some(1));
        assert_eq!(rx.recv(), Some(2));
        assert_eq!(rx.recv(), Some(3));
        assert_eq!(rx.recv(), None);
    }

    #[test]
    fn overwrite_try_recv_empty_then_disconnected() {
        // 空 → try_recv Ok(None)；drop sender 后 → Err(())。
        let (tx, rx) = OverwriteSender::<i32>::new(2);
        assert_eq!(rx.try_recv(), Ok(None));
        drop(tx);
        assert_eq!(rx.try_recv(), Err(()));
    }

    #[test]
    fn slot_sender_overwrites_with_latest() {
        // 写 1,2,3 → 单槽覆盖 → recv 只得到 3。
        let (tx, rx) = SlotSender::<i32>::new();
        tx.send(1);
        tx.send(2);
        tx.send(3);
        drop(tx);
        assert_eq!(rx.recv(), Some(3));
        assert_eq!(rx.recv(), None);
    }

    #[test]
    fn slot_try_recv_empty_then_disconnected() {
        let (tx, rx) = SlotSender::<i32>::new();
        assert_eq!(rx.try_recv(), Ok(None));
        drop(tx);
        assert_eq!(rx.try_recv(), Err(()));
    }
}
