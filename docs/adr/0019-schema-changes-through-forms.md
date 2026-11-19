# 0019 — Schema changes through forms: one builder, the statement shown is the statement run

## Status

Accepted. Implemented for the forms of a database on a MySQL or PostgreSQL connection: "New database…" (sections 1 to 6) and, added the same day, "Edit database…", "Drop database…" and MySQL's "Flush" (section 7, which records what the forms that alter and drop needed beyond the first six). Verified in the backend on both engines (unit and live tests) and through the UI in end-to-end stages; what was and was not run is in [TESTING.md](../../TESTING.md), "Results — 2026-11-19 (New database)" and "(Database menu)". On 2026-11-20 the menu of a table or a view followed, on MySQL, PostgreSQL and SQLite: its definition as read-only text, and rename, empty and drop (section 8; "Results — 2026-11-20 (Table menu)"). The same day a form for views followed, on the three engines: "New view…" and "Edit view…" (section 9; "Results — 2026-11-20 (Views through a form)"). It is the one form whose statement holds SQL the user wrote, and section 9 records the rule that takes the place of section 2 for that text. Forms that create a table or an index, or that change a table's columns, are not built. Builds on [ADR 0013](0013-staged-edits-and-optimistic-saves.md) (plan, then apply, from one function).

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
- A Production-labelled connection: a warning and a danger-styled button. No typed confirmation: the statement is on screen and creating an empty database destroys nothing. A form that drops or alters will need more than this. (What it needed is section 7.)
- The server's refusal (the name exists, no such role, no privilege) is shown in its own words and the form stays open.

### 7. Forms that alter and drop (added 2026-11-19)

Sections 1 to 5 hold unchanged for `ALTER DATABASE`, `DROP DATABASE` and `FLUSH`: one builder each (`alter_database`, `drop_database`, `flush`), a `plan_…` command and a command that runs, names as quoted identifiers, keywords by the strict pattern, an option of the other engine refused, the pooled session. Section 6 was not enough, as it said. Decided in addition:

- **A drop is confirmed by typing the database's name, and the backend checks it.** `drop_database` takes the name and the text the user typed, and refuses unless they are equal. The form enables its button on the same comparison, but the rule does not live in the form: there is no request that drops without carrying the confirmation. This is on every connection, not only Production-labelled ones. An edit or a rename has no typed confirmation: it destroys no data, and the statements are on screen.
- **The server's own databases are out of reach.** `ddl::is_protected` names them — MySQL's `information_schema`, `mysql`, `performance_schema` and `sys`; PostgreSQL's `postgres`, `template0` and `template1` — and the builders refuse to alter or drop them. A fixed list in the builder, not a privilege check: the account may well be allowed to. Someone who means it can type the SQL.
- **The session's own database is refused early (PostgreSQL).** A session cannot drop or rename the database it is opened on; the server would say so in its own terms, and PuriCat says it first with what to do instead.
- **PuriCat gets out of its own way, and only its own.** PostgreSQL refuses to drop or rename a database anyone is connected to, and PuriCat is often that someone: a pool per database it has read, a console per query tab. Before a drop or a rename the session closes its pool for the database (`PgSession::release`) and the app releases the consoles of query tabs opened on it (`AppState::release_consoles`). Other clients are not touched: no `WITH (FORCE)`. A form that offered to disconnect other people would be a different decision.
- **An alter is a list of statements.** The builder returns them in the order they run, and the preview shows that list. MySQL needs one. PostgreSQL needs one per kind of change — the owner first, while the database still has the name on screen, then the rename — and they are sent as one simple query, which the server runs as one transaction: a refused rename leaves the owner as it was.
- **The form sends the difference, not the state.** The spec carries only what should change, read against `database_details`, so the statement says exactly what changes; an empty difference is refused by the builder and never offered by the form.
- **Each engine's form says what the engine cannot do.** MySQL cannot rename a database: the name is shown, disabled, with that sentence, and a rename in the spec is refused. A PostgreSQL encoding is fixed at creation: shown, disabled, explained.
- **Unsaved work holds the form back.** A drop, and a rename, are not offered while a tab of the database has unsaved edits or an open transaction. This one is the form's: the backend does not know about staged edits.
- **`FLUSH` is a closed list.** Four kinds, four constant statements; the request names the kind and carries no text. `FLUSH HOSTS` is left out because MySQL 8.4 removed it. The form adds a sentence on what each does, because the menu's word does not say.

