//! psql —— SQL REPL 流水线，用 cell_core 四构件 + runtime 错误/短路驱动重建。
//!
//! 健壮性设计（现实问题驱动 runtime）：
//! - `Lexer`/`Parser` 是会失败的 cell：`Out = Result<_, PErr>`（词法/语法错误显露，
//!   而非静默吞掉变成默认语句）；
//! - `Executor` `Out = Result<ExecOut, PErr>`（执行错误，如表不存在）；
//! - 主流程用 runtime 的 `TryChain` 对 Lexer→Parser→Executor 做单层短路：任一级
//!   错误立即停，不流到后续级。这是"错误/失败通路"（`runtime.md` §9.2）在一个
//!   真实 REPL 上的使用。

use std::collections::BTreeMap;

use axiom::cell_core::PortCell;

// ═══════════════════════════════════════════════════════════════
// Token / Stmt / 错误 / 结果类型
// ═══════════════════════════════════════════════════════════════

/// SQL 词法单元。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Ident(String),
    Number(i64),
    Str(String),
    Comma,
    Semi,
    LParen,
    RParen,
    // 关键字
    Create, Table, Insert, Into, Values, Select, From, Star,
}

/// 语句。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    CreateTable { name: String, cols: Vec<String> },
    Insert { table: String, values: Vec<i64> },
    Select { table: String, cols: Vec<String> },
}

/// 词法/语法/执行错误（现实 REPL 的一等失败语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PErr {
    /// 字符串字面量未闭合（词法）。
    UnterminatedString,
    /// 无法识别的字符（词法）。
    UnexpectedChar(char),
    /// 以无法识别的语句开头 / 空语句（语法）。
    UnknownStatement,
    /// 语句缺少表名（语法）。
    MissingTable,
    /// INSERT 缺少 VALUES（语法）。
    MissingValues,
    /// 引用了不存在的表（执行）。
    NoSuchTable(String),
}

/// 执行成功输出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecOut {
    Ok(String),
    Rows(Vec<Vec<i64>>),
}

// ═══════════════════════════════════════════════════════════════
// Lexer —— SQL 文本 → Result<Tokens, PErr>（无状态、会失败）
// ═══════════════════════════════════════════════════════════════

pub struct Lexer;
impl PortCell for Lexer {
    type In = String;
    type Out = Result<Vec<Token>, PErr>;
    type State = ();
    fn step(_: &mut (), sql: String) -> Result<Vec<Token>, PErr> {
        let mut toks = Vec::new();
        let mut chars = sql.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ';' {
                chars.next();
                continue;
            }
            if c.is_ascii_digit() {
                let mut n = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() { n.push(d); chars.next(); } else { break; }
                }
                toks.push(Token::Number(n.parse().map_err(|_| PErr::UnexpectedChar(c))?));
            } else if c == '\'' {
                chars.next();
                let mut s = String::new();
                let mut closed = false;
                while let Some(&d) = chars.peek() {
                    if d == '\'' { chars.next(); closed = true; break; }
                    else { s.push(d); chars.next(); }
                }
                if !closed { return Err(PErr::UnterminatedString); }
                toks.push(Token::Str(s));
            } else if c == ',' { toks.push(Token::Comma); chars.next(); }
            else if c == '(' { toks.push(Token::LParen); chars.next(); }
            else if c == ')' { toks.push(Token::RParen); chars.next(); }
            else if c == '*' { toks.push(Token::Star); chars.next(); }
            else if c.is_alphabetic() || c == '_' {
                let mut w = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' { w.push(d); chars.next(); } else { break; }
                }
                let lw = w.to_lowercase();
                toks.push(match lw.as_str() {
                    "create" => Token::Create, "table" => Token::Table,
                    "insert" => Token::Insert, "into" => Token::Into,
                    "values" => Token::Values, "select" => Token::Select,
                    "from" => Token::From, "star" => Token::Star,
                    _ => Token::Ident(w),
                });
            } else {
                // 其它不可识别字符：词法错误。
                return Err(PErr::UnexpectedChar(c));
            }
        }
        toks.push(Token::Semi);
        Ok(toks)
    }
}

