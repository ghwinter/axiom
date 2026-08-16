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
    /// 无锁 SPSC 环（`CasFreeRing`：有界 FIFO，满时自旋阻塞）。
    Ring(RingSender<RoutedMsg>),
}

impl ChanSender {
    pub(crate) fn send(&self, msg: RoutedMsg) {
        match self {
            ChanSender::Unbounded(s) => { let _ = s.send(msg); }
            ChanSender::BoundedBlocking(s) => { let _ = s.send(msg); }
            ChanSender::BoundedDropping(s) => { let _ = s.try_send(msg); }
            ChanSender::Overwriting(s) => s.send(msg),
            ChanSender::Slot(s) => s.send(msg),
            ChanSender::Ring(s) => s.send(msg),
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
    Ring(RingReceiver<RoutedMsg>),
}

impl ChanReceiver {
    pub(crate) fn recv(&self) -> Option<RoutedMsg> {
        match self {
            ChanReceiver::Mpsc(r) => r.recv().ok(),
            ChanReceiver::Overwriting(r) => r.recv(),
            ChanReceiver::Slot(r) => r.recv(),
            ChanReceiver::Ring(r) => r.recv(),
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
            ChanReceiver::Ring(r) => r.try_recv(),
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
        // CasFreeRing：真无锁 SPSC 环（有界 FIFO，满时自旋阻塞）。
        LinkKind::CasFreeRing { capacity, .. } => {
            let (tx, rx) = RingSender::<RoutedMsg>::new(*capacity.max(&1));
            (ChanSender::Ring(tx), ChanReceiver::Ring(rx))
        }
        // Inline：跨线程即函数调用→channel 的语义迁移（无界）。
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

    // ── SPSC 无锁环（CasFreeRing）──

    #[test]
    fn ring_single_thread_fifo() {
        // 同线程（仅验证 FIFO 顺序与满/空判定；真 SPSC 在跨线程测试）。
        let (tx, rx) = RingSender::<i32>::new(4);
        tx.send(1);
        tx.send(2);
        tx.send(3);
        assert_eq!(rx.try_recv(), Ok(Some(1)));
        assert_eq!(rx.try_recv(), Ok(Some(2)));
        assert_eq!(rx.try_recv(), Ok(Some(3)));
        assert_eq!(rx.try_recv(), Ok(None));
        drop(tx);
        assert_eq!(rx.try_recv(), Err(()));
    }

    #[test]
    fn ring_try_send_when_full() {
        // cap=2：物理环容量 = next_pow2(3)-1 = 3（≥ 请求的 2——capacity 是
        // 最小保证，2 的幂环的真实容量是 2^k-1）。写满后 try_send 返回 Err。
        let (tx, rx) = RingSender::<i32>::new(2);
        assert!(tx.try_send(1).is_ok());
        assert!(tx.try_send(2).is_ok());
        assert!(tx.try_send(3).is_ok()); // 环容量 3 ≥ 2
        assert_eq!(tx.try_send(4), Err(4)); // 满
        // 消费一个后有空位。
        assert_eq!(rx.try_recv(), Ok(Some(1)));
        assert!(tx.try_send(5).is_ok());
        drop(tx);
        assert_eq!(rx.try_recv(), Ok(Some(2)));
        assert_eq!(rx.try_recv(), Ok(Some(3)));
        assert_eq!(rx.try_recv(), Ok(Some(5)));
        assert_eq!(rx.try_recv(), Err(()));
    }

    #[test]
    fn ring_spsc_cross_thread_exact_once() {
        // 跨线程 SPSC：生产者线程写 100_000 条，消费者线程读——
        // 顺序、不丢、不重（无锁环的正确性核心）。
        const N: usize = 100_000;
        let (tx, rx) = RingSender::<usize>::new(64);
        let producer = std::thread::spawn(move || {
            for i in 0..N {
                tx.send(i);
            }
        });
        let consumer = std::thread::spawn(move || {
            let mut received = Vec::with_capacity(N);
            while received.len() < N {
                if let Some(v) = rx.recv() {
                    received.push(v);
                }
            }
            received
        });
        producer.join().expect("producer");
        let received = consumer.join().expect("consumer");
        assert_eq!(received.len(), N);
        for (i, v) in received.iter().enumerate() {
            assert_eq!(*v, i, "SPSC 必须按序、不丢、不重，index {i}");
        }
    }

    #[test]
    fn ring_receiver_sees_disconnect_after_sender_drop() {
        // 生产者离开 → 消费者 recv 空时 None（级联停机信号）。
        let (tx, rx) = RingSender::<i32>::new(4);
        tx.send(42);
        drop(tx);
        assert_eq!(rx.recv(), Some(42));
        assert_eq!(rx.recv(), None);
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

// ════════════════════════════════════════════════════════════════════════
// SPSC 无锁环形队列（`LinkKind::CasFreeRing` 的物理载体）
// ════════════════════════════════════════════════════════════════════════
//
// # 物理过程（原子 / 屏障 / 缓存行）
//
// 单生产者单消费者（SPSC）无锁环——**不需要 CAS**：
//
// - `tail` 索引只有生产者写、消费者读（Acquire）；`head` 索引只有
//   消费者写、生产者读（Release）——每个索引是单写者，无竞争。
// - 内存序：生产者先写槽位数据、再 `tail.store(Release)`；消费者
//   `tail.load(Acquire)` 后再读槽位——Release/Acquire 配对保证"数据
//   先于索引可见"。head 同理反向。
// - 伪共享防护：`head` 与 `tail` 用 `#[repr(align(64))]` 分开——两个核
//   各自写自己的缓存行，互不驱逐。
// - 容量取 2 的幂（掩码 `&` 代替 `%`）；`capacity + 1` 哨兵槽区分
//   空（head == tail）与满（head == tail + 1）。
// - 满/空时**自旋 + 让出**（yield_now）——阻塞语义，不烧核。
//
// # 安全不变量
//
// `RingInner` 含 `UnsafeCell`（槽位数据）与 `AtomicUsize` 索引；SPSC
// 前提（sender/receiver 各在**单个**线程使用）由 runtime 的
// thread-per-machine 结构保证。`unsafe impl Send/Sync` 以该不变量为
// 前提——违反它（同一 sender 被两线程并发 send）是使用方错误。

struct RingInner<T> {
    /// 消费者索引：消费者独占写（Release），生产者只读（Acquire）。
    /// 独立缓存行（伪共享防护：消费者核只写它，不驱逐生产者的 tail 行）。
    head: HeadSlot,
    /// 生产者索引：生产者独占写（Release），消费者只读（Acquire）。
    tail: TailSlot,
    /// 环形槽位（len = next_pow2(capacity + 1)，哨兵槽在内）。
    buf: Box<[std::cell::UnsafeCell<std::mem::MaybeUninit<T>>]>,
    /// 容量掩码（2 的幂 - 1）。
    mask: usize,
    /// 存活生产者计数：归零 → recv 空时断开（级联停机）。
    senders: std::sync::atomic::AtomicUsize,
}

/// 独立缓存行的消费者索引（只被消费者写）。
#[repr(align(64))]
struct HeadSlot(std::sync::atomic::AtomicUsize);

/// 独立缓存行的生产者索引（只被生产者写）。
#[repr(align(64))]
struct TailSlot(std::sync::atomic::AtomicUsize);

unsafe impl<T: Send> Send for RingInner<T> {}
unsafe impl<T: Send> Sync for RingInner<T> {}

impl<T> RingInner<T> {
    fn new(capacity: usize) -> Self {
        // 2 的幂容量 + 哨兵槽（有效容量 = capacity）。
        let slots = (capacity + 1).max(2).next_power_of_two();
        let mut buf = Vec::with_capacity(slots);
        for _ in 0..slots {
            buf.push(std::cell::UnsafeCell::new(std::mem::MaybeUninit::uninit()));
        }
        RingInner {
            head: HeadSlot(std::sync::atomic::AtomicUsize::new(0)),
            tail: TailSlot(std::sync::atomic::AtomicUsize::new(0)),
            buf: buf.into_boxed_slice(),
            mask: slots - 1,
            senders: std::sync::atomic::AtomicUsize::new(1),
        }
    }

    /// 可用的空位数量（生产者视角；head 是 Acquire 读——见多写少）。
    fn free(&self) -> usize {
        let head = self.head.0.load(std::sync::atomic::Ordering::Acquire);
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Relaxed);
        // 哨兵槽保证：tail + slots > head 时未满；空闲 = (head + slots - tail - 1) & mask
        (head + self.buf.len() - tail - 1) & self.mask
    }

    /// 队列中元素数（消费者视角）。
    #[allow(dead_code)] // 诊断/测试用
    fn len(&self) -> usize {
        let head = self.head.0.load(std::sync::atomic::Ordering::Relaxed);
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Acquire);
        (tail - head) & self.mask
    }

    fn push(&self, value: T) {
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Relaxed);
        let idx = tail & self.mask;
        // SAFETY: 单生产者——该槽位要么未初始化、要么已被消费（head 越过）。
        unsafe {
            (*self.buf[idx].get()).write(value);
        }
        // Release：槽位数据先于 tail 递增对所有消费者可见。
        self.tail
            .0
            .store(tail.wrapping_add(1), std::sync::atomic::Ordering::Release);
    }

    fn try_pop(&self) -> Option<T> {
        let head = self.head.0.load(std::sync::atomic::Ordering::Relaxed);
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Acquire);
        if head == tail {
            return None; // 空
        }
        let idx = head & self.mask;
        // SAFETY: 单消费者——head 未越过 tail，该槽位有有效数据。
        let value = unsafe { (*self.buf[idx].get()).assume_init_read() };
        // Release：数据读取后 head 递增，生产者可见空位。
        self.head
            .0
            .store(head.wrapping_add(1), std::sync::atomic::Ordering::Release);
        Some(value)
    }
}

/// 无锁 SPSC 发送端（有界，满时自旋阻塞——CasFreeRing 的 Blocking 语义）。
pub(crate) struct RingSender<T>(std::sync::Arc<RingInner<T>>);

/// 无锁 SPSC 接收端。
pub(crate) struct RingReceiver<T>(std::sync::Arc<RingInner<T>>);

impl<T> RingSender<T> {
    pub(crate) fn new(capacity: usize) -> (Self, RingReceiver<T>) {
        let inner = std::sync::Arc::new(RingInner::new(capacity));
        (RingSender(inner.clone()), RingReceiver(inner))
    }

    /// 阻塞发送：满则自旋 + 让出。
    pub(crate) fn send(&self, msg: T) {
        let inner = &self.0;
        // 等空位（head 前进）。单生产者：无并发 send 竞争。
        while inner.free() == 0 {
            std::hint::spin_loop();
            std::thread::yield_now();
        }
        inner.push(msg);
    }

    /// 非阻塞发送：满返回 Err（供 Dropping 语义）。
    #[allow(dead_code)] // 未来 Dropping 语义；当前环为 Blocking 语义
    pub(crate) fn try_send(&self, msg: T) -> Result<(), T> {
        let inner = &self.0;
        if inner.free() == 0 {
            return Err(msg);
        }
        inner.push(msg);
        Ok(())
    }
}

impl<T> Drop for RingSender<T> {
    fn drop(&mut self) {
        // 最后一个生产者离开 → 消费者 recv 空时看到断开。
        if self
            .0
            .senders
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst)
            == 1
        {
            // 无等待唤醒（自旋环无需通知）；标记由 senders==0 承担。
        }
    }
}

impl<T> RingReceiver<T> {
    /// 阻塞接收：空则自旋 + 让出；生产者全离开且空 → None（断开）。
    pub(crate) fn recv(&self) -> Option<T> {
        let inner = &self.0;
        loop {
            if let Some(v) = inner.try_pop() {
                return Some(v);
            }
            // 空：若生产者已全部离开 → 断开。
            if inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                // 最后检查一次（竞态窗口：生产者可能刚 push 完正在离开）。
                if let Some(v) = inner.try_pop() {
                    return Some(v);
                }
                return None;
            }
            std::hint::spin_loop();
            std::thread::yield_now();
        }
    }

    /// 非阻塞接收。`Ok(None)` 暂时空；`Err(())` 断开。
    pub(crate) fn try_recv(&self) -> Result<Option<T>, ()> {
        let inner = &self.0;
        if let Some(v) = inner.try_pop() {
            return Ok(Some(v));
        }
        if inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
            Err(())
        } else {
            Ok(None)
        }
    }
}


