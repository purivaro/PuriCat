# PuriCat

PuriCat is a desktop database client built with Tauri 2 (Rust backend, React/TypeScript frontend). The goal is a client that covers routine database work for MySQL, PostgreSQL and SQL Server with spreadsheet-style editing, an AI agent driven through locally installed CLIs, and scheduled backups. macOS (Apple Silicon) is the first target; Windows follows later.

> **Status: early development.** Phase 1 (runnable foundation) is complete; Phase 2 (database connectivity) and Phase 3 (editing) are partially complete. This build can connect to MySQL and PostgreSQL, browse tables, **edit data in tables that have a primary key** (spreadsheet-style, staged until Save), and load a block copied from a spreadsheet with the **Smart Paste wizard** (INSERT / UPDATE / UPSERT matched on a unique key, previewed before anything runs). It cannot run SQL, tunnel over SSH, connect to SQL Server, talk to an AI agent or take backups. **Not ready for production data.** Builds are unsigned.

## Feature status

Labels used across the docs:

- **Implemented & verified** — in the code and exercised by a test or check that passed (see [TESTING.md](TESTING.md)).
- **Implemented, not yet verified** — in the code, but no test or run has confirmed it.
- **Planned (Phase N)** — not in the code.

| Area | Capability | Status |
|---|---|---|
| Shell | Tauri window, custom title bar, resizable explorer / workspace / AI panels, multi-tab workspace | Implemented & verified (E2E) |
| Shell | Light / dark / system theme, English and Thai, command palette | Implemented & verified (E2E) |
| Shell | Keyboard shortcuts other than the palette, explorer filter and arrow-key navigation, panel and column resize | Implemented, not yet verified (no automated test) |
| Connections | Create, edit, test, duplicate, favorite, tag, environment label, delete with confirmation | Implemented & verified (create, test, duplicate, delete in E2E); edit, favorite and tags not covered by a test |
| Connections | Passwords in the OS credential store (macOS Keychain) | Implemented & verified (Keychain round trip) |
| Connections | Windows Credential Manager | Implemented, not yet verified |
| Engines | MySQL: connect, list databases / tables / views, describe, browse | Implemented & verified (MySQL 9.3.0) |
| Engines | PostgreSQL: connect, list databases / schemas / tables / views / materialized views, describe, browse | Implemented & verified (PostgreSQL 16.13) |
| Engines | SQL Server | Planned (Phase 2b) |
| Browsing | Virtualized grid: sort, page, exact server-rendered values, truncation flags, NULL vs empty; structure view | Implemented & verified |
| Browsing | Views, materialized views, tables without a primary key and read-only connections open read-only, with the reason shown | Implemented & verified (E2E: view and keyless table); the read-only-connection label has no automated test |
| Editing | Staged edits on MySQL and PostgreSQL tables with a primary key: in-cell editor, add / delete / restore rows, undo / redo, Save / Discard; nothing is written before Save | Implemented & verified (unit, live and E2E) |
| Editing | Range selection, copy, and direct paste from Excel / Google Sheets / Numbers (quoted cells, embedded tabs and line breaks, values never reinterpreted) | Implemented & verified (parser, serialiser and paste logic unit-tested; paste, undo / redo in E2E). Copy, drag and Shift+arrow selection, and native key input are not exercised end to end |
| Editing | Save in one transaction: parameterised statements, rows addressed by the complete primary key, optimistic concurrency check, exactly-one-row rule, rollback on any failure | Implemented & verified (live tests on both engines) |
| Editing | Not editable yet: generated / identity / auto-increment columns, binary and MySQL spatial columns, cells truncated for preview, rows whose key is NULL or truncated | Implemented & verified (refusals unit- and live-tested) |
| Editing | Smart Paste wizard: reads a spreadsheet block from the clipboard; header-row detection, column mapping, INSERT / UPDATE / UPSERT matched on the primary key or a unique index (composite keys accepted); read-only preview with one outcome per pasted row; runs as one transaction with progress and "Stop and roll back"; at most 10,000 pasted rows | Implemented & verified in the backend on both engines (unit and live tests: all three modes, single-column and composite keys). Through the UI only UPSERT on a single-column unique key on MySQL is verified (E2E). Composite keys, INSERT / UPDATE modes, the progress bar, Stop and PostgreSQL through the UI are implemented, not yet verified |
| Editing | Smart Paste: no native upsert statements. PuriCat matches rows itself and runs plain parameterised INSERT and UPDATE statements; `ON DUPLICATE KEY UPDATE`, `ON CONFLICT` and `MERGE` are deliberately not used ([ADR 0014](docs/adr/0014-smart-paste-match-then-write.md)) | Implemented & verified (the shared planner emits only single-row INSERT and UPDATE statements; unit and live tests) |
| Editing | Smart Paste follow-ups: "skip invalid rows", value-aware comparison (`1.5` vs a stored `1.500000` counts as an update today), more than 10,000 rows, CSV / XLSX files | Planned (Phase 3; larger and file-based imports with the Phase 4 import tool) |
| Editing | Full-value cell editor, filtering, editing across pages | Planned (Phase 3) |
| Safety | Review step showing the exact SQL and bound values: mandatory for Production-labelled connections, for any delete, and above a per-connection row threshold (default 10) | Implemented & verified for the delete-triggered path (E2E); the production and threshold paths are implemented, not yet verified through the UI |
| Safety | Unsaved-edit guards: tab dot, confirmation before closing a dirty tab, paging / sorting / refresh locked, disconnect and connection changes refused while edits are pending | Implemented & verified for the close confirmation (E2E); the other guards have no automated test |
| Safety | Server-enforced read-only session option | Implemented & verified (server reports the flag and refuses a write) |
| Safety | TLS mode `prefer` (default) | Implemented & verified against local servers only |
| Safety | TLS modes `disable` / `require` / `verify-ca` / `verify-full`, custom CA file | Implemented, not yet verified |
| Safety | SSH tunnels | Planned (Phase 2b) |
| AI | Detect installed Claude Code / Codex CLIs and their sign-in state | Implemented & verified |
| AI | Agent chat, schema tools, approval queue | Planned (Phase 5) |
| SQL | Query editor, results, history, import/export | Planned (Phase 4) |
| Backups | Manual and scheduled backup, restore | Planned (Phase 6) |
| Release | Signed and notarized builds, updater, Windows | Planned (Phase 7) |

