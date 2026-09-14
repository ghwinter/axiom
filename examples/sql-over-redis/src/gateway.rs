//! 批处理网关（综合实例·堆叠演示）：事件接缝 × call/response 关联 × 观测。
//!
//! 把一个真实系统场景的三个接缝堆叠在同一用例上——"往上堆叠功能不语义爆炸"的实证：
//!
//! ```text
//! 字节流 ─块─▶ ChunkSource（行分割）─行─▶ pump_events（分派线程，路由打标）
//!   事件接缝（§9.3）                              │ (行, 路由) 无界投递（快速完成）
//!                                                 ▼
//!   收集主线程（CallDispatch 单属主）：登记在途调用（期限 = now + timeout）
//!       → 有界通道投递执行线程（背压） ────────────────────────────┐
//!       │  settle(now) 期限清扫 ◀── complete(id, resp) ◀── outbox ◀┘
//!   执行线程（唯一：共享组合状态，FIFO）─ComposeLine step─▶ 应答
//!       ─sleep(路由时延，注入)─▶ outbox（返回次序 ≠ 提交次序）
//! ```
//!
//! 每调用恰一条判定（Ok / TimedOut）；超时出账后的迟到响应被吸收（不双出账）。
//!
//! 堆叠判据（每层不变量互不干扰，测试面逐一断言）：
//! - **事件接缝配对律**：`delivered + dropped = 拉取总数`（泵不静默丢、断连即拆除）；
//! - **callresp 每调用恰一条判定**：`settled_ok + timed_out = 提交数`；超时出账后
//!   迟到的响应被吸收进 `late_dropped`（不双出账）——[`callresp`](crate::callresp) 的
//!   **二期真实接线**：路由时延使完成次序 ≠ 提交次序，关联表不可退化为平凡
//!   （若按"第 k 条响应 = 第 k 条请求"假设，慢调用的响应会被错配到错误期限）；
//! - **执行 FIFO**：共享组合状态逐行执行，与 inline 执行一致（响应即使超时，服务端
//!   已按序执行——超时是客户侧判定，不是服务端丢弃）；
//! - **观测**：投递 / 归位 / 超时 / 迟到吸收 / 应答 ok-err 全程计数（[`GatewayReport`]）。
//!
//! 确定性：路由时延是数据（按行号注入）；期限/时延取宽裕 margin，判定不依赖计时器
//! 粒度（慢路由 `delay > timeout` → 必超时、必迟到；快路由 `timeout >> delay` → 必归位）。
//! 并发形态下 margin 是局部的（相对各自提交时刻），与语料规模无关。

use std::io::Cursor;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use axiom::cell_core::PortCell;
use axiom_semantics::seams::event::{
    ChunkSource, EventPumpStats, PushVerdict, pump_events, split_lines,
};

use crate::callresp::{CallDispatch, CallId, CallResult, CompleteOutcome};
use crate::composite::{self, ComposeLine};

/// 路由计划：每条命令的期限（`timeout_ms`）与返回时延（`delay_ms`），按行号注入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteDelay {
    pub timeout_ms: u64,
    pub delay_ms: u64,
}

/// 确定性路由：`i % 4 == 0` 走慢路由（时延 > 期限 → 必超时出账、迟到被吸收）；
/// 其余走快路由（期限 >> 时延 → 必归位）。宽裕 margin：判据不依赖计时器粒度。
pub fn route_for(i: usize) -> RouteDelay {
    if i.is_multiple_of(4) {
        RouteDelay {
            timeout_ms: 10,
            delay_ms: 20,
        }
    } else {
        RouteDelay {
            timeout_ms: 1000,
            delay_ms: 1,
        }
    }
}

/// 分派格（事件接缝的下游）：行 → 路由打标。`Out = (命令, 路由)`——
/// 关联登记（`CallDispatch::submit`）发生在收集主线程（单属主），不在本 cell。
pub struct DispatchCell;

impl PortCell for DispatchCell {
    type In = String;
    type Out = (String, RouteDelay);
    type State = usize; // 行号（路由按行号注入）

    fn step(st: &mut usize, line: String) -> (String, RouteDelay) {
        let route = route_for(*st);
        *st += 1;
        (line, route)
    }
}

