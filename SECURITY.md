# Security

Describes the security posture of the code on 2026-11-16. PuriCat is in early development: builds are unsigned, there has been no external review, and it should not be used with production data yet.

Labels: **Implemented & verified**, **Implemented, not yet verified**, **Planned (Phase N)**. Evidence is in [TESTING.md](TESTING.md).

## Threat model

### Assets

| Asset | Where it lives |
|---|---|
| Database passwords | OS credential store only; in backend memory while a session is open |
| Database contents | The user's servers; pages of rows in app memory while a tab is open; the full rows of a query tab's last run in backend memory (up to 100,000 rows per result); files the user exports |
| Connection metadata (hosts, user names, CA paths) | `puricat.db` in the app-data directory |
| SQL the user wrote | `puricat.db`: the text of open query tabs and the history of runs (see "SQL console") |
| Integrity of the user's data | The servers. PuriCat writes to them only when the user saves staged grid edits, runs a previewed Smart Paste (see "Write path"), or runs SQL in a query tab — typed there, or placed there from an AI answer (see "SQL console", "AI chat"). The AI itself writes to nothing |
| AI CLI accounts | Owned by the CLIs. PuriCat reads only an install and sign-in flag, and starts the CLI under the user's account for each chat turn; it never sees a token |
| What is sent to an AI provider | The user's chat messages, earlier answers and, when sharing is on, the schema context and the SQL of the active query tab (see "AI chat"). The conversation itself lives in web view memory only |

### Trust boundaries

| Boundary | Trusted side | Untrusted side |
|---|---|---|
| IPC | Rust backend | Web view content |
| Database protocol | PuriCat | Server responses, object names and cell values |
| Subprocess | PuriCat | Output of spawned tools, including every answer a model writes |
| AI provider | — | The CLI's provider service: what a chat turn sends leaves the machine under the user's own account and that provider's terms |
| Network | — | Everything between the app and a database server |
| Local machine | The logged-in OS user | Other local users |

Out of scope: an attacker who already runs code as the logged-in user or as root, and a compromised operating system.

### Threats and mitigations

