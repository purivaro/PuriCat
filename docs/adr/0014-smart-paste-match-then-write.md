# 0014 — Smart Paste: match in the database, preview, then plain INSERT / UPDATE in one transaction

## Status

Accepted. Implemented for MySQL and PostgreSQL. Verified in the backend on both engines for single-column and composite keys; through the UI for UPSERT on MySQL only ([TESTING.md](../../TESTING.md)). SQL Server has no implementation. Builds on [ADR 0013](0013-staged-edits-and-optimistic-saves.md).

## Context

The brief asks for a guided way to load a block copied from a spreadsheet as INSERT, UPDATE or UPSERT, matched on a key, with the exact outcome shown before anything runs. It also asks that engine differences are explained rather than hidden, and that SQL Server's `MERGE` is not used blindly.

Constraints:

- The UI holds values as text and must not build SQL ([ADR 0002](0002-core-crate-and-thin-ipc.md), [ADR 0004](0004-server-side-text-rendering.md)).
- Whether two keys are "the same" depends on the column's type and collation. `abc` and `ABC` are one key under a case-insensitive MySQL collation and two keys in PostgreSQL; `7` and `007` are one integer. A client cannot know this in general.
- A table can have several unique indexes. The user means one of them.
- The preview must be exact: per pasted row, what would happen.
- Arbitrary user tables and accounts: PuriCat cannot assume extra privileges or helper objects.
- A write planner with complete-key addressing, an optimistic check and the exactly-one-row rule already exists (ADR 0013).

## Decision

### 1. Match explicitly on one verified unique key

UPDATE and UPSERT need a key chosen by the user; INSERT may use one to refuse rows that already exist. The key must be the table's primary key or a unique index, read from the catalogue by the backend at every analysis, and every key column must be mapped. Composite keys are accepted.

Indexes on which equality is not row identity are not offered and are refused if sent: MySQL unique indexes with a column prefix or an expression key part, PostgreSQL partial and expression unique indexes.

A generated key (auto-increment, identity) may be used to match and is never written. With one in UPSERT mode, a blank key means "new row" and a key that is not found is reported as "no match" and skipped, because inserting it would create a row under a different id than the one pasted. In INSERT mode such a key is an invalid row for the same reason; a blank key inserts.

### 2. The database evaluates equality

