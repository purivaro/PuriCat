# 0028 — Copying rows into a table on any connection, as a large import's batches

## Status

Accepted. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-20 (copy rows)".

Verified between MySQL 9.3.0 and PostgreSQL 16.13 in both directions (live test), and through the window from MySQL to PostgreSQL. SQLite as a source or a target, a remote server, and tables of millions of rows have not been tried.

## Context

Moving data between a local database and a server is a frequent chore while developing: a table filled locally that a server should have, or production rows wanted locally. PuriCat could do it only through an export and an import by hand. A database tool the owner uses has a "data transfer" between connections (survey of 2026-11-20, kept locally); the owner asked for what it has and PuriCat lacks. Since [ADR 0027](0027-large-imports-in-batches.md), PuriCat imports any number of rows in batches with a report of what was not written; a copy is the same import, with another table as its source.

## Decision

1. **"Copy rows to…"** in a table's menu. The target is a table that exists, on any connection — the same one, another server, another engine — chosen by connection, database (schema on PostgreSQL) and table. The target connection is connected when chosen. The source table itself is refused as a target.
2. **Columns are paired by name** (whatever the case) and can be changed; a source column with no partner is not copied. Binary columns (and MySQL spatial ones) are not copied: their text is not what a column takes back.
3. **The rows are read a page at a time** (1,000) by the backend, each column as the text the server renders, whole and with NULL kept, ordered by the source's primary key (or by every column read when it has none) with `LIMIT` and `OFFSET`.
4. **Each page is written as a large import's batch** (ADR 0027): Smart Paste's analysis on the target (insert, update or upsert on a key of the target), one transaction per page, and the same choice between stopping the copy and leaving a row out; a page the server refuses is written again row by row to find the row. "Check the first rows" analyses the first page without writing.
5. **The report** says how the copy ended and what was written, and lists the rows not written by their place in the source and their key, with the reason; "Copy these rows" puts them on the clipboard with the source's column names.
6. A read-only target connection is refused (the source is only read). A Production-labelled target gets a warning and a danger-styled button.

## Alternatives considered

| Option | Why not |
|---|---|
| Create the target table too | Across engines it needs a type mapping (MySQL `TINYINT(1)`, `ENUM`, `JSON`; PostgreSQL arrays, `uuid`, …) that would be a design of its own; within one engine the source's definition could be repeated. Left for a later slice: the new-table form (ADR 0025) can make the target first |
| `INSERT … SELECT` | Only within one server; no matching on a chosen key, no per-row report |
| Dump and load (`mysqldump`, `pg_dump`) | Whole tables, one engine, and the load fails as a whole on the first refused row; backups already offer it for a whole database (ADR 0021) |
| Keyset paging (`WHERE key > last`) | Faster on very large tables; needs typed comparisons and a composite-key form. `OFFSET` is simple and exact for tables of the size this is for; noted as a follow-up |
| Rows through the window | The window would hold every page twice; the backend reads and writes each page in one call |

## Consequences

- A table's rows reach another database, server or engine in a few clicks, with the matching, the statuses and the failure report a paste already has.
- Two IPC commands (`copy_preview`, `copy_rows`; 101 typed commands).

Limitations:

- **The target table must exist.** Its column types decide what is accepted; a value the target cannot hold is reported, not converted.
- **Not atomic as a whole**, as a large import: pages written before a stop stay.
- **Paging by `OFFSET`**: rows added to or removed from the source while it is copied can be read twice or not at all, and a table of millions of rows is slow to page through.
- **Values travel as text.** What one engine renders and the other reads back is usually the same value (numbers, dates, text); a boolean, an array or JSON across engines may not be, and is then reported as a row not written.
- Not verified: SQLite, a remote server, Thai and dark, Production, a stop through the window.
