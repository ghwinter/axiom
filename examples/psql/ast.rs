//! SQL AST — the shared data structures for the psql-axiom example.
//!
//! These types live at the boundary between the `Parser` (which produces them)
//! and the `Executor` (which consumes them). They are pure data — no behaviour,
//! no ports, no axiom traits. Keeping them axiom-free means the SQL layer could
//! in principle be reused under a different runtime.

use core::fmt;

// ════════════════════════════════════════════════════════════════════════════
// Value — the runtime scalar carried by rows and literals.
// ════════════════════════════════════════════════════════════════════════════

/// A scalar value stored in a cell.
///
/// `Clone` is optional — axiom's `HasPortInfo` no longer requires payload
/// types to be `Clone`, so values cross the port boundary by **move**. It is
/// kept here for REPL convenience (error paths, assertions) and snapshotting
/// needs (which belong to `Machine::checkpoint`, not to a blanket port
/// obligation).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Text(String),
    Null,
}

impl Value {
    /// Coerce to the target SQL type, returning `Null` on mismatch.
    pub fn coerce_to(self, ty: SqlType) -> Value {
        match (self, ty) {
            (v @ Value::Int(_), SqlType::Int) => v,
            (v @ Value::Text(_), SqlType::Text) => v,
            (Value::Int(n), SqlType::Text) => Value::Text(n.to_string()),
            (Value::Text(s), SqlType::Int) => {
                s.parse::<i64>().map(Value::Int).unwrap_or(Value::Null)
            }
            (Value::Null, _) => Value::Null,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "INT",
            Value::Text(_) => "TEXT",
            Value::Null => "NULL",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Text(s) => write!(f, "{}", s),
            Value::Null => write!(f, "NULL"),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// SqlType — the column type declared in DDL.
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlType {
    Int,
    Text,
}

impl SqlType {
    pub fn name(&self) -> &'static str {
        match self {
            SqlType::Int => "INT",
            SqlType::Text => "TEXT",
        }
    }

    /// Parse a type name token (case-insensitive) into a `SqlType`.
    pub fn from_keyword(kw: &str) -> Option<SqlType> {
        match kw.to_ascii_uppercase().as_str() {
            "INT" | "INTEGER" => Some(SqlType::Int),
            "TEXT" | "VARCHAR" | "STRING" => Some(SqlType::Text),
            _ => None,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Statement — the top-level AST node.
// ════════════════════════════════════════════════════════════════════════════

/// One SQL statement.
///
/// The grammar implemented here is a deliberately tiny subset of psql's DDL/DML:
/// ```sql
/// CREATE TABLE name ( col TYPE [, col TYPE ...] ) ;
/// INSERT INTO name VALUES ( v [, v ...] ) ;
/// SELECT * FROM name ;
/// SELECT col [, col ...] FROM name ;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>, // None = positional, all columns
        values: Vec<Value>,
    },
    Select {
        columns: SelectCols,
        table: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub ty: SqlType,
}

/// Which columns a `SELECT` projects.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectCols {
    /// `SELECT *` — project all columns in declared order.
    Star,
    /// `SELECT a, b, c` — project the named columns in the given order.
    Cols(Vec<String>),
}
