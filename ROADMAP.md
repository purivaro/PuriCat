# Roadmap

Status on 2026-11-17. Phases follow the delivery plan in the [product brief](docs/PuriCat_Master_Requirements.md). No dates are committed. Per-requirement status is in [REQUIREMENTS.md](REQUIREMENTS.md); evidence is in [TESTING.md](TESTING.md).

| Phase | Scope | Status |
|---|---|---|
| 0 | Discovery and decisions | Done |
| 1 | Runnable foundation | Done |
| 2 | Real database connectivity | Partial (remainder tracked as Phase 2b) |
| 3 | Flagship editing | Partial (grid editing, the row filter and the Smart Paste wizard done for MySQL and PostgreSQL; the leftovers are listed below) |
| 4 | SQL and schema | Partial (the SQL console is done for MySQL and PostgreSQL; CSV and Excel files open in Smart Paste, up to 10,000 rows; object editing, batched import and the rest are listed below) |
| 5 | AI agent | Partial (CLI detection, an agent chat, and — off by default — database tools for the model: catalogue lookups, a read-only query, proposals the user approves, an activity log. Verified for Claude Code; no Codex turn with tools has been observed. Redaction, version checks and the gaps listed below remain) |
| 6 | Backups | Not started |
| 7 | Public beta | Not started |

## Phase 0 — Discovery and decisions: done

- Toolchain, driver and CLI interfaces checked on the development machine.
- Twelve decision records written ([docs/adr/](docs/adr/README.md)); five of them are proposals for later phases.

## Phase 1 — Runnable foundation: done

- Tauri 2 + React app with custom title bar, resizable panels, multi-tab workspace with stored tabs.
- Branding, light / dark / system themes, English and Thai.
- Connection manager, local settings store with migrations, OS credential store.
- Command palette, toasts, error boundary.
- Build scripts and a CI workflow file (first run on 2026-11-16, when the repository was published).

## Phase 2 — Real database connectivity: partial

Done:

- MySQL and PostgreSQL adapters with capability flags.
- Credentials in the OS credential store; TLS options; server-enforced read-only sessions.
- Several concurrent connections with pooling limits and idle timeout.
- Explorer with lazy metadata (databases, schemas, tables, views).
- Table browsing with exact values, paging and sorting; structure view.

### Phase 2b — remaining

| Item | Notes |
|---|---|
| SQL Server adapter | Evaluate `tiberius` first ([ADR 0003](docs/adr/0003-database-drivers.md)); needs a disposable test instance (blocker below) |
| SSH tunnels | `russh`, host-key verification, key / passphrase / agent ([ADR 0009](docs/adr/0009-ssh-tunnels.md)); needs a disposable `sshd` |
| TLS verification test | Run `require`, `verify-ca`, `verify-full` against a TLS-enforcing server, including the failure cases; decide a stricter default for Production connections |
| Procedures, functions, indexes in the explorer | Per engine |
| Query cancellation outside query tabs | A statement in a query tab can be stopped (Phase 4, done). Browse, catalogue and Smart Paste lookup queries cannot; the console's canceller (`KILL QUERY`, `pg_cancel_backend`) shows the way |
| Reconnect on dropped connections | A query tab reconnects its own connection on the next run (Phase 4, done). For the pooled session a dropped connection is still an error on the next action |
| Keyset pagination | Replace `OFFSET` where a usable key exists |
| Exact row count on demand | Opt-in, cancellable |
| Per-connection session time zone (PostgreSQL) | The driver always starts sessions with `TimeZone=UTC`; let the user choose the server default or a named zone ([DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md), known limitations) |

Smaller follow-ups found while documenting: a connect-timeout field in the dialog; display of the TLS state of an open session; a live test for wrong passwords (needs a server with password authentication); a tie-breaker or an "unstable order" flag when a keyless table is sorted.

## Phase 3 — Flagship editing: partial

Done (MySQL and PostgreSQL, tables with a primary key; evidence in [TESTING.md](TESTING.md), design in [ADR 0013](docs/adr/0013-staged-edits-and-optimistic-saves.md)):

