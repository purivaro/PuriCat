# Architecture

Describes the code as it exists on 2026-11-16. Planned pieces are marked as such. Decisions and their trade-offs are in [docs/adr/](docs/adr/README.md).

## Components

```
+--------------------------- PuriCat process ---------------------------+
|                                                                        |
|  Web view (WKWebView)                 Rust                             |
|  +---------------------------+        +-----------------------------+  |
|  | React UI  (src/)          |  IPC   | src-tauri  (crate puricat)  |  |
|  |  components / features    | <----> |  commands.rs  IPC handlers  |  |
|  |  Zustand stores           | invoke |  state.rs     AppState      |  |
|  |  lib/ipc.ts typed wrapper |        +--------------+--------------+  |
|  +---------------------------+                       |                 |
|   no shell / fs / http access         +--------------v--------------+  |
|                                       | crates/core (puricat-core)  |  |
|                                       |  model    store    secrets  |  |
|                                       |  db::{mysql, postgres}      |  |
|                                       |  ai_detect                  |  |
|                                       +--+-------+-------+-------+--+  |
+------------------------------------------|-------|-------|-------|-----+
                                           |       |       |       |
                                  puricat.db   OS credential   MySQL /      claude / codex
                                  (SQLite,     store           PostgreSQL   (version and auth
                                  app data)    (Keychain)      servers      status only)
```

| Component | Responsibility | Must not |
|---|---|---|
| `src/` (React) | Rendering, interaction, view state | Hold stored passwords, build SQL, touch files or processes |
| `src-tauri` (`puricat`) | IPC commands, open sessions, choice of secret store, debug-only test hooks | Contain database or storage logic |
| `crates/core` (`puricat-core`) | Profiles, settings store, secrets, adapters, CLI discovery | Depend on Tauri |

`puricat-core` has no Tauri dependency so it can be unit-tested without a window and reused by the planned backup helper and MCP bridge binaries ([ADR 0002](docs/adr/0002-core-crate-and-thin-ipc.md)).

## Core modules

| Module | Contents |
|---|---|
| `model` | `ConnectionProfile`, `Engine` (`mysql`, `postgres`, `sqlserver` — the last has no adapter), `Environment`, `TlsMode`; input normalisation |
| `store` | SQLite settings database: `connections` (one JSON profile per row plus timestamps) and `settings` (key/value). Append-only migrations keyed on `PRAGMA user_version`; a database from a newer build is refused |
| `secrets` | `SecretStore` trait; `OsKeychain` (via `keyring`) and `MemoryStore` (tests, debug runs). Service `PuriCat`, key `connection/<id>/password` |
| `db` | `Session` enum (`MySql`, `Postgres`), `Capabilities`, shared paging/truncation logic, `ident` quoting, one module of SQL per engine |
| `ai_detect` | Locates `claude` / `codex` and reads their version and sign-in state |
| `error` | `Error` serialised to the UI as `{ kind, message }` |

## Local data

| Data | Location | Notes |
|---|---|---|
| Profiles, UI settings, open tabs | `puricat.db` in the OS app-data directory for identifier `app.puricat.desktop` (macOS: `$HOME/Library/Application Support/app.puricat.desktop/`) | File mode 0600 on Unix. The identifier is a placeholder; changing it later orphans this data |
| Passwords | OS credential store | Never written to `puricat.db` |
| Theme hint | Web view `localStorage` key `puricat.theme` | Avoids a flash of the wrong theme before settings load |

## IPC surface

All commands are registered in `src-tauri/src/lib.rs` and implemented in `commands.rs`. Errors arrive as `{ kind, message }`.

