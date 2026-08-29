//! sqlmini 语法阶段：Token 流 → AST（递归下降；失败为值、带位置）。

use axiom::cell_core::PortCell;

use crate::ast::{AggFn, BinOp, Expr, SelectItem, Stmt, UnOp, Value};
use crate::errors::SqlError;
use crate::lexer::Tok;

/// 词法游标：位置 + 前瞻。
struct Cursor<'a> {
    toks: &'a [Tok],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(toks: &'a [Tok]) -> Self {
        Cursor { toks, pos: 0 }
    }
    fn peek(&self) -> &Tok {
        self.toks
            .get(self.pos)
            .unwrap_or(&Tok::Eof)
    }
    fn next(&mut self) -> Tok {
        let t = self.toks.get(self.pos).cloned().unwrap_or(Tok::Eof);
        self.pos += 1;
        t
    }
    fn eat(&mut self, want: &Tok) -> Result<(), SqlError> {
        if self.peek() == want {
            self.pos += 1;
            Ok(())
        } else {
            Err(SqlError::Parse(
                self.pos,
                format!("{want:?}"),
                format!("{:?}", self.peek()),
            ))
        }
    }
    fn expect_ident(&mut self) -> Result<String, SqlError> {
        match self.next() {
            Tok::Ident(s) => Ok(s),
            other => Err(SqlError::Parse(
                self.pos.saturating_sub(1),
                "标识符".to_string(),
                format!("{other:?}"),
            )),
        }
    }
    fn is_ident(&self) -> bool {
        matches!(self.peek(), Tok::Ident(_))
    }
}

// ── 文法（单表子集）──────────────────────────────────────────────────────
//   stmt  := SELECT [DISTINCT] items FROM ident
//           [WHERE expr] [GROUP BY expr_list] [ORDER BY order_list] [LIMIT num] [;]
//   items := '*' | item (',' item)*
//   item  := expr [AS ident]
//   expr  := or_expr
//   or    := and (OR and)*
//   and   := not (AND not)*
//   not   := [NOT] cmp
//   cmp   := add [(=|!=|<|<=|>|>=) add]?
//   add   := mul ((+|-) mul)*
//   mul   := primary ((*|/) primary)*
//   primary := IDENT | NUM | STR | '(' expr ')' | 聚合 '(' (expr|'*') ')'

/// 语法主函数：词法流 → 语句（或带位置的语法错误）。
pub fn parse(toks: &[Tok]) -> Result<Stmt, SqlError> {
    let mut c = Cursor::new(toks);
    let stmt = parse_stmt(&mut c)?;
    // 允许尾部分号；其后必须 EOF。
    if c.peek() == &Tok::Semi {
        c.next();
    }
    if c.peek() != &Tok::Eof {
        return Err(SqlError::Parse(
            c.pos,
            "语句结束".to_string(),
            format!("{:?}", c.peek()),
        ));
    }
    Ok(stmt)
}

fn parse_stmt(c: &mut Cursor) -> Result<Stmt, SqlError> {
    c.eat(&Tok::Select)?;
    let distinct = if c.peek() == &Tok::Distinct {
        c.next();
        true
    } else {
        false
    };
    let items = parse_items(c)?;
    c.eat(&Tok::From)?;
    let from = c.expect_ident()?;
    let where_ = if c.peek() == &Tok::Where {
        c.next();
        Some(parse_expr(c)?)
    } else {
        None
    };
    let group_by = if c.peek() == &Tok::Group {
        c.next();
        c.eat(&Tok::By)?;
        parse_expr_list(c)?
    } else {
        Vec::new()
    };
    let order_by = if c.peek() == &Tok::Order {
        c.next();
        c.eat(&Tok::By)?;
        parse_order_list(c)?
    } else {
        Vec::new()
    };
    let limit = if c.peek() == &Tok::Limit {
        c.next();
        match c.next() {
            Tok::Num(n) => match n.parse::<usize>() {
                Ok(v) => Some(v),
                Err(_) => {
                    return Err(SqlError::Parse(c.pos.saturating_sub(1), "非负整数".to_string(), n))
                }
            },
            other => {
                return Err(SqlError::Parse(
                    c.pos.saturating_sub(1),
                    "LIMIT 数量".to_string(),
                    format!("{other:?}"),
                ))
            }
        }
    } else {
        None
    };
    Ok(Stmt {
        distinct,
        items,
        from,
        where_,
        group_by,
        order_by,
        limit,
    })
}

