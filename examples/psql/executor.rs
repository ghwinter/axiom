//! SQL executor — the stateful `Machine` that owns the `Database` and runs
//! each `Statement` against it.
//!
//! This is the heart of the psql example. Unlike the lexer and parser (which
//! are stateless `Func`s), the executor must retain the database across
//! invocations, so it is a `Machine` whose `State = Database`.
//!
//! # Port topology
//! ```text
//!                 ┌──────────────┐
//!   Statement ──► │   Executor   │ ──► rows   (ResultSet: query rows / count / msg)
//!                 │  (Database)  │ ──► status (Observe: human-readable status line)
//!                 └──────────────┘
//! ```
//! `MultiOutput` is used because a single `process()` may emit both a result
//! set and a status line — this is a fan-out in axiom's port model.
//!
//! # Core pressure points surfaced here
//! 1. ~~`HasPortInfo` requires `Clone` on the payload~~ — **resolved**:
//!    `HasPortInfo` no longer carries a `Clone` bound — values cross the port
//!    boundary by **move** (Rust's default), so `ResultSet`'s `Clone` is now
//!    optional, not mandatory.
//! 2. ~~`MultiOutput` excludes this machine from `FusedInline`~~ — **resolved**:
//!    this machine now uses `TupleOutput` (fixed two outputs: rows + status),
//!    which is `FusedCompatible` — a data+observe machine can enter the 0-cost
//!    fused pipeline path, and the per-call `Vec` allocation is gone.
//! 3. **Still open**: no streaming port — a million-row `SELECT` must
//!    materialise the full `Vec<Row>` before `process()` returns. A
//!    cursor/streaming model is not expressible in the current
//!    `process() -> ProcessOutput` shape (an application-side cursor state
//!    machine works around it).

use crate::ast::{SelectCols, Statement};
use crate::storage::{Database, Schema};
use axiom::machine::{
    CleanupError, InitError, Machine, MachineHandle, TupleOutput,
};
use axiom::port::{ConfigSchema, MachineContext};
use axiom::declare_ports;

// ════════════════════════════════════════════════════════════════════════════
// Port declaration via the declare_ports! macro.
// ════════════════════════════════════════════════════════════════════════════

