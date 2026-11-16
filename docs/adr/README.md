# Architecture decision records

One record per major choice. Each has: Status, Context, Decision, Alternatives considered, Consequences, Licensing.

Status values: **Accepted** (decided; the record says whether it is implemented) and **Proposed** (the intended direction; not implemented, may change when the phase starts).

| No. | Decision | Status | Implemented |
|---|---|---|---|
| [0001](0001-tauri-react-desktop-shell.md) | Tauri 2 + Rust + React/TypeScript shell | Accepted | Yes |
| [0002](0002-core-crate-and-thin-ipc.md) | Core crate without Tauri, thin IPC layer | Accepted | Yes |
| [0003](0003-database-drivers.md) | sqlx for MySQL and PostgreSQL; tiberius planned for SQL Server | Accepted (MySQL, PostgreSQL); Proposed (SQL Server) | MySQL and PostgreSQL only |
| [0004](0004-server-side-text-rendering.md) | Values rendered to text by the server on the read path | Accepted | Yes |
| [0005](0005-local-store-and-secrets.md) | SQLite settings store + OS credential store | Accepted | Yes |
| [0006](0006-data-grid.md) | Custom grid on TanStack Virtual | Accepted; performance and multi-range validation still open in Phase 3 | Browsing, editing, single-range selection, paste |
| [0007](0007-sql-editor.md) | Monaco for the SQL editor | Proposed (Phase 4) | No |
| [0008](0008-ai-cli-integration.md) | AI agents as CLI subprocesses with a PuriCat-owned MCP server | Proposed (Phase 5) | Detection only |
| [0009](0009-ssh-tunnels.md) | SSH tunnels with russh | Proposed (Phase 2b) | No |
| [0010](0010-backup-scheduler.md) | Vendor dump tools + per-user LaunchAgent helper | Proposed (Phase 6) | No |
| [0011](0011-i18n-and-theming.md) | i18next catalogs, CSS-variable tokens, bundled fonts | Accepted | Yes |
| [0012](0012-e2e-testing-on-macos.md) | Injected-script end-to-end tests in debug builds | Accepted | Yes |
| [0013](0013-staged-edits-and-optimistic-saves.md) | Staged grid edits, plan-then-apply saves, optimistic concurrency on text renderings | Accepted | MySQL and PostgreSQL |
| [0014](0014-smart-paste-match-then-write.md) | Smart Paste: match on a verified unique key in the database, preview, then plain INSERT / UPDATE in one transaction; no native upsert statements | Accepted | MySQL and PostgreSQL |

To add a record: copy the section layout of an existing one, use the next number, and add a row here. Do not rewrite an accepted record; supersede it with a new one.
