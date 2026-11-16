# Database compatibility

Status on 2026-11-16. Labels: **Implemented & verified** (a test against a real server passed), **Implemented, not yet verified**, **Planned (Phase N)**.

## Tested versions

| Engine | Tested version | Other versions |
|---|---|---|
| MySQL | 9.3.0 (local) | Untested, including 8.x, 5.7 and MariaDB |
| PostgreSQL | 16.13 (local) | Untested |
| SQL Server | — | No adapter |

Only these two versions have been exercised. Nothing is claimed for older, newer or compatible-protocol servers (MariaDB, Aurora, CockroachDB and similar).

## Feature matrix

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
| Composite primary keys, names needing quotes | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Keyset pagination, exact row count, cell inspector | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
| Query cancellation, reconnect | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
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
| Smart Paste (INSERT / UPDATE / UPSERT wizard), batching | Planned (Phase 3) | Planned (Phase 3) | Planned (Phase 3) |
| Run SQL, DDL, import/export | Planned (Phase 4) | Planned (Phase 4) | Planned (Phase 4) |
| Backup and restore | Planned (Phase 6) | Planned (Phase 6) | Planned (Phase 6) |

## Capability flags

Reported by each adapter so the UI asks instead of guessing from the engine name.

| Flag | MySQL | PostgreSQL | Meaning |
|---|---|---|---|
| `schemas` | false | true | Objects live in schemas inside a database |
| `transactionalDdl` | false | true | DDL can be rolled back |
| `returning` | false | true | `INSERT/UPDATE/DELETE ... RETURNING` is available |

Only `schemas` affects behaviour today. The write path does not use `returning` or `transactionalDdl` yet: after a save the grid reloads the page instead of reading returned rows.

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
- Truncated cells are flagged. The full value cannot be viewed or edited yet.

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
| Truncated and binary cells are not editable | A cell cut to the preview length (1000 characters) cannot be edited because its full original value is unknown; binary and MySQL spatial columns are never editable. There is no full-value editor yet (Phase 3) |
| Rows with a NULL or truncated key value are not editable | The row cannot be addressed safely |
| Keyless tables are read-only in the UI | The backend refuses UPDATE and DELETE on a table without a primary key and would allow INSERT, but the UI keeps such tables fully read-only for now. Unique keys are not used as a substitute |
| Views and materialized views are read-only | Even where the engine would allow an updatable view |
| Generated, identity, auto-increment and serial columns cannot be written | Including on insert: an explicit value for such a column cannot be supplied from the grid |
| Edits are per page | Staged edits live on the loaded page; paging, sorting and refresh are locked until Save or Discard. One save handles at most 10,000 row changes, as single-row statements without batching or progress |
| Empty input and NULL | An empty cell means NULL only in a nullable column that is not a text type; in text columns it is the empty string. "Set to NULL" in the context menu is explicit. A copied NULL is placed on the clipboard as an empty cell |
| Validation happens on the server | There is no client-side type check; a bad value is reported at Save and nothing is saved |
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
| No cancellation or reconnect | A slow query runs to completion; a dropped connection surfaces as an error on the next action |
| Permissions | Catalogue queries assume read access to `information_schema` / `pg_catalog`. Behaviour under restricted accounts is untested |
| Large tables | No performance measurements exist |

## SQL Server — Planned (Phase 2b)

Nothing is implemented beyond identifier quoting (`[name]`, unit-tested) and the engine value in saved profiles. The connection dialog shows SQL Server as unavailable, and the backend returns an `unsupported` error for it.

Plan:

- Driver: `tiberius`, subject to evaluating its maintenance cadence and its TLS and authentication options ([ADR 0003](docs/adr/0003-database-drivers.md)).
- Testing needs a disposable SQL Server instance. The development machine has no Docker, so this is a blocker ([ROADMAP.md](ROADMAP.md)).
- Namespaces: databases → schemas → objects, as in PostgreSQL.
- Rendering to text will need per-type `CONVERT` styles (for example for `datetime2`, `money`, `varbinary`); to be designed and tested.
- The write path (`db::write`) has no SQL Server statements yet. Upsert will not blindly use `MERGE` (Phase 3).
- Backups: `BACKUP DATABASE` writes to a path on the server, not the client ([BACKUP_RESTORE.md](BACKUP_RESTORE.md)).

**Integrated authentication.** Windows integrated authentication from macOS is only plausible through Kerberos/GSSAPI with a ticket obtained by `kinit` on a machine that can reach the domain's KDC. This is unverified with any driver. Until tested, the only SQL Server authentication PuriCat will claim is SQL login (user name and password). Microsoft Entra ID authentication has not been evaluated.
