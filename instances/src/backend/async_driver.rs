//! 真异步驱动（实例层承载体）：把轮询等待点经**语言原生 `.await`** 挂进 tokio
//! reactor，不经同步 [`Executor::park`](axiom_semantics::seams::async_seam::Executor)。
//!
//! ## 诚实路径（instance-layer-design §5.3 终局）
//!
//! 抽演已判死同步桥：syncc `park` 内 `block_on(tokio::time::sleep)` 连试三形态
//! （current-thread `enable_time` / 多线程 `Runtime::new` / 多线程 `Builder::enable_time`）
//! 均报 **「there is no reactor running」**——同步签名把等待挂不进 tokio 定时器。
//! 真接入因此不在 `Executor` 契约层妥协（扩契约需 §4.3 破坏性许可），而在 **adapter
//! 侧以 async worker 形态**落地：`poll()`/`roll()`（语义层 async_seam 已公开的单步
//! 入口）保持不变，等待点由本模块 [`tokio_poll_until`]/[`tokio_roll_until`] 用
//! `tokio::time::sleep(tick).await` 兑现。sleep 在**运行中的 tokio 运行时内被 await**，
//! 即有 reactor 驱动——这正是同步 `park` 内缺少的上下文。
//!
//! **不扩 [`Executor`] 契约、不改语义层**（additive；`poll`/`roll` 已是公开入口）：
//! 非破坏，无需 §4.3 许可。同步 [`Executor`](axiom_semantics::seams::async_seam::Executor)
//! 插座仍保留（它兑现 trait 化的可替换等待点，`ThreadExec`=sleep / `TokioExec`=占位）；
//! 本模块是**语言原生的异步路径**，二者互补、均可审计，不互相冒充。
//!
//! ## Timeout 升模态（D2 承载域）
//!
//! D2（async-seam.md）钦定：Timeout 现为模态④ 声明，**升 ②③ 的域仅在异步接缝内**
//! （join/timer/select）。本模块即该承载域：
//! - 同步域 [`poll_until`](axiom_semantics::seams::async_seam::Poller::poll_until) 的
//!   `TimedOut` 是**墙钟轮询近似**（`thread::sleep(tick)` 让步、按拍次采墙钟判定）；
//! - 本模块的 `TimedOut` 由**真定时器**（tokio time driver）驱动——运行期**可测、
//!   可记账**，是 Timeout 升 ③（投递态可验证）的机制地面。
//!
//! **不冒充②/不越权**：② 是编译期见证、账本行升 ②③ 属语义层 `obligation.rs` 的
//! 权威变更（LEDGER 不可替换面），不在本步骤内做——本模块只提供"期限从声明变可测"
//! 的机制地面，账本升级留作后续权威变更。
//!
//! ## await ↔ sync 行级等价（T6 / 多物理实现语义等价）
//!
//! 同一 `Poller` / 同一 `std::time::Instant` deadline 下，`tokio_poll_until` 与同步
//! [`poll_until`](axiom_semantics::seams::async_seam::Poller::poll_until)（`ThreadExec` 语义）
//! 裁决一致（同输入同期限 → 同 verdict）：本模块的等价样本以同输入同期限对拍。
//!
//! ## Send 与运行时组合
//!
//! `tokio_poll_until(&mut p)` 的 future 仅借用 `&mut p`：`p`（含 `A::State` 与
//! `A::In`/`A::Out`）为 `Send` 时整体 `Send`，可在 `enable_time` 的**多线程**运行时内
//! `tokio::spawn`——await 驱动在跨线程 reactor 下组合成立（同步 `park` 桥做不到）。
//! 期限以 `std::time::Instant` 断言（与 sync 驱动同墙钟，等价对拍同刻度），等待经
//! tokio time。
//!
//! ## 通道馈入（步骤二）
//!
//! [`tokio_poll_fed`] 在等待窗内经 `rx.recv().await` 索取输入（挂 reactor），收到即
//! 经 `Poller::put`（语义层 additive 入口）注入并同步 `step`——"输入在等待期间异步
//! 抵达"的使能面；通道关闭后按 `tick` 让步至期限（不忙循环）。综合用例
//! （`examples/sql-over-redis`）的异步变体即以此把命令序列在等待窗内喂入。

