# Roadmap

Status on 2026-11-16. Phases follow the delivery plan in the [product brief](docs/PuriCat_Master_Requirements.md). No dates are committed. Per-requirement status is in [REQUIREMENTS.md](REQUIREMENTS.md); evidence is in [TESTING.md](TESTING.md).

| Phase | Scope | Status |
|---|---|---|
| 0 | Discovery and decisions | Done |
| 1 | Runnable foundation | Done |
| 2 | Real database connectivity | Partial (remainder tracked as Phase 2b) |
| 3 | Flagship editing | Not started |
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
- Read-only table browsing with exact values, paging and sorting; structure view.

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

Smaller follow-ups found while documenting: a connect-timeout field in the dialog; display of the TLS state of an open session; a live test for wrong passwords (needs a server with password authentication); a tie-breaker or an "unstable order" flag when a keyless table is sorted.

## Phase 3 — Flagship editing: not started

1. Grid spike that decides custom grid vs Glide Data Grid ([ADR 0006](docs/adr/0006-data-grid.md)).
2. Clipboard parser with unit tests (TSV/CSV, quoting, NULL vs empty, Thai, precision).
3. Staged edits: change tracking, highlight, undo/redo, Save / Discard.
4. Write path per engine: parameterized, keyed by primary or unique key, optimistic concurrency, transactions; no edits on keyless tables.
5. Smart Paste wizard: INSERT / UPDATE / UPSERT, column mapping, key validation, counts preview, batching with progress and cancellation.
6. Per-connection safety policy: review step for bulk changes, impact thresholds, stricter production defaults.

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
| AG Grid Enterprise licence, or stay with the custom grid | Start of Phase 3 | Range selection and clipboard paste are Enterprise-only in AG Grid ([ADR 0006](docs/adr/0006-data-grid.md)) |
| A way to run a disposable SQL Server (Docker or a test server) | Phase 2b | No adapter work can be verified without one |
| Hosting for the update channel | Phase 7 | The signed-update design depends on where artifacts and manifests are served |

Not a product decision, but also needed: a TLS-enforcing MySQL and PostgreSQL test server and a disposable SSH server for Phase 2b verification, and a first CI run.
