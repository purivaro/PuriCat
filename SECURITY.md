# Security

Describes the security posture of the code on 2026-11-16. PuriCat is in early development: the v0.1.0 download is signed with a Developer ID and notarized, but there has been no external review, and it should not be used with production data yet.

Labels: **Implemented & verified**, **Implemented, not yet verified**, **Planned (Phase N)**. Evidence is in [TESTING.md](TESTING.md).

## Threat model

### Assets

| Asset | Where it lives |
|---|---|
| Database passwords | OS credential store only; in backend memory while a session is open |
| Database contents | The user's servers; pages of rows in app memory while a tab is open; the full rows of a query tab's last run in backend memory (up to 100,000 rows per result); files the user exports |
| Connection metadata (hosts, user names, CA paths) | `puricat.db` in the app-data directory |
| SQL the user wrote | `puricat.db`: the text of open query tabs and the history of runs (see "SQL console") |
| SQL the AI ran to read rows, and SQL it proposed | `puricat.db`: the AI activity log, with password literals masked (see "AI database tools"). An approved proposal is also in the connection's SQL history |
| Integrity of the user's data | The servers. PuriCat writes to them only when the user saves staged grid edits, runs a previewed Smart Paste (see "Write path"), runs SQL in a query tab — typed there, or placed there from an AI answer (see "SQL console", "AI chat") — or approves SQL the AI proposed (see "AI database tools"). The AI itself writes to nothing: it is given no tool that writes |
| AI CLI accounts | Owned by the CLIs. PuriCat reads only an install and sign-in flag, and starts the CLI under the user's account for each chat turn; it never sees a token |
| What is sent to an AI provider | The user's chat messages, earlier answers and, when sharing is on, the schema context and the SQL of the active query tab (see "AI chat"). With the database tools on, also what the tools return: catalogue details and — only at the "Structure and rows" level — **rows** (see "AI database tools"). The conversation itself, with every tool result, lives in web view memory only |

### Trust boundaries

| Boundary | Trusted side | Untrusted side |
|---|---|---|
| IPC | Rust backend | Web view content |
| Database protocol | PuriCat | Server responses, object names and cell values |
| Subprocess | PuriCat | Output of spawned tools, including every answer a model writes |
| The socket of a chat turn with tools | PuriCat, which serves the tools | Whatever connects to it: the CLI's relay, and in principle any process of the same OS user that finds it while the turn runs. Every request is treated as a model's: it can only call the turn's tools |
| AI provider | — | The CLI's provider service: what a chat turn sends leaves the machine under the user's own account and that provider's terms |
| Network | — | Everything between the app and a database server |
| Local machine | The logged-in OS user | Other local users |

Out of scope: an attacker who already runs code as the logged-in user or as root, and a compromised operating system.

### Threats and mitigations