| Threat | Mitigation | Status |
|---|---|---|
| Password disclosed through files, logs or the UI | Stored only in the OS credential store; no IPC command returns one; connections use typed options, not URLs, so driver errors do not carry it | Implemented & verified (Keychain round trip; MySQL auth failure does not echo the password) |
| Script running in the web view escalates to the system | No shell, filesystem or HTTP capability; CSP `script-src 'self'`; all effects go through 32 typed commands plus plain-text clipboard read and write and the native Save dialog | Implemented; not penetration-tested. Script in the web view could read the clipboard, could call `apply_changes` or `apply_paste` on an open connection, could run any SQL on an open connection through `run_sql`, could make the backend write an export file to any path the OS user may write (`export_result`), and could send any text to an AI provider under the user's account through `agent_send` — with Codex, whose sandboxed shell tool can read local files, that is a way to read files the web view cannot reach itself (see "AI chat") |
| SQL injection through object names, sort columns, edited or pasted values | Values are bind parameters, including pasted keys in Smart Paste lookups; identifiers pass through per-engine quoting; sort, edited, mapped and key columns must exist in the table's metadata | Implemented & verified (unit and live tests) |
| Unintended writes while browsing | The browse path issues only `SELECT`; edits are staged and written only on Save; the Smart Paste preview issues only `SELECT`; optional server-enforced read-only session | Implemented & verified |
| A typed statement changes far more rows than intended | Impact guard: a row-changing statement runs in a guard transaction and is rolled back when it affected more rows than the connection's threshold (see "SQL console") | Implemented & verified (live tests, both engines; E2E on MySQL). Covers statements classified as row-changing only; the threshold is supplied by the UI |
| A destructive statement runs unnoticed | Confirmation listing `UPDATE` / `DELETE` without `WHERE`, `DROP`, `TRUNCATE`, `ALTER ... DROP`; every non-read statement on a Production-labelled connection | Implemented & verified for `DELETE` without `WHERE` (E2E, MySQL); the Production path has no automated test. UI only; reads the statement text |
| A runaway query, or a result that exhausts memory | Stop interrupts the statement on the server; a row cap per result (at most 100,000) stops a plain read | Implemented & verified in the backend (live tests, both engines); Stop is not exercised through the UI. Memory use at the cap has not been measured |
| Passwords typed in SQL end up in the local history | Password literals, in single or double quotes, are masked before a script is stored; a connection's history is deleted with the connection | Implemented & verified for the masking function (unit test). A heuristic; everything else in a script is stored as typed. The deletion has no test |
| A pasted block updates or duplicates the wrong rows | Rows are matched only on a catalogue-verified unique key chosen by the user; equality is evaluated by the database; duplicates, missing keys and malformed key values block the run; the outcome is previewed per row and re-checked per row at run time; one transaction (see "Smart Paste") | Implemented & verified in the backend for single-column and composite keys (live tests, both engines). Through the UI: UPSERT on a single-column key on MySQL only |
| A save overwrites a change made elsewhere, or touches the wrong rows | Complete-primary-key addressing, optimistic check on edited columns, exactly-one-row rule, one transaction with rollback (see "Write path") | Implemented & verified (live tests, both engines). DELETE checks the key only |
| A value is silently truncated or coerced on save | MySQL session strict mode; PostgreSQL casts to the bare type so the assignment enforces length and precision | Implemented & verified (live tests: bad number, bad enum, NOT NULL, over-long `varchar`, impossible date, malformed JSON) |
| A destructive or large save goes through unnoticed | Review dialog with the exact statements and bound values; mandatory for Production connections, deletes and saves above the row threshold | Implemented & verified for deletes (E2E); production and threshold paths not yet verified through the UI. Enforced in the UI only, not by the backend |
| Unsaved edits lost by accident | Dirty-tab confirmation, locked paging / sorting / refresh, refusal to disconnect or change a connection with dirty tabs | Implemented; only the close confirmation is covered by E2E |
| Eavesdropping or tampering on the network | TLS modes up to `verify-full` | Only the default `prefer` has been run, against local servers; the stricter modes are implemented, not yet verified. `prefer` can be downgraded |
| Other local users read connection metadata | `puricat.db` mode 0600 on Unix | Implemented; the mode is not asserted by a test. No equivalent on Windows yet |
| Malicious cell content rendered as markup | React escapes text; cells are rendered as text nodes | Implemented, not specifically tested |
| Command injection through subprocess arguments | Fixed argument vectors built in Rust, no shell; the chat's prompt travels on stdin; the one user-influenced argument, the model name, is validated so it cannot be read as an option; see "Subprocess policy" | Implemented & verified for detection and for the chat (unit tests: the vectors, the model-name rule, the prompt on stdin and absent from the arguments; E2E on stand-in CLIs) |
| Test hooks abused in a shipped build | Hooks compiled only with `debug_assertions` | Implemented; no release build has been inspected to confirm |
| The AI reads or changes data on its own | The model is given no tools and no database access: it only writes text. SQL from an answer reaches a database only when the user opens it in a query tab and runs it, under the console's safeguards (see "AI chat") | Implemented & verified for what PuriCat passes to the CLI (unit tests; E2E on stand-ins: `--tools ""`, `--strict-mcp-config`, Codex's `read-only` sandbox) and for "Open in query tab" running nothing (E2E). That the real Claude CLI then offers the model no tools is seen in a captured `system/init` line, not asserted by a test. Codex keeps a sandboxed shell tool |
| Secrets or data leak to the AI provider | The shared context is names and types only, rendered by the backend: no rows, defaults, hosts, ports, user names, passwords or connection name. The user can read it before sending and with every sent message, and can switch sharing off | Implemented & verified (unit test of the exact context text; E2E: the inspector's text, and the stand-ins' stdin holding neither the host nor the connection name). Schema names, the active query's text and whatever the user types are sent as they are |
| Text in the database steers the model (prompt injection) | The context is fenced and declared reference data; a closing tag inside it is neutralised; the model has no tools, so a manipulated answer is still only text | Implemented; the fence is unit-tested. No test measures how a model reacts. The user must read SQL before running it |
| An answer is rendered as markup or script | Answers go through a Markdown reader into React elements; nothing is inserted as HTML | Implemented & verified (Vitest: markup-looking text stays text; E2E: a streamed answer with a code block, bold and inline code) |
| An AI agent with database tools mutates data | Read-only tools, approval queue, audit log | Planned (Phase 5) |
| Tampered installer or update | Signing, notarization, signed updates | Planned (Phase 7) |
| SSH man-in-the-middle | Host-key verification | Planned (Phase 2b) |

## Credential handling

- Service name `PuriCat`, key `connection/<id>/password`, through `keyring` (macOS Keychain, Windows Credential Manager).
- The password crosses IPC in one direction only, UI to backend, when the user types it. The UI later learns `hasPassword` and sends `keep`, `set` or `clear`.
- Duplicating a connection copies the password inside the backend. Deleting a connection deletes its password.
- `puricat.db` holds profiles (host, port, user name, database, TLS settings, tags) but no secrets.
- Passwords are held as ordinary strings in memory and are not zeroised.
- PuriCat never asks for, reads or stores AI account passwords or tokens. A chat turn starts the CLI with PuriCat's own environment (plus the tool's directory on `PATH`); PuriCat puts no database credential in the CLI's arguments, input or environment.

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

With the option on, the grid opens every table read-only and does not offer Smart Paste, `apply_changes` and `apply_paste` refuse before contacting the server (they check the saved profile), and if a write were sent anyway the server rejects it. The server-side refusal is verified by a live test on both engines.

