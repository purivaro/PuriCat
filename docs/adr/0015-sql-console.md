# 0015 — SQL console: a connection per tab, text protocol, measured safeguards

## Status

Accepted. Implemented for MySQL and PostgreSQL as the first slice of Phase 4. Verified in the backend on both engines (`live_console`); through the UI on MySQL only ([TESTING.md](../../TESTING.md)). SQL Server has no implementation. Implements the editor chosen in [ADR 0007](0007-sql-editor.md) and answers the open question of [ADR 0004](0004-server-side-text-rendering.md) about results of arbitrary queries.

Saved scripts, object editing with DDL preview, import, SQL dump export, an EXPLAIN view, query parameters and results beyond 100,000 rows are not part of this record and remain Planned (Phase 4).

## Context

The brief asks for a SQL workspace: run a selection or a script, cancel it, see several result sets, duration and affected rows, export results, keep a history, and confirm dangerous scripts by risk, "not keyword matching alone".

Constraints:

- Sessions are pooled and small (4 connections on MySQL, 3 per database on PostgreSQL). Two statements sent through a pool may run on different connections, so `USE`, `SET`, temporary tables and an open transaction would not behave as in a terminal client. A tab that held a pooled connection for its lifetime would starve the explorer and the grids.
- The browse path gets exact values by rewriting the projection so the server renders text ([ADR 0004](0004-server-side-text-rendering.md)). Arbitrary SQL cannot be rewritten that way.
- The web view holds values as text; JavaScript numbers cannot carry 64-bit integers or long decimals.
- The session set-up (MySQL strict mode, the read-only option) is applied to each pooled connection when it is opened.
- The text of a statement does not say how many rows it will change.
- The web view has no filesystem access, and that should stay so.

## Decision

### 1. One dedicated connection per query tab

A query tab owns a `Console` (`db::console`). Its connection is acquired through the session's pool, so the pool's set-up applies (strict mode, the read-only statement), and is then detached from the pool, so open query tabs do not count against the pool's limit.

- The connection is opened by the tab's first run, not when the tab opens, and lives until the tab closes, the connection is disconnected, or the server ends it.
- Session state — `USE`, `SET`, temporary tables, an open transaction — lasts as long as that connection.
- MySQL: after another database is chosen in the tab's selector, the next run opens a new console on it and the old connection is dropped. PostgreSQL: a connection is bound to one database, so the tab is too; it is the database the tab was opened on.
- The IPC layer keeps one console per tab id (`AppState.consoles`), with the canceller and a stop flag outside the console's lock so a running statement can be interrupted.

### 2. Statements travel over the simple (text) protocol

The backend splits the script (section 3) and sends one statement at a time with `sqlx::raw_sql(...).fetch_many`. In that protocol the server renders every value as text, which gives the same exactness as the grid for any result shape, with no per-type decoder:

- cells are `string | null`; NULL stays distinct from the empty string;
- binary values (`bytea`, the MySQL `BINARY` / `BLOB` family) are shown as `0x` followed by lowercase hex; MySQL `BIT` is shown as a decimal number;
- a query that returns no rows still gets its column names, through a separate describe of the statement;
- one statement may produce several result sets; each becomes a result tab.

The backend keeps the full rows of the last run. The UI receives the same rows with cells longer than 1,000 characters cut for display and flagged.

### 3. The script is split and classified in the backend, per dialect

`db::script` is a small scanner, not a SQL parser. It walks the text outside strings and comments and knows the lexical rules that differ: MySQL backticks, `#` comments, backslash escapes, and `--` needing a following space; PostgreSQL dollar-quoted bodies, nested block comments and `E'...'` strings. It splits on the remaining semicolons and classifies each statement:

