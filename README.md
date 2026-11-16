# PuriCat

PuriCat is a desktop database client built with Tauri 2 (Rust backend, React/TypeScript frontend). The goal is a client that covers routine database work for MySQL, PostgreSQL and SQL Server with spreadsheet-style editing, an AI agent driven through locally installed CLIs, and scheduled backups. macOS (Apple Silicon) is the first target; Windows follows later.

> **Status: early development.** Phase 1 (runnable foundation) is complete and Phase 2 (database connectivity) is partially complete. This build can connect to MySQL and PostgreSQL and browse tables **read-only**. It cannot edit data, run SQL, tunnel over SSH, talk to an AI agent or take backups. **Not ready for production data.** Builds are unsigned.

## Feature status

Labels used across the docs:

- **Implemented & verified** — in the code and exercised by a test or check that passed (see [TESTING.md](TESTING.md)).
- **Implemented, not yet verified** — in the code, but no test or run has confirmed it.
- **Planned (Phase N)** — not in the code.

| Area | Capability | Status |
|---|---|---|
| Shell | Tauri window, custom title bar, resizable explorer / workspace / AI panels, multi-tab workspace | Implemented & verified (E2E) |
| Shell | Light / dark / system theme, English and Thai, command palette | Implemented & verified (E2E) |
| Shell | Keyboard shortcuts other than the palette, explorer filter and arrow-key navigation, panel and column resize, copy cell/row | Implemented, not yet verified (no automated test) |
| Connections | Create, edit, test, duplicate, favorite, tag, environment label, delete with confirmation | Implemented & verified (create, test, duplicate, delete in E2E); edit, favorite and tags not covered by a test |
| Connections | Passwords in the OS credential store (macOS Keychain) | Implemented & verified (Keychain round trip) |
| Connections | Windows Credential Manager | Implemented, not yet verified |
| Engines | MySQL: connect, list databases / tables / views, describe, browse | Implemented & verified (MySQL 9.3.0) |
| Engines | PostgreSQL: connect, list databases / schemas / tables / views / materialized views, describe, browse | Implemented & verified (PostgreSQL 16.13) |
| Engines | SQL Server | Planned (Phase 2b) |
| Browsing | Read-only virtualized grid: sort, page, exact server-rendered values, truncation flags, NULL vs empty | Implemented & verified |
| Safety | Server-enforced read-only session option | Implemented & verified (server reports the flag) |
| Safety | TLS mode `prefer` (default) | Implemented & verified against local servers only |
| Safety | TLS modes `disable` / `require` / `verify-ca` / `verify-full`, custom CA file | Implemented, not yet verified |
| Safety | SSH tunnels | Planned (Phase 2b) |
| AI | Detect installed Claude Code / Codex CLIs and their sign-in state | Implemented & verified |
| AI | Agent chat, schema tools, approval queue | Planned (Phase 5) |
| Editing | Editable grid, staged changes, Smart Paste | Planned (Phase 3) |
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
  cargo test -p puricat-core --test live_db

e2e/run.sh        # end-to-end smoke test in the real window (macOS, local MySQL + PostgreSQL; uses only a database named puricat_e2e)
```

Without the `PURICAT_TEST_*` variables the live tests skip themselves and still report "ok". See [TESTING.md](TESTING.md) for what each suite covers and the recorded results.

## Repository layout

```
crates/core/        puricat-core: model, settings store, secrets, DB adapters, AI CLI discovery (no Tauri dependency)
  src/db/           Session enum, MySQL and PostgreSQL adapters, identifier quoting
  tests/live_db.rs  integration tests against real servers
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