This is a guard rail, not a privilege boundary: a session can switch itself back with another `SET`. On the browse, save and Smart Paste paths that cannot happen, because PuriCat sends no user-supplied SQL there — only generated statements with bound values.

**In a query tab it can happen.** The tab's connection is acquired through the same pool, so it starts read-only and the server refuses a write (live-tested on MySQL; the PostgreSQL console was not run read-only). But the person typing can run `SET SESSION TRANSACTION READ WRITE` (MySQL) or `SET default_transaction_read_only = off` (PostgreSQL), or open a `READ WRITE` transaction, and then write. PuriCat does not filter such statements, and `run_sql` does not check the profile's read-only flag as `apply_changes` and `apply_paste` do. So in the SQL console the read-only option protects against accidents, not against the person at the keyboard — and it will not hold back the AI tools planned for Phase 5 either. The switching statements themselves have not been run by a test; this follows from the engines' documented behaviour.

The strong control is a database account that lacks write privileges.

## SQL injection defences

- All values in catalogue and data queries are bind parameters.
- Identifiers cannot be bound, so every name goes through `db::ident` (backticks, double quotes or brackets doubled; empty names and NUL bytes rejected).
- Sort columns are checked against the table's metadata before they are quoted.
- Page size is limited to 1–1000; `LIMIT` and `OFFSET` are bound.
- On the write path every value — new values, key values and the original values used for the concurrency check — is a bind parameter. Column names from the UI must match the table's metadata and are then quoted by `db::ident`. A unit test confirms that hostile key and cell values never appear in the SQL text.
- Smart Paste lookups bind every pasted key value; the mapped, key and update column names must match the table's metadata and are quoted by `db::ident`. A unit test asserts the lookup's shape: one typed placeholder per key value, the values in the parameter list.
- The SQL console is different by nature: it sends the SQL a person typed, as typed. What PuriCat adds around it is fixed text or safely built: `USE` with the database name quoted by `db::ident`, the guard's `START TRANSACTION` / `BEGIN` / `COMMIT` / `ROLLBACK`, `KILL QUERY` with the numeric connection id the server reported, and `pg_cancel_backend` with the backend pid as a bind parameter.

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

### Smart Paste

Smart Paste ([ADR 0014](docs/adr/0014-smart-paste-match-then-write.md)) writes through the same planner and executor as grid saves, so the backend rules of the table above (parameters only, identifier quoting, complete-key addressing, optimistic concurrency, exactly one row, one transaction, strict input) apply to it, with the chosen key in place of the primary key. The grid's review dialog and row threshold do not; the preview takes their place. What it adds, enforced in the backend unless noted:

| Defence | Detail | Status |
|---|---|---|
| Verified matching key | UPDATE and UPSERT need a key; the key must be the primary key or a plain unique index read from the catalogue at analysis time, and every key column must be mapped. Unique indexes over a column prefix or an expression (MySQL) and partial or expression unique indexes (PostgreSQL) do not qualify. Anything else is refused, whatever the UI sent | Implemented & verified (unit and live tests; the MySQL exclusion of expression indexes has no test) |
| Database-evaluated equality | Pasted keys are compared with stored keys inside parameterised lookup queries, so type and collation rules are the server's. PuriCat never decides that two keys are equal by comparing text, except to flag exact-text repeats within the paste | Implemented & verified (case-insensitive key on MySQL, case-sensitive on PostgreSQL) |
| Well-formed key values | A key value for an integer column must be a whole number: `12abc`, `1.9`, `1e2` and ` 1` are invalid rows. Other key columns get the value pre-checks (number shape, declared length). Such rows are never sent to the database, so MySQL's lenient `CAST('12abc' AS SIGNED)` cannot match row 12 | Implemented & verified (unit and live tests, both engines). Key values of other types that the database coerces (dates, for example) are not pre-validated beyond those checks |
| No ambiguous rows | A pasted row without a key value, two pasted rows with the same key, a pasted key matching more than one existing row, and (INSERT with a key) a key that already exists are problems; any problem blocks the whole run | Implemented & verified. Two spellings of one key are duplicates when both reach the same existing row, in one 40-row lookup (unit and live tests) or in different lookups (unit test with simulated answers) |
| Read-only preview | `analyze_paste` issues only catalogue queries and `SELECT`s | Implemented & verified (row count unchanged after a preview) |
| Re-analysis before the run | `apply_paste` analyses the table again and refuses with a `conflict` error unless it reproduces the fingerprint of the preview the user approved. The fingerprint covers every row's status and changed columns (not the stored values, which the optimistic check guards), so a change that leaves the counts equal is caught | Implemented & verified (live tests, both engines: re-running an approved paste is refused; so is an outcome with the same counts but different rows) |
| Optimistic concurrency | UPDATEs address the row by the key as the database rendered it and require every changed column to still hold the value seen by the re-analysis | Implemented & verified through the shared planner; no Smart Paste test changes a row between analysis and write |
| One transaction, exactly one row per statement | A driver error, a wrong row count or a cancel rolls back everything, including statements that had succeeded | Implemented & verified (an impossible date after a valid row; cancel after 50 of 130 statements) |
| Cancellation | "Stop and roll back" sets a flag that is read before each statement. While a run is in flight the dialog cannot be dismissed (Esc, outside click and the close button are blocked), so the outcome is always shown | Implemented & verified for the executor's stop callback (live test); the flag itself, `cancel_job`, the button and the blocked dismissal are not exercised by a test |
| Size limit | At most 10,000 pasted rows per run | Implemented, not yet verified |
| Generated keys are never written | An auto-increment or identity key column may be used to match; its pasted value is never inserted or updated. In INSERT mode a pasted generated key that is not found is an invalid row, not a row inserted under another id; a blank key inserts | Implemented & verified |
| Read-only profile | `apply_paste` refuses when the saved profile is read-only; the UI does not show the button on a read-only tab | Implemented, not yet verified |
| Production connections | UI only: a red warning and a danger-styled Run button. No typed confirmation, and the grid's row threshold does not apply; the preview is always required | Implemented, not yet verified |

