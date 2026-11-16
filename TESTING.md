# Testing

## Layers

| Layer | Where | Needs | In `pnpm check` | In CI workflow |
|---|---|---|---|---|
| Rust unit tests | `crates/core/src/**` (`#[cfg(test)]`) | Nothing | Yes | Yes |
| Frontend unit tests (Vitest) | `src/**/*.test.ts` | Nothing | Yes | Yes |
| Static checks | `tsc`, `cargo fmt`, `cargo clippy -D warnings` | Nothing | Yes | Yes |
| Live database tests | `crates/core/tests/live_db.rs` (connect, browse), `crates/core/tests/live_write.rs` (save edits) | Disposable MySQL and PostgreSQL servers | Server tests run only if the variables are set | Server tests skip (variables unset) |
| OS credential store test | `secrets::tests::os_keychain_round_trip` (ignored by default) | A real Keychain / Credential Manager | No | No |
| End-to-end smoke test | `e2e/run.sh`, `e2e/smoke.js` | macOS desktop session, local MySQL and PostgreSQL | No | No |

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
```

### Live database tests

```sh
PURICAT_TEST_MYSQL=you@127.0.0.1:3306      PURICAT_TEST_MYSQL_PASSWORD=...      \
PURICAT_TEST_POSTGRES=you@127.0.0.1:5432   PURICAT_TEST_POSTGRES_PASSWORD=...   \
  cargo test -p puricat-core --test live_db --test live_write -- --nocapture
