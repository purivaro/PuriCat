# 0019 — Schema changes through forms: one builder, the statement shown is the statement run

## Status

Accepted. Implemented for one form: "New database…" on a MySQL or PostgreSQL connection. Verified in the backend on both engines (unit and live tests) and through the UI in one end-to-end stage; what was and was not run is in [TESTING.md](../../TESTING.md), "Results — 2026-11-19". Forms for tables, indexes and views, and anything that alters, renames or drops, are not built. Builds on [ADR 0013](0013-staged-edits-and-optimistic-saves.md) (plan, then apply, from one function).

## Context

Until this record every statement that changed a schema was SQL a person typed in a query tab, or SQL a model proposed and a person approved ([ADR 0015](0015-sql-console.md), [ADR 0017](0017-ai-database-tools.md)). Object editing through forms is a Phase 4 requirement, with the DDL shown before it runs and engine differences explained, not hidden. Creating a database is the smallest such form, and the first statement of this kind that PuriCat writes itself.

Constraints:

- The UI must not build SQL ([ADR 0002](0002-core-crate-and-thin-ipc.md)).
- DDL takes no bind parameters. Everything a user types or chooses ends up in the statement's text.
- Some of it is not an identifier at all: a character set, a collation and an encoding are keywords to the server, so the identifier quoting of `db::ident` does not apply to them.
- What a server offers (character sets, collations, roles, templates, encodings) differs per server and per version. A list written into the app would be wrong somewhere.
- MySQL and PostgreSQL share the words `CREATE DATABASE` and little else. A SQLite connection is one file: it has no databases to create.

## Decision

### 1. One builder per form, used for the preview and for the run

`db::ddl::create_database(engine, spec)` is the only place the statement is written. `plan_create_database` returns its text and runs nothing; `create_database` calls the same function again with the same spec and runs the result. The web view sends the spec — a name and options — in both calls and never sends SQL, so there is no statement for it to alter between reading and running. Further forms follow the same shape: a spec, one builder, a `plan_…` command and a command that runs.

### 2. Typed text has two places it can go, and no third

- **A quoted identifier**, through `db::ident` (the quote character doubled, NUL refused): the name, and on PostgreSQL the owner and the template. Any text is then one name, including spaces, Thai letters and the quote character.
- **A keyword that matches a strict pattern**: character set, collation, encoding. ASCII letters, digits, `_` and `-`, at most 64 characters; anything else is refused before a statement exists. PostgreSQL's encoding is additionally written inside single quotes, which the pattern cannot contain.

The name is trimmed; an empty name and one that is too long are refused: more than 64 characters on MySQL, more than 63 bytes on PostgreSQL, which would otherwise shorten the name with only a notice and create a database the statement did not name. The server keeps the last word on what a valid name is.

### 3. Each engine has its own options, and a wrong one is an error

The spec carries MySQL's options (character set, collation) and PostgreSQL's (owner, template, encoding). An option that belongs to the other engine is refused, not dropped: a silent drop would run a statement that is not what was asked for. SQLite and SQL Server are refused with the reason. An option left empty adds nothing to the statement, so the server's default applies and the preview shows exactly that.

### 4. The lists come from the server

`database_options` reads them when the form opens: character sets and collations from `information_schema`, with the server's defaults; roles, template databases and `template1`'s encoding from the PostgreSQL catalogue. The first entry of every list means "leave it to the server". One list is not read from the server as a whole: PostgreSQL has no catalogue of encodings, so the names are asked for by id (`pg_encoding_to_char`) up to the last encoding a database can be stored in, a constant of PostgreSQL's source (34).

### 5. Run as one statement on the pooled session, not through the console

`CREATE DATABASE` is one short statement that changes no rows: the console's impact guard, row cap and statement splitting have nothing to do for it, and on PostgreSQL it may not run inside the transaction a guard would open. It runs with `sqlx::raw_sql` on a pooled connection — on PostgreSQL to the database the session was opened on, since the new one does not exist yet.

### 6. The guards are the session's and the server's

- A connection saved as read-only: the form disables its button, the command refuses, and the server refuses on its own because the pooled connections are read-only sessions.
- A Production-labelled connection: a warning and a danger-styled button. No typed confirmation: the statement is on screen and creating an empty database destroys nothing. A form that drops or alters will need more than this.
- The server's refusal (the name exists, no such role, no privilege) is shown in its own words and the form stays open.

## Alternatives considered

- **Build the SQL in the form and send it to `run_sql`.** No new backend code, and the console's history for free. Rejected: the UI would build SQL from typed text, and the preview would be whatever the web view chose to show.
- **Send the previewed text back to be run.** Rejected for the same reason as in ADR 0013 and ADR 0017: the backend runs its own statement, never one that crossed IPC.
- **Ignore an option that does not belong to the engine.** Rejected: see 3.
- **Fixed lists of character sets and encodings in the app.** Wrong on some server; see the context.
- **Through the console path**, as ARCHITECTURE.md had planned for object editing. See 5. Open again for a statement that can run long and needs Stop.
- **A typed confirmation on Production** (type the database name). Not added for a statement that destroys nothing; open again for forms that drop or alter.

## Consequences

- The promise "you read the SQL before it runs" now covers a schema change PuriCat writes, by construction: one function, called twice.
- The whole injection defence for DDL is two small rules with unit tests. Every later form must put each typed value in one of the two places.
- **Not recorded.** The statement goes neither to the SQL history, which is the console's, nor to an audit log, which does not exist for saves either. Stated as a gap in [SECURITY.md](../../SECURITY.md).
- **The preview trails the form.** It is asked for 120 ms after the last change; until the answer arrives the previous statement is shown dimmed and the button is disabled, so only the statement on screen can be run.
- **The backend enforces the builder and the read-only flag; the Production warning is the form's.**
- The 63-byte limit for PostgreSQL assumes a server built with the usual `NAMEDATALEN`.
- The encoding cut-off is a constant and was checked on PostgreSQL 16.13 only.
- Creation only, with the common options: no locale, tablespace, connection limit or `IS_TEMPLATE` on PostgreSQL, no `ENCRYPTION` or `READ ONLY` on MySQL ([DATABASE_COMPATIBILITY.md](../../DATABASE_COMPATIBILITY.md), "New database").
- No Stop on this path. Acceptable for `CREATE DATABASE`; to be decided again for a form whose statement can run long.
- Not verified: a read-only or Production-labelled connection through the UI, the bundled app, an account without the privilege, Thai and the dark theme, other server versions.

## Licensing

No new dependency.
