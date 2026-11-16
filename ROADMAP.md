# Roadmap

Status on 2026-11-16. Phases follow the delivery plan in the [product brief](docs/PuriCat_Master_Requirements.md). No dates are committed. Per-requirement status is in [REQUIREMENTS.md](REQUIREMENTS.md); evidence is in [TESTING.md](TESTING.md).

| Phase | Scope | Status |
|---|---|---|
| 0 | Discovery and decisions | Done |
| 1 | Runnable foundation | Done |
| 2 | Real database connectivity | Partial (remainder tracked as Phase 2b) |
| 3 | Flagship editing | Partial (grid editing and the Smart Paste wizard done for MySQL and PostgreSQL; the leftovers are listed below) |
| 4 | SQL and schema | Partial (the SQL console is done for MySQL and PostgreSQL; object editing, import and the rest are listed below) |
| 5 | AI agent | Partial (CLI detection and an agent chat in which the model has no tools; the MCP server, read-only tools and the approval queue are not built) |
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
- Build scripts and a CI workflow file (not yet run).

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
| Cell inspector | Fetch and show the full value of a truncated cell |
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
- Smart Paste wizard ([ADR 0014](docs/adr/0014-smart-paste-match-then-write.md)): clipboard block, header-row detection, column mapping, INSERT / UPDATE / UPSERT matched on the primary key or a plain unique index read from the catalogue, generated keys used for matching only, duplicate detection, cheap pre-checks, a read-only preview with one outcome per pasted row, re-analysis at run time, one transaction of at most 10,000 rows through the same planner as grid saves, progress and "Stop and roll back". Verified in the backend on both engines; through the UI for UPSERT on MySQL only.

Remaining:

| Item | Notes |
|---|---|
| Smart Paste: "skip invalid rows" | Today any row with a problem blocks the whole run until the source is fixed |
| Smart Paste: value-aware comparison | "Update" means the pasted text differs from the stored text, so `1.5` against a stored `1.500000` is counted and run as an update |
| Smart Paste: verification gaps | PostgreSQL, INSERT and UPDATE modes, composite keys, the progress bar, Stop and the blocked dismissal during a run through the UI (composite-key matching itself is verified in the backend on both engines); pastes near the 10,000-row limit and their duration on a remote server |
| Smart Paste: more than 10,000 rows, CSV / XLSX files | One run is one transaction of single-row statements. Multi-batch or streamed imports and file sources belong to the Phase 4 import tool |
| Full-value cell inspector / editor | For long values (beyond the 1000-character preview) and binary values; until then such cells are not editable. The read-only inspector is also listed under Phase 2b |
| Filtering | Not started |
| Column reorder / hide, pinned key columns | Not started |
| Multi-range (non-contiguous) selection | Only one rectangular range today |
| Keyset pagination | Also listed under Phase 2b |
| Editing across pages | Edits are limited to the loaded page; paging is locked while edits are pending |
| Client-side type hints and validation before save | Today the server validates at Save |
| Audit log of saves | Not started |
| Grid validation | Performance with many columns and pages of 1000 wide rows; real keyboard, IME and drag input; the Production and bulk-threshold review paths through the UI ([ADR 0006](docs/adr/0006-data-grid.md), [TESTING.md](TESTING.md) gaps) |

Known limitations of what is built (DELETE checks the key only; PostgreSQL sessions run in UTC; keyless tables stay read-only in the UI; Smart Paste compares text and runs as a single transaction) are in [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md) and [SECURITY.md](SECURITY.md).

## Recommended next step

Do not give the model tools yet. Take three things in this order. This is a recommendation, not a commitment.

1. **Close the verification gaps of the SQL console, and make read-only hold against SQL.** The console runs arbitrary SQL, which makes it the riskiest surface in the app, and since the agent chat it is also where AI-written SQL lands ("Open in query tab"). Several of its safeguards are verified below the UI only: Stop, the lost-connection path, the Production confirmation, and running anything on PostgreSQL through the UI. Extending the end-to-end test to a PostgreSQL query tab, a Stop, and a Production-labelled connection is small work with the fixtures that exist. In the same pass, decide the open hardening questions listed under Phase 4 "Remaining" (transaction state from the server instead of the text; a read-only refusal that typed SQL cannot switch off). The second is a precondition for the AI's read-only query tool, not only a console nicety.
2. **Finish the first slice of the chat before widening it.** See a successful Codex turn from the real CLI (it needs an account the service accepts) or label Codex as unverified in the panel; decide what to do about Codex's shell tool; add a version check; and fix the small things listed under Phase 5 "Remaining" (Stop while the context is being read, the schema of an opened query tab, masking the active query before it is sent). Repeat the end-to-end run, dev and bundled, on the turn handling as it is now, with the screen awake: the last change to it (process groups, commit `1515bac`) was not followed by one.
3. **Then the MCP server with read-only tools** ([ADR 0008](docs/adr/0008-ai-cli-integration.md)), which first needs its open questions answered: how each CLI is handed a per-invocation MCP configuration, and which Claude permission mode allows only PuriCat's tools.

