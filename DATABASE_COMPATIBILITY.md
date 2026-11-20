# Database compatibility

Status on 2026-11-19. Labels: **Implemented & verified** (a test against a real server passed), **Implemented, not yet verified**, **Planned (Phase N)**.

## Tested versions

| Engine | Tested version | Other versions |
|---|---|---|
| MySQL | 9.3.0 (local) | Untested, including 8.x, 5.7 and MariaDB |
| PostgreSQL | 16.13 (local) | Untested |
| SQLite | 3.46.0 (the library compiled into the app); files made by that same library | Files written by other versions or other programs: untested |
| SQL Server | — | No adapter |

Only these versions have been exercised. Nothing is claimed for older, newer or compatible-protocol servers (MariaDB, Aurora, CockroachDB and similar).

## Feature matrix

SQLite has its own section, "SQLite — a database file", further down: what applies to a file differs enough from a server that one more column here would mislead.

| Feature | MySQL | PostgreSQL | SQL Server |
|---|---|---|---|
| Connect with user name and password | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Fast, specific connection errors | Implemented & verified (refused, unknown user) | Implemented & verified (refused, unknown database) | Planned (Phase 2b) |
| TLS `prefer` (default) | Implemented & verified (local; session encrypted) | Implemented & verified (local; session not encrypted) | Planned (Phase 2b) |
| TLS `disable` / `require` / `verify-ca` / `verify-full`, custom CA | Implemented, not yet verified | Implemented, not yet verified | Planned (Phase 2b) |
| Server-enforced read-only session | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| SSH tunnel | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
| Several open connections at once, mixed engines | Implemented & verified | Implemented & verified | — |
| List databases | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| List schemas | Not applicable (no schemas inside a database) | Implemented & verified | Planned (Phase 2b) |
| List tables and views | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Materialized views | Not applicable | Implemented & verified (listed) | — |
| Procedures, functions, indexes in the explorer | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
| Describe table (columns, types, defaults, primary key, generated flag) | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Browse rows: page, sort | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Browse rows of a view | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Filter rows by conditions on columns (see "Row filter") | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Composite primary keys, names needing quotes | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| One cell read whole, by the row's primary key (a value the page shows shortened) | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Exact row count, asked for by the user and stoppable (of the table, or of the rows a filter leaves) | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Keyset pagination | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
| Cancellation of browse and catalogue queries; reconnect of the pooled session | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
| Edit rows of a table with a primary key (UPDATE by complete key) | Implemented & verified | Implemented & verified | Planned (Phase 3) |
| Insert rows (omitted columns take server defaults) | Implemented & verified | Implemented & verified | Planned (Phase 3) |
| Delete rows (by complete key) | Implemented & verified | Implemented & verified | Planned (Phase 3) |
| Composite primary keys on the write path | Implemented & verified | Implemented & verified | Planned (Phase 3) |
| 64-bit integer keys beyond 2^53 address exactly one row | Implemented & verified | Implemented, not yet verified (typed `bigint` comparison; no test with such a key) | Planned (Phase 3) |
| Optimistic concurrency on edited columns, whole save rolled back on conflict | Implemented & verified | Implemented & verified | Planned (Phase 3) |
| Invalid input rejected instead of coerced | Implemented & verified (session strict mode) | Implemented & verified (assignment rules) | Planned (Phase 3) |
| Write refused by the server in a read-only session | Implemented & verified | Implemented & verified | Planned (Phase 3) |
| Edit views, materialized views, keyless tables | Not offered (read-only in the UI) | Not offered (read-only in the UI) | — |
| Direct paste from a spreadsheet into the grid | Implemented & verified (E2E) | Implemented, not yet verified on this engine (same client code; E2E pastes into MySQL only) | Planned (Phase 3) |
| Smart Paste: unique-key discovery (primary key, plain unique indexes, composite keys) | Implemented & verified. Unique indexes over a column prefix are excluded (verified); over an expression: excluded, not yet verified | Implemented & verified. Partial and expression unique indexes are excluded (verified) | Planned (Phase 3) |
| Smart Paste INSERT (optional check that the key does not exist yet) | Implemented & verified (live test); not through the UI | Implemented & verified (live test); not through the UI | Planned (Phase 3) |
| Smart Paste UPDATE (unmatched rows skipped, never inserted) | Implemented & verified (live test); not through the UI | Implemented & verified (live test); not through the UI | Planned (Phase 3) |
| Smart Paste UPSERT | Implemented & verified (live test, and E2E through the UI) | Implemented & verified (live test); not through the UI | Planned (Phase 3) |
| Smart Paste: matching on a composite key | Implemented & verified (live test); not through the UI | Implemented & verified (live test); not through the UI | Planned (Phase 3) |
| Smart Paste: key equality decided by the database | Implemented & verified (case-insensitive collation: `c3` and `C3` are one key) | Implemented & verified (case-sensitive: `c3` and `C3` are two keys) | Planned (Phase 3) |
| Smart Paste: one transaction with progress, cancel and rollback | Implemented & verified (live test; not through the UI) | Implemented & verified (live test; not through the UI) | Planned (Phase 3) |
| Imports of more than 10,000 rows in batches; CSV / XLSX files | Planned (Phase 4) | Planned (Phase 4) | Planned (Phase 4) |
| Run a script in a query tab: own connection, statement by statement, several result sets, exact text values | Implemented & verified (live test; E2E through the UI) | Implemented & verified (live test); not through the UI | Planned (Phase 4) |
| Session state kept for the life of the tab (`SET`, temporary tables, user variables) | Implemented & verified (`SET @v`, `USE`) | Implemented & verified (`CREATE TEMP TABLE`) | Planned (Phase 4) |
| Statement splitting by the engine's lexical rules | Implemented & verified (unit tests: backticks, `#` comments, backslash escapes, `--` needing a space) | Implemented & verified (unit tests: dollar quotes, nested block comments, `E''` strings; a `DO $$ ... $$` block in the live test) | Planned (Phase 4) |
| Impact guard: a row-changing statement above the row threshold is rolled back | Implemented & verified (live test; E2E) | Implemented & verified (live test) | Planned (Phase 4) |
| A transaction the user opens spans runs and is left alone by the guard | Implemented & verified (live test, also with autocommit off; E2E) | Implemented & verified (live test, including an error inside the transaction) | Planned (Phase 4) |
| Row cap per result; a plain read is cancelled on the server at the cap | Implemented & verified (live test) | Implemented & verified (live test; a capped `UPDATE ... RETURNING` still commits; a capped read inside a transaction does not abort it) | Planned (Phase 4) |
| Stop a statement running in a query tab | Implemented & verified in the backend (`KILL QUERY`); not through the UI | Implemented & verified in the backend (`pg_cancel_backend`); not through the UI | Planned (Phase 4) |
| A query tab's lost connection is reported; the next run opens a new one | Implemented & verified in the backend (`KILL <id>` from another connection) | Implemented & verified in the backend (`pg_terminate_backend`) | Planned (Phase 4) |
| Choose another database for a query tab | Implemented: the selector makes the next run open a new connection on that database (no automated test); `USE` inside a script is verified (live test) | Not available: a tab stays on the database it was opened on. Planned (Phase 4) | Planned (Phase 4) |
| Export a result to CSV, JSON, XLSX | Implemented & verified (unit tests; CSV in the live test; all three in E2E) | Implemented & verified (unit tests; CSV in the live test); not through the UI | Planned (Phase 4) |
| Create a database from a form that shows the exact statement (see "New database") | Implemented & verified (unit and live tests; through the UI in the E2E stage `new-database`, with a character set and a collation, and the same name refused) | Implemented & verified (unit and live tests; through the UI in the E2E stage `new-database`, with a template and an encoding) | Planned (Phase 4) |
| Edit a database from a form that shows the exact statements (see "Database menu") | Default character set and collation. Implemented & verified (unit and live tests; a collation change through the UI in the E2E stage `database-edit`). A database cannot be renamed on MySQL | Name and owner. Implemented & verified (unit and live tests; a rename through the UI in the end-to-end test, after the stage `database-edit`). The owner change is not verified through the UI; the encoding cannot be changed | Planned (Phase 4) |
| Drop a database after typing its name (see "Database menu") | Implemented & verified (unit and live tests); not through the UI on this engine | Implemented & verified (unit and live tests; through the UI in the E2E stage `database-drop`) | Planned (Phase 4) |
| Open a SQL file in a new query tab; `FLUSH` from the connection's menu (see "Database menu") | Open SQL file: implemented & verified through the UI for a small file (E2E). Flush: implemented & verified for Privileges, Tables and Status (live test), Status through the UI (E2E); `FLUSH LOGS` accepted once by hand | Open SQL file: implemented, not yet verified on this engine. Flush: not available, it is a MySQL statement | Planned (Phase 4) |
| The definition of a table or a view, as read-only text (see "Table menu") | The server's `SHOW CREATE TABLE` / `SHOW CREATE VIEW` text. Implemented & verified (live test; a table through the UI in the E2E stage `table-definition`, also opened in a query tab) | A view's stored query under a `CREATE` line; a table's text put together by PuriCat from the catalogue, with what that leaves out. Implemented & verified (live test: one sample table, whose text run in a second database gives the same text back; a view and a materialized view; `shop.orders` through the UI in the end-to-end test) | Planned (Phase 4) |
| Rename, empty or drop a table; rename or drop a view — each with its statement shown, empty and drop after the name is typed (see "Table menu") | Implemented & verified (unit and live tests; a table through the UI: renamed, emptied and dropped in the end-to-end test, stage `table-drop`). A view through the UI is not verified | Implemented & verified (unit and live tests, materialized views included). Not through the UI on this engine | Planned (Phase 4) |
| Create a table, an index or a view through a form; change a table's columns, keys or indexes | Planned (Phase 4) | Planned (Phase 4) | Planned (Phase 4) |
| Import from CSV / XLSX, SQL dump export, EXPLAIN view | Planned (Phase 4) | Planned (Phase 4) | Planned (Phase 4) |
| Schema context for the agent chat (engine and version, database, tables and views with column names, types, key / NOT NULL / generated flags, row estimates; no rows) | Implemented, not yet verified on this engine (the same code reads the catalogue as for the structure view; no test reads the text of a MySQL context. The E2E stage `agent-mysql` sends one turn with a MySQL context and asserts only the context button's label) | Implemented & verified (unit test of the text; E2E: the context inspector and the prompt handed to the stand-in CLI, for a table tab in a non-`public` schema) | Planned |
| Database tools for the AI, off by default: catalogue lookups (`list_tables`, `describe_table`; `list_schemas` where the engine has schemas) | Implemented & verified (live test; E2E through the UI at the "Structure" level, on a stand-in CLI: `list_tables` and `describe_table`, whose step opens to the structure the model was given) | Implemented & verified (live test; E2E through the UI at "Structure and rows", on a stand-in CLI; `list_schemas` is not called there) | Planned |
| The AI's read-only query: one statement by its text, prepared by the server, in a `READ ONLY` transaction that is rolled back | Implemented & verified (live test: 18 write attempts refused, the data unchanged — among them version comments `/*! … */`, `INTO OUTFILE`, `LOAD_FILE`, and a text that is two statements only under `NO_BACKSLASH_ESCAPES`, stopped by the server's prepare); not through the UI — the E2E stage on this engine runs at the "Structure" level, where the query tool is not offered, and shows that no query step is made. A query can name another database the account can read | Implemented & verified (live test: 20 write attempts refused, the data unchanged — among them a data-modifying `WITH`, `EXPLAIN (ANALYZE) DELETE`, a function that writes, `nextval`, `COPY … TO PROGRAM`; E2E: a `DELETE` through the tool refused; two queries by the real Claude Code CLI in one manual turn through the window). Unqualified names resolve through the session's `search_path`, not the schema of the tab in front; the instructions for a turn with tools ask the model to write the schema, and nothing enforces it | Planned |
| Changes proposed by the AI, run by PuriCat after approval with the impact guard | Implemented & verified for an approved one-row `UPDATE` (E2E at the "Structure" level: the review dialog names the MySQL connection, and the row read back on a separate console is higher by exactly 1). A proposal rolled back above the row threshold, run without the limit, or rejected is not exercised on this engine (the run uses the console path, which is live-tested) | Implemented & verified (E2E: approved, rolled back above the row threshold, run without the limit, rejected; one proposal by the real Claude Code CLI approved by hand through the window) | Planned |
| Backup and restore | Planned (Phase 6) | Planned (Phase 6) | Planned (Phase 6) |

