# sqlmini — SQL subset compiler + execution engine

> **Scale (dual reference, stated honestly)**: in absolute terms this is a **micro SQL
> subset engine** (measured against SQLite/PostgreSQL it is demo-grade: ~2200 lines,
> single table, no optimizer); relative to this repository it is **axiom's largest
> single example** (larger than redis_like/netpath/psql). Purpose = paradigm evidence
> (whether axiom can carry a multi-stage real semantic pipeline); it **makes no claim**
> of engine-grade scale or performance.

## The system

Single-table SQL subset: `SELECT [DISTINCT] expr-list FROM table [WHERE predicate]
[GROUP BY expr] [ORDER BY expr [ASC|DESC]] [LIMIT n]`, aggregates
`COUNT/SUM/AVG/MIN/MAX`; CSV input; text-table output.

Size (`semantics/examples/sqlmini/`): ~2200 lines (lexer 290 / parser 490 /
planner 330 / exec 620 / ast 190 / data 110 / schema 60 / main 150),
38 tests.

## Architecture (in axiom vocabulary)

```
text ──▶ [Lexer] ─▶ [Parser] ─▶ [Planner] ─▶ plan ─▶ [Executor] ─▶ result table
         cell         cell         cell              Inline / partition-parallel
         └────────── TryChain<TryChain<Lexer, Parser>, Planner> ─────────┘
```

- The first three stages are each a `PortCell` (each stage's `In` = the previous
  stage's `Ok` value); the whole chain = nested `TryChain`, **failure is a value**
  (`SqlError{Lex,Parse,Plan,Exec}`, with position/object); short-circuit is
  type-guaranteed (downstream stages do not run on `Err`); T1 pairing is judged by
  `Conforms`.
- Planner's `State = Schema`: registered before driving; unregistered columns are
  rejected (no guessing).
- The executor has two physical paths over the same plan: **Inline** (single-threaded,
  row by row) and **partition-parallel** (Filter and per-group accumulation on
  threads; the aggregates are associative under merge — COUNT/SUM/MIN/MAX/Avg are
  splittable; ORDER/LIMIT are not, and run once after merge). Both paths are
  row-for-row equal (**T6**).

## Paradigm verification points (axiom claim ↔ evidence in this example)

| axiom claim | sqlmini evidence |
|---|---|
| Five concepts can carry real software | 4-stage compile pipeline + executor = causal-flow composition, no new concepts |
| Failure as value, short-circuit, typed errors | One error type across the chain; any stage's error carries its position, test-locked |
| Multi-physics equivalence (T6) is verifiable | Both paths row-for-row equal (4 query classes covered) |
| Obligation/budget discipline | Execution errors are values (`Exec` variant); no-panic convention |
| Composition closure | `TryChain` nesting + `Schema` state injection, both pass the §8.3 form |

## Build & test

```
cargo run   --manifest-path semantics/Cargo.toml --example sqlmini -- "SELECT …"
cargo test  --manifest-path semantics/Cargo.toml --example sqlmini
```

## Known subset boundaries (stated honestly)

- Single table; no JOIN / nested queries / transactions; CSV without quoting/escaping
  (fields = comma split);
- `ORDER BY` keys support output column names only (expression keys fall to `NULL`);
- NULL semantics simplified to three rules: predicates with Null are false, aggregates
  skip Null, operators propagate Null (`COUNT(*)` counts rows, other aggregates count
  numeric values);
- No indexes / no optimizer (the Planner only does legality and basic type checks —
  optimization is a separate engineering effort, outside this example's promise).
