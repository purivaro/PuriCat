# Roadmap

Status on 2026-11-16. Phases follow the delivery plan in the [product brief](docs/PuriCat_Master_Requirements.md). No dates are committed. Per-requirement status is in [REQUIREMENTS.md](REQUIREMENTS.md); evidence is in [TESTING.md](TESTING.md).

| Phase | Scope | Status |
|---|---|---|
| 0 | Discovery and decisions | Done |
| 1 | Runnable foundation | Done |
| 2 | Real database connectivity | Partial (remainder tracked as Phase 2b) |
| 3 | Flagship editing | Partial (grid editing and the Smart Paste wizard done for MySQL and PostgreSQL; the leftovers are listed below) |
| 4 | SQL and schema | Not started |
| 5 | AI agent | Not started (CLI detection only) |
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
| Query cancellation | Verify driver support per engine |
| Reconnect on dropped connections | Today a dropped connection is an error on the next action |
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

Phase 4 (SQL and schema), starting with the editor spike and the query execution path. Reasons: the largest Phase 2b items (SQL Server, SSH tunnels, TLS verification) wait on test infrastructure that is not available yet (see the blockers below), and the Phase 3 leftovers are refinements of working features. The import tool in Phase 4 is also where Smart Paste's row limit and file sources are meant to be lifted. Phase 2b items that need no extra infrastructure (query cancellation, reconnect, keyset pagination, the cell inspector) can be taken alongside. This is a recommendation, not a commitment.

## Phase 4 — SQL and schema: not started

1. Editor spike: Monaco under the production CSP ([ADR 0007](docs/adr/0007-sql-editor.md)).
2. Query execution path with native result decoding ([ADR 0004](docs/adr/0004-server-side-text-rendering.md), open question), multiple result sets, cancellation.
3. History, saved scripts, formatting, schema-aware completion.
4. Risk-based confirmation for destructive statements.
5. Object editing with DDL preview; import and export (CSV, XLSX, JSON, SQL).

## Phase 5 — AI: not started

1. Resolve the open questions in [AI_INTEGRATION.md](AI_INTEGRATION.md) from official CLI documentation.
2. Subprocess runner with streaming, cancellation and version gating.
3. PuriCat MCP server with read-only tools.
4. Proposal and approval queue; audit log.
5. Context preview and redaction.
6. Tests with a mocked CLI, plus opt-in smoke tests against the real CLIs.

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
