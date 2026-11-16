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
|   plain-text clipboard, Save dialog   | crates/core (puricat-core)  |  |
|                                       |  model    store    secrets  |  |
|                                       |  db (adapters, write, smart,|  |
|                                       |     console, script, export)|  |
|                                       |  ai_detect    agent         |  |
|                                       +--+-------+-------+-------+--+  |
+------------------------------------------|-------|-------|-------|-----+
                                           |       |       |       |
                                  puricat.db   OS credential   MySQL /      claude / codex
                                  (SQLite,     store           PostgreSQL   (status commands;
                                  app data)    (Keychain)      servers      one process per
                                                                            chat turn)
```

The backend also writes export files (CSV, JSON, XLSX) to paths the user picks in the native Save dialog. The `claude` and `codex` processes are the only part that talks to an AI provider; they are given text on stdin and no database access ([ADR 0016](docs/adr/0016-agent-chat-without-tools.md)).

| Component | Responsibility | Must not |
|---|---|---|
| `src/` (React) | Rendering, interaction, view state, staged edits, Smart Paste set-up (mapping, mode, key), the SQL editor and the confirmation before a risky script, the agent chat (the conversation, which context to ask for, rendering answers) | Hold stored passwords, build SQL (text typed in a query tab is passed to the backend unchanged; it is split, classified and run there), touch files or processes, run SQL from an AI answer (it can only be placed in a query tab's editor) |
| `src-tauri` (`puricat`) | IPC commands, open sessions, one console per query tab, cancel flags of running jobs (Smart Paste runs and agent turns), the location of the AI CLIs' scratch directory, choice of secret store, clipboard and dialog plugin registration, debug-only test hooks | Contain database or storage logic |
| `crates/core` (`puricat-core`) | Profiles, settings store and SQL history, secrets, adapters, the SQL console, export writers, CLI discovery, the agent chat runner (prompt, context text, CLI process, stream parsing) | Depend on Tauri |

`puricat-core` has no Tauri dependency so it can be unit-tested without a window and reused by the planned backup helper and MCP bridge binaries ([ADR 0002](docs/adr/0002-core-crate-and-thin-ipc.md)).

## Core modules

| Module | Contents |
|---|---|
| `model` | `ConnectionProfile`, `Engine` (`mysql`, `postgres`, `sqlserver` — the last has no adapter), `Environment`, `TlsMode`; input normalisation |
| `store` | SQLite settings database: `connections` (one JSON profile per row plus timestamps), `settings` (key/value) and, since migration 2, `query_history` (one row per run of a query tab; at most 1,000 rows are kept). Append-only migrations keyed on `PRAGMA user_version`; a database from a newer build is refused |
| `secrets` | `SecretStore` trait; `OsKeychain` (via `keyring`) and `MemoryStore` (tests, debug runs). Service `PuriCat`, key `connection/<id>/password` |
| `db` | `Session` enum (`MySql`, `Postgres`), `Capabilities`, shared paging/truncation logic, `ident` quoting, one module of SQL per engine |
| `db::write` | Turns a change set (updates, inserts, deletes) into parameterised statements and enforces the write rules; the engine modules execute the plan in one transaction ([ADR 0013](docs/adr/0013-staged-edits-and-optimistic-saves.md)) |
| `db::smart` | Smart Paste ([ADR 0014](docs/adr/0014-smart-paste-match-then-write.md)). `Draft` holds the pure logic: validation of the set-up against the table metadata and its unique keys, duplicate detection, the lookup queries to run, classification of every pasted row and the resulting change set. It performs no I/O; `Session` supplies the catalogue data, runs the lookups it asks for and feeds the results back |
| `db::script` | Reads SQL text without parsing it: a dialect-aware scanner (strings, comments, MySQL backticks, PostgreSQL dollar quotes) that splits a script into statements, classifies each as `read` / `dml` / `ddl` / `transaction` / `other` with an optional "danger" reason, says whether a transaction-control statement leaves a transaction open, says whether a statement mentions a keyword outside strings and comments (`mentions`, used for `TEMPORARY`), and masks password literals in single or double quotes for the history. Pure functions, no I/O |
| `db::console` | The SQL console ([ADR 0015](docs/adr/0015-sql-console.md)). `Console` owns one connection detached from the session's pool and runs a script statement by statement over the simple/text protocol: impact guard, row cap, stop flag, tracking of the user's transaction and of MySQL autocommit, detection of a lost connection. It keeps the full rows of the last run for export. `Canceller` interrupts the console's statement from another pooled connection |
| `db::export` | Writes one stored result to a file as CSV, JSON or XLSX (`rust_xlsxwriter`). In JSON, columns that share a name get the keys `name`, `name_2`, `name_3`, … |
| `ai_detect` | Locates `claude` / `codex` and reads their version and sign-in state. `locate`, `path_with` and `Provider::binary` are shared with `agent` |
| `agent` | The agent chat ([ADR 0016](docs/adr/0016-agent-chat-without-tools.md)). Pure parts: the fixed instructions, `validate_model`, `render_context` (schema text from catalogue metadata), `render_prompt` (instructions for Codex, context block, trimmed conversation), `arguments` (the fixed vector per provider) and `StreamParser` (the CLIs' JSON lines to `Started` / `Delta` / `Message` events and an outcome). I/O: `models` (runs `codex debug models`), `chat` / `chat_with` (one CLI process per turn, in a process group of its own on Unix: prompt on stdin, events from stdout until the first verdict, stop flag, 10-minute timeout, the whole group killed on Stop or timeout). It holds no database session and no state between turns |
| `error` | `Error` serialised to the UI as `{ kind, message }` |

## Local data

| Data | Location | Notes |
|---|---|---|
| Profiles, UI settings, open tabs | `puricat.db` in the OS app-data directory for identifier `app.puricat.desktop` (macOS: `$HOME/Library/Application Support/app.puricat.desktop/`) | File mode 0600 on Unix. The identifier is a placeholder; changing it later orphans this data |
| Text of open query tabs | `puricat.db`, table `settings`, key `sql:<tab id>` | Written 600 ms after the last keystroke (a write still pending when the tab closes is dropped); written at once for a tab opened with SQL from the agent chat; set to an empty value when the tab closes |
| SQL history | `puricat.db`, table `query_history` | The script text of each run with password literals masked, the connection id, database, status, a summary and the duration. The newest 1,000 rows over all connections are kept; a connection's rows are deleted with the connection. Clear text otherwise ([SECURITY.md](SECURITY.md)) |
| Query results | Backend memory, per query tab | The full rows of the tab's last run, replaced by the next run and dropped with the tab's console. Never written to disk except by an export |
| Exported files | Wherever the user saves them | Written by the backend; PuriCat keeps no record of them |
| Passwords | OS credential store | Never written to `puricat.db` |
| Agent chat preferences | `puricat.db`, table `settings`, key `agent` | Provider, model per provider, the "Share context" switch |
| Agent conversation | Web view memory (`stores/agent.ts`) | The turns, and with each sent message the exact context that went with it. Gone with "New chat" or when the app closes; never written to disk by PuriCat |
| AI CLI scratch directory | `agent-scratch` in the app-data directory | The working directory of every chat turn. Created by the first turn; PuriCat writes nothing into it and tests assert that it stays empty |
| Theme hint | Web view `localStorage` key `puricat.theme` | Avoids a flash of the wrong theme before settings load |

## IPC surface

All commands are registered in `src-tauri/src/lib.rs` and implemented in `commands.rs`. Errors arrive as `{ kind, message }`.

| Command | Arguments | Returns |
|---|---|---|
| `list_connections` | — | Profiles, each with `hasPassword` |
| `save_connection` | `profile`, `password` (`keep` / `set` / `clear`) | Saved profile, `sessionClosed` |
| `duplicate_connection` | `id`, `name` | New profile (password copied inside the credential store) |
| `delete_connection` | `id` | — (closes the session, deletes the password and the connection's query history) |
| `test_connection` | `profile`, `password` | `ServerInfo`; nothing is saved |
| `open_connection` | `id` | `ServerInfo`; session kept in `AppState` |
| `close_connection` | `id` | — |
| `list_databases` | `id` | Namespaces |
| `list_schemas` | `id`, `database` | Namespaces (PostgreSQL only) |
| `list_objects` | `id`, `database`, `schema?` | Tables, views, materialized views |
| `describe_table` | `id`, `table` | Columns and primary key of one table (used by the SQL editor's completion) |
| `fetch_rows` | `id`, `table`, `page` (`offset`, `limit`, `sort?`) | `RowPage`: columns (with `editable`), rows, truncation flags, primary key, estimated rows |
| `plan_changes` | `id`, `table`, `changes` (`updates`, `inserts`, `deletes`) | The statements a save would run, in order: `kind`, `sql`, `params`, row label. Nothing is executed |
| `apply_changes` | `id`, `table`, `changes` | Counts of updated / inserted / deleted rows and elapsed time. One transaction; refused up front when the saved profile is read-only |
| `unique_keys` | `id`, `table` | The keys a pasted row can be matched on: the primary key first, then plain unique indexes, each with its columns in key order |
| `analyze_paste` | `id`, `table`, `request` (`mode`, `columns`, `key`, `update`, `rows`) | Smart Paste preview: one report per pasted row (`status`, `message`, `changed` columns), counts (`insert`, `update`, `unchanged`, `unmatched`, `problems`), a `fingerprint` of every row's status and changed columns, set-up problems, elapsed time. Reads only |
| `apply_paste` | `id`, `table`, `request`, `expected` (the preview's fingerprint), `job` (an id chosen by the UI), `onProgress` (channel) | Counts of updated / inserted rows and elapsed time. Analyses again, refuses with `conflict` unless the fresh analysis produces the fingerprint in `expected`, then runs one transaction. Sends the number of statements done over the channel every 20 statements. Refused up front when the saved profile is read-only |
| `cancel_job` | `job` | — (sets the job's cancel flag. A Smart Paste run rolls back before its next statement; an agent turn's CLI process is killed. Unknown ids are ignored) |
| `analyze_sql` | `id`, `sql` | The script split into statements, each with `text`, `start` (byte offset), `kind` and `danger` (a reason, or null). Nothing is sent to the server |
| `run_sql` | `id`, `tab`, `database?`, `sql`, `options` (`maxRows` 1–100,000, `changeLimit?`) | `RunOutcome`: one entry per statement that ran (`sql`, `kind`, `status` `ok` / `error` / `blocked` / `cancelled`, `results`, `rowsAffected`, `message`, `elapsedMs`), `skipped`, `resume` (the blocked statement and the rest of the script), `inTransaction`, `connectionLost`, `elapsedMs`. Opens the tab's console on first use, or again when the connection or database differs. Each result carries `id`, `columns`, `rows` with long cells cut, `truncated` and `capped`. Records the run in the history |
| `cancel_sql` | `tab` | — (sets the tab's stop flag and interrupts its running statement from another pooled connection. Unknown tabs are ignored) |
| `close_console` | `tab` | — (drops the tab's console and connection; a running statement is interrupted first, an open transaction is rolled back by the server) |
| `list_history` | `id`, `limit` (capped at 500) | History entries of one connection, newest first |
| `clear_history` | `id` | — (deletes that connection's history entries) |
| `export_result` | `tab`, `result` (a result `id` of the tab's last run), `format` (`csv` / `json` / `xlsx`), `path` | Number of data rows written. The backend writes the file |
| `get_settings` | — | All key/value settings |
| `set_setting` | `key`, `value` | — |
| `detect_ai_clis` | — | Install and sign-in status per provider |
| `agent_models` | `provider` (`claude` / `codex`) | The models offered in the chat's selector: `id`, `label`, `description`. A fixed list of aliases for Claude Code; the output of `codex debug models` for Codex (empty when it cannot be read) |
| `agent_context` | `id`, `database`, `schema?`, `sql?` | The context text for a chat: engine and version, database, schema, the first 40 tables and views with their columns, the rest by name, and `sql` if given. Reads the catalogue of the open session; sends nothing anywhere |
| `agent_send` | `job` (an id chosen by the UI), `request` (`provider`, `model?`, `context?`, `messages`), `onEvent` (channel) | The complete answer. Starts the CLI for one turn, streams `started` / `delta` / `message` events over the channel, and fails with the CLI's own message (`unsupported`), with `conflict` "Stopped" after `cancel_job`, with `invalid` for a bad model name or an empty message, or with `not_found` when the CLI is not installed. Uses no database session |
| `e2e_report` | `text` | Debug builds with `PURICAT_E2E_REPORT` set: writes the report file. Otherwise returns an `unsupported` error |

That is 32 commands. No command returns a stored password. A saved session is closed when the password or any connection-affecting field changes; cosmetic fields (name, environment, favorite, tags, review threshold) leave it open. Closing a session also drops the consoles of its query tabs, interrupting whatever they are running.

Besides these commands the web view may call two commands of the official `tauri-plugin-clipboard-manager`, `read_text` and `write_text`, for copy and paste in the grid, for reading the block in Smart Paste and for "Copy" on a code block of an AI answer (`src/lib/clipboard.ts`), and one command of the official `tauri-plugin-dialog`, `save`, which shows the native Save dialog and returns the chosen path for an export. The browser clipboard API is not used, so no permission prompt appears.

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

## Data flow: run a script in a query tab

Decisions and limits: [ADR 0015](docs/adr/0015-sql-console.md).

1. "New query" (explorer context menu on a connection or a database, the command palette, ⌘T) calls `useWorkspace.openSql`, which adds a tab of kind `sql` with an id `sql:<uuid>`, the connection id, a database (or none, for the connection's default) and a title "Query n". The tab view (`src/features/sql/SqlTab.tsx`) and Monaco are separate chunks that load now; the saved text of the tab is read from the settings store. A tab opened from the agent chat ("Open in query tab") starts instead with the SQL it was opened with, and runs nothing.
2. ⌘↵ or Run takes the selection, or the whole text when nothing is selected, and calls `analyze_sql`. `script::split` scans the text with the engine's lexical rules and returns the statements with their kind and danger reason. If any statement has a danger reason — or, on a Production-labelled connection, is not a `read` — the tab lists those statements and waits for the user's confirmation.
3. `run_sql` receives the text, the tab id, the tab's database and the options: `maxRows` from the tab's "Rows per result" selector and `changeLimit` from the connection's `bulkReviewRows`. The command finds the tab's console in `AppState.consoles` or opens one: `Session::open_console` acquires a connection from the session's pool (so strict mode and the read-only statement have run on it), issues `USE` for the chosen database on MySQL or takes the pool of the tab's database on PostgreSQL, detaches the connection from the pool, and notes the server's connection id or backend pid for cancelling.
4. `Console::run` splits the script again and runs the statements in order. For each one it decides: *guarded* (a `dml` statement while no transaction of the user's is open and, on MySQL, autocommit is on) and *cancel at the cap* (a `read`, and not inside a user's transaction on PostgreSQL).
5. A guarded statement is wrapped in `START TRANSACTION` / `BEGIN` … `COMMIT`. The statement goes out with `sqlx::raw_sql(...).fetch_many`; rows arrive as text and are decoded per engine (binary as `0x…`, MySQL `BIT` as a number). Rows beyond `maxRows` are not kept; at that point a cancellable statement is interrupted through the `Canceller`, any other is read to its end.
6. After the statement: an error, a stop request or — for a guarded statement — more affected rows than `changeLimit` turns the `COMMIT` into a `ROLLBACK`, and the statement is reported as `error`, `cancelled` or `blocked`. A `blocked` statement sets `resume` to the text from that statement to the end of the script. Any status other than `ok` ends the run; the statements not reached are counted in `skipped`.
7. The console then updates what it knows about the session. After a successful statement: `transaction_effect` for a transaction-control statement, and an implicit commit after MySQL DDL, except `CREATE` / `DROP TEMPORARY …`, which do not commit (recognised by `TEMPORARY` being the statement's second word). After any `other` statement on MySQL the remembered `@@autocommit` is forgotten and asked again when next needed. A successful `read` that returned no rows is described to get its column names. Each result set is stored in full in the console and returned as a preview with cells cut to 1,000 characters.
8. The command records the run in `query_history` (text masked by `script::redact`) and, if the connection was lost, removes the console so the next run opens a new one.
9. The tab shows one result tab per result set in the shared grid (read-only, not sortable) and a Messages list. `inTransaction` sets the tab's dirty flag, which drives the same guards as staged edits. For a `blocked` statement, "Run it without the limit" calls `run_sql` with `resume` and no `changeLimit`.
10. Stop calls `cancel_sql`: the stop flag is set and the `Canceller` sends `KILL QUERY` or `pg_cancel_backend` on a pooled connection; if the command fails, the tab shows the error. Export, which is disabled while a run is in flight, asks the Save dialog for a path and calls `export_result`, which writes the stored result with `db::export`. Closing the tab calls `close_console`, clears the saved text and drops a save that was still pending.

## Data flow: one agent chat turn

Decisions and limits: [ADR 0016](docs/adr/0016-agent-chat-without-tools.md) and [AI_INTEGRATION.md](AI_INTEGRATION.md). The model gets no tools and no database access; this flow only moves text.

1. The AI panel (`components/layout/AiPanel.tsx`) calls `detect_ai_clis` when it appears and loads the chat preferences from setting `agent`. It shows the chat (`features/agent/AgentChat.tsx`) for the selected provider only when that CLI is installed and not known to be signed out. The model selector calls `agent_models`.
2. The chat derives its scope from the tab in front: the connection, database and schema of a table tab, or the connection and database of a query tab together with that tab's id. With no table or query tab, a closed connection or no database there is no scope.
3. Send (Enter or the button) calls `useAgent.send`. The store appends the user's turn and marks a turn as starting, so a second Enter cannot start another; Stop or "New chat" at this point gives the claim up and nothing is sent. If "Share context" is on and there is a scope, it calls `agent_context` with the scope and, for a query tab, the editor's current text — read through `sqlEditors`, a map from tab id to a getter that every mounted query tab registers.
4. `agent_context` takes the open `Session`, reads `server_info`, `list_objects`, and `describe_table` for the first 40 objects one after another, and returns `agent::render_context(...)`: names and types only. The store records that text on the user's turn; it is what "Context sent" shows. If the call fails, the message is sent without context.
5. The store appends an empty assistant turn and calls `agent_send` with a new job id and the request: the provider, the model chosen for that provider (or none), the context text, and the transcript — every earlier turn except answers that ended in an error.
6. The command registers a cancel flag under the job id in `AppState.jobs` and calls `agent::chat`, which locates the binary (`ai_detect::locate`), validates the model name, builds the prompt (`render_prompt`: for Codex the fixed instructions, then the context block, then the most recent turns up to 60,000 characters) and creates the scratch directory if it does not exist.
7. The CLI is spawned with the fixed vector from `arguments(...)`, the scratch directory as its working directory, all three standard streams piped and, on Unix, a process group of its own. Two tasks run beside the turn: one writes the prompt to stdin and closes it, one keeps the tail of stderr. The turn itself reads stdout line by line, each line going through `StreamParser::line`; before every read, and at least every 150 ms, it looks at the stop flag and at the 10-minute deadline.
8. The parser's events go over the channel as they come: `started` (with the model the CLI names), `delta` (text to append) and `message` (the whole answer so far, replacing what was streamed). The store patches the assistant turn, and the chat renders its text through `parseMarkdown` into React elements on every change.
9. The turn ends in one of four ways. With the parser's verdict, as soon as a line carries one: success returns the complete answer, failure returns the CLI's own message. With the CLI closing its output without a verdict: exit status 0 with some text is a success, anything else fails with the last line of stderr. In both cases the CLI gets 5 seconds to exit by itself before it is killed. On Stop: `cancel_job` sets the flag, the reader sees it, the CLI's process group is killed and the command returns a `conflict` error "Stopped". On timeout: the same kill, and the error names the limit. The command then removes the job's flag.
10. The store marks the assistant turn `done`, `stopped` (keeping the text that had arrived) or `error` (keeping the message). "Try again" drops that turn and repeats steps 5–10 with the context recorded in step 4.
11. "Open in query tab" on a SQL block calls `useWorkspace.openSql(connectionId, database, text)` with the scope recorded on the question. The store keeps the text as the new tab's seed and saves it under the tab's settings key; the tab view starts its editor with it. Nothing is sent to the database. Running it is the flow of the previous section, with its confirmation, impact guard and row cap.

## Sessions and pools

| Engine | Pooling |
|---|---|
| MySQL | One pool per connection, max 4 connections |
| PostgreSQL | One pool per database, created on first use, max 3 connections each (a PostgreSQL connection is bound to one database) |

Both: idle timeout 600 s; acquire timeout equals the profile's connect timeout; a direct probe connection is made before the pool so the user sees the driver's real error; the read-only statement runs on every new pooled connection.

Query tabs do not use these pools for their statements. Each tab that has run something holds one connection of its own, acquired through the pool (so the same set-up has run on it) and then detached, which frees the pool slot; six open consoles next to a working session are covered by a live test. There is no limit on the number of consoles other than the server's connection limit. The pool is still used to send a Stop or a row-cap interrupt.

Session settings: every MySQL connection adds `STRICT_ALL_TABLES` to its session `sql_mode` and otherwise keeps the server's defaults (the driver's own defaults of forcing the time zone to UTC and enabling `PIPES_AS_CONCAT` are turned off). PostgreSQL connections run with `TimeZone=UTC`, which the driver sends at start-up and PuriCat does not yet override (see [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md)). Sessions live in `AppState` keyed by connection id; consoles live there keyed by tab id. Pooled sessions have no reconnect logic yet, and browse, catalogue and lookup queries cannot be cancelled (Phase 2b). What can be stopped: a Smart Paste run, between statements, and the statement a query tab is running, on the server. A query tab whose connection was lost opens a new one on its next run.

## Frontend state

| Store (`src/stores`) | Holds | Persisted |
|---|---|---|
| `connections` | Profiles, session state per connection, lazily loaded explorer levels, expanded nodes | Profiles only, in the backend store |
| `workspace` | Tabs (welcome, table, query) and the active tab; `dirty` (table tabs with unsaved edits, query tabs with an open transaction) and `closing` (a dirty tab waiting for the user's confirmation). Beside the store, two module-level maps: the text a query tab was opened with (`sqlSeed`, until the tab closes) and a getter for each mounted query tab's editor text (`sqlEditors`, read for the agent's context) | Tabs and active tab only, in setting `workspace` |
| `settings` | Theme, language, panel visibility and widths | Setting `ui` |
| `ui` | Dialog and palette open state | No |
| `agent` | Chat preferences (provider, model per provider, "Share context"); the conversation's turns, each user turn with the context sent and the tab scope it was asked about; the job id of the running turn | Preferences only, in setting `agent` |

The Smart Paste dialog keeps its own state (pasted block, mapping, preview) and is created afresh on every opening. Table data and staged edits (`Pending` plus its undo history) are local component state in the tab view and are never persisted; the tab reports only a dirty flag to the `workspace` store. That flag drives the unsaved dot on the tab, the confirmation before closing a dirty tab, bulk closes that keep dirty tabs open, and the refusal to disconnect, delete or re-save a connection that has dirty tabs. A query tab sets the same flag while the last run reported an open transaction, so all of these apply to it too (the dialog then offers "Roll back and close"). The query tab's results, the chosen row cap and the editor height are local component state and are not persisted; its text is (see "Local data"). `src/lib/types.ts` mirrors the serde types in `crates/core` by hand.

## Where future pieces attach

| Piece | Attachment point | Phase |
|---|---|---|
| SQL Server adapter | New `Session` variant and module; the compiler lists every `match` to extend. `ident::sqlserver` already exists | 2b |
| SSH tunnels | In `puricat-core`, before `Session::connect`: open a local forward, connect the driver to it, tie its lifetime to the session | 2b |
| Rest of editing (full-value editor, filtering, Smart Paste follow-ups) | The tab view's `Pending` model for staging; `db::smart` for "skip invalid rows" and value-aware comparison | 3 |
| Imports beyond 10,000 rows, CSV / XLSX files | A batching executor next to `db::write`; `db::smart::Draft` can be reused for matching | 4 |
| Rest of the SQL workspace (saved scripts, statement at cursor, EXPLAIN view, query parameters, larger or streamed results) | The query tab (`src/features/sql`) and `db::console`; streaming needs a result store that does not hold every row in memory | 4 |
| Object editing with DDL preview, SQL dump export | New tab kinds or dialogs; DDL generation per engine module, run through the console path | 4 |
| AI agent with tools | The subprocess runner exists (`agent`, today without tools). Still to attach: a PuriCat-owned MCP server built on `puricat-core` and handed to the CLI for the turn, read-only tools on the existing sessions, an approval queue in the UI, an audit log | 5 |
| Backups | `puricat-backup` helper binary on `puricat-core`, run by the OS scheduler | 6 |
