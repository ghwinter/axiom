//! sqlmini 语义/计划阶段：AST → 校验后的逻辑计划（列存在、聚合合法性、
//! 表名归属、基本类型检查）。错误为 [`SqlError::Plan`]——带对象、带说明。

use axiom::cell_core::PortCell;

use crate::ast::{AggFn, BinOp, Expr, Stmt};
use crate::errors::SqlError;
use crate::schema::Schema;

/// 输出列（投影期求值）。
#[derive(Debug, Clone, PartialEq)]
pub struct OutCol {
    pub expr: Expr,
    pub name: String,
}

/// 排序键。
#[derive(Debug, Clone, PartialEq)]
pub struct OrderKey {
    pub expr: Expr,
    pub asc: bool,
}

/// 逻辑计划（执行期的全部输入）。
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub table: String,
    pub distinct: bool,
    pub out_cols: Vec<OutCol>,
    pub where_: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub order_by: Vec<OrderKey>,
    pub limit: Option<usize>,
    /// 计划内出现的聚合函数列表（执行器据此组装聚合器）。
    pub aggs: Vec<AggFn>,
}

// ── 校验助手 ─────────────────────────────────────────────────────────────

/// 列引用检查 + 收集：在 schema 上逐表达式核对；见错误带对象。
fn check_expr(
    schema: &Schema,
    expr: &Expr,
    in_agg: bool,
    aggs: &mut Vec<AggFn>,
) -> Result<(), SqlError> {
    match expr {
        Expr::Col(name) => {
            if schema.index(name).is_none() {
                return Err(SqlError::Plan(
                    name.clone(),
                    format!("列不存在（表 {}）", schema.table),
                ));
            }
            Ok(())
        }
        Expr::Lit(_) | Expr::Star => Ok(()),
        Expr::Bin(l, op, r) => {
            check_expr(schema, l, in_agg, aggs)?;
            check_expr(schema, r, in_agg, aggs)?;
            // 数字运算的操作数类型检查（字面量/列）。
            if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
                check_numeric_operand(schema, l)?;
                check_numeric_operand(schema, r)?;
            }
            Ok(())
        }
        Expr::Un(_, e) => check_expr(schema, e, in_agg, aggs),
        Expr::Agg(f, arg) => {
            if in_agg {
                return Err(SqlError::Plan(
                    "聚合嵌套".to_string(),
                    "聚合参数内不得再含聚合调用".to_string(),
                ));
            }
            if !aggs.contains(f) {
                aggs.push(*f);
            }
            check_expr(schema, arg, true, aggs)
        }
    }
}

fn check_numeric_operand(schema: &Schema, e: &Expr) -> Result<(), SqlError> {
    match e {
        Expr::Lit(crate::ast::Value::Int(_)) | Expr::Lit(crate::ast::Value::Float(_)) => Ok(()),
        Expr::Col(name) => match schema.col_type(name) {
            Some(t) if t.is_numeric() => Ok(()),
            Some(_) => Err(SqlError::Plan(
                name.clone(),
                "数字运算要求 Int/Float 列".to_string(),
            )),
            None => unreachable!("列存在性已检查"),
        },
        _ => Ok(()), // 复合表达式类型由执行期判定（Exec 错误），语义期不越界承诺
    }
}

/// 语义/计划主函数：语句 × schema → 计划。
pub fn plan(schema: &Schema, stmt: Stmt) -> Result<Plan, SqlError> {
    if !schema.is_registered() {
        return Err(SqlError::Plan(
            "schema".to_string(),
            "未注册表结构（驱动前先 Schema::from_columns 载入）".to_string(),
        ));
    }
    if stmt.from != schema.table {
        return Err(SqlError::Plan(
            stmt.from.clone(),
            format!("未知表（已注册: {}）", schema.table),
        ));
    }

    let mut aggs: Vec<AggFn> = Vec::new();
    let has_agg = stmt.items.iter().any(|i| expr_has_agg(&i.expr));

    // 输出列校验与命名。
    let mut out_cols = Vec::with_capacity(stmt.items.len());
    for (i, item) in stmt.items.iter().enumerate() {
        match &item.expr {
            Expr::Star => {
                // * 展开为全部列。
                for (n, _) in &schema.cols {
                    out_cols.push(OutCol {
                        expr: Expr::Col(n.clone()),
                        name: n.clone(),
                    });
                }
            }
            e => {
                check_expr(schema, e, false, &mut aggs)?;
                let name = item
                    .alias
                    .clone()
                    .or_else(|| e.col_name().map(str::to_string))
                    .or_else(|| match e {
                        Expr::Agg(f, _) => Some(f.default_name().to_string()),
                        _ => None,
                    })
                    .unwrap_or_else(|| format!("col{i}"));
                out_cols.push(OutCol {
                    expr: e.clone(),
                    name,
                });
            }
        }
    }

    // 聚合合法性：无 GROUP BY 时普通列与聚合不得混用。
    if !stmt.group_by.is_empty() {
        for k in &stmt.group_by {
            check_expr(schema, k, false, &mut aggs)?;
        }
    } else if has_agg {
        for o in &out_cols {
            if expr_has_agg(&o.expr) {
                continue;
            }
            if !matches!(&o.expr, Expr::Col(_)) {
                return Err(SqlError::Plan(
                    o.name.clone(),
                    "含聚合的查询不得带非分组普通表达式".to_string(),
                ));
            }
            return Err(SqlError::Plan(
                o.name.clone(),
                "含聚合的查询不得混合普通列（需 GROUP BY）".to_string(),
            ));
        }
    }

    // WHERE / ORDER。
    if let Some(w) = &stmt.where_ {
        check_expr(schema, w, false, &mut aggs)?;
    }
    // 别名解析（标准 SQL）：ORDER BY 可引用 SELECT 定义的输出名（含聚合别名）；
    // 命中别名 ⟹ 按被选表达式排序；未命中 ⟹ 回落表列校验。
    let aliases: std::collections::HashMap<&str, &Expr> = out_cols
        .iter()
        .map(|o| (o.name.as_str(), &o.expr))
        .collect();
    let mut order_by = Vec::with_capacity(stmt.order_by.len());
    for (e, asc) in &stmt.order_by {
        let aliased = match e {
            Expr::Col(c) => aliases.get(c.as_str()).copied(),
            _ => None,
        };
        match aliased {
            Some(ae) => order_by.push(OrderKey {
                expr: ae.clone(),
                asc: *asc,
            }),
            None => {
                check_expr(schema, e, false, &mut aggs)?;
                order_by.push(OrderKey {
                    expr: e.clone(),
                    asc: *asc,
                });
            }
        }
    }

    Ok(Plan {
        table: stmt.from,
        distinct: stmt.distinct,
        out_cols,
        where_: stmt.where_,
        group_by: stmt.group_by,
        order_by,
        limit: stmt.limit,
        aggs,
    })
}