`db::smart::Draft` first sets aside rows without a key, rows whose key value is malformed for its type (a key value for an integer column must be a whole number, because MySQL's `CAST('12abc' AS SIGNED)` would otherwise match row 12; other key columns get the value pre-checks) and rows whose key repeats as exact text. None of these reach the database. For the rest it emits one parameterised lookup per 40 pasted rows:

```
SELECT <flag for pasted row 1>, ..., <flag for pasted row n>, <key and update columns as text>
FROM t
WHERE (key = <pasted key 1>) OR ... OR (key = <pasted key n>)
```

Each flag is the same condition as in `WHERE`, rendered as text. Every returned row therefore says which pasted rows it belongs to, decided by the server's own type and collation rules, and carries the stored values rendered as the grid would show them. One existing row answering to two pasted rows makes both duplicates, also when the two fall into different lookups: each matched existing row is remembered by its key as the database rendered it. Two existing rows answering to one pasted row make it invalid. Key values use the typed comparison expressions of the save path.

`Draft` is pure: it validates, builds the lookups, takes their results and classifies. The session supplies the catalogue data and runs the queries, so the logic is unit-tested without a server.

### 3. Preview, approve, analyse again

`analyze_paste` returns one status per pasted row — insert, update (with the changed columns), unchanged, unmatched, missing key, duplicate, exists, invalid — the counts, and a fingerprint of every row's status and changed columns. It issues only reads. Cheap pre-checks (number shape, declared `char` / `varchar` length, NULL into NOT NULL, required columns not mapped) flag the usual spreadsheet mistakes; the server remains the authority.

Any problem blocks the run, unless the request says to leave the rows with problems out (`skipProblems`, added on 2026-11-17): those rows were never part of the change set, so the option only lifts the refusal, and the analysis and its fingerprint are the same with or without it. `apply_paste` receives the fingerprint of the preview the user approved, repeats the whole analysis, and refuses with a `conflict` error unless the fresh analysis produces the same fingerprint; equal counts are not enough. The statements that run come from that fresh analysis, not from the preview.

### 4. Reuse the optimistic planner

The resulting change set goes through `write::plan_keyed`, the planner behind grid saves, with the chosen key in place of the primary key. Rows are addressed by the key as the database rendered it, each UPDATE requires the changed columns to still hold the values the fresh analysis saw, and every statement must affect exactly one row. Only INSERT and UPDATE statements are produced; only columns whose text differs are written.

### 5. One transaction, with progress and cancel

A run is a single transaction of at most 10,000 single-row statements. Before each statement the executor calls back: the IPC layer reports progress over a channel every 20 statements and reads a cancel flag registered under a job id. A cancel, a driver error or a wrong row count rolls back everything.

The brief asked for bounded batches. A single bounded transaction was chosen instead, so that a paste is atomic and "Stop" always means "nothing happened". Jobs larger than the bound belong to the Phase 4 import tool, which will need batches and a different recovery story.

## Alternatives considered

| Option | Why not |
|---|---|
| MySQL `INSERT ... ON DUPLICATE KEY UPDATE` | Fires on a clash with *any* unique index, not the one the user chose, so a row can be updated because of an unrelated unique column. Affected-row counts (1 inserted, 2 updated, 0 unchanged, and different again with some client flags) are an ambiguous basis for a per-row report, and nothing can be shown before it runs |
| PostgreSQL `INSERT ... ON CONFLICT` | Needs a conflict target per statement and still hides the per-row outcome until it has run; a pure UPDATE mode and the generated-key rules would need separate statements anyway |
| SQL Server `MERGE` (and PostgreSQL `MERGE`) | `MERGE` has well-known concurrency pitfalls on SQL Server and needs explicit locking hints to be safe; the brief rules out using it blindly. It would also be a third engine-specific shape for the same job |
| Staging or temporary table plus a JOIN | Fast for large inputs, but needs the `CREATE TEMPORARY TABLE` privilege, must pin one connection for the table's lifetime, and loses the per-row preview unless a second set of queries reads the join back. Worth revisiting for the Phase 4 import tool |
| Match in the client by comparing text | Wrong under collations (`abc` vs `ABC`), numeric spellings (`7` vs `007`, `1.5` vs `1.50`) and padded types. It would silently insert duplicates or miss updates |
| One `SELECT` per pasted row | Simple and exact, but one round trip per row: 10,000 queries for a full paste before any write |
| Trust the unique key named by the UI | The web view is the untrusted side of the IPC boundary; a non-unique "key" would let one pasted row address many rows. The backend reads the catalogue itself |
| Several transactions (batches) | A failure or a cancel part-way would leave some rows written, and the preview's promise would no longer hold |

## Consequences

- The same explicit semantics on both engines, and a natural shape for SQL Server later: lookup, then plain statements. Only the chosen key decides which row is the same; the dialog says so.
- The preview is exact for the moment it was taken, and the run cannot silently do something else: every row's outcome is re-derived and compared, and the UPDATEs are guarded.
- No extra privileges, helper tables or schema requirements. Lookups cost one query per 40 pasted rows; each written row costs one statement.
- All write rules of ADR 0013 apply unchanged, because the same planner and executor run.

Limitations:

- **"Unchanged" is decided on this side.** The pasted text is compared with the server's rendering. Since 2026-11-17 a value that is plainly the stored one in another spelling is also unchanged (`db::smart::same_value`: decimal numbers, dates and times without a time zone, PostgreSQL booleans); before, `1.5` against a stored `1.500000` was counted and run as an update. This is the one place where equality is not left to the database, and the reasons that rule out comparing keys here (table above) do not apply to it: a key decides *which* row is touched, this decides only whether a column of the row already found needs writing, a wrong "changed" writes the same value again, and the accepted spellings are limited to those both servers were seen to store identically. Asking the server instead would add one comparison per pasted row and column to every lookup.
- **One transaction, 10,000 rows.** Locks are held for the whole run; duration near the limit or on a remote server has not been measured.
- **Problems block the whole run unless the user leaves them out.** Leaving out covers what the analysis found. A row the server refuses during the run (a date that does not exist, a foreign key) still rolls the whole run back.
- **The run-time fingerprint covers each row's status and changed columns, not the stored values.** A stored value that changes without changing a row's outcome passes this step; the statements use the values of the fresh analysis, and the optimistic check on each UPDATE and the unique index on each INSERT still apply.
- **The analysis is not inside the write transaction.** Lookups run on pooled connections just before it starts.
- **Key values of non-integer types are pre-checked only generically.** Integer keys must be whole numbers; a key value of another type that the database coerces (a date, for example) gets only the number-shape and length checks and is then cast and compared by the server. A malformed one is not tested.
- **Empty cells.** The wizard sends an empty cell as NULL for a key column and for every non-text column, so a NOT NULL column with a default takes the default on an inserted row. For a text column an empty cell is an empty string; its default is reached by leaving the column unmapped or by pasting the text `NULL` with that option on.
- **Entry point.** The wizard is offered only on editable table tabs, which today require a primary key, so a table with only a unique index cannot use it from the UI.
- **Cancellation is between statements.** Lookup queries cannot be cancelled, and a stop request arriving after the last statement has started does not prevent the commit.
- **The preview shows operations, not SQL text.** Production-labelled connections get a warning and a danger-styled button, with no typed confirmation.
- Not verified: composite keys, PostgreSQL and the INSERT / UPDATE modes through the UI (composite-key matching is verified in the backend on both engines), the progress bar, Stop and the blocked dismissal of the dialog during a run through the UI, the 10,000-row limit, other server versions.

## Licensing

No new third-party component. The clipboard is read through `tauri-plugin-clipboard-manager`, already introduced by ADR 0013.
