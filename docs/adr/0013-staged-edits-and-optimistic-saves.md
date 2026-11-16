# 0013 — Staged grid edits, plan-then-apply saves, optimistic concurrency on text renderings

## Status

Accepted. Implemented for MySQL and PostgreSQL (tables with a primary key). SQL Server has no write path. Smart Paste, batching and a full-value editor are not part of this record and remain Planned (Phase 3).

## Context

The grid must be editable like a spreadsheet: type, paste a block, undo, then commit. The brief requires that nothing reaches the server before Save, that bulk and production changes are reviewed, and that writes are parameterised, keyed, transactional and protected against overwriting someone else's change.

Constraints from earlier decisions:

- The grid holds every value as the text the server rendered ([ADR 0004](0004-server-side-text-rendering.md)). There are no typed values in the UI and no row versions.
- The frontend must not build SQL ([ADR 0002](0002-core-crate-and-thin-ipc.md)).
- Sessions are pooled, so two statements of one tab may run on different connections unless they share a transaction.
- Arbitrary user tables: PuriCat cannot require a version or timestamp column.

## Decision

### 1. Edits are staged in the UI

The tab view keeps a `Pending` value (`src/features/table/pending.ts`): cell edits for loaded rows, new rows, and rows marked for deletion, plus an undo history. Every edit operation is a pure function from one `Pending` to the next. The database is not contacted until Save.

- Rows are addressed by their index in the loaded page. To keep that stable, paging, sorting, page size and refresh are locked while edits are pending, and a reload discards them.
- A cell set back to its original value is no longer an edit.
- A cell is editable only if its column is writable, its original value is complete (not truncated for preview), and its row has a complete, non-NULL, untruncated primary key.
- An empty input means NULL only for a nullable column that is not a text type; an untouched cell of a new row is omitted so the server default applies.

### 2. Plan, then apply, from one code path

On Save the UI sends a `ChangeSet` (updates with key, original and new values; inserts; deletes with key). `db::write::plan` turns it into parameterised statements. Two IPC commands use that same function:

- `plan_changes` returns the statements and their bound values for the review dialog;
- `apply_changes` plans again and executes.

So what is reviewed is produced by the code that runs. The review dialog is mandatory for Production-labelled connections, for any delete, and above a per-connection row threshold (`bulkReviewRows`, default 10); smaller saves apply directly.

Execution rules, enforced in the backend:

