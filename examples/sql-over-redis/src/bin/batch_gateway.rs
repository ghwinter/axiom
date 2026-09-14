//! 批处理网关演示（综合实例·堆叠演示）：事件接缝 × call/response 关联 × 观测。
//!
//! 运行：`cargo run -p axiom-demo-sql-over-redis --bin batch_gateway [corpus]`
//!
//! 一条命令流（字节块 → 行）经事件接缝进入网关：每行登记为一次在途调用（带期限），
//! 投递给唯一执行线程（共享组合状态，FIFO）；应答带注入路由时延返回，完成次序 ≠
//! 提交次序（慢路由必超时出账、迟到响应被吸收）。收集循环经 `CallDispatch` 关联归位
//! 并观测全程计数。四个接缝堆叠在同一用例上，每层不变量各自成立（堆叠不语义爆炸）。

use axiom_demo_sql_over_redis::composite;
use axiom_demo_sql_over_redis::gateway::{print_report, route_for, run_gateway};

fn main() {
    let n = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(48);
    let lines = composite::build_corpus(n);
    let slow = (0..lines.len()).filter(|&i| i % 4 == 0).count();
    println!("=== 批处理网关演示（corpus={}，慢路由 {slow} 条） ===", lines.len());

    let report = run_gateway(&lines, 16, std::time::Duration::from_millis(2));
    print_report("summary", &report);

    // 慢/快路由计划展示（确定性前提）。
    println!("\n      路由计划（确定性）：");
    println!("        慢路由（行 ≡ 0 mod 4）: 期限 {}ms < 时延 {}ms → 必超时出账",
        route_for(0).timeout_ms, route_for(0).delay_ms);
    println!("        快路由（其余）       : 期限 {}ms >> 时延 {}ms → 必归位",
        route_for(1).timeout_ms, route_for(1).delay_ms);
    println!(
        "\n解读: 字节流经事件接缝进入因果流；每行携带独立期限在途（CallDispatch 关联）;\n\
         \x20    执行线程 FIFO 共享组合状态（响应即使超时，服务端已按序执行）;\n\
         \x20    路由时延使完成次序 ≠ 提交次序 → 关联表不可退化为平凡（callresp 二期接线）;\n\
         \x20    收集循环 settle 优先 → 慢调用必判超时、迟到响应被吸收（不双出账）。\n\
         \x20    四个接缝（事件/callresp/观测/组合）堆叠，各层不变量互不干扰。"
    );
}
