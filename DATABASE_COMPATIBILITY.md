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
| Browse rows of a view | Implemented, not yet verified | Implemented & verified | Planned (Phase 2b) |
| Composite primary keys, names needing quotes | Implemented & verified | Implemented & verified | Planned (Phase 2b) |
| Keyset pagination, exact row count, cell inspector | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
| Query cancellation, reconnect | Planned (Phase 2b) | Planned (Phase 2b) | Planned (Phase 2b) |
| Edit rows, paste, Smart Paste | Planned (Phase 3) | Planned (Phase 3) | Planned (Phase 3) |
| Run SQL, DDL, import/export | Planned (Phase 4) | Planned (Phase 4) | Planned (Phase 4) |
| Backup and restore | Planned (Phase 6) | Planned (Phase 6) | Planned (Phase 6) |

## Capability flags

Reported by each adapter so the UI asks instead of guessing from the engine name.

| Flag | MySQL | PostgreSQL | Meaning |
|---|---|---|---|
| `schemas` | false | true | Objects live in schemas inside a database |
| `transactionalDdl` | false | true | DDL can be rolled back |
| `returning` | false | true | `INSERT/UPDATE/DELETE ... RETURNING` is available |

Only `schemas` affects behaviour today. The other two are declared for the editing phase.

## Type rendering

Every value is converted to text by the server ([ADR 0004](docs/adr/0004-server-side-text-rendering.md)). The grid shows that text unchanged.

### MySQL

| Type | Shown as | Verified |
|---|---|---|
| Integers, `BIGINT` beyond 2^53, `DECIMAL` | Exact digits | Yes |
| `TINYINT(1)` | `0` / `1` (classified as boolean) | Classification only |
| `DATETIME(6)` and other temporal types | Server text, fractional seconds kept | Yes (`DATETIME(6)`) |
| `VARCHAR`, `TEXT` (utf8mb4) | Text; first 1000 characters | Yes, including Thai |
| `JSON` | Server's normalised JSON text | Yes |
| `BLOB`, `BINARY` family | `0x` + uppercase hex of the first 24 bytes | Yes |
| `BIT(n)` | Decimal number | Yes |
| Spatial types | WKT from `ST_AsText` | SQL builder unit-tested; not run against a server |
| `ENUM`, `SET` | Label text | No |

### PostgreSQL

| Type | Shown as | Verified |
|---|---|---|
| `bigint`, `numeric` | Exact digits | Yes |
| `boolean` | `true` / `false` | Yes |
| `timestamptz` | Text in the session time zone | Yes (prefix only) |
| `text` | Text; first 1000 characters | Yes, including Thai |
| `jsonb`, `json` | Server text | Yes (`jsonb`) |
| `bytea` | `0x` + lowercase hex of the first 24 bytes | Yes |
| Arrays | Array literal, e.g. `{x,"y z"}` | Yes |
| `uuid`, enum types | Text | Yes |
| Ranges, intervals, geometric, network, composite, PostGIS | Whatever `::text` yields | No |

Notes for both engines:

- NULL is transported separately from text, so NULL and the empty string are distinct in the grid.
- Temporal values are shown in the session's time zone and format, which come from server or role defaults. PuriCat does not set a time zone.
- Truncated cells are flagged. The full value cannot be viewed yet.

## Object classification

| Engine | Shown under Tables | Shown under Views |
|---|---|---|
| MySQL | `BASE TABLE` | Everything else (`VIEW`, `SYSTEM VIEW`) |
| PostgreSQL | Ordinary, partitioned and foreign tables | Views and materialized views |

System namespaces are marked: `information_schema`, `mysql`, `performance_schema`, `sys` in MySQL; `information_schema` and `pg_*` schemas in PostgreSQL (`pg_toast*` and `pg_temp*` are hidden). PostgreSQL template databases and databases that do not allow connections are not listed.

## Known limitations

| Limitation | Detail |
|---|---|
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
- Upsert will not blindly use `MERGE` (Phase 3).
- Backups: `BACKUP DATABASE` writes to a path on the server, not the client ([BACKUP_RESTORE.md](BACKUP_RESTORE.md)).

**Integrated authentication.** Windows integrated authentication from macOS is only plausible through Kerberos/GSSAPI with a ticket obtained by `kinit` on a machine that can reach the domain's KDC. This is unverified with any driver. Until tested, the only SQL Server authentication PuriCat will claim is SQL login (user name and password). Microsoft Entra ID authentication has not been evaluated.
