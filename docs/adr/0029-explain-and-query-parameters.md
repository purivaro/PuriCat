# 0029 — EXPLAIN as the server's own plan, and named parameters written in as literals

## Status

Accepted. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-21 (explain and parameters)".

Verified on MySQL 9.3.0 and PostgreSQL 16.13 (live tests on both; through the window on MySQL). SQLite's plan is unit-tested only; MariaDB (which has no `FORMAT=TREE`, so the classic table is shown) has not been tried.

## Context

Two things a database tool the owner uses has and PuriCat lacked (survey of 2026-11-20, kept locally): a view of a query's plan, to see why it is slow, and parameters, to run the same query again with other values without editing it.

## Decision

### EXPLAIN

1. **"Explain the statement"** in the menu beside Run explains what is selected, or the statement the cursor is in. One statement at a time.
2. **The plan is the server's own text**: MySQL `EXPLAIN FORMAT=TREE` (the classic `EXPLAIN` table on a server without it), PostgreSQL's text plan, SQLite's `EXPLAIN QUERY PLAN` as an indented tree. No plan of PuriCat's making stands between the user and the server's words.
3. **Whole-table reads are pointed out**: MySQL's `Table scan on`, PostgreSQL's `Seq Scan on`, SQLite's `SCAN` without an index (MySQL's table: `type` `ALL`). The lines are marked in the plan and listed above it. Whether a full scan is a problem is left to the reader: on a small table it is the right plan.
4. **"Run and measure"** asks for `EXPLAIN ANALYZE` (MySQL) or `EXPLAIN (ANALYZE, BUFFERS)` (PostgreSQL), which runs the statement. It is accepted only for a statement the splitter classifies as a read, so it never makes changes; SQLite has none.
5. **A connection of its own** on the tab's database runs the EXPLAIN: the tab's connection keeps its last results for export, and an EXPLAIN there would replace them. Session state of the tab (a `SET`, a temporary table) is therefore not seen. Stop interrupts it on the server, as counting a table's rows does.

### Named parameters

6. **`:name`** in a script is a parameter: outside strings, comments, quoted names and PostgreSQL dollar quotes, and not PostgreSQL's `::` cast, MySQL's `:=`, or a colon right after a word or number (`arr[a:b]`). The backend finds them (`sql_parameters`) with the statement splitter's own scanner.
7. **Before a run (or an explain), their values are asked for**, each as Automatic (a number when it reads as one, otherwise text), Text, Number or NULL. The values typed last are offered again in that tab.
8. **The backend writes each value into the script as a literal of the engine**: a number as it is (in brackets when negative: `x-:n` must not become `x--5`, a comment on PostgreSQL), NULL as `NULL`, text in single quotes with the quote doubled. Text with a backslash is written so that it reads the same whatever the server's settings: on MySQL as `_utf8mb4 X'…'` (hex, independent of `NO_BACKSLASH_ESCAPES`), on PostgreSQL as `E'…'` with the backslash doubled. NUL is refused. The script that runs, and the history, hold the values.

## Alternatives considered

| Option | Why not |
|---|---|
| A plan diagram of PuriCat's making from `FORMAT=JSON` | Two very different JSON shapes to keep up with, and a picture that can drift from what the server says; the text plan is what people search for and share. A diagram can be built on top later |
| EXPLAIN on the tab's connection | It would clear the tab's results kept for export, and a measured run would mix into the tab's transaction |
| Bound parameters (`?`, `$1`) | The console runs scripts as text, statement by statement, through the simple protocol so that any statement works; binding would need prepared statements, which some statements refuse. The values are the user's own, typed for their own query: the literals only have to be correct, and the tests show them coming back from both servers as typed |
| `$name` or `${name}` as the syntax | `$1` is PostgreSQL's own parameter and `$$` its quote; `:name` is what other tools and SQLite use |

## Consequences

- A slow query's plan is one click away, with the full scans in view, and a query with parameters runs again with other values without editing it.
- Two IPC commands (`sql_parameters`, `explain_sql`; 103 typed commands) and an optional `params` on `run_sql`.

Limitations:

- **"Run and measure" runs the statement** on a connection of its own: a query that takes minutes takes minutes (Stop ends it), and one that calls a function with side effects has them.
- **The plan does not see the tab's session state** (a `SET search_path`, a temporary table).
- **A parameter is text in the script**: the server sees a literal, not a bound value, so a plan is made for that value, and a value typed as Number must be one.
- **`:name` inside a construct the scanner does not know** (a MySQL `JSON_TABLE` path outside quotes, say) would be taken for a parameter; the dialog shows every name it found before anything runs.