## Capability flags

Reported by each adapter so the UI asks instead of guessing from the engine name.

| Flag | MySQL | PostgreSQL | SQLite | Meaning |
|---|---|---|---|---|
| `schemas` | false | true | false | Objects live in schemas inside a database |
| `transactionalDdl` | false | true | true | DDL can be rolled back |
| `returning` | false | true | true | `INSERT/UPDATE/DELETE ... RETURNING` is available |

Only `schemas` affects behaviour today. The write path does not use `returning` or `transactionalDdl` yet: after a save the grid reloads the page instead of reading returned rows. The SQL console does not consult them either; it branches on the engine.

## Type rendering

Every value is converted to text by the server ([ADR 0004](docs/adr/0004-server-side-text-rendering.md)). The grid shows that text unchanged.

### MySQL

| Type | Shown as | Verified |
|---|---|---|
| Integers, `BIGINT` beyond 2^53, `DECIMAL` | Exact digits | Yes |
| `TINYINT(1)` | `0` / `1` (classified as boolean) | Yes |
| `DATETIME(6)` and other temporal types | Server text, fractional seconds kept | Yes (`DATETIME(6)`) |
| `VARCHAR`, `TEXT` (utf8mb4) | Text; first 1000 characters | Yes, including Thai |
| `JSON` | Server's normalised JSON text | Yes |
| `BLOB`, `BINARY` family | `0x` + uppercase hex of the first 24 bytes | Yes |
| `BIT(n)` | Decimal number | Yes |
| Spatial types | WKT from `ST_AsText` | SQL builder unit-tested; not run against a server |
| `ENUM`, `SET` | Label text | `ENUM` yes; `SET` no |

### PostgreSQL

| Type | Shown as | Verified |
|---|---|---|
| `bigint`, `numeric` | Exact digits | Yes |
| `boolean` | `true` / `false` | Yes |
| `timestamptz` | Text in UTC with an explicit `+00` offset (the session time zone is always UTC, see known limitations) | Yes (date and fractional seconds asserted; the offset was observed, not asserted) |
| `text` | Text; first 1000 characters | Yes, including Thai |
| `jsonb`, `json` | Server text | Yes (`jsonb`) |
| `bytea` | `0x` + lowercase hex of the first 24 bytes | Yes |
| Arrays | Array literal, e.g. `{x,"y z"}` | Yes |
| `uuid`, enum types, `date` | Text | Yes |
| Ranges, intervals, geometric, network, composite, PostGIS | Whatever `::text` yields | No |

Notes for both engines:

- NULL is transported separately from text, so NULL and the empty string are distinct in the grid.
- Temporal values are shown in the session's time zone and format. MySQL sessions keep the server's time zone (verified). PostgreSQL sessions always run in UTC, whatever the server or role default is (see known limitations).
- Truncated cells are flagged. "View whole value" (or a double-click) fetches the whole value by the row's primary key with the same rendering as the page: up to 1,000,000 characters of text, or the first 65,536 bytes of a binary value in hex. A longer value is shown up to that point and cannot be edited there. Without a primary key, or when a key value is NULL or itself shortened, only the preview is available.

### Query results (SQL console)

The tables above describe the browse path, where PuriCat writes the `SELECT`. In a query tab the user's statement is sent over the simple/text protocol and the server renders each value as text itself ([ADR 0015](docs/adr/0015-sql-console.md)). The exactness is the same; a few renderings differ:

| Value | In a query result | Verified |
|---|---|---|
| Integers, `BIGINT` beyond 2^53, `DECIMAL` / `numeric`, text including Thai | Exact text, as on the browse path | Yes, both engines |
| NULL | Distinct from the empty string | Yes, both engines |
| MySQL `BINARY` / `VARBINARY` / `BLOB` family | `0x` + **lowercase** hex of the whole value (the browse path shows uppercase hex of the first 24 bytes) | Yes (`VARBINARY`) |
| MySQL `BIT(n)` | Decimal number | Yes |
| MySQL spatial types | Treated as binary: hex of the internal form, not WKT. Use `ST_AsText()` in the query | No (from the code) |
| MySQL `TINYINT(1)`, `JSON`, temporal types, `ENUM` | Server text | No |
| PostgreSQL `bytea` | `0x` + lowercase hex of the whole value (the server's `\x…` with the prefix replaced) | Yes |
| PostgreSQL enum, array, `timestamptz` | Server text, e.g. `{a,"b c"}`; `timestamptz` in the session's zone (UTC) | Yes (the date and microseconds asserted) |
| PostgreSQL `boolean` | `t` / `f`, the server's text output (the browse path's `::text` cast gives `true` / `false`) | No (follows from the protocol; not asserted) |
| Other types | Whatever the server's text output is | No |

- Nothing is cut on the server. The backend keeps full values; the grid shows the first 1,000 characters of a long cell and flags it; an export has the whole value (verified with a 1,500-character Thai value).
- A column's kind (number, text, temporal, binary, JSON, bool) is derived from the driver's type name. It decides alignment in the grid, which values are shown as hex, and which XLSX cells may become numbers. The name is matched by substring; `INTERVAL` is temporal and `POINT` is text although both contain "int" (unit-tested). Other type names that happen to contain a matched word are not checked one by one.
- A query that returns no rows still shows its column names (verified on both engines).

## Row filter

A table tab can be narrowed with conditions on its columns. A row must meet all of them or — with two conditions or more and "Match: any" — any one. The conditions become one `WHERE` clause in the page query. Every value is a bind parameter, and a column name must exist in the table's metadata before it is quoted (`db::filter`).

| Condition | MySQL | PostgreSQL |
|---|---|---|
| `=`, `≠`, `>`, `≥`, `<`, `≤` on a number, boolean, date / time or text column | The column against the typed value: `CAST(? AS SIGNED / UNSIGNED)` for integers, `CAST(? AS DECIMAL(65,30))` for decimals, the bare parameter otherwise. Text follows the column's collation, so it usually ignores case | The column against `$n::<bare type>`. Text is case-sensitive |
| The same on a `FLOAT` column | `=` and `≠` compare the text the grid shows (a `FLOAT` widened to double never equals the double parsed from the same digits); the ordering comparisons are typed | Typed, as above |
| `=`, `≠` on JSON and other types (uuid, enum, array, …) | JSON: the text the grid shows, `CAST(col AS CHAR)`. Spatial columns: refused (NULL tests only) — the grid shows their WKT text, the column itself is binary | The text the grid shows: `col::text` |
| `>`, `≥`, `<`, `≤` on those types | Refused | Refused |
| contains, does not contain, starts with, ends with | `LIKE … ESCAPE '!'` on a text column itself (its own collation), on `CAST(col AS CHAR)` otherwise; refused on spatial columns | `ILIKE … ESCAPE '!'` on a text column itself, on `col::text` otherwise: always case-insensitive |
| is one of, is not one of (a list of 1 to 200 values) | So many `=`: each value is checked and bound exactly as `=` would bind it. `col IN (CAST(? AS SIGNED / UNSIGNED), …)` on an integer, `col IN (?, …)` on text, `CAST(col AS CHAR) IN (?, …)` on JSON and on a `FLOAT`. Refused on spatial columns | `col IN ($n::<bare type>, …)`; `col::text IN ($n, …)` on JSON and other types. The placeholders are numbered through every condition and list |
| is between (two values, both included) | Number, date / time and text columns: `col BETWEEN a AND b`, each end typed as for `>` and `<`. Refused on boolean, JSON and other types | The same, each end as `$n::<bare type>` |
| is empty, is not empty | Text columns only: `col = ''`, `col <> ''`. Refused on every other kind of column | The same |
| is NULL, is not NULL | Any column, including binary | Any column, including binary |
| Anything else on a binary column | Refused | Refused |

