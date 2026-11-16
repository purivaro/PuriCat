# Security

Describes the security posture of the code on 2026-11-16. PuriCat is in early development: builds are unsigned, there has been no external review, and it should not be used with production data yet.

Labels: **Implemented & verified**, **Implemented, not yet verified**, **Planned (Phase N)**. Evidence is in [TESTING.md](TESTING.md).

## Threat model

### Assets

| Asset | Where it lives |
|---|---|
| Database passwords | OS credential store only; in backend memory while a session is open |
| Database contents | The user's servers; pages of rows in app memory while a tab is open |
| Connection metadata (hosts, user names, CA paths) | `puricat.db` in the app-data directory |
| Integrity of the user's data | The servers. PuriCat writes to them only when the user saves staged grid edits (see "Write path") |
| AI CLI accounts | Owned by the CLIs; PuriCat reads only an install and sign-in flag |

### Trust boundaries

| Boundary | Trusted side | Untrusted side |
|---|---|---|
| IPC | Rust backend | Web view content |
| Database protocol | PuriCat | Server responses, object names and cell values |
| Subprocess | PuriCat | Output of spawned tools |
| Network | — | Everything between the app and a database server |
| Local machine | The logged-in OS user | Other local users |

Out of scope: an attacker who already runs code as the logged-in user or as root, and a compromised operating system.

### Threats and mitigations

| Threat | Mitigation | Status |
|---|---|---|
| Password disclosed through files, logs or the UI | Stored only in the OS credential store; no IPC command returns one; connections use typed options, not URLs, so driver errors do not carry it | Implemented & verified (Keychain round trip; MySQL auth failure does not echo the password) |
| Script running in the web view escalates to the system | No shell, filesystem or HTTP capability; CSP `script-src 'self'`; all effects go through 17 typed commands plus plain-text clipboard read and write | Implemented; not penetration-tested. Script in the web view could read the clipboard and could call `apply_changes` on an open connection |
| SQL injection through object names, sort columns or edited values | Values are bind parameters; identifiers pass through per-engine quoting; sort and edited columns must exist in the table's metadata | Implemented & verified (unit and live tests) |
| Unintended writes while browsing | The browse path issues only `SELECT`; edits are staged and written only on Save; optional server-enforced read-only session | Implemented & verified |
| A save overwrites a change made elsewhere, or touches the wrong rows | Complete-primary-key addressing, optimistic check on edited columns, exactly-one-row rule, one transaction with rollback (see "Write path") | Implemented & verified (live tests, both engines). DELETE checks the key only |
| A value is silently truncated or coerced on save | MySQL session strict mode; PostgreSQL casts to the bare type so the assignment enforces length and precision | Implemented & verified (live tests: bad number, bad enum, NOT NULL, over-long `varchar`, impossible date, malformed JSON) |
| A destructive or large save goes through unnoticed | Review dialog with the exact statements and bound values; mandatory for Production connections, deletes and saves above the row threshold | Implemented & verified for deletes (E2E); production and threshold paths not yet verified through the UI. Enforced in the UI only, not by the backend |
| Unsaved edits lost by accident | Dirty-tab confirmation, locked paging / sorting / refresh, refusal to disconnect or change a connection with dirty tabs | Implemented; only the close confirmation is covered by E2E |
| Eavesdropping or tampering on the network | TLS modes up to `verify-full` | Only the default `prefer` has been run, against local servers; the stricter modes are implemented, not yet verified. `prefer` can be downgraded |
| Other local users read connection metadata | `puricat.db` mode 0600 on Unix | Implemented; the mode is not asserted by a test. No equivalent on Windows yet |
| Malicious cell content rendered as markup | React escapes text; cells are rendered as text nodes | Implemented, not specifically tested |
| Command injection through subprocess arguments | Fixed argument vectors built in Rust; see "Subprocess policy" | Implemented & verified for detection |
| Test hooks abused in a shipped build | Hooks compiled only with `debug_assertions` | Implemented; no release build has been inspected to confirm |
| AI agent mutates data or leaks secrets | Read-only tools, approval queue, confinement | Planned (Phase 5) |
| Tampered installer or update | Signing, notarization, signed updates | Planned (Phase 7) |
| SSH man-in-the-middle | Host-key verification | Planned (Phase 2b) |

## Credential handling

- Service name `PuriCat`, key `connection/<id>/password`, through `keyring` (macOS Keychain, Windows Credential Manager).
- The password crosses IPC in one direction only, UI to backend, when the user types it. The UI later learns `hasPassword` and sends `keep`, `set` or `clear`.
- Duplicating a connection copies the password inside the backend. Deleting a connection deletes its password.
- `puricat.db` holds profiles (host, port, user name, database, TLS settings, tags) but no secrets.
- Passwords are held as ordinary strings in memory and are not zeroised.
- PuriCat never asks for, reads or stores AI account passwords or tokens.

