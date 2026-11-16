# PuriCat — Master Requirements & Claude Code Implementation Prompt

> Copy this entire document into Claude Code as the project brief. The product requirements below are confirmed by the product owner. Do not silently narrow their scope.

## Role and mission

Act as a principal desktop software architect, Rust/Tauri engineer, React/TypeScript engineer, database specialist, security engineer, and product designer. Build **PuriCat — The Intelligent Database Studio**, a production-quality, publicly distributable desktop database client. It should replace Navicat for nearly all routine workflows while feeling more polished, approachable, and spreadsheet-native. **Build functioning software, not a static mockup.**

## Confirmed product decisions

1. **Platforms:** macOS first (Apple Silicon priority), Windows later; keep platform abstractions portable.
2. **Scope:** replace Navicat for nearly all routine database management, delivered incrementally.
3. **Editing:** support both direct Excel-style paste into an editable grid with staged changes and a guided Insert/Update/Upsert smart-paste wizard.
4. **AI:** integrated agent capable of schema exploration, SQL drafting, diagnostics, and proposing multi-step operations; any database mutation initiated by AI requires explicit approval.
5. **AI connectivity:** prioritize locally installed, already authenticated **Claude Code CLI** and **OpenAI Codex CLI**, using their documented supported interfaces. Do not assume undocumented CLI flags, subscription entitlements, or API access.
6. **Production safety:** ordinary manual row edits may be committed normally, but bulk changes and AI-initiated mutations require preview and confirmation. Make this policy configurable per connection, with stronger safeguards for production.
7. **Distribution:** intended for the general public; prioritize secure defaults, licensing, installers, updates, onboarding, and clear documentation.
8. **Additional requirements:** SSH tunnels, simultaneous connections to multiple databases, Light and Dark modes, PostgreSQL support, and automatic scheduled backups.
9. **Branding:** PuriCat, elegant minimal cat identity, white/orange primary Light theme, coordinated dark theme. Thai and English localization-ready.

## Product principles

- Spreadsheet-first editing without compromising data integrity.
- Native-feeling, fast, keyboard-friendly desktop experience.
- Multi-engine support through capability-aware database adapters, not fragile SQL string substitutions.
- AI assists but does not silently act on production data.
- Connection secrets never reach AI prompts or logs.
- Visible operation status, accurate affected-row counts, error recovery, and auditability.
- Public distribution means no hard-coded paths, accounts, or private infrastructure.

## Proposed architecture — validate before implementation

- **Desktop:** Tauri 2, Rust backend, React + TypeScript strict + Vite frontend.
- **UI:** Tailwind CSS, shadcn/ui (adapted for desktop), Lucide, accessible components; state management such as Zustand only where useful.
- **Grid:** evaluate AG Grid Community, Glide Data Grid, and alternatives for multi-range clipboard paste, virtual scrolling, editable cells, and license suitability. Do not rely on paid-only features without a product licensing decision.
- **SQL editor:** Monaco Editor with dialect-aware completion and execution.
- **DB adapters:** MySQL (e.g., sqlx), PostgreSQL (e.g., sqlx), SQL Server (e.g., Tiberius or a maintained alternative); verify actual TLS, auth, streaming, cancellation, and transaction capabilities. Expose per-engine capability flags.
- **Settings/history:** local SQLite with migrations; secrets in macOS Keychain / Windows Credential Manager, not plain SQLite.
- **SSH:** secure, host-key-verified tunnels; support keys/passphrases and appropriate agent integration; manage lifecycle and port conflicts.
- **AI adapters:** subprocess-based integrations using officially supported structured CLI invocation, streaming, process cancellation, auth checks, explicit tool permissions, and careful output parsing. Detect installation and explain missing prerequisites.
- **Backup scheduler:** native background scheduling architecture with clear limitations when app is closed or machine sleeps. Evaluate a helper/service/OS scheduler for reliable unattended backups; do not promise unattended runs without a working installed scheduler and credentials. Support encryption, retention, restore tests, and notification/reporting.

Create an Architecture Decision Record for each major choice, including alternatives and licensing implications.

## UX and visual requirements

