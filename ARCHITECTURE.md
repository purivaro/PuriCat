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
|   no shell / fs / http access;        +--------------v--------------+  |
|   plain-text clipboard only           | crates/core (puricat-core)  |  |
|                                       |  model    store    secrets  |  |
|                                       |  db::{mysql, postgres,write}|  |
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
| `src/` (React) | Rendering, interaction, view state, staged edits | Hold stored passwords, build SQL, touch files or processes |
| `src-tauri` (`puricat`) | IPC commands, open sessions, choice of secret store, clipboard plugin registration, debug-only test hooks | Contain database or storage logic |
| `crates/core` (`puricat-core`) | Profiles, settings store, secrets, adapters, CLI discovery | Depend on Tauri |

`puricat-core` has no Tauri dependency so it can be unit-tested without a window and reused by the planned backup helper and MCP bridge binaries ([ADR 0002](docs/adr/0002-core-crate-and-thin-ipc.md)).

## Core modules

| Module | Contents |
|---|---|
| `model` | `ConnectionProfile`, `Engine` (`mysql`, `postgres`, `sqlserver` — the last has no adapter), `Environment`, `TlsMode`; input normalisation |
| `store` | SQLite settings database: `connections` (one JSON profile per row plus timestamps) and `settings` (key/value). Append-only migrations keyed on `PRAGMA user_version`; a database from a newer build is refused |
| `secrets` | `SecretStore` trait; `OsKeychain` (via `keyring`) and `MemoryStore` (tests, debug runs). Service `PuriCat`, key `connection/<id>/password` |
| `db` | `Session` enum (`MySql`, `Postgres`), `Capabilities`, shared paging/truncation logic, `ident` quoting, one module of SQL per engine |
| `db::write` | Turns a change set (updates, inserts, deletes) into parameterised statements and enforces the write rules; the engine modules execute the plan in one transaction ([ADR 0013](docs/adr/0013-staged-edits-and-optimistic-saves.md)) |
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
| `fetch_rows` | `id`, `table`, `page` (`offset`, `limit`, `sort?`) | `RowPage`: columns (with `editable`), rows, truncation flags, primary key, estimated rows |
| `plan_changes` | `id`, `table`, `changes` (`updates`, `inserts`, `deletes`) | The statements a save would run, in order: `kind`, `sql`, `params`, row label. Nothing is executed |
| `apply_changes` | `id`, `table`, `changes` | Counts of updated / inserted / deleted rows and elapsed time. One transaction; refused up front when the saved profile is read-only |
| `get_settings` | — | All key/value settings |
| `set_setting` | `key`, `value` | — |
| `detect_ai_clis` | — | Install and sign-in status per provider |
| `e2e_report` | `text` | Debug builds with `PURICAT_E2E_REPORT` set: writes the report file. Otherwise returns an `unsupported` error |

No command returns a stored password. A saved session is closed when the password or any connection-affecting field changes; cosmetic fields (name, environment, favorite, tags, review threshold) leave it open.

Besides these commands the web view may call two commands of the official `tauri-plugin-clipboard-manager`, `read_text` and `write_text`, for copy and paste in the grid (`src/lib/clipboard.ts`). The browser clipboard API is not used, so no permission prompt appears.

## Data flow: open a table

1. The user clicks a table in the explorer. `useWorkspace.openTable` adds or activates a tab and stores the tab list with `set_setting("workspace", ...)`.
2. The tab view, once visible and connected, calls `fetch_rows`; the page carries the column metadata, primary key and row estimate, so the structure view needs no second call. Hidden restored tabs do not query until shown.
3. `AppState::session(id)` returns the open `Session` (an error if the connection is not open).
4. `Session::fetch_rows` checks the page size (1–1000), reads table metadata from the catalogue with bound parameters, and resolves the order: the requested sort column must exist in the metadata, then primary-key columns are appended.
5. The engine module builds one `SELECT` in which every column is rendered to text by the server, with identifiers quoted by `ident` and `LIMIT`/`OFFSET` bound as parameters. It asks for `limit + 1` rows and one unit beyond each preview length.
6. `finish_page` trims to the limit, sets `hasMore`, cuts long cells on a character boundary, records them in `truncated`, and prefixes binary previews with `0x`.
7. The grid renders the page with row virtualization; a response superseded by a newer request is dropped.

## Data flow: save edits

