//! 有界邮箱（actix 型反饥饿背压；义务类·资源/投递态轴的实例；runtime-constitution 阶段 3）。
//!
//! 语义（对标 actix 邮箱,以 axiom 词汇陈述）：
//! - **容量** = `CAP`（缓冲槽）+ 每生产者 1 个**保底席位**（`parked`）——一个生产者
//!   无法饿死其他生产者：满时先占自己的席位,永不占用他人；
//! - **三投递模式**：
//!   - `try_send`：非阻塞,仅用缓冲槽,满即 `Delivery::Full(v)`（值回传,不消失）；
//!   - `send`：阻塞背压,缓冲满时占保底席位后等待消费端腾空,断连返回 `Err(v)`；
//!   - `fire`：尽力投递,先用缓冲槽,再用保底席位,两处皆满才返回 `Full(v)`（不阻塞）；
//! - **关闭**：`close()` 后：已入队/入席的值仍可被消费（drain 语义）,之后 `recv` 得
//!   `Closed`,投递得 `Closed(v)`（值回传）；
//! - **下单者公平**：队列 FIFO;席位按生产者独立,消费端先缓冲后席位（轮转）。
//!
//! 模态② 门：`CAP ≥ 1` 由 [`assert_capacity_nonzero`](crate::contract::assert_capacity_nonzero)
//! 在构造点强制（`CAP = 0` 为 rendezvous,拒绝）。
//!
//! 诚实声明（A5）：本邮箱为安全的标准库组合实现（`Mutex`+`Condvar`,零 unsafe）;
//! 投递状态经 [`Delivery`](crate::delivery::Delivery) 显式分类（Full/Closed 机械化）。
//! 每生产者席位 = 1（资源义务:总容量 = CAP + 生产者数,文档化声明）。

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use crate::delivery::Delivery;

struct Inner<T> {
    queue: VecDeque<T>,
    parked: Vec<Option<T>>,
    next_producer: usize,
    closed: bool,
}

/// 有界邮箱：`CAP` 缓冲槽 + 每生产者保底席位。可克隆（内部 `Arc`）。
pub struct BoundedMailbox<T, const CAP: usize> {
    inner: Arc<Mutex<Inner<T>>>,
    cond: Arc<Condvar>,
}

impl<T, const CAP: usize> Clone for BoundedMailbox<T, CAP> {
    fn clone(&self) -> Self {
        BoundedMailbox {
            inner: Arc::clone(&self.inner),
            cond: Arc::clone(&self.cond),
        }
    }
}

/// 生产者句柄：持有自己的保底席位（id 索引）。
pub struct Producer<T, const CAP: usize> {
    id: usize,
    mailbox: BoundedMailbox<T, CAP>,
}

impl<T, const CAP: usize> BoundedMailbox<T, CAP> {
    /// 新建邮箱。模态②：`CAP ≥ 1` 编译期强制。
    pub fn new() -> Self {
        const { crate::contract::assert_capacity_nonzero::<CAP>() };
        BoundedMailbox {
            inner: Arc::new(Mutex::new(Inner {
                queue: VecDeque::new(),
                parked: Vec::new(),
                next_producer: 0,
                closed: false,
            })),
            cond: Arc::new(Condvar::new()),
        }
    }

    /// 注册一个生产者（分配保底席位;总容量义务 = CAP + 生产者数）。
    pub fn producer(&self) -> Producer<T, CAP> {
        let mut g = self.inner.lock().unwrap();
        let id = g.next_producer;
        g.next_producer += 1;
        g.parked.push(None);
        drop(g);
        Producer {
            id,
            mailbox: self.clone(),
        }
    }

