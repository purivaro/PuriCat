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
|                                       |  db (adapters, write, smart)|  |
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
| `src/` (React) | Rendering, interaction, view state, staged edits, Smart Paste set-up (mapping, mode, key) | Hold stored passwords, build SQL, touch files or processes |
| `src-tauri` (`puricat`) | IPC commands, open sessions, cancel flags of running jobs, choice of secret store, clipboard plugin registration, debug-only test hooks | Contain database or storage logic |
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
| `db::smart` | Smart Paste ([ADR 0014](docs/adr/0014-smart-paste-match-then-write.md)). `Draft` holds the pure logic: validation of the set-up against the table metadata and its unique keys, duplicate detection, the lookup queries to run, classification of every pasted row and the resulting change set. It performs no I/O; `Session` supplies the catalogue data, runs the lookups it asks for and feeds the results back |
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
| `unique_keys` | `id`, `table` | The keys a pasted row can be matched on: the primary key first, then plain unique indexes, each with its columns in key order |
| `analyze_paste` | `id`, `table`, `request` (`mode`, `columns`, `key`, `update`, `rows`) | Smart Paste preview: one report per pasted row (`status`, `message`, `changed` columns), counts (`insert`, `update`, `unchanged`, `unmatched`, `problems`), a `fingerprint` of every row's status and changed columns, set-up problems, elapsed time. Reads only |
| `apply_paste` | `id`, `table`, `request`, `expected` (the preview's fingerprint), `job` (an id chosen by the UI), `onProgress` (channel) | Counts of updated / inserted rows and elapsed time. Analyses again, refuses with `conflict` unless the fresh analysis produces the fingerprint in `expected`, then runs one transaction. Sends the number of statements done over the channel every 20 statements. Refused up front when the saved profile is read-only |
| `cancel_job` | `job` | — (sets the job's cancel flag; the run rolls back before its next statement. Unknown ids are ignored) |
| `get_settings` | — | All key/value settings |
| `set_setting` | `key`, `value` | — |
| `detect_ai_clis` | — | Install and sign-in status per provider |
| `e2e_report` | `text` | Debug builds with `PURICAT_E2E_REPORT` set: writes the report file. Otherwise returns an `unsupported` error |

No command returns a stored password. A saved session is closed when the password or any connection-affecting field changes; cosmetic fields (name, environment, favorite, tags, review threshold) leave it open.

Besides these commands the web view may call two commands of the official `tauri-plugin-clipboard-manager`, `read_text` and `write_text`, for copy and paste in the grid and for reading the block in Smart Paste (`src/lib/clipboard.ts`). The browser clipboard API is not used, so no permission prompt appears.

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

## Data flow: Smart Paste

1. "Smart Paste…" is shown on an editable table tab and is disabled while the tab has unsaved edits. The dialog reads the system clipboard through the clipboard plugin and parses it with the same TSV parser as direct paste. It also calls `unique_keys`.
2. Set-up happens in the web view (`src/features/table/smartPaste.ts`, pure functions): header-row detection (the first row is a header when at least half of its non-empty cells name table columns, case-insensitively, and there is a second row), automatic mapping (by header name, otherwise by position; a table column receives at most one pasted column), the mode, the key (only keys whose columns are all mapped are offered; UPDATE and UPSERT always use one, the first usable unless the user picked another) and the columns to update. `buildRequest` drops the header row, unmapped columns and completely empty rows, and types empty cells: NULL for a key column and for every non-text column (also a NOT NULL one), an empty string for a text column; the literal text `NULL` becomes NULL only when the option is on.
3. Preview calls `analyze_paste`. `Session::prepare_paste` reads the table metadata and unique keys and builds a `smart::Draft`, which refuses a set-up that cannot work (unknown or doubly mapped columns, a key that is not a unique key of the table or is not fully mapped, a key column in the update list, a non-writable column that is not part of the key) and finds missing keys, malformed key values (a key value for an integer column must be a whole number; other key columns get the value pre-checks) and exact-text duplicates without a query. Such rows are never sent to the database.
4. For the remaining rows `Draft::lookups` produces one parameterised `SELECT` per 40 pasted rows. Each returns, for every existing row matched by the chunk, one boolean flag per pasted row followed by the key and update columns rendered as text. The session runs them; `Draft::matched` records the hits and remembers each matched existing row by its key as the database rendered it, so a second pasted row reaching the same existing row — in the same lookup or another — makes both duplicates. Because the comparison happens in the database, its own type and collation rules decide which rows are the same.
5. `Draft::finish` classifies every pasted row (insert, update with the changed columns, unchanged, unmatched, missing key, duplicate, exists, invalid), applies the pre-checks to the values that would be written, and builds a `ChangeSet` and a fingerprint of every row's status and changed columns. The preview shows the counts and the first 200 rows; nothing has been written. The UI ties the preview to the exact request, so any change to the set-up makes it stale and disables Run.
6. Run calls `apply_paste` with `expected` set to the preview's fingerprint. The command refuses a read-only profile, registers a cancel flag under the job id in `AppState.jobs`, and `Session::apply_paste` runs `prepare_paste` again. Unless the fresh analysis produces the same fingerprint it returns a `conflict` error and nothing runs; equal counts are not enough.
7. Otherwise the change set goes through `write::plan_keyed` — the planner behind `write::plan`, addressing rows by the chosen key as the database rendered it instead of the primary key. UPDATEs carry the optimistic check on the values seen by this fresh analysis.
8. The engine module executes the statements in one transaction, each required to affect exactly one row. Before every statement it calls back into the command, which reports progress over the channel every 20 statements and reads the cancel flag; `cancel_job` sets that flag. A cancel, a driver error or a wrong row count rolls everything back. The lookups of step 6 run on pooled connections before this transaction starts, not inside it. While the run is in flight the dialog cannot be dismissed (Esc, outside click and the close button are blocked); "Stop and roll back" is the way out.
9. On success the dialog closes and the tab reloads the page. On failure the message is shown and a new preview is required.

## Sessions and pools

| Engine | Pooling |
|---|---|
| MySQL | One pool per connection, max 4 connections |
| PostgreSQL | One pool per database, created on first use, max 3 connections each (a PostgreSQL connection is bound to one database) |

Both: idle timeout 600 s; acquire timeout equals the profile's connect timeout; a direct probe connection is made before the pool so the user sees the driver's real error; the read-only statement runs on every new pooled connection.

Session settings: every MySQL connection adds `STRICT_ALL_TABLES` to its session `sql_mode` and otherwise keeps the server's defaults (the driver's own defaults of forcing the time zone to UTC and enabling `PIPES_AS_CONCAT` are turned off). PostgreSQL connections run with `TimeZone=UTC`, which the driver sends at start-up and PuriCat does not yet override (see [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md)). Sessions live in `AppState` keyed by connection id. There is no reconnect logic yet, and no cancellation of a running query (Phase 2b); the only cancellable operation is a Smart Paste run, between statements.

## Frontend state

| Store (`src/stores`) | Holds | Persisted |
|---|---|---|
| `connections` | Profiles, session state per connection, lazily loaded explorer levels, expanded nodes | Profiles only, in the backend store |
| `workspace` | Tabs and the active tab; `dirty` (tabs with unsaved edits) and `closing` (a dirty tab waiting for the user's confirmation) | Tabs and active tab only, in setting `workspace` |
| `settings` | Theme, language, panel visibility and widths | Setting `ui` |
| `ui` | Dialog and palette open state | No |

The Smart Paste dialog keeps its own state (pasted block, mapping, preview) and is created afresh on every opening. Table data and staged edits (`Pending` plus its undo history) are local component state in the tab view and are never persisted; the tab reports only a dirty flag to the `workspace` store. That flag drives the unsaved dot on the tab, the confirmation before closing a dirty tab, bulk closes that keep dirty tabs open, and the refusal to disconnect, delete or re-save a connection that has dirty tabs. `src/lib/types.ts` mirrors the serde types in `crates/core` by hand.

## Where future pieces attach

| Piece | Attachment point | Phase |
|---|---|---|
| SQL Server adapter | New `Session` variant and module; the compiler lists every `match` to extend. `ident::sqlserver` already exists | 2b |
| SSH tunnels | In `puricat-core`, before `Session::connect`: open a local forward, connect the driver to it, tie its lifetime to the session | 2b |
| Rest of editing (full-value editor, filtering, Smart Paste follow-ups) | The tab view's `Pending` model for staging; `db::smart` for "skip invalid rows" and value-aware comparison | 3 |
| Imports beyond 10,000 rows, CSV / XLSX files | A batching executor next to `db::write`; `db::smart::Draft` can be reused for matching | 4 |
| SQL editor | New tab kind in `workspace`; new execution path in `db` ([ADR 0004](docs/adr/0004-server-side-text-rendering.md) explains why the browse path cannot be reused as is) | 4 |
| AI agent | Subprocess runner and a PuriCat-owned MCP server built on `puricat-core`; approval queue in the UI | 5 |
| Backups | `puricat-backup` helper binary on `puricat-core`, run by the OS scheduler | 6 |
