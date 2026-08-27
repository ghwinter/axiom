//! psql —— 用 cell_core 四构件 + runtime 错误/短路驱动构建的 SQL REPL 流水线。
//!
//! 健壮性增强（现实问题驱动 runtime）：`Lexer`/`Parser` 会失败（`Out = Result`），
//! 主流程把 Lexer→Parser→Executor **串成一个单层可失败链** `TryChain`——词法/语法/执行
//! 错误都是单层 `Result`，任一环节 `Err` 即短路；整条腐用管线是一个可组合的 `PortCell`。
//!
//! 运行：`cargo run --example psql`

mod cells;

use axiom::cell_core::PortCell;
use axiom_runtime::drive::flow::TryChain;

use cells::{Executor, ExecOut, Lexer, Parser};

fn main() {
    println!("=== psql: robust SQL REPL with error short-circuit ===");

    // 一批有对有错的 SQL（健壮性：错误不静默、短路不执行）。
    let sqls = [
        "CREATE TABLE users (id, name)",
        "INSERT INTO users VALUES (1)",
        "INSERT INTO users VALUES (2)",
        "INSERT INTO users VALUES (42)",
        "SELECT * FROM users",
        "SELECT * FROM missing",          // 有 Stmt，但表不存在 → Executor 报错
        "INSERT INTO users VALUES",       // 缺 VALUES → Parser 短路
        "CREATE TABLE",                   // 缺表名 → Parser 短路
        "SELECT * FROM 'oops",            // 未闭合字符串 → Lexer 短路
    ];

    // 整条可失败管线 = 一个单层 Result 的 PortCell：
    //   In = String（SQL），Out = Result<ExecOut, PErr>（LEX/PARSE/EXEC 三层错误合一短路）。
    // 内部：Lexer(Result<Tokens>) -> Parser(Result<Stmt>) -> Executor(Result<ExecOut>)。
    type Pipeline = TryChain<TryChain<Lexer, Parser>, Executor>;
    let mut pipeline_state = <Pipeline as PortCell>::State::default();

    for sql in sqls {
        match <Pipeline as PortCell>::step(&mut pipeline_state, sql.to_string()) {
            Err(e) => println!("  {sql:<28} => ERROR {e:?}"),
            Ok(ExecOut::Ok(msg)) => println!("  {sql:<28} => {msg}"),
            Ok(ExecOut::Rows(rows)) => println!("  {sql:<28} => {:?}", rows),
        }
    }

    println!("\npsql ok: robust error-handling REPL (Lexer→Parser→Executor) via TryChain");
}
