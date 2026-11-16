# Testing

## Layers

| Layer | Where | Needs | In `pnpm check` | In CI workflow |
|---|---|---|---|---|
| Rust unit tests | `crates/core/src/**` (`#[cfg(test)]`) | Nothing | Yes | Yes |
| Frontend unit tests (Vitest) | `src/**/*.test.ts` | Nothing | Yes | Yes |
| Static checks | `tsc`, `cargo fmt`, `cargo clippy -D warnings` | Nothing | Yes | Yes |
| Live database tests | `crates/core/tests/live_db.rs` (connect, browse), `crates/core/tests/live_write.rs` (save edits, Smart Paste), `crates/core/tests/live_console.rs` (SQL console) | Disposable MySQL and PostgreSQL servers | Server tests run only if the variables are set | Server tests skip (variables unset) |
| OS credential store test | `secrets::tests::os_keychain_round_trip` (ignored by default) | A real Keychain / Credential Manager | No | No |
| Real AI CLI test | `agent::tests::real_claude_cli_answers` (ignored by default) | The real Claude Code CLI, installed and signed in; sends one small prompt on that account | No | No |
| End-to-end smoke test | `e2e/run.sh`, `e2e/smoke.js`, stand-in AI CLIs in `e2e/fixtures/bin` | macOS desktop session, local MySQL and PostgreSQL. No AI account, network or quota | No | No |

The CI workflow (`.github/workflows/ci.yml`, macOS runner) exists but **has not run yet**.

## Commands

```sh
pnpm check                      # everything that needs no server

pnpm typecheck                  # tsc --noEmit
pnpm test                       # Vitest
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p puricat-core      # unit tests (live tests skip without variables)

cargo test -p puricat-core -- --ignored os_keychain    # real credential store round trip
cargo test -p puricat-core --lib -- --ignored real_claude   # one small prompt through the real Claude Code CLI, on your own account
```

There is no equivalent for Codex.

### Live database tests

```sh
PURICAT_TEST_MYSQL=you@127.0.0.1:3306      PURICAT_TEST_MYSQL_PASSWORD=...      \
PURICAT_TEST_POSTGRES=you@127.0.0.1:5432   PURICAT_TEST_POSTGRES_PASSWORD=...   \
  cargo test -p puricat-core --test live_db --test live_write --test live_console -- --nocapture
```

`live_db` covers connecting and browsing; `live_write` covers the save path and Smart Paste; `live_console` covers the SQL console. Each can be run alone with a single `--test`.

| Variable | Format | Required |
|---|---|---|
| `PURICAT_TEST_MYSQL` | `user@host:port` | To run the MySQL tests |
| `PURICAT_TEST_MYSQL_PASSWORD` | Password | Only if the account has one |
| `PURICAT_TEST_POSTGRES` | `user@host:port` | To run the PostgreSQL tests |
| `PURICAT_TEST_POSTGRES_PASSWORD` | Password | Only if the account has one |

The account must be allowed to create and drop databases. PostgreSQL must support `DROP DATABASE ... WITH (FORCE)`. `live_console` also interrupts and kills its own connections (`KILL QUERY`, `KILL`, `pg_cancel_backend`, `pg_terminate_backend`), which an account may do to its own sessions.

**A test whose variable is unset prints "skipped" to stderr and passes.** A green `cargo test` therefore does not prove the live tests ran; use `--nocapture` and check for the absence of "skipped" lines.

### End-to-end smoke test

```sh
e2e/run.sh              # run against `pnpm tauri dev`
SHOTS=1 e2e/run.sh      # also save a window screenshot per stage to screenshots/e2e/
KEEP_DB=1 e2e/run.sh    # leave the puricat_e2e databases in place afterwards
APP=<binary> e2e/run.sh # run against a bundled debug build instead of the dev server
```

| Variable | Default | Meaning |
|---|---|---|
| `MYSQL_USER` | `root` | MySQL account, connecting to `127.0.0.1` without a password |
| `PG_USER` | `$USER` | PostgreSQL account, connecting to `127.0.0.1` without a password |
| `MYSQL`, `PSQL` | `mysql`, `psql` | Client binaries used to load fixtures |

Requirements: macOS with a logged-in desktop session whose screen is **awake and unlocked, with the app window visible**, for the whole run (see below); `mysql` and `psql` clients, local servers that accept the accounts above without a password, `unzip` and `xxd` (the runner checks the exported files with them), and `swift` when `SHOTS=1`. The report is copied to `target/e2e-report.json`. How it works: [ADR 0012](docs/adr/0012-e2e-testing-on-macos.md).

The AI CLIs are stand-ins. The runner puts `e2e/fixtures/bin` first on `PATH`, so the app finds two shell scripts named `claude` and `codex` instead of the installed tools. They answer the status commands as an installed, signed-in CLI, print canned stream lines (Claude's as captured from Claude Code 2.1.303; Codex's refusal as captured from codex-cli 0.133.0, its success in the documented shape), and — because the runner sets `PURICAT_STAND_IN_DIR` — record the arguments, the stdin and (the `claude` stand-in only) the working directory of the last turn they were given. The run therefore needs no account, network or quota, and **does not exercise the real CLIs at all**, including their detection.

**The screen must stay on.** WKWebView suspends the timers and animations of a window that is not shown. When the screen is off or locked the injected script simply stops advancing: nothing fails, no error is logged, and the runner reports the last stage it saw once its limit of 5 minutes is over. Before a run, wake and unlock the Mac and keep it from sleeping; a run that stalls at an early stage without an error is first of all a reason to look at the screen.