- `%`, `_` and `!` in a search text are taken literally.
- "≠", "is not one of" and "does not contain" keep the rows whose value is NULL (`NOT (a <=> b)` on MySQL, `IS DISTINCT FROM` on PostgreSQL, `(col NOT IN (…) OR col IS NULL)` for the list, `… OR col IS NULL` for the text search). A person reading the grid sees an empty cell as "not X".
- The empty text is not NULL. "Is empty" finds the rows that hold a text of no characters, not the NULL ones; "is not empty" (`col <> ''`) leaves the NULL rows out as well, and the bar says so beside the condition.
- **A value the column cannot hold is an error, never a silent result.** PostgreSQL refuses the cast itself and its message is shown. MySQL would read text that is not a number as `0` (and show the rows holding 0) and text that is not a date as NULL (and show no rows), each with only a warning, so PuriCat checks these itself before sending anything: a number must look like one, and a date or time must be complete (`2026-01-31`, `2026-01-31 13:45:00`, `13:45:00`). To find part of a date, use "starts with" or "contains". Every value of a list and both ends of a range go through the same checks, and the message names the value that was refused.
- Filters do not use an index when they compare the shown text (JSON, uuid, enum, the text searches on non-text columns). On a large table an equality filter on a `uuid` column scans the table.
- **All or any.** Conditions are joined with `AND`. With "Match: any" (offered once the bar holds two conditions) each condition is put in its own parentheses and they are joined with `OR`; one condition is the same either way. The page and the count are sent the same choice. On PostgreSQL, a list, a text search and a NULL test under "any" read:

  ```sql
  WHERE ("id" IN ($1::bigint, $2::bigint)) OR (("doc"::text NOT ILIKE $3 ESCAPE '!' OR "doc" IS NULL)) OR ("na`me" IS NULL)
  ```

  There are no brackets or nested groups — all of the conditions or any one of them, nothing in between — no NOT on a group and no free `WHERE` text. Use a query tab for those.
- **Limits.** At most 20 conditions, 200 values in one list, and 1,000 bound values in one request over all its conditions; more is refused before anything is sent. An empty list, and a range without exactly two values, are refused too.
- **The list box.** Values are separated by commas or line breaks. Spaces around a value are dropped, so are empty entries, and a value written twice is sent once. Double quotes keep a value as written, commas and spaces included; a quote inside them is written twice, and `""` is the empty text. A pasted text that has line breaks — a column copied from a spreadsheet — is turned into that form in the box. The box counts the values and says "more than 200" when the list is too long; such a list is still refused by the backend if it is sent.
- **A condition switched off.** The checkbox in front of a condition takes it out without deleting it: it is not in the `WHERE` clause, not in the number of conditions the footer shows, and not in a count. It lives in the open bar only — hiding the bar drops it, as it drops a condition that is not written yet.
- **From the grid.** "Hide rows with this value" in a cell's menu adds `≠` that value (rows that hold NULL stay), or "is not NULL" on a NULL cell. With several cells of one column selected, "Show rows with these N values" adds "is one of" with the different values of those cells; NULL cells and rows that are not saved yet are left out. It is greyed out, with the reason as its tooltip, on a binary column, when a selected cell shows only part of its value, when the cells hold NULL only, and for more than 200 different values. Both add to the conditions already applied, as "Filter by this value" does.
- Sorting and paging apply to the rows that are left. The row estimate in the footer is still the whole table's. The number of matching rows is counted when asked for ("Count matches"): `SELECT COUNT(*)` with the same `WHERE` clause and parameters as the page, on a connection of its own so that it can be interrupted (`KILL QUERY` on MySQL, `pg_cancel_backend` on PostgreSQL, the connection's own stop flag on SQLite). Reading the table's structure comes first and cannot be interrupted: if the table is locked exclusively by another session, the count waits there, as a page does.

## Writing values

Saving staged edits is described in [ADR 0013](docs/adr/0013-staged-edits-and-optimistic-saves.md). Values are sent as text parameters, exactly as typed or pasted; PuriCat does not parse or reformat them. The server converts and validates. A save is one transaction of single-row statements (deletes, then updates, then inserts; at most 10,000 row changes), and every statement must affect exactly one row.

| Aspect | MySQL | PostgreSQL |
|---|---|---|
| New value | Plain parameter; the server converts the string. `BIT` is written as `CAST(? AS UNSIGNED)` (a plain string would store its character codes) | `$n::<bare type>`, for example `$1::character varying` or `$2::numeric`, never `varchar(5)` or `numeric(12,2)`: an explicit cast to a sized type truncates silently, whereas the column assignment rejects an over-long value |
| Key comparison | Integer, `BIT` and `YEAR` keys: `CAST(? AS SIGNED)` or `CAST(? AS UNSIGNED)`, so 64-bit values are never compared as floats and the key index is used. `DECIMAL` keys: `CAST(? AS DECIMAL(65,30))`. Other key types: plain parameter | `col = $n::<bare type>` |
| "Unchanged since load" check on each edited column | `CAST(CAST(col AS CHAR) AS BINARY) <=> CAST(? AS BINARY)`: byte-wise and NULL-safe, so case and trailing spaces count (`BIT`: `CAST(col + 0 AS CHAR)`) | `col::text IS NOT DISTINCT FROM $n` (NULL-safe) |
| Rejecting bad input | `STRICT_ALL_TABLES` is added to the session `sql_mode` | Normal cast and assignment errors |
| Insert with no values | `INSERT INTO t () VALUES ()` | `INSERT INTO t DEFAULT VALUES` |
| Not writable | `auto_increment` and generated columns, `BINARY` / `BLOB` family, spatial types | Identity and generated columns, columns whose default is `nextval(...)` (serial), `bytea` |
| Read-only session | Server rejects the write | Server rejects the write |

Verified round trips (written through the save path and read back unchanged):

| Engine | Values |
|---|---|
| MySQL 9.3.0 | Thai text with quotes, a tab and a line break; `DECIMAL(24,6)`; negative `BIGINT` beyond 2^53; `TINYINT(1)`; `DATETIME(6)`; `JSON`; `BIT(8)`; `ENUM`; a value set to NULL; server defaults on insert; a stored generated column recomputed by the server |
| PostgreSQL 16.13 | Thai text with quotes, a tab and a line break; `numeric(24,6)`; `boolean`; `timestamptz` with microseconds; `jsonb`; `text[]`; `uuid`; an enum set to NULL; `date`; `varchar(5)` holding multi-byte text; server defaults on insert |

Verified rejections ("Nothing was saved", transaction rolled back): MySQL — non-numeric text in `DECIMAL` and `BIGINT`, an unknown `ENUM` label, NULL in a `NOT NULL` column. PostgreSQL — non-numeric text in `numeric`, an unknown enum label, an over-long `varchar(5)`, an impossible date, malformed JSON.

Not verified on the write path: `SET`, `FLOAT` / `DOUBLE`, `TIME`, `YEAR`, `TIMESTAMP` (MySQL); ranges, intervals, `time`, `timestamp without time zone`, network, geometric and composite types, domains (PostgreSQL); float or temporal primary keys on either engine.

### Session settings PuriCat applies

| Engine | Setting | Why |
|---|---|---|
| MySQL | `STRICT_ALL_TABLES` appended to the session `sql_mode`; the rest of the server's `sql_mode` is kept | Invalid values are rejected instead of truncated or zeroed |
| MySQL | Driver defaults switched off: no forced UTC time zone, no `PIPES_AS_CONCAT` | The session behaves like the server's default session (verified against the server's global variables) |
| MySQL | `utf8mb4` connection character set | Unicode round trips |
| MySQL, when the read-only option is on | `SET SESSION TRANSACTION READ ONLY` | Server-enforced read-only |
| PostgreSQL | `application_name = PuriCat` | Identifies the session |
| PostgreSQL | `TimeZone = UTC`, sent by the driver at start-up; not chosen by PuriCat and not yet configurable | See known limitations |
| PostgreSQL, when the read-only option is on | `SET default_transaction_read_only = on` | Server-enforced read-only |

A query tab's connection is opened through the same pool, so it starts with the same settings. SQL typed in the tab can change them for that tab (`SET sql_mode`, `SET TIME ZONE`, the read-only setting); such a change lasts until the tab's connection is closed and affects no other tab.

## Smart Paste

Design and alternatives: [ADR 0014](docs/adr/0014-smart-paste-match-then-write.md). The wizard takes a block copied from a spreadsheet and inserts, updates or upserts rows, matched on one key chosen by the user. PuriCat finds the matching rows with `SELECT`s, shows the outcome per pasted row, and then runs plain single-row `INSERT` and `UPDATE` statements — the statements described under "Writing values", addressed by the chosen key instead of the primary key — in one transaction.

| Aspect | MySQL | PostgreSQL |
|---|---|---|
| Keys offered for matching | The primary key and every unique index from `information_schema.STATISTICS`, except indexes with a prefix length on any column (`name(3)`) or with a functional key part | The primary key and every valid unique index from `pg_index`, except partial indexes (`WHERE ...`) and indexes on expressions. `INCLUDE` columns are not part of the key |
| Why those are excluded | Equality on a prefix or an expression is not equality of the row's values | A partial or expression index does not make the plain column values unique |
| Lookup, per 40 pasted rows | `SELECT CAST((key = ?) AS CHAR), ..., CAST(col AS CHAR), ... FROM t WHERE (key = ?) OR ...` | `SELECT (key = $1::type)::text, ..., col::text, ... FROM t WHERE (key = $n::type) OR ...` |
| Key comparison | The same typed expressions as the save path (`CAST(? AS SIGNED / UNSIGNED)` for integer keys, `CAST(? AS DECIMAL(65,30))` for decimals, a plain parameter otherwise) | `key = $n::<bare type>` |
| Who decides that two keys are equal | The column's collation and type. Under a case-insensitive collation `c3` and `C3` are the same key; two pasted rows spelled that way are reported as duplicates (verified). `7` and `007` are the same integer key (unit-tested on the classification, not run against a server) | The type's equality. Text is case-sensitive unless the column uses a non-deterministic collation or `citext` (neither tested): `c3` does not match a stored `C3` (verified) |
| Generated key (auto-increment / identity) | Usable for matching; never written | Usable for matching; never written |
| Native upsert | `INSERT ... ON DUPLICATE KEY UPDATE` is not used: it treats a clash on *any* unique index as "the same row" and its affected-row counts do not say plainly what happened | `INSERT ... ON CONFLICT` is not used: it needs a conflict target per statement and does not report the per-row outcome before running. `MERGE` is not used either |

Behaviour common to both engines:

- **Modes.** INSERT adds every row; with a key chosen, rows whose key already exists are reported ("exists") and block the run, unless the rows with problems are left out (below). UPDATE changes rows that exist; rows without a match are reported and skipped, never inserted. UPSERT changes rows that exist and inserts the rest.
- **Generated key in UPSERT mode.** A blank key means "new row" (the server assigns the key). A key that is not found is reported as "no match" and skipped; it is not inserted under a different id.
- **Generated key in INSERT mode.** A blank key inserts. A key that is not found is an invalid row ("assigned by the server and cannot be set; leave it empty"), for the same reason (live-tested on both engines).
- **Update columns.** Chosen per column; key columns are never updated. Only columns whose pasted text differs from the stored text are written.
- **Defaults.** Columns that are not mapped take their defaults. A NULL for a NOT NULL column that has a default is left out of the INSERT so the default applies (verified in the backend). The wizard sends an empty cell of every non-text column as NULL, also a NOT NULL one, so an empty cell takes that path (request building unit-tested in the frontend; not exercised end to end).
- **Pre-checks before anything runs.** Text that does not look like a number for a numeric column (`money` and `bit` are left to the server), text longer than the declared length of a `char` / `varchar` column, NULL for a NOT NULL column, and — for rows that would be inserted — required columns that are not mapped. Key values get the number and length checks too and, for an integer key column, must be whole numbers: `12abc`, `1.9`, `1e2` and ` 1` are invalid rows and are never sent to the database, so MySQL's lenient `CAST('12abc' AS SIGNED)` cannot match row 12 (unit and live tests, both engines). The server stays the authority: a value only it can judge (an impossible date, an unknown enum label, a foreign key) fails at run time and the whole run is rolled back (verified with `2026-02-30`).
- **Run.** The table is analysed again; nothing runs unless the fresh analysis reproduces the preview's fingerprint, which covers every row's status and changed columns (verified on both engines, including an outcome with the same counts but different rows). Otherwise one transaction, one statement per row, each required to affect exactly one row, UPDATEs carrying the optimistic check on the values seen by that fresh analysis. Progress is reported every 20 statements; "Stop and roll back" and any error undo everything.

Smart Paste limitations:

| Limitation | Detail |
|---|---|
| "Unchanged" is decided on this side | A column is unchanged when the pasted text equals the server's text rendering of the stored value, or is plainly the same value in another spelling: a number in another spelling (`1.5`, `01.50`, `+1.5` against a stored `1.500000`; whole-number spellings only for an integer column), a date or time in another spelling for the types without a time zone (`2026-1-5 9:05`, a `T` between date and time, trailing zeros of the fraction, a date alone against midnight), and on PostgreSQL the spellings of a boolean (`TRUE`, `t`, `yes`, `1`). Whatever is not plainly one of these is compared as text and counts as a change — a number with an exponent (`1.5e0`), text with spaces around it, `money` and `bit` columns, types with a time zone, JSON, uuid, text columns (case, trailing spaces) — and is then run as an update that stores the same value. A value the column would round or cut (`10.004` into two decimals, a time of day into a date) is a change every time it is pasted |
| One transaction, not batches | A run is a single transaction of single-row statements. This keeps a paste atomic but holds locks for its whole duration. Multi-batch and streamed imports are Planned (Phase 4) |
| 10,000 pasted rows per run | Larger sources are refused. Lookups cost one query per 40 rows and each written row is one statement; duration near the limit, or on a remote server, has not been measured |
| Problems block the whole run unless they are left out | A row with a problem (no key, a key used twice, a key that already exists in INSERT mode, a value that fails a pre-check) is never written. By default one such row blocks the run; "Leave these rows out and run the other N" in the preview runs the rest in the same single transaction. Both of two pasted rows with one key are left out, not the second only. What the pre-checks do not catch is still judged by the server during the run, and such a refusal rolls the whole run back: that row is not left out |
| Empty cells in text columns | The wizard sends an empty cell as NULL for a key column and for every non-text column, and as an empty string for a text column. A NOT NULL text column with a default therefore stores an empty string, not the default: to get the default, leave the column unmapped, or paste the text `NULL` with the "Read the text NULL" option on. An empty cell of a NOT NULL non-text column takes the default only on an inserted row; without a default ("is required"), or on a row that would be updated ("cannot be empty"), the row is reported as invalid |
| Key values of non-integer types | Integer keys must be whole numbers; other key columns get only the number-shape and declared-length checks. A key value of a type the database coerces in another way (a date, for example) is cast and compared by the server: PostgreSQL would reject a malformed one and fail the preview with the server's error instead of flagging the row; MySQL's behaviour is unknown. Not tested on either engine |
| Tables without a primary key | Smart Paste is offered only where the grid is editable, so a table that has a unique index but no primary key has no entry point in the UI, although the backend could match on that index |
| Composite keys | Matching is verified in the backend on both engines (live test: update, a different day unmatched, a NULL part of the key reported as a missing key, applied and read back). Not verified through the UI |
| Engines and versions | MySQL 9.3.0 and PostgreSQL 16.13 only. Through the UI only UPSERT on MySQL has been run |

### From a file

"Open a file…" in the wizard puts the rows of a file where the clipboard's would be. What the wizard then does with them is unchanged, and independent of the engine; what differs from a copied block is how the cells become text.

| Source | How a cell becomes text |
|---|---|
| CSV / TSV text (`.csv`, `.tsv`, `.tab`, `.txt`) | Exactly the characters in the file: nothing is trimmed or reinterpreted, `007` stays `007`. Quoted fields may hold the separator, quotes and line breaks. Empty lines are skipped |
| Encoding | UTF-8, or UTF-16 with a byte-order mark. Anything else is refused until its encoding is chosen (Thai Windows-874, Windows-1252, Windows-1251, Shift_JIS, GBK, Big5, EUC-KR, UTF-16 without a mark); bytes the chosen encoding has no character for are an error, not `�` |
| Separator | A `.tsv` or `.tab` file is tab-separated. Otherwise the one of comma, semicolon, tab and vertical bar that occurs most in the first ten lines, outside quotes; it can be chosen instead |
| Workbook (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`) | One sheet, the first unless another is chosen; its used range, without the empty rows below the data |
| Workbook: number | The stored value, not its format: `0.5` for a cell shown as `50%`, no thousands separators or currency signs. Whole numbers without a fraction; at most 15 significant digits, as Excel itself writes them, so `0.1 + 0.2` is `0.3`. **A workbook number is a double: a whole number beyond 2^53 (16 digits and more) is not exact in the file itself.** Long identifiers must be text cells, or come from CSV, where every character is kept |
| Workbook: date, time | `2026-01-31`, `2026-01-31 13:45:07`, `13:45:00` (a value below one day is a time of day); a duration as `hh:mm:ss`. No time zone: a workbook has none |
| Workbook: boolean, formula, error | `TRUE` / `FALSE` (what the cell shows; MySQL's `TINYINT(1)` wants `1` / `0`, so the pre-check flags it); a formula's stored result; an error cell as its text, such as `#N/A` |

- **NULL and empty are the same in a file.** Neither CSV nor a workbook can say NULL. As with a pasted block, an empty cell becomes NULL in a nullable column that is not text and the empty string in a text column; "Read the text NULL as an empty (NULL) value" covers files that spell it out. A table exported with NULLs in text columns therefore comes back with empty strings there, and the preview shows those rows as updates.
- At most 64 MB and 10,000 data rows (the limit of one run). A longer file is refused whole, with its row count; nothing is cut silently.
- Verified with files written by PuriCat's own exporter and by `rust_xlsxwriter`. Workbooks saved by Excel, Numbers or LibreOffice, and the `.xls`, `.xlsb` and `.ods` formats, have not been tried.

## SQL console

Design and alternatives: [ADR 0015](docs/adr/0015-sql-console.md). A query tab runs the selection or the whole script on a connection of its own, one statement at a time, and reports each statement's outcome.

| Aspect | MySQL | PostgreSQL |
|---|---|---|
| The tab's connection | Taken from the session's pool and detached; `USE <database>` when the tab names one | Taken from the pool of the tab's database (the profile's default when the tab names none) and detached |
| Changing database | The tab's selector: the next run opens a new connection on the chosen database. `USE` inside a script also works but does not move the selector | Not possible inside a tab; open a new query tab from the other database in the explorer |
| Protocol | Text protocol (`COM_QUERY`), one statement per request | Simple query protocol, one statement per request |
| Lexical rules when splitting | Backtick identifiers; `#` and `-- ` comments (`--` must be followed by white space); backslash escapes in strings; block comments do not nest | Dollar-quoted strings (`$$…$$`, `$tag$…$tag$`; `$1` is a parameter); nested block comments; backslash escapes only in `E'…'` strings |
| Guard transaction around a row-changing statement | `START TRANSACTION` … `COMMIT` / `ROLLBACK` | `BEGIN` … `COMMIT` / `ROLLBACK` |
| When the guard stays out | After the user's `BEGIN` / `START TRANSACTION`, and whenever `@@autocommit` is 0 (asked from the server; asked again after `SET`, `CALL` and other unclassified statements) | After the user's `BEGIN` / `START TRANSACTION` |
| DDL inside the user's transaction | Commits implicitly; the console then treats the transaction as ended | Transactional; the transaction stays open |
| An error inside the user's transaction | The transaction stays open, as the server leaves it; a deadlock, which rolls the whole transaction back, is not noticed | The server aborts the transaction; the message says to run `ROLLBACK`; the tab stays flagged until then |
| Row cap reached | A plain read is interrupted with `KILL QUERY`, also inside a user's transaction (not tested there). Statements of other kinds are read to the end | A plain read outside a user's transaction is interrupted with `pg_cancel_backend`. Inside a user's transaction, and for statements of other kinds, the rest is read and dropped |
| Stop | `KILL QUERY <connection id>` from a pooled connection | `SELECT pg_cancel_backend(<pid>)` from a pooled connection of the same database |
| Connection treated as lost | I/O or protocol error; server errors 1053 (shutdown), 1927 (connection killed), 4031 (idle disconnect) | I/O or protocol error; SQLSTATE class `08`, `57P01`, `57P02`, `57P03` |
| Read-only option | The connection starts with `SET SESSION TRANSACTION READ ONLY`; the server refuses a write (verified). `SET SESSION TRANSACTION READ WRITE` switches it back | The connection starts with `default_transaction_read_only = on` (the console was not run read-only in a test). `SET default_transaction_read_only = off` switches it back |
| Verified | Live test and E2E through the UI | Live test only |

