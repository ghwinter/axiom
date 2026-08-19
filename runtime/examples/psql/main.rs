//! psql —— 用 cell_core 四构件 + runtime Carrier 构建的 SQL REPL 流水线。
//!
//! 重建旧 psql（Lexer → Parser → Executor），用新核心表达：三者都是 `PortCell`，
//! 差异在 `State`（Lexer/Parser 无状态、Executor 有 Database）。用 runtime Carrier
//! 驱动（InlineCarrier 单线程零分配），体现实解析/执行流水线作为真实用例。
//!
//! 运行：`cargo run --example psql`

mod cells;

use axiom::cell_core::PortCell;
use axiom_runtime::carrier::{InlineCarrier, spawned_flow};
use axiom_runtime::flow::drive_link;

use cells::{Executor, Lexer, Parser, Result};

fn main() {
    println!("=== psql: cell_core + Carrier SQL REPL 流水线 ===\n");

    // 一批 SQL（模拟 REPL 输入）。
    let sqls = [
        "CREATE TABLE users (id, name)",
        "INSERT INTO users VALUES (1)",
        "INSERT INTO users VALUES (2)",
        "INSERT INTO users VALUES (42)",
        "SELECT * FROM users",
        "SELECT * FROM missing",
    ];

    // 皮层状态（Lexer/Parser 无状态 = ()；Executor 有 Database）。
    let mut lex_state = <Lexer as PortCell>::State::default();
    let mut par_state = <Parser as PortCell>::State::default();
    let mut exe_state = <Executor as PortCell>::State::default();

    for sql in sqls {
        let toks = Lexer::step(&mut lex_state, sql.to_string());
        let stmt = Parser::step(&mut par_state, toks);
        let res = Executor::step(&mut exe_state, stmt);
        match &res {
            Result::Ok(msg) => println!("  {sql:<32} => {msg}"),
            Result::Rows(rows) => println!("  {sql:<32} => {:?}", rows),
            Result::Error(e) => println!("  {sql:<32} => ERR {e}"),
        }
    }

    // ── 用 runtime Carrier 驱动一条皮层对（Lexer -> Parser）作为链路 ──
    // 展示 drive_link + InlineCarrier（零分配单线程）。
    let mut slex = <Lexer as PortCell>::State::default();
    let mut spar = <Parser as PortCell>::State::default();
    let stmt = drive_link::<Lexer, Parser, InlineCarrier>(
        &mut slex, &mut spar, "INSERT INTO t VALUES (7)".to_string());
    println!("\n  Carrier INSERT 解析 => {stmt:?}");

    // ── 跨线程：把 Parser 放到工作线程 ──
    let mut slex2 = <Lexer as PortCell>::State::default();
    let stmt2 = spawned_flow::<Lexer, Parser>(&mut slex2, || (), "SELECT x FROM t".to_string());
    println!("  跨线程 SELECT 解析 => {stmt2:?}");

    println!("\npsql ok: SQL 解析执行流水线（Lexer→Parser→Executor）基于 cell_core + Carrier");
}