- Staged edits on the loaded page: change tracking, highlight, undo / redo, Save / Discard; nothing is written before Save.
- Editable custom grid: single-range selection (mouse, Shift+arrows, row gutter, select all), in-cell editor, Tab / Enter navigation, multi-line values, clear, set to NULL, add / delete / restore rows, context menu.
- Clipboard: TSV parser and serialiser with unit tests; copy; direct paste from Excel / Google Sheets / Numbers with fill, append and overflow reporting; system clipboard through the official Tauri clipboard plugin.
- Write path per engine: parameterised statements, complete-primary-key addressing, optimistic concurrency on edited columns, exactly-one-row rule, one transaction, 10,000 row changes per save; strict mode for MySQL sessions; bare-type casts for PostgreSQL.
- Review dialog with the exact statements and bound values; mandatory for Production-labelled connections, deletes and saves above the per-connection row threshold (default 10).
- Guards for unsaved edits: tab dot, close confirmation, locked paging / sorting / refresh, refusal to disconnect or change a connection with dirty tabs.
- Read-only presentation, with the reason, for views, materialized views, keyless tables and read-only connections.
- One cell, whole (2026-11-17): a dialog for a long or multi-line value, opened from the cell's menu or by double-click; a cell shortened for preview is fetched in full by the row's key and can be edited there, the edit staged with its complete original. Verified in the backend on both engines; through the UI on MySQL.
- Smart Paste from a file (2026-11-17): "Open a file…" reads a CSV / TSV file or an Excel / OpenDocument workbook into the wizard; the encoding is never guessed, a sheet and a separator can be chosen, workbook cells are read as stored values; at most 64 MB and 10,000 data rows; the backend shows the Open dialog, so the web view never names a path. Reading is unit-tested; through the UI on MySQL with files PuriCat exported itself.
- Row filter (2026-11-17): a bar of conditions on columns, joined with AND, sent on Enter or "Apply"; twelve operators; "Filter by this value" on a cell; values as bind parameters, typed comparison for numbers, dates and text and the shown text for other types; a value the column cannot hold is an error instead of a silent result. Verified in the backend on both engines; through the UI on PostgreSQL only ([DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md), "Row filter").
- Smart Paste wizard ([ADR 0014](docs/adr/0014-smart-paste-match-then-write.md)): clipboard block, header-row detection, column mapping, INSERT / UPDATE / UPSERT matched on the primary key or a plain unique index read from the catalogue, generated keys used for matching only, duplicate detection, cheap pre-checks, a read-only preview with one outcome per pasted row, re-analysis at run time, one transaction of at most 10,000 rows through the same planner as grid saves, progress and "Stop and roll back". Verified in the backend on both engines; through the UI for UPSERT on MySQL only.

Remaining:

| Item | Notes |
|---|---|
| Smart Paste: "skip invalid rows" | Today any row with a problem blocks the whole run until the source is fixed |
| Smart Paste: value-aware comparison | "Update" means the pasted text differs from the stored text, so `1.5` against a stored `1.500000` is counted and run as an update |
| Smart Paste: verification gaps | PostgreSQL, INSERT and UPDATE modes, composite keys, the progress bar, Stop and the blocked dismissal during a run through the UI (composite-key matching itself is verified in the backend on both engines); pastes near the 10,000-row limit and their duration on a remote server |
| Smart Paste: more than 10,000 rows | One run is one transaction of single-row statements. A longer file is refused today. Multi-batch or streamed imports belong to the Phase 4 import tool |
| Smart Paste from a file: follow-ups and gaps | Drag a file onto the table; a header row that is not the first row; skipping leading lines; number formats (percent, currency) applied on request. Not verified: the native Open dialog, a run from a file, workbooks made by Excel / Numbers / LibreOffice, `.xls` / `.xlsb` / `.ods`, the sheet chooser through the UI, files near 64 MB |
| Whole-value dialog: follow-ups | A formatted view of JSON; binary values beyond the first 65,536 bytes, and saving one to a file; values beyond 1,000,000 characters; a shortened cell in a table without a primary key (it cannot be fetched). Not verified through the UI: PostgreSQL, read-only tables, Thai and the dark theme |
| Filter: follow-ups | OR and groups of conditions, a list of values (`IN`), a free `WHERE` box; the filter kept with the tab across a restart; an exact count of the matching rows; typed comparison (and index use) for `uuid` and other PostgreSQL types that have an equality operator. Verification gaps: MySQL, ⌘F and Escape, a view, the lock while edits are unsaved — all through the UI |
| Column reorder / hide, pinned key columns | Not started |
| Multi-range (non-contiguous) selection | Only one rectangular range today |
| Keyset pagination | Also listed under Phase 2b |
| Editing across pages | Edits are limited to the loaded page; paging is locked while edits are pending |
| Client-side type hints and validation before save | Today the server validates at Save |
| Audit log of saves | Not started |
| Grid validation | Performance with many columns and pages of 1000 wide rows; real keyboard, IME and drag input; the Production and bulk-threshold review paths through the UI ([ADR 0006](docs/adr/0006-data-grid.md), [TESTING.md](TESTING.md) gaps) |