Behaviour common to both engines:

- **Statement kinds.** `read` (`SELECT`, `SHOW`, `EXPLAIN`, `WITH` without a row-changing word, …), `dml` (`INSERT`, `UPDATE`, `DELETE`, `REPLACE`, `MERGE`, a `WITH` containing one, `EXPLAIN ANALYZE` of one), `ddl`, `transaction`, `other` (`SET`, `USE`, `CALL`, `DO`, `SELECT ... INTO`, anything unrecognised). The same keyword table is used for both engines.
- **Confirmation.** `UPDATE` or `DELETE` without a top-level `WHERE`, `DROP`, `TRUNCATE` and `ALTER ... DROP` are listed before the script runs; on a Production-labelled connection every statement that is not `read` is.
- **Impact guard.** A `dml` statement outside the user's transaction that affects more rows than the connection's "review larger saves" threshold (default 10) is rolled back and reported as blocked; "Run it without the limit" runs it and the rest of the script with no limit.
- **Errors.** The first error stops the script; the number of statements not run is shown.
- **Results.** At most the chosen cap of rows per result (100 / 1,000 / 10,000 / 100,000; default 1,000). Results are read-only and cannot be sorted in the grid.
- **Export.** CSV (UTF-8 with a byte-order mark, RFC 4180, CRLF; NULL is an empty field, an empty string is `""`), JSON (an array of objects, every value a string or `null`), XLSX (a cell of a numeric column becomes a number only when it has at most 15 significant digits in plain notation, otherwise it stays text; bold frozen header; at most 1,048,575 data rows). Fetched rows only.

