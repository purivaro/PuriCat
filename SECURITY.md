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
| Integrity of the user's data | The servers; PuriCat currently issues no writes to them |
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
| Script running in the web view escalates to the system | No shell, filesystem or HTTP capability; CSP `script-src 'self'`; all effects go through 16 typed commands | Implemented; not penetration-tested |
| SQL injection through object names or sort columns | Values are bind parameters; identifiers pass through per-engine quoting; a sort column must exist in the table's metadata | Implemented & verified (unit and live tests) |
| Unintended writes while browsing | The browse path issues only `SELECT`; optional server-enforced read-only session | Implemented & verified |
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

This is a guard rail, not a privilege boundary: a session can switch itself back with another `SET`. That is harmless today because PuriCat sends no user-supplied SQL, but once the SQL editor (Phase 4) and AI tools (Phase 5) exist, the option alone will not stop a determined statement. The strong control is a database account that lacks write privileges.

## SQL injection defences

- All values in catalogue and data queries are bind parameters.
- Identifiers cannot be bound, so every name goes through `db::ident` (backticks, double quotes or brackets doubled; empty names and NUL bytes rejected).
- Sort columns are checked against the table's metadata before they are quoted.
- Page size is limited to 1–1000; `LIMIT` and `OFFSET` are bound.

## Tauri capabilities and CSP

- Capabilities for the main window: `core:default` and `core:window:allow-start-dragging`. No shell, filesystem, HTTP, dialog or clipboard plugin is installed.
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
| Audit log of approvals and mutations | 3–5 |
| Production safeguards for edits and bulk changes | 3 |
| AI confinement and context redaction | 5 |
| Backup file permissions and encryption | 6 |
| Code signing, notarization, hardened runtime, signed updates | 7 |
| Dependency audit in CI (`cargo audit` / `cargo deny`, `pnpm audit`) | 7 |
| Generated third-party notices | 7 |
| Windows: file permissions, Credential Manager verification | After macOS beta |
| Independent security review | Before public beta |

## Reporting a vulnerability

A private reporting contact is to be published before public beta. Until then there is no public release and no supported version.
