//! sqlmini 词法阶段：SQL 文本 → Token 流。一个纯转换 cell（`State = ()`）。
//!
//! 失败为值：非法字符/未闭合字符串返回 [`SqlError::Lex`]（带位置），
//! 不静默跳过（保守：宁缺毋滥，返回错误而非猜测）。

use axiom::cell_core::PortCell;

use crate::errors::SqlError;

/// SQL 词法单元。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// 标识符（含关键字以下列变体分派）。
    Ident(String),
    /// 数值字面量（原文保留；语义期解析为 Int/Float）。
    Num(String),
    /// 字符串字面量（已去引号）。
    Str(String),
    // ── 关键字 ──
    Select,
    From,
    Where,
    Group,
    By,
    Order,
    Limit,
    As,
    Count,
    Sum,
    Avg,
    Min,
    Max,
    Distinct,
    And,
    Or,
    Not,
    // ── 符号 ──
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Mul,
    Div,
    Comma,
    Dot,
    LParen,
    RParen,
    Semi,
    Eof,
}

/// 关键字 → 单元（大小写不敏感；标识符优先检查）。
fn keyword(s: &str) -> Option<Tok> {
    Some(match s.to_ascii_uppercase().as_str() {
        "SELECT" => Tok::Select,
        "FROM" => Tok::From,
        "WHERE" => Tok::Where,
        "GROUP" => Tok::Group,
        "BY" => Tok::By,
        "ORDER" => Tok::Order,
        "LIMIT" => Tok::Limit,
        "AS" => Tok::As,
        "COUNT" => Tok::Count,
        "SUM" => Tok::Sum,
        "AVG" => Tok::Avg,
        "MIN" => Tok::Min,
        "MAX" => Tok::Max,
        "DISTINCT" => Tok::Distinct,
        "AND" => Tok::And,
        "OR" => Tok::Or,
        "NOT" => Tok::Not,
        _ => return None,
    })
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 词法主函数（纯函数；cell 包装见下）。
pub fn lex(sql: &str) -> Result<Vec<Tok>, SqlError> {
    let mut toks = Vec::new();
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if is_ident_start(c) {
            let start = i;
            while i < chars.len() && is_ident_part(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            toks.push(keyword(&word).unwrap_or(Tok::Ident(word)));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            toks.push(Tok::Num(chars[start..i].iter().collect()));
            continue;
        }
        if c == '\'' {
            // 字符串字面量：支持 '' 转义。
            let mut s = String::new();
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        s.push('\'');
                        i += 2;
                        continue;
                    }
                    closed = true;
                    i += 1;
                    break;
                }
                s.push(chars[i]);
                i += 1;
            }
            if !closed {
                return Err(SqlError::Lex(i, "unterminated string literal".to_string()));
            }
            toks.push(Tok::Str(s));
            continue;
        }
        // 双字符运算符。
        if i + 1 < chars.len() {
            let two: String = chars[i..i + 2].iter().collect();
            match two.as_str() {
                "==" => {
                    toks.push(Tok::Eq);
                    i += 2;
                    continue;
                }
                "!=" => {
                    toks.push(Tok::Ne);
                    i += 2;
                    continue;
                }
                "<=" => {
                    toks.push(Tok::Le);
                    i += 2;
                    continue;
                }
                ">=" => {
                    toks.push(Tok::Ge);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        match c {
            '=' => toks.push(Tok::Eq),
            '<' => toks.push(Tok::Lt),
            '>' => toks.push(Tok::Gt),
            '+' => toks.push(Tok::Plus),
            '-' => toks.push(Tok::Minus),
            '*' => toks.push(Tok::Mul), // 星号统一为乘法符号；SELECT 顶层 / COUNT(*) 由解析语境截获
            '/' => toks.push(Tok::Div),
            ',' => toks.push(Tok::Comma),
            '.' => toks.push(Tok::Dot),
            '(' => toks.push(Tok::LParen),
            ')' => toks.push(Tok::RParen),
            ';' => toks.push(Tok::Semi),
            _ => return Err(SqlError::Lex(i, c.to_string())),
        }
        i += 1;
    }
    toks.push(Tok::Eof);
    Ok(toks)
}

/// 词法 cell：`In = 查询文本` → `Out = Result<词法流, SqlError>`。
/// 纯转换（State = ()）；经 `TryChain` 与语法阶段串接。
pub struct Lexer;
impl PortCell for Lexer {
    type In = String;
    type Out = Result<Vec<Tok>, SqlError>;
    type State = ();
    #[inline(always)]
    fn step(_: &mut (), sql: String) -> Result<Vec<Tok>, SqlError> {
        lex(&sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_ok(sql: &str) -> Vec<Tok> {
        lex(sql).expect("lex should succeed")
    }

    #[test]
    fn keywords_and_identifiers_are_case_insensitive() {
        let toks = lex_ok("SELECT name FROM t");
        assert_eq!(
            toks,
            vec![
                Tok::Select,
                Tok::Ident("name".to_string()),
                Tok::From,
                Tok::Ident("t".to_string()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn numeric_and_string_literals_keep_lexemes() {
        let toks = lex_ok("SELECT 'a''b', 3.14, -2");
        assert_eq!(
            toks,
            vec![
                Tok::Select,
                Tok::Str("a'b".to_string()),
                Tok::Comma,
                Tok::Num("3.14".to_string()),
                Tok::Comma,
                Tok::Minus,
                Tok::Num("2".to_string()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn operators_and_punctuation() {
        let toks = lex_ok("a==b AND c>=1 OR NOT d!=2");
        assert_eq!(
            toks,
            vec![
                Tok::Ident("a".into()),
                Tok::Eq,
                Tok::Ident("b".into()),
                Tok::And,
                Tok::Ident("c".into()),
                Tok::Ge,
                Tok::Num("1".into()),
                Tok::Or,
                Tok::Not,
                Tok::Ident("d".into()),
                Tok::Ne,
                Tok::Num("2".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn lexical_errors_are_typed_values() {
        // 非法字符：带位置、带值，不静默。
        let err = lex("SELECT @").expect_err("lex should fail");
        assert_eq!(err.stage(), "lex");
        match err {
            SqlError::Lex(pos, ch) => {
                assert_eq!(ch, "@");
                assert!(pos > 0);
            }
            other => panic!("expected Lex, got {other:?}"),
        }
        // 未闭合字符串。
        assert!(matches!(
            lex("SELECT 'oops"),
            Err(SqlError::Lex(..))
        ));
    }

    #[test]
    fn lexer_cell_drives_via_try_chain_shape() {
        // cell 形态（TryChain 对 `Ok` 直通）：词法阶段可直接经 `Conforms` 判定。
        use axiom::cell_core::{Slot, assert_conforms};
        assert_conforms::<Slot<String, Result<Vec<Tok>, SqlError>>, Lexer>();
        // 直接驱动。
        let out = <Lexer as PortCell>::step(&mut (), "SELECT 1".to_string());
        assert!(matches!(out, Ok(ref t) if t.len() == 3));
    }
}