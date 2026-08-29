//! 综合用例 sync 演示：SQL-over-Redis 计划经三类物理驱动（sync 域）。
//!
//! 运行：`cargo run -p axiom-demo-sql-over-redis --bin sync_demo [corpus]`
//!
//! 演示：① inline `drive_seq` 全行（含错误行）输出；② 有界泵 `bounded_pump_try`
//! （RouteParse 短路 + 有界背压）输出 + 错误计数；③ 模态③ 装配（`assemble_link`
//! 预算校验 + `drive_link` 探针）；④ 账本摘要；⑤ **T6 断言**：泵与 inline 在共享
//! Ok 通道上逐行等价，解析短路计数一致。

use axiom::cell_core::PortCell;
use axiom_demo_sql_over_redis::composite::{self, ComposeLine, DemuxCell, ExecCell, RouteParse};
use axiom_demo_sql_over_redis::plans::redis_plan;
use axiom_semantics::drive::flow::drive_seq;
use axiom_semantics::prelude_all::{
    CarrierCost, InlineCarrier, QueueCarrier, assemble_link, bounded_pump_try,
};

fn main() {
    let n = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(120);
    let lines = composite::build_corpus(n);
    println!("=== SQL-over-Redis sync 演示（corpus={n}） ===");

    // ① inline：全行经 ComposeLine（含错误行）。
    let mut st = composite::new_composite_state();
    let inline: Vec<String> =
        drive_seq::<ComposeLine, String, String, Vec<String>>(&mut st, lines.clone());

    // ② 有界泵：RouteParse 短路 + 有界通道背压。
    let (pump_outs, parse_errs) = bounded_pump_try::<
        RouteParse,
        ExecCell,
        composite::Route,
        redis_plan::Error,
        Vec<String>,
        16,
    >(|| (), composite::new_composite_state, lines.clone());

    // ③ 模态③：装配点预算校验 → drive_link 函数指针。
    let link = assemble_link::<RouteParse, DemuxCell, InlineCarrier>(CarrierCost::ZeroAllocInline)
        .expect("Inline 满足零分配预算");
    let mut st2 = composite::new_composite_state();
    let r_set = link(&mut (), &mut st2, "SET probe 1".to_string());
    let r_sql = link(&mut (), &mut st2, "SELECT * FROM users".to_string());
    let rejected = assemble_link::<RouteParse, DemuxCell, QueueCarrier>(CarrierCost::ZeroAllocInline);
    println!(
        "      模态③: link SET probe => {r_set}  SELECT => {r_sql}  Queue 超零分配预算 = {}",
        rejected.is_err()
    );

    // ④ 账本摘要。
    let ok_ct = inline.iter().filter(|r| !r.starts_with("-ERR")).count();
    let err_ct = inline.len() - ok_ct;
    println!(
        "      账本: 总 {} → ok {ok_ct} / err {err_ct}（解析 + 存储 + SQL 错误为值）",
        inline.len()
    );
    for (line, resp) in lines.iter().zip(inline.iter()).take(8) {
        println!("        < {line:>28} => {resp}");
    }
    if lines.len() > 8 {
        println!("        … 其余 {} 条省略", lines.len() - 8);
    }

    // ⑤ T6：泵输出 == inline 中 RouteParse 为 Ok 的行（解析短路不进队列）。
    let mut expect: Vec<String> = Vec::new();
    {
        let mut sref = composite::new_composite_state();
        for line in &lines {
            if RouteParse::step(&mut (), line.clone()).is_ok() {
                expect.push(ComposeLine::step(&mut sref, line.clone()));
            }
        }
    }
    assert_eq!(pump_outs, expect, "T6：有界泵与 inline 在共享 Ok 通道上逐行等价");
    let expect_errs = lines
        .iter()
        .filter(|l| RouteParse::step(&mut (), (*l).clone()).is_err())
        .count();
    assert_eq!(parse_errs, expect_errs, "解析短路计数一致");
    assert!(r_set.starts_with("+OK"), "SET probe 应答为 +OK");

    println!("\nSQL-over-Redis sync ok: 组合核心 + 三物理驱动（T6）+ 模态③装配");
}