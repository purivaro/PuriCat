# PuriCat requirements

This is a condensed restatement of the product brief. The source of truth is [docs/PuriCat_Master_Requirements.md](docs/PuriCat_Master_Requirements.md); if the two disagree, the brief wins.

Status values: **Done** (implemented and verified), **Partial** (some of it is implemented; the note says what), **Planned (Phase N)** (not in the code). Evidence for "Done" is in [TESTING.md](TESTING.md). Status as of 2026-11-16.

## Confirmed product decisions

- macOS first (Apple Silicon), Windows later; platform abstractions stay portable.
- Replace Navicat for routine work, delivered in phases.
- Two editing paths: direct paste into an editable grid with staged changes, and a guided Insert / Update / Upsert wizard.
- AI agent through locally installed Claude Code and Codex CLIs, using documented interfaces only. Every AI-initiated mutation needs explicit approval.
- Ordinary row edits commit on Save; bulk changes and AI mutations need preview and confirmation, configurable per connection and stricter for production.
- Public distribution: secure defaults, no hard-coded paths, accounts or private infrastructure.
- SSH tunnels, several simultaneous connections, light and dark themes, PostgreSQL, scheduled backups, English and Thai.

## 1. Application shell and design

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Tauri desktop app with explorer (left), tabbed workspace (centre), AI panel (right) | Panels resize and collapse; widths and visibility persist | Done |
| Multi-tab workspace with restoration | Tabs identify their connection and database; open tabs are stored and reloaded | Partial: tabs are stored and reloaded on start; relaunch restore is not covered by a test |
| Light and dark themes, white/orange palette | Theme follows the OS or an explicit choice; tokens match the brief | Partial: implemented; contrast audit Planned (Phase 7), see [ADR 0011](docs/adr/0011-i18n-and-theming.md) |
| English and Thai | Every UI string comes from a catalog; language switch needs no restart | Done |
| Command palette and shortcuts | Palette lists actions, connections, tabs and loaded tables | Partial: palette verified; other shortcuts have no automated test |
| Loading, empty and error states | No blank panels; errors show the server's message | Done for implemented views |
| Accessibility | Keyboard reachable, ARIA roles, reduced motion | Partial: roles and reduced-motion rule exist; no audit (Phase 7) |
| Original cat icon and brandmark | No third-party logos | Done (see [LICENSES.md](LICENSES.md)) |

## 2. Connection manager

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Create, edit, test, duplicate, tag, favorite, delete | Test reports engine, version, TLS state and latency without saving | Done (edit, tag and favorite lack an automated test) |
| Reconnect | Dropped sessions recover or report clearly | Planned (Phase 2b) |
| MySQL and PostgreSQL: host, port, user, password, database, TLS | Connect with typed options; real error on failure | Done |
| Timeouts and connection options | Per-connection connect timeout and options | Partial: timeout is stored (default 10 s, clamped 1–120) but has no field in the dialog |
| SQL Server, including an honest account of integrated auth | Connect, browse; limits documented | Planned (Phase 2b) |
| Secrets in the OS credential store | Passwords never in SQLite, logs or IPC responses | Done on macOS; Windows not yet verified |
| TLS modes with verification | `verify-full` rejects a wrong host or untrusted CA | Partial: modes are mapped to the drivers; verification untested against a TLS-enforcing server |
| SSH tunnels with host-key verification | Key, passphrase and agent auth; no insecure bypass by default | Planned (Phase 2b) |
| Several concurrent connections, pooling with limits, idle timeout | Two engines browsable at once | Done |
| Query cancellation | Long operations can be stopped | Planned (Phase 2b) |
| Read-only mode | Enforced by the server, not only the UI | Done (server reports the flag; see limits in [SECURITY.md](SECURITY.md)) |
| Development / Staging / Production labels | Visible on the connection and its tabs | Done (label only; production-specific safeguards arrive with editing) |
| Lazy, searchable explorer | Levels load on expand; filter narrows loaded nodes | Partial: databases, schemas, tables, views. Procedures, functions and indexes are Planned (Phase 2b) |

