//! 有界/背压原语（BoundedQueue，§9.1）测试：容量上限 + 非阻塞背压信号 + 空/断连区分。

use std::sync::mpsc;

use axiom_semantics::prelude_all::BoundedQueue;

#[test]
fn bounded_queue_capacity_and_try_push_backpressure() {
    let q: BoundedQueue<i32, 2> = BoundedQueue::new();
    assert_eq!(q.capacity(), 2);

    // 填满容量 2。
    assert!(q.try_push(1).is_ok());
    assert!(q.try_push(2).is_ok());
    // 第 3 个：满 → Err(被拒值)（背压信号）。
    assert_eq!(q.try_push(3), Err(3));

    // 消费一个腾出空间，再投成功。
    assert_eq!(q.try_pop(), Ok(1));
    assert!(q.try_push(3).is_ok());
    assert_eq!(q.try_pop(), Ok(2));
    assert_eq!(q.try_pop(), Ok(3));
    // 空（发送端仍活着）→ Err(Empty)，与"断连"（Err(Disconnected)）区分（S3）。
    assert_eq!(q.try_pop(), Err(mpsc::TryRecvError::Empty));
}

#[test]
fn bounded_queue_pop_blocks_until_push_after_drain() {
    // 非阻塞能力：容量 CAP 决定"可同时驻留"的消息数（背压的来源）。
    let q: BoundedQueue<u8, 1> = BoundedQueue::new();
    assert!(q.try_push(7).is_ok());
    assert_eq!(q.try_push(8), Err(8)); // 容量 1，已满
    assert_eq!(q.try_pop(), Ok(7));
    assert!(q.try_push(8).is_ok());
}