# 0003 — Database drivers and adapter shape

## Status

Accepted and implemented for MySQL and PostgreSQL. Proposed and unverified for SQL Server (Phase 2b).

## Context

PuriCat targets MySQL, PostgreSQL and SQL Server. Engines differ in namespaces, quoting, transaction semantics and catalogue queries; the brief asks for capability-aware adapters rather than SQL string substitution. The local settings store also needs SQLite.

## Decision

- **MySQL and PostgreSQL:** `sqlx` 0.8 with features `runtime-tokio`, `tls-rustls-ring-native-roots`, `mysql`, `postgres` (and `sqlite` for the local store). TLS is rustls with the OS root store. Connections are built from typed options, never URLs, so driver errors cannot echo a password embedded in a connection string.
- **Adapter shape:** a `Session` enum with one variant and one module per engine. Each module owns its SQL. A `Capabilities` struct (`schemas`, `transactionalDdl`, `returning`) tells the UI what an engine can do. Identifiers are quoted per engine in `db::ident`.
- **Pools:** MySQL max 4 connections; PostgreSQL max 3 per database, created on first use; idle timeout 600 s. A direct probe connection precedes the pool so failures report the driver's reason.
- **sqlx version:** stay on 0.8. sqlx 0.9 exists; upgrading is a tracked task, not yet evaluated.
- **SQL Server (planned):** `tiberius`. Before adoption, Phase 2b must evaluate its maintenance cadence, TLS options (it must work with rustls or the platform TLS stack) and authentication modes against a disposable server.

### SQL Server integrated authentication

Windows integrated authentication from macOS is only plausible through Kerberos/GSSAPI with a ticket obtained by `kinit`, on a machine that can reach the domain's KDC. Whether `tiberius` supports this on macOS has not been tested. Until it is, PuriCat will state SQL login (username and password) as the only supported SQL Server authentication and will not claim integrated auth.

## Alternatives considered

| Option | Why not |
|---|---|
| `mysql_async` + `tokio-postgres` | Two APIs and two pool implementations; no shared SQLite support |
| ODBC for all engines | Requires drivers installed on the user's machine; poor first-run experience for a public app |
| A trait object per adapter | An enum makes adding an engine a compile-time checklist: every `match` must handle it |
| Shelling out to `mysql` / `psql` | Text parsing, no pooling, client tools not guaranteed to exist |

## Consequences

- One async runtime (tokio) and one TLS stack (rustls) for both engines and the local store.
- SQL Server will be a second driver with its own types and TLS configuration; behaviour parity must be tested, not assumed.
- Only MySQL 9.3.0 and PostgreSQL 16.13 have been tested ([DATABASE_COMPATIBILITY.md](../../DATABASE_COMPATIBILITY.md)).
- Cancellation, streaming and reconnection are not implemented yet; the driver capabilities for them still need verification.

## Licensing

sqlx MIT/Apache-2.0; tokio MIT; bundled SQLite (via `libsqlite3-sys`) public domain; tiberius MIT/Apache-2.0 (candidate). No copyleft obligations.
