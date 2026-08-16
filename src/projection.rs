//! 可重建性契约——从事件流重建状态。
//!
//! # 定位
//!
//! `Projection` 是"可观测 ⟺ 可重建"的契约化：任何实现了 [`Projection`]
//! 的状态，都可以从事件流无副作用地重建。这是调试、审计、分叉、时间
//! 旅行、确定性回放的基础——axiom 的 `Observe` 流保证"不反向影响源"，
//! `Projection` 进一步保证"从事件流可重建"，把可观测性从哲学承诺升级为
//! 类型契约。
//!
//! 与 [`Entity`](crate::entity::Entity) 的 `checkpoint`/`restore` 互补：
//!
//! - `Projection`：**增量事件投影**，`apply(state, event)` 逐步重建；
//! - `checkpoint`：**全量快照**，`restore` 直接恢复到某时刻。
//!
//! # 为什么在 core
//!
//! 可重建性是结构层契约——它声明"状态如何从事件流派生"，与执行无关。
//! runtime 的录制/重放/分叉（`ReplayJournal`）都建立在这个契约之上。
//!
//! # 纯函数约束
//!
//! [`Projection::apply`] 必须是**纯函数**：同状态 + 同事件 → 同新状态，
//! 且无外部副作用。这是可重建性成立的前提——否则重放不可确定。

/// 可重建性契约：状态 `S` 从事件流的投影。
///
/// 实现者声明事件类型 [`Event`](Projection::Event) 与纯函数
/// [`apply`](Projection::apply)——应用一个事件，无副作用地更新状态。
/// 给定初始状态与事件序列，[`replay`] 可重建任意时刻的状态。
pub trait Projection<S> {
    /// 事件类型——append-only 事件流的元素。
    type Event;

    /// 应用一个事件，更新状态（纯函数，无副作用）。
    fn apply(state: &mut S, event: &Self::Event);
}

/// 从事件流重建状态：`fold(initial, events, apply)`。
///
/// 给定初始状态与事件序列，依次应用每个事件，返回重建后的状态。
/// 这是确定性回放、分叉、时间旅行的核心原语——同一事件流 + 同一初始
/// 状态，必定得到同一最终状态（`apply` 纯函数）。
pub fn replay<S, P: Projection<S>>(initial: S, events: &[P::Event]) -> S {
    let mut state = initial;
    for event in events {
        P::apply(&mut state, event);
    }
    state
}

// ════════════════════════════════════════════════════════════════════════════
// 单元测试
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec;

    /// 计数器投影：事件是增量，状态是累计和。
    struct Counter;
    impl Projection<i32> for Counter {
        type Event = i32;
        fn apply(state: &mut i32, event: &i32) {
            *state += event;
        }
    }

    #[test]
    fn replay_folds_events() {
        let s = replay::<i32, Counter>(0, &[1, 2, 3]);
        assert_eq!(s, 6);
    }

    #[test]
    fn replay_empty_is_initial() {
        let s = replay::<i32, Counter>(42, &[]);
        assert_eq!(s, 42);
    }

    /// 字符串拼接投影：事件是字符串片段，状态是累积文本。
    struct Concat;
    impl Projection<String> for Concat {
        type Event = String;
        fn apply(state: &mut String, event: &String) {
            state.push_str(event);
        }
    }

    #[test]
    fn replay_accumulates_string() {
        let events: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        let s = replay::<String, Concat>(String::new(), &events);
        assert_eq!(s, "abc");
    }

    #[test]
    fn replay_is_deterministic() {
        // 同一事件流 + 同一初始状态，重放两次结果一致（纯函数前提）。
        let events = [10, -3, 5];
        let a = replay::<i32, Counter>(0, &events);
        let b = replay::<i32, Counter>(0, &events);
        assert_eq!(a, b);
    }
}