SQL console limitations:

| Limitation | Detail |
|---|---|
| Read-only is not enforced against typed SQL | A query tab inherits the connection's read-only setting, but a statement can switch the session back (see the table above). In the console the option protects against accidents, not against the person at the keyboard; a database account without write privileges is the real control ([SECURITY.md](SECURITY.md)) |
| The classification reads keywords | A function called from a `SELECT` can write and is still a `read`: not confirmed, not guarded, cancelled at the row cap. A `WITH` that mentions a word such as `replace` is treated as row-changing. Vendor statements the table does not know are `other` |
| Unguarded statements | DDL, `CALL`, `DO` and every `other` statement run without the impact guard. MySQL DDL cannot be rolled back |
| What the guard counts | The server's affected-row count of the statement. Rows changed by triggers or foreign-key cascades are not included. `EXPLAIN ANALYZE UPDATE` and a data-modifying `WITH` that ends in a `SELECT` report a count that is not the number of changed rows. Not tested |
| Transaction tracking is textual | A transaction opened inside a stored procedure or with `XA` is not seen. On MySQL a deadlock rollback, and an implicit commit caused by a statement that is not DDL, are not noticed; a successful DDL statement counts as a commit unless it is `CREATE` / `DROP TEMPORARY …` (recognised by `TEMPORARY` being the second word, so a table merely named `temporary` is not mistaken for one). The rule has a unit test and no test against a server |
| 100,000 rows per result, in memory | No streaming and no "load more": a capped result has to be run again with a higher cap. Memory use and grid responsiveness at the cap have not been measured |
| Export | Only the results of the tab's last run, and only fetched rows (a capped result exports capped). In JSON, columns with the same name are kept under the keys `name`, `name_2`, `name_3`, … in column order (unit-tested for the keys). XLSX numbers: a value such as `007` in a numeric column becomes the number 7 |
| PostgreSQL tabs cannot change database | Planned (Phase 4) |
| MySQL `USE` and the selector | A `USE` in a script moves the session; the tab's database selector keeps showing its own choice |
| One server connection per query tab | Each tab that has run something holds a connection until it is closed, beyond the pool limits (4 on MySQL, 3 per database on PostgreSQL). There is no cap other than the server's |
| Stop and the row cap need the pool | The interrupt is sent on a pooled connection; with every pooled connection busy it waits for one |
| Completion | Table names the explorer has loaded, columns of the tables named in the text (and of `table.` before the cursor), keywords. Aliases are not understood; for PostgreSQL only the `public` schema of the tab's database is loaded automatically |
| Messages | Messages produced by the backend (danger reasons, "Cancelled", the lost-connection note, the history summary) are English only |
| Engines and versions | MySQL 9.3.0 and PostgreSQL 16.13 only. Through the UI the console has been run on MySQL only |

## New database

Added on 2026-11-19 ([ADR 0019](docs/adr/0019-schema-changes-through-forms.md)). "New database…" in the menu of a connected MySQL or PostgreSQL connection opens a form. The form only chooses; the backend builds the one `CREATE DATABASE` statement (`db::ddl`), the form shows it as "Statement that will run", and the same function builds it again when it is run. Each engine gets its own options, and an option of the other engine is refused, not ignored.

| Aspect | MySQL | PostgreSQL |
|---|---|---|
| Options in the form | Character set, collation | Owner, template, encoding |
| Where the lists come from, read when the form opens | `information_schema.CHARACTER_SETS` and `COLLATIONS`; the server's defaults from `@@character_set_server` and `@@collation_server` | Owners: `pg_roles` without the `pg_…` roles, and `current_user`. Templates: the databases marked as templates. Encodings: those a database can be stored in — encoding ids 0 to 34; the client-only ones (`SJIS`, `BIG5`, …) are left out — and the encoding of `template1` |
| The first entry of each list | "Server default (…)" naming the server's value; for the collation, once a character set is chosen, that character set's default. Adds nothing to the statement | "You (…)" for the owner, "template1 (default)", "Same as the template (…)" for the encoding. Adds nothing to the statement |
| How the lists depend on each other | Choosing a character set narrows the collations to that character set and clears the chosen collation. With no character set chosen, the collations of the server's default character set are listed | They do not. A hint under the encoding says that an encoding other than the template's needs `template0`; the server decides |
| The statement | ``CREATE DATABASE `name` [CHARACTER SET cs] [COLLATE co]`` | `CREATE DATABASE "name" [OWNER "role"] [TEMPLATE "db"] [ENCODING 'enc']` |
| How it is run | One statement on a connection of the session's pool | One statement on a pooled connection to the database the session was opened on, outside a transaction (`CREATE DATABASE` cannot run inside one) |
| Read-only connection | The form says so and its button is disabled; the command refuses; and the server refuses on its own (verified below the UI) | The same (verified below the UI) |
| Refusals by the server, shown in the form in the server's words | A name that exists, an unknown character set (both verified); a missing privilege (not tried) | A name that exists, a role that does not exist (both verified); an encoding the template's locale does not allow (the live test tries `LATIN1` with `template0` and accepts either answer: created, or refused in words that name the encoding, after which `UTF8` is created); a missing privilege (not tried) |
| After creation | The connection's database list is read again and the new database is in it | The same |

Rules of the statement, the same on both engines:

- **The name** is trimmed and then quoted as one identifier (backticks or double quotes doubled). Spaces, Thai letters and the quote character itself are part of the name: a database named with all three is created and found again under exactly that name on both engines (live test). An empty name, a name with a NUL character and a name that is too long are refused before anything is sent: more than 64 characters on MySQL, more than 63 bytes on PostgreSQL (63 ASCII letters, 21 Thai ones).
- **A character set, collation or encoding** is a keyword to the server, not an identifier, so the identifier quoting does not apply to it. It must consist of ASCII letters, digits, `_` and `-`, at most 64 of them; anything else is refused before anything is sent. The lists in the form only ever offer what the server named, so the rule matters for a request that does not come from the form.
- **Owner and template** are quoted identifiers.

**Why SQLite has no such form.** A SQLite connection is one file, and that file is its one database, `main`: there is nothing to create inside it. The menu item is not shown for a SQLite connection, and the backend refuses both the option lists and the creation (verified). Creating a new database *file* is a different feature and is not built (see "SQLite — a database file").

Limits:

| Limitation | Detail |
|---|---|
| Databases only | No form creates a table, an index or a view. Planned (Phase 4); the SQL can be typed in a query tab. Editing and dropping a database are in "Database menu" below; the definition, rename, empty and drop of a table or a view are in "Table menu" |
| PostgreSQL options left out | Everything but owner, template and encoding: among them `LC_COLLATE`, `LC_CTYPE`, the locale provider and ICU locale, tablespace, connection limit and `IS_TEMPLATE`. A database gets the locale of its template |
| MySQL options left out | `ENCRYPTION` and `READ ONLY` |
| PostgreSQL templates | Only databases marked as templates are offered, although PostgreSQL lets a superuser or the owner copy any database |
| PostgreSQL encodings | The cut-off between encodings for databases and encodings for clients only (id 34) is a constant of PostgreSQL's source, written into PuriCat; it is not read from the server. Checked on 16.13 only. A value the server does not accept is refused by the server |
| PostgreSQL names longer than 63 bytes | Refused by PuriCat before a statement exists (unit test). PostgreSQL itself would shorten such a name to 63 bytes and say so only in a notice, so the database would get another name than the statement shows. The limit assumes a server built with the usual `NAMEDATALEN` of 64; a name of exactly 63 bytes has not been sent to a server |
| Owner | Any role that is not a `pg_…` role is offered, including roles the connected role may not hand a database to; the server refuses those |
| Privileges | Tested with accounts that may create databases. The refusal for an account that may not has not been seen |
| No record | The statement is not written to the SQL history or to any log |
| Engines and versions | MySQL 9.3.0 and PostgreSQL 16.13 only. MariaDB and other versions are untested; the form relies on `information_schema.CHARACTER_SETS` / `COLLATIONS` and on the `pg_encoding_to_char` function |

## Database menu

Added on 2026-11-19, on the rule of "New database" ([ADR 0019](docs/adr/0019-schema-changes-through-forms.md), section 7): a form only chooses, the backend builds every statement (`db::ddl`), the form shows it as "Statement that will run", and the same builder is called again when it is run.

Right-click a database in the explorer: New query, Open SQL file…, New database…, Edit database…, Copy name, Refresh, Drop database…. Right-click a connection: besides what was there, New connection, Open SQL file… (while connected) and, on a connected MySQL connection, a Flush submenu.

