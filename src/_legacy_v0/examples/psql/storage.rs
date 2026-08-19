//! In-memory storage layer — Schema, Row, Table, Database.
//!
//! This is the Executor's `State`. It is plain Rust data with no axiom
//! dependencies, so it could be unit-tested in isolation and swapped for a
//! disk-backed store without touching the Machine trait surface.

use crate::ast::{SqlType, Value};
use std::collections::HashMap;

// ════════════════════════════════════════════════════════════════════════════
// Schema — the declared column layout of a table.
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub columns: Vec<(String, SqlType)>,
    /// Case-insensitive column-name → index lookup (O(1)).
    /// Maintained by `with_column`; do not mutate `columns` directly.
    index: std::collections::HashMap<String, usize>,
}

impl Schema {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            index: std::collections::HashMap::new(),
        }
    }

    pub fn with_column(mut self, name: impl Into<String>, ty: SqlType) -> Self {
        let name = name.into();
        self.index.insert(name.to_ascii_lowercase(), self.columns.len());
        self.columns.push((name, ty));
        self
    }

    /// Look up a column's index by name (case-insensitive), or `None`.
    /// O(1) via the name index (was O(columns) linear scan).
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.index.get(&name.to_ascii_lowercase()).copied()
    }

    pub fn arity(&self) -> usize {
        self.columns.len()
    }

    /// Render the column header row, e.g. ` id | name `.
    pub fn header(&self) -> String {
        self.columns
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Row — a single tuple.
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub values: Vec<Value>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    /// Render the row as ` v1 | v2 | ... `, matching `Schema::header` layout.
    ///
    /// Single-buffer rendering with a capacity estimate sized to the actual
    /// cell contents — one `String` allocation per row, no realloc growth.
    /// (The previous `collect::<Vec<_>>().join(...)` allocated per-cell
    /// strings plus the join result — 2n allocations per row.)
    pub fn render(&self) -> String {
        use std::fmt::Write;
        let cap: usize = self
            .values
            .iter()
            .map(|v| match v {
                Value::Int(_) => 16,
                Value::Text(s) => s.len() + 4,
                Value::Null => 4,
            })
            .sum::<usize>()
            + 2;
        let mut out = String::with_capacity(cap);
        for (i, v) in self.values.iter().enumerate() {
            if i > 0 {
                out.push_str(" | ");
            }
            let _ = write!(out, "{}", v);
        }
        out
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Table — a named, schema-bound collection of rows.
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct Table {
    pub schema: Schema,
    pub rows: Vec<Row>,
}

impl Table {
    pub fn new(schema: Schema) -> Self {
        Self { schema, rows: Vec::new() }
    }

    /// Append a row after coercing each value to its declared column type.
    /// Returns `Err` if the arity mismatches the schema.
    pub fn insert(&mut self, values: Vec<Value>) -> Result<(), String> {
        if values.len() != self.schema.arity() {
            return Err(format!(
                "arity mismatch: expected {} columns, got {} values",
                self.schema.arity(),
                values.len()
            ));
        }
        let coerced: Vec<Value> = values
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                let (_, ty) = &self.schema.columns[i];
                v.coerce_to(*ty)
            })
            .collect();
        self.rows.push(Row::new(coerced));
        Ok(())
    }

    /// Project a row to the selected columns, returning a new Row.
    /// For `Star`, returns the row unchanged.
    ///
    /// NOTE: the SELECT hot path uses [`render_row`](Self::render_row)
    /// instead — it renders the projection directly without materialising
    /// an intermediate `Row` (a `Star` projection would clone the whole row).
    pub fn project(&self, row: &Row, cols: &crate::ast::SelectCols) -> Result<Row, String> {
        match cols {
            crate::ast::SelectCols::Star => Ok(row.clone()),
            crate::ast::SelectCols::Cols(names) => {
                let mut out = Vec::with_capacity(names.len());
                for n in names {
                    let idx = self.schema.index_of(n).ok_or_else(|| {
                        format!("unknown column: {}", n)
                    })?;
                    out.push(row.values[idx].clone());
                }
                Ok(Row::new(out))
            }
        }
    }

    /// Render a row by pre-resolved column indices — the SELECT hot path.
    /// One `String` allocation per row, no per-row column lookups, no
    /// intermediate `Row` materialisation.
    pub fn render_row_by_idx(&self, row: &Row, idxs: &[usize]) -> String {
        use std::fmt::Write;
        let cap: usize = idxs
            .iter()
            .map(|&i| match &row.values[i] {
                Value::Int(_) => 16,
                Value::Text(s) => s.len() + 4,
                Value::Null => 4,
            })
            .sum::<usize>()
            + 2;
        let mut out = String::with_capacity(cap);
        for (i, &idx) in idxs.iter().enumerate() {
            if i > 0 {
                out.push_str(" | ");
            }
            let _ = write!(out, "{}", row.values[idx]);
        }
        out
    }

    /// Render a row with the given projection directly into a `String`.
    /// Convenience wrapper over [`render_row_by_idx`](Self::render_row_by_idx);
    /// resolves column indices per call (fine for one-off use, not the
    /// SELECT hot path).
    pub fn render_row(&self, row: &Row, cols: &crate::ast::SelectCols) -> Result<String, String> {
        match cols {
            crate::ast::SelectCols::Star => Ok(row.render()),
            crate::ast::SelectCols::Cols(names) => {
                let mut idxs = Vec::with_capacity(names.len());
                for n in names {
                    let idx = self
                        .schema
                        .index_of(n)
                        .ok_or_else(|| format!("unknown column: {}", n))?;
                    idxs.push(idx);
                }
                Ok(self.render_row_by_idx(row, &idxs))
            }
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Database — the top-level state container, owned by the Executor Machine.
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Default)]
pub struct Database {
    /// Keyed by **lowercased** table name (case-insensitive O(1) lookup).
    pub tables: HashMap<String, Table>,
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }

    /// Case-insensitive table lookup, O(1) (was O(tables) linear scan).
    pub fn get(&self, name: &str) -> Option<&Table> {
        self.tables.get(&name.to_ascii_lowercase())
    }

    /// Case-insensitive mutable table lookup, O(1).
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Table> {
        self.tables.get_mut(&name.to_ascii_lowercase())
    }

    /// Create a table; returns `Err` if a table with this name already exists
    /// (case-insensitive).
    pub fn create_table(&mut self, name: String, schema: Schema) -> Result<(), String> {
        let key = name.to_ascii_lowercase();
        if self.tables.contains_key(&key) {
            return Err(format!("table already exists: {}", name));
        }
        self.tables.insert(key, Table::new(schema));
        Ok(())
    }

    /// Drop a table by name (case-insensitive); returns `Err` if not found.
    pub fn drop_table(&mut self, name: &str) -> Result<(), String> {
        let key = name.to_ascii_lowercase();
        if self.tables.remove(&key).is_none() {
            return Err(format!("unknown table: {}", name));
        }
        Ok(())
    }
}