fn parse_items(c: &mut Cursor) -> Result<Vec<SelectItem>, SqlError> {
    let mut items = Vec::new();
    loop {
        // 表达式起始位置的 `*` = SELECT 顶层星号（词法统一为 Mul，此处语境截获）。
        if c.peek() == &Tok::Mul {
            c.next();
            // 若 * 后跟着逗号则非法（本子集仅允许独立 *）。
            if c.peek() == &Tok::Comma {
                return Err(SqlError::Parse(
                    c.pos,
                    "独立 *（不允许 * 与其他列混用）".to_string(),
                    format!("{:?}", c.peek()),
                ));
            }
            items.push(SelectItem {
                expr: Expr::Star,
                alias: None,
            });
        } else {
            let expr = parse_expr(c)?;
            let alias = if c.peek() == &Tok::As {
                c.next();
                Some(c.expect_ident()?)
            } else if c.is_ident() {
                // 非关键字标识符紧跟表达式 = 隐式别名（简单起见允许）。
                Some(c.expect_ident()?)
            } else {
                None
            };
            items.push(SelectItem { expr, alias });
        }
        if c.peek() == &Tok::Comma {
            c.next();
        } else {
            break;
        }
    }
    if items.is_empty() {
        return Err(SqlError::Parse(c.pos, "选择项".to_string(), "空".to_string()));
    }
    Ok(items)
}

fn parse_expr(c: &mut Cursor) -> Result<Expr, SqlError> {
    parse_or(c)
}

fn parse_or(c: &mut Cursor) -> Result<Expr, SqlError> {
    let mut e = parse_and(c)?;
    while c.peek() == &Tok::Or {
        c.next();
        let rhs = parse_and(c)?;
        e = Expr::Bin(Box::new(e), BinOp::Or, Box::new(rhs));
    }
    Ok(e)
}

fn parse_and(c: &mut Cursor) -> Result<Expr, SqlError> {
    let mut e = parse_not(c)?;
    while c.peek() == &Tok::And {
        c.next();
        let rhs = parse_not(c)?;
        e = Expr::Bin(Box::new(e), BinOp::And, Box::new(rhs));
    }
    Ok(e)
}

fn parse_not(c: &mut Cursor) -> Result<Expr, SqlError> {
    if c.peek() == &Tok::Not {
        c.next();
        let e = parse_cmp(c)?;
        return Ok(Expr::Un(UnOp::Not, Box::new(e)));
    }
    parse_cmp(c)
}

fn parse_cmp(c: &mut Cursor) -> Result<Expr, SqlError> {
    let lhs = parse_add(c)?;
    let op = match c.peek() {
        Tok::Eq => Some(BinOp::Eq),
        Tok::Ne => Some(BinOp::Ne),
        Tok::Lt => Some(BinOp::Lt),
        Tok::Le => Some(BinOp::Le),
        Tok::Gt => Some(BinOp::Gt),
        Tok::Ge => Some(BinOp::Ge),
        _ => None,
    };
    match op {
        Some(op) => {
            c.next();
            let rhs = parse_add(c)?;
            Ok(Expr::Bin(Box::new(lhs), op, Box::new(rhs)))
        }
        None => Ok(lhs),
    }
}

fn parse_add(c: &mut Cursor) -> Result<Expr, SqlError> {
    let mut e = parse_mul(c)?;
    loop {
        let op = match c.peek() {
            Tok::Plus => Some(BinOp::Add),
            Tok::Minus => Some(BinOp::Sub),
            _ => None,
        };
        match op {
            Some(op) => {
                c.next();
                let rhs = parse_mul(c)?;
                e = Expr::Bin(Box::new(e), op, Box::new(rhs));
            }
            None => return Ok(e),
        }
    }
}

