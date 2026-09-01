#![cfg(feature = "tokio")]
//! T6 对拍器械（蓝图级）：同一蓝图在同步域与异步域的迹零分歧。
//!
//! 蓝图 = `Chain<Inc, Scaler>`（core 布线词汇的两段组合；状态变体
//! `Chain<Acc, Inc>`）。两个物理域按 §0.6 解散裁定定义：
//!
//! - **同步域**：合并轮询——`Poller::poll` + 通道 `recv_timeout(tick)` 递延
//!   （"编译期已知的轮询填充"的测试面实现，与 `tokio_poll_fed` 逐行同构）；
//! - **异步域**：`tokio_poll_fed`——真定时器（tokio time）+ `mpsc` 馈入
//!   （"运行期 ∃ 绑定的定时器填充"）。
//!
//! 对拍三景：全序列（Ready 迹）、期限耗尽（TimedOut 迹 + 真定时器下限）、
//! 延迟馈入（Pending→Ready 同序）。判据：同输入序列 → 同裁决序列、同输出
//! 序列（零分歧）。另含迹展开等价断言（听证 B）：组合蓝图输出 =
//! 逐段展开（`Scaler ∘ Inc` / 状态累加 `Acc`）。
//!
//! 多核面：multi-thread runtime + 跨线程馈入任务（`tokio_poll_fed` future
//! 借用 `&mut Poller`，Send 组合成立），迹仍与同步域零分歧。

use axiom::cell_core::{Chain, PortCell};
use axiom_instances::backend::async_driver::tokio_poll_fed;
use axiom_semantics::seams::async_seam::{Poll, PollResult, Poller};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

// ── 被测格（与 core/src/laws.rs 测试格同族：wrapping 算术覆盖全值域）──

struct Inc;
impl PortCell for Inc {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_add(1)
    }
}

struct Scaler;
impl PortCell for Scaler {
    type In = i32;
    type Out = i32;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), x: i32) -> i32 {
        x.wrapping_mul(2)
    }
}

/// 状态格：验证对拍对携带状态的蓝图同样成立（状态演化只经 `step`）。
struct Acc;
impl PortCell for Acc {
    type In = i32;
    type Out = i32;
    type State = i32;
    #[inline(always)]
    fn step(s: &mut i32, x: i32) -> i32 {
        *s = s.wrapping_add(x);
        *s
    }
}

type Blueprint = Chain<Inc, Scaler>;
type StatefulBlueprint = Chain<Acc, Inc>;

// ── 同步域驱动：合并轮询（与 tokio_poll_fed 逐行同构）────────────────

fn sync_poll_fed<A: PortCell>(
    p: &mut Poller<A>,
    rx: &mut std::sync::mpsc::Receiver<A::In>,
    deadline: Instant,
    tick: Duration,
) -> PollResult<A::Out> {
    loop {
        match p.poll() {
            Poll::Ready(o) => return PollResult::Ready(o),
            Poll::Pending => {
                if Instant::now() >= deadline {
                    return PollResult::TimedOut;
                }
                match rx.recv_timeout(tick) {
                    Ok(x) => p.put(x),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => std::thread::sleep(tick),
                }
            }
        }
    }
}

/// 同步域迹：每个槽位一次带期限馈入轮询。
fn sync_trace<A: PortCell>(
    slots: usize,
    rx: &mut std::sync::mpsc::Receiver<A::In>,
    deadline: Duration,
    tick: Duration,
) -> Vec<PollResult<A::Out>> {
    let mut p = Poller::<A>::new(A::State::default(), None);
    let mut trace = Vec::with_capacity(slots);
    for _ in 0..slots {
        trace.push(sync_poll_fed(&mut p, rx, Instant::now() + deadline, tick));
    }
    trace
}

// ── 异步域迹 ─────────────────────────────────────────────────────────

