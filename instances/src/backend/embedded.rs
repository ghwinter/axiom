//! 同步块环流水线（实例层 embedded 基座；`async_flow` 的单线程退化极限）。
//!
//! 把 [`async_flow`](axiom_semantics::drive::async_flow) 的「`Source ─step→ ring ─recv→ Transform ─step→ ring ─recv→ Sink`」
//! 块流水线形态，在 **单线程物理** 上兑现：交接点用
//! [`BoundedRing`](axiom_semantics::movers::ring::BoundedRing)（EmbeddedProfile 白名单存储原语）——
//! 构造一次预留、稳态每消息零分配。
//!
//! 与异步路径的分工（§11 基座优先）：并发/异步流水线的等待点（等非满 / 等新块）以
//! tokio `Notify` 承载；本模块是那个意义的**退化极限**——单线程下无并发的等待可选，
//! 背压 = 立即 `Full` 判定（[`BoundedRing::push`] 满即交还原值），泵在此让出给消费侧排空。
//! t6 多物理：缓冲不改变流经顺序（单线程排空即 FIFO），故
//! 本泵输出序列 = `Chain<A,B>` 逐输入 step 的序列——退化极限与并发形态语义一致。
//!
//! 诚实边界（A5）：稳态零分配是**结构**性质（`BoundedRing::push/pop` 无分配、驱动不新建
//! 对象），非分配计数断言；`CAP ≥ 1` 由 [`BoundedRing::new`] 的模态②门强制。本模块不提供
//! `Executor`/async 语义（无等待点、无 reactor）。

use axiom::cell_core::PortCell;
use axiom_semantics::movers::ring::{BoundedRing, Full};

/// 单线程有界块流水线泵：逐输入 `A::step` → 推入 `BoundedRing`（满→`Full` 交还），
/// 满即排空余量经 `B::step`；全部输入处理后排空并返回 `B` 输出序列。
///
/// 返回 `Vec<B::Out>` 且恰在 `BoundedRing::new()` 构造一次预留，稳态每消息零分配。
/// 输出序列（FIFO 排空）= `Chain<A,B>` 逐输入 step 序列（T6；测试 `pump_equals_chain`）。
pub fn pump<A, B, It, const CAP: usize>(inputs: It) -> Vec<B::Out>
where
    A: PortCell,
    B: PortCell<In = A::Out>,
    It: IntoIterator<Item = A::In>,
{
    let mut ring = <BoundedRing<A::Out, CAP>>::new(); // 构造一次性预留；模态② 门强制 CAP≥1
    let mut sa = A::State::default();
    let mut sb = B::State::default();
    let mut out = Vec::new();

    for input in inputs {
        let x = A::step(&mut sa, input);
        if let Err(Full(v)) = ring.push(x) {
            // 满：让出给消费侧排空（单线程退化极限的"背压"），腾位后重推。
            drain_into::<<A as PortCell>::Out, B, CAP>(&mut ring, &mut sb, &mut out);
            // 排空后必有空位（CAP≥1，drain 令 writable≥1），此刻不可失败；不用 `expect`
            // 以免给公共 API 强加 `A::Out: Debug`。
            match ring.push(v) {
                Ok(()) => {}
                Err(_) => unreachable!("排空后必有空位：CAP≥1"),
            }
        }
    }
    drain_into::<<A as PortCell>::Out, B, CAP>(&mut ring, &mut sb, &mut out);
    out
}

/// 排空环到 `B::step`，追加到输出序列。
fn drain_into<O, B, const CAP: usize>(
    ring: &mut BoundedRing<O, CAP>,
    sb: &mut B::State,
    out: &mut Vec<B::Out>,
) where
    B: PortCell<In = O>,
{
    while let Ok(v) = ring.pop() {
        out.push(B::step(sb, v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Inc1;
    impl PortCell for Inc1 {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x + 1
        }
    }

    struct Triple;
    impl PortCell for Triple {
        type In = i32;
        type Out = i32;
        type State = ();
        #[inline(always)]
        fn step(_: &mut (), x: i32) -> i32 {
            x * 3
        }
    }

    fn chain5(x: i32) -> i32 {
        Triple::step(&mut (), Inc1::step(&mut (), x))
    }

    #[test]
    fn pump_equals_chain() {
        // T6：单线程块环泵（退化极限）输出 = Chain<A,B> 逐输入 step（Inc1+1 后 Triple×3）。
        let inputs = vec![1, 2, 5, 100];
        let pumped = pump::<Inc1, Triple, _, 4>(inputs.clone());
        let chained: Vec<_> = inputs.into_iter().map(chain5).collect();
        assert_eq!(pumped, chained, "泵输出与内联链等价");
    }

    #[test]
    fn pump_exercises_backpressure_with_cap_one() {
        // CAP=1：每个输入都触发一次满→排空让出路径（背压被激活，非恒等通流）。
        let inputs = vec![3, 4, 9];
        let pumped = pump::<Inc1, Triple, _, 1>(inputs.clone());
        let chained: Vec<_> = inputs.into_iter().map(chain5).collect();
        assert_eq!(pumped, chained, "满→排空不改变 FIFO 顺序（T6 退化极限）");
    }

    #[test]
    fn pump_empty_input_yields_empty() {
        assert!(pump::<Inc1, Triple, Vec<i32>, 2>(vec![]).is_empty());
    }

    #[test]
    fn steady_state_zero_realloc_structure() {
        // 结构性质：BoundedRing 构造后 push/pop 零分配（无增长/无目标重建）。
        // 诚实：不作分配计数断言，只确认背压路径动用的是 Full 交还而非复制/扩展。
        let mut r = <BoundedRing<i32, 2>>::new();
        assert!(r.push(1).is_ok());
        assert!(r.push(2).is_ok(), "容量 2 两入可容");
        assert!(matches!(r.push(3), Err(Full(3))), "满即交还原值，不静默丢、不越界");
        assert_eq!(r.pop(), Ok(1));
    }
}