Known limitations of what is built (DELETE checks the key only; PostgreSQL sessions run in UTC; keyless tables stay read-only in the UI; Smart Paste compares text and runs as a single transaction) are in [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md) and [SECURITY.md](SECURITY.md).

## Recommended next step

The model now has tools, off by default ([ADR 0017](docs/adr/0017-ai-database-tools.md)). That was step 3 of the earlier recommendation and was taken first; it is done for Claude Code. Steps 1 and 2 are still open, and the tools added verification gaps of their own. Do not widen the AI work before closing them. This is a recommendation, not a commitment.

1. **Verify what the tools shipped without.** In order of risk:
   - **Codex.** No Codex turn has run, with or without tools. Whether `codex exec` runs an MCP tool without asking, and what a turn with tools looks like in its stream, are unknown. It needs an account the service accepts. Until then the access popover says that Codex is unverified; consider refusing the tools for Codex instead of warning.
   - **The real Claude Code CLI through the window.** Done once by hand, with no automated test: one turn of Claude Code 2.1.303 through the bundled debug app on the end-to-end PostgreSQL database — two `run_select` calls, a correct answer, a proposal that changed nothing until it was approved and then exactly the proposed row ([TESTING.md](TESTING.md), "Results"). The end-to-end test still drives a stand-in. Not checked in that run, and still open: what the real CLI reports as available in a turn with tools (that no built-in tool exists), and that `--setting-sources ""` kept the user's instruction files out.
   - **The paths the end-to-end test does not take**: a query tab in front and no tab in front, rows on MySQL and the "Structure" level on PostgreSQL (the `agent-mysql` stage now covers MySQL at "Structure" with a table tab in front), a Production-labelled connection (the two warnings), a read-only and a closed connection at approval, Stop of a running proposal, a failing statement in a proposal, "Clear log". The fixtures and the stand-in exist; most of these are additions to the script.
   - **Small things the documentation pass found**: the reading tool and an approved proposal resolve unqualified names through the session's `search_path`, not the schema of the tab in front (the instructions for a turn with tools now tell the model to write the schema on PostgreSQL; the resolution itself is unchanged and nothing enforces the rule); the 30-second limit of the reading tool has no test.
2. **Close the verification gaps of the SQL console.** It is still the riskiest surface, and it is now also what runs an approved proposal: the impact guard, Stop and the statement-by-statement commit are the console's. Stop, the lost-connection path, the Production confirmation and anything on PostgreSQL through a query tab are verified below the UI only. In the same pass decide the open hardening questions listed under Phase 4 "Remaining" (transaction state from the server instead of the text; a read-only refusal that typed SQL cannot switch off). The second no longer blocks the AI's reading tool, which got its own guarantee (one statement, prepared by the server, in a read-only transaction); it remains open for query tabs.
3. **Finish the first slice of the chat.** Decide what to do about Codex's shell tool; add a version check; fix the small things listed under Phase 5 "Remaining" (the schema of an opened query tab, masking literals other than passwords in the active query).

After that, the next step for the AI is per-column redaction — rows can now leave the machine, cut for length only — and a policy per connection for what the AI may do.

