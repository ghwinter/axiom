//! psql-axiom — a tiny psql-like REPL built on axiom's `Func` + `Machine`.
//!
//! # Topology
//! ```text
//!   stdin line ──► LexerFunc ──► ParserFunc ──► Executor (Machine) ──► stdout
//!                  (Func)         (Func)        (State = Database)
//! ```
//! The lexer and parser are stateless `Func`s (pure computation); the executor
//! is a stateful `Machine` that owns the `Database` and persists across input
//! lines. The REPL loop lives here in `main` because axiom core deliberately
//! ships no runtime driver — this is the most visible core pressure point:
//! there is no `static_path::pipeline2` or `run_loop(input_rx)` helper, so we drive
//! the typestate `MachineHandle` by hand.
//!
//! # Run
//! ```bash
//! cargo run --example psql
//! ```

// This example defines a public API surface (storage methods, parser helpers,
// port enums) that is not yet fully exercised by the REPL. The dead-code
// warnings are expected for a work-in-progress example and would be resolved
// as more SQL features are added.
#![allow(dead_code)]

mod ast;
mod executor;
mod lexer;
mod parser;
mod storage;

use std::io::{self, BufRead, Write};

use axiom::func::{Func, FuncRef};
use axiom::machine::{Init, MachineHandle, TupleOutput};
use axiom::port::MachineContext;

use executor::{Executor, ExecutorInput, ExecutorOutput, ResultSet};
use lexer::LexerFunc;
use parser::ParserFunc;

// ════════════════════════════════════════════════════════════════════════════
// REPL
// ════════════════════════════════════════════════════════════════════════════

fn main() {
    if std::env::args().any(|a| a == "--bench") {
        run_bench();
        return;
    }
    println!("psql-axiom 0.1 — axiom Func + Machine demo");
    println!("Type SQL (CREATE TABLE / INSERT / SELECT). 'exit' to quit.\n");

    // ── Construct the Executor as a long-lived Machine ───────────────────
    //
    // axiom core provides only the `MachineHandle` typestate driver. There is
    // no convenience `static_path::pipeline2` that hides the init→start→stop→
    // finish→cleanup ceremony. For a long-running REPL this is acceptable
    // (one-time setup), but it is the first thing a real runtime adapter
    // would wrap.
    let ctx = MachineContext::new("psql_executor");
    let handle = MachineHandle::<Executor, Init>::new(ctx).expect("executor init");
    let mut running = handle.start();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        // ── Read a line ──────────────────────────────────────────────────
        write!(stdout, "psql> ").ok();
        stdout.flush().ok();

        let mut line = String::new();
        let n = stdin.lock().read_line(&mut line).unwrap_or(0);
        if n == 0 {
            // EOF (Ctrl-D)
            println!();
            break;
        }
        // Borrow the trimmed view for command checks; keep the original
        // String alive for the zero-copy `call_ref` path below (A4).
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
            break;
        }
        // Allow `\q` like real psql.
        if trimmed == "\\q" {
            break;
        }

        // ── Stage 1: Lexer (FuncRef) ─────────────────────────────────────
        //
        // A4: `call_ref` borrows the read_line buffer — the lexer only reads
        // it, so no per-line `to_string()` copy is needed (the owned
        // `Func::call` path remains for non-borrowing drivers).
        let tokens = match LexerFunc::call_ref(&line) {
            Ok(t) => t,
            Err(e) => {
                println!("lex error: {}", e);
                continue;
            }
        };

        // ── Stage 2: Parser (FuncRef) ─────────────────────────────────────
        //
        // A4: `call_ref` borrows the token slice — the parser does not move
        // the `Vec<Token>` produced by the lexer. (A fused pipeline would
        // chain both `call_ref`s into one expression; the REPL stages them
        // by hand.)
        let stmt = match ParserFunc::call_ref(&tokens) {
            Ok(s) => s,
            Err(e) => {
                println!("parse error: {}", e);
                continue;
            }
        };

        // ── Stage 3: Executor (Machine) ──────────────────────────────────
        //
        // The executor is a long-lived `Machine`. Each `process()` call takes
        // one `Statement` and returns a `MultiOutput` (fan-out: rows + status).
        // The state (Database) persists across calls.
        let out = running.process(ExecutorInput::sql(stmt));
        render_output(out);
    }

    // ── Shutdown ceremony ────────────────────────────────────────────────
    //
    // The full typestate sequence is stop → finish → cleanup. This is the
    // fourth core pressure point: even for a REPL that just wants to drop the
    // database and exit, the typestate forces three explicit transitions. A
    // runtime adapter would encapsulate this in a `Drop`-like guard
    // (the historical `CleanupGuard` did exactly this).
    let stopping = running.stop();
    let stopped = stopping.finish();
    stopped.cleanup().expect("executor cleanup");
}

// ════════════════════════════════════════════════════════════════════════════
// Output rendering
// ════════════════════════════════════════════════════════════════════════════