The report carries a `problems` list: every Content-Security-Policy violation, uncaught error and unhandled promise rejection the page saw while the test ran (Monaco's own "Canceled" rejections, raised when focus moves, are ignored). **The run fails if the list is not empty**, so a resource blocked by the production CSP cannot pass unnoticed.

Side effects to know about:

- In dev-server mode the runner's cleanup kills the debug `puricat` process and whatever is listening on TCP port 1420.
- The smoke test does not run the installed `claude` or `codex`: stand-ins come first on `PATH` for the app it launches. Nothing is sent to any AI provider.
- **The smoke test overwrites the system clipboard** with its paste fixtures (direct paste and Smart Paste) and writes the previous text back at the end. If the run fails or is interrupted after the paste step, the fixture text stays on the clipboard. Non-text clipboard contents (an image, files) are not restored. Do not copy anything important while it runs.
- The smoke test writes rows through PuriCat into the `puricat_e2e` databases only. Its SQL console steps run `UPDATE` statements on `puricat_e2e.orders` (one blocked by the impact guard and then committed without the limit, one inside a transaction that is rolled back); the `DELETE` without `WHERE` it types is cancelled at the confirmation and never runs.
- The smoke test exports three files (`export.csv`, `export.json`, `export.xlsx`) into the runner's temporary work directory. The runner checks them and deletes the directory on exit.

## What each suite covers

### Rust unit tests (56, plus 2 ignored)

| Module | Covers |
|---|---|
| `model` | Input trimming and defaults, rejection of incomplete profiles, stable wire format |
| `store` (5) | Connection CRUD and ordering, update of a missing connection, settings round trip, reopening an existing database file. Query history (1 test): entries are per connection and newest first, the database name and start time are kept, no more than 1,000 entries survive over all connections, clearing one connection's history leaves the other's |
| `secrets` | In-memory store round trip; (ignored) real OS credential store round trip |
| `db::ident` | Quoting for MySQL, PostgreSQL and SQL Server, escaping of the quote character, rejection of empty and NUL names |
| `db` | Default order by primary key, validation of the sort column, page limit, truncation on character boundaries, binary prefix |
| `db::mysql`, `db::postgres` | The generated `SELECT` text, type classification; MySQL expression defaults (`DEFAULT_GENERATED`) not treated as generated columns |
| `db::write` | The exact MySQL plan and the exact PostgreSQL plan for one change set (statement order, typed key comparison, bare-type casts, `BIT` handling, concurrency conditions, parameter order, empty insert); refusal of empty, incomplete-key, NULL-key, wrong-key, unknown-column, generated-column, duplicate-column and no-change updates; keyless tables (delete refused, insert planned); hostile key and cell values absent from the SQL text |
| `db::smart` (8) | Number recognition for the pre-check (signs, decimals, exponents, `NaN` / `Infinity`; thousands separators and non-ASCII digits rejected). UPSERT on a unique key: every status in one paste (update with the changed column, unchanged, insert with an empty NOT NULL-with-default cell left out, missing key, exact-text duplicates, not a number, longer than `varchar(8)`), the counts, the lookup SQL shape for PostgreSQL and its parameters, the resulting change set. A generated key: blank means new, unknown means skipped, the key is never written, the MySQL lookup shape. INSERT with a key (existing rows refused), INSERT without a key (no lookup), UPDATE never inserting. Two spellings of one key (`7`, `007`) matching one existing row reported as duplicates. One existing row is claimed once even across lookup chunks: two spellings (`abc`, `ABC`) answered by separate lookups are both duplicates. Key values must be well formed for their type: `12abc`, `1.9` and `1e2` for an integer key are invalid and never looked up, `-7` is looked up. Refused set-ups: a key that is not unique, no key, nothing to update, unknown / doubly mapped / non-writable columns, a key column in the update list, a required column not mapped, ragged rows. The database answers are simulated in these tests |
| `db::script` (6) | Splitting on real semicolons only (not in strings, quoted identifiers, line or block comments), empty statements dropped, byte offsets of statements. Dialect rules: MySQL backticks, `#` comments, backslash escapes and `--` without a following space not being a comment; PostgreSQL dollar-quoted bodies, nested block comments, literal backslashes, `E''` strings, `$1` not opening a dollar quote. Classification: reads (`SELECT`, `WITH`, `SHOW`, `EXPLAIN DELETE`, `VALUES`), row-changing statements (including a data-modifying `WITH`, `EXPLAIN ANALYZE UPDATE`, `MERGE`), DDL, transaction control, `SELECT ... INTO` and `CALL` as `other`, keywords inside a string or an identifier ignored. Danger reasons: `DELETE` and `UPDATE` without `WHERE` (a `WHERE` only in a subquery still counts as none), `DROP`, `TRUNCATE`, `ALTER ... DROP`, and their harmless counterparts. Transaction effect: `BEGIN`, `START TRANSACTION`, `COMMIT`, `ROLLBACK`, `ROLLBACK TO` and `SAVEPOINT` (no change), a comment in front, a keyword inside a comment, `AND CHAIN` / `AND NO CHAIN`, `START REPLICA`. Masking of password literals after `IDENTIFIED BY` and `PASSWORD`, in single quotes and in double quotes (`IDENTIFIED BY "x"`), including the over-masking of a literal after a column named `password`. `mentions`: the keyword `TEMPORARY` is found in `create temporary table …` and not inside a string |
| `db::console` (2) | Value kind from the driver's type name (numbers, text, temporal, JSON, binary, bool), including `INTERVAL` as temporal and `POINT` as text although both contain "int", and `INT4` as a number. The preview: a cell longer than 1,000 characters is cut on a character boundary and flagged while the stored value stays whole, NULL is kept, the result id and the `capped` flag are passed through; hex rendering of bytes. The run loop itself is covered by the live tests only |
| `db::export` (3) | CSV: exact bytes of a file with a byte-order mark, CRLF, quoted fields (comma, quotes, line break, padding), Thai text, a 64-bit integer beyond 2^53, NULL as an empty field and an empty string as `""`. JSON: every value a string or `null`, the row count; the keys for columns that share a name (`id`, `id_2`, `id_3`) — the keys are checked, a file with such columns is not written in the test. XLSX: which texts become numbers (`12.50`, `-7`, 15 digits) and which stay text (16 or more significant digits, exponents, `NaN`, empty, `.5`, `5.`, thousands separators, a leading `+`); the written file is a zip archive (its cells are not read back in the unit test) |
| `ai_detect` (3) | Parsing of `claude auth status --json` and `codex login status` output, binary lookup in directories |
| `agent` (12, plus 1 ignored) | **Stream parsers**, against lines captured from Claude Code 2.1.303: `system/init`, `text_delta` events and the complete `assistant` message become one `Started` and the deltas, the repeated complete message is not sent twice, the outcome is success; without partial messages the complete message is the answer; a rejected model (`assistant` with `error`, `result` with `is_error`) yields no answer text and the CLI's own sentence as the error. Codex: the refusal captured from codex-cli 0.133.0 (`error`, `turn.failed`) yields the sentence inside the wrapped JSON; the success shape (`item.completed` / `agent_message`, `turn.completed`) is **the documented one, not a capture** — two messages are joined as paragraphs, a reasoning item is ignored. Lines that are not events are ignored; the fallback message from stderr. **Prompt**: the exact text for a three-turn conversation with context; the instructions lead the prompt for Codex; a `</context>` inside the context cannot close the block; no message, an empty message and a last message from the assistant are refused. **Trimming**: a 60,000-character old turn is dropped with the note, later turns kept; the newest message is kept whatever its length. **Model names and arguments**: accepted and refused names (`-p`, `--dangerously-skip-permissions`, spaces, `;`, `$(…)`, 101 characters); the Claude vector ends in `--model <name>`, has `--tools ""`, `--strict-mcp-config`, `--no-session-persistence` and nothing with "dangerously" or "bypass"; the Codex vector has `--sandbox read-only`, `--ephemeral`, `-m <name>` only when a model is chosen, and ends in `-`. **Codex catalogue**: hidden entries, entries without a slug and a slug that looks like an option are left out; a missing display name falls back to the slug. **Context text**: the exact text for a PostgreSQL schema with one described table and one names-only view, with the active SQL trimmed; a column default is not in it. **With a stand-in shell script as the CLI** (Unix only): the answer streams; the conversation and the context arrive on stdin and are absent from the arguments; the working directory is the scratch directory and stays empty; an error described in the stream and one that is only on stderr are each reported in the CLI's words; Stop ends a turn that is sleeping, in under 10 seconds, with the event that had arrived; a model name that looks like an option is refused before anything is started. Two further cases in the same test were added when the turn handling was reworked (commit `1515bac`): Stop also ends a child process that the stand-in started and that holds the pipes (the child's process id is no longer alive), and a turn whose stream has given its verdict returns in under 6 seconds although a child of the stand-in keeps the pipe open for 8. (Ignored) `real_claude_cli_answers`: one prompt through the real Claude Code CLI with the `haiku` alias — an answer comes back, the CLI names a model, the scratch directory is empty afterwards |

### Live database tests: `live_db` (4 tests)

| Test | Covers |
|---|---|
| `mysql_browse_and_page` | Server info, database list with system flag, object kinds, describe (types, nullability, generated, primary key), exact values (Thai text with quotes and a tab, `DECIMAL(24,6)`, `BIGINT` beyond 2^53, `DATETIME(6)`, JSON, BLOB, `BIT(8)`), NULL, truncation flags, paging, sorting, rejected inputs (unknown sort column, page size 0 and 1001, missing table), the session keeping the server's time zone and `sql_mode` (including `PIPES_AS_CONCAT` unchanged) with `STRICT_ALL_TABLES` added, composite key order, reading rows through a view, an expression default not flagged as generated, a table name containing a backtick, keyless table order, server-confirmed read-only, failed login without the password in the message |
| `postgres_browse_and_page` | The same shape for PostgreSQL, plus schemas, system schema flags, enum, array, `uuid`, `jsonb`, `bytea`, `timestamptz`, identity column, view and materialized view listing, reading rows through a view, a name containing a double quote, a profile without a default database, server-confirmed read-only on a named database, failure on an unknown database |
| `unreachable_server_fails_fast_with_the_real_reason` | Both engines against a closed port: a "refused" error in under 5 seconds. Needs no server |
| `sort_spec_wire_format` | Default of `descending`. Needs no server |

### Live database tests: `live_write` (4 tests)

Two tests per engine: saving edits (`mysql_save_edits`, `postgres_save_edits`) and Smart Paste (`mysql_smart_paste`, `postgres_smart_paste`).

Saving edits. Each test creates its own scratch database, loads fixtures over a separate admin connection, and then saves change sets through `Session::apply_changes`, reading the result back through `Session::fetch_rows`.

| Area | MySQL | PostgreSQL |
|---|---|---|
| `editable` flags (key and generated columns not writable) | Yes | Yes |
| The plan is parameterised (values absent from the SQL text) | Yes | Unit test only |
| One save with an update, an insert and a delete; exact read-back | Thai text with quotes, tab and line break; `DECIMAL(24,6)`; negative `BIGINT` beyond 2^53; `TINYINT(1)`; `DATETIME(6)`; JSON; `BIT(8)`; `ENUM`; a value set to NULL; stored generated column recomputed | Thai text with quotes, tab and line break; `numeric(24,6)`; `boolean`; `timestamptz` with microseconds; `jsonb`; `text[]`; `uuid`; enum set to NULL; `date`; `varchar(5)` |
| Inserted row takes server defaults for omitted columns | Yes | Yes |
| Update to an identical value still matches one row | Yes | No |
| 64-bit key beyond 2^53 addresses exactly one of two adjacent ids | Yes | No |
| Conflict: row changed elsewhere; the whole save is rolled back, including an earlier valid statement | Yes | Yes |
| Conflict: a NULL the user saw was filled in elsewhere | Yes | No |
| Deleting an already-deleted row is a conflict | Yes | No |
| Invalid input rejected with "Nothing was saved" | Bad `DECIMAL`, bad `BIGINT`, unknown `ENUM` label; NULL into `NOT NULL` | Bad `numeric`, unknown enum label, over-long `varchar(5)`, impossible date, malformed JSON |
| Auto-increment / identity, generated and unknown columns refused before any SQL | Yes | Yes |
| Composite key update | Yes | Yes (quoted table name) |
| Partial composite key refused | Yes | No |
| Keyless table: delete refused, insert allowed | Yes | Yes |
| Read-only session: the server refuses the write, data unchanged | Yes | Yes |

Smart Paste. One scenario runs on both engines against a scratch table `stock` (generated `id` primary key, unique `sku`, composite unique `(sku, day)`, a non-unique index, `DECIMAL(24,6)` / `numeric(24,6)` price, `qty` NOT NULL DEFAULT 0), through `Session::unique_keys`, `analyze_paste` and `apply_paste`.

| Area | MySQL | PostgreSQL |
|---|---|---|
| Key discovery: primary key first, unique index, composite unique index; a non-unique index is not a key | Yes | Yes |
| Excluded unique indexes | Prefix index `(name(3), day)` | Expression index `lower(name)`, partial index `WHERE qty > 100` |
| UPSERT on the unique key: update (changed columns listed), unchanged, insert; Thai text with quotes and a 20-digit decimal read back exactly | Yes | Yes |
| The preview writes nothing | Yes | Yes |
| Inserted row: server-assigned id; a NULL for the NOT NULL-with-default column takes the default | Yes | Yes |
| Re-running an approved paste is refused as a conflict ("Preview again") | Yes | Yes |
| INSERT with a key: an existing key is reported and the whole paste refused | Yes | Yes |
| UPDATE by the generated id: an unknown id is skipped, never inserted | Yes | Yes |
| Row problems flagged: duplicate key, missing key, not a number, too long | Yes | Yes |
| Key equality decided by the database: `c3` / `C3` | Same key: both rows reported as duplicates | Different keys: one unmatched, one update |
| A malformed value for the integer key (`<id>abc`, `<id>.9`, ` 1`) is an invalid row ("not a whole number"), not a match on the row its numeric prefix names | Yes | Yes |
| INSERT with the generated key chosen: an unknown id is invalid, a blank id is an insert (preview) | Yes | Yes |
| Composite key `(sku, day)`: update; the same sku with another day unmatched; a NULL part of the key reported as a missing key; applied and read back | Yes | Yes |
| Same counts, different outcome: the fingerprint of one preview (A1 update, B2 unchanged) is refused as a conflict for a paste whose fresh analysis is A1 unchanged, B2 update | Yes | Yes |
| A key that is not a unique key is refused, in the preview and at run time | Yes | Yes |
| A value only the server can judge (`2026-02-30`) rolls back the earlier valid row; "Nothing was saved" | Yes | Yes |
| Cancel after 50 of 130 statements leaves nothing behind | Yes | Yes |
| 130 rows over several lookup chunks, then pasted again with half of them edited: 65 updates, 65 unchanged | Yes | Yes |

Not covered by the Smart Paste scenario: the exclusion of a MySQL expression (functional) unique index, a malformed key value of a non-integer type (a date, for example), a table changed by another session between preview and run (the conflict cases above re-run or swap the request instead), a row changed between the run-time analysis and the write, two spellings of a key in different lookup chunks (unit-tested with simulated answers only), the 10,000-row limit, the progress callback's values, and the IPC commands themselves (the tests call the core functions below the IPC layer, so the read-only refusal in `apply_paste`, `cancel_job` and the progress channel are not exercised here).

### Live database tests: `live_console` (2 tests)

`mysql_console` and `postgres_console`. Each creates its own scratch database with a table `items` (Thai text with quotes, a `DECIMAL(24,6)` / `numeric(24,6)` price, a `BIGINT` beyond 2^53, a 1,500-character value, NULLs), opens consoles through `Session::open_console` and runs scripts through `Console::run`. One shared scenario runs on both engines, followed by engine-specific checks.

| Area (shared scenario) | MySQL | PostgreSQL |
|---|---|---|
| A three-statement script with a comment containing a semicolon: one outcome per statement with its kind (`read`, `dml`, `read`), all `ok`, nothing skipped | Yes | Yes |
| Exact values in a result: column names, Thai text with quotes, a 20-digit decimal, `9007199254740993`, NULL | Yes | Yes |
| A query reports rows, not rows affected; an `UPDATE` reports 1 row affected and no result | Yes | Yes |
| A query returning no rows still has its column names | Yes | Yes |
| The first error stops the script: the failing statement carries the server's message, the statement after it is counted as skipped and has not run | Yes | Yes |
| Impact guard: an `UPDATE` touching 3 rows with a limit of 2 is `blocked` with 3 rows affected, `resume` is the statement, and nothing changed | Yes | Yes |
| `resume` is the blocked statement and the rest of the script, not the part that already ran | Yes | Yes |
| With the limit lifted the same statement commits | Yes | Yes |
| A transaction the user opens is not guarded (3 rows with a limit of 1), is reported as open, spans runs, and `ROLLBACK` undoes it | Yes | Yes |
| The same with a line comment before `BEGIN` and a block comment before the `UPDATE` | Yes | Yes |
| Session state lasts between runs | `SET @kept = 5`, read back | `CREATE TEMP TABLE`, read back |
| Row cap: 5,000 rows with a cap of 100 gives 100 rows flagged `capped`; the console runs the next query normally | Yes (recursive CTE) | Yes (`generate_series`) |
| A 1,500-character cell is cut to 1,000 characters and flagged in the preview; the CSV export has it whole, with quotes doubled; an unknown result id is an error | Yes | Yes |
| Cancel: a 30-second sleep followed by a second statement, interrupted after 0.4 s, returns in under 10 s as `cancelled`; the second statement is not run; the console is usable afterwards | Yes (`SELECT SLEEP(30)`) | Yes (`SELECT pg_sleep(30)`) |
| A script with no statement, and a row cap of 0, are refused | Yes | Yes |
| Lost connection: after the server kills the console's connection, the run reports `connectionLost`, no open transaction, one failed statement saying the connection was lost; a further run on that console is refused; a new console on the same session works | Yes (`KILL <id>`) | Yes (`pg_terminate_backend`) |

| Area (engine-specific) | Covered |
|---|---|
| MySQL: `VARBINARY` shown as `0x00ff10`, `BIT(8)` as `5` | Yes |
| MySQL: `USE information_schema` moves the tab's session; the statement is classified `other` | Yes |
| MySQL: with `SET autocommit = 0` an `UPDATE` over the limit is neither blocked nor committed by the guard; the run reports a transaction open; after `ROLLBACK` it still does (autocommit is off); `SET autocommit = 1` clears it and the guard blocks again | Yes |
| MySQL: six open consoles do not keep the session's own pool (4 connections) from answering | Yes |
| MySQL: on a read-only session the server refuses a `DELETE` sent through the console; the rows are still there | Yes |
| PostgreSQL: `bytea` as `0x00ff10`, a user-defined enum, a `text[]` literal, `timestamptz` with microseconds | Yes |
| PostgreSQL: `INSERT ... RETURNING` is `dml`, returns its row and reports 1 row affected | Yes |
| PostgreSQL: a `DO $$ ... $$` block containing semicolons is one statement | Yes |
| PostgreSQL: `UPDATE ... RETURNING` with a cap of 2 shows 2 rows flagged `capped`, reports 4 rows affected, and all 4 rows are changed and committed | Yes |
| PostgreSQL: inside the user's transaction a 5,000-row read capped at 100 does not abort the transaction; the earlier `UPDATE` in it is still visible | Yes |
| PostgreSQL: an error inside the user's transaction adds "run ROLLBACK" to the message; the transaction is still reported open until `ROLLBACK` | Yes |

Not covered by `live_console`: the IPC commands (`run_sql`, `cancel_sql`, `close_console`, `export_result` and the history are below or beside what the tests call, so the stop flag set by `cancel_sql`, the interruption on closing a tab or a session, the replacement of a lost console and the masking of the stored script are not exercised here); a read-only session on PostgreSQL through the console, and a statement that switches a read-only session back; MySQL DDL inside a user transaction; a deadlock; a transaction opened in a stored procedure; a statement returning several result sets (a MySQL procedure); the row cap inside a user's transaction on MySQL; a result at or near 100,000 rows; JSON and XLSX export (unit tests and the end-to-end test cover them).

Not covered by the live tests: a wrong password on PostgreSQL, a wrong password (as opposed to an unknown user) on MySQL, reading rows through a PostgreSQL materialized view, any TLS mode other than the default, the 10,000-change limit, two PuriCat sessions editing the same row, a foreign-key or unique-constraint failure (the end-to-end test covers one foreign-key refusal), the PostgreSQL `TimeZone=UTC` behaviour (documented in [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md), not asserted), and the types listed there as not verified on the write path.

### Frontend unit tests (39 cases in 6 files)

| File | Cases | Covers |
|---|---|---|
| `src/lib/clipboard.test.ts` | 7 | Parsing spreadsheet clipboard text: plain ranges and the final line break, empty cells and rows, Unicode, quoted cells with tabs / line breaks / doubled quotes, ragged rows, an unterminated quote, no reinterpretation of numbers, dates or leading zeros; serialising back with quotes only where needed |
| `src/features/table/pending.test.ts` | 11 | Staging and un-staging an edit, immutability of earlier states (undo relies on it), blockers (non-editable column, truncated cell, deleted row, unaddressable row), empty input vs NULL, adding / filling / dropping new rows, marking and restoring rows, block paste with appended rows and overflow reporting, single-cell fill of a selection, empty clipboard, change-set building with original values, composite keys in key order, undo / redo history |
| `src/features/table/smartPaste.test.ts` | 9 | Spreadsheet column letters; header-row detection by column names (case-insensitive, a lone header row is data); automatic mapping by header name and by position, each table column used once; keys offered only when all their columns are mapped; update columns limited to writable non-key columns; request building: header, unmapped columns and blank rows left out, source row numbers kept, empty cells typed (NULL for a nullable number and for a blank key, empty string for text), an empty cell of a NOT NULL non-text column sent as NULL so the default can apply, the `NULL`-text option, no update list for INSERT |
| `src/stores/workspace.test.ts` | 5 | Tab closing and neighbour activation, tab id uniqueness across connection / database / schema |
| `src/features/table/sort.test.ts` | 2 | The sort cycle of a header click |
| `src/features/agent/markdown.test.ts` | 5 | The Markdown reader for AI answers: paragraphs, fenced code with its language, headings and lists in one answer; a code block still streaming (no closing fence) and the text inside a fence kept verbatim; simple tables, and a line with a pipe character that is not a table; markup-looking text (`<img … onerror=…>`) stays text and single asterisks are not emphasis; which code blocks count as runnable SQL (by tag, or untagged by first keyword; `bash` and `json` blocks do not) |

There are no component tests: `DataGrid`, `TableTab`, `ReviewDialog`, `SmartPasteDialog`, the query tab (`SqlTab`, `monaco.ts`) and the agent chat (`AgentChat`, `AiPanel`) are exercised only by the end-to-end test. The SQL console added no frontend unit test: the workspace store's handling of query tabs (`openSql` with or without seed text, the numbering of "Query n", releasing a closed tab's console and saved text) has none. Neither has the agent store (`src/stores/agent.ts`): which turns are resent, the claim that stops a second send, retry, stop, loading and saving the preferences.

### End-to-end smoke test

Empty welcome state; create and test a MySQL and a PostgreSQL connection through the dialog and save them; a clear error for an unreachable server and the production read-only hint; browse MySQL (tree groups, grid rows, footer); sort ascending and descending; next page; structure view; browse PostgreSQL at the same time in a second tab; the AI panel and the agent chat on stand-in CLIs (below); duplicate and delete through the context menu and confirmation dialog; dark theme and Thai through the command palette; finally the stored connections, UI settings and tab list are read back over IPC.

Editing, in the real window against both engines:

1. MySQL: double-click a cell, type Thai text, see it staged.
2. Paste a 3×2 block (written to the system clipboard through the clipboard plugin) containing doubled quotes and a tab inside a cell; the third row falls past the end and becomes a new row.
3. Undo and redo the paste.
4. Add a row with the button and type straight into it.
5. Mark a row for deletion: Save opens the mandatory review dialog with the planned statements; committing is refused by the server (foreign key); the error is shown, the staged edits are kept and nothing is saved.
6. Restore the row and save directly; the reloaded grid shows exactly what was typed and pasted.
7. Closing a tab with unsaved edits asks first; "Keep editing" keeps the edit; Discard clears it.
8. A keyless table and a view are labelled read-only.
9. Smart Paste on MySQL `orders`: the clipboard holds a header row, four data rows and an empty row. The header row is detected, columns are mapped by name (case-insensitively), an unknown pasted column is skipped, and the unique index on `order_no` is chosen as the key. The preview shows 1 insert, 1 update, 1 unchanged and 1 problem ("not a number"), and Run is disabled. After the clipboard is corrected and read again the preview is clean; Run; the reloaded grid shows the updated row and the inserted row.
10. PostgreSQL: `abc` in a numeric column is refused by the server with the edit kept staged; a corrected value and a second cell then save.

SQL console, in the real window on MySQL (between steps 9 and 10; stages `sql-console`, `sql-blocked`, `sql-transaction` and the `sql-more` log entry). The editor's text is set through the Monaco instance and scripts are started with the Run button.

1. "New query" from the connection's context menu in the explorer opens a query tab; the Monaco editor loads.
2. A two-statement script with a leading comment runs and the first result's rows appear in the grid. The result tabs (one per statement, with row counts) and the summary line are recorded in the report, not asserted.
3. `DELETE FROM audit_log` (no `WHERE`) opens the confirmation, which lists the statement and its reason; Cancel closes it. That nothing ran is visible in the history recorded in step 8, not asserted.
4. An `UPDATE` of 50 rows on a connection with the default threshold of 10 is reported as blocked. "Run it without the limit" then runs it: 50 rows affected.
5. A three-statement script whose second statement fails: the Messages list shows `ok`, `error` with the server's message, and "1 later statement was not run".
6. `BEGIN` behind a comment followed by an `UPDATE` of 40 rows: both statements are `ok` (the guard stays out), the transaction badge appears, the tab gets its dot, the database selector is disabled, and closing the tab opens the "Roll back and close" dialog; "Keep the tab" keeps it. `ROLLBACK` followed by a count then shows 0 rows changed and clears the badge, the dot and the lock.
7. A script that contains only a comment cannot start; the reason is shown in Messages.
8. The history menu lists the scripts that ran on the connection.
9. A query over `customers` is run and its result exported as CSV, JSON and XLSX by calling `export_result` over IPC with paths in the runner's work directory. After the run the runner checks the files: the CSV starts with a byte-order mark and holds the first row with its Thai name, the JSON holds the name as a string, and the XLSX's shared strings hold it.

Agent chat, in the real window through the stand-in CLIs (after step 10, with the PostgreSQL `orders` table tab in front; stages `agent-chat` and `agent-codex`). The composer's text is set programmatically and sent with the Send button.

1. Opening the AI panel shows the stand-in Claude Code as installed and signed in (its version line and "Signed in"), and the model selector lists "Default model", Fable, Opus, Sonnet, Haiku and "Other…".
2. The context button opens the context inspector. Its text must contain `Engine: PostgreSQL` and a `Table orders` entry, and must not contain `127.0.0.1` or `<user>@`. The button's label and the first 500 characters of the text are recorded in the report.
3. A question in Thai is answered: the streamed answer ends as `done`, its code block holds exactly the two-line `SELECT`, and the bold and inline-code parts are rendered as such.
4. "Open in query tab" on that block: one more tab exists, its Monaco editor holds exactly the SQL, and no run summary is present — nothing ran.
5. "Other…" in the model selector, the name `broken`, then a question: the turn ends as `error` and shows the stand-in's sentence ("There's an issue with the selected model (broken)…"). After choosing `haiku`, "Try again" produces an answer.
6. A question the stand-in answers slowly: once the first words have streamed, Stop ends the turn as `stopped` and the words stay.
7. Switching to Codex shows its stand-in as signed in and the model selector lists the catalogue's visible entries (the hidden one is absent). A question with the default model is refused in the service's words ("The 'stand-in' model is not supported when using Codex with a ChatGPT account."); after choosing a listed model, "Try again" produces the answer.

After the run, `run.sh` checks what the stand-ins recorded for the last turn each of them received (Claude's stopped turn, Codex's successful one): the context (`Engine: PostgreSQL`, `Database: puricat_e2e`) and the question are on Claude's stdin; Codex's stdin begins with the fixed instructions; neither stdin contains `127.0.0.1` or a connection's name; neither argument list contains `orders` or `puricat_e2e`; Claude's arguments contain `--strict-mcp-config`, `--no-session-persistence` and an empty value after `--tools`; Codex's contain `read-only`; Claude's working directory ended in `/data/agent-scratch`, and that directory is empty.

For the agent chat it does **not** cover: the real CLIs in any way; a MySQL context, a query tab's context (its SQL in the prompt) or a database with more than 40 tables; the "Share context" switch, "Context sent" under a message, "No context" states; Enter, Shift+Enter and IME composition in the composer; "Copy"; "New chat"; the signed-out and not-installed panels and "Check again"; a refused model name in the "Other…" field; the model and provider choices surviving a relaunch; the 10-minute timeout; a long conversation being trimmed; running the SQL that was opened in the query tab; Codex's working directory and its `-C` argument (the Codex stand-in records neither); any turn but the last of each CLI in the runner's checks.

It uses synthetic DOM events and empty passwords, so it does **not** cover: saving a password through the UI, restoring tabs after a relaunch, native keyboard shortcuts other than the palette (the grid's shortcuts are dispatched as synthetic `keydown` events, so Cmd+V, Cmd+Z and Cmd+S arriving from the operating system are untested), IME composition, drag selection, Shift+arrow selection, copy, the grid's context menu, drag-resizing, the explorer filter or arrow-key navigation. It also does not exercise the review dialog on a Production-labelled connection, the bulk-threshold trigger, a concurrent-edit conflict, the locked paging / sorting controls, the bulk tab closes, or the refusal to disconnect with unsaved edits. For Smart Paste it covers UPSERT on a single-column unique key on MySQL only: not INSERT or UPDATE mode, a composite or generated key, changing the mapping, key or update columns by hand, the header and `NULL`-text switches, the key picked after switching mode, an empty cell of a NOT NULL column taking its default, the progress bar, "Stop and roll back", the blocked dismissal of the dialog during a run, a run-time conflict, a Production-labelled connection, or PostgreSQL.

For the SQL console it covers MySQL only, with the text set programmatically. Not covered: typing into Monaco, ⌘↵, running a selection, ⌘T and the palette's "New query", the Stop button, a lost connection, the row-cap selector and a capped result, the database selector actually switching database, confirming the destructive-statement dialog ("Run anyway"), the confirmation on a Production-labelled connection, "Roll back and close", the refusal to disconnect with a transaction open, completion, formatting, loading a history entry into the editor, "Clear history", the Export menu and the native Save dialog (the export goes straight to the IPC command), a result with several long or binary cells, restoring a query tab and its text after a relaunch, and any of it on PostgreSQL (the agent-chat steps open a PostgreSQL query tab with text and check that nothing ran; they run nothing in it).

## Safety guarantees

- Live tests create a database named `puricat_it_<random>` per test and drop it afterwards, even when the test body fails. If the test process is killed, a `puricat_it_*` database can be left behind and is safe to drop.
- The live MySQL test attempts one login as `puricat_no_such_user`; it appears in the server's log as a failed login.
- The end-to-end runner drops and recreates only the database `puricat_e2e` on each server, and drops it again on exit unless `KEEP_DB=1`.
- The end-to-end app instance uses a temporary settings directory and an in-memory secret store; the user's real settings and Keychain are not touched.
- The ignored Keychain test writes and deletes one entry under a random `connection/selftest-<uuid>/password` key.
- `live_write` and the end-to-end test issue `INSERT`, `UPDATE` and `DELETE` through PuriCat's own write path, but only against tables in the scratch databases they created (`puricat_it_<random>`, `puricat_e2e`). `live_db` issues only reads through PuriCat. DDL and fixtures are run by separate admin connections.
- `live_console` sends SQL through the console to its own scratch database: `UPDATE`, `INSERT`, `DELETE` (refused, on a read-only session), a temporary table, `SET` statements, and on MySQL `USE information_schema` followed by a read. It interrupts and kills only connections it opened itself, by the id the server gave that connection. It writes one CSV file to the system temporary directory and removes it.
- The end-to-end test replaces the text on the system clipboard and restores the previous text when it finishes (see the side effects above).
- The end-to-end test starts no real AI CLI and sends nothing to an AI provider: `e2e/fixtures/bin` is put first on `PATH` for the app it launches, and the stand-ins only print canned lines and write what they were given into the runner's temporary work directory. `PURICAT_STAND_IN_DIR` is read by the stand-in scripts, not by PuriCat.
- The unit tests of the agent chat use a shell script written to a fresh directory under the system temporary directory as the CLI, and remove it. Only the ignored `real_claude_cli_answers` starts the real Claude Code CLI: it sends one prompt ("Reply with exactly the word: pong") with the `haiku` alias on the account that CLI is signed in to, in a temporary working directory it removes. It sends no schema and touches no database.
- Never point `PURICAT_TEST_*` at a server that holds real data. The tests need broad privileges and the guarantees above are only as good as the code.

## Results — 2026-11-16

Environment: macOS 26 on Apple Silicon, Rust 1.99, Node 25, pnpm 10, local MySQL 9.3.0 and PostgreSQL 16.13. Run on the development machine, not in CI.

| Check | Result |
|---|---|
| `cargo test -p puricat-core` (unit) | Pass: 56 passed, 2 ignored (the Keychain test and `real_claude_cli_answers`) |
| `cargo test -p puricat-core --lib -- --ignored real_claude` | Pass, against the real Claude Code 2.1.303 with the `haiku` alias, one small prompt on the developer's account each time: once on the first version of the chat and again after commit `1515bac`. There is no equivalent for Codex |
| `cargo test -p puricat-core --test live_db` with both servers configured | Pass: 4 passed (unchanged by the agent chat; passing after it) |
| `cargo test -p puricat-core --test live_write` with both servers configured | Pass: 4 passed (2 save-edit tests, 2 Smart Paste tests; unchanged, passing after it) |
| `cargo test -p puricat-core --test live_console` with both servers configured | Pass: 2 passed (`mysql_console`, `postgres_console`; unchanged, passing after it). The second-round console fixes added no live test |
| `cargo test -- --ignored os_keychain` (macOS Keychain) | Pass (recorded before the editing work; not repeated afterwards) |
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean when recorded with the SQL console; no result after the agent chat was recorded here |
| `cargo fmt --check` | Clean when recorded with the SQL console; no result after the agent chat was recorded here |
| `pnpm typecheck` | Clean when recorded with the SQL console; no result after the agent chat was recorded here |
| `pnpm test` (Vitest) | Pass: 39 cases in 6 files |
| `vite build` | Succeeded as part of the bundle build recorded with the SQL console |
| `e2e/run.sh` (dev-server mode), including the editing, Smart Paste, SQL console and agent chat steps (stages `agent-chat`, `agent-codex`) and the runner's checks of what the stand-in CLIs received | Pass, with `problems: []`, on the build of commit `b0e89cd`. **Not repeated after `1515bac`** (see below). An earlier run the same day, before the agent chat, failed once (see "Known instability") |
| `pnpm tauri build --debug --bundles app` | Built `PuriCat.app` (unsigned) when recorded with the SQL console |
| `APP=<bundle binary> e2e/run.sh` (embedded assets, production CSP), **before the agent chat**: the editing, Smart Paste and SQL console steps | Pass, with an empty `problems` list (no CSP violation, uncaught error or unhandled rejection), with a bundle rebuilt after the SQL console work. This is the evidence that the Monaco editor and its worker run under the production CSP |
| Bundled run after the agent chat (`APP=<bundle binary> e2e/run.sh`, embedded assets, production CSP, including the agent chat steps and the runner's checks of what the stand-in CLIs received) | Pass, twice, with `problems: []`, against the bundled debug app built from commit `b0e89cd`. **Not repeated after `1515bac`** (see below) |

Observed during the end-to-end runs: the local MySQL session reported itself encrypted and the local PostgreSQL session not encrypted (default TLS mode `prefer`). In the runs before the agent chat, which used the installed tools, both CLIs were detected as installed and signed in (Claude Code 2.1.303, codex-cli 0.133.0). Since then the run sees the stand-ins ("9.9.9 (stand-in Claude Code)", "codex-cli 0.0.0 (stand-in)"), so it says nothing about the installed CLIs.

The turn handling changed once more after the first results (commit `1515bac`): the CLI now runs in a process group of its own, the first verdict in the stream ends the turn, the stop flag and the deadline are looked at before every read, and Stop or a timeout kills the whole group. Two cases were added to an existing unit test for it, so the number of tests is unchanged. The unit tests (56 passed, 2 ignored) and the opt-in test against the real Claude Code CLI passed again with that change. **The end-to-end run was not repeated after it.** Two further attempts, one in dev mode and one against the bundled app, stalled at the first connection dialog with no error: the development Mac had been idle for more than 30 minutes, and its screen was off and locked (established from `pmset -g log`). Those two attempts say nothing about the code. The end-to-end results in this table therefore describe the build of `b0e89cd`: the turn handling before process groups.

Agent chat against the real services, on this machine: Claude Code answered (the opt-in test above). **Codex never answered**: for every model that was tried the service replied "The '<model>' model is not supported when using Codex with a ChatGPT account", so the lines of a successful Codex turn have not been seen, and PuriCat's handling of them rests on the documented format alone.

A defect found by `live_write` and fixed before these results: the PostgreSQL write path first cast parameters to the column's full type (`$n::varchar(5)`), and an explicit cast to a sized type truncates silently, so an over-long value was stored shortened. Parameters are now cast to the bare type and the column assignment rejects the value ([ADR 0013](docs/adr/0013-staged-edits-and-optimistic-saves.md)).

A defect found by the end-to-end run against the bundled app and fixed before these results: the Monaco editor never appeared in the bundle, although it worked under the dev server. The production CSS build rewrites colour tokens to their shortest spelling (`#ffffff` becomes `#fff`), and Monaco's theme accepts only `#rrggbb` / `#rrggbbaa`. Design tokens are now normalised before they reach the theme (`css()` in `src/features/sql/monaco.ts`). There is no unit test for that function; the bundled end-to-end run is what guards it.

A fault in the test itself, also fixed: one bundled run timed out closing the history menu, because Escape was sent to whichever element had focus at that instant. The step now sends Escape to the menu until it is gone.

Known instability: a run stalls without an error when the screen is off or locked, because the web view of a window that is not shown stops its timers (see "End-to-end smoke test"). That is the established cause of the two stalled attempts after `1515bac`. One earlier dev-server run the same day, during the SQL console verification — the first after a Rust rebuild — timed out the same way, waiting for the MySQL "Test connection" result, and the next three runs passed; for that one the same cause is assumed, not established (it was first put down to a dev-server reload during the first compile). Check the screen first; then treat a single failure as a reason to re-run and a repeated one as a bug.

Known dev-only warning: under `pnpm tauri dev` the web view logs "IPC custom protocol failed, Tauri will now use the postMessage interface" at start-up. IPC still works through the fallback; the cause in the development CSP has not been found. The bundled build could not be checked for this message.

## Gaps

| Not verified | Why |
|---|---|
| SQL Server | No adapter; no disposable instance available (no Docker on the development machine) |
| SSH tunnels | Not implemented |
| TLS `require` / `verify-ca` / `verify-full` and custom CA | Only local servers were used; no TLS-enforcing server |
| Other MySQL / PostgreSQL versions, MariaDB | Not tried. The write path relies on MySQL 9.3 behaviour for `CAST(... AS CHAR)`, strict mode and affected-row counts; older MySQL and MariaDB are unverified |
| Review dialog on a Production-labelled connection; the bulk-threshold trigger | Only the delete-triggered review was exercised end to end |
| Real keyboard and mouse input in the grid | The end-to-end test dispatches synthetic DOM events: native key routing (Cmd+V and the other shortcuts arriving from the OS), IME composition and drag selection are not automated |
| Concurrent-edit conflicts through the UI | Conflict detection is covered at the backend level (`live_write`) only |
| Editing guards other than the close confirmation | Locked paging / sorting / refresh, bulk closes keeping dirty tabs, refusal to disconnect, delete or re-save a connection with dirty tabs: no automated test |
| The read-only refusal in the `apply_changes` command and the read-only label in the grid | The live test covers the server's own refusal, below the IPC layer |
| The 10,000-change limit; saves of hundreds or thousands of rows | Not exercised |
| Smart Paste through the UI on PostgreSQL | Backend-tested only (`live_write`); the end-to-end test runs it on MySQL |
| Smart Paste INSERT and UPDATE modes, the progress bar and "Stop and roll back" through the UI | Backend-tested only |
| Smart Paste with a composite key through the UI | Matching on one is covered by the live test on both engines; no frontend or end-to-end test chooses a composite key |
| Smart Paste pastes near the 10,000-row limit; duration on a remote server | The largest tested paste is 130 rows on a local server. Each written row is one statement and lookups are one query per 40 rows |
| Smart Paste with a malformed key value of a non-integer type (a date, for example), and the exclusion of MySQL expression unique indexes | No test (malformed integer keys are unit- and live-tested) |
| Smart Paste duplicates across lookup chunks against a real server | Unit-tested with simulated database answers; in the live scenario both spellings fall into one lookup |
| Smart Paste dialog: no dismissal during a run (Esc, outside click, close button); the key used after switching from INSERT with "no key" to UPDATE / UPSERT | No automated test |
| Smart Paste IPC layer: the read-only refusal in `apply_paste`, `cancel_job`, the values sent over the progress channel | The live tests call the core functions directly. The end-to-end test goes through `unique_keys`, `analyze_paste` and `apply_paste` but does not check progress or press Stop |
| Smart Paste on a Production-labelled connection | The warning and the danger-styled button have no test |
| SQL console through the UI on PostgreSQL | Backend-tested only (`live_console`); the end-to-end test opens its query tab on MySQL |
| SQL console: Stop through the UI, and the `cancel_sql` command | Cancellation is verified in the backend (`live_console` sets the stop flag and calls the canceller itself); nothing presses Stop or calls the command, and the error shown when the command fails has no test |
| SQL console: the lost-connection path through the UI and the IPC layer | Verified in the backend only: the console reports the loss. Removing the dead console and opening a new one on the next run (`run_sql`) is not exercised |
| SQL console: interrupting a running statement when its tab or session closes | No automated test |
| SQL console: confirmation on a Production-labelled connection; confirming a destructive statement ("Run anyway") | Only the destructive confirmation followed by Cancel is exercised |
| SQL console: "Roll back and close", and the refusal to disconnect, change or delete a connection with a transaction open | The close dialog is opened and dismissed with "Keep the tab"; the rest has no automated test |
| SQL console: a read-only connection | The server's refusal is live-tested through the console on MySQL only. A statement that switches the session back to read-write has not been run by a test on either engine |
| SQL console: transaction tracking beyond `BEGIN` / `COMMIT` / `ROLLBACK` text | MySQL DDL inside a transaction — including the rule that `CREATE` / `DROP TEMPORARY TABLE` leaves it open, whose keyword test alone is unit-tested — a deadlock, a transaction opened by a stored procedure or `XA`: no test against a server |
| SQL console: what the impact guard counts | Triggers, foreign-key cascades, `EXPLAIN ANALYZE` of a row-changing statement and a data-modifying `WITH` are not tested |
| SQL console: results at or near 100,000 rows; memory and grid responsiveness | The largest result in a test is 5,000 rows, capped at 100 |
| SQL console: statements returning several result sets (a MySQL stored procedure) | Not tested; scripts with several statements are |
| SQL console: the row cap inside a user's transaction on MySQL | Tested on PostgreSQL only |
| Export through the Save dialog and the Export menu | The end-to-end test calls `export_result` directly; the menu being disabled during a run and a failing Save dialog are not exercised. XLSX cell types are not read back by any test (the unit test checks which texts convert; the runner checks that the Thai name is in the workbook). A JSON file from a result with two columns of one name is not written by a test (the key naming is unit-tested) |
| History: masking on the `run_sql` path, loading an entry, "Clear history", deletion with the connection | The masking function and the store are unit-tested; the end-to-end test only lists the entries. `delete_connection` clearing the history is not asserted (the end-to-end test deletes a connection that has none). A literal after a double-quoted `"password"` identifier is not tested ([SECURITY.md](SECURITY.md)) |
| Completion and formatting in the SQL editor | No automated test |
| ⌘↵, ⌘T, the palette's "New query", running a selection | No automated test; the end-to-end test uses the explorer menu and the Run button |
| A query tab's saved text and its restoration after a relaunch | No automated test, including the pending save that is dropped when the tab closes |
| Agent chat: Codex's successful path against the real CLI | Never observed: the service refused every model tried from this machine. The parser and the UI are tested against the documented shape only (unit test, stand-in) |
| Agent chat: the real CLIs through the UI | No automated test drives the installed `claude` or `codex` through the window or the IPC commands. The only real-CLI check is the opt-in unit test for Claude Code, run twice by hand |
| Agent chat: that the real CLIs give the model no tools | The arguments are asserted. Their effect is seen only in a captured `system/init` line (`tools: []`); no test asserts it, and nothing probes Codex's sandbox or its shell tool |
| Agent chat: the 10-minute timeout | No test |
| Agent chat: long conversations and large schemas | Trimming at 60,000 characters and the names-only rendering are unit-tested only. The cut after 40 tables (in the `agent_context` command) and after 80 columns is not exercised by any test; no database with more than 40 tables was used |
| Agent chat: contexts other than a PostgreSQL table tab | Through the UI the chat ran with a PostgreSQL table tab in front only. A MySQL context, a query tab's context with its SQL, and the no-tab / closed / no-database states have no test (the unit test renders a PostgreSQL context) |
| Agent chat: Stop while the CLI is printing continuously | The unit test and the end-to-end test stop a stand-in that has gone silent. The stop flag is looked at before every read, so this should work; no test streams while stopping |
| Agent chat: Stop through the UI | Exercised only against the stand-in CLI, and only on the build before `1515bac`. Killing the CLI's whole process group is covered by unit tests with stand-in scripts, not by the end-to-end test |
| Agent chat: the turn handling of commit `1515bac` through the UI | Process group, the turn ending at the stream's verdict, the 5-second grace: covered by stand-in unit-test cases and the opt-in run against the real Claude Code CLI; the end-to-end test was not run again after the change. Killing on a platform other than Unix (there is no group kill there), and a Codex `error` line that is not final, are not tested |
| The end-to-end test with the screen off or locked | Cannot run: the web view of a window that is not shown is suspended, and the script stops advancing without an error until the runner's 5-minute limit. The run needs an awake, unlocked screen with the app window visible |
| Agent chat: the "Share context" switch, "Context sent" per message, "New chat", "Copy", Enter / Shift+Enter / IME in the composer, the signed-out and not-installed panels | No automated test |
| Agent chat: the agent store and the IPC commands on their own | `agent_models`, `agent_context` and `agent_send` are reached only through the end-to-end test on stand-ins; `src/stores/agent.ts` has no unit test |
| Agent chat on Windows | The stand-in tests are Unix-only; nothing was run |
| CLI detection against the installed tools | Was part of the end-to-end run until the stand-ins were introduced; now only the parsers are unit-tested |
| The colour normalisation for the Monaco theme (`css()`) | No unit test; guarded only by the bundled end-to-end run |
| PostgreSQL session time zone | The `TimeZone=UTC` behaviour is a known limitation with no test asserting it |
| Windows, Linux | Not built or run |
| Release-profile build, DMG, signed or notarized builds | Only the unsigned debug-profile `.app` bundle was built and tested; no signing identity |
| CI on GitHub | Workflow has not run |
| Accessibility | No audit |
| Performance on large or wide tables | No measurements. The editable grid has not been measured with many columns or with pages of 1000 wide rows |
| Restricted database accounts | Tests used accounts with broad privileges |
| TypeScript types matching the Rust wire types | Maintained by hand; no check |
| Thai catalog completeness | No check for missing keys |