| Aspect | MySQL | PostgreSQL |
|---|---|---|
| What "Edit database…" can change | The default character set and the default collation. They are the defaults for tables created from then on; existing tables keep their own, and the form says so | The name and the owner |
| What it shows and cannot change | The name: MySQL has no statement that renames a database, and the form says so under the disabled field. A request to rename is refused by the builder too | The encoding: it is set when a database is created. Shown in a disabled field with that explanation |
| Where the form's values come from | `information_schema.SCHEMATA` for the database as it is now; the lists of character sets and collations as in "New database". Choosing another character set empties the collation and offers "that character set's default" first | `pg_database` for the owner and the encoding; the owner list as in "New database", plus the current owner if that list leaves it out |
| What is sent | Only what differs from the database as it is now. With nothing changed there is no statement and "Save changes" is disabled | The same. The same name again is not a change |
| The statements | One: ``ALTER DATABASE `name` [CHARACTER SET cs] [COLLATE co]``. A collation alone sends only `COLLATE`; a character set alone leaves the collation to that character set's default (`latin1` gave `latin1_swedish_ci` in the live test) | Up to two, in this order: `ALTER DATABASE "name" OWNER TO "role"`, then `ALTER DATABASE "name" RENAME TO "new name"` — the owner first, while the database still has the name on screen |
| How they run | One statement on a connection of the session's pool. MySQL commits DDL at once | Sent together as one simple query on a pooled connection to the database the session was opened on, which the server treats as one transaction: when the rename is refused, the owner is not changed either (verified: live test) |
| "Drop database…" | ``DROP DATABASE `name` `` | `DROP DATABASE "name"` — without `WITH (FORCE)`: other clients are not disconnected |
| What the drop form says the database holds | The number of rows in `information_schema.TABLES` for it: tables and views | The number of tables, partitioned tables, views and materialized views outside the catalogue schemas, counted on a connection to that database. Best effort on both engines: when the count cannot be made the form shows the statement without a number |
| The server's own databases | `information_schema`, `mysql`, `performance_schema`, `sys`, in any letter case: refused by the builder for edit and drop. "Edit database…" and "Drop database…" are not in their menu | `postgres`, `template0`, `template1`: refused by the builder. The templates are not listed in the explorer. `postgres` is listed, and its menu **does** show both items: the refusal then appears in the form in place of the statement (read from the code; not tried through the UI) |
| The database the connection is opened on | No such limit: a MySQL session is not tied to a database | A drop and a rename are refused before anything is sent, with a sentence that says to use a connection whose default database is another one: the server would refuse anyway, since PuriCat's own session is connected to it. An owner change is not refused |
| PuriCat's own connections to the database | Query tabs opened on it give up their connection before a drop (the server does not require it) | The server refuses to drop or rename a database anyone is connected to. Before either, PuriCat closes the session's pooled connections to that database and the connections of query tabs opened on it. Another client's connection still makes the server refuse; its message is shown (not tried) |
| Read-only connection | The forms say so and disable their button; the commands refuse a profile saved as read-only; the server refuses `ALTER DATABASE` and `DROP DATABASE` in a read-only session (the server's refusals are verified below the UI; the commands' and the forms' are not) | The same |
| After a rename or a drop | — (no rename). After a drop: as on PostgreSQL | The database's table tabs close; its query tabs keep their text and move to the new name, or after a drop to the connection's default database; what the explorer had loaded under the old name is forgotten and the database list is read again |
| Flush | A submenu of the connection: Privileges, Tables, Logs, Status. Four fixed statements — `FLUSH PRIVILEGES`, `FLUSH TABLES`, `FLUSH LOGS`, `FLUSH STATUS` — each shown with a sentence on what it does, then run on a pooled connection | Not offered; the backend refuses with "FLUSH is a MySQL statement" (verified) |

Rules shared with "New database": names are quoted as one identifier, whatever they contain (verified with a name of spaces, Thai letters and the engine's quote character: altered and dropped on MySQL, renamed to and dropped on PostgreSQL); a character set or collation must be a plain word; an option of the other engine is refused, not ignored. A new PostgreSQL name longer than 63 bytes is refused before a statement exists, as at creation (read from the code: the rename has no test of its own for the length).

**The name typed to confirm a drop** must equal the database's name exactly — case, spaces and all. The form enables its button only then, and the `drop_database` command compares the two again before it does anything.

**Tabs with unsaved work.** The forms do not drop or rename a database while a table tab of it has unsaved edits or a query tab opened on it has an open transaction; they ask to save, discard or end them first. The check is the form's and goes by the database a tab was opened on.

**Open SQL file…** shows the native Open dialog (filter: `.sql`) and opens the file's text in a new query tab — on the database whose menu was used, or on the connection's default database from the connection's menu. The file must be UTF-8 and at most 1 MB (the limit of a saved script); a byte-order mark at the start is removed. The tab is an ordinary query tab from then on: it is named "Query n", not after the file, has no link back to the file, and nothing runs until the user runs it. Offered on every engine, SQLite included.

**Why SQLite has no Edit or Drop.** As for "New database": the file is the one database. The items are not in the menu of a SQLite database, and the backend refuses the details, the plans and `FLUSH` as "unsupported" (verified).

Limits:

| Limitation | Detail |
|---|---|
| MySQL: no rename | The server has no such statement. Not offered and not emulated (no dump and reload) |
| MySQL: existing tables are not converted | `ALTER DATABASE` changes the defaults for new tables only |
| PostgreSQL: what cannot be edited | Encoding, locale, tablespace, connection limit, `IS_TEMPLATE`, per-database settings. MySQL: `ENCRYPTION`, `READ ONLY` |
| PostgreSQL: a database other clients use | Not forced. The server's refusal of a drop or a rename while someone else is connected has not been seen in a test |
| PostgreSQL: `postgres` in the menu | Edit and Drop are offered and refused only in the form (see the table) |
| MySQL `FLUSH` | `FLUSH HOSTS` is not offered: MySQL 8.4 removed it. `FLUSH PRIVILEGES` is deprecated in recent MySQL — 9.3.0 answers it with warning 1681 — and the form says so; PuriCat does not show the server's warnings. `FLUSH LOGS` starts a new binary log file where binary logging is on. Each needs a privilege on the server (`RELOAD`, in MySQL's manual); the refusal for an account without it has not been seen. Checked on 9.3.0 only; MariaDB untested |
| SQL files | UTF-8 only: a file in another encoding is refused, not converted. More than 1 MB is refused. The text is copied into the tab; changes are not written back to the file |
| Tabs the forms cannot see | On MySQL, a query tab opened on the connection's default database, or one that moved with `USE`, is not counted as a tab of the database: it neither holds a drop back nor moves afterwards |
| Stop | None. A statement that waits (`FLUSH TABLES` behind a long query, a drop behind a lock) can only be waited for; the form cannot be closed meanwhile |
| No record | Nothing done from this menu is written to the SQL history or to any log |
| Through the UI | Verified there: a MySQL collation change, opening a SQL file, Flush → Status, a PostgreSQL rename and a PostgreSQL drop (E2E, dev mode). Not verified there: a MySQL drop, an owner change, the other Flush kinds, a read-only or Production-labelled connection, the refusals, Thai and the dark theme, the bundled app |
| Engines and versions | MySQL 9.3.0 and PostgreSQL 16.13 only |

## Table menu

Added on 2026-11-20, on the same rule ([ADR 0019](docs/adr/0019-schema-changes-through-forms.md), section 8): a form only chooses, the backend builds the statement (`db::ddl::table_change`), the form shows it as "Statement that will run", and the same function builds it again when it is run.

Right-click a table, a view or a materialized view in the explorer: Open data, Definition…, Copy name, Rename…, Empty table… (tables only), Drop table… or Drop view…. On MySQL, PostgreSQL and SQLite. "Rename…" is not in the menu of a SQLite view.

### Definition

The statements that would create the object as it is now. Read from the server and shown; nothing is run. The dialog has "Copy" and "Open in query tab" (a new query tab on the table's database, holding the text; nothing runs there either), and one line under the text that says where it comes from.

| Engine | Where the text comes from | What it holds |
|---|---|---|
| MySQL | The server's own statement: `SHOW CREATE TABLE`, or `SHOW CREATE VIEW` for a view | Whatever that server version prints. A table's text names the table without its database; a view's text names the database, after whatever the server writes between `CREATE` and `VIEW` (on 9.3.0 the live test requires only that the text starts with `CREATE` and holds `` VIEW `db`.`v` AS select ``) |
| SQLite | The text SQLite keeps in the file (`sqlite_master`), exactly as the object was created | The table's or view's own statement, then the statements of its indexes and triggers, each ended with `;`. Indexes SQLite made by itself (for a `PRIMARY KEY` or `UNIQUE` written in the table) have no text and are not listed |
| PostgreSQL, a view or a materialized view | `pg_get_viewdef`, under a `CREATE VIEW` or `CREATE MATERIALIZED VIEW` line written by PuriCat | The stored query as the server prints it, then the object's indexes (a materialized view's), triggers and comments |
| PostgreSQL, a table | **Put together by PuriCat from the catalogue.** PostgreSQL has no statement that prints a table; `pg_dump` is the tool that does, and it remains the authority | See below |

What the PostgreSQL text of a table holds, in this order: `CREATE TABLE` (or `CREATE UNLOGGED TABLE`) with the schema and the name; every column with its type as `format_type` prints it, its collation when that is not the type's default, then `GENERATED ALWAYS AS IDENTITY` / `GENERATED BY DEFAULT AS IDENTITY`, or `GENERATED ALWAYS AS (…) STORED`, or `DEFAULT …`, then `NOT NULL`; the constraints by name — primary key, unique, check, exclusion, then foreign keys — each as `pg_get_constraintdef` prints it; `PARTITION BY …` for a partitioned table; then the indexes that do not back one of those constraints (`pg_get_indexdef`), the triggers that are not the server's own (`pg_get_triggerdef`), and `COMMENT ON` for the table and its columns.

Limits of the PostgreSQL text. The first row is what the dialog itself says; the others are not said there:

| Limitation | Detail |
|---|---|
| Not included | Owner, grants, row-level-security policies, storage parameters and inheritance |
| `serial` columns | Printed as `DEFAULT nextval('…')`. The sequence is not part of the text, so the text does not run in a database that lacks it |
| Identity columns | The options of the sequence (start, increment, …) are not printed; the text gives a default sequence |
| Partitions | A partition prints as a plain table, not as `PARTITION OF` its parent. `PARTITION BY` on the parent is printed, and has no test |
| Foreign tables | Listed under Tables in the explorer, but the definition looks for ordinary and partitioned tables only: "not found" (read from the code; not tried) |
| Verified with one table | The live test's sample: an identity column, a default, a generated column, a non-default collation, a primary key, a check, a foreign key with `ON DELETE CASCADE`, a partial index, a trigger, a table comment in Thai with a quote in it, a column comment. Run in a second database, the text made a table whose own definition was the same text and whose columns `describe_table` reported the same. An unlogged table, an exclusion constraint, a unique constraint and a partitioned table are in the code and not in a test |

### Rename, Empty, Drop

| Aspect | MySQL | PostgreSQL | SQLite |
|---|---|---|---|
| Rename | ``RENAME TABLE `db`.`a` TO `db`.`b` `` — the same statement for a view; the database is named on both sides, so the new name cannot land in the session's database | `ALTER TABLE "schema"."a" RENAME TO "b"`, or `ALTER VIEW …`, or `ALTER MATERIALIZED VIEW …` | `ALTER TABLE "a" RENAME TO "b"`. A view cannot be renamed: SQLite has no statement for it. The item is not in a view's menu, and the builder refuses with the advice to drop the view and create it again |
| Empty (tables only) | ``TRUNCATE TABLE `db`.`a` `` | `TRUNCATE TABLE "schema"."a"` | `DELETE FROM "a"`: SQLite has no `TRUNCATE` |
| What the Empty form says | `TRUNCATE` commits at once and cannot be rolled back, and `AUTO_INCREMENT` starts again from the beginning (verified: the next row inserted got id 1) | It cannot be undone; identity and serial counters keep their values (no `RESTART IDENTITY`); a table that another table refers to with a foreign key is refused by the server (verified: refused, the rows kept) | It cannot be undone; a `DELETE` runs, so the table's triggers fire (verified: a trigger fired once per row) |
| Drop | ``DROP TABLE `db`.`a` `` or ``DROP VIEW `db`.`a` `` | `DROP TABLE`, `DROP VIEW` or `DROP MATERIALIZED VIEW`, with `"schema"."a"` | `DROP TABLE "a"` or `DROP VIEW "a"` |
| Where it runs | On a connection of the session's pool | On a pooled connection to **the database that holds the table**, which need not be the one the session was opened on (verified: the live test's session is opened on another database) | On the file's writable handle. A file opened read-only is refused before anything runs, with the advice to turn the read-only session off (verified) |
| The row count in the Empty and Drop forms | "It holds about N rows", from `information_schema.TABLES.TABLE_ROWS`. The estimate can lag: in the end-to-end run the Drop form still said "about 2 rows" right after the table had been emptied | The estimate from `pg_class.reltuples`; no number for a table that was never analysed | No number: SQLite keeps no estimate |