1. Edits are staged in the tab view as a `Pending` value (`src/features/table/pending.ts`): cell edits keyed by row and column index, new rows, and rows marked for deletion, with a bounded undo history (the last 200 states). Typing, paste, clear, add row and delete only change `Pending`; nothing reaches the server. A cell set back to its original value is dropped from `Pending`.
2. Row indexes are stable because the page cannot change while edits are pending: paging, sorting, page size and refresh are locked, and a reload discards `Pending`.
3. On Save, `toChangeSet` translates `Pending` into a `ChangeSet`: each update and delete carries the row's complete primary key as loaded, each changed cell carries the original text and the new value, and each insert carries only the columns the user filled in (the rest take server defaults).
4. The tab decides whether the review step is required: a Production-labelled connection, any delete, or more touched rows than the connection's `bulkReviewRows` (default 10). The "Review" button always opens it. The review dialog calls `plan_changes` and shows each statement with its bound values (the first 60 in full, the rest counted). Small saves call `apply_changes` directly.
5. `apply_changes` refuses a profile saved as read-only, then `Session::apply_changes` reads the table metadata again and calls `write::plan` — the same function `plan_changes` uses — so the reviewed statements and the executed statements come from one code path. The plan is rebuilt at apply time from the same change set; it is not cached between the two calls.
6. `write::plan` validates the change set (non-empty, at most 10,000 row changes, known and editable columns, no column listed twice, complete non-NULL key in key order, updates and deletes refused on keyless tables) and emits statements in the order deletes, updates, inserts.
7. The engine module runs the statements in one transaction with values bound as text parameters. A driver error, or a statement that affects anything other than exactly one row, rolls the transaction back and returns a `database` or `conflict` error naming the row.
8. On success the tab reloads the page, which shows what the server stored and clears the staged edits. On failure the staged edits are kept and the message is shown.

The rules and their limits are in [SECURITY.md](SECURITY.md) and [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md).

## Sessions and pools

| Engine | Pooling |
|---|---|
| MySQL | One pool per connection, max 4 connections |
| PostgreSQL | One pool per database, created on first use, max 3 connections each (a PostgreSQL connection is bound to one database) |

Both: idle timeout 600 s; acquire timeout equals the profile's connect timeout; a direct probe connection is made before the pool so the user sees the driver's real error; the read-only statement runs on every new pooled connection.

Session settings: every MySQL connection adds `STRICT_ALL_TABLES` to its session `sql_mode` and otherwise keeps the server's defaults (the driver's own defaults of forcing the time zone to UTC and enabling `PIPES_AS_CONCAT` are turned off). PostgreSQL connections run with `TimeZone=UTC`, which the driver sends at start-up and PuriCat does not yet override (see [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md)). Sessions live in `AppState` keyed by connection id. There is no reconnect or cancellation logic yet (Phase 2b).

## Frontend state

| Store (`src/stores`) | Holds | Persisted |
|---|---|---|
| `connections` | Profiles, session state per connection, lazily loaded explorer levels, expanded nodes | Profiles only, in the backend store |
| `workspace` | Tabs and the active tab; `dirty` (tabs with unsaved edits) and `closing` (a dirty tab waiting for the user's confirmation) | Tabs and active tab only, in setting `workspace` |
| `settings` | Theme, language, panel visibility and widths | Setting `ui` |
| `ui` | Dialog and palette open state | No |

Table data and staged edits (`Pending` plus its undo history) are local component state in the tab view and are never persisted; the tab reports only a dirty flag to the `workspace` store. That flag drives the unsaved dot on the tab, the confirmation before closing a dirty tab, bulk closes that keep dirty tabs open, and the refusal to disconnect, delete or re-save a connection that has dirty tabs. `src/lib/types.ts` mirrors the serde types in `crates/core` by hand.

## Where future pieces attach

| Piece | Attachment point | Phase |
|---|---|---|
| SQL Server adapter | New `Session` variant and module; the compiler lists every `match` to extend. `ident::sqlserver` already exists | 2b |
| SSH tunnels | In `puricat-core`, before `Session::connect`: open a local forward, connect the driver to it, tie its lifetime to the session | 2b |
| Rest of editing (Smart Paste, batching, full-value editor) | `db::write` for new statement shapes (upsert, batches); the tab view's `Pending` model for staging | 3 |
| SQL editor | New tab kind in `workspace`; new execution path in `db` ([ADR 0004](docs/adr/0004-server-side-text-rendering.md) explains why the browse path cannot be reused as is) | 4 |
| AI agent | Subprocess runner and a PuriCat-owned MCP server built on `puricat-core`; approval queue in the UI | 5 |
| Backups | `puricat-backup` helper binary on `puricat-core`, run by the OS scheduler | 6 |