// ═══════════════════════════════════════════════════════════════
// Parser —— Tokens → Result<Stmt, PErr>（无状态、会失败）
// ═══════════════════════════════════════════════════════════════

pub struct Parser;
impl PortCell for Parser {
    type In = Vec<Token>;
    type Out = Result<Stmt, PErr>;
    type State = ();
    fn step(_: &mut (), toks: Vec<Token>) -> Result<Stmt, PErr> {
        match toks.first() {
            Some(Token::Create) => {
                // CREATE TABLE name (col, col, ...)
                let name = toks.iter().find_map(
                    |t| if let Token::Ident(n) = t { Some(n.clone()) } else { None });
                let name = name.ok_or(PErr::MissingTable)?;
                let mut cols = Vec::new();
                let mut in_paren = false;
                for t in &toks {
                    match t {
                        Token::LParen => in_paren = true,
                        Token::RParen => in_paren = false,
                        Token::Ident(n) if in_paren => cols.push(n.clone()),
                        _ => {}
                    }
                }
                Ok(Stmt::CreateTable { name, cols })
            }
            Some(Token::Insert) => {
                // INSERT INTO table VALUES (v, v, ...)
                let table = toks.iter().find_map(
                    |t| if let Token::Ident(n) = t { if !n.is_empty()
                        && n.to_lowercase() != "into" { Some(n.clone()) } else { None } }
                    else { None });
                let table = table.ok_or(PErr::MissingTable)?;
                let values: Vec<i64> = toks.iter().filter_map(
                    |t| if let Token::Number(v) = t { Some(*v) } else { None }).collect();
                if values.is_empty() { return Err(PErr::MissingValues); }
                Ok(Stmt::Insert { table, values })
            }
            Some(Token::Select) => {
                // SELECT col, ... FROM table
                let mut cols = Vec::new();
                let mut table = String::new();
                let mut after = false;
                for t in &toks {
                    match t {
                        Token::Ident(n) => {
                            if after && table.is_empty() { table = n.clone(); }
                            else if !after && n != "select" && n != "from" { cols.push(n.clone()); }
                        }
                        Token::Star => cols.push("*".to_string()),
                        Token::From => after = true,
                        _ => {}
                    }
                }
                if table.is_empty() { return Err(PErr::MissingTable); }
                Ok(Stmt::Select { table, cols })
            }
            _ => Err(PErr::UnknownStatement),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Database / Executor —— 有状态、会失败
// ═══════════════════════════════════════════════════════════════

/// 微型数据库：表名 → (列名, 行)。
#[derive(Default)]
pub struct Database {
    /// name -> (columns, rows)
    pub tables: BTreeMap<String, (Vec<String>, Vec<Vec<i64>>)>,
}

/// 执行语句。State = Database（有状态皮层）。
pub struct Executor;
impl PortCell for Executor {
    type In = Stmt;
    type Out = Result<ExecOut, PErr>;
    type State = Database;
    fn step(db: &mut Database, stmt: Stmt) -> Result<ExecOut, PErr> {
        match stmt {
            Stmt::CreateTable { name, cols } => {
                db.tables.insert(name.clone(), (cols.clone(), vec![]));
                Ok(ExecOut::Ok(format!("CREATE TABLE {name} ({} cols)", cols.len())))
            }
            Stmt::Insert { table, values } => {
                let entry = db.tables.get_mut(&table);
                match entry {
                    Some((_, rows)) => {
                        rows.push(values);
                        Ok(ExecOut::Ok(format!("INSERT 1 row into {table}")))
                    }
                    None => Err(PErr::NoSuchTable(table)),
                }
            }
            Stmt::Select { table, cols: _ } => {
                match db.tables.get(&table) {
                    Some((_, rows)) => Ok(ExecOut::Rows(rows.to_vec())),
                    None => Err(PErr::NoSuchTable(table)),
                }
            }
        }
    }
}
