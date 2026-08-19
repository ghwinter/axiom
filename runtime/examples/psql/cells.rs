//! psql —— SQL REPL 流水线，用 cell_core 四构件 + runtime Carrier 重建。
//!
//! 对应旧 psql（Lexer → Parser → Executor 流水线），但用新核心表达：
//! - `Lexer`：无状态 `PortCell`（SQL 文本 → Tokens）；
//! - `Parser`：无状态 `PortCell`（Tokens → Stmt）；
//! - `Executor`：有状态 `PortCell`（State = Database，执行 Stmt → 结果）。
//!
//! 旧 lexer/parser 是无状态 Func、executor 是有状态 Machine；在新核心中三者都是
//! `PortCell`（轴)，差异只在是否有 `State`——"开放系统"统一了这一区别。

use std::collections::BTreeMap;

use axiom::cell_core::PortCell;

// ═══════════════════════════════════════════════════════════════
// Token / Stmt / 结果类型
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

/// 执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Result {
    Ok(String),
    Rows(Vec<Vec<i64>>),
    Error(String),
}

// ═══════════════════════════════════════════════════════════════
// Lexer —— SQL 文本 → Tokens（无状态）
// ═══════════════════════════════════════════════════════════════

pub struct Lexer;
impl PortCell for Lexer {
    type In = String;
    type Out = Vec<Token>;
    type State = ();
    fn step(_: &mut (), sql: String) -> Vec<Token> {
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
                toks.push(Token::Number(n.parse().unwrap()));
            } else if c == '\'' {
                chars.next();
                let mut s = String::new();
                while let Some(&d) = chars.peek() {
                    if d == '\'' { chars.next(); break; } else { s.push(d); chars.next(); }
                }
                toks.push(Token::Str(s));
            } else if c == ',' { toks.push(Token::Comma); chars.next(); }
            else if c == '(' { toks.push(Token::LParen); chars.next(); }
            else if c == ')' { toks.push(Token::RParen); chars.next(); }
            else {
                let mut w = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' { w.push(d); chars.next(); } else { break; }
                }
                let is_empty = w.is_empty();
                let lw = w.to_lowercase();
                toks.push(match lw.as_str() {
                    "create" => Token::Create, "table" => Token::Table,
                    "insert" => Token::Insert, "into" => Token::Into,
                    "values" => Token::Values, "select" => Token::Select,
                    "from" => Token::From, "star" => Token::Star,
                    _ => Token::Ident(w),
                });
                if is_empty { chars.next(); } // 跳过未知字符
            }
        }
        toks.push(Token::Semi);
        toks
    }
}

// ═══════════════════════════════════════════════════════════════
// Parser —— Tokens → Stmt（无状态）
// ═══════════════════════════════════════════════════════════════

pub struct Parser;
impl PortCell for Parser {
    type In = Vec<Token>;
    type Out = Stmt;
    type State = ();
    fn step(_: &mut (), toks: Vec<Token>) -> Stmt {
        match toks.first() {
            Some(Token::Create) => {
                // CREATE TABLE name (col, col, ...)
                let mut name = String::new();
                for t in &toks {
                    match t { Token::Ident(n) => { name = n.clone(); break; } _ => {} }
                }
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
                Stmt::CreateTable { name, cols }
            }
            Some(Token::Insert) => {
                // INSERT INTO table VALUES (v, v, ...)
                let mut table = String::new();
                let mut values = Vec::new();
                for t in &toks {
                    match t {
                        Token::Ident(n) if !n.is_empty() && table.is_empty() => table = n.clone(),
                        Token::Number(v) => values.push(*v),
                        _ => {}
                    }
                }
                Stmt::Insert { table, values }
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
                Stmt::Select { table, cols }
            }
            _ => Stmt::Select { table: String::new(), cols: vec![] },
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Database / Executor —— 有状态
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
    type Out = Result;
    type State = Database;
    fn step(db: &mut Database, stmt: Stmt) -> Result {
        match stmt {
            Stmt::CreateTable { name, cols } => {
                db.tables.insert(name.clone(), (cols.clone(), vec![]));
                Result::Ok(format!("CREATE TABLE {name} ({} cols)", cols.len()))
            }
            Stmt::Insert { table, values } => {
                let entry = db.tables.get_mut(&table);
                match entry {
                    Some((_, rows)) => {
                        rows.push(values.clone());
                        Result::Ok(format!("INSERT 1 row into {table}"))
                    }
                    None => Result::Error(format!("no table {table}")),
                }
            }
            Stmt::Select { table, cols: _ } => {
                match db.tables.get(&table) {
                    Some((_, rows)) => {
                        // 返回所有行的全部列值（简化）。
                        let out = rows.iter().map(|r| r.clone()).collect();
                        Result::Rows(out)
                    }
                    None => Result::Error(format!("no table {table}")),
                }
            }
        }
    }
}