| Kind | Examples |
|---|---|
| `read` | `SELECT`, `TABLE`, `VALUES`, `SHOW`, `DESCRIBE`, `EXPLAIN` (without `ANALYZE` of a row-changing statement), `WITH` without a row-changing word |
| `dml` | `INSERT`, `UPDATE`, `DELETE`, `REPLACE`, `MERGE`, `WITH` containing one of them, `EXPLAIN ANALYZE` of one |
| `ddl` | `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, `RENAME`, `COMMENT`, `GRANT`, `REVOKE` |
| `transaction` | `BEGIN`, `START`, `COMMIT`, `ROLLBACK`, `SAVEPOINT`, `RELEASE`, `END`, `ABORT` |
| `other` | Everything else, including `SET`, `USE`, `CALL`, `DO` and `SELECT ... INTO`; treated as "may change something" |

A statement also gets a "danger" reason when it is an `UPDATE` or `DELETE` with no `WHERE` at the top level (a `WHERE` inside a subquery does not count), a `DROP`, a `TRUNCATE`, or an `ALTER` with a top-level `DROP`.

The classification decides conveniences only: which statements are listed for confirmation, which run under the impact guard, and which may be cut short at the row cap. It is never the only safeguard.

### 4. Safeguards measure what a statement did

1. **Confirmation before running (UI).** The tab asks `analyze_sql` for the statements and lists every statement with a danger reason; on a Production-labelled connection it lists every statement that is not `read`. Nothing runs until the user confirms.
2. **Impact guard (backend).** A `dml` statement that is not inside the user's own transaction runs in a guard transaction. If the server reports more affected rows than the limit, the statement is rolled back and reported as `blocked`, and the run returns the blocked statement plus the rest of the script as `resume`. The UI passes the connection's "review larger saves" threshold (`bulkReviewRows`, default 10) as the limit and offers "Run it without the limit", which runs `resume` with no limit.
3. **Row cap per result (backend).** A result keeps at most the chosen number of rows (100, 1,000, 10,000 or 100,000 in the UI; default 1,000; the backend refuses more than 100,000). A plain `read` is then cancelled on the server. A statement of any other kind, and any statement inside a transaction the user opened on PostgreSQL, is never cancelled at the cap: the remaining rows are read and dropped, because cancelling would undo the write (`UPDATE ... RETURNING`) or abort the user's transaction.
4. **Stop.** The running statement is interrupted from another pooled connection: `KILL QUERY <id>` on MySQL, `pg_cancel_backend(<pid>)` on PostgreSQL. The statements after it are not run.
5. **The first error stops the script.** The outcome reports how many statements were not reached.

Closing a query tab, or disconnecting its connection, interrupts a statement that is still running, so it does not carry on unseen on the server.

### 5. A transaction the user opens is theirs

The console tracks the user's `BEGIN` / `START TRANSACTION` / `COMMIT` / `ROLLBACK` from the statement text, read with the same scanner (a comment in front does not hide the keyword; `AND CHAIN` keeps the transaction open; `ROLLBACK TO` and `SAVEPOINT` change nothing). On MySQL it also asks the server for `@@autocommit`, remembers the answer, and asks again after any `other` statement, since `SET` or `CALL` may have changed it; a successful DDL statement on MySQL marks the transaction as ended (implicit commit), unless it contains the keyword `TEMPORARY`: `CREATE` and `DROP TEMPORARY TABLE` do not commit (second round, 2026-11-16).

While a transaction is open, or MySQL autocommit is off, the guard stays out: it adds no transaction of its own and blocks nothing. The UI shows a badge and a dot on the tab, disables the database selector, asks before closing the tab ("Roll back and close"), and refuses to disconnect, change or delete the connection — the same guard as for staged table edits. On PostgreSQL an error inside the user's transaction adds "run ROLLBACK" to the message, because the server has aborted the transaction.

### 6. A lost connection is reported, and the next run reconnects

An error that is not a server answer (I/O, protocol, TLS), a PostgreSQL SQLSTATE of class `08` or `57P01`–`57P03`, or MySQL error 1053, 1927 or 4031 marks the console as lost. The run says so, the IPC layer forgets the console, and the tab's next run opens a new connection. Whatever was open on the old one was rolled back by the server.

### 7. Export is written by the backend

`export_result` writes one stored result of the tab's last run to a path chosen in the native Save dialog. The web view gets only the dialog (`dialog:allow-save`); it still has no filesystem access, and the values never pass through JavaScript on their way to the file.

| Format | Rules |
|---|---|
| CSV | UTF-8 with a byte-order mark, RFC 4180, CRLF line ends. NULL is an empty field and an empty string is `""`, so the two stay distinct |
| JSON | An array of objects, one per row. Every value is a string or `null`: JSON numbers would lose digits of large integers and decimals in most consumers. Columns that share a name are kept under `name`, `name_2`, `name_3`, … so no value is dropped |
| XLSX | Bold, frozen header row. A cell of a numeric column becomes a number only when its text is plain decimal notation with at most 15 significant digits; everything else stays text. NULL is an empty cell. At most 1,048,575 data rows |

Only fetched rows are exported, so the row cap applies; long cells are exported in full.

### 8. History is local and masked

Each run that started is recorded in the SQLite settings store (table `query_history`, store migration 2): the connection, the database, the script text, the status of the last statement, a one-line summary and the duration. At most 1,000 entries are kept in total; the UI lists the latest 100 of the tab's connection and can clear that connection's entries. Before storage `script::redact` masks any quoted literal — in single quotes, or in the double quotes MySQL also accepts — that follows `password`, `identified by` or `identified with` within 48 characters. Deleting a connection deletes its history entries.

### 9. The editor

Monaco, as proposed in [ADR 0007](0007-sql-editor.md): the core editor, a chosen set of features and the `sql` / `mysql` / `pgsql` grammars, in a chunk that loads when the first query tab opens; the worker is bundled through Vite's `?worker` import. Completion offers table names the explorer has loaded, columns fetched through `describe_table`, and a keyword list. Formatting uses `sql-formatter`, loaded on demand. Editor text is saved per tab in the settings store (debounced) and cleared when the tab closes.

## Alternatives considered

| Option | Why not |
|---|---|
| Run each statement on the session's pool | Session state would not survive from one statement to the next, and a transaction could not span two runs |
| Keep a pooled connection checked out per tab | Four query tabs would exhaust the MySQL pool and leave nothing for the explorer, the grids or a cancel request |
| Extended protocol with driver-native decoding | The per-type matrix ADR 0004 avoided, and values would still need string transport to the web view. Not every statement can be prepared |
| Send the whole script as one multi-statement request | No per-statement outcome, timing or stopping point, and nowhere to put the guard transaction |
| A full SQL parser for splitting and classification | Vendor syntax a parser does not know would make a valid script impossible to run. The classification is advisory, so a lexical scanner that never refuses is enough |
| Decide risk from keywords alone | Ruled out by the brief; the text does not say how many rows a `WHERE` matches |
| Estimate impact first (`EXPLAIN`, `SELECT COUNT(*)`) | An estimate, or a second execution of the predicate, and the data can change before the real statement runs. The guard measures the statement itself and can still undo it |
| Wrap the whole script in one transaction | Changes what the user's script means, holds locks for the whole run, and MySQL DDL commits implicitly anyway |
| Cancel every statement at the row cap | Would undo a write that returns rows and abort a PostgreSQL transaction the user is in the middle of |
| Add `LIMIT` to the user's query | Rewriting arbitrary SQL is unreliable and changes the statement the user asked for |
| Write exports from the web view (filesystem plugin or a download) | Needs filesystem access in the web view, and the web view only has the display copy with long cells cut |
| JSON numbers and XLSX numbers for every numeric value | Silent loss of digits for 64-bit integers and long decimals |
| CodeMirror 6 instead of Monaco | See [ADR 0007](0007-sql-editor.md); Monaco ran under the production CSP, so the fallback was not needed |

## Consequences

- A query tab behaves like a terminal client: its own session, its own transaction, exact values.
- Open query tabs do not reduce what the explorer and grids can do. Each tab that has run something holds one extra server connection.
- Results of any shape are shown without a type matrix. Formatting is the server's, per session, as on the browse path.
- A wide `UPDATE` or `DELETE` is stopped by what it did, whatever it looked like, and nothing is left behind.
- The Content-Security-Policy is unchanged. One capability was added (`dialog:allow-save`).
- Monaco's production chunk is about 3.1 MB of JavaScript before compression, plus its worker (about 0.3 MB), stylesheet and icon font, loaded only when a query tab is first opened (sizes of the build of 2026-11-16; start-up time was not measured).

A defect found by the end-to-end run against the bundled app, and fixed: the production CSS build shortens colour tokens (`#ffffff` becomes `#fff`), Monaco's theme accepts only `#rrggbb` / `#rrggbbaa`, and the editor never appeared in the bundle although it worked under the dev server. Design tokens are now normalised before they are handed to the theme (`css()` in `src/features/sql/monaco.ts`).

