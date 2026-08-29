//! sqlmini —— SQL 子集编译器 + 执行引擎（中大型真实用例；axiom 范式验证）。
//!
//! 目标（project goal：范式证据优先）：以 multi-stage 失败管线（词法→语法→
//! 语义→计划→执行）真实地使用 axiom 的因果数据流、失败为值（`TryChain`）、
//! typed errors、组合与 T6（同计划多物理等价）。支持单表子集：
//! `SELECT 表达式 … FROM 表 [WHERE …] [GROUP BY …] [ORDER BY …] [LIMIT n]`，
//! 聚合 `COUNT/SUM/AVG/MIN/MAX`。
//!
//! 运行：`cargo run --manifest-path runtime/Cargo.toml --example sqlmini -- <query>`
//! 测试：`cargo test --manifest-path runtime/Cargo.toml --example sqlmini`

mod errors;
mod lexer;

mod ast;
mod parser;

mod data;
mod exec;
mod planner;
mod schema;

use axiom::cell_core::PortCell;
use axiom_semantics::prelude_all::TryChain;
use lexer::Lexer;

use crate::schema::{ColType, Schema};

// ── 阶段 1–3 链：Lexer → Parser → Planner（嵌套 TryChain，单层 Result）─────

/// 词法 → 语法复合。
type LexParse = TryChain<Lexer, parser::Parser>;
/// 全编译链：文本 → 计划；任一段 Err 即止。
type Compile = TryChain<LexParse, planner::Planner>;

/// 演示表。
pub fn demo_schema() -> Schema {
    Schema::from_columns(
        "e",
        vec![
            ("id".to_string(), ColType::Int),
            ("dept".to_string(), ColType::Str),
            ("salary".to_string(), ColType::Int),
        ],
    )
}

/// 演示数据（CSV 形态；测试亦可内联）。
pub const DEMO_CSV: &str = "\
id,dept,salary
1,eng,100
2,eng,120
3,ops,90
4,ops,80
5,gov,50
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let query = if args.is_empty() {
        "SELECT dept, COUNT(*), SUM(salary) FROM e GROUP BY dept ORDER BY dept".to_string()
    } else {
        args.join(" ")
    };

    // 阶段链驱动：任一段失败 = 值，带阶段名；成功 = 计划。
    let mut state = <Compile as PortCell>::State::default();
    let schema = demo_schema();
    // Planner 的 State = Schema：驱动前注册（TryChain 复合状态的第二个分量）。
    state.1 = schema.clone();
    let plan = match <Compile as PortCell>::step(&mut state, query.clone()) {
        Ok(plan) => plan,
        Err(e) => {
            println!("sqlmini: query rejected at stage {} — {e}", e.stage());
            std::process::exit(1);
        }
    };

    // 数据加载（执行输入）。
    let rows = match data::load_csv(&schema, DEMO_CSV) {
        Ok(r) => r,
        Err(e) => {
            println!("sqlmini: data rejected — {e}");
            std::process::exit(1);
        }
    };

    // 执行（物理路径 1：Inline）。
    let (out, stats) = match exec::execute(&plan, &schema, &rows) {
        Ok(r) => r,
        Err(e) => {
            println!("sqlmini: exec failed — {e}");
            std::process::exit(1);
        }
    };
    println!("sqlmini: rows scanned={} filtered={} out={}", stats.scanned, stats.filtered, out.len());

    // 物理路径 2：分区并行规约（T6 逐行等价断言）。
    let (out_p, _) = exec::execute_parallel(&plan, &schema, &rows, 2).expect("parallel exec");
    assert_eq!(out, out_p, "T6：同计划双物理路径逐行等价");
    println!("sqlmini: T6 ok — parallel route identical ({})", out_p.len());

    print!("{}", exec::format_result(&plan, &out));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_chain_lexer_parser_drives_to_stmt() {
        // 阶段链：文本 → (Lexer → Parser) 单层 Result。
        let mut st = <LexParse as PortCell>::State::default();
        let out = <LexParse as PortCell>::step(&mut st, "SELECT a, b FROM t".to_string());
        let stmt = out.expect("chain should parse");
        assert_eq!(stmt.from, "t");
        assert_eq!(stmt.items.len(), 2);
    }

    #[test]
    fn stage_chain_short_circuits_on_lex_error() {
        // 词法失败：Parser 不执行（TryChain 短路），错误为值。
        let mut st = <LexParse as PortCell>::State::default();
        let out = <LexParse as PortCell>::step(&mut st, "SELECT @".to_string());
        let e = out.expect_err("chain should reject at lex");
        assert_eq!(e.stage(), "lex");
        assert!(matches!(e, crate::errors::SqlError::Lex(..)));
    }

    #[test]
    fn stage_chain_short_circuits_on_parse_error() {
        // 语法失败：短路于 parse，错误为值、带位置。
        let mut st = <LexParse as PortCell>::State::default();
        let out = <LexParse as PortCell>::step(&mut st, "SELECT a t".to_string());
        let e = out.expect_err("chain should reject at parse");
        assert_eq!(e.stage(), "parse");
        assert!(matches!(e, crate::errors::SqlError::Parse(..)));
    }

    #[test]
    fn compile_chain_drives_text_to_plan_with_registered_schema() {
        // 全编译链：文本 → 计划（schema 注册于 Planner 状态位）。
        let mut st = <Compile as PortCell>::State::default();
        st.1 = demo_schema();
        let plan = <Compile as PortCell>::step(
            &mut st,
            "SELECT dept, SUM(salary) FROM e GROUP BY dept".to_string(),
        )
        .expect("compile ok");
        assert_eq!(plan.out_cols.len(), 2);
        assert!(plan.aggs.contains(&crate::ast::AggFn::Sum));
    }

    #[test]
    fn compile_chain_rejects_unknown_column_at_plan_stage() {
        let mut st = <Compile as PortCell>::State::default();
        st.1 = demo_schema();
        let e = <Compile as PortCell>::step(&mut st, "SELECT nope FROM e".to_string())
            .expect_err("reject");
        assert_eq!(e.stage(), "plan");
    }

    #[test]
    fn compile_chain_refuses_unregistered_schema_after_parse() {
        // 语法通过、schema 未注册 → Plan 阶段拒绝（诚实：不猜列）。
        let mut st = <Compile as PortCell>::State::default();
        let e = <Compile as PortCell>::step(&mut st, "SELECT id FROM e".to_string())
            .expect_err("no schema");
        assert_eq!(e.stage(), "plan");
    }

    #[test]
    fn lexical_failure_is_a_value_not_a_panic() {
        let mut st = <LexParse as PortCell>::State::default();
        let out: Result<ast::Stmt, crate::errors::SqlError> =
            <LexParse as PortCell>::step(&mut st, "SELECT @".to_string());
        assert!(out.is_err(), "非法字符 = 类型化失败");
    }
}