Opening a SQL file from the same menus is not a schema change and follows [ADR 0015](0015-sql-console.md): the text lands in a query tab and nothing runs.

### 8. The menu of a table or a view (added 2026-11-20)

Sections 1, 2 and 5 hold unchanged: one builder (`ddl::table_change`) for a rename, an empty and a drop, a `plan_table_change` command and an `apply_table_change` command that calls the builder again, every typed or stored name as a quoted identifier. There are no keywords to match here: the only words that vary — `TABLE`, `VIEW`, `MATERIALIZED VIEW` — follow from a kind the request names out of three. Of section 7 the typed confirmation, the read-only refusal and the hold for unsaved work carry over. Decided in addition:

- **Emptying is confirmed like dropping.** `TRUNCATE` leaves the table standing and takes everything in it; to the person whose rows they were there is no difference. So "Empty table…" asks for the table's name exactly as "Drop table…" does, on every connection, and `apply_table_change` compares the typed text for both. A rename destroys no data and has no typed confirmation, as a database's rename has none.
- **One statement, and nothing that widens it.** No `IF EXISTS`, no `CASCADE`, no `RESTART IDENTITY`. A form that added `CASCADE` would drop objects the statement does not name; one that added `IF EXISTS` would report success for something that did not happen. What stands in the way — a view that depends on the table, a foreign key that points at it, a name that is taken — is the server's to say, and its message is shown.
- **Each engine's statement, with what it does said in the form.** MySQL renames a view with `RENAME TABLE` and names the database on both sides, so the new name cannot land in the session's database. SQLite has no `TRUNCATE`: the builder writes `DELETE FROM`, and the form says that triggers fire. SQLite cannot rename a view: the item is left out and the builder refuses. The Empty form has one paragraph per engine because the three statements are not the same act: an implicit commit and a counter reset on MySQL, counters kept and a refusal for a referenced table on PostgreSQL, a row-by-row delete on SQLite.
- **The statement runs inside the table's database (PostgreSQL).** A database's statements run on the database the session was opened on, because the database in question may not exist yet or must not be connected to. A table's statement can only run from inside its database, so `PgSession::execute_on` takes the pool of the database the table belongs to. Nothing is released first: the server does not ask for it.
- **On SQLite the statement goes to the writable handle, and a file opened read-only is refused by the session**, not only by the command: the refusal belongs where the handle is chosen ([ADR 0018](0018-sqlite-files.md)).
- **No list of protected tables.** The database forms refuse the server's own databases by name. For tables no such list was written: which tables are the server's own differs per engine and version, and the typed name, the account's privileges and the server are what stands between a user and `mysql.user`. This is a gap stated, not a decision that none is needed.
- **The row count in the form is the estimate, labelled "about".** An exact count can take long on the very tables where the number matters; the form reads what the engine keeps and shows nothing where it keeps nothing.

The definition is not a change and needed its own decision:

- **Each engine's own text where it has one.** MySQL prints a table or a view with `SHOW CREATE`; SQLite keeps the text of every object in the file. PuriCat shows that text and adds only, for SQLite, the statements of the table's indexes and triggers, which live in separate rows of the same catalogue.
- **PostgreSQL has no such statement, so PuriCat writes one — and says so.** The alternatives were to offer nothing on PostgreSQL, or to run `pg_dump --schema-only` for one table. The first leaves the engine where a definition is hardest to come by without one. The second needs a dump tool of the right version on the computer ([ADR 0021](0021-backups-with-dump-tools.md)) and a process per click, for a dialog that should open at once. The text is therefore put together from the catalogue, with the server's own `pg_get_…` functions wherever one exists (constraints, indexes, triggers, view queries, default and generation expressions, the partition key) and PuriCat's own words only for the `CREATE` line, the column list and the `COMMENT ON` lines around them.
- **A reconstruction is labelled as one.** The response carries where the text comes from (`server`, `file`, `catalogue`), and the dialog turns that into a sentence under the text. For `catalogue` the sentence says that PuriCat put the text together, names what is left out (owner, grants, row-level-security policies, storage parameters, inheritance) and names `pg_dump` as the authority. Without the label a user would take the text for the server's, as it is on the other two engines.
- **Its test is a round trip.** Asserting pieces of the text would only show that the text is what its author expected. The live test runs the text in a second database and requires the table made there to have the same definition and the same columns as `describe_table` reports them. That shows the text is valid SQL and a fixed point for one sample table; it does not show completeness, and the known limits are listed in [DATABASE_COMPATIBILITY.md](../../DATABASE_COMPATIBILITY.md), "Table menu".
- **Read-only text, with two ways out.** "Copy", and "Open in query tab", which seeds a query tab on the table's database and runs nothing, as for a SQL file. The definition is available on a read-only connection.