fn parse_mul(c: &mut Cursor) -> Result<Expr, SqlError> {
    let mut e = parse_primary(c)?;
    loop {
        let op = match c.peek() {
            Tok::Mul => Some(BinOp::Mul),
            Tok::Div => Some(BinOp::Div),
            _ => None,
        };
        match op {
            Some(op) => {
                c.next();
                let rhs = parse_primary(c)?;
                e = Expr::Bin(Box::new(e), op, Box::new(rhs));
            }
            None => return Ok(e),
        }
    }
}

fn parse_primary(c: &mut Cursor) -> Result<Expr, SqlError> {
    match c.next() {
        Tok::Ident(name) => Ok(Expr::Col(name)),
        Tok::Num(n) => parse_number(&n, c),
        Tok::Str(s) => Ok(Expr::Lit(Value::Str(s))),
        Tok::LParen => {
            let e = parse_expr(c)?;
            c.eat(&Tok::RParen)?;
            Ok(e)
        }
        Tok::Minus => {
            let e = parse_primary(c)?;
            Ok(Expr::Un(UnOp::Neg, Box::new(e)))
        }
        // 聚合函数调用。
        Tok::Count | Tok::Sum | Tok::Avg | Tok::Min | Tok::Max => {
            let f = match c.toks.get(c.pos.saturating_sub(1)).unwrap_or(&Tok::Eof) {
                Tok::Count => AggFn::Count,
                Tok::Sum => AggFn::Sum,
                Tok::Avg => AggFn::Avg,
                Tok::Min => AggFn::Min,
                _ => AggFn::Max,
            };
            parse_agg(c, f)
        }
        other => Err(SqlError::Parse(
            c.pos.saturating_sub(1),
            "表达式起始".to_string(),
            format!("{other:?}"),
        )),
    }
}

fn parse_number(n: &str, c: &mut Cursor) -> Result<Expr, SqlError> {
    if n.contains('.') {
        match n.parse::<f64>() {
            Ok(v) => Ok(Expr::Lit(Value::Float(v))),
            Err(_) => Err(SqlError::Parse(c.pos.saturating_sub(1), "浮点字面量".to_string(), n.to_string())),
        }
    } else {
        match n.parse::<i64>() {
            Ok(v) => Ok(Expr::Lit(Value::Int(v))),
            Err(_) => Err(SqlError::Parse(c.pos.saturating_sub(1), "整数字面量".to_string(), n.to_string())),
        }
    }
}

fn parse_agg(c: &mut Cursor, f: AggFn) -> Result<Expr, SqlError> {
    c.eat(&Tok::LParen)?;
    let arg = if c.peek() == &Tok::Mul {
        // COUNT(*)：星号在参数位置由语境截获为 Star。
        c.next();
        Expr::Star
    } else {
        parse_expr(c)?
    };
    c.eat(&Tok::RParen)?;
    Ok(Expr::Agg(f, Box::new(arg)))
}

fn parse_expr_list(c: &mut Cursor) -> Result<Vec<Expr>, SqlError> {
    let mut list = Vec::new();
    loop {
        list.push(parse_expr(c)?);
        if c.peek() == &Tok::Comma {
            c.next();
        } else {
            break;
        }
    }
    Ok(list)
}

fn parse_order_list(c: &mut Cursor) -> Result<Vec<(Expr, bool)>, SqlError> {
    let mut list = Vec::new();
    loop {
        let e = parse_expr(c)?;
        let asc = match c.peek() {
            Tok::Ident(s) if s.eq_ignore_ascii_case("asc") => {
                c.next();
                true
            }
            Tok::Ident(s) if s.eq_ignore_ascii_case("desc") => {
                c.next();
                false
            }
            _ => true,
        };
        list.push((e, asc));
        if c.peek() == &Tok::Comma {
            c.next();
        } else {
            break;
        }
    }
    Ok(list)
}