async fn async_trace<A: PortCell>(
    slots: usize,
    rx: &mut tokio::sync::mpsc::Receiver<A::In>,
    deadline: Duration,
    tick: Duration,
) -> Vec<PollResult<A::Out>>
where
    A::In: Send,
{
    let mut p = Poller::<A>::new(A::State::default(), None);
    let mut trace = Vec::with_capacity(slots);
    for _ in 0..slots {
        trace.push(
            tokio_poll_fed(&mut p, rx, Instant::now() + deadline, tick).await,
        );
    }
    trace
}

fn current_thread_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("current-thread rt")
}

// ── 景一：全序列（Ready 迹）＋ 迹展开等价 ────────────────────────────

#[test]
fn full_sequence_traces_match() {
    let inputs = [1i32, 2, 3, 42, -7, i32::MAX / 2];

    // 同步域：馈入线程逐条投递。
    let (tx, mut rx) = std::sync::mpsc::channel::<i32>();
    for &x in &inputs {
        tx.send(x).expect("send");
    }
    drop(tx);
    let sync = sync_trace::<Blueprint>(inputs.len(), &mut rx, Duration::from_secs(5), Duration::from_millis(1));

    // 异步域：馈入任务逐条投递。
    let rt = current_thread_rt();
    let async_ = rt.block_on(async {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(inputs.len());
        for &x in &inputs {
            tx.send(x).await.expect("send");
        }
        async_trace::<Blueprint>(inputs.len(), &mut rx, Duration::from_secs(5), Duration::from_millis(1))
            .await
    });

    // 零分歧：同输入序列 → 同裁决序列。
    assert_eq!(sync, async_, "T6 蓝图级：同步/异步裁决迹零分歧");
    // 全 Ready 且展开等价：组合输出 = Scaler(Inc(x))（听证 B 迹展开，纯段）。
    for (i, v) in sync.iter().enumerate() {
        let x = inputs[i];
        let expanded = x.wrapping_add(1).wrapping_mul(2);
        assert_eq!(*v, PollResult::Ready(expanded), "展开等价（段 {i}）");
    }
}

#[test]
fn full_sequence_traces_match_stateful() {
    // 状态蓝图：展开 = (Σx) + 1（Acc 累加后过 Inc）——状态只经 step 演化，
    // 两个域的状态演化逐拍一致。
    let inputs = [3i32, -1, 10, 0, -4];

    let (tx, mut rx) = std::sync::mpsc::channel::<i32>();
    for &x in &inputs {
        tx.send(x).expect("send");
    }
    drop(tx);
    let sync =
        sync_trace::<StatefulBlueprint>(inputs.len(), &mut rx, Duration::from_secs(5), Duration::from_millis(1));

    let rt = current_thread_rt();
    let async_ = rt.block_on(async {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(inputs.len());
        for &x in &inputs {
            tx.send(x).await.expect("send");
        }
        async_trace::<StatefulBlueprint>(inputs.len(), &mut rx, Duration::from_secs(5), Duration::from_millis(1))
            .await
    });

    assert_eq!(sync, async_, "T6 蓝图级（状态）：同步/异步裁决迹零分歧");
    let mut acc = 0i32;
    for (i, v) in sync.iter().enumerate() {
        acc = acc.wrapping_add(inputs[i]);
        assert_eq!(*v, PollResult::Ready(acc.wrapping_add(1)), "展开等价（状态，段 {i}）");
    }
}

// ── 景二：期限耗尽（TimedOut 迹 + 真定时器下限）─────────────────────