Files now open in Smart Paste (CSV and workbooks, read by `table_file` with `csv` and `calamine`), within its limit of 10,000 rows in one transaction. Lifting that limit is the remaining part of the Phase 4 import: several batches, a report of the rows that failed, and a long job that can be stopped. It reuses what exists (`db::smart` for matching, the console's dedicated connection and cancellation). It is independent of the AI work and can be taken in parallel. Object editing with DDL preview is the larger remaining Phase 4 item and can follow. The largest Phase 2b items (SQL Server, SSH tunnels, TLS verification) still wait on test infrastructure that is not available (see the blockers below). Phase 2b items that need none of it — cancelling browse queries with the console's canceller, keyset pagination — can be taken alongside.

## Phase 4 — SQL and schema: partial

Done (MySQL and PostgreSQL; evidence in [TESTING.md](TESTING.md), design in [ADR 0015](docs/adr/0015-sql-console.md)):

- Query tabs with a Monaco editor ([ADR 0007](docs/adr/0007-sql-editor.md)), loaded as its own chunk and running under the unchanged production CSP; opened from the explorer, the command palette or ⌘T; text saved per tab and restored with the workspace.
- Execution on one dedicated connection per tab over the simple/text protocol (the answer to the open question of [ADR 0004](docs/adr/0004-server-side-text-rendering.md)): run the selection or the whole script, statement by statement, several result sets, exact text values, a Messages list with status, kind, rows and time.
- Dialect-aware statement splitting and classification in the backend.
- Safeguards: confirmation before destructive statements (every non-read statement on Production), an impact guard that rolls back a row-changing statement above the connection's row threshold, a row cap per result, Stop, the first error stops the script.
- Transactions the user opens: tracked, left alone by the guard, flagged on the tab, protected by the same guards as staged edits. MySQL autocommit is read from the server.
- A lost connection is reported and replaced on the next run; closing a tab or disconnecting interrupts a running statement.
- Export of a result to CSV, JSON and XLSX, written by the backend through the native Save dialog.
- History per connection in the local store, with password literals masked (single- and double-quoted); a connection's history is deleted with it.
- Completion (table names, columns, keywords) and formatting.

Verified in the backend on both engines; through the UI on MySQL only.

Remaining:

| Item | Notes |
|---|---|
| SQL console: verification gaps | PostgreSQL through the UI; Stop and the lost-connection path through the UI; the confirmation on a Production-labelled connection; completion, formatting, the Save dialog, restoring a tab's text after a relaunch; results near 100,000 rows ([TESTING.md](TESTING.md) gaps) |
| SQL console: hardening | Transaction tracking is textual (procedures, `XA`, a MySQL deadlock and non-DDL implicit commits are not seen; the exception for `CREATE` / `DROP TEMPORARY TABLE` has no test against a server); the confirmation and the impact threshold are applied by the UI, not enforced by the backend; a read-only connection can be switched back by typed SQL; history has no off switch, and its password mask is a heuristic ([SECURITY.md](SECURITY.md), SQL console) |
| Saved scripts | Today a tab's text lives only as long as the tab |
| Run the statement at the cursor | Today: the selection, or the whole script |
| Alias-aware completion | Today: loaded table names, columns of tables named in the text, keywords. Inline error markers are not built either |
| Streaming, and results or exports beyond 100,000 rows | Results are held in memory, capped at 100,000 rows; a capped result cannot be continued. Sorting and filtering a result are not available |
| Switching database inside a PostgreSQL tab | A PostgreSQL tab is bound to the database it was opened on |
| Query parameters | Not started |
| EXPLAIN view | `EXPLAIN` can be typed; its output is shown as an ordinary result |
| Object editing (table, index, view) with DDL preview | Not started. DDL can be typed in a query tab |
| Import of more than 10,000 rows | Not started. Files up to 10,000 rows open in Smart Paste (Phase 3, done). Beyond that an import needs batches, an error report per row and a job that can be stopped; a file is read whole into memory today |
| SQL dump export | Not started |
| Translating backend messages | Danger reasons, the lost-connection note, "Cancelled" and the history summary are English only |

## Phase 5 — AI: partial

Done (the first slice: a chat in which the model has no tools and no database access; design in [ADR 0016](docs/adr/0016-agent-chat-without-tools.md), details in [AI_INTEGRATION.md](AI_INTEGRATION.md), evidence in [TESTING.md](TESTING.md)):

- CLI detection for Claude Code and Codex; the chat is offered only for a CLI that is installed and not known to be signed out, otherwise the panel says what is missing.
- Subprocess runner: one CLI process per turn in a process group of its own, fixed argument vector, no shell, the prompt on stdin, an empty working directory owned by PuriCat, kill-on-drop, a 10-minute timeout, Stop killing the whole group.
- Stream parsers for `claude -p --output-format stream-json` (confirmed by capture from Claude Code 2.1.303) and `codex exec --json` (start and failure lines captured from codex-cli 0.133.0; **the success lines are from the documented format and unverified against the real CLI**).
- Chat UI in the AI panel: provider switch, model selector (default, the CLI's models, any name), streamed answers rendered from Markdown as text, errors in the CLI's own words with "Try again", "New chat"; English and Thai.
- Schema context produced by the backend — names and types only, 40 tables in full and the rest by name, plus the active query tab's SQL — shown in a context inspector and kept with every sent message; a switch turns sharing off.
- "Open in query tab" for SQL in an answer: a new query tab with the text, nothing run.
- Stateless turns: the CLIs keep no session; the conversation lives in memory and is resent each turn; only the provider, the model per provider and the sharing switch are stored.
- Tests with stand-in CLIs (unit and end-to-end) and one opt-in test against the real Claude Code CLI.

Verified for Claude Code on stand-ins (unit tests; end-to-end in dev mode and against the bundled app, repeated on the build with the database tools) and by an opt-in test against the real CLI. Through the UI a turn without tools ran with a PostgreSQL context only.

Done (the second slice: database tools for the model, off by default; design in [ADR 0017](docs/adr/0017-ai-database-tools.md), details in [AI_INTEGRATION.md](AI_INTEGRATION.md), evidence in [TESTING.md](TESTING.md)):

- Three access levels next to the model selector — Off (default), Structure, Structure and rows — remembered in the settings and bound per turn to the database of the tab in front; without a table or query tab in front a turn runs without tools.
- PuriCat as the MCP server: JSON-RPC over a Unix socket that exists for one turn, in a directory only the user can enter, reached by the CLI through PuriCat's own executable started with `--mcp-bridge`. No port, no MCP library, no credential outside the app.
- Tools: `list_schemas` (PostgreSQL), `list_tables`, `describe_table`; `run_select` when rows are allowed (50 rows by default, 200 at most, cells cut at 300 characters, 30 seconds); `propose_sql`, which runs nothing.
- A reading tool that SQL cannot switch off: one statement by its text, prepared by the server first, in a read-only transaction that is rolled back, on a connection of its own.
- Proposals: stored in the backend; a card in the conversation; a review dialog with the SQL, the connection, its environment and database, the flagged statements and the guard; "Approve and run" runs it on a console of its own with the SQL console's impact guard, the row threshold read by the backend; above the threshold the statement is rolled back and running what was held back is a third confirmation; "Reject"; "Open in query tab". A read-only connection refuses the run.
- Every tool call shown in the answer as a step that opens to what was asked and exactly what the model was given.
- An activity log in the local store (third migration): row-reading queries, refused or failed reads, proposals, approvals, rejections and outcomes, with password literals masked; at most 5,000 entries; "Clear log".
- Stateless turns kept: the next turn is told in one line per item which tools were used and what became of each proposal; tool results are not resent.
- Tests: unit tests for the MCP server, the text check, the argument vectors, the stream of a turn with tool calls and the log; a live suite on both engines (`live_agent_tools`); the end-to-end stages on a stand-in CLI that is a real MCP client (PostgreSQL, and one stage on MySQL); one opt-in test with the real Claude Code CLI.

Verified for Claude Code: the tools and the read-only guarantee on MySQL and PostgreSQL below the UI; through the UI with a table tab in front, on a stand-in CLI, in dev mode and against the bundled app — on PostgreSQL at the "Structure and rows" level and on MySQL at the "Structure" level; the real Claude Code CLI through PuriCat's own server and bridge below the UI (the opt-in test), and once through the window, by hand, on PostgreSQL at "Structure and rows" — a single turn, not an automated test. **Codex: the MCP handshake only.**

Remaining:

| Item | Notes |
|---|---|
| Codex verified against the real CLI | No successful turn has been seen, with or without tools; the service refused every model tried. Until then Codex support rests on the documented stream format, and for the tools on a handshake: a tool call, and whether `codex exec` runs an MCP tool without asking, are unknown. The stream parser ignores Codex's tool items |
| Codex's shell tool | Codex keeps its own shell tool inside a read-only sandbox; no documented option was found to remove it. Decide: accept and say so in the panel, or find a way to run it without |
| Version gating | Nothing checks the CLI version; a changed option or stream format surfaces as a failed turn. Minimum versions are not established |
| Chat: verification gaps | The real CLIs through the UI in a turn without tools (the one manual turn of the real Claude Code CLI through the window had the tools on); the 10-minute timeout; the cut after 40 tables and 80 columns; the text of a MySQL context (the `agent-mysql` stage checks only the context button's label) and a query tab's context; the sharing switch, "Context sent", "New chat", "Copy", the composer's keys and IME; loading and saving the chat preferences ([TESTING.md](TESTING.md) gaps) |
| Chat: known defects and rough edges | a query tab opened from an answer carries no schema, so unqualified names from a non-`public` PostgreSQL schema do not resolve; only password literals are masked in the active query's text; the first 40 tables are taken in catalogue order, not by relevance, and foreign keys are not part of the context; switching the provider resends the conversation to the other provider |
| Conversation history | The conversation is lost when the app closes, with its tool steps and undecided proposals; one conversation at a time; no export |
| Tools: verification gaps | An automated test of the real Claude Code CLI with tools through the UI (it was done once by hand); in a real turn through the window, what the CLI makes available besides PuriCat's tools and that `--setting-sources ""` keeps the user's instruction files out (neither was looked at in the manual run, and no test asserts either); the tools through the UI with a query tab in front or no tab in front, with rows on MySQL, and at the "Structure" level on PostgreSQL; the Production warnings; a read-only or closed connection at approval; Stop of a running proposal; a failing proposal statement; the 30-second query limit; "Clear log" and the log's size cap through the UI; the tool stages in Thai and the dark theme in the committed end-to-end test (looked at by hand once) ([TESTING.md](TESTING.md) gaps) |
| Tools: Unix only | The socket and the bridge use Unix domain sockets. On other platforms a turn with tools fails with a message; a transport for Windows is not designed |
| Tools: limits of the read-only guarantee | It is about writing through SQL: a function with effects outside the transaction can be called if the account may call it, and the list of refused function names matches the plain spelling only. On MySQL a query can name another database the account can read. An account without write privileges remains the strong control |
| Tools: rough edges | The level is one global setting and a UI rule (the backend takes the scope from the web view); no per-connection policy; proposals live in memory; statements of a proposal are committed one by one; "Run without the limit" lifts the limit for the held statement and for every statement after it (the confirmation shows them all); the reading tool and an approved proposal resolve unqualified names through the session's `search_path`, not the tab's schema (the instructions for a turn with tools ask the model to write the schema on PostgreSQL; nothing enforces it); step order in an answer is best effort; any process of the same OS user that finds the socket during a turn can call the tools |
| Context redaction | Not started: columns marked sensitive, sample rows on request, pinning, a scope the user can narrow. With "Structure and rows" the rows the model reads are sent as the database returned them, cut for length only |
| Translating backend messages | The chat's own error messages are English only; so are, in the Thai UI, the reason a tool step failed, the note on a flagged statement, the row count on a query step and the detail line of an activity-log entry |

## Phase 6 — Backups: not started

1. Tool detection and version checks; manual backup for MySQL and PostgreSQL.
2. Restore workflow with the safety rules in [BACKUP_RESTORE.md](BACKUP_RESTORE.md).
3. `puricat-backup` helper and LaunchAgent install with consent ([ADR 0010](docs/adr/0010-backup-scheduler.md)).
4. Retention, optional encryption, history, notifications.
5. SQL Server strategy, clearly labelled as server-side.

## Phase 7 — Public beta: not started

- Security hardening and an independent review; dependency audit in CI.
- Accessibility and contrast audit; performance work on large tables.
- macOS signing, notarization, hardened runtime; signed auto-update.
- Onboarding, privacy policy, release notes, generated third-party notices.
- Reproducible release process. Windows follows after macOS stabilises.

## Blockers and decisions needed from the product owner

| Decision | Needed for | Why it blocks |
|---|---|---|
| The pricing model | Decided on 2026-11-16: PuriCat is free for anyone to use, and open source under MIT ([LICENSE](LICENSE), [LICENSES.md](LICENSES.md)) | No longer open |
| Signing secrets on the repository | Done on 2026-11-17: the owner added the six `APPLE_*` secrets, and a hand-started run of the release workflow produced a signed and notarized download | No longer open. v0.1.0 itself was signed and notarized by hand (`scripts/release-mac.sh`) before that. The bundle identifier was set to `com.purivaro.puricat` for the first public release, following the owner's other app; changing it later orphans users' settings |
| AG Grid Enterprise licence, or stay with the custom grid | Only if the remaining Phase 3 grid validation fails | Editing, single-range selection and paste are now built on the custom grid; the question returns only if its performance or multi-range validation fails ([ADR 0006](docs/adr/0006-data-grid.md)) |
| A way to run a disposable SQL Server (Docker or a test server) | Phase 2b | No adapter work can be verified without one |
| Hosting for the update channel | Phase 7 | The signed-update design depends on where artifacts and manifests are served |

Not a product decision, but also needed: a TLS-enforcing MySQL and PostgreSQL test server and a disposable SSH server for Phase 2b verification, and a first CI run.