Details per engine are in [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md).

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.85 or newer (`rust-version` in `Cargo.toml`) | `cargo` must be on `PATH`; the Tauri CLI and `pnpm check` call it directly |
| Node.js | 22 or newer | CI uses 22; development used 25 |
| pnpm | 10 | |
| macOS | 12.0 or newer (bundle minimum) | Only macOS 26 on Apple Silicon has been tested. Tauri's usual platform prerequisites apply (Xcode Command Line Tools) |

Windows and Linux builds have not been attempted.

## Run

```sh
pnpm install
pnpm tauri dev
```

The first build compiles the Rust workspace and takes several minutes. The UI only works inside the Tauri window; opening the Vite URL in a browser shows a notice instead.

Unsigned debug builds can trigger a Keychain prompt after each rebuild. For development without the Keychain (debug builds only):

```sh
PURICAT_SECRET_STORE=memory pnpm tauri dev   # passwords kept in memory, lost on exit
```

## Build

```sh
pnpm tauri build
```

Produces an **unsigned** `.app` and `.dmg` (bundle targets in `src-tauri/tauri.conf.json`). The frontend production bundle (`vite build`) is verified; the full `pnpm tauri build` output has not been verified, and signing/notarization is Planned (Phase 7).

## Test

```sh
pnpm check        # typecheck, Vitest, cargo fmt --check, clippy -D warnings, cargo test --workspace

# Live database tests: point them at DISPOSABLE servers only.
PURICAT_TEST_MYSQL=you@127.0.0.1:3306 PURICAT_TEST_MYSQL_PASSWORD=... \
PURICAT_TEST_POSTGRES=you@127.0.0.1:5432 PURICAT_TEST_POSTGRES_PASSWORD=... \
  cargo test -p puricat-core --test live_db --test live_write

e2e/run.sh        # end-to-end smoke test in the real window (macOS, local MySQL + PostgreSQL; uses only a database named puricat_e2e)
```

`live_db` covers connecting and browsing; `live_write` covers saving edits and Smart Paste. Both create and drop their own scratch databases. Without the `PURICAT_TEST_*` variables the live tests skip themselves and still report "ok". The end-to-end test temporarily overwrites the system clipboard and restores the previous text. See [TESTING.md](TESTING.md) for what each suite covers and the recorded results.

## Repository layout

```
crates/core/        puricat-core: model, settings store, secrets, DB adapters, AI CLI discovery (no Tauri dependency)
  src/db/           Session enum, MySQL and PostgreSQL adapters, identifier quoting, write planner (write.rs), Smart Paste matching (smart.rs)
  tests/            live_db.rs (browse) and live_write.rs (save edits, Smart Paste): integration tests against real servers
src-tauri/          puricat: Tauri app, IPC commands, app state, capabilities, bundle config
src/                React frontend (components, features, stores, i18n)
e2e/                end-to-end smoke test (runner, injected script, SQL fixtures)
branding/           icon artwork
scripts/            icon processing script
docs/               product brief and architecture decision records
```

## Documentation

| Document | Contents |
|---|---|
| [REQUIREMENTS.md](REQUIREMENTS.md) | Requirements, acceptance criteria and status |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Components, data flow, IPC surface |
| [docs/adr/](docs/adr/README.md) | Architecture decision records |
| [SECURITY.md](SECURITY.md) | Threat model, credentials, TLS, AI permissions |
| [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md) | Engine / version / feature matrix |
| [AI_INTEGRATION.md](AI_INTEGRATION.md) | CLI detection today, planned agent protocol |
| [BACKUP_RESTORE.md](BACKUP_RESTORE.md) | Backup design and limits (nothing implemented yet) |
| [TESTING.md](TESTING.md) | Test layers, commands, results, gaps |
| [ROADMAP.md](ROADMAP.md) | Phases, status, blockers |
| [LICENSES.md](LICENSES.md) | Dependency licences and distribution obligations |
| [docs/PuriCat_Master_Requirements.md](docs/PuriCat_Master_Requirements.md) | Product brief (source of truth) |

## License

PuriCat's own licence has not been decided; the workspace is marked `publish = false`. See [LICENSES.md](LICENSES.md).
