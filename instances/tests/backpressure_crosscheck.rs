#![cfg(feature = "tokio")]
//! T6 背压对拍器械：同一有界背压蓝图在同步域与异步域的迹零分歧 + 值保全。
//!
//! 蓝图 = `Inc`（`i32 → i32`，`+1`）经有界背压链（`CAP = 2`，小容量使 Block
//! 真实发生）。两个物理域：
//!
//! - **同步域**：`SeamPoller`（[`SaturationPolicy::Block`]，有界 `sync_channel`
//!   投递）——背压等待点的同步域机械：饱和 → 值滞留 `held`，腾位后自动重投
//!   （`roll_with`/`roll_until` 家族；此处经 `roll` + 馈入驱动）；
//! - **异步域**：`TokioBlockRing`（tokio `Notify` 双唤醒，等非满/等新块）——
//!   背压等待点的异步域机械（`AsyncBlockRing` 契约）。
//!
//! 判据：同输入序列 → 同步域从有界通道收集的输出序 == 异步域 `recv` 序；
//! 每块 = `x_i + 1`（展开等价）；输出长度 == 输入长度（**值保全**：Block 不丢值）。
//!
//! **边界声明**：两域"等待是否真实发生"不同步（同步 `sleep`/`recv_timeout`
//! 递延 vs 异步 notify），故**只比输出序与值保全，不比 Blocked 次数/等待时长**——
//! 本测试不做计时断言（避免被误读为计时契约）。
//!
//! 关联：`semantics/movers/async_ring.rs` 序保持由对拍见证（R004）；本文件补齐
//! `SeamPoller`（同步背压等待点）侧——`t6_crosscheck` 只对拍了 `Poller` 的
//! 输入就绪/期限两类等待点。

use axiom::cell_core::PortCell;
use axiom_instances::backend::async_ring::TokioBlockRing;
use axiom_semantics::movers::async_ring::AsyncBlockRing;
use axiom_semantics::movers::carrier::SaturationPolicy;
use axiom_semantics::seams::async_seam::{PollResult, SeamPoller, SeamRoll};
use std::time::{Duration, Instant};

const CAP: usize = 2;

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

// ── 同步域驱动：SeamPoller（Block）＋ 馈入 ────────────────────────────

/// 单槽带期限驱动：`Idle` 时经输入通道 `recv_timeout` 馈入并投递；`Blocked`
/// 只等消费侧腾位（held 滞留值自动重投），**不喂入**——避免 `put` 覆盖尚未
/// 处理的 pending 输入（值保全）。返回该输入的投递裁决。
fn sync_roll_fed(
    p: &mut SeamPoller<Inc>,
    feed: &mut std::sync::mpsc::Receiver<i32>,
    deadline: Instant,
    tick: Duration,
) -> PollResult<SeamRoll<i32>> {
    loop {
        match p.roll() {
            SeamRoll::Idle => {
                if Instant::now() >= deadline {
                    return PollResult::TimedOut;
                }
                match feed.recv_timeout(tick) {
                    Ok(x) => p.put(x),
                    Err(_) => {} // timeout / disconnected：继续等
                }
            }
            SeamRoll::Blocked => {
                if Instant::now() >= deadline {
                    return PollResult::TimedOut;
                }
                std::thread::sleep(tick); // 只等腾位，不喂入（held 值保留，重投后腾出）
            }
            other => return PollResult::Ready(other),
        }
    }
}

