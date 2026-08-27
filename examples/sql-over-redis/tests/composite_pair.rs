//! SQL-over-Redis 组合对拍（跨层用例测试面）。
//!
//! - 路由分派 / 组合语义 / 错误为值：sync 单测；
//! - T6 对拍：有界泵 ↔ inline（Ok 通道逐行等价）与 async 馈入 ↔ inline（全行等价，
//!   `tokio` feature 门控）。

use axiom::cell_core::PortCell;
use axiom_demo_sql_over_redis::composite::{self, ComposeLine, ExecCell, Route, RouteParse};
use axiom_demo_sql_over_redis::redis_plan::{Cmd, Error, ParseErr};

#[test]
fn route_dispatches_kv_and_sql_and_errors() {
    assert_eq!(
        RouteParse::step(&mut (), "SET a 1".into()),
        Ok(Route::Kv(Cmd::Set("a".into(), 1)))
    );
    assert!(matches!(
        RouteParse::step(&mut (), "SELECT * FROM t".into()),
        Ok(Route::Sql(_))
    ));
    assert!(matches!(
        RouteParse::step(&mut (), "INSERT INTO t VALUES (1)".into()),
        Ok(Route::Sql(_))
    ));
    assert_eq!(
        RouteParse::step(&mut (), "GET".into()),
        Err(Error::Parse(ParseErr::MissingKey))
    );
}

#[test]
fn composite_honors_kv_and_sql_on_shared_state() {
    let mut st = composite::new_composite_state();
    // KV：SET / GET。
    assert_eq!(ComposeLine::step(&mut st, "SET k 7".into()), "+OK\r\n");
    assert_eq!(ComposeLine::step(&mut st, "GET k".into()), ":7\r\n");
    // SQL：CREATE → INSERT → SELECT（同一状态，行级累积）。
    let r3 = ComposeLine::step(&mut st, "CREATE TABLE users (id, val)".into());
    let r4 = ComposeLine::step(&mut st, "INSERT INTO users VALUES (1)".into());
    let _ = ComposeLine::step(&mut st, "INSERT INTO users VALUES (2)".into());
    let r6 = ComposeLine::step(&mut st, "SELECT * FROM users".into());
    assert!(r3.starts_with('+') && r3.contains("CREATE TABLE users"), "{r3}");
    assert!(r4.starts_with('+') && r4.contains("INSERT 1 row"), "{r4}");
    assert!(r6.starts_with("*2 rows:"), "{r6}");
}

#[test]
fn composite_errors_are_values_not_silent() {
    let mut st = composite::new_composite_state();
    // KV 解析错误（协议面短路）。
    assert_eq!(
        ComposeLine::step(&mut st, "GET".into()),
        "-ERR GET requires a key\r\n"
    );
    // SQL 词法错误（计算面短路）。
    assert!(ComposeLine::step(&mut st, "SELECT * FROM 'oops".into()).starts_with("-ERR"));
    // SQL 执行错误（表不存在）。
    assert!(ComposeLine::step(&mut st, "SELECT * FROM missing".into()).starts_with("-ERR"));
}

#[test]
fn pump_try_equivalence_with_inline() {
    use axiom_runtime::prelude_all::bounded_pump_try;

    let lines = composite::build_corpus(40);

    let (pump_outs, parse_errs) = bounded_pump_try::<
        RouteParse,
        ExecCell,
        Route,
        Error,
        Vec<String>,
        16,
    >(|| (), composite::new_composite_state, lines.clone());

    // 泵输出 == inline 中 RouteParse 为 Ok 的行。
    let mut expect: Vec<String> = Vec::new();
    let mut sref = composite::new_composite_state();
    for line in &lines {
        if RouteParse::step(&mut (), line.clone()).is_ok() {
            expect.push(ComposeLine::step(&mut sref, line.clone()));
        }
    }
    assert_eq!(pump_outs, expect, "泵与 inline 在 Ok 通道上逐行等价");
    let expect_errs = lines
        .iter()
        .filter(|l| RouteParse::step(&mut (), (*l).clone()).is_err())
        .count();
    assert_eq!(parse_errs, expect_errs, "解析短路计数一致");
}

#[cfg(feature = "tokio")]
mod async_pairs {
    use super::*;
    use axiom_instances::async_driver::tokio_poll_fed;
    use axiom_runtime::async_seam::{PollResult, Poller};
    use axiom_runtime::flow::drive_seq;
    use std::time::{Duration, Instant};

    fn run_pair() -> (Vec<String>, Vec<String>) {
        let lines = composite::build_corpus(30);
        let mut sst = composite::new_composite_state();
        let sync_outs: Vec<String> =
            drive_seq::<ComposeLine, String, String, Vec<String>>(&mut sst, lines.clone());

        let rt = tokio::runtime::Runtime::new().expect("multi-thread rt + time driver");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(16);
        let to_send = lines.clone();
        rt.spawn(async move {
            for l in to_send {
                let _ = tx.send(l).await;
            }
        });
        let async_outs: Vec<String> = rt.block_on(async {
            let mut p = Poller::<ComposeLine>::new(composite::new_composite_state(), None);
            let mut outs = Vec::new();
            for _ in 0..lines.len() {
                let r = tokio_poll_fed(
                    &mut p,
                    &mut rx,
                    Instant::now() + Duration::from_millis(10_000),
                    Duration::from_millis(1),
                )
                .await;
                if let PollResult::Ready(o) = r {
                    outs.push(o);
                }
            }
            outs
        });
        (sync_outs, async_outs)
    }

    #[test]
    fn async_fed_matches_sync_inline_rowwise() {
        let (sync, async_) = run_pair();
        assert_eq!(sync.len(), async_.len(), "全行数一致");
        assert_eq!(async_, sync, "T6：async 馈入与 sync inline 全行逐行等价");
    }
}