use axiom::cell_core::PortCell;
use axiom_semantics::seams::async_seam::{Poll, PollResult, Poller, SeamPoller, SeamRoll};
use std::time::{Duration, Instant};

/// 异步带期限轮询（真定时器驱动）：无输入 → `tokio::time::sleep(tick).await`，
/// deadline 前反复尝试；输入就绪 → `Ready`；到期仍未就绪 → `TimedOut`。
///
/// 语义与同步 [`poll_until`](axiom_semantics::seams::async_seam::Poller::poll_until) 一致
/// （同墙钟 deadline、同 verdict），但等待点挂进 tokio reactor（TimeOT 升模态承载域，
/// 见模块文档）。未就绪时让出给运行时——**不阻塞 runtime 线程**（区别于 sync
/// `thread::sleep` / EX `park` 的线程级等待）。
pub async fn tokio_poll_until<A: PortCell>(
    poller: &mut Poller<A>,
    deadline: Instant,
    tick: Duration,
) -> PollResult<A::Out> {
    loop {
        match poller.poll() {
            Poll::Ready(o) => return PollResult::Ready(o),
            Poll::Pending => {
                if Instant::now() >= deadline {
                    return PollResult::TimedOut;
                }
                tokio::time::sleep(tick).await; // 等待点：真定时器（无 reactor 即挂不上）
            }
        }
    }
}

/// 异步带期限轮询（背压等待点，真定时器驱动）：`Idle`/`Blocked` 在期限内反复尝试、
/// 经 `tokio::time::sleep(tick).await` 让出；消费侧腾位后滞留值自动重投；到期 →
/// `TimedOut`。
///
/// 承 D2 的第三类等待点（期限 + 背压）进异步域——与同步
/// [`roll_until`](axiom_semantics::seams::async_seam::SeamPoller::roll_until) 同 verdict，
/// 等待点挂 tokio reactor。
pub async fn tokio_roll_until<A>(
    poller: &mut SeamPoller<A>,
    deadline: Instant,
    tick: Duration,
) -> PollResult<SeamRoll<A::Out>>
where
    A: PortCell,
    A::Out: Send,
{
    loop {
        match poller.roll() {
            SeamRoll::Idle | SeamRoll::Blocked => {
                if Instant::now() >= deadline {
                    return PollResult::TimedOut;
                }
                tokio::time::sleep(tick).await;
            }
            other => return PollResult::Ready(other),
        }
    }
}

/// 异步带期限轮询 + **通道馈入**（步骤二）：无预置输入时经 `rx.recv().await` 挂进
/// reactor 等待输入，收到即 `put` 注入并同步 `step` → `Ready`；期限由**真定时器**
/// 驱动（`tokio::time::timeout(剩余期限, recv)`——Timeout 升模态的承载，见模块
/// 文档）——`recv` 不返回不阻塞期限判定；通道关闭后按 `tick` 让步至期限。
pub async fn tokio_poll_fed<A: PortCell>(
    poller: &mut Poller<A>,
    rx: &mut tokio::sync::mpsc::Receiver<A::In>,
    deadline: Instant,
    tick: Duration,
) -> PollResult<A::Out>
where
    A::In: Send,
{
    loop {
        if let Poll::Ready(o) = poller.poll() {
            return PollResult::Ready(o);
        }
        let now = Instant::now();
        if now >= deadline {
            return PollResult::TimedOut;
        }
        match tokio::time::timeout(deadline - now, rx.recv()).await {
            Ok(Some(input)) => poller.put(input),
            Ok(None) => tokio::time::sleep(tick).await, // 通道关闭：让步至期限（不忙循环）
            Err(_elapsed) => return PollResult::TimedOut, // 真定时器到点（期限驱动）
        }
    }
}