/// 网关报告（观测账：事件接缝 + callresp + 应答分类 + 乱序返回序）。
#[derive(Debug, Clone, Default)]
pub struct GatewayReport {
    /// 事件接缝泵账（配对律：`delivered + dropped = 拉取总数`）。
    pub pump: EventPumpStats,
    /// callresp：期限内归位的调用数（每调用恰一条判定之一）。
    pub settled_ok: usize,
    /// callresp：期限耗尽出账的调用数。
    pub timed_out: usize,
    /// callresp：超时出账后迟到响应被吸收数（不双出账）。
    pub late_dropped: usize,
    /// callresp：伪造 `CallId` 的 complete（协议违例；正确接线下恒为 0）。
    pub spurious: usize,
    /// 观测：归位应答中 ok（非 `-ERR`）数。
    pub ok_resp: usize,
    /// 观测：归位应答中 err（`-ERR`）数。
    pub err_resp: usize,
    /// 乱序返回序（执行 FIFO：到达序 == 提交序；关联经 `CallId`）。
    pub outbox_seq: Vec<(CallId, String)>,
}

impl GatewayReport {
    /// callresp 配对律：判定之和 = 提交数。
    pub fn submitted(&self) -> usize {
        self.settled_ok + self.timed_out
    }
}

/// 执行线程：唯一持有共享组合状态，FIFO 执行每条命令，按注入时延返回应答。
/// `outbox_tx` 断开 = 全部应答已发出（收集循环的完成信号）。
fn store_worker(
    jobs: Receiver<(CallId, String, RouteDelay)>,
    outbox_tx: Sender<(CallId, String)>,
) {
    let mut store = composite::new_composite_state();
    for (id, line, route) in jobs {
        let resp = ComposeLine::step(&mut store, line); // FIFO 共享状态
        thread::sleep(Duration::from_millis(route.delay_ms)); // 路由时延（注入）
        if outbox_tx.send((id, resp)).is_err() {
            break; // 网关已撤
        }
    }
}

