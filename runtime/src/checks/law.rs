//! 运行期律探针（六元组 T 构件深化；`debug_assertions` 门控，
//! release 零开销）。运行期律是定理（boundary-ontology §9 分层的"可判定合法性"）在
//! 运行期侧的执行：**配对律**（N 次投递 ↔ N 个区分性判定）、**序列单调律**（FIFO 序
//! 保持）、**扇出计数律**（广播 N 份源消息 ⟹ 恰好 N×fanout 条分支产出）。
//!
//! 诚实声明（A5/A6）：探针为 dev 构建的审计件（`debug_assertions`），不构成 release 承诺；
//! 违反即测试失败（外审）。

use core::cell::Cell;
use core::fmt::Debug;
use core::cmp::PartialOrd;

use crate::checks::delivery::{Delivery, Receipt};

/// 配对律探针：每次投递必得一个判定（Delivered/Full/Closed，职责守恒定）；
/// 已投递值最终被收取或随关闭排空（不静默消失）。
pub struct PairLaw {
    sends: Cell<u64>,
    verdicts: Cell<u64>,
    delivered: Cell<u64>,
    received: Cell<u64>,
}

impl PairLaw {
    /// 新建探针。
    pub const fn new() -> Self {
        PairLaw {
            sends: Cell::new(0),
            verdicts: Cell::new(0),
            delivered: Cell::new(0),
            received: Cell::new(0),
        }
    }

    /// 记录一次投递动作。
    pub fn on_send(&self) {
        self.sends.set(self.sends.get() + 1);
    }

    /// 记录一个投递判定（Delivered/Full/Closed 中的一种）。
    pub fn on_verdict(&self, d: &Delivery<impl Sized>) {
        self.verdicts.set(self.verdicts.get() + 1);
        if d.is_delivered() {
            self.delivered.set(self.delivered.get() + 1);
        }
    }

    /// 记录一次接收（Item/Empty/Closed 中的一种；Empty 不计入已收）。
    pub fn on_receive(&self, r: &Receipt<impl Sized>) {
        if matches!(r, Receipt::Item(_)) {
            self.received.set(self.received.get() + 1);
        }
    }

    /// 校验配对律（debug 构建）：N 投递 ↔ N 判定；已收 ≤ 已投。
    pub fn assert_pairing(&self) {
        debug_assert_eq!(self.sends.get(), self.verdicts.get(), "N 次投递 ↔ N 个判定");
        debug_assert!(
            self.received.get() <= self.delivered.get(),
            "已收数量不得超出已投数量"
        );
    }
}

impl Default for PairLaw {
    fn default() -> Self {
        Self::new()
    }
}

/// 序列单调律：断言 `prev ≤ next`（已排序流不得逆序；`debug_assertions` 门控）。
pub fn assert_monotonic<T: PartialOrd + Debug>(prev: &T, next: &T) {
    debug_assert!(prev <= next, "序列违反单调律: {prev:?} > {next:?}");
}

/// 扇出计数律：源消息数 × 扇出数 = 分支产出总数。
pub fn assert_fanout(total_out: u64, sources: u64, fanout: u64) {
    debug_assert_eq!(
        total_out,
        sources * fanout,
        "广播扇出计数违反: {total_out} != {sources} × {fanout}"
    );
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::checks::delivery::{Delivery, Receipt};

    #[test]
    fn pairing_law_holds_for_verdicts() {
        let law = PairLaw::new();
        law.on_send();
        law.on_verdict(&Delivery::Delivered::<i32>);
        law.on_send();
        law.on_verdict(&Delivery::Full(9));
        law.on_receive(&Receipt::Item(3));
        law.assert_pairing(); // 2 投递 ↔ 2 判定;received(1) ≤ delivered(1)
    }

    #[test]
    fn monotonic_and_fanout() {
        assert_monotonic(&1, &2);
        assert_monotonic(&2, &2);
        assert_fanout(6, 3, 2);
    }
}