#[test]
fn timeout_traces_match_with_real_timer_floor() {
    const DEADLINE: Duration = Duration::from_millis(40);

    // 同步域：馈入端存活但悬置（通道开、无投递）——recv_timeout 递延至期限。
    let (_tx, mut rx) = std::sync::mpsc::channel::<i32>();
    let t0 = Instant::now();
    let sync = sync_trace::<Blueprint>(1, &mut rx, DEADLINE, Duration::from_millis(2));
    let sync_elapsed = t0.elapsed();

    // 异步域：同悬置形态——tokio 真定时器至期限。
    let rt = current_thread_rt();
    let t1 = Instant::now();
    let async_ = rt.block_on(async {
        let (_tx, mut rx) = tokio::sync::mpsc::channel::<i32>(1);
        async_trace::<Blueprint>(1, &mut rx, DEADLINE, Duration::from_millis(2)).await
    });
    let async_elapsed = t1.elapsed();

    assert_eq!(
        sync,
        vec![PollResult::<i32>::TimedOut],
        "同步域：期限耗尽 → TimedOut"
    );
    assert_eq!(sync, async_, "T6 期限景：同期限 → 同裁决（TimedOut 零分歧）");
    assert!(sync_elapsed >= DEADLINE, "同步域递延不早于期限");
    assert!(async_elapsed >= DEADLINE, "异步域真定时器不早于期限");
}

// ── 景三：延迟馈入（Pending→Ready 同序）─────────────────────────────

#[test]
fn delayed_feed_traces_match() {
    // 馈入节奏慢于轮询 tick：每条输入到达前两个域都先经历 Pending 间隙。
    let inputs = [5i32, -9, 100];

    // 同步域：馈入线程按 20ms 间隔投递。
    let (tx, mut rx) = std::sync::mpsc::channel::<i32>();
    let feeder = std::thread::spawn(move || {
        for x in inputs {
            std::thread::sleep(Duration::from_millis(20));
            tx.send(x).expect("send");
        }
    });
    let sync = sync_trace::<Blueprint>(inputs.len(), &mut rx, Duration::from_secs(5), Duration::from_millis(2));
    feeder.join().expect("feeder");

    // 异步域：馈入任务按 20ms 间隔投递（tokio sleep，真定时器）。
    let rt = current_thread_rt();
    let async_ = rt.block_on(async {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(1);
        let feeder = tokio::spawn(async move {
            for x in inputs {
                tokio::time::sleep(Duration::from_millis(20)).await;
                tx.send(x).await.expect("send");
            }
        });
        let trace = async_trace::<Blueprint>(
            inputs.len(),
            &mut rx,
            Duration::from_secs(5),
            Duration::from_millis(2),
        )
        .await;
        feeder.await.expect("feeder");
        trace
    });

    assert_eq!(sync, async_, "T6 延迟馈入：Pending 间隙不改变迹（零分歧）");
    for (i, v) in sync.iter().enumerate() {
        let x = inputs[i];
        assert_eq!(*v, PollResult::Ready(x.wrapping_add(1).wrapping_mul(2)));
    }
}

// ── 多核面：multi-thread runtime + 跨线程馈入 ────────────────────────

#[test]
fn multicore_runtime_trace_matches_sync() {
    let inputs = [11i32, -11, 0, i32::MIN / 2];

    let (tx, mut rx) = std::sync::mpsc::channel::<i32>();
    for &x in &inputs {
        tx.send(x).expect("send");
    }
    drop(tx);
    let sync = sync_trace::<Blueprint>(inputs.len(), &mut rx, Duration::from_secs(5), Duration::from_millis(1));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .expect("multi-thread rt");
    let async_ = rt.block_on(async {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(inputs.len());
        // 馈入任务在另一 worker 线程上执行（Send 组合：future 只借用
        // &mut rx / &mut Poller，驱动手持所有权）。
        let feeder = tokio::spawn(async move {
            for x in inputs {
                tx.send(x).await.expect("send");
            }
        });
        let trace = async_trace::<Blueprint>(
            inputs.len(),
            &mut rx,
            Duration::from_secs(5),
            Duration::from_millis(1),
        )
        .await;
        feeder.await.expect("feeder");
        trace
    });

    assert_eq!(sync, async_, "T6 多核面：跨线程馈入下迹仍零分歧");
}