fn expr_has_agg(e: &Expr) -> bool {
    match e {
        Expr::Agg(..) => true,
        Expr::Bin(l, _, r) => expr_has_agg(l) || expr_has_agg(r),
        Expr::Un(_, x) => expr_has_agg(x),
        _ => false,
    }
}

/// 计划 cell：`In = 语句` → `Out = Result<计划, SqlError>`。`State = Schema`
/// （默认空表 = 未注册；驱动前以 [`Schema::from_columns`] 载入）。
pub struct Planner;
impl PortCell for Planner {
    type In = Stmt;
    type Out = Result<Plan, SqlError>;
    type State = Schema;
    #[inline(always)]
    fn step(schema: &mut Schema, stmt: Stmt) -> Result<Plan, SqlError> {
        plan(schema, stmt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ColType;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn demo_schema() -> Schema {
        Schema::from_columns(
            "e",
            vec![
                ("id".to_string(), ColType::Int),
                ("dept".to_string(), ColType::Str),
                ("salary".to_string(), ColType::Int),
            ],
        )
    }

    fn plan_sql(schema: &Schema, sql: &str) -> Result<Plan, SqlError> {
        let stmt = parse(&lex(sql).expect("lex")).expect("parse");
        plan(schema, stmt)
    }

    #[test]
    fn unknown_column_is_plan_error() {
        let e = plan_sql(&demo_schema(), "SELECT nope FROM e").expect_err("unknown col");
        assert_eq!(e.stage(), "plan");
        assert!(matches!(e, SqlError::Plan(obj, _) if obj == "nope"));
    }

    #[test]
    fn unknown_table_is_plan_error() {
        let e = plan_sql(&demo_schema(), "SELECT id FROM other").expect_err("unknown table");
        assert!(matches!(e, SqlError::Plan(..)));
    }

    #[test]
    fn unregistered_schema_is_refused_not_guessed() {
        let e = plan_sql(&Schema::empty(), "SELECT id FROM e").expect_err("no schema");
        assert!(matches!(e, SqlError::Plan(..)));
    }

    #[test]
    fn aggregate_without_group_by_rejects_plain_columns() {
        let e = plan_sql(&demo_schema(), "SELECT dept, SUM(salary) FROM e").expect_err("mixed");
        assert!(matches!(e, SqlError::Plan(..)));
    }

    #[test]
    fn aggregate_with_group_by_is_legal() {
        let p = plan_sql(&demo_schema(), "SELECT dept, COUNT(*), SUM(salary) FROM e GROUP BY dept").unwrap();
        assert_eq!(p.aggs.len(), 2);
        assert!(p.aggs.contains(&AggFn::Count));
        assert!(p.aggs.contains(&AggFn::Sum));
        assert_eq!(p.group_by.len(), 1);
    }

    #[test]
    fn star_expands_to_all_columns_with_names() {
        let p = plan_sql(&demo_schema(), "SELECT * FROM e").unwrap();
        assert_eq!(p.out_cols.len(), 3);
        assert_eq!(p.out_cols[0].name, "id");
        assert_eq!(p.out_cols[2].name, "salary");
    }

    #[test]
    fn numeric_operand_type_check_rejects_string_column() {
        let e = plan_sql(&demo_schema(), "SELECT dept + 1 FROM e").expect_err("str+int");
        assert!(matches!(e, SqlError::Plan(..)), "Str 列参与数字运算 → Plan 错误");
    }

    #[test]
    fn planner_cell_conforms_to_its_dual_pair() {
        use axiom::cell_core::{Slot, assert_conforms};
        assert_conforms::<Slot<Stmt, Result<Plan, SqlError>>, Planner>();
    }
}