Smart Paste limitations:

- **The preview requirement is a UI rule plus a fingerprint check.** The backend cannot tell whether a human saw the preview; it only requires the caller to state the fingerprint a fresh analysis produces, which `analyze_paste` returns.
- **The re-analysis is not in the write transaction.** Its lookups run on pooled connections just before the transaction starts. A change made in between is caught by the optimistic check (updates) or by the unique index (inserts), and the run rolls back; a row classified as unchanged or unmatched is simply not touched.
- **"Update" is a text comparison.** A pasted `1.5` against a stored `1.500000` is counted and run as an update that stores the same number.
- **Key values of non-integer types are pre-checked only generically.** Integer keys must be whole numbers (see the table). A key value of another type that the database coerces, such as a date, gets only the number-shape and length checks and is then cast and compared by the server; a malformed one has not been tested on either engine.
- **The SQL statements are not shown.** The preview lists the operation per row and the changed columns, not the statement text that the grid's review dialog shows.
- **A stop request that arrives after the last statement has started does not prevent the commit.**

Data-integrity limitations to know about:

- **DELETE checks the key only.** A row that was changed elsewhere after it was loaded, but still exists, is deleted without a conflict. Only a row that is already gone is reported.
- **The concurrency check covers edited columns only.** A concurrent change to another column of the same row is not detected, and is not overwritten either, because only edited columns are written.
- **The comparison is on the server's text rendering.** Types whose text form depends on session settings compare correctly only while those settings match the session that loaded the page; PuriCat's sessions are configured identically, so this holds within one connection profile.
- **PostgreSQL sessions run in UTC.** The driver (sqlx 0.8) always sends `TimeZone=UTC` at start-up. `timestamptz` values are therefore shown in UTC with an explicit `+00` offset (unambiguous), a `timestamptz` typed without an offset is read as UTC, and `now()`-based defaults of `timestamp without time zone` columns — including those filled in for a row inserted through the grid — are computed in UTC rather than in the server's configured zone. Not fixed; a per-connection session time zone is Planned (Phase 2b). MySQL sessions keep the server's time zone (verified by a live test).
- **No client-side validation in the grid.** Invalid input is rejected by the server at Save, not while typing. Smart Paste adds a few cheap pre-checks; the server remains the authority there too.
- **No audit log.** Saves and Smart Paste runs are not recorded anywhere by PuriCat. The SQL history is a convenience, not an audit log: it covers query tabs only, keeps 1,000 entries, can be cleared, and a failure to write it is ignored.
- **Policy lives in the UI.** The review step and the keyless-table rule (the backend would accept INSERT on a keyless table) are not enforced by the IPC layer.

## SQL console

Query tabs run SQL the user typed ([ADR 0015](docs/adr/0015-sql-console.md)). The rules of "Write path" do not apply here: there are no generated statements, no key addressing and no optimistic check. What stands in their place:

| Defence | Detail | Status |
|---|---|---|
| A connection of its own per tab, set up like the pool's | Acquired through the session's pool, so MySQL strict mode and the read-only statement have run on it, then detached. Session state and transactions stay within the tab | Implemented & verified (live tests: session state on both engines; a read-only session refusing a write and six open consoles next to a working session on MySQL) |
| Splitting and classification in the backend | A dialect-aware scanner (`db::script`) finds statement boundaries and classifies each statement as `read`, `dml`, `ddl`, `transaction` or `other`; keywords inside strings, comments, quoted identifiers and dollar-quoted bodies do not count | Implemented & verified (unit tests) |
| Confirmation before running | UI: statements with a danger reason (`UPDATE` / `DELETE` without a top-level `WHERE`, `DROP`, `TRUNCATE`, `ALTER ... DROP`) are listed and must be confirmed; on a Production-labelled connection every statement that is not `read` is listed | Implemented & verified for a `DELETE` without `WHERE`, cancelled (E2E, MySQL). Confirming, and the Production path, have no automated test |
| Impact guard | Backend: a `dml` statement outside the user's own transaction runs in a guard transaction and is rolled back, status `blocked`, when the server reports more affected rows than the limit. The UI sends the connection's `bulkReviewRows` (default 10) | Implemented & verified (live tests, both engines: blocked with nothing changed, committed when within the limit or when the limit is lifted; E2E on MySQL) |
| The guard never touches the user's transaction | After the user's `BEGIN` / `START TRANSACTION`, or with MySQL autocommit off, no guard transaction is added (on MySQL a `START TRANSACTION` would commit the user's work) | Implemented & verified (live tests: a transaction spanning runs on both engines, also with a comment in front of `BEGIN`; autocommit off on MySQL) |
| Row cap | Backend: `maxRows` must be 1–100,000. A plain read is cancelled on the server at the cap. A statement that writes, and any statement inside the user's PostgreSQL transaction, is read to its end instead, so a cap never undoes a write or aborts a transaction | Implemented & verified (live tests: capped read with the console still usable on both engines; capped `UPDATE ... RETURNING` committed and a capped read inside a transaction on PostgreSQL) |
| Stop | The statement is interrupted on the server from another pooled connection; later statements are not run. A guarded statement is rolled back | Implemented & verified in the backend (live tests, both engines). The `cancel_sql` command and the Stop button are not exercised by a test |
| First error stops the script | Later statements are not sent | Implemented & verified (live tests; E2E on MySQL) |
| Open-transaction guards | UI: badge, dot on the tab, database selector disabled, confirmation before closing, refusal to disconnect, change or delete the connection | Implemented & verified for the badge, the dot, the selector and the close confirmation (E2E, MySQL); "Roll back and close" and the refusals have no automated test |
| Nothing keeps running unseen | Closing a query tab or closing its session interrupts the tab's running statement before the connection is dropped | Implemented, not yet verified |
| Lost connection | A connection that ended is reported as such, never reused, and replaced on the next run | Implemented & verified in the backend (live tests, both engines: the server kills the connection) |
| Masked history | `script::redact` masks quoted literals — single quotes, or the double quotes MySQL also accepts — that follow `password`, `identified by` or `identified with` before a script is stored; a quoted column named `"password"` does not shield the value after it | Implemented & verified for the function (unit test, both kinds of quote and the quoted name); the `run_sql` path that calls it is not asserted |
| History goes with the connection | `delete_connection` deletes the connection's history entries before the profile | Implemented, not yet verified (the store's `clear_history` is unit-tested; the command is not) |
| Exports written by the backend | The web view gets a path from the native Save dialog and passes it to `export_result`; it has no filesystem permission | Implemented & verified (unit tests, live CSV, E2E through the command). The dialog itself is not exercised |

SQL console limitations:

- **The statement classification is a convenience, not a boundary.** It reads keywords. A function called from a `SELECT` can write and is classified `read`: not confirmed, not guarded, and cancelled at the row cap (which rolls it back). `CALL`, `DO`, DDL and every `other` statement run unguarded.
- **DDL is not guarded, and MySQL DDL cannot be rolled back.** A confirmed `DROP` is final.
- **The guard measures the server's affected-row count.** Rows changed by triggers and foreign-key cascades are not counted. A statement whose reported count is not the number of rows it changed (`EXPLAIN ANALYZE UPDATE`, a data-modifying `WITH` ending in a `SELECT`) is judged by that count. Not tested.
- **The confirmation and the threshold are supplied by the UI.** `analyze_sql` only reports; `run_sql` runs the text it receives with the `changeLimit` it receives, and no limit is a valid value. The backend enforces the mechanism, not the policy.
- **"Run it without the limit" lifts the limit for the blocked statement and for every statement after it in that script**, not only for the one that was shown as blocked.
- **Transaction tracking reads the statement text.** A transaction opened inside a stored procedure or with `XA` is not seen, so the guard may send its own `START TRANSACTION` / `BEGIN` for a later statement; on MySQL that implicitly commits a transaction a procedure left open. A MySQL error that rolls back the whole transaction (a deadlock) is not noticed, so the badge stays and later statements run unguarded in autocommit. A successful MySQL DDL statement is taken as an implicit commit unless it contains the word `TEMPORARY` outside strings, comments and quoted identifiers (`CREATE TEMPORARY TABLE`, `DROP TEMPORARY TABLE`). That is a keyword test: DDL on an unquoted table or column that is itself named `temporary` is read the same way, and the badge then stays although the server committed. The rule has a unit test for the keyword test only; MySQL DDL inside a transaction is not run by any test.
- **Read-only is inherited, not enforced against SQL** (see "Read-only sessions").
- **History and query text are stored in clear.** `puricat.db` holds the text of every open query tab exactly as typed (not masked) and up to 1,000 history entries with only password literals masked. The mask is a heuristic: the first quoted literal (single or double quotes) within 48 characters of the keyword, and it can hide an unrelated literal (`WHERE password_hint = '...'`) or, on PostgreSQL, a double-quoted identifier. It pairs the first quote character it meets after the keyword with the next one of the same kind, so where the word `password` is itself a double-quoted identifier (`SET "password" = '...'`) the identifier's closing quote is taken as the opening one: the literal is then masked only together with everything up to the next double quote, and not at all when no double quote follows. That case follows from reading the code and has no test. Keys, tokens or personal data written into a script are stored as typed. The file is mode 0600 on Unix. Deleting a connection deletes its history entries; there is no setting to turn history off.
- **`export_result` takes its path from the web view.** The backend creates or overwrites that file with the result's contents, with the OS user's rights and default permissions. The Save dialog is the only caller in the UI; asking before an existing file is replaced is left to the operating system's dialog. Exported files contain database contents and are the user's to protect.
- **Results are held in memory**: the full rows of each tab's last run, up to 100,000 rows per result, until the next run or until the tab closes.
- **Stop depends on a free pooled connection** to carry the interrupt.