## 3. Table browsing

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Virtualized grid | Only visible rows are rendered | Done |
| Exact values | Decimals, 64-bit integers, timestamps, JSON, Unicode shown without loss | Done |
| Paging and sorting | Stable order by primary key; sort column validated against metadata | Done (OFFSET paging; keyset is Planned, Phase 2b) |
| NULL vs empty, truncated previews flagged | Distinct rendering; flag on shortened cells | Done; a cell inspector for full values is Planned (Phase 2b) |
| Structure view | Columns, types, nullability, defaults, keys | Done |
| Exact row count on demand | Estimate is labelled; exact count is opt-in | Partial: estimate shown and labelled; exact count Planned (Phase 2b) |

## 4. Table editing (flagship)

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Editable grid with staged changes, undo/redo, Save / Discard | Paste creates pending edits only; nothing reaches the server before Save | Planned (Phase 3) |
| Clipboard parser (TSV/CSV, quotes, embedded tabs/newlines, Thai, NULL vs empty, precision) | Unit-tested round trips | Planned (Phase 3) |
| Smart Paste: INSERT / UPDATE / UPSERT with mapping and key matching | Requires a primary or validated unique key; previews insert/update/skip/conflict counts | Planned (Phase 3) |
| Safe execution | Parameterized values, quoted identifiers, bounded batches, transactions, optimistic concurrency, no edits on keyless tables | Planned (Phase 3) |
| Bulk and production safeguards | Review step and configurable impact thresholds | Planned (Phase 3) |

## 5. SQL workspace and schema management

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Editor with dialect highlighting, completion, formatting, history | Selection execution, cancellation, duration and affected rows shown | Planned (Phase 4) |
| Multiple result sets, large results, export CSV / XLSX / JSON | UI stays responsive on large results | Planned (Phase 4) |
| Risk-based confirmation for dangerous scripts | Not keyword matching alone | Planned (Phase 4) |
| Create / alter / drop objects with DDL preview | Engine differences explained, not hidden | Planned (Phase 4) |
| Import with mapping and error reports | Batch import with cancellation | Planned (Phase 4) |

## 6. AI agent

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Detect Claude Code and Codex CLIs, version and sign-in state | Missing prerequisites explained; no account details kept | Done |
| Agent conversation with schema context | Streamed responses through a documented CLI interface | Planned (Phase 5) |
| Read-only tools by default; mutations only after approval | Approval shows SQL, target, impact estimate; outcome logged | Planned (Phase 5) |
| Context preview and redaction | User sees and controls what is shared; secrets never sent | Planned (Phase 5) |
| No fabricated responses | No chat UI until it is backed by a real integration | Done (the panel shows detection only) |

## 7. Backup and restore

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Manual backup per engine with version checks | Accurate status; contents documented | Planned (Phase 6) |
| Scheduled backup that runs without the app open | Uses an installed OS scheduler; limits shown in the UI | Planned (Phase 6) |
| Retention, optional encryption, history, notifications | Failures are visible, never silent | Planned (Phase 6) |
| Restore with integrity verification | Never overwrites production without explicit confirmation | Planned (Phase 6) |

## 8. Distribution, security and quality

| Requirement | Acceptance criteria | Status |
|---|---|---|
| Scoped Tauri capabilities and CSP | No shell, filesystem or HTTP access from the web view | Done |
| Subprocesses without shell interpolation of untrusted input | Fixed argument vectors, timeouts | Done for CLI detection (see the note in [SECURITY.md](SECURITY.md)) |
| No telemetry by default | No network calls except to the user's databases | Done (nothing in the code sends telemetry) |
| Signed and notarized macOS build, signed updates | Installs without Gatekeeper warnings | Planned (Phase 7) |
| Licence review and third-party notices | Notices shipped with the app | Partial: [LICENSES.md](LICENSES.md) drafted; generated notices Planned (Phase 7) |
| Tests against disposable databases only | Suites create and drop their own databases | Done for MySQL and PostgreSQL |
| CI | Typecheck, lint and unit tests on every push | Partial: workflow file exists; it has not run yet |
| Windows | Credential store, paths, scheduler, installer | Planned (after macOS beta) |

## Definition of done for the first public macOS beta

From the brief: a new user can install PuriCat, create SSH/TLS-enabled connections to MySQL, SQL Server and PostgreSQL, browse several databases at once, open large tables, edit and paste from Excel in both modes, review and commit safely, run SQL and export results, use an installed Claude Code or Codex CLI to propose SQL with explicit mutation approval, configure manual and scheduled backups with accurate status, switch themes, and understand every limitation from the UI and docs. All of it verified against disposable test databases.

Current distance from that bar is tracked in [ROADMAP.md](ROADMAP.md).