### Design system
- Light: white #FFFFFF, warm off-white #FAFAF9, orange #FF7A18, hover #F56800, pale orange #FFF1E6, text #202124, border #E8E8E8.
- Dark: charcoal/near-black surfaces with restrained orange accents, excellent contrast; avoid excessive saturation.
- Modern macOS aesthetic: refined spacing, typography (system/Inter, JetBrains Mono for SQL), subtle borders, restrained shadows, excellent focus states, responsive panels, reduced-motion support.
- Distinctive original orange cat app icon and minimal brandmark; do not copy another app's assets.
- All text should be internationalization-ready, starting with English and Thai.

### Main window
- Left: connection explorer with favorites, recent items, databases, schemas, tables, views, procedures, functions, indexes as supported, connection status, search, context menus.
- Center: multi-tab table data/structure, SQL editor/results, import/export, object designer, backups; persistent unsaved-change indicators.
- Right: collapsible AI agent panel with Claude/Codex provider switch, context inspector, permissions, streamed responses, generated SQL diffs and approval queue.
- Top: global search/command palette, quick connection switch, active connection/environment label, Light/Dark toggle.
- Support multiple connections and query tabs simultaneously, including different engines; every tab clearly identifies its connection and database.
- Context menus, keyboard shortcuts, adjustable panel widths, tab restoration, loading/empty/error states, accessibility.

## Database connection manager

- Create, edit, test, duplicate, tag, favorite, delete, reconnect connections.
- MySQL, SQL Server, PostgreSQL: host, port, username, password, database, TLS, timeouts, connection options; support engine-specific authentication accurately.
- SQL Server integrated auth support must be researched for macOS and described honestly; do not claim universal support.
- SSH tunneling with private keys, agent, passphrases, verified host keys, jump hosts only if feasible; no insecure host-key bypass by default.
- Multiple concurrent connections, pooling with limits, cancellation, reconnection, idle timeout, read-only mode, Development/Staging/Production labels.
- Lazy metadata introspection and searchable explorer; handle permissions and schema-specific objects gracefully.

## Table editor: flagship feature

### Excel-like direct editing
- Virtualized grid, fast pagination/keyset navigation when possible, editable cells, row insertion/deletion, multi-cell selection, keyboard navigation, copy/paste, column resize/reorder/hide, sorting/filtering, pinned key columns.
- **Direct paste mode:** paste rectangular TSV/clipboard data into selected cells/rows; create pending edits only; highlight changed cells and show Save/Discard/Undo/Redo.
- Clipboard parser handles TSV, CSV where appropriate, quotes, embedded tabs/newlines, Unicode/Thai, empty vs NULL, dates/time zones, booleans, decimals, binary and JSON fields, preserving precision.
- Grid changes should not mutate the database on paste; save commits validated changes with a review step for bulk operations.

### Guided Smart Paste
- Modes: **INSERT**, **UPDATE**, **UPSERT**.
- Parse Excel/Google Sheets clipboard; choose whether first row is headers; preview source/destination; map/reorder columns; choose unique matching key; choose update fields; show type/constraint errors and unmatched/duplicate rows.
- UPDATE/UPSERT must use a primary key or a validated unique key (including composite keys). Refuse ambiguous matching and detect duplicate keys in pasted data.
- Explain engine-specific semantics; for SQL Server do not blindly use MERGE. Use safe, parameterized, transactional per-engine strategies with concurrency checks where feasible.
- Respect generated/identity/computed columns, defaults, foreign keys, nullable constraints, collations, decimals, time zones and driver-specific bulk limits.
- Preview exact candidate insert/update/skip/conflict counts; preview SQL or operation plan; run in bounded batches and transactions with progress, cancellation and rollback where supported.
- On errors, show affected rows, failed rows, recoverability and whether the transaction rolled back.

### Manual editing safety
- Row editing requires stable unique identification; no unsafe edits on keyless tables.
- Parameterized values and safely quoted identifiers; avoid SQL injection.
- Optimistic concurrency or equivalent safeguards against overwriting changes made elsewhere.
- Explicit transaction state and unsaved changes; ordinary edits may commit via Save, but bulk changes and production AI mutations always require review/confirmation.
- Never perform unbounded UPDATE/DELETE silently. Row count/impact warning thresholds configurable per connection.

## SQL workspace

- Monaco editor with MySQL/T-SQL/PostgreSQL dialect-aware highlighting, formatting, completion from schema, multiple query tabs, selection execution, cancellation, parameters, snippets, history, favorites and saved scripts.
- Clearly display active connection/schema, query duration, affected rows, errors, query plan when supported.
- Support multiple result sets, server-side pagination/streaming, copy, editable results only when safe, CSV/XLSX/JSON export, large result handling.
- Confirm dangerous scripts (e.g., destructive DDL, mass UPDATE/DELETE) based on risk policy; never rely on simplistic keyword matching as the sole safeguard.

