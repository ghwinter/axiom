//! 批处理网关·堆叠判据（跨层用例测试面）。
//!
//! 每层不变量互不干扰（"往上堆叠不语义爆炸"的逐层断言）：
//! - **事件接缝配对律**：`delivered + dropped = 拉取总数`，全投递零拆除；
//! - **callresp 每调用恰一条判定**：`settled_ok + timed_out = 提交数`；慢路由
//!   必超时出账、迟到响应必被吸收（`late_dropped == 慢路由数`），无伪造 `CallId`；
//! - **执行 FIFO**：乱序返回序 == inline 逐行输出（共享组合状态与 inline 一致——
//!   响应即使超时，服务端已按序执行）；
//! - **观测**：归位应答 ok/err 分类与 inline 对快路由行的分类一致。

use axiom_demo_sql_over_redis::composite::{self, ComposeLine};
use axiom_demo_sql_over_redis::gateway::{route_for, run_gateway};
use axiom_semantics::drive::flow::drive_seq;

fn slow_count(lines: &[String]) -> usize {
    (0..lines.len()).filter(|&i| i % 4 == 0).count()
}

#[test]
fn stacked_gateway_holds_every_layer_law() {
    let lines = composite::build_corpus(16);
    let n = lines.len();
    let report = run_gateway(&lines, 16, std::time::Duration::from_millis(2));

    // ① 事件接缝配对律：全投递、零拆除。
    assert_eq!(report.pump.delivered, n, "泵投递数 == 拉取总数");
    assert_eq!(report.pump.dropped, 0, "无拆除（执行线程存活到结束）");
    assert_eq!(report.pump.total(), n, "配对律：判定之和 = 拉取总数");

    // ② callresp：每调用恰一条判定；慢/快路由各归其位。
    let slow = slow_count(&lines);
    let fast = n - slow;
    assert_eq!(report.submitted(), n, "每调用恰一条判定（无静默丢）");
    assert_eq!(report.timed_out, slow, "慢路由必超时出账");
    assert_eq!(report.settled_ok, fast, "快路由必归位");
    assert_eq!(report.late_dropped, slow, "迟到响应必被吸收（不双出账）");
    assert_eq!(report.spurious, 0, "无伪造 CallId（接线正确）");
    assert_eq!(
        report.settled_ok + report.timed_out + report.late_dropped,
        n + slow,
        "每调用恰一条判定 + 每迟到响应一次吸收"
    );

    // ③ 执行 FIFO：乱序返回序 == inline 逐行输出（共享组合状态一致）。
    let mut sref = composite::new_composite_state();
    let inline: Vec<String> =
        drive_seq::<ComposeLine, String, String, Vec<String>>(&mut sref, lines.clone());
    let fifo: Vec<String> = report.outbox_seq.iter().map(|(_, r)| r.clone()).collect();
    assert_eq!(fifo, inline, "执行 FIFO：到达序 == inline 输出（含错误为值）");

    // ④ 观测：归位应答 ok/err 分类与 inline 对快路由行的分类一致。
    let fast_inline_ok = inline
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 4 != 0)
        .filter(|(_, r)| !r.starts_with("-ERR"))
        .count();
    let fast_inline_err = fast - fast_inline_ok;
    assert_eq!(report.ok_resp, fast_inline_ok, "归位应答 ok 分类一致");
    assert_eq!(report.err_resp, fast_inline_err, "归位应答 err 分类一致");
}

#[test]
fn route_plan_is_self_consistent() {
    // 路由计划的 margin 自洽：慢路由必超时、快路由必归位（确定性前提）。
    for i in 0..16 {
        let r = route_for(i);
        if i % 4 == 0 {
            assert!(r.delay_ms > r.timeout_ms, "慢路由 {i}: 时延 > 期限");
        } else {
            assert!(r.timeout_ms > r.delay_ms, "快路由 {i}: 期限 >> 时延");
        }
    }
}