| Command | Arguments | Returns |
|---|---|---|
| `list_connections` | — | Profiles, each with `hasPassword` |
| `save_connection` | `profile`, `password` (`keep` / `set` / `clear`) | Saved profile, `sessionClosed` |
| `duplicate_connection` | `id`, `name` | New profile (password copied inside the credential store) |
| `delete_connection` | `id` | — (closes the session, deletes the password) |
| `test_connection` | `profile`, `password` | `ServerInfo`; nothing is saved |
| `open_connection` | `id` | `ServerInfo`; session kept in `AppState` |
| `close_connection` | `id` | — |
| `list_databases` | `id` | Namespaces |
| `list_schemas` | `id`, `database` | Namespaces (PostgreSQL only) |
| `list_objects` | `id`, `database`, `schema?` | Tables, views, materialized views |
| `fetch_rows` | `id`, `table`, `page` (`offset`, `limit`, `sort?`) | `RowPage`: columns, rows, truncation flags, primary key, estimated rows |
| `get_settings` | — | All key/value settings |
| `set_setting` | `key`, `value` | — |
| `detect_ai_clis` | — | Install and sign-in status per provider |
| `e2e_report` | `text` | Debug builds with `PURICAT_E2E_REPORT` set: writes the report file. Otherwise returns an `unsupported` error |

No command returns a stored password. A saved session is closed when the password or any connection-affecting field changes; cosmetic fields (name, environment, favorite, tags) leave it open.

## Data flow: open a table

1. The user clicks a table in the explorer. `useWorkspace.openTable` adds or activates a tab and stores the tab list with `set_setting("workspace", ...)`.
2. The tab view, once visible and connected, calls `fetch_rows`; the page carries the column metadata, primary key and row estimate, so the structure view needs no second call. Hidden restored tabs do not query until shown.
3. `AppState::session(id)` returns the open `Session` (an error if the connection is not open).
4. `Session::fetch_rows` checks the page size (1–1000), reads table metadata from the catalogue with bound parameters, and resolves the order: the requested sort column must exist in the metadata, then primary-key columns are appended.
5. The engine module builds one `SELECT` in which every column is rendered to text by the server, with identifiers quoted by `ident` and `LIMIT`/`OFFSET` bound as parameters. It asks for `limit + 1` rows and one unit beyond each preview length.
6. `finish_page` trims to the limit, sets `hasMore`, cuts long cells on a character boundary, records them in `truncated`, and prefixes binary previews with `0x`.
7. The grid renders the page with row virtualization; a response superseded by a newer request is dropped.


## Sessions and pools

| Engine | Pooling |
|---|---|
| MySQL | One pool per connection, max 4 connections |
| PostgreSQL | One pool per database, created on first use, max 3 connections each (a PostgreSQL connection is bound to one database) |

Both: idle timeout 600 s; acquire timeout equals the profile's connect timeout; a direct probe connection is made before the pool so the user sees the driver's real error; the read-only statement runs on every new pooled connection. Sessions live in `AppState` keyed by connection id. There is no reconnect or cancellation logic yet (Phase 2b).

## Frontend state

| Store (`src/stores`) | Holds | Persisted |
|---|---|---|
| `connections` | Profiles, session state per connection, lazily loaded explorer levels, expanded nodes | Profiles only, in the backend store |
| `workspace` | Tabs and the active tab | Setting `workspace` |
| `settings` | Theme, language, panel visibility and widths | Setting `ui` |
| `ui` | Dialog and palette open state | No |

Table data is local component state in the tab view and is never persisted. `src/lib/types.ts` mirrors the serde types in `crates/core` by hand.

## Where future pieces attach

| Piece | Attachment point | Phase |
|---|---|---|
| SQL Server adapter | New `Session` variant and module; the compiler lists every `match` to extend. `ident::sqlserver` already exists | 2b |
| SSH tunnels | In `puricat-core`, before `Session::connect`: open a local forward, connect the driver to it, tie its lifetime to the session | 2b |
| Editing | New write methods on `Session`, gated by `Capabilities` and the primary key from `TableMeta` | 3 |
| SQL editor | New tab kind in `workspace`; new execution path in `db` ([ADR 0004](docs/adr/0004-server-side-text-rendering.md) explains why the browse path cannot be reused as is) | 4 |
| AI agent | Subprocess runner and a PuriCat-owned MCP server built on `puricat-core`; approval queue in the UI | 5 |
| Backups | `puricat-backup` helper binary on `puricat-core`, run by the OS scheduler | 6 |