Shared by the three engines:

- **One statement, nothing added.** No `IF EXISTS` and no `CASCADE`: a table that is gone, a name that is taken, a view that depends on the table — the server refuses in its own words and the form stays open (verified below the UI: a taken name on MySQL; on PostgreSQL a table two views depend on, dropped only after them; a second drop of the same table on MySQL and SQLite).
- **Names are quoted as one identifier**, whatever they contain (verified with a name of Thai letters, a space and backticks on MySQL, and a space and double quotes on PostgreSQL). The new name is trimmed; an empty one, the same name again and a name with a NUL character are refused; so is one that is too long — more than 64 characters on MySQL, more than 63 bytes on PostgreSQL, which would otherwise shorten it silently. The 64-character rule is applied to SQLite as well, although SQLite has no such limit.
- **The name typed to confirm.** "Empty table" and "Drop table" / "Drop view" are enabled only once the object's name has been typed exactly, and `apply_table_change` compares the typed text with the name again. A rename has no typed confirmation.
- **Read-only connection.** The definition is still shown. The three forms say so and disable their button; the command refuses a profile saved as read-only; the servers refuse the statements in a read-only session (the servers' refusals are verified below the UI; the command's and the forms' are not).
- **Production-labelled connection.** A warning in each of the three forms, and a danger-styled Rename button (the Empty and Drop buttons are danger-styled on every connection).
- **Unsaved edits.** None of the three runs while the tab of that table has unsaved edits; the form asks to save or discard them first. The check is the form's and looks at that table's tab only.
- **Afterwards.** After a rename or a drop the table's tab closes and the explorer reads the object list again; a renamed table is opened again from the explorer under its new name. After an empty the open tab reads its rows again; the object list is not read again, since nothing in it changed.

Limits:

| Limitation | Detail |
|---|---|
| What the menu does not do | Create a table, change columns, keys or indexes, move a table to another schema or database, refresh a materialized view. Planned (Phase 4); the SQL can be typed in a query tab |
| No protected names | Unlike the database menu, nothing is refused by name: the tables of the server's own databases and schemas (`mysql`, `pg_catalog`, …) have the same menu, and what happens is up to the account's privileges and the server (read from the code) |
| PostgreSQL foreign tables | Listed under Tables, so the statements say `TABLE` for them. Not tried |
| Emptying is not the same thing on the three engines | See the table: an implicit commit and a counter reset on MySQL, counters kept on PostgreSQL, a row-by-row `DELETE` with triggers on SQLite. PostgreSQL's `RESTART IDENTITY` and `CASCADE` are not offered |
| The row count is an estimate, or missing | It is information for the reader, read when the form opens; it does not guard anything, and on MySQL it can be stale |
| Other tabs | Only the table's own tab is closed or reloaded. A query tab that uses the table, and the tab of a view that reads it, are left as they are |
| Stop | None. A statement that waits behind a lock can only be waited for; the form cannot be closed meanwhile |
| No record | Nothing done from this menu is written to the SQL history or to any log |
| Through the UI | Verified there (E2E, dev mode): on a MySQL table the definition, "Open in query tab", a rename, an empty and a drop; the definition of the PostgreSQL table `shop.orders` with its note. Not verified there: SQLite; views and materialized views; a rename, an empty or a drop on PostgreSQL; "Copy"; unsaved edits holding a form back; a read-only or Production-labelled connection; a refusal by the server; Thai and the dark theme; the bundled app |
| Engines and versions | MySQL 9.3.0, PostgreSQL 16.13 and the SQLite the app carries (3.46.0) only |

## Object classification

| Engine | Shown under Tables | Shown under Views |
|---|---|---|
| MySQL | `BASE TABLE` | Everything else (`VIEW`, `SYSTEM VIEW`) |
| PostgreSQL | Ordinary, partitioned and foreign tables | Views and materialized views |

System namespaces are marked: `information_schema`, `mysql`, `performance_schema`, `sys` in MySQL; `information_schema` and `pg_*` schemas in PostgreSQL (`pg_toast*` and `pg_temp*` are hidden). PostgreSQL template databases and databases that do not allow connections are not listed.

## Known limitations

| Limitation | Detail |
|---|---|
| **PostgreSQL sessions always use `TimeZone=UTC`** | sqlx 0.8 sends `TimeZone=UTC` at connection start-up, overriding the server and role default. Consequences: `timestamptz` values are displayed in UTC with an explicit `+00` offset (unambiguous, but not the local time other clients show); a `timestamptz` typed without an offset is interpreted as UTC; and `now()`-based defaults of `timestamp without time zone` columns are computed in UTC rather than in the server's configured zone, including for rows inserted through the grid. Not fixed. A per-connection session time zone is Planned (Phase 2b). MySQL is not affected: its sessions keep the server's time zone |
| DELETE checks the key only | A row that was changed elsewhere after it was loaded, but still exists, is deleted without a conflict. Only an already-deleted row is detected |
| Concurrency check covers edited columns only | A concurrent change to a column the user did not edit is neither detected nor overwritten |
| Binary cells, and some shortened cells, are not editable | A cell cut to the preview length (1000 characters) is edited in the whole-value dialog, which first fetches its complete original. That needs the row's primary key: in a table without one, or beyond 1,000,000 characters, the cell stays read-only. Binary and MySQL spatial columns are never editable |
| Rows with a NULL or truncated key value are not editable | The row cannot be addressed safely |
| Keyless tables are read-only in the UI | The backend refuses UPDATE and DELETE on a table without a primary key and would allow INSERT, but the UI keeps such tables fully read-only for now. Unique keys are not used as a substitute in the grid, and Smart Paste is not offered on such tables either |
| Views and materialized views are read-only | Even where the engine would allow an updatable view |
| Generated, identity, auto-increment and serial columns cannot be written | Including on insert: an explicit value for such a column cannot be supplied from the grid |
| Edits are per page | Staged edits live on the loaded page; paging, sorting and refresh are locked until Save or Discard. One save handles at most 10,000 row changes, as single-row statements without batching or progress (a Smart Paste run reports progress and can be stopped) |
| Empty input and NULL | An empty cell means NULL only in a nullable column that is not a text type; in text columns it is the empty string. "Set to NULL" in the context menu is explicit. A copied NULL is placed on the clipboard as an empty cell |
| Validation happens on the server | The grid has no client-side type check; a bad value is reported at Save and nothing is saved. Smart Paste adds a few pre-checks (see "Smart Paste") |
| MySQL strict mode changes session behaviour | On a server whose default `sql_mode` is not strict, PuriCat's sessions are stricter than other clients' |
| `OFFSET` paging | Cost grows with the offset; deep pages of large tables are slow. Keyset pagination is Planned (Phase 2b) |
| Row counts are estimates | From `information_schema.TABLES.TABLE_ROWS` (MySQL) or `pg_class.reltuples` (PostgreSQL). Can be far off, and absent for a never-analysed PostgreSQL table. Labelled as an estimate |
| Keyless tables | No stable order; flagged as server order. Pages can overlap or skip rows if data changes |
| Sorting a keyless table | Ties in the sort column have no tie-breaker, so rows with equal values can move between pages |
| Preview truncation | 1000 characters of text, 24 bytes of binary |
| Page size | 1–1000 rows; the UI offers 100, 200, 500 and 1000 |
| PostgreSQL pools | One pool (up to 3 connections) per browsed database, kept until the connection is closed. Browsing many databases on one server holds many connections |
| PostgreSQL default database | When the profile has none, the session starts on `postgres`; the user must be allowed to connect to it |
| MySQL database visibility | `SHOW DATABASES` returns only what the account's privileges allow |
| MySQL `generated` flag | True for `auto_increment` and `STORED`/`VIRTUAL GENERATED` columns, read from the `EXTRA` column. Expression defaults (`DEFAULT_GENERATED`) are not treated as generated (unit-tested) |
| No cancellation or reconnect outside query tabs | A slow browse or catalogue query runs to completion; a dropped pooled connection surfaces as an error on the next action. A Smart Paste run can be stopped between statements, but its lookup queries cannot be cancelled. A statement running in a query tab can be stopped, and a query tab whose connection was lost reconnects on its next run (see "SQL console") |
| Read-only connections and the SQL console | The read-only option is a session setting. The grid and Smart Paste never send SQL that could change it; a query tab runs what the user types, including a statement that switches the session back to read-write (see "SQL console") |
| Permissions | Catalogue queries assume read access to `information_schema` / `pg_catalog`. Behaviour under restricted accounts is untested |
| Large tables | No performance measurements exist |

## SQLite — a database file

Added on 2026-11-18 ([ADR 0018](docs/adr/0018-sqlite-files.md)). A connection is the path of an existing file; there is no server, user or password. The file's one database is called `main`.

**What was tested:** files made by the same SQLite library the app carries (3.46.0), in the default rollback-journal mode and in WAL mode, by the tests in `crates/core/tests/sqlite.rs` and by the end-to-end test. **Not tested:** files written by other programs or other SQLite versions (iOS and Android apps, Django, browsers), a file another program is writing to at the same moment (beyond a held lock, which is tested), very large files, virtual tables (FTS and the like), an encrypted (SQLCipher) file, Windows paths, the native Open dialog.

| Feature | SQLite |
|---|---|
| Open an existing file; a path that does not exist, a folder, or a file that is not a database is refused in plain words, and no file is created | Implemented & verified |
| Read-only connection: the file is opened so that the handle cannot write (`SQLITE_OPEN_READONLY`); the default for a new SQLite connection | Implemented & verified (grid, console, `PRAGMA`, DDL all refused; the file's bytes unchanged) |
| List tables and views (SQLite's own `sqlite_…` tables left out) | Implemented & verified |
| Describe: columns, declared types, defaults, NOT NULL, composite keys in key order, generated columns, `WITHOUT ROWID`, `STRICT` | Implemented & verified |
| Browse rows: page, sort; a view | Implemented & verified |
| Filter rows | Implemented & verified (every operator) |
| One cell read whole | Implemented & verified |
| Edit, insert, delete rows of a table with a primary key, in one transaction, with the optimistic check | Implemented & verified |
| Smart Paste: INSERT, UPDATE, UPSERT matched on the primary key or a unique index | Implemented & verified |
| SQL console: scripts, impact guard, row cap, Stop, the user's own transaction, triggers | Implemented & verified |
| The AI's tools: catalogue, a reading query on a handle that cannot write, proposals | Implemented & verified in the backend; not run through the chat panel |
| Estimated row count | Not available: SQLite keeps none |
| Exact row count, asked for and stoppable | Implemented & verified in the backend; not through the UI |
| Tables without a primary key | Read-only in the grid (rows can be added), as on the other engines. The row id is not offered as a key |
| "New database…", "Edit database…" and "Drop database…" in the menus | Not offered: the file is its one database, so there is nothing to create, rename or drop inside it; the backend refuses as well (verified). See "New database" and "Database menu" |
| "Open SQL file…" in the menus | Offered, as on the other engines. Implemented, not yet verified on SQLite |
| The table menu: Definition, Rename, Empty, Drop (see "Table menu") | Implemented & verified in the backend (one test on a real file: the definition as the file keeps it, with the table's index and trigger; a table renamed; a view's rename refused; Empty as a `DELETE` that fires the table's trigger for every row; a view and a table dropped; a file opened read-only refused, while planning still works). Not through the UI |
| Create a database file; attached databases; encrypted databases | Not built |

### Types: a declared type is a suggestion

SQLite gives a column an *affinity* from its declared type and lets any value into any column, unless the table is `STRICT`. PuriCat reads two things from the declared type:

| Declared type contains | Affinity (SQLite's rule, in this order) | Shown and compared as |
|---|---|---|
| `INT` | INTEGER | Number |
| `CHAR`, `CLOB`, `TEXT` | TEXT | Text |
| `BLOB` | BLOB | Binary: `0x` and hex, the first 24 bytes in the grid; not editable; filtered on NULL only |
| `REAL`, `FLOA`, `DOUB` | REAL | Number |
| anything else | NUMERIC | By name: `NUMERIC`, `DECIMAL`, `DEC`, `NUMBER` → number; `BOOL…` → boolean; `DATE…`, `TIME…` → date/time; `JSON` → JSON; any other name (`UUID`, `STRING`, …) → "other" |
| nothing (no declared type), or `ANY` in a `STRICT` table | none | "Other"; **shown, not edited** (see below) |

- **Integers are exact**, to the last digit of a 64-bit value, in the grid, in filters, in keys and in the console.
- **A REAL is shown as SQLite converts it to text: 15 significant digits** (`0.1`, `1.0e+20`). Two different stored values can therefore look the same; the optimistic check compares what is shown.
- **SQLite has no exact decimal type.** A `DECIMAL(10,2)` column stores an integer or a floating-point number. `12.50` is stored and shown as `12.5`.
- **Dates and times are text** (or numbers, if the application stores them so). They are shown as stored and compared as text: `2026-01-31` sorts before `2026-02-01`, and "starts with `2026-01`" finds January. Nothing validates them.
- **Booleans** are whatever is stored, usually `1` and `0`.
- **JSON** is text; the whole-value dialog lays it out for reading. A column holding SQLite's binary JSONB format has not been tested.
- A declared length (`VARCHAR(20)`) is not enforced by SQLite, and PuriCat does not enforce it either.
- A value that is not what its column's kind says (text in an `INTEGER` column, a BLOB in a `TEXT` column) is shown as text; bytes that are not valid UTF-8 are replaced.

### What PuriCat checks because SQLite would not

- **Text that is not a number is refused for a number column** when saving from the grid, in Smart Paste, and as a filter value. SQLite would store it as text (or, in a comparison, quietly match nothing). The SQL console is not checked: there SQLite behaves as SQLite.
- **A column without a declared type is not edited.** It has no affinity, so a number typed in the grid would be stored as text. As a key column it is matched on the text shown.
- A `STRICT` table is enforced by SQLite itself; its refusal is shown as it is.

### Keys and generated values

- `INTEGER PRIMARY KEY` of an ordinary table is the row id. SQLite assigns it; the grid does not write it, and a blank key in a Smart Paste UPSERT means "new row". A deleted row's id can be reused by a later insert (without `AUTOINCREMENT`).
- In a `WITHOUT ROWID` table an `INTEGER PRIMARY KEY` is an ordinary, editable column.
- Generated columns (`GENERATED ALWAYS AS`, virtual or stored) are not written.
- Smart Paste can match on the primary key or on a unique index over plain columns. Partial indexes and indexes over expressions are not offered.

### Row filter

The same operators as the other engines, with "all" or "any" of the conditions as there. Comparison is by the column's affinity: text sent for a number column meets it as a number. "Is not" keeps NULL rows (`IS NOT`). A list is `col IN (?, …)` without casts, and "is not one of" keeps NULL rows as on the servers; every value of a list or a range for a number column must be a number. Dates are text there, so a range of dates compares as text. Text search uses `LIKE … ESCAPE '!'`, which in SQLite ignores the case of ASCII letters only.

### Writing

- A save is one transaction that takes the write lock at once (`BEGIN IMMEDIATE`). A lock held by another program is waited for, for as long as the connection timeout, then reported ("database is locked"); nothing is saved.
- Foreign keys are enforced on PuriCat's connections (`PRAGMA foreign_keys = ON`, a per-connection setting that is not stored in the file), whether or not the application that owns the file enforces them.
- Nothing else about the file is set: journal mode, page size and the rest stay as they are.

### SQL console

- A query tab has its own connection to the file. Stop interrupts the statement from inside (a progress handler looks at a flag every 1,000 steps of SQLite's virtual machine).
- **An interrupted write ends the user's transaction**: SQLite rolls all of it back. The console says so and no longer shows a transaction as open.
- DDL is transactional; nothing commits implicitly.
- `CREATE TRIGGER … BEGIN … END` is kept as one statement although its body has semicolons.
- `PRAGMA`, `VACUUM`, `ATTACH`, `ANALYZE` are classed as "other": they run as typed, outside the impact guard. `ATTACH` gives the tab access to another file; attached databases do not appear in the explorer.
- Names may be quoted as `"…"`, `` `…` `` or `[…]`. Block comments do not nest.
- Result values are shown by their own type (SQLite types values, not columns): a BLOB as `0x` and hex wherever it appears.

### Known limitations (SQLite)

- No row-count estimate. The exact count is there when asked for (verified in the backend, with and without a filter, and stopped on a view that never ends).
- A table without a primary key cannot be edited, though SQLite could address its rows by row id.
- A file in WAL mode opened read-only needs its `-wal` and `-shm` files to be readable (and, if they do not exist yet, the folder to be writable); otherwise SQLite reports that it cannot open the file. This case has not been tested.
- An encrypted (SQLCipher) file should be refused as "not a SQLite database, or it is encrypted"; only a plain non-database file was tested.

## SQL Server — Planned (Phase 2b)

Nothing is implemented beyond identifier quoting (`[name]`, unit-tested) and the engine value in saved profiles. The connection dialog shows SQL Server as unavailable, and the backend returns an `unsupported` error for it.

Plan:

- Driver: `tiberius`, subject to evaluating its maintenance cadence and its TLS and authentication options ([ADR 0003](docs/adr/0003-database-drivers.md)).
- Testing needs a disposable SQL Server instance. The development machine has no Docker, so this is a blocker ([ROADMAP.md](ROADMAP.md)).
- Namespaces: databases → schemas → objects, as in PostgreSQL.
- Rendering to text will need per-type `CONVERT` styles (for example for `datetime2`, `money`, `varbinary`); to be designed and tested.
- The write path (`db::write`) and Smart Paste (`db::smart`) have no SQL Server statements yet. When they do, Smart Paste will likewise match rows first and run plain `INSERT` and `UPDATE` statements; `MERGE` will not be used blindly ([ADR 0014](docs/adr/0014-smart-paste-match-then-write.md)). Planned.
- The SQL console has no SQL Server path either. The statement scanner (`db::script`) knows the MySQL and PostgreSQL lexical rules only; bracketed identifiers, `GO` batches and T-SQL transaction statements are not handled. Planned.
- Backups: `BACKUP DATABASE` writes to a path on the server, not the client ([BACKUP_RESTORE.md](BACKUP_RESTORE.md)).

**Integrated authentication.** Windows integrated authentication from macOS is only plausible through Kerberos/GSSAPI with a ticket obtained by `kinit` on a machine that can reach the domain's KDC. This is unverified with any driver. Until tested, the only SQL Server authentication PuriCat will claim is SQL login (user name and password). Microsoft Entra ID authentication has not been evaluated.
