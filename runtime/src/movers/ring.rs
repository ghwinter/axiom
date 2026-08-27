//! 有界环形队列（no_std＋alloc；双计数器有界 FIFO 的机械化——C4）。
//!
//! 双计数器语义：`readable`/`writable` 两计数器，构造时
//! 初始化为 0/CAP。判满＝writable==0，判空＝readable==0，均为一次比较；下标回绕用
//! 分支而非取模。**满/空是两个不同的类型化结果**（`Full(v)` 值随错误回传 /
//! `Empty`），静默丢失被逐出（宪法 L1）。
//!
//! 与 [`crate::buffer::BoundedQueue`]（std mpsc 版）的关系：本原语是**单线程存储层**，
//! 无阻塞语义——"满时阻塞"需要生产/消费处于不同执行上下文，属 std 通道/邮箱域。
//! 单线程下背压＝立即的 `Full` 判定（调用侧选择重试/丢弃/上抛）。
//!
//! 成本声明（模态③）：构造期一次预留分配（`Vec::resize`），**稳态每消息零分配**；
//! push/pop 均为 O(1)。跨线程变体待关键节选型裁定（claims-ledger D4），本形态不承诺
//! `Sync`。

use alloc::vec::Vec;

/// 满：容量已尽，值未入队（原值随错误回传）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Full<T>(pub T);

/// 空：无值可取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Empty;

/// 有界 FIFO，容量 `CAP`（编译期常量；CAP ≥ 1 由构造点模态②门强制）。
pub struct BoundedRing<T, const CAP: usize> {
    buf: Vec<Option<T>>,
    r: usize,
    w: usize,
    readable: usize,
    writable: usize,
}

impl<T, const CAP: usize> BoundedRing<T, CAP> {
    /// 新建（模态②门：`CAP == 0` 在编译期拒绝—— rendezvous 形态不属于有界队列语域）。
    pub fn new() -> Self {
        crate::contract::assert_capacity_nonzero::<CAP>();
        let mut buf = Vec::new();
        buf.resize_with(CAP, || None);
        BoundedRing { buf, r: 0, w: 0, readable: 0, writable: CAP }
    }

    /// 容量（编译期常量）。
    pub const fn capacity(&self) -> usize {
        CAP
    }

    /// 当前可读数（O(1) 直读）。
    pub const fn readable(&self) -> usize {
        self.readable
    }

    /// 当前可写数（O(1)）。
    pub const fn writable(&self) -> usize {
        self.writable
    }

    /// 入队：满则返回 `Err(Full(v))`（值守恒）。O(1)，零分配。
    pub fn push(&mut self, v: T) -> Result<(), Full<T>> {
        if self.writable == 0 {
            return Err(Full(v));
        }
        self.buf[self.w] = Some(v);
        self.w = if self.w + 1 == CAP { 0 } else { self.w + 1 };
        self.writable -= 1;
        self.readable += 1;
        Ok(())
    }

    /// 出队：空则返回 `Err(Empty)`。O(1)，零分配。
    pub fn pop(&mut self) -> Result<T, Empty> {
        if self.readable == 0 {
            return Err(Empty);
        }
        let v = self.buf[self.r].take();
        self.r = if self.r + 1 == CAP { 0 } else { self.r + 1 };
        self.readable -= 1;
        self.writable += 1;
        match v {
            Some(v) => Ok(v),
            // 不可达：readable>0 ⟹ 槽位必有值（计数器即不变量）；防御性分支保持全函数。
            None => Err(Empty),
        }
    }

    /// 排空：按序弹出全部剩余值。
    pub fn drain(&mut self) -> Drain<'_, T, CAP> {
        Drain { ring: self }
    }
}

impl<T, const CAP: usize> Default for BoundedRing<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

/// 排空迭代器（`drain()` 的借用视图）。
pub struct Drain<'a, T, const CAP: usize> {
    ring: &'a mut BoundedRing<T, CAP>,
}

impl<T, const CAP: usize> Iterator for Drain<'_, T, CAP> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.ring.pop().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_gate_rejects_zero() {
        // 模态②：CAP=0 编译期拒绝（此处验证 CAP≥1 可构造）。
        let mut r = BoundedRing::<u8, 1>::new();
        assert_eq!(r.capacity(), 1);
        assert!(r.push(7).is_ok());
        assert!(matches!(r.push(8), Err(Full(8))));
        assert_eq!(r.pop(), Ok(7));
    }

    #[test]
    fn counter_wraparound_cap3() {
        let mut r = BoundedRing::<i32, 3>::new();
        for i in 0..3 {
            assert!(r.push(i).is_ok());
        }
        assert!(r.push(9).is_err());
        assert_eq!(r.readable(), 3);
        assert_eq!(r.writable(), 0);
        assert_eq!(r.pop(), Ok(0));
        assert!(r.push(9).is_ok(), "弹出一位后可再写入（回绕）");
        assert_eq!(r.pop(), Ok(1));
        assert_eq!(r.pop(), Ok(2));
        assert_eq!(r.pop(), Ok(9));
        assert_eq!(r.pop(), Err(Empty));
    }

    #[test]
    fn drain_yields_fifo_then_empty() {
        let mut r = BoundedRing::<&'static str, 4>::new();
        r.push("a").unwrap();
        r.push("b").unwrap();
        let drained: Vec<_> = r.drain().collect();
        assert_eq!(drained, vec!["a", "b"]);
        assert_eq!(r.pop(), Err(Empty));
        assert_eq!(r.writable(), 4);
    }

    #[test]
    fn long_run_counter_law_via_pairlaw_shape() {
        // 计数律（law 探针同款）：N 次 push 成功 ⟹ 恰 N 次可得 pop。
        let mut r = BoundedRing::<u32, 2>::new();
        let mut pushes = 0u64;
        let mut pops = 0u64;
        for i in 0..1000u32 {
            if r.push(i).is_ok() {
                pushes += 1;
            }
            if r.pop().is_ok() {
                pops += 1;
            }
        }
        assert_eq!(pops, pushes, "交替负载下配对律成立");
    }
}
