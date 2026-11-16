# Testing

## Layers

| Layer | Where | Needs | In `pnpm check` | In CI workflow |
|---|---|---|---|---|
| Rust unit tests | `crates/core/src/**` (`#[cfg(test)]`) | Nothing | Yes | Yes |
| Frontend unit tests (Vitest) | `src/**/*.test.ts` | Nothing | Yes | Yes |
| Static checks | `tsc`, `cargo fmt`, `cargo clippy -D warnings` | Nothing | Yes | Yes |
| Live database tests | `crates/core/tests/live_db.rs` | Disposable MySQL and PostgreSQL servers | Server tests run only if the variables are set | Server tests skip (variables unset) |
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
  cargo test -p puricat-core --test live_db -- --nocapture
```

| Variable | Format | Required |
|---|---|---|
| `PURICAT_TEST_MYSQL` | `user@host:port` | To run the MySQL test |
| `PURICAT_TEST_MYSQL_PASSWORD` | Password | Only if the account has one |
| `PURICAT_TEST_POSTGRES` | `user@host:port` | To run the PostgreSQL test |
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

Side effects to know about: in dev-server mode the runner's cleanup kills the debug `puricat` process and whatever is listening on TCP port 1420. The smoke test also runs the real CLI detection, which executes the installed `claude` and `codex` status commands.

## What each suite covers

### Rust unit tests (21, plus 1 ignored)

| Module | Covers |
|---|---|
| `model` | Input trimming and defaults, rejection of incomplete profiles, stable wire format |
| `store` | Connection CRUD and ordering, update of a missing connection, settings round trip, reopening an existing database file |
| `secrets` | In-memory store round trip; (ignored) real OS credential store round trip |
| `db::ident` | Quoting for MySQL, PostgreSQL and SQL Server, escaping of the quote character, rejection of empty and NUL names |
| `db` | Default order by primary key, validation of the sort column, page limit, truncation on character boundaries, binary prefix |
| `db::mysql`, `db::postgres` | The generated `SELECT` text, type classification |
| `ai_detect` | Parsing of `claude auth status --json` and `codex login status` output, binary lookup in directories |

### Live database tests (4 tests)

| Test | Covers |
|---|---|
| `mysql_browse_and_page` | Server info, database list with system flag, object kinds, describe (types, nullability, generated, primary key), exact values (Thai text with quotes and a tab, `DECIMAL(24,6)`, `BIGINT` beyond 2^53, `DATETIME(6)`, JSON, BLOB, `BIT(8)`), NULL, truncation flags, paging, sorting, rejected inputs (unknown sort column, page size 0 and 1001, missing table), composite key order, a table name containing a backtick, keyless table order, server-confirmed read-only, failed login without the password in the message |
| `postgres_browse_and_page` | The same shape for PostgreSQL, plus schemas, system schema flags, enum, array, `uuid`, `jsonb`, `bytea`, `timestamptz`, identity column, view and materialized view listing, reading rows through a view, a name containing a double quote, a profile without a default database, server-confirmed read-only on a named database, failure on an unknown database |
| `unreachable_server_fails_fast_with_the_real_reason` | Both engines against a closed port: a "refused" error in under 5 seconds. Needs no server |
| `sort_spec_wire_format` | Default of `descending`. Needs no server |

Not covered by the live tests: a wrong password on PostgreSQL, a wrong password (as opposed to an unknown user) on MySQL, reading rows through a PostgreSQL materialized view, an actual write being rejected in a read-only session, any TLS mode other than the default.

### Frontend unit tests (7 cases)

Tab closing and neighbour activation, tab id uniqueness across connection / database / schema, the sort cycle of a header click.

### End-to-end smoke test

Empty welcome state; create and test a MySQL and a PostgreSQL connection through the dialog and save them; a clear error for an unreachable server and the production read-only hint; browse MySQL (tree groups, grid rows, footer); sort ascending and descending; next page; structure view; browse PostgreSQL at the same time in a second tab; AI panel detection for both providers; duplicate and delete through the context menu and confirmation dialog; dark theme and Thai through the command palette; finally the stored connections, UI settings and tab list are read back over IPC.

It uses synthetic DOM events and empty passwords, so it does **not** cover: saving a password through the UI, restoring tabs after a relaunch, native keyboard shortcuts other than the palette, the clipboard, drag-resizing, the explorer filter or arrow-key navigation.

## Safety guarantees

- Live tests create a database named `puricat_it_<random>` per test and drop it afterwards, even when the test body fails. If the test process is killed, a `puricat_it_*` database can be left behind and is safe to drop.
- The live MySQL test attempts one login as `puricat_no_such_user`; it appears in the server's log as a failed login.
- The end-to-end runner drops and recreates only the database `puricat_e2e` on each server, and drops it again on exit unless `KEEP_DB=1`.
- The end-to-end app instance uses a temporary settings directory and an in-memory secret store; the user's real settings and Keychain are not touched.
- The ignored Keychain test writes and deletes one entry under a random `connection/selftest-<uuid>/password` key.
- No test issues `INSERT`, `UPDATE`, `DELETE` or DDL through PuriCat's own adapters; fixtures are loaded by separate admin connections.
- Never point `PURICAT_TEST_*` at a server that holds real data. The tests need broad privileges and the guarantees above are only as good as the code.

## Results — 2026-11-16

Environment: macOS 26 on Apple Silicon, Rust 1.99, Node 25, pnpm 10, local MySQL 9.3.0 and PostgreSQL 16.13. Run on the development machine, not in CI.

| Check | Result |
|---|---|
| `cargo test -p puricat-core` (unit) | Pass |
| `cargo test -p puricat-core --test live_db` with both servers configured | Pass |
| `cargo test -- --ignored os_keychain` (macOS Keychain) | Pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean |
| `cargo fmt --check` | Clean |
| `pnpm typecheck` | Clean |
| `pnpm test` (Vitest) | Pass |
| `vite build` | Succeeds |
| `e2e/run.sh` (dev-server mode) | Pass, with one unexplained failure in about ten runs (see below) |
| `pnpm tauri build --debug --bundles app` | Builds `PuriCat.app` (unsigned) |
| `APP=<bundle binary> e2e/run.sh` (embedded assets, production CSP) | Pass |

Observed during the end-to-end run: the local MySQL session reported itself encrypted and the local PostgreSQL session not encrypted (default TLS mode `prefer`); both CLIs were detected as installed and signed in (Claude Code 2.1.303, codex-cli 0.133.0).

Known instability: one dev-server run, the first after a Rust rebuild, timed out waiting for the MySQL "Test connection" result; the next three runs passed. The cause is not established (a dev-server reload during the first compile is the suspect). Treat a single failure as a reason to re-run and a repeated one as a bug.

Known dev-only warning: under `pnpm tauri dev` the web view logs "IPC custom protocol failed, Tauri will now use the postMessage interface" at start-up. IPC still works through the fallback; the cause in the development CSP has not been found. The bundled build could not be checked for this message.

## Gaps

| Not verified | Why |
|---|---|
| SQL Server | No adapter; no disposable instance available (no Docker on the development machine) |
| SSH tunnels | Not implemented |
| TLS `require` / `verify-ca` / `verify-full` and custom CA | Only local servers were used; no TLS-enforcing server |
| Other MySQL / PostgreSQL versions, MariaDB | Not tried |
| Windows, Linux | Not built or run |
| Release-profile build, DMG, signed or notarized builds | Only the unsigned debug-profile `.app` bundle was built and tested; no signing identity |
| CI on GitHub | Workflow has not run |
| Accessibility | No audit |
| Performance on large or wide tables | No measurements |
| Restricted database accounts | Tests used accounts with broad privileges |
| TypeScript types matching the Rust wire types | Maintained by hand; no check |
| Thai catalog completeness | No check for missing keys |
