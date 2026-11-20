# 0027 — Files larger than one run: imported in batches, with the rows that were not written reported

## Status

Accepted. Extends [ADR 0014](0014-smart-paste-match-then-write.md) (Smart Paste) for files of more than 10,000 rows; a paste or a file within the limit runs as before. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-20 (large imports)".

Verified on MySQL 9.3.0 and PostgreSQL 16.13 (live tests on both; through the window on MySQL). SQLite, a remote server, and files near the 64 MB limit have not been tried.

## Context

Smart Paste runs at most 10,000 rows, in one transaction, so that "Stop" always means "nothing happened" (ADR 0014, section 5). A longer file was refused with "split the file". ADR 0014 left larger jobs to an import tool with batches and "a different recovery story". The owner asked for what a database tool they use has and PuriCat lacks; importing a large file, going on past rows that fail and reporting them, is one of those (survey of 2026-11-20, kept locally).

## Decision

1. **The same wizard.** A file with more rows than one run takes opens in Smart Paste as any other: the first 10,000 rows are mapped and previewed (mode, key, columns, the preview's statuses). The dialog says that the import will read the whole file, 1,000 rows per transaction, and that what was imported stays if it stops part-way.
2. **The file stays in the backend.** `read_table_file` parses it once and keeps every row in memory when there are more than 10,000 (the 64 MB file limit stays); the window receives the first 10,000. The import asks for the rows a page at a time (`table_file_rows`), so the window never holds the whole file.
3. **Each batch is made exactly as the preview's rows were.** The window builds the request for a page with the same function as for the preview (header row, mapping, empty cells, `NULL` text), then `import_batch` analyses it as a paste is (the lookups, the statuses) and writes it in one transaction with the same planner and executor (ADR 0013, 0014). There is no fingerprint check per batch: no preview of that batch was approved; the mapping and mode were.
4. **What to do with a row that cannot be written** is the user's choice, asked before the import:
   - **Stop the import** (the default): a batch with a row the analysis finds a problem with is not run at all; a batch the server refuses is rolled back. The import ends there; the batches before it stay.
   - **Leave it out and go on**: rows the analysis finds a problem with are left out of the batch's transaction; if the server refuses the transaction (a constraint the analysis cannot see), the batch is written again one row per transaction, so that each refused row is found, reported with the server's error, and the others are written.
5. **The report** says how it ended (done, stopped by the user, stopped at a line, stopped by an error), what was inserted, updated, left unchanged or had no match, and lists the lines not written, by their line in the file, with the reason. "Copy these lines" puts them on the clipboard as the file had them — with the header row and a reason column when the file has a header — to be fixed and imported again.
6. **Stop** stops the batch being written (rolled back) and starts no other. While a batch is being written row by row, a stop leaves the rows already written; the rest of that batch are listed as not written.
7. A read-only connection is refused, as for a paste. A Production connection gets the same warning and danger-styled button.

## Alternatives considered

| Option | Why not |
|---|---|
| Raise the one-transaction limit | Locks held for the whole file, a rollback that undoes minutes of work, and memory for every statement at once; "nothing happened" stops being cheap |
| A separate import tool | A second mapping and preview to learn and to keep in step with Smart Paste's rules; the wizard already does the hard part (matching on a chosen key) |
| `LOAD DATA` / `COPY` | Fast, but each engine's own format and rules, no per-row matching on a chosen key, no UPDATE or UPSERT, a file the server must be allowed to read (or a local-infile option), and errors that do not say which line failed |
| A staging table and one `INSERT … SELECT` | Needs `CREATE TEMPORARY TABLE` and one pinned connection, and loses the per-row report (ADR 0014 considered it too) |
| The batches driven by the backend, with progress events | The window already builds requests from rows; doing it again in the backend would be a second copy of that logic to keep equal. A loop in the window gives progress, stopping and partial results with plain calls |
| Matching across the whole file before writing | A full preview of every row before any write is the paste's promise, and costs a second pass over a large file; here only the first 10,000 are previewed |

## Consequences

- A file of any length within 64 MB can be imported, with the same matching and mapping as a paste, and its failures come back as lines to fix.
- Three IPC changes: `read_table_file` keeps a large file's rows; `table_file_rows` and `import_batch` are new (99 typed commands).

Limitations:

- **Not atomic.** Batches already written stay when the import stops or fails; the dialog says so before it runs, and the report says up to which line.
- **Matching is per batch.** A key repeated in two batches is seen by the second as a row that exists (updated in UPDATE and UPSERT, "exists" in INSERT), not as a duplicate in the file. Within a batch duplicates are reported as in a paste.
- **The preview covers the first 10,000 rows.** Problems further down appear in the report.
- **The whole file is in memory** while the dialog is open (64 MB at most, more once parsed). A file larger than 64 MB is still refused.
- **Retrying row by row is slow** on a batch with a refused row (1,000 transactions); a file with many such rows takes long.
- Not verified: SQLite, a remote server's timing, a stop while a batch is written row by row (built, not exercised by a test), Thai and dark, Production.