### 9. The view form: the one statement with the user's SQL in it (added 2026-11-20)

A view is a name and a query. Sections 1 and 5 hold: one builder (`ddl::view_statements`), a `plan_view` command that returns the statements and a `save_view` command that builds them again and runs them; the name is a quoted identifier as section 2 says. The query is where section 2 ends. It is not an identifier and not a keyword: it is SQL, and the form exists so that the user can write it. "Typed text has two places it can go, and no third" is therefore not true of this form, and the record says so here instead of stretching the rule.

- **The form holds the query alone; the statement around it is the backend's.** The user writes what follows `AS`. `CREATE VIEW <name> AS` — or `CREATE OR REPLACE VIEW` — is written by the builder, shown as it will run and run by the same code. So the words of the statement are still never the web view's, and what the web view does send as SQL has one place to go: after `AS`.
- **The rule for the query: exactly one statement, and one that reads.** `ddl::view_query` refuses an empty text, applies `script::single_read`, and requires the first word to be `SELECT`, `WITH`, `VALUES` or `TABLE` (`SHOW` and `EXPLAIN` pass `single_read`, and no view is made of them). Anything else is refused before a statement exists. A final semicolon is removed and the rest is taken as written, comments included. What the query may read is not PuriCat's to judge: the server and the account decide.
- **Why `single_read`, as it stands.** It is the test the read-only query path makes for the AI's query tool ([ADR 0017](0017-ai-database-tools.md)): is this text one statement that only reads. It exists, with tests on the three dialects, and a text that passes it is a text that path will run — which is what makes the preview below possible. It is used unchanged, so the form inherits rules made for another purpose: a locking clause, the word `INTO`, a MySQL version comment, a function name from that check's list, and on SQLite any semicolon inside the text, even in a string, are refused although a view could hold some of them. Such a view is typed as SQL in a query tab; the refusal says what was not accepted.
- **The same check is what lets the form preview.** A text that passes is, by the same test, a text the read-only path will take. "Preview rows" runs the query — not the view's statement — through `Console::read_only`, exactly as the AI's tool does: a connection of its own, the server's prepare, a `READ ONLY` transaction that is rolled back; on SQLite a handle that cannot write. So a preview cannot write, whatever the text check missed, and nothing has to exist for the user to see the rows. It is capped at 100 rows and cancelled after 15 seconds: it is there to show that the query is the right one, not to be a result grid.
- **MySQL: `USE` first, on a connection of its own.** The statement names the view with its database, but MySQL resolves the tables a view's query names without a database against the session's default database, not against the view's. The pooled session has whatever default the profile gave it. So the save takes a connection out of the pool, issues ``USE `db` ``, runs the statement and closes the connection — detached, so that the chosen database does not stay behind on a connection the pool hands to someone else. The preview runs on a console opened on the same database, so both read the same names. PostgreSQL has no such step: the statement runs inside the view's database with the connection's search path, and the form says that a table off the search path needs its schema.
- **SQLite: drop and create, in one transaction.** SQLite has no `OR REPLACE` for a view. The builder returns two statements for an edit — `DROP VIEW`, then `CREATE VIEW` — the form shows both, and `SqliteSession::execute_all` runs them in a transaction that is rolled back when either fails, so a query SQLite will not take leaves the old view in place. As for a database's alter (section 7) the builder returns a list and the preview shows the list; here the list is run statement by statement inside an explicit transaction.
- **The edit form opens on the query as the server keeps it, and says whose text that is.** PostgreSQL prints it (`pg_get_viewdef`); SQLite keeps the statement as typed, and the query is cut out of it after the first `AS` outside parentheses, strings and comments; MySQL keeps only its own rewriting, with every name qualified, and the form says so under the editor — otherwise the user would take a text they did not write for a fault of the form. The name is fixed in the edit form: a rename is another statement, and the view's menu has it.
- **No typed confirmation.** The rule of sections 7 and 8 is that what destroys data is confirmed by typing a name. Giving a view another query, or creating one, destroys no rows, and the statement is on screen; it is treated as a database edit and a rename are. What it can break is whatever reads the view; the form says that, and that PostgreSQL will not let `OR REPLACE` remove, rename or retype a column.
- **A plain view only.** A materialized view holds data and has other statements; "Edit view…" is not in its menu and `view_select` does not find it. Options — algorithm, definer, security, check option — are not in the form, and the statement names none.
- **Read-only, Production, the server's refusal:** as section 6. `save_view` refuses a read-only profile, the session and the server refuse on their own, and the preview stays available because it only reads.