/// 同步域迹：每输入一槽驱动；输出由消费线程从有界通道收集（腾位使 Block 成立）。
fn sync_trace(inputs: &[i32]) -> Vec<i32> {
    let (feed_tx, mut feed_rx) = std::sync::mpsc::channel::<i32>();
    for &x in inputs {
        feed_tx.send(x).expect("feed");
    }
    drop(feed_tx);

    let (tx, out_rx) = std::sync::mpsc::sync_channel::<i32>(CAP);
    let mut p = SeamPoller::<Inc>::new((), None, tx, SaturationPolicy::Block);

    // 消费线程：无限收输出（腾位，使 Block 真实发生）；`drop(p)` 断开 tx 后收尾。
    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::<i32>::new()));
    let consumer = {
        let collected = collected.clone();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            while let Ok(v) = out_rx.recv() {
                out.push(v);
            }
            *collected.lock().unwrap() = out;
        })
    };

    // 每输入一槽：驱动至该输入投递（Block 下滞留值随腾位重投，终达 Accepted）。
    for _ in inputs {
        match sync_roll_fed(
            &mut p,
            &mut feed_rx,
            Instant::now() + Duration::from_secs(5),
            Duration::from_millis(2),
        ) {
            PollResult::Ready(SeamRoll::Accepted) => {}
            other => panic!("同步域应终达 Accepted（Block 值保全），got {other:?}"),
        }
    }

    drop(p); // tx 断开 → 消费线程收尾
    consumer.join().expect("consumer");
    collected.lock().unwrap().clone()
}

// ── 异步域：TokioBlockRing（AsyncBlockRing 契约）───────────────────────

async fn async_trace(inputs: &[i32]) -> Vec<i32> {
    let owned = inputs.to_vec(); // tokio::spawn 需 'static
    let ring = TokioBlockRing::<i32>::new(CAP).into_shared();
    let producer_ring = ring.clone();
    let producer_inputs = owned.clone(); // 独立副本：主任务仍需 owned.len() 定收尾
    // 生产任务：逐块 send（满则等非满——背压等待点挂 tokio reactor）。
    let producer = tokio::spawn(async move {
        for &x in &producer_inputs {
            producer_ring
                .send(x.wrapping_add(1))
                .await
                .expect("send：环未关闭");
        }
    });
    // 主任务：逐块 recv（空则等新块）。
    let mut out = Vec::with_capacity(owned.len());
    while out.len() < owned.len() {
        match ring.recv().await {
            Some(v) => out.push(v),
            None => panic!("环提前关闭（值丢失）"),
        }
    }
    producer.await.expect("producer");
    out
}

fn current_thread_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("current-thread rt")
}

// ── 景一：全序列——输出序零分歧 + 值保全 ──────────────────────────────

#[test]
fn backpressure_traces_match_and_preserve_all_values() {
    // 输入 > CAP：Block 必然真实发生（同步域滞留重投 / 异步域 send 等非满）。
    let inputs = [1i32, 2, 3, 42, -7, i32::MAX / 2, 100];

    let sync = sync_trace(&inputs);

    let rt = current_thread_rt();
    let async_ = rt.block_on(async_trace(&inputs));

    assert_eq!(sync, async_, "T6 背压：同步/异步输出序零分歧");
    assert_eq!(sync.len(), inputs.len(), "值保全：Block 不丢值（无静默丢失，L1）");
    for (i, v) in sync.iter().enumerate() {
        let x = inputs[i];
        assert_eq!(*v, x.wrapping_add(1), "展开等价（段 {i}）：Inc(+1)");
    }
}

// ── 景二：更长序列（多重 Block 往返）─── ─────────────────────────────

#[test]
fn longer_sequence_multiple_block_roundtrips() {
    // 40 个输入 × CAP=2：Block→腾位→重投 多次往返，序与值保全仍成立。
    let inputs: Vec<i32> = (0..40).map(|i| i * 3 - 17).collect();

    let sync = sync_trace(&inputs);

    let rt = current_thread_rt();
    let async_ = rt.block_on(async_trace(&inputs));

    assert_eq!(sync, async_, "T6 背压（长序列）：多次 Block 往返下迹仍零分歧");
    assert_eq!(sync.len(), inputs.len(), "长序列值保全");
    for (i, v) in sync.iter().enumerate() {
        assert_eq!(*v, inputs[i].wrapping_add(1), "展开等价（段 {i}）");
    }
}
