//! tokio 桥接执行器：把 axiom 异步接缝的等待点（[`Executor::park`]）对接 tokio。
//!
//! ## 形态与诚实边界（instance-layer-design §5.3 落地）
//!
//! axiom 的 [`Executor`] 契约是**同步签名** `park(&mut self, dur)`；而 tokio 的
//! 计时器是异步语义（`tokio::time::sleep` 需挂在 reactor 上由 runtime 驱动）。
//!
//! **实测发现（M5 展出，非静默）**：在同步 `park` 内以三种方式接入
//! `block_on(tokio::time::sleep)` —— current-thread `enable_time`、多线程
//! `Runtime::new`、多线程 `Builder::enable_time` —— 全部运行时报
//! **「there is no reactor running」**。即：同步签名无法把等待真正挂进
//! tokio 的 reactor。这是 §5.3 预先标注的开放问题，初版作诚实占位：
//!
//! - [`Executor::park`] 用线程级 [`std::thread::park_timeout`] 兑现等待；
//!   **不冒充 tokio 语义**（与 `ThreadExec`（sleep）仅在唤醒机制上略异；
//!   相同地未挂 reactor）。
//! - 真接入（把处理改成异步 `park`【破坏性，需 §4.3 契约升级】或 reactor
//!   预热方案）列为 **开放项**（internal-design §8），落地时实测。
//!
//! 本模块持 [`tokio`] 依赖（`tokio` feature）作为接入所需；当前代码**未引用**
//! 其运行时类型（诚实：接入未落地，不假用类型）。
//!
//! MSRV / 行为待实测（§8 观测缺损）：不在本文件确权。

use axiom_runtime::seams::async_seam::Executor;
use std::time::Duration;

/// tokio 桥接执行器（首版诚实占位）。
///
/// 当前 `park` 退化为线程级等待；真 tokio reactor 接入为开放项（§5.3/§8）。
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioExec;

impl TokioExec {
    /// 构造：占位执行器（无状态、无外部资源）。
    pub fn new() -> Self {
        TokioExec
    }
}

impl Executor for TokioExec {
    /// 等待点：线程级 `park_timeout`。
    ///
    /// M0 声明：此为**诚实占位**，未挂 tokio reactor（同步 `block_on(sleep)`
    /// 连试三形态均报 no reactor，见模块文档）。不冒充 tokio 期限语义；
    /// 与 `ThreadExec`（`thread::sleep`）同属线程级等待，真 tokio 接入为开放项。
    fn park(&mut self, dur: Duration) {
        std::thread::park_timeout(dur);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom::cell_core::PortCell;
    use axiom_runtime::seams::async_seam::{PollResult, Poller};
    use std::time::Instant;

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

    #[test]
    fn tokio_exec_parks_for_at_least_requested() {
        // 占位 park_timeout 至少等待 dur（诚实：非 tokio 期限，而是线程级）。
        let mut ex = TokioExec::new();
        let t0 = Instant::now();
        ex.park(Duration::from_millis(15));
        assert!(t0.elapsed() >= Duration::from_millis(10), "等待至少 ~dur");
    }

    #[test]
    fn tokio_exec_powers_wait_point_until_timeout() {
        // 占位执行器经 poll_with 跑等待点：无输入 → 期限内 Pending、期限到 TimedOut。
        let mut ex = TokioExec::new();
        let mut p = Poller::<Inc>::new((), None);
        let r = p.poll_with(
            &mut ex,
            Instant::now() + Duration::from_millis(25),
            Duration::from_millis(5),
        );
        assert_eq!(r, PollResult::TimedOut, "期限耗尽");
    }

    #[test]
    fn tokio_exec_ready_path_matches() {
        let mut ex = TokioExec::new();
        let mut p = Poller::<Inc>::new((), Some(7));
        assert_eq!(
            p.poll_with(&mut ex, Instant::now() + Duration::from_millis(100), Duration::from_millis(1)),
            PollResult::Ready(8)
        );
    }

    #[test]
    fn thread_exec_vs_tokio_exec_ready_equiv() {
        // T6 多物理实现语义等价对拍：同就绪输入下 ThreadExec 与 TokioExec
        // 的 poll_with 裁决一致（实例层替换协议 L7 的验证面；执行期证据雏形）。
        use axiom_runtime::seams::async_seam::{ThreadExec};
        let mut te = ThreadExec;
        let mut te_p = Poller::<Inc>::new((), Some(5));
        let mut tke = TokioExec::new();
        let mut tke_p = Poller::<Inc>::new((), Some(5));
        let deadline = Instant::now() + Duration::from_millis(50);
        assert_eq!(
            te_p.poll_with(&mut te, deadline, Duration::from_millis(1)),
            tke_p.poll_with(&mut tke, deadline, Duration::from_millis(1)),
            "同输入 → 同裁决（多物理实现语义等价）"
        );
    }
}