/// 语法 cell：`In = 词法流` → `Out = Result<语句, SqlError>`。
/// 纯转换（State = ()）；经 `TryChain` 与前/后阶段串接。
pub struct Parser;
impl PortCell for Parser {
    type In = Vec<Tok>;
    type Out = Result<Stmt, SqlError>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), toks: Vec<Tok>) -> Result<Stmt, SqlError> {
        parse(&toks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Value;
    use crate::lexer::lex;

    fn parse_sql(sql: &str) -> Result<Stmt, SqlError> {
        parse(&lex(sql).expect("lex"))
    }

    #[test]
    fn basic_select_with_where_order_limit() {
        let s = parse_sql("SELECT a, b AS x FROM t WHERE a >= 3 ORDER BY b DESC LIMIT 5").unwrap();
        assert_eq!(s.from, "t");
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.items[0].expr.col_name(), Some("a"));
        assert_eq!(s.items[1].alias.as_deref(), Some("x"));
        assert!(matches!(s.where_, Some(Expr::Bin(_, BinOp::Ge, _))));
        assert_eq!(s.order_by.len(), 1);
        assert_eq!(s.order_by[0].1, false, "DESC");
        assert_eq!(s.limit, Some(5));
    }

    #[test]
    fn operator_precedence_is_explicit_in_the_tree() {
        let s = parse_sql("SELECT 2 + 3 * 4 FROM t").unwrap();
        // 2 + (3*4)：根为 Add，右子树为 Mul。
        match &s.items[0].expr {
            Expr::Bin(l, BinOp::Add, r) => {
                assert_eq!(**l, Expr::Lit(Value::Int(2)));
                assert!(matches!(**r, Expr::Bin(_, BinOp::Mul, _)));
            }
            other => panic!("expected Add root, got {other:?}"),
        }
    }

    #[test]
    fn group_by_with_aggregates() {
        let s = parse_sql("SELECT dept, COUNT(*), SUM(salary) FROM e GROUP BY dept").unwrap();
        assert_eq!(s.items.len(), 3);
        assert!(matches!(
            s.items[1].expr,
            Expr::Agg(AggFn::Count, ref arg) if matches!(**arg, Expr::Star)
        ));
        assert!(matches!(s.items[2].expr, Expr::Agg(AggFn::Sum, _)));
        assert_eq!(s.group_by.len(), 1);
    }

    #[test]
    fn boolean_and_not_precedence() {
        let s = parse_sql("SELECT 1 FROM t WHERE NOT a AND b OR c == 2").unwrap();
        // OR 根；左为 AND(NOT a, b)；右为 Eq(c,2)。
        match s.where_.unwrap() {
            Expr::Bin(l, BinOp::Or, r) => {
                assert!(matches!(*l, Expr::Bin(_, BinOp::And, _)));
                assert!(matches!(*r, Expr::Bin(_, BinOp::Eq, _)));
            }
            other => panic!("expected Or root, got {other:?}"),
        }
    }

    #[test]
    fn parse_errors_are_typed_with_position() {
        assert!(matches!(
            parse_sql("SELECT a t"), // 缺 FROM
            Err(SqlError::Parse(..))
        ));
        assert!(matches!(
            parse_sql("SELECT (a FROM t"), // 缺右括号
            Err(SqlError::Parse(..))
        ));
        assert!(matches!(
            parse_sql("SELECT FROM t"), // 缺选择项
            Err(SqlError::Parse(..))
        ));
    }

    #[test]
    fn star_mixed_with_columns_is_rejected() {
        assert!(matches!(
            parse_sql("SELECT *, a FROM t"),
            Err(SqlError::Parse(..))
        ));
    }

    #[test]
    fn parser_cell_conforms_to_its_dual_pair() {
        use axiom::cell_core::{Slot, assert_conforms};
        assert_conforms::<Slot<Vec<Tok>, Result<Stmt, SqlError>>, Parser>();
    }
}