/// 便利（示例/对拍）：current-thread 运行时（`enable_time`），`block_on` 接受非 Send。
pub fn block_on_current<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("build current-thread rt with time driver")
        .block_on(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_semantics::movers::carrier::SaturationPolicy;
    use std::sync::mpsc::sync_channel;

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

    // ── await ↔ sync 行级等价（T6）──────────────────────────────────────

    #[test]
    fn ready_path_matches_sync() {
        // 同输入 Some(5)：sync poll_until（ThreadExec）与 tokio_poll_until 均 → Ready(6)。
        let mut sp = Poller::<Inc>::new((), Some(5));
        let s = sp.poll_until(Instant::now() + Duration::from_millis(300), Duration::from_millis(1));
        let a = block_on_current(async {
            let mut ap = Poller::<Inc>::new((), Some(5));
            tokio_poll_until(&mut ap, Instant::now() + Duration::from_millis(300), Duration::from_millis(1)).await
        });
        assert_eq!(s, PollResult::Ready(6));
        assert_eq!(a, PollResult::Ready(6));
        assert_eq!(a, s, "就绪输入 → 同步与异步同裁决");
    }

    #[test]
    fn timeout_path_matches_sync() {
        // 无输入、同期限：sync 与 tokio_poll_until 均 → TimedOut（墙钟 vs 真定时器，verdict 一致）。
        let mut sp = Poller::<Inc>::new((), None);
        let s = sp.poll_until(Instant::now() + Duration::from_millis(20), Duration::from_millis(1));
        let a = block_on_current(async {
            let mut ap = Poller::<Inc>::new((), None);
            tokio_poll_until(&mut ap, Instant::now() + Duration::from_millis(20), Duration::from_millis(1)).await
        });
        assert_eq!(s, PollResult::TimedOut);
        assert_eq!(a, PollResult::TimedOut);
        assert_eq!(a, s, "期限耗尽 → 同步与异步同裁决");
    }

    #[test]
    fn timeout_is_measured_not_just_declared() {
        // Timeout ②③ 承载域：真定时器产出的 TimedOut 是"测得的"（③ 可测地面），
        // 到点即应下（经实时 TestClock 下限），而非永不等。下限放宽防 flaky。
        let t0 = Instant::now();
        let r = block_on_current(async {
            let mut ap = Poller::<Inc>::new((), None);
            tokio_poll_until(&mut ap, Instant::now() + Duration::from_millis(40), Duration::from_millis(5)).await
        });
        let elapsed = t0.elapsed();
        assert_eq!(r, PollResult::TimedOut);
        // 真定时器应在 40ms 及随后的 tick 内产出：给出 [40ms, ~10×] 松界。
        assert!(elapsed >= Duration::from_millis(40), "真定时器到点下（测得的期限）：{elapsed:?}");
        assert!(elapsed < Duration::from_millis(1500), "不无限挂起：{elapsed:?}");
    }

    // ── 背压等待点进异步域（tokio_roll_until）────────────────────────────

    #[test]
    fn roll_waits_for_space_async_and_matches_sync() {
        // Block + 期限：占满通道 → tokio_roll_until 经真定时器等待 → 消费侧（独立线程）
        // 腾位 → Ready(Accepted)。同步 roll_until 同输入 → 同 verdict。
        let (tx, rx) = sync_channel::<i32>(1);
        tx.try_send(99).expect("占满容量 1");
        let drain = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            let _ = rx.recv(); // 腾位
            std::thread::sleep(Duration::from_millis(100)); // 保持 rx 存活防断连
        });
        let a = block_on_current(async {
            let mut ap = SeamPoller::<Inc>::new((), Some(5), tx, SaturationPolicy::Block);
            tokio_roll_until(&mut ap, Instant::now() + Duration::from_millis(400), Duration::from_millis(5)).await
        });
        drain.join().expect("drain");
        assert_eq!(a, PollResult::Ready(SeamRoll::Accepted));

        // sync 侧（ThreadExec）同输入同期限 → 同 verdict：
        let (tx2, rx2) = sync_channel::<i32>(1);
        tx2.try_send(99).expect("占满容量 1");
        let drain2 = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            let _ = rx2.recv();
            std::thread::sleep(Duration::from_millis(100));
        });
        let mut sp = SeamPoller::<Inc>::new((), Some(5), tx2, SaturationPolicy::Block);
        let s = sp.roll_until(Instant::now() + Duration::from_millis(400), Duration::from_millis(5));
        drain2.join().expect("drain");
        assert_eq!(s, PollResult::Ready(SeamRoll::Accepted));
    }

    // ── 多线程运行时组合（同步 park 桥不可能，await 驱动可行）──────────────

    #[test]
    fn awaiting_on_multi_thread_rt_composes() {
        // `&mut Poller<Inc>` future 是 Send：在多线程 Runtime::new（enable_time）内
        // `tokio::spawn` 组合。这证明 await 驱动把 sleep 挂进了跨线程 reactor——
        // 同步 `park` 内 block_on(sleep) 会报 no-reactor，此为判死路径的对立证据。
        let rt = tokio::runtime::Runtime::new().expect("multi-thread rt with time driver");
        let out = rt.block_on(async {
            let handle = tokio::spawn(async {
                let mut p = Poller::<Inc>::new((), Some(9));
                tokio_poll_until(&mut p, Instant::now() + Duration::from_millis(200), Duration::from_millis(5)).await
            });
            handle.await.expect("spawned task")
        });
        assert_eq!(out, PollResult::Ready(10));
    }

    // ── 通道馈入（步骤二）：输入在等待期间异步抵达 ────────────────────────

    #[test]
    fn fed_ready_when_input_arrives_within_deadline() {
        // 无预置输入 → rx.recv().await 挂 reactor → 15ms 后发送 41 → Ready(42)。
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(4);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("rt");
        rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            let _ = tx.send(41).await;
        });
        let r = rt.block_on(async {
            let mut p = Poller::<Inc>::new((), None);
            tokio_poll_fed(
                &mut p,
                &mut rx,
                Instant::now() + Duration::from_millis(300),
                Duration::from_millis(1),
            )
            .await
        });
        assert_eq!(r, PollResult::Ready(42), "馈入 41 → Inc(+1) = 42");
    }

    #[test]
    fn fed_times_out_without_any_input() {
        // 无发送、期限 30ms：真定时器到点 → TimedOut（测得，非墙钟近似）。
        let (_tx, mut rx) = tokio::sync::mpsc::channel::<i32>(4);
        let t0 = Instant::now();
        let r = block_on_current(async {
            let mut p = Poller::<Inc>::new((), None);
            tokio_poll_fed(
                &mut p,
                &mut rx,
                Instant::now() + Duration::from_millis(30),
                Duration::from_millis(2),
            )
            .await
        });
        assert_eq!(r, PollResult::TimedOut, "期限耗尽");
        assert!(t0.elapsed() >= Duration::from_millis(30), "真定时器下限");
    }

    #[test]
    fn fed_seeded_pending_wins_before_channel() {
        // 预置输入优先：Poller::new(Some(7)) 首拍即 Ready(8)，不触碰通道。
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(4);
        let _ = tx.try_send(99); // 通道中有一条不会被消费的输入
        let r = block_on_current(async {
            let mut p = Poller::<Inc>::new((), Some(7));
            tokio_poll_fed(
                &mut p,
                &mut rx,
                Instant::now() + Duration::from_millis(100),
                Duration::from_millis(1),
            )
            .await
        });
        assert_eq!(r, PollResult::Ready(8), "预置 7 优先，不触碰通道");
    }

    #[test]
    fn fed_equivalence_with_sync_poll_until() {
        // T6：通道馈入序列 vs 同步逐条 poll_until——同输入序列 → 同裁决序列。
        let input = [1, 2, 3, 42];
        let mut sync_outs: Vec<PollResult<i32>> = Vec::new();
        for x in input {
            let mut p = Poller::<Inc>::new((), Some(x));
            sync_outs.push(p.poll_until(
                Instant::now() + Duration::from_millis(100),
                Duration::from_millis(1),
            ));
        }
        let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(4);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("rt");
        rt.spawn(async move {
            for x in input {
                let _ = tx.send(x).await;
            }
        });
        let mut async_outs: Vec<PollResult<i32>> = Vec::new();
        rt.block_on(async {
            let mut p = Poller::<Inc>::new((), None);
            for _ in 0..input.len() {
                let r = tokio_poll_fed(
                    &mut p,
                    &mut rx,
                    Instant::now() + Duration::from_millis(300),
                    Duration::from_millis(1),
                )
                .await;
                async_outs.push(r);
            }
        });
        assert_eq!(async_outs, sync_outs, "同输入序列 → 同裁决序列（T6 馈入对拍）");
    }
}