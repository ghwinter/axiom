//! Timeout ②③ 演示：真异步驱动下，轮询等待点经语言原生 `.await` 挂进 tokio reactor，
//! 使期限从"同步轮询域的墙钟近似（④ 邻）"升为"真定时器驱动的事件化期限（③ 可测）"。
//!
//! # 演示点
//!
//! 1. **真异步落地**：在同一运行中的 tokio 运行时内，`await` 驱动的
//!    [`tokio_poll_until`] 让 `tokio::time::sleep(tick).await` 真正挂上 reactor——
//!    tokio 定时器被 runtime 驱动。这正是同步 `park` 内 `block_on(tokio::time::sleep)`
//!    连试三形态都报 **「there is no reactor running」** 的判死路径的对立证据：
//!    真接入不在 `Executor` 契约层妥协（扩契约需 §4.3 破坏性许可），而在 adapter 侧
//!    用 async worker 兑现。
//! 2. **Timeout 升模态承载域**：`TimedOut` 由真定时器产出，运行期**测得的**（投递态可
//!    验证/可记账的机制地面）——非"声称超时却无定时器"的退化态（第五轴，命题 2.7）。
//!    不冒充②/不越权：② 是编译期见证，账本行升 ②③ 属 runtime `obligation.rs` 的
//!    权威变更（LEDGER 不可替换面），不在本演示内做。
//! 3. **多线程运行时组合**：`tokio_poll_until(&mut p)` 在 `Poller` 为 `Send` 时整体
//!    `Send`，可 `tokio::spawn` 到多线程 reactor——await 驱动在跨线程组合成立。
//!
//! # 运行
//!
//! ```text
//! cargo run -p axiom-instances --features tokio --example tokio_timeout_async
//! ```

fn main() {
    // 多线程运行时兼 time driver（`tokio` feature 提供 rt-multi-thread + time）。
    // 真实逻辑（引用 feature 门控的 async_driver 与 tokio）收进 run_tokio()。
    #[cfg(feature = "tokio")]
    run_tokio();
    #[cfg(not(feature = "tokio"))]
    eprintln!("skip: requires `tokio` feature; run with `--features tokio`");
}

/// 真异步演示（[`tokio_poll_until`] 驱动）。
#[cfg(feature = "tokio")]
fn run_tokio() {
    use axiom::cell_core::PortCell;
    use axiom_instances::backend::async_driver::tokio_poll_until;
    use axiom_semantics::seams::async_seam::{PollResult, Poller};
    use std::time::{Duration, Instant};

    /// 单步 cell：`x -> x+1`（同步、全函数；`step` 永不等，等待只在边界投递）。
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

    let rt = tokio::runtime::Runtime::new().expect("build multi-thread rt with time driver");

    rt.block_on(async {
        // 1) 就绪路径 → Ready（await 驱动同步完成 step，等待点不触发）。
        let mut p = Poller::<Inc>::new((), Some(5));
        let r = tokio_poll_until(&mut p, Instant::now() + Duration::from_millis(300), Duration::from_millis(10)).await;
        assert_eq!(r, PollResult::Ready(6), "5 → Inc(+1) = 6");
        println!("ready: {r:?}");

        // 2) Timeout ②③ 承载域：真定时器产出的 TimedOut，测得的期限（③ 可测地面）。
        let t0 = Instant::now();
        let mut p2 = Poller::<Inc>::new((), None); // 输入永不就绪
        let r2 = tokio_poll_until(&mut p2, Instant::now() + Duration::from_millis(50), Duration::from_millis(5)).await;
        let elapsed = t0.elapsed();
        assert_eq!(r2, PollResult::TimedOut, "期限耗尽 → TimedOut");
        assert!(elapsed >= Duration::from_millis(50), "真定时器到点下（测得的）：{elapsed:?}");
        println!("timeout after {elapsed:?}: {r2:?}  ← 真定时器产出、运行期可测（③ 承载域）");

        // 3) 多线程 `tokio::spawn` 组合：await 驱动的 future 在跨线程 reactor 成立。
        //    同步 `park` 桥（block_on(sleep)）在此报 no-reactor——判死路径的对立证据。
        let handle = tokio::spawn(async {
            let mut p3 = Poller::<Inc>::new((), Some(9));
            tokio_poll_until(&mut p3, Instant::now() + Duration::from_millis(200), Duration::from_millis(5)).await
        });
        let r3 = handle.await.expect("spawned task");
        assert_eq!(r3, PollResult::Ready(10), "9 → Inc(+1) = 10（跨线程 reactor 内 await 驱动）");
        println!("spawned-on-multi-thread: {r3:?}  ← sync park 桥不可能，await 驱动可行");
    });
}