/// 运行网关：事件接缝（分派线程）→ 收集主线程登记/清扫 → 执行线程 FIFO。
///
/// `jobs_cap`：投递有界通道容量（背压）；`tick`：收集循环的让步节拍。
pub fn run_gateway(lines: &[String], jobs_cap: usize, tick: Duration) -> GatewayReport {
    // 执行线程先起（消费即来即走）。
    let (jobs_tx, jobs_rx) = sync_channel::<(CallId, String, RouteDelay)>(jobs_cap);
    let (outbox_tx, outbox_rx) = channel::<(CallId, String)>();
    let worker = thread::spawn(move || store_worker(jobs_rx, outbox_tx));

    // 事件接缝（分派线程）：字节流 → 行（跨块分割）→ 路由打标 → 无界投递。
    // 无界投递使分派不依赖执行吞吐（并发形态：提交边进行，期限从各自提交时刻起算）。
    let (dispatch_tx, dispatch_rx) = channel::<(String, RouteDelay)>();
    let mut blob: Vec<u8> = Vec::new();
    for line in lines {
        blob.extend_from_slice(line.as_bytes());
        blob.push(b'\n');
    }
    let pump_thread = thread::spawn(move || {
        let mut source = ChunkSource::<Cursor<&[u8]>, _, String, String, 512>::new(
            Cursor::new(blob.as_slice()),
            String::new(),
            |buf: &mut String, chunk: &[u8]| split_lines(buf, chunk),
        );
        let pump = pump_events::<DispatchCell, _, _>(&mut 0usize, &mut source, |(line, route)| {
            if dispatch_tx.send((line, route)).is_err() {
                PushVerdict::Closed // 拆除：泵停止拉取（不静默延续）
            } else {
                PushVerdict::Delivered
            }
        });
        drop(dispatch_tx); // 投递完成：收集主线程据此判定"无新登记"
        pump
    });

    // 收集主线程（CallDispatch 单属主）：登记新调用 → 期限清扫 → 迟到吸收 → 出账。
    // settle 先于 drain：慢调用即使响应已到、只要期限已过即判超时（确定性不依赖调度）。
    let mut calls = CallDispatch::new();
    let mut report = GatewayReport::default();
    let mut dispatch_done = false;
    let mut outbox_done = false;
    let mut jobs_tx = Some(jobs_tx);
    loop {
        let now = Instant::now();
        // 登记新调用（事件接缝 → 在途调用 → 投递执行线程；有界通道满则阻塞 = 背压）。
        loop {
            match dispatch_rx.try_recv() {
                Ok((line, route)) => {
                    let deadline = now + Duration::from_millis(route.timeout_ms);
                    let (id, _) = calls.submit(deadline, now);
                    if let Some(tx) = &jobs_tx
                        && tx.send((id, line, route)).is_err()
                    {
                        // 执行线程已撤：该调用无响应，将由期限清扫出账（仍一条判定）。
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    dispatch_done = true;
                    break;
                }
            }
        }
        if dispatch_done {
            drop(jobs_tx.take()); // 无新登记：关闭投递 → 执行线程排空后断开 outbox（完成信号）
        }
        // 期限清扫。
        calls.settle(Instant::now());
        // 迟到响应吸收 / 归位。
        loop {
            match outbox_rx.try_recv() {
                Ok((id, resp)) => {
                    let is_err = resp.starts_with("-ERR"); // 分类在 move 前取
                    report.outbox_seq.push((id, resp.clone()));
                    match calls.complete(id, resp) {
                        CompleteOutcome::Settled => {
                            report.settled_ok += 1;
                            if is_err {
                                report.err_resp += 1;
                            } else {
                                report.ok_resp += 1;
                            }
                        }
                        CompleteOutcome::StaleDropped => report.late_dropped += 1,
                        CompleteOutcome::Spurious => report.spurious += 1,
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    outbox_done = true;
                    break;
                }
            }
        }
        // 出账消费（每调用恰一条判定）。
        for (_id, result) in calls.drain_outcomes() {
            if matches!(result, CallResult::TimedOut) {
                report.timed_out += 1;
            }
        }
        if dispatch_done && outbox_done && calls.in_flight() == 0 {
            break;
        }
        thread::sleep(tick); // 让步（非忙轮询）
    }
    report.pump = pump_thread.join().expect("dispatch thread");
    let _ = worker.join();
    report
}

/// 便捷入口：默认有界通道容量 16、让步节拍 2ms。
pub fn run_gateway_default(lines: &[String]) -> GatewayReport {
    run_gateway(lines, 16, Duration::from_millis(2))
}

/// 打印报告（输出目的地，与 `observe::print_summary` 同职责）。
pub fn print_report(tag: &str, r: &GatewayReport) {
    println!(
        "[gateway] {tag}: 投递 {delivered} / 拆除 {dropped}（配对 {total}）",
        delivered = r.pump.delivered,
        dropped = r.pump.dropped,
        total = r.pump.total()
    );
    println!(
        "[gateway] {tag}: 归位 {settled_ok} / 超时 {timed_out} / 迟到吸收 {late_dropped} / 伪造 {spurious}（判定和 {submitted}）",
        settled_ok = r.settled_ok,
        timed_out = r.timed_out,
        late_dropped = r.late_dropped,
        spurious = r.spurious,
        submitted = r.submitted()
    );
    println!(
        "[gateway] {tag}: 归位应答 ok {ok_resp} / err {err_resp}",
        ok_resp = r.ok_resp,
        err_resp = r.err_resp
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_for_is_deterministic_with_margins() {
        let slow = route_for(0);
        let fast = route_for(1);
        assert!(slow.delay_ms > slow.timeout_ms, "慢路由：时延 > 期限 → 必超时");
        assert!(fast.timeout_ms > fast.delay_ms, "快路由：期限 >> 时延 → 必归位");
        assert_eq!(route_for(4), slow, "确定性：同余类同路由");
    }

    #[test]
    fn dispatch_cell_tags_routes_in_order() {
        let mut n = 0usize;
        let (line0, r0) = DispatchCell::step(&mut n, "SET a 1".into());
        let (line1, r1) = DispatchCell::step(&mut n, "GET a".into());
        let (_, r2) = DispatchCell::step(&mut n, "INCR a".into());
        assert_eq!(line0, "SET a 1");
        assert_eq!(line1, "GET a");
        assert_eq!(r0, route_for(0), "慢路由在行 0");
        assert_eq!(r1, route_for(1), "快路由在行 1");
        assert_eq!(r2, route_for(2), "快路由在行 2");
    }
}
