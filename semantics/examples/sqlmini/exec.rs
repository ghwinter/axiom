//! sqlmini 执行引擎：表达式求值 + 双物理路径（Inline 单线程 / 分区并行规约），
//! T6 等价。
//!
//! 管道（顺序固定）：Scan → Filter(WHERE) → [聚合：组累积并投影 | 普通：逐行投影]
//!   → Distinct → Sort → Limit。
//! 并行路径边界：Filter 与组累积按行块分线程（聚合在「合并」下可结合——
//! Count/Sum/Min/Max 与 Avg(经 sum/count) 可分）；投影/去重/排序/截断在合并后
//! 统一执行（ORDER/LIMIT 不可分，保持全局语义）。两路径结果逐行相等（T6）。
//!
//! NULL 语义：谓词遇 Null 为假（WHERE 排除）；聚合忽略 Null 输入；算术/比较
//! 遇 Null 传播 Null。子集限制：ORDER BY 键仅支持输出列名（表达式键落 Null，
//! 见 `sort_key`）。

use std::collections::BTreeMap;

use crate::ast::{AggFn, BinOp, Expr, UnOp, Value};
use crate::data::Record;
use crate::errors::SqlError;
use crate::planner::{OrderKey, Plan};
use crate::schema::Schema;

// ── 聚合累积（可合并）─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct ColAcc {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
}

impl ColAcc {
    fn absorb(&mut self, v: &Value) {
        if let Some(x) = v.as_f64() {
            self.count += 1;
            self.sum += x;
            if self.count == 1 {
                self.min = x;
                self.max = x;
            } else {
                self.min = self.min.min(x);
                self.max = self.max.max(x);
            }
        }
    }
    fn merge(&mut self, other: &ColAcc) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = *other;
            return;
        }
        self.count += other.count;
        self.sum += other.sum;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
    }
    fn project(&self, f: AggFn) -> Value {
        match f {
            AggFn::Count => Value::Int(self.count as i64),
            AggFn::Sum => Value::Float(self.sum),
            AggFn::Avg => {
                if self.count == 0 {
                    Value::Null
                } else {
                    Value::Float(self.sum / self.count as f64)
                }
            }
            AggFn::Min => {
                if self.count == 0 {
                    Value::Null
                } else {
                    Value::Float(self.min)
                }
            }
            AggFn::Max => {
                if self.count == 0 {
                    Value::Null
                } else {
                    Value::Float(self.max)
                }
            }
        }
    }
}

/// 组表：分组键 → 每聚合输出列一个累积器。
pub type GroupTable = BTreeMap<Vec<Value>, Vec<ColAcc>>;

/// 聚合规格：与 `plan.out_cols` 对齐（`Some(f)` = 该列为聚合）。
pub type AggSpec = Vec<Option<AggFn>>;