## Object/schema management

- Browse/create/alter/drop supported tables, columns, indexes, constraints, foreign keys, views, procedures and functions.
- Visual table designer and relationship inspector; generate and preview engine-specific DDL.
- Explain operations unsupported by a given engine or permission set; avoid pretending all engines are identical.
- Provide data transfer/import/export, SQL dump where applicable, CSV/XLSX/JSON, schema comparison and migration planning as later milestones.

## AI Agent integration: Claude Code and Codex

### Core behavior
- Provider selector for locally installed/authenticated Claude Code CLI and Codex CLI; discover binaries, verify versions and authenticated state via supported methods.
- Do not request or store consumer account passwords. Do not assume CLI login implies access to unrelated APIs.
- Document the exact supported CLI protocol used; if unavailable, degrade gracefully rather than fabricating responses.
- Agent can inspect selected schema metadata, draft SQL, explain errors, optimize queries, compare schemas, generate migration plans, and propose multi-step database operations.
- Share only explicitly scoped schema and optionally user-selected sample rows; redact sensitive fields; show a context-sharing preview and controls.
- Treat database contents and AI output as untrusted; prevent prompt injection from data values. Limit process permissions and filesystem access.
- **Read-only tools by default.** AI may propose write operations, but must not execute INSERT/UPDATE/DELETE/DDL or restore operations without explicit user review and approval, including on non-production databases unless the user deliberately changes the policy.
- Show SQL preview, target connection/environment, impact estimate, warnings, transaction behavior, and approval UI; log user approvals and outcomes without storing secrets.
- No hidden autonomous mutation; cancellation and timeout handling; no misleading 'completed' status without DB verification.
- AI context should follow the selected tab/connection and allow explicit pinning.

## Backup and restore — manual + scheduled

- Support **manual backup**, **scheduled backup**, **backup history**, **retention policy**, **restore workflow**, **integrity verification**, and **notifications**.
- Provide daily/weekly/custom schedules, time zone, destination folder, naming, compression, optional encryption, retention count/days, failure alerts, last/next run and logs.
- Implement per-engine backup strategy based on documented, compatible utilities/APIs and server privileges:
  - MySQL: consistent logical dumps with verified utility compatibility and transaction/locking behavior.
  - PostgreSQL: pg_dump/pg_restore with version checks, formats and permissions.
  - SQL Server: server-side BACKUP DATABASE only when supported and permitted, or an explicitly documented alternative export strategy; **do not equate SQL Server backup paths on the server with local client paths**.
- Clarify what 'backup' contains (schema, data, roles, server objects, etc.) and what it does not.
- Credential access for unattended jobs must be secure; backups should not silently fail when the app is closed, asleep, or disconnected. Show scheduling limitations explicitly.
- Test restore in isolated environments; never overwrite production without explicit confirmation and a verified recovery plan.
- Backup files may contain sensitive data: restrictive permissions, optional encryption, no upload to cloud by default.

## Import/export and interoperability

- CSV, XLSX, JSON, SQL scripts; preview, mapping, encoding, delimiters, null semantics, batch import, error reports, cancellation.
- Export selected rows, filtered results, whole tables with progress; stream large files; do not freeze the UI.
- Cross-engine migration/export as a later phase with explicit type mappings and limitations.

## Public distribution and security

- macOS packaging (DMG/app), signing/notarization plan, permission prompts, safe auto-update design with signed artifacts, crash reporting opt-in, privacy policy, release notes, license review, support documentation.
- Windows later: abstraction for credential store, path handling, shell/process execution, scheduler, code signing and installers.
- Secrets in OS credential stores; encrypt sensitive backup config; do not log SQL parameters containing secrets.
- Strong TLS defaults, SSH host key verification, least privilege, parameterized queries, scoped Tauri capabilities, command invocation without shell interpolation, subprocess isolation and safe cancellation.
- No telemetry or cloud data transfer by default without informed consent. Document exactly what AI context is sent to CLI providers and their services.
- Public product must not embed personal server addresses, passwords, branding assets without rights, or local absolute paths.

## Testing and release quality