Import from CSV / XLSX remains the next Phase 4 feature users will miss: it is where Smart Paste's 10,000-row limit and file sources are meant to be lifted, and it reuses what exists (`db::smart` for matching, the console's dedicated connection and cancellation for a long job). A library for reading XLSX has still to be chosen (`rust_xlsxwriter` only writes). It is independent of the AI work and can be taken in parallel. Object editing with DDL preview is the larger remaining Phase 4 item and can follow. The largest Phase 2b items (SQL Server, SSH tunnels, TLS verification) still wait on test infrastructure that is not available (see the blockers below). Phase 2b items that need none of it — cancelling browse queries with the console's canceller, keyset pagination, the cell inspector — can be taken alongside.

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
| SQL console: hardening | Transaction tracking is textual (procedures, `XA`, a MySQL deadlock and non-DDL implicit commits are not seen; the exception for `CREATE` / `DROP TEMPORARY TABLE` is a keyword test); the confirmation and the impact threshold are applied by the UI, not enforced by the backend; a read-only connection can be switched back by typed SQL; history has no off switch, and its password mask misses a literal that follows a double-quoted `"password"` identifier ([SECURITY.md](SECURITY.md), SQL console) |
| Saved scripts | Today a tab's text lives only as long as the tab |
| Run the statement at the cursor | Today: the selection, or the whole script |
| Alias-aware completion | Today: loaded table names, columns of tables named in the text, keywords. Inline error markers are not built either |
| Streaming, and results or exports beyond 100,000 rows | Results are held in memory, capped at 100,000 rows; a capped result cannot be continued. Sorting and filtering a result are not available |
| Switching database inside a PostgreSQL tab | A PostgreSQL tab is bound to the database it was opened on |
| Query parameters | Not started |
| EXPLAIN view | `EXPLAIN` can be typed; its output is shown as an ordinary result |
| Object editing (table, index, view) with DDL preview | Not started. DDL can be typed in a query tab |
| Import from CSV / XLSX | Not started. Also where Smart Paste's 10,000-row limit and file sources are to be lifted (Phase 3 table above) |
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

Verified for Claude Code on stand-ins (unit tests; end-to-end in dev mode and against the bundled app, before the process-group change) and by an opt-in test against the real CLI. Through the UI the chat ran with a PostgreSQL context only.

Remaining:

| Item | Notes |
|---|---|
| Codex verified against the real CLI | No successful turn has been seen; the service refused every model tried. Until then Codex support rests on the documented stream format |
| Codex's shell tool | Codex keeps its own shell tool inside a read-only sandbox; no documented option was found to remove it. Decide: accept and say so in the panel, or find a way to run it without |
| Version gating | Nothing checks the CLI version; a changed option or stream format surfaces as a failed turn. Minimum versions are not established |
| Chat: verification gaps | The real CLIs through the UI; the 10-minute timeout; the cut after 40 tables and 80 columns; a MySQL context and a query tab's context; the sharing switch, "Context sent", "New chat", "Copy", the composer's keys and IME; the agent store ([TESTING.md](TESTING.md) gaps) |
| Chat: known defects and rough edges | Stop is ignored while the context is still being read, before the CLI has started; the first `error` line from Codex ends the turn as failed, whether or not Codex would have recovered; a query tab opened from an answer carries no schema, so unqualified names from a non-`public` PostgreSQL schema do not resolve; the active query's text is sent unmasked; the first 40 tables are taken in catalogue order, not by relevance, and foreign keys are not part of the context; switching the provider resends the conversation to the other provider |
| Conversation history | The conversation is lost when the app closes; one conversation at a time; no export |
| PuriCat MCP server with read-only tools | Not started. List schemas, describe a table, a row-limited single `SELECT` in a read-only transaction. Open questions: per-invocation MCP configuration for Codex, the Claude permission mode that allows only these tools, transport and library ([ADR 0008](docs/adr/0008-ai-cli-integration.md)) |
| Proposal and approval queue; audit log | Not started. Until then the user is the only path from an answer to a database |
| Context redaction | Not started: columns marked sensitive, sample rows on request, pinning, a scope the user can narrow |
| Translating backend messages | The chat's own error messages are English only |

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
| PuriCat's own licence, and the pricing / licensing model | Phase 7; affects dependency choices earlier | The workspace is `publish = false` with no licence. A commercial model changes what can be bundled |
| Final bundle identifier and an Apple Developer ID | Signing and notarization; ideally before any external tester | `app.puricat.desktop` is a placeholder. Changing it after users have data orphans their settings |
| AG Grid Enterprise licence, or stay with the custom grid | Only if the remaining Phase 3 grid validation fails | Editing, single-range selection and paste are now built on the custom grid; the question returns only if its performance or multi-range validation fails ([ADR 0006](docs/adr/0006-data-grid.md)) |
| A way to run a disposable SQL Server (Docker or a test server) | Phase 2b | No adapter work can be verified without one |
| Hosting for the update channel | Phase 7 | The signed-update design depends on where artifacts and manifests are served |

Not a product decision, but also needed: a TLS-enforcing MySQL and PostgreSQL test server and a disposable SSH server for Phase 2b verification, and a first CI run.