- order: deletes, updates, inserts (a deleted row's unique values can be reused by an insert in the same save);
- one transaction per save; at most 10,000 row changes;
- every statement must affect exactly one row; anything else, or any driver error, rolls the whole save back;
- UPDATE and DELETE address the row by its complete primary key, compared with a typed expression so the key index is used and 64-bit keys are not compared as floats (MySQL integer keys: `CAST(? AS SIGNED|UNSIGNED)`; PostgreSQL: `$n::<type>`);
- unknown, generated, binary and spatial columns, duplicate columns, incomplete keys, and updates or deletes on keyless tables are refused before any SQL is sent.

### 3. Optimistic concurrency by comparing text renderings

Each UPDATE adds, for every edited column, the condition that the column still renders to the text the user saw when the page was loaded:

| Engine | Condition |
|---|---|
| PostgreSQL | `col::text IS NOT DISTINCT FROM $n` |
| MySQL | `CAST(CAST(col AS CHAR) AS BINARY) <=> CAST(? AS BINARY)` (`BIT`: `CAST(col + 0 AS CHAR)`) |

Both are NULL-safe; the MySQL form is byte-wise so collation does not hide a change in case or trailing spaces. If the row was changed or removed elsewhere, the statement matches zero rows, the exactly-one-row rule fails, and the user gets a `conflict` error with nothing saved.

This reuses the read path's rendering, so the check needs no extra metadata, no typed decoding and no schema requirements.

### 4. Values as text parameters; bare-type casts on PostgreSQL

Values are bound as text and converted by the server.

- PostgreSQL: `$n::<bare type>` — `character varying`, `numeric`, `bigint`, a schema-qualified enum — taken from `format_type(atttypid, NULL)`. The length or precision is deliberately left out: an explicit cast to `varchar(5)` truncates silently, while a cast to the bare type leaves the limit to the column assignment, which raises an error. The first implementation cast to the full type and a live test caught the truncation.
- MySQL: plain parameters, converted by the server; `BIT` goes through `CAST(? AS UNSIGNED)` because a string assigned to `BIT` would store its character codes.

### 5. Strict mode for MySQL sessions

Every pooled MySQL connection appends `STRICT_ALL_TABLES` to its session `sql_mode`, so out-of-range numbers, unknown enum labels and over-long strings are errors instead of silent coercions, whatever the server default is. The rest of the server's session defaults are kept: the driver's forced UTC time zone and `PIPES_AS_CONCAT` are switched off.

## Alternatives considered

| Option | Why not |
|---|---|
| Row versions: PostgreSQL `xmin` / `ctid`, or a version / `updated_at` timestamp column | `xmin` and `ctid` are PostgreSQL-only, change on any update including untouched columns, and `ctid` changes on `VACUUM FULL`; MySQL has no equivalent. Timestamp or version columns exist only if the schema's author added them, and PuriCat edits arbitrary tables |
| `SELECT ... FOR UPDATE`, compare in the application, then write | Two round trips per row and locks held across them; the comparison would need the same text rendering anyway. A single conditional UPDATE is atomic and needs no lock management |
| Compare all columns, not only the edited ones | Stricter, but fails on rows with truncated or binary values (their full originals are not in the grid), makes statements very large on wide tables, and reports conflicts for changes the user's edit does not touch |
| Updatable cursors / positioned updates | Engine-specific (MySQL offers them only inside stored programs); would tie an edit session to one open cursor, transaction and connection for as long as the user is editing |
| Commit each cell immediately | No undo, no review, no atomic multi-row paste; a half-applied paste on error. The brief requires staged changes |
| Build SQL in the frontend | Breaks the rule that the web view never builds SQL; no single place to enforce the write rules |
| Decode to typed values and bind natively | Needs the per-type matrix [ADR 0004](0004-server-side-text-rendering.md) avoided; lossy for decimals and 64-bit integers in JavaScript |
| Cast PostgreSQL parameters to the full column type | Silently truncates over-long strings (found by a live test) |

## Consequences

- A paste of many cells is one undoable step and one transaction; a failed save leaves the database untouched and the staged edits in place.
- Conflicts are detected per edited column: two users editing different columns of the same row both succeed, without overwriting each other, because only edited columns are written.
- No schema requirements and no extra queries per row.
- Validation is the server's: the error message is the engine's own, and it arrives at Save, not while typing.

Limitations:

- **DELETE checks the key only.** A row changed elsewhere but still present is deleted without a conflict. Carrying the full original row would hit the truncated-value problem above.
- **Changes to columns the user did not edit are not detected.**
- **Text renderings depend on session settings** (time zone, date style, float formatting). The comparison is sound because the session that loads the page and the session that saves are configured identically; a session setting that changed between load and save would at worst produce false conflicts.
- **Lossy renderings compare coarsely.** If a type's text form does not capture the full stored value (for example floating-point output with reduced digits), two different stored values with the same rendering are not told apart. Not observed with the tested types.
- Cells truncated for preview, binary and spatial columns, and rows with a NULL or truncated key cannot be edited until a full-value editor exists.
- Keyless tables: the planner allows INSERT, but the UI keeps them fully read-only for now. Unique keys are not used as a substitute for a primary key.
- Edits are confined to the loaded page; staged edits do not survive paging, a reload or closing the tab.
- Saves are single-row statements in one transaction, without batching, progress or cancellation; hence the 10,000-change limit. Larger jobs belong to the planned Smart Paste / import tools.
- The plan is rebuilt at apply time from fresh metadata. If the table's structure changes between review and apply, the executed statements can differ from the reviewed ones.
- The review policy is decided in the UI; the backend does not enforce it. The backend does enforce every rule in section 2 and refuses `apply_changes` for a profile saved as read-only.
- MySQL strict mode makes PuriCat's sessions stricter than other clients on servers with a lax default `sql_mode`.
- PostgreSQL sessions run with `TimeZone=UTC` (sent by sqlx 0.8 at start-up), so `timestamptz` text is shown and compared in UTC and `now()`-based defaults of `timestamp without time zone` columns are computed in UTC. A per-connection time-zone option is Planned (Phase 2b).
- Verified on MySQL 9.3.0 and PostgreSQL 16.13 only ([TESTING.md](../../TESTING.md)). The production and bulk-threshold review paths, native keyboard input and concurrent edits through the UI are not yet verified.

## Licensing

The system clipboard is reached through `tauri-plugin-clipboard-manager` 2 (MIT OR Apache-2.0), with only the plain-text read and write permissions granted. No other third-party component is introduced by this decision.
