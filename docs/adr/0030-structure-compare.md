# 0030 — Structure compare: one catalogue read per side, compared without the server

## Status

Accepted. A first slice: the report. Generating the statements that would make one side like the other is not built. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-21 (structure compare)".

Verified on MySQL 9.3.0 (two databases) and PostgreSQL 16.13 (two schemas) by live tests, and through the window on MySQL. Two servers of different versions (MySQL 5.7 against 8, MariaDB) have not been tried.

## Context

The owner keeps many sites whose local database and production database drift apart: a column added locally and not yet on the server, an index made on the server by hand. Finding out means reading two schemas side by side. A database tool the owner uses compares structures and writes the changes (survey of 2026-11-20, kept locally); the owner asked for what it has and PuriCat lacks.

## Decision

1. **"Compare structure…"** in the menu of a MySQL database and of a PostgreSQL schema: that is side A. Side B is chosen — any connection of the same engine (connected when chosen), a database, on PostgreSQL a schema. Different engines are refused: their types do not compare.
2. **Each side is read with four catalogue queries for the whole schema** (`Session::structure`): tables and views; every column (type, NULL, default, MySQL `EXTRA` or PostgreSQL identity/generated, comment, MySQL collation); every index (columns in order, unique, primary, PostgreSQL's partial-index condition); every foreign key (columns, referenced table — with its schema when it is another — and columns, delete and update actions). The number of queries does not grow with the number of tables, which matters on a remote server. Both sides are read at the same time.
3. **The compare is pure** (`compare::compare`, unit-tested without a server):
   - tables, views and columns are matched by name;
   - a column differs in its type, NULL, default, extras, comment or collation, and the report names which;
   - indexes and foreign keys are matched by what they are, not by name, so a server's own naming (`orders_ibfk_1`) is not a difference; what one side has and the other does not is listed;
   - MySQL's integer display widths (`int(11)` before 8.0.19, `int` after) are ignored, except `tinyint(1)`, which tools read as a boolean.
4. **The report**: how many tables are alike, differ, or are on one side only; the tables and views each side has alone; and for each table that differs, its columns (A and B side by side), indexes and foreign keys. "Copy the report" puts it on the clipboard as plain text. Nothing is written.

## Alternatives considered

| Option | Why not |
|---|---|
| `SHOW CREATE TABLE` / `pg_dump --schema-only` text, compared as text | Order of clauses, server-written names and version details make every line a difference; a structural compare says what actually differs |
| Per-table queries (as the table form uses) | Three queries per table: hundreds on a real schema, each a round trip to a remote server |
| Generate ALTER statements in this slice | The statements must be built by the same rules as the table form (ADR 0025), reviewed and run with care on production; worth a slice of its own, on top of this report |
| Match indexes by name | Two servers name the same index differently whenever it was made without a name |

## Consequences

- The drift between two databases is visible in one dialog, without reading DDL.
- One IPC command (`compare_structure`; 104 typed commands).

Limitations:

- **The report only.** Nothing is generated or changed yet.
- **Not compared**: column order, table options (engine, character set, collation of the table), check constraints, triggers, routines, view definitions (views are compared by name), sequences, PostgreSQL column collations.
- **A renamed table or column** shows as one removed and one added.
- **Defaults are compared as the servers write them**: two servers of different versions may write the same default differently (MariaDB quotes string defaults; MySQL 8 does not); a PostgreSQL `serial` default names its sequence, which differs between schemas.
- **SQLite** is not offered.
