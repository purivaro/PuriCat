# 0025 — Tables through a form: a new table, and changes to one

## Status

Accepted. Three slices of the table designer: creating a table (section "Decision"); adding, renaming and dropping columns and indexes of an existing one (section "Changing a table"); and changing a column's type, NULL, default and comment, and foreign keys (section "Changing a column, and foreign keys"), all added on 2026-11-20. The primary key and column order are not built. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-20 (new table)".

Verified on MySQL 9.3.0 and PostgreSQL 16.13 (live tests on both; through the window on MySQL).

## Context

Creating a table took typed SQL in a query tab. A database tool the owner uses has a table designer, and the owner asked for what it has and PuriCat lacks (2026-11-20). [ADR 0019](0019-schema-changes-through-forms.md) set the rules for forms that change a schema: one builder for the preview and the run, names quoted as identifiers, nothing else typed reaching a statement except through a strict pattern. A table form needs two more kinds of input: a column's type, and values (a default, a comment).

## Decision

1. **"New table…"** in the menu of a MySQL database, of a PostgreSQL schema, and of the "Tables" group. The form: a name, an optional comment, and rows of columns — name, type, length or precision and scale, NULL, self-numbering (MySQL `AUTO_INCREMENT`, PostgreSQL identity), MySQL `UNSIGNED`, default, comment, primary key — that can be added, moved and removed. It starts with an `id` that numbers itself and is the key.
2. **A type is chosen from a fixed list per engine** (`table_design::types`), with numbers for what it takes in parentheses. The backend checks the name against the list and the numbers against their range; no typed text becomes a type. The list goes to the form from the backend, so the two agree.
3. **Values are string literals** (a default, a comment): the quote doubled, and NUL and backslash refused, so the literal reads the same whatever MySQL's `sql_mode` or PostgreSQL's `standard_conforming_strings`. A default is none, NULL, a value, or `CURRENT_TIMESTAMP` (date-and-time types only); no other expression.
4. **What the server would refuse anyway is refused first, with the reason**: a NULL default on a NOT NULL column, a default on a self-numbering column, self-numbering on a non-integer, more than one, or (MySQL) one that is not the first column of the primary key; two columns of the same name; a name longer than the engine allows (PostgreSQL would shorten it). A key column is NOT NULL whatever the form says.
5. **One builder, one run.** `plan_create_table` returns the statements; `create_table` builds them again from the same spec and runs them: on MySQL one `CREATE TABLE` with inline comments; on PostgreSQL the `CREATE TABLE` and a `COMMENT ON` per comment, sent as one simple query, which the server runs as one transaction. A read-only connection is refused; a Production-labelled one gets a warning and a danger-styled button, as the database form does.
6. After it is made the explorer shows it and its tab opens.

## Changing a table (added 2026-11-20)

7. **"Change table…"** in a table's menu (MySQL, PostgreSQL). The form shows the table's columns and indexes as the server has them, and collects changes: rename or drop a column; add columns (the same column editor as the new-table form, without the key and self-numbering); drop an index; make an index (optional name, unique or not, columns in the order they are picked, from the columns the table will have).
8. **The changes are one unit.** MySQL: one `ALTER TABLE` with a clause per change, which the server applies as one or not at all. PostgreSQL: one statement per change, sent as one simple query, so one transaction (the live tests show the first change undone when a later one fails). The order keeps every name valid: columns added and renamed, indexes dropped, columns dropped, indexes made.
9. **Dropping a column is confirmed by typing the table's name**, and the backend checks it (`alter_table` refuses without it), as dropping a database is confirmed. Dropping an index destroys no data and needs no typing. A column of the primary key is not offered for dropping; an added column cannot join the key or number itself (the form says to use a query tab). An index that includes a dropped column is named in a warning: the server changes or drops it.
10. A column changed twice in one go is refused: one change per column.

## Changing a column, and foreign keys (added 2026-11-20)

11. **"Change" on a column that is there** opens its fields: a new type (or "keep" with the current type shown), NULL, the default (keep, none, a value, NULL, `CURRENT_TIMESTAMP`) and the comment. Only what differs from the server's definition is sent (`TableEdit::ChangeColumn`, each field optional). A column is renamed or changed in one go, not both; a generated or identity column is not offered.
12. **MySQL: the form reads the full definition first.** `MODIFY COLUMN` rewrites a column whole, so `column_definitions` reads what the server has (`COLUMN_TYPE`, character set and collation, NULL, the default and whether it is an expression, `EXTRA`, the comment) and the statement says again everything the change leaves alone: a kept type is the server's own `COLUMN_TYPE` text, and `AUTO_INCREMENT` and `ON UPDATE CURRENT_TIMESTAMP(n)` stay. A column whose `EXTRA` or default is something else (a generated column, an expression default other than `CURRENT_TIMESTAMP`) is refused with the reason rather than rewritten. The preview is built from the same read (`plan_alter_table` reads the definitions too).
13. **PostgreSQL: one statement per property** (`ALTER COLUMN … TYPE … USING col::type`, `SET/DROP NOT NULL`, `SET/DROP DEFAULT`, `COMMENT ON COLUMN`). A type change converts the values with a cast; the server cannot carry a default across it, so the default is dropped first and the change must say what it becomes (the form asks for it). The live test shows a failed conversion undoing the earlier changes of the same unit.
14. **Foreign keys**: the table's keys are listed (columns, referenced table and columns, `ON DELETE` / `ON UPDATE`) and can be dropped (`DROP FOREIGN KEY` on MySQL, `DROP CONSTRAINT` on PostgreSQL). A new one has an optional name, columns of the table (as it will be), a table of the same database (and schema) and its columns, picked in order, and `NO ACTION`, `RESTRICT`, `CASCADE` or `SET NULL` for each action. It is left out of the change until both sides are chosen, and the apply button waits for it. The order of the unit: keys dropped first, keys made last, so a key may name a column the same change adds or changes.

Not built: the primary key; column order (MySQL `AFTER`); `SET DEFAULT` as a key action (shown when the server has it, not offered); keys to a table in another database or schema.

## Alternatives considered

| Option | Why not |
|---|---|
| A type typed freely | It would reach the statement as SQL; and a misspelt type is the server's error at the end instead of a choice at the start |
| Every type of each engine | The list is the common ones; the rest (enums, arrays, spatial types, domains) are typed in a query tab, which the form says nothing against |
| Expressions as defaults | An expression is SQL; `CURRENT_TIMESTAMP` covers the case that is asked for most |
| Indexes, foreign keys and checks in the first form | More than one slice; the key is the one constraint every table needs |
| A table's character set, collation and engine (MySQL) | The database's defaults apply; noted as a follow-up |

## Consequences

- A table with the usual columns is made without typing SQL, and the statement is read before it runs.
- Eight IPC commands (`column_types`, `plan_create_table`, `create_table`, `table_indexes`, `plan_alter_table`, `alter_table`, `column_definitions`, `foreign_keys`). No new dependency, capability or CSP change.

Limitations:

- **The primary key and column order** are not changed by the form; a query tab does it.
- **A type change on PostgreSQL is a plain cast** (`USING col::type`); a conversion that needs an expression is typed in a query tab.
- **A default is a string literal**: a number given as `'0'` is converted by the server, which every type here accepts; a boolean default on PostgreSQL must be written as the server reads it (`true`, `f`, …).
- **MySQL's `TIMESTAMP` and `DATETIME` without a fraction**; no `ON UPDATE CURRENT_TIMESTAMP`.
- **SQLite** has no table form.
- The list of types is fixed in the code; a server's own types are not read.