| Threat | Mitigation | Status |
|---|---|---|
| Password disclosed through files, logs or the UI | Stored only in the OS credential store; no IPC command returns one; connections use typed options, not URLs, so driver errors do not carry it | Implemented & verified (Keychain round trip; MySQL auth failure does not echo the password) |
| Script running in the web view escalates to the system | No shell, filesystem or HTTP capability; CSP `script-src 'self'`; all effects go through 36 typed commands plus plain-text clipboard read and write and the native Save dialog | Implemented; not penetration-tested. Script in the web view could read the clipboard, could call `apply_changes` or `apply_paste` on an open connection, could run any SQL on an open connection through `run_sql`, could make the backend write an export file to any path the OS user may write (`export_result`), and could send any text to an AI provider under the user's account through `agent_send` — with Codex, whose sandboxed shell tool can read local files, that is a way to read files the web view cannot reach itself (see "AI chat"). It could also start a turn with the database tools at any level on an open connection, whatever the user chose, and approve a stored proposal (`agent_proposal_run`); neither gives it more than `run_sql` already does, except that rows read by a model go to the provider |
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
| Command injection through subprocess arguments | Fixed argument vectors built in Rust, no shell; the chat's prompt travels on stdin; the one user-influenced argument, the model name, is validated so it cannot be read as an option; with the database tools on, the vector also carries the path of PuriCat's own executable and of the turn's socket, both produced by PuriCat and encoded as JSON; see "Subprocess policy" | Implemented & verified for detection and for the chat (unit tests: the vectors with and without tools, including an executable path with a space in it, the model-name rule, the prompt on stdin and absent from the arguments; E2E on stand-in CLIs) |
| Test hooks abused in a shipped build | Hooks compiled only with `debug_assertions` | Implemented; no release build has been inspected to confirm |
| The AI reads or changes data on its own | By default the model is given no tools and no database access: it only writes text. SQL from an answer reaches a database only when the user opens it in a query tab and runs it, under the console's safeguards (see "AI chat"). The database tools exist only after the user turns them on | Implemented & verified for what PuriCat passes to the CLI (unit tests; E2E on stand-ins: `--tools ""`, `--strict-mcp-config`, Codex's `read-only` sandbox), for the tools being off by default (E2E) and for "Open in query tab" running nothing (E2E). That the real Claude CLI then offers the model no tools is seen in a captured `system/init` line, not asserted by a test. Codex keeps a sandboxed shell tool |
| An AI with database tools writes to the database | The model gets no tool that writes. Its one query tool accepts a single reading statement, has the server prepare it, and runs it in a `READ ONLY` transaction that is rolled back, on a connection of its own (see "AI database tools") | Implemented & verified for Claude Code (unit test of the text check; live tests on MySQL 9.3.0 and PostgreSQL 16.13: 18 and 20 write attempts through the tool refused, the data unchanged; E2E on PostgreSQL: a `DELETE` through the tool refused). The guarantee is about writing through SQL: a function with effects outside the transaction is not held back by it |
| SQL proposed by a model runs without the user meaning it to | `propose_sql` runs nothing. The backend stores the proposal and runs what it stored only on `agent_proposal_run`, which the UI calls from "Approve and run" in a review dialog — never from the card, never from an answer's text. The run has the SQL console's impact guard with the row threshold read by the backend, and a read-only connection refuses it | Implemented & verified (Vitest; live tests: a proposal changes nothing; E2E on PostgreSQL: nothing changes before approval, the approved statement changes exactly its row, a 1,250-row `DELETE` is rolled back by the guard, a rejected proposal changes nothing; E2E on MySQL: an approved statement changes exactly its row). The Production warning and the read-only refusal have no automated test. Approval is a UI step: the backend cannot tell that a person read the dialog |
| Secrets or data leak to the AI provider | The shared context is names and types only, rendered by the backend: no rows, defaults, hosts, ports, user names, passwords or connection name. The user can read it before sending and with every sent message, and can switch sharing off. With the tools on the model can also obtain column defaults and keys and, only at the "Structure and rows" level, rows — at most 200 per query, each call shown in the answer with the exact text that was given, each row-reading query logged. No tool returns a host, a user name, a password or the connection's name | Implemented & verified (unit test of the exact context text; E2E: the inspector's text, and the stand-ins' arguments and stdin holding neither the host nor the connection name, with and without tools; E2E: a step opens to the rows given, and the log holds the read). Schema names, the active query's text, whatever the user types and whatever a tool returns are sent as they are, cut for length only. There is no per-column redaction |
| Text in the database steers the model (prompt injection) | The context is fenced and declared reference data; a closing tag inside it is neutralised; tool results are declared data too. With the tools off a manipulated answer is still only text. With them on, an injection can make the model call tools: read the catalogue and, if rows are allowed, rows the account can read — which go to the provider — and propose SQL, which the user must approve. It cannot make the model write | Implemented; the fence is unit-tested. No test measures how a model reacts. The user must read SQL before running or approving it |
| Another local process uses the tools of a running turn | The socket lives in a directory of its own with mode 0700, under a random name, created so that an existing name fails; it exists only for the turn; no TCP port is opened | Implemented & verified for the mode and for the removal (unit test; the E2E runner checks the directory is gone). Not a defence against a process of the same OS user, which is out of scope here as elsewhere |
| An answer is rendered as markup or script | Answers go through a Markdown reader into React elements; nothing is inserted as HTML | Implemented & verified (Vitest: markup-looking text stays text; E2E: a streamed answer with a code block, bold and inline code) |
| What an AI read or changed cannot be reconstructed afterwards | A local activity log: every row-reading query, refused or failed read, proposal, approval, rejection and outcome, with the connection's name, the provider and the model | Implemented & verified (store unit test; E2E: the log holds `read`, `read_failed`, `proposed`, `approved`, `executed`, `blocked` and `rejected` entries of the run). A convenience record, not tamper-proof: it can be cleared, keeps 5,000 entries, and a failure to write it is ignored |
| Tampered installer or update | Signing, notarization, signed updates | Planned (Phase 7) |
| SSH man-in-the-middle | Host-key verification | Planned (Phase 2b) |

## Credential handling

