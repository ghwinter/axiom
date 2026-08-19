//! Link carriers for Parallel mode — the physical channel implementation is selected by `LinkKind`.
//!
//! Unifies four physical carriers — `mpsc::Sender` (unbounded), `mpsc::SyncSender`
//! (bounded blocking/dropping), the custom bounded overwrite carrier (`Overwriting`), and the
//! single-slot overwrite carrier (`Latest`/`SharedState`) — so the routing table can hold
//! heterogeneous senders.

use alloc::boxed::Box;
use alloc::string::String;

/// Message type of the Parallel-mode channel: (port_name, payload).
/// The port name travels with the message — the downstream thread uses it to inject, rather than
/// a fixed port.
pub(crate) type RoutedMsg = (String, Box<dyn core::any::Any + Send>);

/// Link carrier for Parallel mode — unifies heterogeneous senders so the routing table can hold
/// `mpsc::Sender` / `SyncSender` / custom carriers.
///
/// `send` delivers according to the carrier semantics:
/// - `Unbounded`        → `Sender::send` (never blocks);
/// - `BoundedBlocking`  → `SyncSender::send` (blocks when full, backing `WritePolicy::Blocking`);
/// - `BoundedDropping`  → `SyncSender::try_send` (drops the new message when full, backing
///   `WritePolicy::Dropping` / `Channel { drop_when_full }`);
/// - `Overwriting`      → custom bounded overwrite (overwrites the oldest when full, backing
///   `WritePolicy::Overwriting`'s native semantics);
/// - `Slot`             → single-slot overwrite (`Latest` / `SharedState`, the reader sees the
///   latest value).
///
/// A failed send (downstream disconnected) is silently dropped — cascaded shutdown is detected on
/// the receiver side; errors are not propagated upward.
pub(crate) enum ChanSender {
    Unbounded(std::sync::mpsc::Sender<RoutedMsg>),
    BoundedBlocking(std::sync::mpsc::SyncSender<RoutedMsg>),
    BoundedDropping(std::sync::mpsc::SyncSender<RoutedMsg>),
    Overwriting(OverwriteSender<RoutedMsg>),
    Slot(SlotSender<RoutedMsg>),
    /// Lock-free SPSC ring (`CasFreeRing`: bounded FIFO, spin-blocks when full).
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

/// The unified receiving end of Parallel mode — machine threads / forward threads recv from it.
/// `recv` (blocking) returning `None` means the carrier disconnected (cascaded shutdown);
/// `try_recv` (non-blocking) returning `None` means temporarily no message (for
/// `ReadPolicy::NonBlocking` polling).
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
    /// Non-blocking message fetch. `Ok(Some)` = message; `Ok(None)` = temporarily none (polling);
    /// `Err(())` = carrier disconnected (cascaded shutdown) — `NonBlocking` polling exits on it.
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

// ── Bounded overwrite carrier ────────────────────────────────────────────────────────────

/// Bounded overwrite carrier: overwrites the oldest element when full (`WritePolicy::Overwriting`'s
/// native semantics). When all senders are released it becomes closed → `recv` returns `None`
/// (cascaded shutdown).
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
            b.pop_front(); // overwrite the oldest
        }
        b.push_back(msg);
        self.0.cv.notify_one();
    }
}
impl<T> Drop for OverwriteSender<T> {
    fn drop(&mut self) {
        if self.0.senders.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
            self.0.cv.notify_all(); // last sender: wake recv to check closed
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
                return None; // disconnected: cascaded shutdown
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

// ── Single-slot overwrite carrier ───────────────────────────────────────────────────────

/// Single-slot overwrite carrier: `Latest` (single slot, overwriting write, the reader sees the
/// latest) and `SharedState` (shared state, single-consumer approximation). When all senders are
/// released it becomes closed → `recv` returns `None`.
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
        *s = Some(msg); // overwrite the old value: the reader only sees the latest
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

/// `SlotReceiver` can be multi-instantiated — multiple receivers share the same slot (the basis
/// of `SharedState`'s multi-reader semantics). recv stays "take latest" (competitive: each message
/// is consumed by one receiver); true broadcast-style multi-reader (each reader independently sees
/// the latest value) is future work, see `docs/philosophy.md`.
impl<T> Clone for SlotReceiver<T> {
    fn clone(&self) -> Self {
        SlotReceiver(self.0.clone())
    }
}

// ── Carrier selection ───────────────────────────────────────────────────────────────────

/// Select the Parallel carrier by `LinkKind`:
/// - `BoundedBuf { write_policy: Blocking }` / `Channel { !drop_when_full }`
///   → `sync_channel(capacity)` (blocks when full, natural backpressure);
/// - `BoundedBuf { write_policy: Dropping }` / `Channel { drop_when_full }`
///   → `sync_channel(capacity)` + `try_send` (drops the new message when full);
/// - `BoundedBuf { write_policy: Overwriting }` → **custom bounded overwrite carrier**
///   (overwrites the oldest when full — `Overwriting`'s native semantics);
/// - `Latest` / `SharedState` → **single-slot overwrite carrier** (the reader sees the latest value);
/// - `Inline` / `CasFreeRing` → unbounded channel (the `CasFreeRing` lock-free fixed-address
///   carrier targets embedded scenarios; the runtime migrates it to an unbounded channel —
///   noted in the docs).
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
        // Latest / SharedState: single-slot overwrite (latest-value semantics).
        LinkKind::Latest { .. } | LinkKind::SharedState => {
            let (tx, rx) = SlotSender::<RoutedMsg>::new();
            (ChanSender::Slot(tx), ChanReceiver::Slot(rx))
        }
        // CasFreeRing: a true lock-free SPSC ring (bounded FIFO, spin-blocks when full).
        LinkKind::CasFreeRing { capacity, .. } => {
            let (tx, rx) = RingSender::<RoutedMsg>::new(*capacity.max(&1));
            (ChanSender::Ring(tx), ChanReceiver::Ring(rx))
        }
        // Inline: cross-thread means a semantic migration from function call → channel (unbounded).
        _ => {
            let (tx, rx) = mpsc::channel::<RoutedMsg>();
            (ChanSender::Unbounded(tx), ChanReceiver::Mpsc(rx))
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Carrier overwrite-semantics unit tests
//
// End-to-end tests (tests.rs's runtime_*) cannot reliably trigger overwrite — Sequential's
// per-hop routing lets d2 consume before d1's next output, so the buffer does not accumulate;
// in Parallel, whether it accumulates depends on thread scheduling (Windows happens to accumulate,
// Linux preemptively consumes). Overwrite semantics are a deterministic property of the carrier
// itself, verified here directly, without going through the runtime.
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_sender_covers_oldest_when_full() {
        // cap=2: write 1,2,3 → overwrite the oldest when full (1) → recv yields 2,3.
        let (tx, rx) = OverwriteSender::<i32>::new(2);
        tx.send(1);
        tx.send(2);
        tx.send(3); // full, overwrites 1
        drop(tx);   // release the sender → recv does not block
        assert_eq!(rx.recv(), Some(2));
        assert_eq!(rx.recv(), Some(3));
        assert_eq!(rx.recv(), None); // disconnected
    }

    #[test]
    fn overwrite_sender_keeps_all_when_not_full() {
        // cap=4: write 1,2,3 → not full, no overwrite → recv yields 1,2,3.
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
        // empty → try_recv Ok(None); after dropping the sender → Err(()).
        let (tx, rx) = OverwriteSender::<i32>::new(2);
        assert_eq!(rx.try_recv(), Ok(None));
        drop(tx);
        assert_eq!(rx.try_recv(), Err(()));
    }

    // ── SPSC lock-free ring (CasFreeRing) ──

    #[test]
    fn ring_single_thread_fifo() {
        // Same thread (only verifies FIFO order and full/empty determination; true SPSC is in
        // the cross-thread test).
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
        // cap=2: the physical ring capacity = next_pow2(3)-1 = 3 (≥ the requested 2 — capacity
        // is a minimum guarantee; a power-of-two ring's real capacity is 2^k-1). After filling,
        // try_send returns Err.
        let (tx, rx) = RingSender::<i32>::new(2);
        assert!(tx.try_send(1).is_ok());
        assert!(tx.try_send(2).is_ok());
        assert!(tx.try_send(3).is_ok()); // ring capacity 3 ≥ 2
        assert_eq!(tx.try_send(4), Err(4)); // full
        // After consuming one there is space.
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
        // Cross-thread SPSC: the producer thread writes 100_000 items, the consumer thread reads —
        // in order, none lost, none duplicated (the core correctness of the lock-free ring).
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
        // The producer leaves → the consumer recv returns None when empty (cascaded-shutdown signal).
        let (tx, rx) = RingSender::<i32>::new(4);
        tx.send(42);
        drop(tx);
        assert_eq!(rx.recv(), Some(42));
        assert_eq!(rx.recv(), None);
    }

    #[test]
    fn slot_sender_overwrites_with_latest() {
        // write 1,2,3 → single-slot overwrite → recv only yields 3.
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

    #[test]
    fn slot_receiver_clone_shares_slot() {
        // Multiple receivers share the same slot (the basis of SharedState's multi-reader
        // semantics): after a send, any receiver takes the latest value and the other receivers
        // see an empty slot.
        let (tx, rx) = SlotSender::<i32>::new();
        let rx2 = rx.clone();
        tx.send(42);
        drop(tx);
        assert_eq!(rx.recv(), Some(42));
        assert_eq!(rx2.recv(), None, "same slot: value consumed by first receiver");
    }

    #[test]
    fn slot_multi_sender_multi_receiver() {
        // Multiple senders + multiple receivers share the same slot: the latest write overwrites,
        // any reader takes it.
        let (tx1, rx) = SlotSender::<i32>::new();
        let tx2 = tx1.clone();
        let rx2 = rx.clone();
        tx1.send(1);
        tx2.send(2); // overwrites 1 — the reader only sees the latest
        drop(tx1);
        drop(tx2);
        assert_eq!(rx.recv(), Some(2));
        assert_eq!(rx2.recv(), None);
    }
}

// ════════════════════════════════════════════════════════════════════════
// SPSC lock-free ring queue (the physical carrier of `LinkKind::CasFreeRing`)
// ════════════════════════════════════════════════════════════════════════
//
// # Physical mechanics (atomics / barriers / cache lines)
//
// Single-producer single-consumer (SPSC) lock-free ring — **no CAS needed**:
//
// - The `tail` index is only written by the producer and read by the consumer (Acquire); the
//   `head` index is only written by the consumer and read by the producer (Release) — each index
//   has a single writer, so there is no contention.
// - Memory ordering: the producer writes the slot data first, then `tail.store(Release)`; the
//   consumer does `tail.load(Acquire)` before reading the slot — the Release/Acquire pairing
//   guarantees "data is visible before the index". head works the same in reverse.
// - False-sharing protection: `head` and `tail` are separated by `#[repr(align(64))]` — each core
//   writes only its own cache line, without evicting the other's.
// - The capacity is a power of two (mask `&` instead of `%`); a `capacity + 1` sentinel slot
//   distinguishes empty (head == tail) from full (head == tail + 1).
// - On full/empty it **spins + yields** (yield_now) — blocking semantics without burning the core.
//
// # Safety invariants
//
// `RingInner` contains `UnsafeCell` (slot data) and `AtomicUsize` indexes; the SPSC precondition
// (the sender/receiver is each used on a **single** thread) is guaranteed by the runtime's
// thread-per-machine structure. The `unsafe impl Send/Sync` assumes that invariant — violating it
// (e.g. the same sender used concurrently by two threads) is a usage error.

struct RingInner<T> {
    /// Consumer index: written exclusively by the consumer (Release), read-only by the producer
    /// (Acquire). On its own cache line (false-sharing protection: the consumer core only writes
    /// it, never evicting the producer's tail line).
    head: HeadSlot,
    /// Producer index: written exclusively by the producer (Release), read-only by the consumer
    /// (Acquire).
    tail: TailSlot,
    /// Ring slots (len = next_pow2(capacity + 1), including the sentinel slot).
    buf: Box<[std::cell::UnsafeCell<std::mem::MaybeUninit<T>>]>,
    /// Capacity mask (power of two - 1).
    mask: usize,
    /// Live producer count: reaching zero → recv disconnects when empty (cascaded shutdown).
    senders: std::sync::atomic::AtomicUsize,
}

/// Consumer index on its own cache line (only written by the consumer).
#[repr(align(64))]
struct HeadSlot(std::sync::atomic::AtomicUsize);

/// Producer index on its own cache line (only written by the producer).
#[repr(align(64))]
struct TailSlot(std::sync::atomic::AtomicUsize);

unsafe impl<T: Send> Send for RingInner<T> {}
unsafe impl<T: Send> Sync for RingInner<T> {}

impl<T> RingInner<T> {
    fn new(capacity: usize) -> Self {
        // Power-of-two capacity + sentinel slot (effective capacity = capacity).
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

    /// Number of free slots (producer view; head is an Acquire read — sees more writes).
    fn free(&self) -> usize {
        let head = self.head.0.load(std::sync::atomic::Ordering::Acquire);
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Relaxed);
        // Sentinel slot guarantee: not full when tail + slots > head; free = (head + slots - tail - 1) & mask
        (head + self.buf.len() - tail - 1) & self.mask
    }

    /// Number of elements in the queue (consumer view).
    #[allow(dead_code)] // for diagnostics/tests
    fn len(&self) -> usize {
        let head = self.head.0.load(std::sync::atomic::Ordering::Relaxed);
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Acquire);
        (tail - head) & self.mask
    }

    fn push(&self, value: T) {
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Relaxed);
        let idx = tail & self.mask;
        // SAFETY: single producer — the slot is either uninitialized or already consumed (head passed it).
        unsafe {
            (*self.buf[idx].get()).write(value);
        }
        // Release: slot data becomes visible to all consumers before tail increments.
        self.tail
            .0
            .store(tail.wrapping_add(1), std::sync::atomic::Ordering::Release);
    }

    fn try_pop(&self) -> Option<T> {
        let head = self.head.0.load(std::sync::atomic::Ordering::Relaxed);
        let tail = self.tail.0.load(std::sync::atomic::Ordering::Acquire);
        if head == tail {
            return None; // empty
        }
        let idx = head & self.mask;
        // SAFETY: single consumer — head has not passed tail, so the slot holds valid data.
        let value = unsafe { (*self.buf[idx].get()).assume_init_read() };
        // Release: after the data is read, head increments so the producer sees the free slot.
        self.head
            .0
            .store(head.wrapping_add(1), std::sync::atomic::Ordering::Release);
        Some(value)
    }
}

/// Lock-free SPSC sending end (bounded, spin-blocks when full — CasFreeRing's Blocking semantics).
pub(crate) struct RingSender<T>(std::sync::Arc<RingInner<T>>);

/// Lock-free SPSC receiving end.
pub(crate) struct RingReceiver<T>(std::sync::Arc<RingInner<T>>);

impl<T> RingSender<T> {
    pub(crate) fn new(capacity: usize) -> (Self, RingReceiver<T>) {
        let inner = std::sync::Arc::new(RingInner::new(capacity));
        (RingSender(inner.clone()), RingReceiver(inner))
    }