```

`live_db` covers connecting and browsing; `live_write` covers the save path. Either can be run alone with a single `--test`.

| Variable | Format | Required |
|---|---|---|
| `PURICAT_TEST_MYSQL` | `user@host:port` | To run the MySQL tests |
| `PURICAT_TEST_MYSQL_PASSWORD` | Password | Only if the account has one |
| `PURICAT_TEST_POSTGRES` | `user@host:port` | To run the PostgreSQL tests |
| `PURICAT_TEST_POSTGRES_PASSWORD` | Password | Only if the account has one |

The account must be allowed to create and drop databases. PostgreSQL must support `DROP DATABASE ... WITH (FORCE)`.

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

Requirements: macOS with a logged-in desktop session, `mysql` and `psql` clients, local servers that accept the accounts above without a password, and `swift` when `SHOTS=1`. The report is copied to `target/e2e-report.json`. How it works: [ADR 0012](docs/adr/0012-e2e-testing-on-macos.md).

Side effects to know about:

- In dev-server mode the runner's cleanup kills the debug `puricat` process and whatever is listening on TCP port 1420.
- The smoke test runs the real CLI detection, which executes the installed `claude` and `codex` status commands.
- **The smoke test overwrites the system clipboard** with its paste fixture and writes the previous text back at the end. If the run fails or is interrupted after the paste step, the fixture text stays on the clipboard. Non-text clipboard contents (an image, files) are not restored. Do not copy anything important while it runs.
- The smoke test writes rows through PuriCat into the `puricat_e2e` databases only.

## What each suite covers

### Rust unit tests (24, plus 1 ignored)

| Module | Covers |
|---|---|
| `model` | Input trimming and defaults, rejection of incomplete profiles, stable wire format |
| `store` | Connection CRUD and ordering, update of a missing connection, settings round trip, reopening an existing database file |
| `secrets` | In-memory store round trip; (ignored) real OS credential store round trip |
| `db::ident` | Quoting for MySQL, PostgreSQL and SQL Server, escaping of the quote character, rejection of empty and NUL names |
| `db` | Default order by primary key, validation of the sort column, page limit, truncation on character boundaries, binary prefix |
| `db::mysql`, `db::postgres` | The generated `SELECT` text, type classification; MySQL expression defaults (`DEFAULT_GENERATED`) not treated as generated columns |
| `db::write` | The exact MySQL plan and the exact PostgreSQL plan for one change set (statement order, typed key comparison, bare-type casts, `BIT` handling, concurrency conditions, parameter order, empty insert); refusal of empty, incomplete-key, NULL-key, wrong-key, unknown-column, generated-column, duplicate-column and no-change updates; keyless tables (delete refused, insert planned); hostile key and cell values absent from the SQL text |
| `ai_detect` | Parsing of `claude auth status --json` and `codex login status` output, binary lookup in directories |

### Live database tests: `live_db` (4 tests)

| Test | Covers |
|---|---|
| `mysql_browse_and_page` | Server info, database list with system flag, object kinds, describe (types, nullability, generated, primary key), exact values (Thai text with quotes and a tab, `DECIMAL(24,6)`, `BIGINT` beyond 2^53, `DATETIME(6)`, JSON, BLOB, `BIT(8)`), NULL, truncation flags, paging, sorting, rejected inputs (unknown sort column, page size 0 and 1001, missing table), the session keeping the server's time zone and `sql_mode` (including `PIPES_AS_CONCAT` unchanged) with `STRICT_ALL_TABLES` added, composite key order, reading rows through a view, an expression default not flagged as generated, a table name containing a backtick, keyless table order, server-confirmed read-only, failed login without the password in the message |
| `postgres_browse_and_page` | The same shape for PostgreSQL, plus schemas, system schema flags, enum, array, `uuid`, `jsonb`, `bytea`, `timestamptz`, identity column, view and materialized view listing, reading rows through a view, a name containing a double quote, a profile without a default database, server-confirmed read-only on a named database, failure on an unknown database |
| `unreachable_server_fails_fast_with_the_real_reason` | Both engines against a closed port: a "refused" error in under 5 seconds. Needs no server |
| `sort_spec_wire_format` | Default of `descending`. Needs no server |

### Live database tests: `live_write` (2 tests)

One test per engine (`mysql_save_edits`, `postgres_save_edits`). Each creates its own scratch database, loads fixtures over a separate admin connection, and then saves change sets through `Session::apply_changes`, reading the result back through `Session::fetch_rows`.

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

Not covered by the live tests: a wrong password on PostgreSQL, a wrong password (as opposed to an unknown user) on MySQL, reading rows through a PostgreSQL materialized view, any TLS mode other than the default, the 10,000-change limit, two PuriCat sessions editing the same row, a foreign-key or unique-constraint failure (the end-to-end test covers one foreign-key refusal), the PostgreSQL `TimeZone=UTC` behaviour (documented in [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md), not asserted), and the types listed there as not verified on the write path.

### Frontend unit tests (25 cases in 4 files)

| File | Cases | Covers |
|---|---|---|
| `src/lib/clipboard.test.ts` | 7 | Parsing spreadsheet clipboard text: plain ranges and the final line break, empty cells and rows, Unicode, quoted cells with tabs / line breaks / doubled quotes, ragged rows, an unterminated quote, no reinterpretation of numbers, dates or leading zeros; serialising back with quotes only where needed |
| `src/features/table/pending.test.ts` | 11 | Staging and un-staging an edit, immutability of earlier states (undo relies on it), blockers (non-editable column, truncated cell, deleted row, unaddressable row), empty input vs NULL, adding / filling / dropping new rows, marking and restoring rows, block paste with appended rows and overflow reporting, single-cell fill of a selection, empty clipboard, change-set building with original values, composite keys in key order, undo / redo history |
| `src/stores/workspace.test.ts` | 5 | Tab closing and neighbour activation, tab id uniqueness across connection / database / schema |
| `src/features/table/sort.test.ts` | 2 | The sort cycle of a header click |

There are no component tests: `DataGrid`, `TableTab` and `ReviewDialog` are exercised only by the end-to-end test.

### End-to-end smoke test

Empty welcome state; create and test a MySQL and a PostgreSQL connection through the dialog and save them; a clear error for an unreachable server and the production read-only hint; browse MySQL (tree groups, grid rows, footer); sort ascending and descending; next page; structure view; browse PostgreSQL at the same time in a second tab; AI panel detection for both providers; duplicate and delete through the context menu and confirmation dialog; dark theme and Thai through the command palette; finally the stored connections, UI settings and tab list are read back over IPC.

Editing, in the real window against both engines:

1. MySQL: double-click a cell, type Thai text, see it staged.
2. Paste a 3×2 block (written to the system clipboard through the clipboard plugin) containing doubled quotes and a tab inside a cell; the third row falls past the end and becomes a new row.
3. Undo and redo the paste.
4. Add a row with the button and type straight into it.
5. Mark a row for deletion: Save opens the mandatory review dialog with the planned statements; committing is refused by the server (foreign key); the error is shown, the staged edits are kept and nothing is saved.
6. Restore the row and save directly; the reloaded grid shows exactly what was typed and pasted.
7. Closing a tab with unsaved edits asks first; "Keep editing" keeps the edit; Discard clears it.
8. A keyless table and a view are labelled read-only.
9. PostgreSQL: `abc` in a numeric column is refused by the server with the edit kept staged; a corrected value and a second cell then save.

It uses synthetic DOM events and empty passwords, so it does **not** cover: saving a password through the UI, restoring tabs after a relaunch, native keyboard shortcuts other than the palette (the grid's shortcuts are dispatched as synthetic `keydown` events, so Cmd+V, Cmd+Z and Cmd+S arriving from the operating system are untested), IME composition, drag selection, Shift+arrow selection, copy, the grid's context menu, drag-resizing, the explorer filter or arrow-key navigation. It also does not exercise the review dialog on a Production-labelled connection, the bulk-threshold trigger, a concurrent-edit conflict, the locked paging / sorting controls, the bulk tab closes, or the refusal to disconnect with unsaved edits.

## Safety guarantees

- Live tests create a database named `puricat_it_<random>` per test and drop it afterwards, even when the test body fails. If the test process is killed, a `puricat_it_*` database can be left behind and is safe to drop.
- The live MySQL test attempts one login as `puricat_no_such_user`; it appears in the server's log as a failed login.
- The end-to-end runner drops and recreates only the database `puricat_e2e` on each server, and drops it again on exit unless `KEEP_DB=1`.
- The end-to-end app instance uses a temporary settings directory and an in-memory secret store; the user's real settings and Keychain are not touched.
- The ignored Keychain test writes and deletes one entry under a random `connection/selftest-<uuid>/password` key.
- `live_write` and the end-to-end test issue `INSERT`, `UPDATE` and `DELETE` through PuriCat's own write path, but only against tables in the scratch databases they created (`puricat_it_<random>`, `puricat_e2e`). `live_db` issues only reads through PuriCat. DDL and fixtures are run by separate admin connections.
- The end-to-end test replaces the text on the system clipboard and restores the previous text when it finishes (see the side effects above).
- Never point `PURICAT_TEST_*` at a server that holds real data. The tests need broad privileges and the guarantees above are only as good as the code.

## Results — 2026-11-16

Environment: macOS 26 on Apple Silicon, Rust 1.99, Node 25, pnpm 10, local MySQL 9.3.0 and PostgreSQL 16.13. Run on the development machine, not in CI.

| Check | Result |
|---|---|
| `cargo test -p puricat-core` (unit) | Pass: 24 passed, 1 ignored |
| `cargo test -p puricat-core --test live_db` with both servers configured | Pass: 4 passed |
| `cargo test -p puricat-core --test live_write` with both servers configured | Pass: 2 passed |
| `cargo test -- --ignored os_keychain` (macOS Keychain) | Pass (recorded before the editing work; not repeated afterwards) |
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean |
| `cargo fmt --check` | Clean |
| `pnpm typecheck` | Clean |
| `pnpm test` (Vitest) | Pass: 25 cases in 4 files |
| `vite build` | Succeeds (as part of the bundle build) |
| `e2e/run.sh` (dev-server mode), including the editing steps | Pass. An earlier run the same day failed once without explanation (see below) |
| `pnpm tauri build --debug --bundles app` | Builds `PuriCat.app` (unsigned) |
| `APP=<bundle binary> e2e/run.sh` (embedded assets, production CSP), including the editing steps | Pass |

Observed during the end-to-end run: the local MySQL session reported itself encrypted and the local PostgreSQL session not encrypted (default TLS mode `prefer`); both CLIs were detected as installed and signed in (Claude Code 2.1.303, codex-cli 0.133.0).

A defect found by `live_write` and fixed before these results: the PostgreSQL write path first cast parameters to the column's full type (`$n::varchar(5)`), and an explicit cast to a sized type truncates silently, so an over-long value was stored shortened. Parameters are now cast to the bare type and the column assignment rejects the value ([ADR 0013](docs/adr/0013-staged-edits-and-optimistic-saves.md)).

Known instability: one dev-server run, the first after a Rust rebuild, timed out waiting for the MySQL "Test connection" result; the next three runs passed. The cause is not established (a dev-server reload during the first compile is the suspect). Treat a single failure as a reason to re-run and a repeated one as a bug.

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
| PostgreSQL session time zone | The `TimeZone=UTC` behaviour is a known limitation with no test asserting it |
| Windows, Linux | Not built or run |
| Release-profile build, DMG, signed or notarized builds | Only the unsigned debug-profile `.app` bundle was built and tested; no signing identity |
| CI on GitHub | Workflow has not run |
| Accessibility | No audit |
| Performance on large or wide tables | No measurements. The editable grid has not been measured with many columns or with pages of 1000 wide rows |
| Restricted database accounts | Tests used accounts with broad privileges |
| TypeScript types matching the Rust wire types | Maintained by hand; no check |
| Thai catalog completeness | No check for missing keys |