    /// 消费一条（**阻塞**）：先缓冲（FIFO），再轮转席位；关闭且排空 → `Closed`。
    /// 不返回 `Empty`（`Empty` 属非阻塞 [`try_recv`](Self::try_recv) 语义）。
    pub fn recv(&self) -> crate::delivery::Receipt<T> {
        let mut g = self.inner.lock().unwrap();
        loop {
            if let Some(item) = g.queue.pop_front() {
                self.cond.notify_all(); // 缓冲腾出:唤醒阻塞投递者
                return crate::delivery::Receipt::Item(item);
            }
            if let Some(i) = g.parked.iter().position(Option::is_some) {
                let item = g.parked[i].take().expect("position is_some");
                self.cond.notify_all();
                return crate::delivery::Receipt::Item(item);
            }
            if g.closed {
                return crate::delivery::Receipt::Closed;
            }
            g = self.cond.wait(g).unwrap();
        }
    }

    /// 非阻塞消费：当时为空 → `Empty`（可重试）；排空且关闭 → `Closed`。
    pub fn try_recv(&self) -> crate::delivery::Receipt<T> {
        let mut g = self.inner.lock().unwrap();
        if let Some(item) = g.queue.pop_front() {
            self.cond.notify_all();
            crate::delivery::Receipt::Item(item)
        } else if let Some(i) = g.parked.iter().position(Option::is_some) {
            let item = g.parked[i].take().expect("position is_some");
            self.cond.notify_all();
            crate::delivery::Receipt::Item(item)
        } else if g.closed {
            crate::delivery::Receipt::Closed
        } else {
            crate::delivery::Receipt::Empty
        }
    }

    /// 关闭：已入队/入席的值仍可消费（drain）,之后投递得 `Closed(v)`、消费得 `Closed`。
    pub fn close(&self) {
        let mut g = self.inner.lock().unwrap();
        g.closed = true;
        self.cond.notify_all();
    }

    /// 是否已关闭。
    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }
}

impl<T, const CAP: usize> Default for BoundedMailbox<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const CAP: usize> Producer<T, CAP> {
    /// 非阻塞投递：仅用缓冲槽,满 → `Full(v)`;断连 → `Closed(v)`（值均回传）。
    pub fn try_send(&self, v: T) -> Delivery<T> {
        let mut g = self.mailbox.inner.lock().unwrap();
        if g.closed {
            return Delivery::Closed(v);
        }
        if g.queue.len() < CAP {
            g.queue.push_back(v);
            self.mailbox.cond.notify_one();
            Delivery::Delivered
        } else {
            Delivery::Full(v)
        }
    }

    /// 阻塞投递（背压）：缓冲满则占保底席位并等待消费端腾空;断连 → `Err(v)`（值回传）。
    pub fn send(&self, v: T) -> Result<(), T> {
        let mut g = self.mailbox.inner.lock().unwrap();
        loop {
            if g.closed {
                return Err(v);
            }
            if g.queue.len() < CAP {
                g.queue.push_back(v);
                self.mailbox.cond.notify_one();
                return Ok(());
            }
            if g.parked[self.id].is_none() {
                g.parked[self.id] = Some(v);
                self.mailbox.cond.notify_all(); // 唤醒可能阻塞的消费端
                while g.parked[self.id].is_some() && !g.closed {
                    g = self.mailbox.cond.wait(g).unwrap();
                }
                // 退出条件二选一:席位被消费端取走(drain)或邮箱关闭。
                return match g.parked[self.id].take() {
                    Some(v) => Err(v), // 关闭时撤回,值回传
                    None => Ok(()),    // 已 drain
                };
            }
            // 自身席位被占（异常路径）：等待缓冲腾出。
            while g.queue.len() >= CAP && g.parked[self.id].is_some() && !g.closed {
                g = self.mailbox.cond.wait(g).unwrap();
            }
        }
    }