    /// Blocking send: spins + yields when full.
    pub(crate) fn send(&self, msg: T) {
        let inner = &self.0;
        // Wait for a free slot (head advances). Single producer: no concurrent send contention.
        while inner.free() == 0 {
            std::hint::spin_loop();
            std::thread::yield_now();
        }
        inner.push(msg);
    }

    /// Non-blocking send: returns Err when full (for the Dropping semantics).
    #[allow(dead_code)] // future Dropping semantics; the current ring has Blocking semantics
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
        // The last producer leaves → the consumer sees the disconnect when recv'ing on empty.
        if self
            .0
            .senders
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst)
            == 1
        {
            // No need to wake (a spinning ring needs no notification); the marker is carried
            // by senders==0.
        }
    }
}

impl<T> RingReceiver<T> {
    /// Blocking receive: spins + yields when empty; all producers gone and empty → None (disconnected).
    pub(crate) fn recv(&self) -> Option<T> {
        let inner = &self.0;
        loop {
            if let Some(v) = inner.try_pop() {
                return Some(v);
            }
            // Empty: if all producers have left → disconnected.
            if inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                // Check once more (race window: a producer may have just pushed and be leaving).
                if let Some(v) = inner.try_pop() {
                    return Some(v);
                }
                return None;
            }
            std::hint::spin_loop();
            std::thread::yield_now();
        }
    }

    /// Non-blocking receive. `Ok(None)` = temporarily empty; `Err(())` = disconnected.
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