fn render_output(out: TupleOutput<ExecutorOutput>) {
    match out {
        TupleOutput::Yield(a, b) => {
            render_one(&a);
            render_one(&b);
        }
        TupleOutput::Idle => {}
        TupleOutput::Done => {}
    }
}

fn render_one(o: &ExecutorOutput) {
    match o {
        ExecutorOutput::rows(rs) => match rs {
            ResultSet::Rows { header, rows } => {
                // Only SELECT produces a table to print; the status port
                // already carries the "SELECT N" tag.
                println!("{}", header);
                println!("{}", "-".repeat(header.len().max(8)));
                for r in rows {
                    println!("{}", r);
                }
            }
            // RowsAffected / Ok are the structured form of what the `status`
            // observe port already prints as a command tag. In a real
            // deployment these would route to a metrics collector, not stdout
            // — the REPL prints only the status line for DDL/DML.
            ResultSet::RowsAffected(_) | ResultSet::Ok(_) => {}
        },
        ExecutorOutput::status(s) => {
            println!("{}", s);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Performance probe — `psql --bench`
// ════════════════════════════════════════════════════════════════════════════
//
// Measures heap allocations and elapsed time for a SELECT over a warm table,
// with the row-rendering optimisations applied (B1: direct projection render,
// B2: single-buffer row render, B3: O(1) table lookup, B4: O(1) column lookup).

mod alloc_count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub static COUNT: AtomicUsize = AtomicUsize::new(0);
    pub static BYTES: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            COUNT.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            COUNT.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(new_size, Ordering::Relaxed);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }
}

#[global_allocator]
static ALLOCATOR: alloc_count::Counting = alloc_count::Counting;

fn run_bench() {
    use crate::ast::{ColumnDef, SelectCols, SqlType, Statement, Value};
    use axiom::machine::{Init, MachineHandle, TupleOutput};
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    const N: usize = 50_000;

    let ctx = MachineContext::new("psql_bench");
    let handle = MachineHandle::<Executor, Init>::new(ctx).expect("executor init");
    let mut running = handle.start();

    // DDL
    let create = Statement::CreateTable {
        name: "t".into(),
        columns: vec![
            ColumnDef { name: "id".into(), ty: SqlType::Int },
            ColumnDef { name: "name".into(), ty: SqlType::Text },
        ],
    };
    running.process(ExecutorInput::sql(create));

    // Warm-up inserts — stabilise HashMap/Vec capacities so the measured
    // SELECT sees no growth allocations from the storage layer.
    for i in 0..N {
        let stmt = Statement::Insert {
            table: "t".into(),
            columns: None,
            values: vec![
                Value::Int(i as i64),
                Value::Text(format!("name_{}", i)),
            ],
        };
        running.process(ExecutorInput::sql(stmt));
    }

    // ── Probe 1: SELECT * ────────────────────────────────────────────────
    alloc_count::COUNT.store(0, Ordering::Relaxed);
    alloc_count::BYTES.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let sel = Statement::Select { columns: SelectCols::Star, table: "t".into() };
    let out = running.process(ExecutorInput::sql(sel));
    let dt = t0.elapsed();
    let allocs = alloc_count::COUNT.load(Ordering::Relaxed);
    let bytes = alloc_count::BYTES.load(Ordering::Relaxed);
    let rows = match out {
        TupleOutput::Yield(ExecutorOutput::rows(ResultSet::Rows { rows, .. }), _) => rows.len(),
        _ => 0,
    };
    println!("=== psql SELECT * probe ({} rows) ===", rows);
    println!("  elapsed : {:?}", dt);
    println!("  allocs  : {}  ({:.2} per row)", allocs, allocs as f64 / rows.max(1) as f64);
    println!("  bytes   : {}  ({:.2} per row)", bytes, bytes as f64 / rows.max(1) as f64);

    // ── Probe 2: SELECT id, name (projection) ────────────────────────────
    alloc_count::COUNT.store(0, Ordering::Relaxed);
    alloc_count::BYTES.store(0, Ordering::Relaxed);
    let t1 = Instant::now();
    let sel2 = Statement::Select {
        columns: SelectCols::Cols(vec!["id".into(), "name".into()]),
        table: "t".into(),
    };
    let out2 = running.process(ExecutorInput::sql(sel2));
    let dt2 = t1.elapsed();
    let allocs2 = alloc_count::COUNT.load(Ordering::Relaxed);
    let bytes2 = alloc_count::BYTES.load(Ordering::Relaxed);
    let rows2 = match out2 {
        TupleOutput::Yield(ExecutorOutput::rows(ResultSet::Rows { rows, .. }), _) => rows.len(),
        _ => 0,
    };
    println!("=== psql SELECT id,name probe ({} rows) ===", rows2);
    println!("  elapsed : {:?}", dt2);
    println!("  allocs  : {}  ({:.2} per row)", allocs2, allocs2 as f64 / rows2.max(1) as f64);
    println!("  bytes   : {}  ({:.2} per row)", bytes2, bytes2 as f64 / rows2.max(1) as f64);

    let stopping = running.stop();
    let stopped = stopping.finish();
    stopped.cleanup().expect("cleanup");
}
