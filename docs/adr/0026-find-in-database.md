# 0026 — Find in database: the row filter's own tests, one pass per table

## Status

Accepted. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-20 (find in database)".

Verified on MySQL 9.3.0, PostgreSQL 16.13 and SQLite (live tests on all three; through the window on MySQL). MariaDB and very large tables have not been tried.

## Context

In a database one does not know — an old system being moved, a table someone else designed — the first question is often "where is this value kept?" or "which table has a column called that?". PuriCat could filter one table at a time. A database tool the owner uses has a "find in database" for both (survey of 2026-11-20, kept locally); the owner asked for what it has and PuriCat lacks.

## Decision

1. **"Find in database…"** in the menu of a database (MySQL, SQLite; on PostgreSQL every schema of the database that is not the server's own) and of a PostgreSQL schema. The dialog looks in **data** or in **names**.
2. **Data: one statement per table, one table after another.** For each table, the columns the value can be in are each tested with the row filter's own condition (`filter::condition`: "contains", "is" or "starts with", the value bound as a parameter, `LIKE … ESCAPE '!'` with the wildcards made literal, `ILIKE` on PostgreSQL), and one `SELECT` reads the table once and counts, per column, the rows that pass, and the rows in which any column does. The frontend runs the tables in order, shows which one is being read and how far it is, and keeps what it found so far.
3. **A result opens its table with the same filter** — the columns that hold the value, any one of them, or a single column — so the tab shows exactly the rows that were counted. The table tab takes the filter once (a workspace preset), then it is the tab's own.
4. **What cannot hold the value is not looked in, rather than failing the table**: binary and spatial columns; for "is exactly", dates, times and booleans (a cast of text that is not one would fail the statement on PostgreSQL) and numbers the text is not — an integer column only when the text is an integer within the column's range (MySQL would turn `1.5` into 1, PostgreSQL would refuse `1.5` for an `integer`). A table with no column left is not read, and the dialog says how many there were.
5. **Stop** interrupts the statement on the server (the same mechanism as counting a table's rows) and ends the search; closing the dialog stops it too. A table that cannot be read (an account without the privilege, say) is listed with the server's error and the search goes on.
6. **Names**: one catalogue query — MySQL `information_schema.TABLES` and `COLUMNS` of the database; PostgreSQL `pg_class` and `pg_attribute` of the schema, or of every schema that is not the server's own; SQLite `sqlite_master` and `pragma_table_info` — for tables, views (and materialized views) and columns whose name contains the text, whatever the case.
7. **Read-only**: the search never writes, and is offered on read-only connections. On a Production-labelled connection the dialog warns that every table of the server will be read.
8. The last search of each place is kept while the app runs, so that opening a result (which closes the dialog) does not lose the list.

## Alternatives considered

| Option | Why not |
|---|---|
| One statement for the whole database (a `UNION ALL` of every table) | A long statement no one can follow; nothing to show until all of it is done; one table that fails fails all |
| `LIMIT 1` per table: only "is it there?" | Reads the whole table anyway when the value is absent, which is the common case; the counts per column say where to look |
| The search in the backend, with progress events | The frontend loop gives progress, partial results and stopping with the commands that exist; the backend stays one table per call |
| Regular expressions | Different on every engine (and absent on SQLite without an extension); noted as a follow-up |
| Searching views' rows | A view can be an expensive query; its base tables are searched instead |

## Consequences

- A value or a name is found across a database without writing SQL, and every result is one click from the rows themselves.
- Two IPC commands (`find_in_table`, `find_names`). No new dependency, capability or CSP change. `count_rows` now shares its stoppable query with `find_in_table`.

Limitations:

- **Each table is read whole** (a `LIKE '%…%'` uses no index): on a large database a search takes as long as reading it, and loads the server while it runs.
- **Case** follows the engine: MySQL by the column's collation (usually not case-sensitive), PostgreSQL never case-sensitive (`ILIKE`), SQLite case-sensitive outside ASCII.
- **"Is exactly"** does not look in dates, times or booleans; "contains" does, on the text the grid shows.
- No regular expressions; no search of views' rows; one database (or one PostgreSQL database's schemas) at a time.
- A table with more than 20 matching columns opens filtered on the first 20 (the filter's limit).