## Alternatives considered

- **Build the SQL in the form and send it to `run_sql`.** No new backend code, and the console's history for free. Rejected: the UI would build SQL from typed text, and the preview would be whatever the web view chose to show.
- **Send the previewed text back to be run.** Rejected for the same reason as in ADR 0013 and ADR 0017: the backend runs its own statement, never one that crossed IPC.
- **Ignore an option that does not belong to the engine.** Rejected: see 3.
- **Fixed lists of character sets and encodings in the app.** Wrong on some server; see the context.
- **Through the console path**, as ARCHITECTURE.md had planned for object editing. See 5. Open again for a statement that can run long and needs Stop.
- **A typed confirmation on Production** (type the database name). Not added for a statement that destroys nothing; open again for forms that drop or alter. Decided in section 7: typed for a drop, on every connection; not for an alter.
- **Check the typed name in the form only.** Rejected: the one destructive request PuriCat's forms can make should not depend on a button's state.
- **For a PostgreSQL definition, `pg_dump --schema-only --table`, or nothing at all.** See section 8. Open again if the reconstruction's limits turn out to matter in use.
- **`CASCADE` as a checkbox in the Drop form.** Not offered: see section 8. Someone who means it can type the SQL.
- **A check of its own for a view's query, milder than the reading tool's.** Not written: section 9 uses `single_read` as it stands. Open again if the refusals of harmless queries turn out to matter in use.

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

Added with section 7:

- PuriCat's forms can now destroy data. The guard is the typed name, enforced in the backend; there is no backup first and no undo.
- The typed name stops a slip, not a script: code running in the web view could send the name twice, as it could already send `DROP DATABASE` to `run_sql`.
- Releasing consoles is done without asking the backend's opinion of the tab: an open transaction there is rolled back with its connection, a running statement is interrupted. The form holds back for unsaved edits and open transactions; it is the only place that does.
- The list of protected databases is a constant. A server with other databases of its own (a managed service's, say) is not covered, and the explorer hides the menu items only where its listing marks a database as the server's.
- Still not recorded, still no Stop: `FLUSH TABLES` or a drop that waits can only be waited for.
- Not verified for section 7: the commands' own checks (read-only, the typed name, releasing the consoles of an open query tab), a drop or rename while another client is connected to a PostgreSQL database, an owner change and the Flush kinds other than Status through the UI, a read-only or Production-labelled connection through the UI, Thai and the dark theme, the bundled app, other server versions.

Added with section 8:

- The forms can now destroy the rows of one table as well as a whole database, on three engines. The guard is the same typed name; there is still no backup first and no undo.
- **PuriCat now shows SQL it wrote that it does not run.** The PostgreSQL definition of a table is the first text of this kind whose correctness nothing checks at the moment of use: a statement that is run is judged by the server, a definition is only read. The label in the dialog and the round-trip test are what stands behind it.
- Known limits of that text: a `serial` column prints as `DEFAULT nextval('…')` without its sequence; the options of an identity sequence are not printed; a partition prints as a plain table, not as `PARTITION OF`; `PARTITION BY` is printed for a partitioned parent and has no test; a foreign table is listed under Tables and has no definition. Each new PostgreSQL feature that changes what a table is will have to be added by hand.
- The round trip covers one sample table. An unlogged table, an exclusion or unique constraint and a partitioned table are in the code and in no test.
- The length rule of section 2 is applied to a new table name on SQLite too (64 characters), where the engine has no such limit.
- After a rename the table's tab closes; the form does not open the table again under its new name.
- Only the table's own tab is looked at: a query tab with an open transaction on the table neither holds a form back nor is told afterwards.
- Still not recorded, still no Stop.
- Not verified for section 8: `apply_table_change`'s own checks (the typed name, the read-only refusal); SQLite, views and materialized views through the UI; a rename, an empty or a drop on PostgreSQL through the UI; "Copy"; unsaved edits holding a form back; a read-only or Production-labelled connection through the UI; Thai and the dark theme; the bundled app; other server versions.

Added with section 9:

- **The injection defence for DDL is no longer two rules.** For every other form it still is. For the view form it is a third thing: a text scanner that decides whether SQL the user wrote is one reading statement. That is a weaker kind of guarantee than quoting, and it is confined to one function, `ddl::view_query`.
- **At the save the scanner stands alone.** The reading tool, for which `single_read` was written, has the server's prepare and a read-only transaction behind it; `single_read`'s own comment calls it a first filter. The preview has both. The save cannot have the transaction, and has no prepare: the statement goes to the server as text. Where the scanner and a server disagree about the end of a string — known for a MySQL session with `NO_BACKSLASH_ESCAPES`, from the tool's own live test — the text could be more than one statement to the server. Read from the code; not tried on the save path. The text is the user's own and on screen, so nothing reaches the server that the user did not write and could not have run in a query tab; but "exactly one statement" is then the scanner's opinion, not a fact. A step in which the server confirms that the text is one statement, as the reading path has in its prepare, is not built for the save.
- **A view's query can still call a function that writes.** The check reads the text of the query, not what its functions do. The preview's transaction stops that (verified on PostgreSQL); the view, once created, runs with whatever it calls.
- **Harmless queries are refused**: a semicolon in a SQLite string, a name that contains one of the listed function names, `FOR UPDATE`. Listed in [DATABASE_COMPATIBILITY.md](../../DATABASE_COMPATIBILITY.md), "Views through a form".
- **An edit replaces the whole statement.** On MySQL the new statement names no algorithm, definer or security option, whatever the view had; what the server makes of that was not tried. On SQLite the drop takes the view's `INSTEAD OF` triggers with it (seen outside PuriCat, with the `sqlite3` of macOS; in no test), and the form, which says that the view is dropped and created again, does not say that.
- On MySQL the user never gets back the text they typed, only the server's rewriting; saving the edit form unchanged creates the view from that rewriting.
- The preview shows what the connected account reads now, from the view's database. It is not a promise about what the view returns to another account or session.
- **No list of protected names here either**: "New view…" is left out of the menu of a database or schema the listing marks as the server's own, but the "Views" group inside one has it, and the views there have "Edit view…".
- The preview has no Stop of its own; it ends after 15 seconds at the latest. The save has none, as before. Still not recorded.
- The form attaches its editor object to its `<form>` element as `__editor` in every build, as a query tab does, so that the end-to-end test can set the text.
- Not verified for section 9: `save_view`'s own read-only refusal; a read-only session on MySQL (PostgreSQL's and SQLite's refusals are tested); the 15-second limit (no test waits for it) and the limit of 100 rows (the tests cut at 2); PostgreSQL and SQLite through the UI; "New view…" from a schema and from the "Views" group; through the UI a read-only or Production-labelled connection, a preview that fails, the "query changed since this preview" state and a refusal by the server on save; real typing and the suggestion list in the form's editor; an open tab of an edited view reading again; MySQL's answer for an account without `SHOW VIEW`; Thai and the dark theme; the bundled app; other server versions.

## Licensing

No new dependency.