declare_ports! {
    #[derive(Debug, Clone, PartialEq)]
    pub struct ExecutorPorts {
        input type ExecutorInput {
            sql [Data] => Statement,
        }
        output type ExecutorOutput {
            rows   [Data]    => ResultSet,
            status [Observe] => String,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// ResultSet — what the executor emits on the `rows` port.
// ════════════════════════════════════════════════════════════════════════════

/// The payload of the `rows` output port.
///
/// `Clone` is optional — axiom's `HasPortInfo` no longer requires it, so
/// result sets cross the port boundary by move. It is kept here for REPL
/// convenience (error paths, assertions); snapshotting needs belong to
/// `Machine::checkpoint`, not to a blanket port obligation.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultSet {
    /// A query returned these rows (with the projected schema header).
    Rows { header: String, rows: Vec<String> },
    /// A DML statement affected this many rows.
    RowsAffected(usize),
    /// A DDL statement succeeded (carry a short message).
    Ok(String),
}

// ════════════════════════════════════════════════════════════════════════════
// Executor Machine
// ════════════════════════════════════════════════════════════════════════════

/// The stateful SQL executor.
pub struct Executor;

impl Machine for Executor {
    type State = Database;
    type Input = ExecutorInput;
    type Output = ExecutorOutput;
    type Ports = ExecutorPorts;
    type ProcessOutput = TupleOutput<ExecutorOutput>;

    fn name() -> &'static str {
        "psql_executor"
    }

    fn config_schema() -> ConfigSchema {
        ConfigSchema::new()
    }

    fn init(_ctx: &MachineContext) -> Result<Database, InitError> {
        Ok(Database::new())
    }

    #[inline]
    fn process(
        state: &mut Database,
        _ctx: &MachineContext,
        input: ExecutorInput,
    ) -> TupleOutput<ExecutorOutput> {
        let stmt = match input {
            ExecutorInput::sql(s) => s,
        };

        let (result, status) = execute(state, stmt);
        TupleOutput::Yield(
            ExecutorOutput::rows(result),
            ExecutorOutput::status(status),
        )
    }

    fn cleanup(_state: Database, _ctx: &MachineContext) -> Result<(), CleanupError> {
        // The database is dropped in place; nothing to release explicitly.
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════════════════
// execute — the free-function interpreter.
// ════════════════════════════════════════════════════════════════════════════

/// Run a statement against the database, returning `(rows_result, status_line)`.
fn execute(db: &mut Database, stmt: Statement) -> (ResultSet, String) {
    match stmt {
        Statement::CreateTable { name, columns } => {
            let mut schema = Schema::new();
            for c in columns {
                schema = schema.with_column(c.name, c.ty);
            }
            match db.create_table(name.clone(), schema) {
                Ok(()) => (
                    ResultSet::Ok(format!("CREATE TABLE")),
                    format!("CREATE TABLE"),
                ),
                Err(e) => (
                    ResultSet::Ok(format!("ERROR: {}", e)),
                    format!("ERROR: {}", e),
                ),
            }
        }

        Statement::Insert { table, columns, values } => {
            let target = match db.get_mut(&table) {
                Some(t) => t,
                None => {
                    let msg = format!("ERROR: unknown table: {}", table);
                    return (ResultSet::Ok(msg.clone()), msg);
                }
            };

            // If a column list was given, reorder/extend values to full width.
            let ordered = match &columns {
                None => values,
                Some(cols) => {
                    let mut full: Vec<crate::ast::Value> =
                        (0..target.schema.arity())
                            .map(|_| crate::ast::Value::Null)
                            .collect();
                    for (i, col_name) in cols.iter().enumerate() {
                        let idx = target.schema.index_of(col_name);
                        match idx {
                            Some(idx) => full[idx] = values.get(i).cloned().unwrap_or(crate::ast::Value::Null),
                            None => {
                                let msg = format!("ERROR: unknown column: {}", col_name);
                                return (ResultSet::Ok(msg.clone()), msg);
                            }
                        }
                    }
                    full
                }
            };

            match target.insert(ordered) {
                Ok(()) => {
                    let n = 1;
                    (
                        ResultSet::RowsAffected(n),
                        format!("INSERT 0 {}", n),
                    )
                }
                Err(e) => (
                    ResultSet::Ok(format!("ERROR: {}", e)),
                    format!("ERROR: {}", e),
                ),
            }
        }

        Statement::Select { columns, table } => {
            let table_ref = match db.get(&table) {
                Some(t) => t,
                None => {
                    let msg = format!("ERROR: unknown table: {}", table);
                    return (ResultSet::Ok(msg.clone()), msg);
                }
            };

            let header = match &columns {
                SelectCols::Star => table_ref.schema.header(),
                SelectCols::Cols(names) => names.join(" | "),
            };

            let mut rendered = Vec::with_capacity(table_ref.rows.len());
            match &columns {
                // B1: Star renders rows directly — no projection clone.
                SelectCols::Star => {
                    for row in &table_ref.rows {
                        rendered.push(row.render());
                    }
                }
                SelectCols::Cols(names) => {
                    // Resolve column indices ONCE per query — the previous
                    // per-row `index_of` was a lowercase String allocation
                    // per column per row.
                    let mut idxs = Vec::with_capacity(names.len());
                    for n in names {
                        let idx = match table_ref.schema.index_of(n) {
                            Some(i) => i,
                            None => {
                                let msg = format!("ERROR: unknown column: {}", n);
                                return (ResultSet::Ok(msg.clone()), msg);
                            }
                        };
                        idxs.push(idx);
                    }
                    for row in &table_ref.rows {
                        rendered.push(table_ref.render_row_by_idx(row, &idxs));
                    }
                }
            }

            let n = rendered.len();
            (
                ResultSet::Rows { header, rows: rendered },
                format!("SELECT {}", n),
            )
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Re-export the handle type alias for the REPL.
// ════════════════════════════════════════════════════════════════════════════

/// Convenience alias: a Running-state handle to the executor, as the REPL
/// holds it between input lines.
pub type ExecutorRunning = MachineHandle<Executor, axiom::machine::Running>;
