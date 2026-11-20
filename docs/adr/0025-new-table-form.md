# 0025 — A new table through a form: types from a list, the rest as the database forms do it

## Status

Accepted. The first slice of the table designer: creating a table. Changing an existing table's columns, indexes and foreign keys is not built. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-20 (new table)".

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
- Three IPC commands (`column_types`, `plan_create_table`, `create_table`). No new dependency, capability or CSP change.

Limitations:

- **No change to an existing table** through a form yet: add, change, rename or drop a column; indexes; foreign keys. Those are the next slices.
- **A default is a string literal**: a number given as `'0'` is converted by the server, which every type here accepts; a boolean default on PostgreSQL must be written as the server reads it (`true`, `f`, …).
- **MySQL's `TIMESTAMP` and `DATETIME` without a fraction**; no `ON UPDATE CURRENT_TIMESTAMP`.
- **SQLite** has no table form.
- The list of types is fixed in the code; a server's own types are not read.