- Unit: clipboard parser, null/empty semantics, decimals, Unicode, column mapping, composite keys, SQL identifier quoting, SQL dialect operations, concurrency checks, change tracking, backup schedule calculations.
- Integration: isolated disposable MySQL, SQL Server and PostgreSQL test databases; SELECT/INSERT/UPDATE/UPSERT/DELETE, transaction rollback, TLS/SSH where testable, permissions, schema introspection, large datasets, backup/restore.
- E2E: connection wizard, multiple connections, grid paste direct and wizard, save/discard, SQL editor, AI approval flow with mocked CLI plus real opt-in CLI smoke tests, scheduled backup status.
- Never run destructive tests on production or user databases. Test dangerous workflows against disposable fixtures.
- Verify actual build, test and runtime results; report failures honestly.

## Delivery plan

**Phase 0 — Discovery and decisions:** inspect environment/repository; verify driver and CLI compatibility, licensing, security and scheduler feasibility; create concise ADRs and prioritized backlog. Do not over-plan.

**Phase 1 — Runnable foundation:** Tauri/React app, branding, premium Light and Dark themes, navigation, multi-tab workspace, connection manager shell, local settings, localization scaffolding, CI/build scripts.

**Phase 2 — Real database connectivity:** MySQL, SQL Server and PostgreSQL adapters; credentials, SSL/TLS, SSH, concurrent connections, explorer, metadata and safe read-only table browsing.

**Phase 3 — Flagship editing:** virtual grid, direct Excel paste with staged edits, Smart Paste Insert/Update/Upsert, key matching, change preview, transactions, tests.

**Phase 4 — SQL & schema:** Monaco query workspace, results, history, formatting/completion, essential object editing and import/export.

**Phase 5 — AI:** real Claude Code and Codex CLI integrations, schema context, agent tools, permission boundaries, SQL previews and approvals.

**Phase 6 — Backups:** manual and scheduled per-engine backup, retention, status, encryption options, restore testing and reliable background scheduling.

**Phase 7 — Public beta:** security hardening, onboarding, accessibility, performance, macOS signing/notarization, updater, licensing/privacy docs, reproducible release process. Windows follows after macOS stabilization.

Scope discipline: prioritize an end-to-end usable slice before broad feature completeness. Clearly label unfinished capabilities and never substitute fake data or UI-only buttons for claimed functionality.

## Deliverables and repo files

Maintain:
- README.md — product overview, setup, run/build
- REQUIREMENTS.md — product requirements and acceptance criteria
- ARCHITECTURE.md — component boundaries and data flow
- docs/adr/ — decisions and tradeoffs
- SECURITY.md — threat model, credential and AI permission handling
- DATABASE_COMPATIBILITY.md — engine/version/feature matrix
- AI_INTEGRATION.md — documented CLI interfaces, auth and limitations
- BACKUP_RESTORE.md — scheduler, formats, retention, restore and limits
- TESTING.md — local integration setup and results
- ROADMAP.md — phases, status, blockers
- LICENSES.md — dependency and distribution obligations

## Definition of done for first public macOS beta

A fresh user can install and launch PuriCat, create secure SSH/TLS-enabled connections to MySQL, SQL Server and PostgreSQL, browse multiple databases at once, open large tables efficiently, edit and paste from Excel in both direct and guided modes, review/commit safely, run SQL and export results, invoke genuinely installed Claude Code or Codex CLIs to propose SQL with explicit mutation approval, configure manual/scheduled backups with accurate operational status, switch Light/Dark modes, and understand any limitations through clear UI and documentation. All workflows must be verified with actual disposable test databases and reproducible tests.

## Immediate instructions to Claude Code

1. Inspect the current repository and macOS toolchain before changing files. If this is a new project, create a clean `puricat` repository in the current authorized workspace; do not touch unrelated projects.
2. Produce a brief architecture and feasibility report: exact framework/driver/grid/CLI/scheduler choices, licensing, risks, and a phase plan. Call out uncertain CLI integration and SQL Server backup constraints.
3. Begin **Phase 1 implementation immediately** after that concise report; do not stop at a mockup or ask unnecessary questions.
4. Make incremental, testable commits or changes; run actual build/tests and report commands and results.
5. Never access real production databases, run destructive operations, deploy publicly, or install privileged background services without explicit approval.
6. At the end of each phase, summarize implemented features, evidence of tests, remaining gaps, and the next step. Continue when safe and feasible.

**North star:** Navicat-level everyday capability + Excel-like data editing + refined white/orange macOS experience + safe, real Claude Code/Codex agents + reliable multi-engine backup.