    /// 尽力投递（不阻塞）：缓冲槽 → 保底席位,两处皆满才 `Full(v)`。
    pub fn fire(&self, v: T) -> Delivery<T> {
        let mut g = self.mailbox.inner.lock().unwrap();
        if g.closed {
            return Delivery::Closed(v);
        }
        if g.queue.len() < CAP {
            g.queue.push_back(v);
            self.mailbox.cond.notify_one();
            Delivery::Delivered
        } else if g.parked[self.id].is_none() {
            g.parked[self.id] = Some(v);
            self.mailbox.cond.notify_all(); // 席位可见:唤醒可能阻塞的消费端
            Delivery::Delivered
        } else {
            Delivery::Full(v)
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn capacity_semantics_include_per_producer_slots() {
        // CAP=2,双生产者:无阻塞可容纳 = 2 缓冲 + 2 席位 = 4。
        let mb = BoundedMailbox::<i32, 2>::new();
        let p1 = mb.producer();
        let p2 = mb.producer();
        for i in 0..2 {
            assert_eq!(p1.try_send(i), Delivery::Delivered);
        }
        // 缓冲已满:p1 的 try_send 严格 → Full;fire 用保底席位 → Delivered。
        assert!(matches!(p1.try_send(9), Delivery::Full(9)));
        assert_eq!(p1.fire(10), Delivery::Delivered);
        // p2 的 fire 亦可用自己的席位(反饥饿:不被 p1 的席位挤占)。
        assert_eq!(p2.fire(20), Delivery::Delivered);
        // p2 的 try_send 仍严格:队列满 → Full。
        assert!(matches!(p2.try_send(21), Delivery::Full(21)));
        // 消费:先缓冲(FIFO)后席位。
        let mut got = Vec::new();
        for _ in 0..4 {
            got.push(mb.recv());
        }
        let items: Vec<i32> = got
            .into_iter()
            .map(|r| match r {
                crate::delivery::Receipt::Item(v) => v,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(items, vec![0, 1, 10, 20], "FIFO 缓冲优先,席位随后");
    }

    #[test]
    fn send_blocks_until_consumer_drains() {
        // 阻塞背压须由并发消费端释放:send 在 CAP 满时挂起,消费后恢复。
        let mb = BoundedMailbox::<i32, 1>::new();
        let p = mb.producer();
        let handle = std::thread::spawn(move || {
            p.send(1).expect("not closed");
            p.send(2).expect("not closed: 队列满 → 占席位 → 等待消费");
        });
        assert_eq!(mb.recv(), crate::delivery::Receipt::Item(1));
        assert_eq!(mb.recv(), crate::delivery::Receipt::Item(2));
        handle.join().unwrap();
        // 阻塞 recv 不返回 Empty:空态由 try_recv 观察。
        assert_eq!(mb.try_recv(), crate::delivery::Receipt::Empty);
    }

    #[test]
    fn close_drains_then_rejects_with_value() {
        let mb = BoundedMailbox::<i32, 2>::new();
        let p = mb.producer();
        assert_eq!(p.send(1), Ok(()));
        mb.close();
        // 已入队值仍可消费(drain)。
        assert_eq!(mb.recv(), crate::delivery::Receipt::Item(1));
        // 排空后:消费 Closed,投递 Closed(v)。
        assert_eq!(mb.recv(), crate::delivery::Receipt::Closed);
        assert_eq!(p.try_send(7), Delivery::Closed(7));
        assert_eq!(p.send(8), Err(8));
    }

    #[test]
    fn cross_thread_backpressure_roundtrip() {
        // 跨线程:生产者线程 send 100 条(CAP=4),主线程消费;全员到达、顺序保持。
        let mb = BoundedMailbox::<i32, 4>::new();
        let p = mb.producer();
        let ptx = p;
        let handle = std::thread::spawn(move || {
            for i in 0..100 {
                ptx.send(i).expect("not closed");
            }
        });
        let mut items = Vec::new();
        for _ in 0..100 {
            items.push(mb.recv());
        }
        handle.join().unwrap();
        let vals: Vec<i32> = items
            .into_iter()
            .map(|r| match r {
                crate::delivery::Receipt::Item(v) => v,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(vals, (0..100).collect::<Vec<i32>>());
    }
}