Limitations:

- **The classification is textual, and advisory.** A function called from a `SELECT` can write; it is classified `read`, is not guarded, and may be cancelled at the row cap. `CALL`, `DO`, DDL and every `other` statement are not guarded. A `WITH` that merely mentions a word such as `replace` is treated as row-changing (the safe direction).
- **DDL cannot be undone on MySQL**, and nothing in the console tries to.
- **The guard measures the server's affected-row count for the statement.** Rows changed by triggers or foreign-key cascades are not in that count, and a statement whose count is not that of its changes (`EXPLAIN ANALYZE UPDATE`, a data-modifying `WITH` whose final statement is a `SELECT`) is measured by what the server reports. None of these cases is tested.
- **The confirmation and the limit are the UI's.** `run_sql` runs the text it is given with the limit it is given; a caller can pass no limit. The backend enforces the mechanism, not the policy.
- **"Run it without the limit" lifts the limit for the blocked statement and everything after it in that script.**
- **Transaction tracking is textual.** A transaction opened inside a stored procedure or with `XA` is not seen. A MySQL error that rolls back the whole transaction (a deadlock) is not noticed, nor is an implicit commit caused by a statement that is not DDL. The exception for MySQL DDL that does not commit is `CREATE` / `DROP TEMPORARY …`, recognised by `TEMPORARY` being the statement's second word. No test runs MySQL DDL inside a transaction.
- **Read-only is inherited, not enforced against the person typing.** The console's connection carries the session's read-only setting, and the server refuses a write (live-tested on MySQL). A statement can switch the session back (`SET SESSION TRANSACTION READ WRITE`, `SET default_transaction_read_only = off`). `run_sql` does not check the profile's read-only flag. The real control is a database account without write privileges ([SECURITY.md](../../SECURITY.md)).
- **Results are held in memory**, in the backend in full and in the web view with long cells cut: up to 100,000 rows per result, with no streaming. Memory use with wide rows at the cap has not been measured.
- **Results cannot be sorted or filtered** in the grid, and a capped result cannot be continued; the query has to be run again with a higher cap.
- **Only the last run's results can be exported**, and only the fetched rows.
- **History stores SQL text in clear** in the local settings database, apart from masked password literals. The mask is a heuristic: it takes the first quote character after the keyword, so it can hide an unrelated literal or a double-quoted identifier that follows the word `password`, and it misses the literal in `"password" = '...'`, where the identifier's own closing quote is the first one it meets. Other secrets in a script are stored as typed.
- **Stop needs a free pooled connection** to send the interrupt, and what an interrupt leaves behind is the server's rule: a guarded statement is rolled back; a statement in the user's PostgreSQL transaction aborts that transaction.
- **A `USE` typed in a MySQL tab moves the session but not the tab's database selector.**
- **PostgreSQL tabs cannot change database.**
- **Messages from the backend are English only**, including the danger reasons and the history summary.
- **Completion is name-based**: it does not understand aliases or scopes, and for PostgreSQL it preloads only the `public` schema of the tab's database.
- Verified on MySQL 9.3.0 and PostgreSQL 16.13 only. Through the UI the console was exercised on MySQL only. Stop and the lost-connection path are verified in the backend, not through the UI; the confirmation on a Production-labelled connection, completion, formatting, the Save dialog and restoring a tab's text after a relaunch have no automated test.

## Licensing

| Component | Licence |
|---|---|
| `monaco-editor` 0.57 | MIT (ships its own `ThirdPartyNotices.txt`, to be carried into the generated notice) |
| `sql-formatter` 15 | MIT |
| `tauri-plugin-dialog` 2 and `@tauri-apps/plugin-dialog` 2 | Apache-2.0 OR MIT |
| `rust_xlsxwriter` 0.96 | MIT OR Apache-2.0 |
| `futures-util` 0.3 | MIT OR Apache-2.0 |

Only the `dialog:allow-save` permission of the dialog plugin is granted.