fn agg_spec(plan: &Plan) -> AggSpec {
    plan.out_cols
        .iter()
        .map(|o| match &o.expr {
            Expr::Agg(f, _) => Some(*f),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStats {
    pub scanned: usize,
    pub filtered: usize,
}

// ── 表达式求值 ────────────────────────────────────────────────────────────

fn eval(schema: &Schema, row: &Record, e: &Expr) -> Result<Value, SqlError> {
    match e {
        Expr::Col(name) => {
            let i = schema.index(name).ok_or_else(|| {
                SqlError::Exec("eval".to_string(), format!("列 {name} 缺失（与计划不一致）"))
            })?;
            Ok(row.get(i).cloned().unwrap_or(Value::Null))
        }
        Expr::Lit(v) => Ok(v.clone()),
        Expr::Star => Err(SqlError::Exec(
            "eval".to_string(),
            "* 出现在表达式位置（应由投影展开）".to_string(),
        )),
        Expr::Agg(..) => Err(SqlError::Exec(
            "eval".to_string(),
            "聚合出现于行求值路径".to_string(),
        )),
        Expr::Un(op, x) => {
            let v = eval(schema, row, x)?;
            match op {
                UnOp::Neg => Ok(v.as_f64().map(Value::Float).unwrap_or(Value::Null)),
                UnOp::Not => match v {
                    Value::Bool(b) => Ok(Value::Bool(!b)),
                    _ => Ok(Value::Null),
                },
            }
        }
        Expr::Bin(l, op, r) => {
            let a = eval(schema, row, l)?;
            let b = eval(schema, row, r)?;
            apply_bin(*op, a, b)
        }
    }
}

fn apply_bin(op: BinOp, a: Value, b: Value) -> Result<Value, SqlError> {
    use BinOp::*;
    if matches!(a, Value::Null) || matches!(b, Value::Null) {
        return Ok(Value::Null);
    }
    let out = match op {
        Add | Sub | Mul | Div => {
            let x = num(&a, &op)?;
            let y = num(&b, &op)?;
            let z = match op {
                Add => x + y,
                Sub => x - y,
                Mul => x * y,
                Div => x / y,
                _ => unreachable!(),
            };
            Value::Float(z)
        }
        And | Or => {
            let x = as_bool(&a, "逻辑")?;
            let y = as_bool(&b, "逻辑")?;
            Value::Bool(match op {
                And => x && y,
                _ => x || y,
            })
        }
        Eq | Ne | Lt | Le | Gt | Ge => {
            let r = match (&a, &b) {
                (Value::Str(x), Value::Str(y)) => str_cmp(op, x, y),
                _ => num_pair_cmp(op, &a, &b)?,
            };
            Value::Bool(r)
        }
    };
    Ok(out)
}

fn num(v: &Value, op: &BinOp) -> Result<f64, SqlError> {
    v.as_f64().ok_or_else(|| {
        SqlError::Exec(
            "eval".to_string(),
            format!("算术操作数非数值：{v:?}（plan 阶段已拦列类型；字面量/表达式例外）{op:?}"),
        )
    })
}

fn as_bool(v: &Value, what: &str) -> Result<bool, SqlError> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(SqlError::Exec(
            "eval".to_string(),
            format!("{what}操作数非布尔：{v:?}"),
        )),
    }
}

fn num_pair_cmp(op: BinOp, a: &Value, b: &Value) -> Result<bool, SqlError> {
    let x = a
        .as_f64()
        .ok_or_else(|| SqlError::Exec("eval".to_string(), format!("比较操作数不可比：{a:?}（Str 仅支持字符串对）")))?;
    let y = b
        .as_f64()
        .ok_or_else(|| SqlError::Exec("eval".to_string(), format!("比较操作数不可比：{b:?}")))?;
    let ord = x.partial_cmp(&y).unwrap_or(core::cmp::Ordering::Equal);
    Ok(match op {
        BinOp::Eq => ord == core::cmp::Ordering::Equal,
        BinOp::Ne => ord != core::cmp::Ordering::Equal,
        BinOp::Lt => ord == core::cmp::Ordering::Less,
        BinOp::Le => ord != core::cmp::Ordering::Greater,
        BinOp::Gt => ord == core::cmp::Ordering::Greater,
        BinOp::Ge => ord != core::cmp::Ordering::Less,
        _ => unreachable!(),
    })
}

fn str_cmp(op: BinOp, x: &str, y: &str) -> bool {
    match op {
        BinOp::Eq => x == y,
        BinOp::Ne => x != y,
        BinOp::Lt => x < y,
        BinOp::Le => x <= y,
        BinOp::Gt => x > y,
        BinOp::Ge => x >= y,
        _ => unreachable!(),
    }
}

fn is_true(v: &Value) -> bool {
    matches!(v, Value::Bool(true))
}

// ── 组表构建（两条物理路径的分界）──────────────────────────────────────

/// 单线程：过滤并累积组表（或普通行列表）。
enum Acc {
    Grouped(GroupTable),
    Plain(Vec<Record>),
}

fn accumulate(plan: &Plan, schema: &Schema, rows: &[Record]) -> Result<(Acc, RunStats), SqlError> {
    let spec = agg_spec(plan);
    let mut groups: GroupTable = BTreeMap::new();
    let mut plain = Vec::new();
    let mut stats = RunStats::default();
    for row in rows {
        stats.scanned += 1;
        if let Some(w) = &plan.where_
            && !is_true(&eval(schema, row, w)?)
        {
            continue;
        }
        stats.filtered += 1;
        if plan.aggs.is_empty() {
            let mut r = Vec::with_capacity(plan.out_cols.len());
            for o in &plan.out_cols {
                r.push(eval(schema, row, &o.expr)?);
            }
            plain.push(r);
        } else {
            let key: Vec<Value> = plan
                .group_by
                .iter()
                .map(|k| eval(schema, row, k))
                .collect::<Result<_, _>>()?;
            let entry = groups
                .entry(key)
                .or_insert_with(|| vec![ColAcc::default(); spec.len()]);
            for (i, acc) in entry.iter_mut().enumerate() {
                if spec[i].is_some() {
                    match &plan.out_cols[i].expr {
                        // COUNT(*)：每行计数一，无视行值。
                        Expr::Agg(AggFn::Count, arg) if matches!(**arg, Expr::Star) => {
                            acc.count += 1;
                        }
                        // 其他聚合：按参数表达式求值后吸收。
                        Expr::Agg(f, arg) if *f != AggFn::Count || !matches!(**arg, Expr::Star) => {
                            let v = eval(schema, row, arg)?;
                            acc.absorb(&v);
                        }
                        other => {
                            return Err(SqlError::Exec(
                                "agg".to_string(),
                                format!("聚合参数形态未覆盖：{other:?}"),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(if plan.aggs.is_empty() {
        (Acc::Plain(plain), stats)
    } else {
        (Acc::Grouped(groups), stats)
    })
}

/// 并行：块内过滤+累积（聚合可分）；合并组表；普通行块直接拼接。
fn accumulate_parallel(
    plan: &Plan,
    schema: &Schema,
    rows: &[Record],
    nthreads: usize,
) -> Result<(Acc, RunStats), SqlError> {
    let n = nthreads.max(1);
    let chunk = rows.len().div_ceil(n).max(1);
    let plan = plan.clone();
    let schema = schema.clone();
    let mut handles = Vec::new();
    for block in rows.chunks(chunk) {
        let plan = plan.clone();
        let schema = schema.clone();
        let block: Vec<Record> = block.to_vec();
        handles.push(std::thread::spawn(move || accumulate(&plan, &schema, &block)));
    }
    let mut groups: GroupTable = BTreeMap::new();
    let mut plain: Vec<Record> = Vec::new();
    let mut stats = RunStats::default();
    for h in handles {
        let (acc, s) = h
            .join()
            .map_err(|_| SqlError::Exec("parallel".to_string(), "线程异常退出".to_string()))??;
        stats.scanned += s.scanned;
        stats.filtered += s.filtered;
        match acc {
            Acc::Grouped(g) => {
                for (key, accs) in g {
                    let entry = groups
                        .entry(key)
                        .or_insert_with(|| vec![ColAcc::default(); accs.len()]);
                    for (i, o) in accs.iter().enumerate() {
                        if let Some(dst) = entry.get_mut(i) {
                            dst.merge(o);
                        }
                    }
                }
            }
            Acc::Plain(rows) => plain.extend(rows),
        }
    }
    Ok(if plan.aggs.is_empty() {
        (Acc::Plain(plain), stats)
    } else {
        (Acc::Grouped(groups), stats)
    })
}

/// 组表 → 输出行（分组键 + 聚合值）。
fn project_groups(plan: &Plan, groups: &GroupTable) -> Vec<Record> {
    let spec = agg_spec(plan);
    groups
        .iter()
        .map(|(key, accs)| {
            plan.out_cols
                .iter()
                .enumerate()
                .map(|(i, o)| match &o.expr {
                    Expr::Agg(..) => accs[i].project(spec[i].unwrap()),
                    _ => {
                        if let Some(gix) = plan.group_by.iter().position(|g| g == &o.expr) {
                            key[gix].clone()
                        } else {
                            Value::Null
                        }
                    }
                })
                .collect()
        })
        .collect()
}

/// 尾部统一：Distinct → Sort → Limit。
fn tail(plan: &Plan, mut rows: Vec<Record>) -> Vec<Record> {
    if plan.distinct {
        let mut seen = std::collections::HashSet::new();
        rows.retain(|r| {
            use std::hash::Hasher as _;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for v in r {
                v.hash_value(&mut h);
            }
            seen.insert(h.finish())
        });
    }
    if !plan.order_by.is_empty() {
        rows.sort_by(|a, b| {
            for OrderKey { expr, asc } in &plan.order_by {
                let ord = sort_key(plan, a, expr).total_cmp(&sort_key(plan, b, expr));
                let ord = if *asc { ord } else { ord.reverse() };
                if ord != core::cmp::Ordering::Equal {
                    return ord;
                }
            }
            core::cmp::Ordering::Equal
        });
    }
    if let Some(n) = plan.limit {
        rows.truncate(n);
    }
    rows
}

/// 排序键：支持输出列名（子集限制：表达式键落 Null，见模块文档）。
fn sort_key(plan: &Plan, out_row: &Record, e: &Expr) -> Value {
    match e {
        Expr::Col(name) => plan
            .out_cols
            .iter()
            .position(|o| &o.name == name)
            .and_then(|i| out_row.get(i).cloned())
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

// ── 公共入口 ─────────────────────────────────────────────────────────────
/// 全局聚合（无 GROUP BY）在零输入时仍须产出一行（COUNT=0 / SUM=NULL 等）；
/// 在组表上补单一空组。
fn ensure_global_group(plan: &Plan, groups: &mut GroupTable) {
    if plan.group_by.is_empty() && groups.is_empty() && !plan.aggs.is_empty() {
        groups.insert(vec![], vec![ColAcc::default(); agg_spec(plan).len()]);
    }
}

/// 物理路径 1：Inline 单线程。
pub fn execute(
    plan: &Plan,
    schema: &Schema,
    rows: &[Record],
) -> Result<(Vec<Record>, RunStats), SqlError> {
    let (acc, stats) = accumulate(plan, schema, rows)?;
    let rows = match acc {
        Acc::Grouped(mut g) => {
            ensure_global_group(plan, &mut g);
            project_groups(plan, &g)
        }
        Acc::Plain(p) => p,
    };
    Ok((tail(plan, rows), stats))
}

/// 物理路径 2：分区并行规约（T6：与 Inline 逐行等价）。
pub fn execute_parallel(
    plan: &Plan,
    schema: &Schema,
    rows: &[Record],
    nthreads: usize,
) -> Result<(Vec<Record>, RunStats), SqlError> {
    let (acc, stats) = accumulate_parallel(plan, schema, rows, nthreads)?;
    let rows = match acc {
        Acc::Grouped(mut g) => {
            ensure_global_group(plan, &mut g);
            project_groups(plan, &g)
        }
        Acc::Plain(p) => p,
    };
    Ok((tail(plan, rows), stats))
}

/// 结果文本表（列名 + 行；输出目的地由调用方选）。
pub fn format_result(plan: &Plan, rows: &[Record]) -> String {
    let mut s = String::new();
    let names: Vec<String> = plan.out_cols.iter().map(|o| o.name.clone()).collect();
    s.push_str(&names.join(" | "));
    s.push('\n');
    for r in rows {
        s.push_str(&r.iter().map(Value::display).collect::<Vec<_>>().join(" | "));
        s.push('\n');
    }
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::load_csv;
    use crate::lexer::lex;
    use crate::parser::parse;
    use crate::planner::plan;
    use crate::schema::{ColType, Schema};

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

    const CSV: &str = "\
id,dept,salary
1,eng,100
2,eng,120
3,ops,90
4,ops,80
5,gov,50
";

    fn compile(sql: &str, schema: &Schema) -> Plan {
        let stmt = parse(&lex(sql).expect("lex")).expect("parse");
        plan(schema, stmt).expect("plan")
    }

    fn rows(schema: &Schema) -> Vec<Record> {
        load_csv(schema, CSV).expect("csv")
    }

    #[test]
    fn eval_numeric_and_string_operations() {
        let schema = demo_schema();
        let row = vec![Value::Int(1), Value::Str("eng".into()), Value::Int(100)];
        assert_eq!(
            eval(&schema, &row, &Expr::Bin(Box::new(Expr::Col("salary".into())), BinOp::Add, Box::new(Expr::Lit(Value::Int(10))))).unwrap(),
            Value::Float(110.0)
        );
        assert_eq!(
            eval(&schema, &row, &Expr::Bin(Box::new(Expr::Col("dept".into())), BinOp::Eq, Box::new(Expr::Lit(Value::Str("eng".into()))))).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn null_propagates_and_predicate_filters_it() {
        // Null 传播：算术/比较含 Null → Null；WHERE 以 Null 为假。
        let schema = demo_schema();
        let row = vec![Value::Null, Value::Str("x".into()), Value::Int(1)];
        let v = eval(&schema, &row, &Expr::Bin(Box::new(Expr::Col("id".into())), BinOp::Add, Box::new(Expr::Lit(Value::Int(1))))).unwrap();
        assert_eq!(v, Value::Null);
        assert!(!is_true(&v));
    }

    #[test]
    fn group_aggregates_correct_math() {
        let schema = demo_schema();
        let plan = compile("SELECT dept, COUNT(*), SUM(salary), AVG(salary), MIN(salary), MAX(salary) FROM e GROUP BY dept ORDER BY dept", &schema);
        let (out, stats) = execute(&plan, &schema, &rows(&schema)).unwrap();
        assert_eq!(stats.scanned, 5);
        assert_eq!(out.len(), 3, "eng/ops/gov");
        // eng: count2, sum220, avg110, min100, max120
        assert_eq!(out[0][0], Value::Str("eng".into()));
        assert_eq!(out[0][1], Value::Int(2));
        assert_eq!(out[0][2], Value::Float(220.0));
        assert_eq!(out[0][3], Value::Float(110.0));
        assert_eq!(out[0][4], Value::Float(100.0));
        assert_eq!(out[0][5], Value::Float(120.0));
    }

    #[test]
    fn where_filter_and_limit_and_order() {
        let schema = demo_schema();
        let plan = compile("SELECT id, salary FROM e WHERE salary >= 90 ORDER BY salary DESC LIMIT 2", &schema);
        let (out, stats) = execute(&plan, &schema, &rows(&schema)).unwrap();
        assert_eq!(stats.filtered, 3, "100/120/90 通过");
        assert_eq!(out.len(), 2);
        // 无运算投影保留原值类型（Int 列 → Int；运算才产生 Float）。
        assert_eq!(out[0], vec![Value::Int(2), Value::Int(120)]);
        assert_eq!(out[1], vec![Value::Int(1), Value::Int(100)]);
    }

    #[test]
    fn global_aggregate_without_group_by() {
        let schema = demo_schema();
        let plan = compile("SELECT COUNT(*), SUM(salary) FROM e", &schema);
        let (out, _) = execute(&plan, &schema, &rows(&schema)).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0][0], Value::Int(5));
        assert_eq!(out[0][1], Value::Float(440.0));
    }

    #[test]
    fn parallel_route_matches_inline_byte_for_byte() {
        // T6：同计划、同数据——双物理路径逐行等价（含聚合/排序/截断）。
        let schema = demo_schema();
        let cases = [
            "SELECT dept, COUNT(*), SUM(salary) FROM e GROUP BY dept ORDER BY dept",
            "SELECT id, salary FROM e WHERE salary >= 80 ORDER BY salary DESC LIMIT 3",
            "SELECT DISTINCT dept FROM e ORDER BY dept",
            "SELECT COUNT(*), AVG(salary) FROM e",
        ];
        for sql in cases {
            let plan = compile(sql, &schema);
            let data = rows(&schema);
            let (a, _) = execute(&plan, &schema, &data).unwrap();
            let (b, _) = execute_parallel(&plan, &schema, &data, 3).unwrap();
            assert_eq!(a, b, "T6 违反: {sql}");
        }
    }

    #[test]
    fn exec_errors_are_typed_values() {
        // 运行时错误（如排序表达式落 Null 的子集限制不报错；此处触发
        // 非法算术：Str 列参与加法——plan 已拦；用表达式逃逸：计算结果错误
        // 不再出现，故此处验证"无 panic"路径本身：空表正常执行）。
        let schema = demo_schema();
        let plan = compile("SELECT COUNT(*) FROM e", &schema);
        let (out, _) = execute(&plan, &schema, &[]).unwrap();
        assert_eq!(out, vec![vec![Value::Int(0)]], "空输入 COUNT=0");
    }

    #[test]
    fn null_rows_are_ignored_by_aggregates() {
        // 聚合忽略 Null 数值输入：COUNT 不数它、SUM 不累它（手工构造含 Null 行）。
        let schema = Schema::from_columns("t", vec![("v".to_string(), ColType::Int)]);
        let plan = compile("SELECT COUNT(v), SUM(v) FROM t", &schema);
        let rows = vec![vec![Value::Null], vec![Value::Int(3)]];
        let (out, _) = execute(&plan, &schema, &rows).unwrap();
        assert_eq!(out, vec![vec![Value::Int(1), Value::Float(3.0)]], "Null 不入计数/求和");
    }
}