## Tauri capabilities and CSP

- Capabilities for the main window: `core:default`, `core:window:allow-start-dragging`, `clipboard-manager:allow-read-text`, `clipboard-manager:allow-write-text` and `dialog:allow-save`. The clipboard permissions belong to the official `tauri-plugin-clipboard-manager` and cover plain text only (no images, HTML or clearing); they exist for copy and paste in the grid, for Smart Paste, which reads the clipboard when its dialog opens and on "Read clipboard", and for "Copy" on a code block of an AI answer (write only).
- `dialog:allow-save` belongs to the official `tauri-plugin-dialog` and allows one thing: showing the native Save dialog and receiving the chosen path. The Open, message and confirm dialogs are not granted. The export file is written by the backend (`export_result`), not by the web view.
- No shell, filesystem or HTTP plugin is registered and no such permission is granted. The dialog plugin pulls the `tauri-plugin-fs` crate in as a dependency, so its code is compiled into the binary, but the plugin is not registered in `src-tauri/src/lib.rs` and the web view cannot call it.
- Reading the clipboard needs no user gesture or prompt, so any script running in the web view could read it. The CSP and the absence of remote content are what guard this.
- Production CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src ipc: http://ipc.localhost`. `'unsafe-inline'` is allowed for styles only. The SQL editor did not change it: Monaco's script, worker, stylesheet and icon font are bundled and served from the app itself. The end-to-end test records any CSP violation and fails on it; it passes against the bundled app, including the agent chat stages ([TESTING.md](TESTING.md)).
- The development CSP (`devCsp`) is deliberately loose for the Vite dev server and applies to `pnpm tauri dev` only.
- Fonts are bundled; the app loads nothing from the network. The agent chat added no capability and no CSP change: the web view reaches the CLIs only through the `agent_*` commands, and it is the CLI process, not the app, that contacts the AI provider.

## Subprocess policy

Subprocesses are started from Rust, never from the web view, with a fixed argument vector. All but the `kill` helper have a timeout and kill-on-drop.

| Command | Purpose | stdin | Timeout |
|---|---|---|---|
| `claude --version`, `claude auth status --json` | Detection; only `loggedIn` is read, other fields are discarded | Closed | 8 s |
| `codex --version`, `codex login status` | Detection; only whether the text says "logged in" | Closed | 8 s |
| `$SHELL -lic "command -v claude"` (or `codex`) | Fallback when the binary is not in `PATH` or the usual per-user directories; also tried at the start of a chat turn | Closed | 8 s |
| `codex debug models` | The list for the chat's model selector; only the name, display name, description and visibility of each entry are read | Closed | 15 s |
| `claude -p …` / `codex exec …` (the vectors are in [AI_INTEGRATION.md](AI_INTEGRATION.md)) | One chat turn, in a process group of its own on Unix | The prompt, then closed | 10 min; Stop kills it. After its verdict it has 5 s to exit |
| `/bin/kill -KILL -<pid>` (Unix) | Ends a chat turn's whole process group on Stop, on timeout, or when the CLI does not exit after its verdict. `<pid>` is the process id of the CLI PuriCat started | Closed | None |

Points to be aware of:

- The fallback does use a shell. The command string is a constant plus a hard-coded binary name, so nothing untrusted is interpolated, but an interactive login shell runs the user's own start-up files.
- The binary that is executed is whichever `claude` or `codex` is found first, as in any terminal. PuriCat does not verify its origin. Since the chat, that binary also receives the prompt.
- A chat turn runs in `agent-scratch` inside the app-data directory, which PuriCat keeps empty, and inherits PuriCat's environment with the tool's directory prepended to `PATH`.
- The only argument a user can influence is the chat's model name: 1–100 characters out of ASCII letters, digits and `. _ : / @ - [ ]`, not starting with `-`, checked in the backend before the process is started.
- stderr of a chat turn is read; its last non-empty line (at most 500 characters) is shown to the user when the CLI fails without a verdict in its stream.

## AI chat

The agent chat sends text to the user's Claude Code or Codex CLI and shows the text that comes back ([ADR 0016](docs/adr/0016-agent-chat-without-tools.md), [AI_INTEGRATION.md](AI_INTEGRATION.md)). The model is given **no tools and no database access**. There is no MCP server, no query tool, no approval queue and no audit log; those are the planned next step (below).

| Defence | Detail | Status |
|---|---|---|
| The model cannot act | Claude Code is started with `--tools ""` and `--strict-mcp-config`, plus options meant to keep the user's own customisations and slash commands out. Codex is started with `--sandbox read-only`, `--ignore-user-config` and `--ignore-rules` | Implemented & verified for the argument vectors (unit tests; E2E reads what the stand-in CLIs were given). Behaviour of the real CLIs: the opt-in test against Claude Code 2.1.303 returned an answer; the tool list it reported is seen in a captured line, not asserted. Codex keeps its shell tool |
| The user is the only path to a database | SQL in an answer is a code block with "Copy" and "Open in query tab". The latter opens a new query tab with the text in the editor and runs nothing; the SQL console's confirmation, impact guard and row cap apply from there | Implemented & verified (E2E: the new tab holds exactly the SQL and no run happened). The safeguards themselves are those of "SQL console", with their limits |
| Nothing but names and types in the context | Rendered in the backend from the catalogue: engine and version, database and schema, tables and views with column names, types, key / NOT NULL / generated flags and row estimates. No rows, no column defaults, no host, port, user name, password or connection name | Implemented & verified (unit test of the exact text, including an absent default; E2E: the inspector must name the engine and a table and must not contain the host; the stand-ins' stdin holds neither the host nor the connection name) |
| The user sees what is sent | The context inspector shows the text for the tab in front; every sent message keeps the exact context that went with it; a switch turns sharing off | Implemented & verified for the inspector (E2E). The per-message record and the switch have no automated test |
| The prompt is not in the arguments | Instructions (for Claude: `--system-prompt`, a constant), context and conversation: only the constant is an argument, the rest is written to stdin | Implemented & verified (unit test with a stand-in script; E2E: no schema or conversation text in the recorded arguments) |
| The model name cannot be an option | Validated in the backend before any process starts | Implemented & verified (unit tests) |
| An empty working directory | `agent-scratch` in the app-data directory; nothing is written there | Implemented & verified (unit test with a stand-in; the opt-in test on the real Claude CLI; E2E) |
| No session kept by the CLI | `--no-session-persistence` (Claude), `--ephemeral` (Codex); PuriCat resends the conversation each turn | Implemented & verified for the options being passed. Only the working directory was checked for leftovers; the CLIs' own directories were not inspected |
| Database text cannot close the context block | `</context>` inside the context is rewritten before it is sent | Implemented & verified (unit test) |
| Answers are text | A Markdown reader produces data that is rendered as React elements; no HTML from an answer is inserted | Implemented & verified (Vitest; E2E) |
| A turn can be stopped and cannot run forever | Stop kills the CLI and, on Unix, everything in its process group (a CLI can be a launcher with the real program as its child); so does a 10-minute timeout. A CLI that has given its verdict but does not exit is killed after 5 seconds | Stop: implemented & verified (unit test with a stand-in; E2E on a streaming turn). The process-group kill and the end of a turn at its verdict each have a stand-in unit-test case; through the UI, Stop was exercised only against the stand-in CLI and only before they were added ([TESTING.md](TESTING.md)). The timeout has no test. On platforms other than Unix only the CLI process itself is killed |

AI chat limitations:

- **What is sent goes to the provider**, under the user's own account and that provider's terms. PuriCat cannot recall it.
- **Schema names and the active query can themselves be sensitive.** Table and column names are sent as they are. The SQL of the active query tab is sent with the history's password masking applied (`script::redact`, a heuristic) and otherwise as typed, so any other literal written in that tab goes along. There is no per-column redaction and no way to exclude a table.
- **The user's messages are not filtered.** Whatever is typed or pasted into the composer, including rows or credentials, is sent.
- **Switching the provider keeps the conversation.** The next turn resends the earlier questions and answers, with the context of the new message, to the other provider.
- **Database identifiers are untrusted text inside the prompt.** A table name, a column name or the active query can be written to steer the model. The fence and the instructions are a request to the model, not a control. Because the model has no tools, an injection can at most change the text of an answer — which is why SQL from an answer must still be read before it is run. On a connection whose schema other people can write to, treat answers accordingly.
- **Codex still has a shell tool.** PuriCat knows no documented option that removes it. It runs inside Codex's read-only sandbox, started in the empty scratch directory: its commands cannot write, and the sandbox blocks the network for them, but a manipulated Codex could read local files the user can read and put them in its answer. Claude Code runs with no tools at all. PuriCat has not tested the sandbox; it relies on Codex's own description of it. The same reach is open to script running in the web view, which could ask Codex through `agent_send` to read a file.
- **Codex's successful path is unverified against the real CLI**; only its refusal was observed ([TESTING.md](TESTING.md)).
- **Sharing is a UI rule.** `agent_send` forwards the context text it receives and does not check where it came from or whether the switch is on; `agent_context` needs only an open connection. Script in the web view could send anything it can read.
- **The CLIs' own telemetry, update checks, logs and caches are outside PuriCat's control.** PuriCat itself sends nothing anywhere.
- **No version check.** A CLI release that drops an option or changes its stream fails the turn with its own message; one that changed what an option means would not be noticed.
- **No audit log.** Chat turns are not recorded by PuriCat. SQL opened from an answer and run in a query tab appears in that tab's history like any other.
- **Backend messages are English only.**

### Planned (Phase 5): tools and approval

Not implemented. The design ([ADR 0008](docs/adr/0008-ai-cli-integration.md)):

- read-only database tools, served by a PuriCat-owned MCP server;
- writes are returned as proposals, shown with SQL, target and impact, and executed by PuriCat only after approval;
- approvals and outcomes are logged without secrets;
- columns marked sensitive are redacted from what the model receives.

The read-only session option will not be enough to hold such tools back (see "Read-only sessions").

## Debug-only test hooks

| Variable | Effect |
|---|---|
| `PURICAT_DATA_DIR` | Use this directory instead of the app-data directory |
| `PURICAT_SECRET_STORE=memory` | Keep passwords in process memory instead of the OS credential store |
| `PURICAT_E2E_SCRIPT` | Evaluate this file in the web view after each page load |
| `PURICAT_E2E_REPORT` | File that the `e2e_report` command writes |

They exist so tests never touch real settings or the Keychain ([ADR 0012](docs/adr/0012-e2e-testing-on-macos.md)). All four are behind `#[cfg(debug_assertions)]` and are absent from release builds. The `e2e_report` command remains registered in release builds but only returns an error. Debug builds must not be distributed.

One hook is not debug-only: a query tab attaches its Monaco editor object to the tab's root DOM element as `__editor`, in every build, so the end-to-end test can set the editor's text. It gives a script that already runs in the web view nothing it could not reach otherwise.

## Not yet done

| Item | Phase |
|---|---|
| SSH tunnels and host-key verification | 2b |
| TLS verification tested against a TLS-enforcing server; stricter default for production | 2b |
| Audit log of saves, approvals and mutations | 3–5 |
| Verification of the production and bulk-threshold review paths through the UI | 3 |
| Client-side validation before save | 3 |
| Per-connection session time zone (PostgreSQL) | 2b |
| Smart Paste: "skip invalid rows"; value-aware comparison | 3 |
| Smart Paste: pre-check of key values of non-integer types that the database coerces (dates, for example) | Not scheduled |
| Row-content check on DELETE; backend enforcement of the review policy | Not scheduled |
| SQL console: backend enforcement of the confirmation and the impact threshold; a read-only refusal in `run_sql` that SQL cannot switch off | Not scheduled |
| SQL console: transaction state read from the server instead of the statement text | Not scheduled |
| SQL console: a switch to turn history off, masking of secrets other than password literals | Not scheduled |
| SQL console: verification through the UI of Stop, the lost-connection path, the Production confirmation and PostgreSQL | 4 |
| AI: MCP server with read-only tools, approval queue, audit log | 5 |
| AI: per-column redaction, masking of literals other than passwords in the active query's text, a scope the user can narrow | 5 |
| AI: Codex's successful path verified against the real CLI; a way to run Codex without its shell tool; CLI version checks | 5 |
| AI: backend enforcement of the "Share context" switch | Not scheduled |
| Backup file permissions and encryption | 6 |
| Code signing, notarization, hardened runtime, signed updates | 7 |
| Dependency audit in CI (`cargo audit` / `cargo deny`, `pnpm audit`) | 7 |
| Generated third-party notices | 7 |
| Windows: file permissions, Credential Manager verification | After macOS beta |
| Independent security review | Before public beta |

## Reporting a vulnerability

A private reporting contact is to be published before public beta. Until then there is no public release and no supported version.