- Service name `PuriCat`, key `connection/<id>/password`, through `keyring` (macOS Keychain, Windows Credential Manager).
- The password crosses IPC in one direction only, UI to backend, when the user types it. The UI later learns `hasPassword` and sends `keep`, `set` or `clear`.
- Duplicating a connection copies the password inside the backend. Deleting a connection deletes its password.
- `puricat.db` holds profiles (host, port, user name, database, TLS settings, tags) but no secrets.
- Passwords are held as ordinary strings in memory and are not zeroised.
- PuriCat never asks for, reads or stores AI account passwords or tokens. A chat turn starts the CLI with PuriCat's own environment (plus the tool's directory on `PATH`); PuriCat puts no database credential in the CLI's arguments, input or environment. That also holds with the database tools on: the CLI is given the path of PuriCat's executable and of a socket, and the tools run inside the app on the session it already holds.

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

**In a query tab it can happen.** The tab's connection is acquired through the same pool, so it starts read-only and the server refuses a write (live-tested on MySQL; the PostgreSQL console was not run read-only). But the person typing can run `SET SESSION TRANSACTION READ WRITE` (MySQL) or `SET default_transaction_read_only = off` (PostgreSQL), or open a `READ WRITE` transaction, and then write. PuriCat does not filter such statements, and `run_sql` does not check the profile's read-only flag as `apply_changes` and `apply_paste` do. So in the SQL console the read-only option protects against accidents, not against the person at the keyboard. The switching statements themselves have not been run by a test in a query tab; this follows from the engines' documented behaviour.

The AI's reading tool does not rely on this option. It has a guarantee of its own that a statement cannot switch off (see "AI database tools"): `SET SESSION TRANSACTION READ WRITE`, `SET default_transaction_read_only = off`, `SET TRANSACTION READ WRITE` and a `COMMIT` followed by a write are among the attempts the live tests send through the tool and see refused. An approved proposal is different: it is SQL the user chose to run, on a console like a query tab's, and `agent_proposal_run` refuses it up front when the saved profile is read-only (no automated test).

The strong control is a database account that lacks write privileges.

## SQL injection defences

- All values in catalogue and data queries are bind parameters.
- Identifiers cannot be bound, so every name goes through `db::ident` (backticks, double quotes or brackets doubled; empty names and NUL bytes rejected).
- Sort columns are checked against the table's metadata before they are quoted.
- Row filters (`db::filter`) are built from a fixed set of operators. The column of a condition must match the table's metadata and is then quoted; its value — and the pattern of a text search, with `%`, `_` and the escape character made literal — is a bind parameter. Unit tests assert the exact `WHERE` text and that a hostile value appears only in the parameter list.
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
- **No audit log of saves.** Saves and Smart Paste runs are not recorded anywhere by PuriCat. (What the AI read and proposed is logged; see "AI database tools".) The SQL history is a convenience, not an audit log: it covers query tabs and approved AI proposals only, keeps 1,000 entries, can be cleared, and a failure to write it is ignored.
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
- Production CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src ipc: http://ipc.localhost`. `'unsafe-inline'` is allowed for styles only. The SQL editor did not change it: Monaco's script, worker, stylesheet and icon font are bundled and served from the app itself. The end-to-end test records any CSP violation and fails on it; it passes against the bundled app, including the agent chat and database-tool stages ([TESTING.md](TESTING.md)).
- The development CSP (`devCsp`) is deliberately loose for the Vite dev server and applies to `pnpm tauri dev` only.
- Fonts are bundled; the app loads nothing from the network. The agent chat added no capability and no CSP change, and neither did the database tools: the web view reaches the CLIs only through the `agent_*` commands, and it is the CLI process, not the app, that contacts the AI provider. The tools' socket is opened by the backend; the web view has no access to it.

## Subprocess policy

Subprocesses are started from Rust, never from the web view, with a fixed argument vector. All but the `kill` helper have a timeout and kill-on-drop.