## TLS

| Mode | Behaviour | Verified |
|---|---|---|
| `disable` | No TLS | No |
| `prefer` (default) | TLS if the server offers it; **not protected against downgrade** | Local servers: MySQL session reported encrypted, PostgreSQL reported not encrypted |
| `require` | Encrypt, do not verify the certificate | No |
| `verify-ca` | Encrypt and verify the chain | No |
| `verify-full` | Verify chain and host name (`VerifyIdentity` in MySQL) | No |

TLS is rustls with the OS root store; an extra CA file can be set per connection. The connection test reports whether the session is encrypted, as answered by the server. An open session does not display its TLS state anywhere yet. Open item: make a stricter mode the default for Production-labelled connections.

## Read-only sessions

A per-connection option runs `SET SESSION TRANSACTION READ ONLY` (MySQL) or `SET default_transaction_read_only = on` (PostgreSQL) on every pooled connection. `ServerInfo.readOnly` is the server's own answer, not the profile setting.

With the option on, the grid opens every table read-only, `apply_changes` refuses before contacting the server (it checks the saved profile), and if a write were sent anyway the server rejects it. The server-side refusal is verified by a live test on both engines.

This is a guard rail, not a privilege boundary: a session can switch itself back with another `SET`. That is harmless today because PuriCat sends no user-supplied SQL — the write path issues only generated `INSERT`, `UPDATE` and `DELETE` statements with bound values — but once the SQL editor (Phase 4) and AI tools (Phase 5) exist, the option alone will not stop a determined statement. The strong control is a database account that lacks write privileges.

## SQL injection defences

- All values in catalogue and data queries are bind parameters.
- Identifiers cannot be bound, so every name goes through `db::ident` (backticks, double quotes or brackets doubled; empty names and NUL bytes rejected).
- Sort columns are checked against the table's metadata before they are quoted.
- Page size is limited to 1–1000; `LIMIT` and `OFFSET` are bound.
- On the write path every value — new values, key values and the original values used for the concurrency check — is a bind parameter. Column names from the UI must match the table's metadata and are then quoted by `db::ident`. A unit test confirms that hostile key and cell values never appear in the SQL text.

## Write path

Staged edits are saved by `db::write` and the engine modules ([ADR 0013](docs/adr/0013-staged-edits-and-optimistic-saves.md)). These rules are enforced in the backend, independent of the UI, unless noted.

| Defence | Detail | Status |
|---|---|---|
| Parameters only | Values travel as text parameters; none is interpolated into SQL | Implemented & verified |
| Identifier quoting | Database, schema, table and column names go through `db::ident`; unknown, generated, binary and spatial columns are refused before any SQL is sent | Implemented & verified |
| Complete-key addressing | Every UPDATE and DELETE names the full primary key with non-NULL values; a partial key, a missing key column or a keyless table is refused | Implemented & verified |
| Optimistic concurrency | Each UPDATE also requires every edited column to still render to the text the user saw; otherwise the save fails with a `conflict` error | Implemented & verified |
| Exactly one row | A statement that affects zero rows or more than one row aborts the save | Implemented & verified |
| One transaction | Deletes, then updates, then inserts, in a single transaction; any error or conflict rolls everything back, including statements that had succeeded | Implemented & verified |
| Size limit | At most 10,000 row changes per save | Implemented, not yet verified (no test exercises the limit) |
| Strict input | MySQL sessions add `STRICT_ALL_TABLES`; PostgreSQL parameters are cast to the column's bare type so the assignment enforces length and precision | Implemented & verified |
| What is reviewed is what runs | The review dialog's plan and the executed plan come from the same function. The plan is rebuilt at save time from the same change set and fresh metadata, not cached | Implemented; the equality of the two plans is by construction, not asserted by a test |
| Review policy | UI-enforced: review is mandatory for Production-labelled connections, any delete, and more touched rows than `bulkReviewRows` (default 10, set per connection) | Implemented & verified for deletes only |
| Read-only profile | `apply_changes` refuses when the saved profile is read-only | Implemented, not yet verified (the server-side refusal is verified) |

Data-integrity limitations to know about:

- **DELETE checks the key only.** A row that was changed elsewhere after it was loaded, but still exists, is deleted without a conflict. Only a row that is already gone is reported.
- **The concurrency check covers edited columns only.** A concurrent change to another column of the same row is not detected, and is not overwritten either, because only edited columns are written.
- **The comparison is on the server's text rendering.** Types whose text form depends on session settings compare correctly only while those settings match the session that loaded the page; PuriCat's sessions are configured identically, so this holds within one connection profile.
- **PostgreSQL sessions run in UTC.** The driver (sqlx 0.8) always sends `TimeZone=UTC` at start-up. `timestamptz` values are therefore shown in UTC with an explicit `+00` offset (unambiguous), a `timestamptz` typed without an offset is read as UTC, and `now()`-based defaults of `timestamp without time zone` columns — including those filled in for a row inserted through the grid — are computed in UTC rather than in the server's configured zone. Not fixed; a per-connection session time zone is Planned (Phase 2b). MySQL sessions keep the server's time zone (verified by a live test).
- **No client-side validation.** Invalid input is rejected by the server at Save, not while typing.
- **No audit log.** Saves are not recorded anywhere by PuriCat.
- **Policy lives in the UI.** The review step and the keyless-table rule (the backend would accept INSERT on a keyless table) are not enforced by the IPC layer.

## Tauri capabilities and CSP

- Capabilities for the main window: `core:default`, `core:window:allow-start-dragging`, `clipboard-manager:allow-read-text` and `clipboard-manager:allow-write-text`. The clipboard permissions belong to the official `tauri-plugin-clipboard-manager` and cover plain text only (no images, HTML or clearing); they exist for copy and paste in the grid. No shell, filesystem, HTTP or dialog plugin is installed.
- Reading the clipboard needs no user gesture or prompt, so any script running in the web view could read it. The CSP and the absence of remote content are what guard this.
- Production CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src ipc: http://ipc.localhost`. `'unsafe-inline'` is allowed for styles only.
- The development CSP (`devCsp`) is deliberately loose for the Vite dev server and applies to `pnpm tauri dev` only.
- Fonts are bundled; the app loads nothing from the network.

## Subprocess policy

Subprocesses are started from Rust, never from the web view, with stdin closed, an 8-second timeout and kill-on-drop.

| Command | Purpose |
|---|---|
| `claude --version`, `claude auth status --json` | Detection; only `loggedIn` is read, other fields are discarded |
| `codex --version`, `codex login status` | Detection; only whether the text says "logged in" |
| `$SHELL -lic "command -v claude"` (or `codex`) | Fallback when the binary is not in `PATH` or the usual per-user directories |

Two points to be aware of:

- The fallback does use a shell. The command string is a constant plus a hard-coded binary name, so nothing untrusted is interpolated, but an interactive login shell runs the user's own start-up files.
- The binary that is executed is whichever `claude` or `codex` is found first, as in any terminal. PuriCat does not verify its origin.

## AI permission model — Planned (Phase 5)

Not implemented. The design ([ADR 0008](docs/adr/0008-ai-cli-integration.md), [AI_INTEGRATION.md](AI_INTEGRATION.md)):

- read-only database tools by default, served by a PuriCat-owned MCP server;
- writes are returned as proposals, shown with SQL, target and impact, and executed by PuriCat only after approval;
- the CLI's own tools and sandbox are restricted and it runs in an empty directory;
- no secrets in prompts, arguments or environment; database content and model output are untrusted;
- approvals and outcomes are logged without secrets.

## Debug-only test hooks

| Variable | Effect |
|---|---|
| `PURICAT_DATA_DIR` | Use this directory instead of the app-data directory |
| `PURICAT_SECRET_STORE=memory` | Keep passwords in process memory instead of the OS credential store |
| `PURICAT_E2E_SCRIPT` | Evaluate this file in the web view after each page load |
| `PURICAT_E2E_REPORT` | File that the `e2e_report` command writes |

They exist so tests never touch real settings or the Keychain ([ADR 0012](docs/adr/0012-e2e-testing-on-macos.md)). All four are behind `#[cfg(debug_assertions)]` and are absent from release builds. The `e2e_report` command remains registered in release builds but only returns an error. Debug builds must not be distributed.

## Not yet done

| Item | Phase |
|---|---|
| SSH tunnels and host-key verification | 2b |
| TLS verification tested against a TLS-enforcing server; stricter default for production | 2b |
| Audit log of saves, approvals and mutations | 3–5 |
| Verification of the production and bulk-threshold review paths through the UI | 3 |
| Client-side validation before save | 3 |
| Per-connection session time zone (PostgreSQL) | 2b |
| Row-content check on DELETE; backend enforcement of the review policy | Not scheduled |
| AI confinement and context redaction | 5 |
| Backup file permissions and encryption | 6 |
| Code signing, notarization, hardened runtime, signed updates | 7 |
| Dependency audit in CI (`cargo audit` / `cargo deny`, `pnpm audit`) | 7 |
| Generated third-party notices | 7 |
| Windows: file permissions, Credential Manager verification | After macOS beta |
| Independent security review | Before public beta |

## Reporting a vulnerability

A private reporting contact is to be published before public beta. Until then there is no public release and no supported version.
