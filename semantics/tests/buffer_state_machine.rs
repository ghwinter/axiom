//! G22：`BoundedQueue` 有界 FIFO 的 proptest-state-machine 骨架（std 测试面 only）。
//!
//! 参考模型（`ReferenceStateMachine`）= `Vec<i32>` FIFO（容量 `CAP` 封顶）；
//! 被测系统（SUT）= [`BoundedQueue`]。随机 Push/TryPush/TryPop 序列下验证：
//! - **配对律**：成功入队 N 次 ⟹ 恰可取 N 次（计数守恒）；
//! - **序保持（FIFO）**：取出序 = 入队序（async_ring 序保持基声明在同步面的对拍见证）；
//! - **值守恒**：满时 `try_push` 拒绝且回传被拒值（不静默丢失）；
//! - **空 ≠ 断连**：空时 `try_pop` 得 `Err(Empty)`（本测试不断连，断连面见 buffer.rs 单元测试）。
//!
//! **std 门控注记（N-G3）**：`proptest-state-machine` 子 crate 结构性依赖
//! `Arc<AtomicUsize>`（seen-counter），仅能在 std 测试面运行——本文件置于 `tests/`
//! （dev 面，不进 lib 依赖图，零依赖哲学不破）；no_std 侧跨步律维持手写向量。

use proptest::prelude::*;
use proptest::strategy::BoxedStrategy;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};

use axiom_semantics::prelude_all::BoundedQueue;
use std::sync::mpsc::TryRecvError;

const CAP: usize = 4;

/// 抽象转移：阻塞投 / 非阻塞投 / 非阻塞取。值域 0..8（小值域制造重复值，压 FIFO 序）。
#[derive(Clone, Debug, PartialEq)]
enum Op {
    /// 阻塞 push：仅模型非满时生成（单线程测试不能真阻塞）。
    Push(i32),
    /// 非阻塞 push：满时被拒（值回传）。
    TryPush(i32),
    /// 非阻塞取：空时 `Err(Empty)`。
    TryPop,
}

/// 参考模型：FIFO 内容即全部状态。
type Model = Vec<i32>;

struct QueueModel;

impl ReferenceStateMachine for QueueModel {
    type State = Model;
    type Transition = Op;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(Vec::new()).boxed()
    }

    fn transitions(_state: &Self::State) -> BoxedStrategy<Self::Transition> {
        prop_oneof![
            2 => (0..8i32).prop_map(Op::Push),
            3 => (0..8i32).prop_map(Op::TryPush),
            2 => Just(Op::TryPop),
        ]
        .boxed()
    }

    fn apply(state: Self::State, transition: &Self::Transition) -> Self::State {
        let mut state = state;
        match transition {
            Op::Push(v) | Op::TryPush(v) => {
                if state.len() < CAP {
                    state.push(*v);
                }
            }
            Op::TryPop => {
                if !state.is_empty() {
                    state.remove(0);
                }
            }
        }
        state
    }

    fn preconditions(state: &Self::State, transition: &Self::Transition) -> bool {
        match transition {
            Op::Push(_) => state.len() < CAP, // 满 + 阻塞 push = 单线程死锁，不生成
            _ => true,
        }
    }
}

/// SUT：真实队列 + 镜像模型（postcondition 需要转移**前**的状态，
/// 而 `apply` 收到的 `ref_state` 是转移**后**的状态——镜像补齐这一差）。
struct QueueTest {
    q: BoundedQueue<i32, CAP>,
    model: Model,
}

impl StateMachineTest for QueueTest {
    type SystemUnderTest = QueueTest;
    type Reference = QueueModel;

    fn init_test(ref_state: &Model) -> Self::SystemUnderTest {
        assert!(ref_state.is_empty(), "初始参考态恒为空");
        QueueTest { q: BoundedQueue::new(), model: Vec::new() }
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        ref_state: &Model,
        transition: <Self::Reference as ReferenceStateMachine>::Transition,
    ) -> Self::SystemUnderTest {
        match transition {
            Op::Push(v) => {
                assert!(state.model.len() < CAP);
                assert_eq!(state.q.push(v), Ok(()), "非满时阻塞 push 必成功");
                state.model.push(v);
            }
            Op::TryPush(v) => {
                if state.model.len() < CAP {
                    assert_eq!(state.q.try_push(v), Ok(()), "非满时 try_push 必成功");
                    state.model.push(v);
                } else {
                    assert_eq!(state.q.try_push(v), Err(v), "满时 try_push 拒绝且回传被拒值");
                }
            }
            Op::TryPop => {
                if state.model.is_empty() {
                    assert_eq!(state.q.try_pop(), Err(TryRecvError::Empty), "空 ≠ 断连");
                } else {
                    let expected = state.model[0]; // FIFO：取头
                    assert_eq!(state.q.try_pop(), Ok(expected), "取出序 = 入队序");
                    state.model.remove(0);
                }
            }
        }
        assert_eq!(state.model, *ref_state, "镜像模型必须与参考模型同步");
        state
    }

    fn check_invariants(
        state: &Self::SystemUnderTest,
        ref_state: &Model,
    ) {
        assert_eq!(state.model, *ref_state, "镜像与参考恒同步");
        assert!(state.model.len() <= CAP, "容量上界");
    }
}

prop_state_machine! {
    #[test]
    fn bounded_queue_matches_fifo_model(sequential 1..40 => QueueTest);
}