| Command | Purpose | stdin | Timeout |
|---|---|---|---|
| `claude --version`, `claude auth status --json` | Detection; only `loggedIn` is read, other fields are discarded | Closed | 8 s |
| `codex --version`, `codex login status` | Detection; only whether the text says "logged in" | Closed | 8 s |
| `$SHELL -lic "command -v claude"` (or `codex`) | Fallback when the binary is not in `PATH` or the usual per-user directories; also tried at the start of a chat turn | Closed | 8 s |
| `codex debug models` | The list for the chat's model selector; only the name, display name, description and visibility of each entry are read | Closed | 15 s |
| `claude -p …` / `codex exec …` (the vectors are in [AI_INTEGRATION.md](AI_INTEGRATION.md)) | One chat turn, in a process group of its own on Unix | The prompt, then closed | 10 min; Stop kills it. After its verdict it has 5 s to exit |
| `<puricat> --mcp-bridge <socket>` | Not started by PuriCat: the CLI starts it as its MCP server in a turn with the database tools. PuriCat's own executable, which in this mode only copies its stdin to the turn's socket and the socket to its stdout | The CLI's MCP requests | None of its own. It ends when the CLI closes its input or when the socket closes with the turn. Whether Stop's process-group kill reaches it depends on how the CLI started it; that was not examined |
| `/bin/kill -KILL -<pid>` (Unix) | Ends a chat turn's whole process group on Stop, on timeout, or when the CLI does not exit after its verdict. `<pid>` is the process id of the CLI PuriCat started | Closed | None |

Points to be aware of:

- The fallback does use a shell. The command string is a constant plus a hard-coded binary name, so nothing untrusted is interpolated, but an interactive login shell runs the user's own start-up files.
- The binary that is executed is whichever `claude` or `codex` is found first, as in any terminal. PuriCat does not verify its origin. Since the chat, that binary also receives the prompt.
- A chat turn runs in `agent-scratch` inside the app-data directory, which PuriCat keeps empty, and inherits PuriCat's environment with the tool's directory prepended to `PATH`.
- The only argument a user can influence is the chat's model name: 1–100 characters out of ASCII letters, digits and `. _ : / @ - [ ]`, not starting with `-`, checked in the backend before the process is started.
- The bridge mode is decided by the first argument alone. Started with `--mcp-bridge`, the executable never initialises the app: no window, no settings store, no credential store. It connects to whatever socket path it is given and relays; it holds nothing worth taking. That the bridge exits with the turn was not checked separately (the end-to-end runner checks that the socket's directory is gone).
- stderr of a chat turn is read; its last non-empty line (at most 500 characters) is shown to the user when the CLI fails without a verdict in its stream.

## AI chat

The agent chat sends text to the user's Claude Code or Codex CLI and shows the text that comes back ([ADR 0016](docs/adr/0016-agent-chat-without-tools.md), [AI_INTEGRATION.md](AI_INTEGRATION.md)). **By default the model is given no tools and no database access**: no MCP server is configured and nothing is logged, because there is nothing to log. This section describes that default. When the user turns the database tools on, the model can act within the limits described in the next section, "AI database tools"; the defences below still apply to such a turn unless a row says otherwise.

| Defence | Detail | Status |
|---|---|---|
| With the tools off, the model cannot act | Claude Code is started with `--tools ""`, `--strict-mcp-config` and `--safe-mode`, plus options meant to keep slash commands out. Codex is started with `--sandbox read-only`, `--ignore-user-config` and `--ignore-rules` | Implemented & verified for the argument vectors (unit tests; E2E reads what the stand-in CLIs were given). Behaviour of the real CLIs: the opt-in test against Claude Code 2.1.303 returned an answer; the tool list it reported is seen in a captured line, not asserted. Codex keeps its shell tool |
| With the tools off, the user is the only path to a database | SQL in an answer is a code block with "Copy" and "Open in query tab". The latter opens a new query tab with the text in the editor and runs nothing; the SQL console's confirmation, impact guard and row cap apply from there. A code block never has a "Run" button, with or without tools | Implemented & verified (E2E: the new tab holds exactly the SQL and no run happened). The safeguards themselves are those of "SQL console", with their limits |
| Nothing but names and types in the context | Rendered in the backend from the catalogue: engine and version, database and schema, tables and views with column names, types, key / NOT NULL / generated flags and row estimates. No rows, no column defaults, no host, port, user name, password or connection name. (The tools are a separate channel: see the next section for what they add) | Implemented & verified (unit test of the exact text, including an absent default; E2E: the inspector must name the engine and a table and must not contain the host; the stand-ins' stdin holds neither the host nor the connection name) |
| The user sees what is sent | The context inspector shows the text for the tab in front; every sent message keeps the exact context that went with it; a switch turns sharing off | Implemented & verified for the inspector (E2E). The per-message record and the switch have no automated test |
| The prompt is not in the arguments | Instructions (for Claude: `--system-prompt`, one of two fixed texts — without tools, or with them), context and conversation: only the fixed text is an argument, the rest is written to stdin | Implemented & verified (unit test with a stand-in script; E2E: no schema or conversation text in the recorded arguments) |
| The model name cannot be an option | Validated in the backend before any process starts | Implemented & verified (unit tests) |
| An empty working directory | `agent-scratch` in the app-data directory; nothing is written there | Implemented & verified (unit test with a stand-in; the opt-in test on the real Claude CLI; E2E) |
| No session kept by the CLI | `--no-session-persistence` (Claude), `--ephemeral` (Codex); PuriCat resends the conversation each turn | Implemented & verified for the options being passed. Only the working directory was checked for leftovers; the CLIs' own directories were not inspected |
| Database text cannot close the context block | `</context>` inside the context is rewritten before it is sent | Implemented & verified (unit test) |
| Answers are text | A Markdown reader produces data that is rendered as React elements; no HTML from an answer is inserted | Implemented & verified (Vitest; E2E) |
| A turn can be stopped and cannot run forever | Stop kills the CLI and, on Unix, everything in its process group (a CLI can be a launcher with the real program as its child); so does a 10-minute timeout. A CLI that has given its verdict but does not exit is killed after 5 seconds | Stop: implemented & verified (unit test with a stand-in; E2E on a streaming turn). The process-group kill and the end of a turn at its verdict each have a stand-in unit-test case; through the UI, Stop is exercised against the stand-in CLI only ([TESTING.md](TESTING.md)). The timeout has no test. On platforms other than Unix only the CLI process itself is killed |

AI chat limitations:

- **What is sent goes to the provider**, under the user's own account and that provider's terms. PuriCat cannot recall it.
- **Schema names and the active query can themselves be sensitive.** Table and column names are sent as they are. The SQL of the active query tab is sent with the history's password masking applied (`script::redact`, a heuristic) and otherwise as typed, so any other literal written in that tab goes along. There is no per-column redaction and no way to exclude a table.
- **The user's messages are not filtered.** Whatever is typed or pasted into the composer, including rows or credentials, is sent.
- **Switching the provider keeps the conversation.** The next turn resends the earlier questions and answers, with the context of the new message, to the other provider.
- **Database identifiers are untrusted text inside the prompt.** A table name, a column name or the active query can be written to steer the model. The fence and the instructions are a request to the model, not a control. With the tools off, an injection can at most change the text of an answer — which is why SQL from an answer must still be read before it is run. With the tools on it can do more (next section). On a connection whose schema other people can write to, treat answers accordingly.
- **Codex still has a shell tool.** PuriCat knows no documented option that removes it. It runs inside Codex's read-only sandbox, started in the empty scratch directory: its commands cannot write, and the sandbox blocks the network for them, but a manipulated Codex could read local files the user can read and put them in its answer. Claude Code runs with no built-in tools, with or without the database tools. PuriCat has not tested the sandbox; it relies on Codex's own description of it. The same reach is open to script running in the web view, which could ask Codex through `agent_send` to read a file.
- **Codex's successful path is unverified against the real CLI**; only its refusal was observed ([TESTING.md](TESTING.md)).
- **Sharing is a UI rule.** `agent_send` forwards the context text it receives and does not check where it came from or whether the switch is on; `agent_context` needs only an open connection. Script in the web view could send anything it can read.
- **The CLIs' own telemetry, update checks, logs and caches are outside PuriCat's control.** PuriCat itself sends nothing anywhere.
- **No version check.** A CLI release that drops an option or changes its stream fails the turn with its own message; one that changed what an option means would not be noticed.
- **Chat turns without tools are not logged.** The activity log records what the tools did (next section), not the conversation. SQL opened from an answer and run in a query tab appears in that tab's history like any other.
- **Backend messages are English only.**

## AI database tools

Off by default. When the user chooses "Structure" or "Structure and rows" next to the model selector, the model gets tools for the database of the tab in front, for one turn at a time ([ADR 0017](docs/adr/0017-ai-database-tools.md); the mechanism is described in [AI_INTEGRATION.md](AI_INTEGRATION.md), "Database tools"). The model can then act, within these limits: it can read the catalogue, it can read rows if the user allowed rows, and it can propose SQL. It cannot write, and it never holds a credential.

Verified for Claude Code. **No Codex turn with tools has been observed**: only the MCP handshake. The real Claude Code CLI ran the tools through PuriCat's own server and bridge below the UI (an opt-in test), and once through the window, driven by hand: one turn on PostgreSQL, not an automated test ([TESTING.md](TESTING.md), "Results"). The automated runs through the window use a stand-in CLI that is a real MCP client: on PostgreSQL at "Structure and rows" and on MySQL at "Structure".

| Defence | Detail | Status |
|---|---|---|
| Off unless the user turns them on | The level is "Off" in a new installation and whenever the stored value is missing or unknown. With it off, the argument vector is the tool-less one, no socket is opened and no MCP server exists | Implemented & verified (unit test: the tool-less vectors carry no MCP option; Vitest: no scope is sent at "Off"; E2E: the level is "Off" before the test changes it) |
| Bound to one database, for one turn | The tools work on the connection, database and schema of the tab in front when the question is sent. With no table or query tab in front the turn has no tools | Implemented & verified (Vitest for the scope). The catalogue tools are confined to that database; the query tool is not (limitations below) |
| Rows only by explicit choice | `run_select` exists only at "Structure and rows". At "Structure" it is not offered and a call to it is refused without touching the database. The popover warns when rows are allowed on a Production-labelled connection | Implemented & verified below the UI (live tests on both engines: the tool is absent and a call returns an error and no data; Vitest: the scope sent at "Structure" does not allow rows) and, for the tool not being offered, through the UI (E2E on MySQL at "Structure": the stand-in CLI, which calls `run_select` only when it is listed, made no query step). A call to `run_select` at that level through the UI, the level on PostgreSQL through the UI, and the Production warning have no automated test |
| The reading tool cannot write | Three things a statement cannot switch off: its text must be one reading statement (no second statement, no `INTO`, no locking clause, no MySQL version comment, none of a short list of functions that act outside the transaction); the server prepares it first, and neither engine prepares more than one statement; it runs in a `READ ONLY` transaction that is rolled back. The connection is the tool's own and is closed after the call, so nothing a statement sets survives it | Implemented & verified (unit test of the text check, 7 accepted and 28 refused texts; live tests: 18 write attempts on MySQL 9.3.0 and 20 on PostgreSQL 16.13 refused with the data unchanged, among them a function that writes called from a `SELECT` on PostgreSQL, `nextval`, `EXPLAIN (ANALYZE) DELETE` — which passes the text check and is stopped by the transaction — and on MySQL a text that is one statement to the reader and two to the server, stopped by the prepare; E2E: a `DELETE` through the tool is refused and the table keeps its 1,250 rows) |
| What a query can return is bounded | 50 rows unless the model asks otherwise, 200 at most; a cell is cut at 300 characters; a tool's answer at 24,000; the query is cancelled on the server after 30 seconds, and when the turn ends | Implemented & verified for a row cap the model asks for, the cell cut and the cancel when a turn ends (unit test; live tests, both engines). The default of 50, the ceiling of 200 and the 30-second limit have no test. The bound is per query: nothing limits the number of queries in a turn |
| Changes go through the user | `propose_sql` runs nothing; it hands the SQL to the backend, which stores it. The card in the conversation has no run button: "Review and run…" opens a dialog with the whole SQL, the connection, its environment, the database, the statements the classifier flags and what guards the run, and only "Approve and run" there calls the backend | Implemented & verified (live tests: the data is unchanged after a proposal; E2E on PostgreSQL: unchanged until approval, then exactly the proposed row changed; a rejected proposal changes nothing; E2E on MySQL: the review dialog names the MySQL connection, and after approval the proposed row is higher by exactly 1 than before the question. Once by hand with the real Claude Code CLI on PostgreSQL: the row unchanged until approval, then 200.50 → 201.50) |
| What is approved is what was stored | `agent_proposal_run` takes an id. The SQL, the connection and the database come from the backend's copy, not from the web view | Implemented; by construction. Not asserted by a test that tampers with the web view's copy |
| An approved change is still guarded | It runs on a console of its own with the SQL console's impact guard. The row threshold is the connection's `bulkReviewRows`, read by the backend from the saved profile. Above it the statement is rolled back; running what was held back is a third, separate confirmation, and only what was held back is run | Implemented & verified (E2E on PostgreSQL: a 1,250-row `DELETE` approved, rolled back, all rows still there, then declined; a three-statement proposal whose first statement runs once and whose second is held back; the third confirmation shows the second and third statements with a note that one follows the held one, and "Run anyway" runs those two without the first running again). The guard's own limits are those of "SQL console" |
| A read-only connection refuses an approved change | Checked against the saved profile before anything is sent | Implemented, not yet verified (no automated test) |
| A proposal runs once | The backend refuses a proposal that is already running, removes one that ran — wholly or up to an error — and refuses an unknown id. One that could not start stays, to be approved again | Implemented & verified in the store with the backend mocked (Vitest); the command itself only through the E2E paths above |
| No credential and no connection detail reaches the CLI | The CLI is handed the path of PuriCat's executable and of the turn's socket. The tools run inside the app on the open session; no tool returns a host, port, user name, password or the connection's name | Implemented & verified (E2E: the stand-in's arguments and prompt in a turn with tools contain neither the server's address, the user name nor the connection's name) |
| The socket is private and short-lived | A directory with mode 0700 under a random name, created so that an existing name fails; a Unix socket inside it; no TCP port. The socket, its directory and every connection end with the turn | Implemented & verified (unit test: the mode, the removal, an open connection ended; E2E: the directory is gone after the run) |
| The server answers four methods | `initialize`, `ping`, `tools/list`, `tools/call`. A tool that is not offered is never called. A request line over 1 MiB ends the connection | Implemented & verified (unit tests, on the requests the two real CLIs sent to a probe server; live tests: a call to a tool that does not exist is a protocol error) |
| Only PuriCat's tools, none of the user's customisations | Claude Code: `--tools ""`, `--mcp-config` with PuriCat's server alone, `--strict-mcp-config`, `--allowedTools mcp__puricat`, `--permission-prompts none`, `--setting-sources ""`, `--disable-slash-commands`. Codex: the server is added with `-c` overrides beside `--ignore-user-config`, `--ignore-rules` and the read-only sandbox | Implemented & verified for the vectors (unit tests; E2E reads them back from the stand-in). With the real Claude Code CLI the model called PuriCat's four tools (opt-in test), and `run_select` and `propose_sql` in the one manual turn through the window. That it could call nothing else, and that `--setting-sources ""` kept the user's instructions out, are not asserted by a test (the second was checked by hand once, before the manual turn through the window; neither was looked at in that turn). Codex: unverified |
| The user sees what the model was given | Every tool call is a step in the answer that opens to what was asked and the exact text returned, rows included | Implemented & verified (live tests: the event carries exactly the tool's output; E2E: a step opens to "4 rows. …" and the refusal; on MySQL a `describe_table` step opens to the table's structure) |
| What happened is recorded | The activity log: `read`, `read_failed`, `proposed`, `approved`, `rejected`, `executed`, `blocked`, `failed`, with the time, the connection's id and name, the database, the provider and the model. SQL is stored with password literals masked (`script::redact`). Rows are not stored | Implemented & verified (store unit test; E2E: the entries of the run are there). The masking on this path is not asserted; "Clear log" and the 5,000-entry cap have no test through the UI |

AI database tools limitations:

- **Codex is unverified.** A Codex tool call has never been observed, nor whether `codex exec` lets an MCP tool run without asking. The access popover says so when Codex is selected; nothing stops the user from trying.
- **With the tools on, Claude Code runs without `--safe-mode`**, because that option also switches MCP servers off. What keeps the user's settings, instruction files, hooks and other MCP servers out is `--setting-sources ""`, `--strict-mcp-config` and `--disable-slash-commands`. Their combined effect on a real CLI was checked by hand once, not by a test.
- **Rows that are read leave the machine**, under the user's account with the provider and that provider's terms, as the database returned them and cut for length only. There is no per-column redaction and no way to exclude a table. The cap is 200 rows per query, not per turn.
- **The read-only guarantee is about writing through SQL.** A function with effects outside the transaction — ending another session, sleeping, reading a file on the server — can be called by a `SELECT` if the account may call it. The list of refused function names is matched as plain text, so an escaped spelling gets past it; it is a courtesy, not a boundary. An account without write privileges, and without such functions, remains the strong control.
- **The query tool is not confined to the database in front.** On MySQL a query can name another database the account can read; on either engine it reads whatever the account can read. Only the catalogue tools are confined.
- **Prompt injection now reaches tools.** Text in a table name, a column name or a row can steer the model into calling them: reading more than the user asked about, which then goes to the provider, or proposing SQL written by whoever planted the text. The proposal still needs the user's approval, which is why the dialog shows the whole SQL — and why it has to be read.
- **Approval, and the level, are UI rules.** `agent_send` takes the tool scope from the web view and does not read the stored level; `agent_proposal_run` needs only the id of a stored proposal. Script in the web view could use both.
- **Same-user processes are trusted.** Another process of the same OS user that finds the socket during a turn can call the same tools, with the same limits.
- **Proposals run statement by statement, each committed on its own**, like a script in a query tab. An error leaves the earlier statements applied. A transaction the script leaves open is rolled back when its console closes.
- **Running what was held back lifts the limit for more than one statement.** "Run anyway" runs the statement the guard rolled back and every statement after it in the proposal, all without the limit. The third confirmation shows that whole text and says how many statements follow the held one; the limit is not applied again to them.
- **Names are resolved by the connection.** The reading tool and an approved proposal run on a connection opened on the tab's database, not its schema: on PostgreSQL unqualified names go through the session's `search_path`. The instructions for a turn with tools (`agent::tools_prompt`) now tell the model to write every table name with its schema on PostgreSQL. That is a request to the model, not a control: nothing checks a query or a proposal for it, no unit test asserts the sentence, and it was in the build of one real-CLI turn only (the manual one through the window, where the model wrote `shop.orders`).
- **A Production-labelled connection gets a warning, nothing stronger**: one line in the access popover when rows are allowed, one in the review dialog. Neither has an automated test.
- **The activity log is a local convenience record.** It lives in `puricat.db` (mode 0600 on Unix) in clear text apart from masked password literals — the mask is the history's heuristic, with its limits ("SQL console"). It keeps the newest 5,000 entries, can be cleared by the user, is not deleted with a connection, and a failure to write it does not stop the action. Entries for reads and proposals are written when their turn ends. Catalogue lookups are not logged. The refusal of an approved proposal on a read-only connection is not logged.
- **Tool results stay in the conversation**, in web view memory, until "New chat" or the app closes.
- **Unix only.** The socket and the bridge need Unix domain sockets; on other platforms a turn with tools fails with a message. Nothing was run there.
- **Backend text is English in every UI language**: the reason a step failed, the note on a flagged statement, the detail line of a log entry.

### Still planned (Phase 5)

- Columns marked sensitive, redacted from what the model receives through the context and the tools.
- A policy per connection for what the AI may do, in place of one global level; a stricter step for Production-labelled connections.
- Backend enforcement of the access level.
- Codex verified with tools, or refused for them.

## Debug-only test hooks

| Variable | Effect |
|---|---|
| `PURICAT_DATA_DIR` | Use this directory instead of the app-data directory |
| `PURICAT_SECRET_STORE=memory` | Keep passwords in process memory instead of the OS credential store |
| `PURICAT_E2E_SCRIPT` | Evaluate this file in the web view after each page load |
| `PURICAT_E2E_REPORT` | File that the `e2e_report` command writes |

They exist so tests never touch real settings or the Keychain ([ADR 0012](docs/adr/0012-e2e-testing-on-macos.md)). All four are behind `#[cfg(debug_assertions)]` and are absent from release builds. With `PURICAT_E2E_SCRIPT` set, the window is also kept on top of other windows, because a covered web view pauses its timers. The `e2e_report` command remains registered in release builds but only returns an error. Debug builds must not be distributed.

One hook is not debug-only: a query tab attaches its Monaco editor object to the tab's root DOM element as `__editor`, in every build, so the end-to-end test can set the editor's text. It gives a script that already runs in the web view nothing it could not reach otherwise.

## Not yet done

| Item | Phase |
|---|---|
| SSH tunnels and host-key verification | 2b |
| TLS verification tested against a TLS-enforcing server; stricter default for production | 2b |
| Audit log of saves and Smart Paste runs (what the AI read, proposed and had approved is logged) | 3 |
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
| AI tools: Codex verified with a tool call; an automated test of the real Claude Code CLI with tools through the UI (done once by hand), and a check of what a real turn with tools has available besides PuriCat's tools; the Production warnings, the read-only refusal, Stop of a running proposal and the 30-second query limit under test | 5 |
| AI tools: a transport for platforms without Unix domain sockets | After macOS beta |
| AI: per-column redaction of the context and of tool results, masking of literals other than passwords in the active query's text, a scope the user can narrow, a policy per connection | 5 |
| AI: Codex's successful path verified against the real CLI; a way to run Codex without its shell tool; CLI version checks | 5 |
| AI: backend enforcement of the "Share context" switch and of the tools' access level | Not scheduled |
| Backup file permissions and encryption | 6 |
| Code signing, notarization, hardened runtime, signed updates | 7 |
| Dependency audit in CI (`cargo audit` / `cargo deny`, `pnpm audit`) | 7 |
| Generated third-party notices | 7 |
| Windows: file permissions, Credential Manager verification | After macOS beta |
| Independent security review | Before public beta |

## Reporting a vulnerability

A private reporting contact is to be published before public beta. Until then there is no public release and no supported version.
