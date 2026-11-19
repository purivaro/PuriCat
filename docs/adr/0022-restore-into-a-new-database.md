# 0022 — Restore, first slice: a backup of the history, into a database created for it

## Status

Accepted. It builds the first part of the restore that [BACKUP_RESTORE.md](../../BACKUP_RESTORE.md) planned and [ADR 0021](0021-backups-with-dump-tools.md) left out. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-19 (restore into a new database)".

**Verified on one pair of versions only**, the same as the backups: `mysql` 9.3.0 against MySQL 9.3.0, and `pg_restore` / `psql` 16.13 against PostgreSQL 16.13, on the development Mac. MariaDB, other versions, servers that need a password or enforce TLS, and a large backup are handled in the code and its unit tests, but **no restore of any of them has been made**.

## Context

A backup that cannot be restored where it was made is half a feature: until now the user had to copy commands from a document. The owner asked for the remaining features to be taken in the roadmap's order (2026-11-19); restore is the next item of Phase 6.

A restore is the most destructive thing PuriCat could do. The plan in `BACKUP_RESTORE.md` lists what a restore over an existing database needs: the name typed, a fresh backup offered first on Production, a file PuriCat did not make treated as an untrusted script. Each of those is a way to make replacing data safer. The safest restore replaces nothing.

Constraints:

- Nothing that exists may be changed by a restore that goes wrong, or by a slip of the user's.
- What is loaded must be a backup PuriCat made, and still the file it wrote.
- The password must not be an argument, as for a backup.
- A restore that fails must say what the server holds afterwards.

## Decision

### 1. Only into a database that PuriCat creates for it

- The restore dialog asks for one thing: the name of a **new** database on the same connection. The name offered is the source's with when the backup was made (`shop_restored_20261119_1405`), never the source's own.
- A name that is taken is refused before anything runs, and again by the server: the database is made with `CREATE DATABASE`, built by the builder of [ADR 0019](0019-schema-changes-through-forms.md), which has no `IF NOT EXISTS`. The statement is shown before it runs.
- So there is no path by which this feature drops, truncates or replaces anything. The rules for restoring over an existing database are not needed yet, and are not built.
- On MySQL the new database gets the default character set and collation of the source, while the source is still on the server: a dump of one database does not carry them. Otherwise the server's default applies.
- A read-only connection cannot restore.

### 2. Only a backup of the history, and only as PuriCat wrote it

- The web view names a run of the history, never a path. The run must belong to the connection, have been kept, and not have been removed by retention.
- The file must still have the size the history recorded. This is not proof against a deliberate change (no checksum is stored); it catches a file that was cut, replaced or edited by accident.
- A SQL dump is read to its end before any of it is loaded, and must end with the dump tool's closing line — the same check that named it. A PostgreSQL archive is listed by `pg_restore --list` first.

### 3. The vendor's client, the one installed with the dump tool

- `mysql` (for MariaDB's tool: `mariadb`, else `mysql`) loads a MySQL dump; `psql` a PostgreSQL SQL dump; `pg_restore` a PostgreSQL archive. The client is looked for next to the dump tool chosen for the server, so its version is the tool's.
- For an archive, the tools must be at least as new as the `pg_dump` the history says made it.
- The same rules as for a backup: a fixed argument vector, no shell, the database name after `--` or as one `--dbname=` argument, the password in MySQL's private option file or in `PGPASSWORD`, a cleared environment, the connection's TLS setting in the client's own terms. `mysql` runs with `--binary-mode`.
- PuriCat unpacks a gzipped dump itself and feeds the client's standard input; the progress shown is the part of the file that was read. `pg_restore` reads an archive itself, so no progress is known for it.

### 4. PostgreSQL loads all or nothing; MySQL cannot, and the dialog says so

- `psql` runs with `--single-transaction` and `ON_ERROR_STOP`; `pg_restore` with `--single-transaction --exit-on-error --no-owner`. A load that fails or is stopped leaves the new database empty.
- MySQL commits every `CREATE TABLE` as it goes. A load that fails or is stopped leaves what was loaded up to then. The dialog says which of the two applies before the restore starts, and the error says it afterwards.
- PuriCat does not drop the new database after a failure. It was created seconds earlier by the same command, so dropping it would almost certainly be right — and "almost certainly" is not the standard for a `DROP DATABASE` nobody asked for. The message says that the database is still there and how to drop it.

### 5. Not built

- **Restoring over an existing database**, with the typed name and, on Production, a fresh backup first.
- **A file PuriCat did not make**, or one made from another connection; a restore onto another connection.
- **A restore test** that compares the restored database with the source.
- **A record of restores.** A restore is not in the backup history or in any log; the new database is its trace.
- **Encryption** of backup files.

## Alternatives considered

| Option | Why not |
|---|---|
| Restore over the source database, behind a typed confirmation | The first slice of a destructive feature should not be the destructive one. A new database covers "get yesterday's row back" and "is this backup any good?", and replaces nothing |
| Let the user pick any `.sql` file | That is running an untrusted script with the connection's rights; the SQL console does it with its guards. A restore keeps to files whose origin PuriCat knows |
| `CREATE DATABASE IF NOT EXISTS`, or loading into an existing empty database | "Empty" would have to be checked and could change between the check and the load. A database created by this very command is empty by construction |
| Drop the new database after a failed load | A drop the user did not ask for. The partial result may also be what they want to look at |
| Load a MySQL dump through PuriCat's own connection | A dump uses client commands (`DELIMITER`) and version comments that the `mysql` client understands and a driver does not |
| One transaction on MySQL | DDL commits implicitly; the claim would be false |
| A checksum of every backup in the history | The right guard against a changed file. It needs a migration and a pass over every existing file; left for the slice that accepts files from elsewhere |

## Consequences

- A backup can be checked and used without leaving the app: restore it beside the original, look, copy what is needed.
- The server needs room for a second copy, and the account the privilege to create a database. Where it has neither, the server's refusal is shown and nothing changed.
- On MySQL a dump made by an account with more rights than the one that restores may fail on a `DEFINER` clause; the load then stops with the server's message and the new database holds what came before.
- New: two IPC commands (`restore_plan`, `restore_backup`); no migration, no new dependency, no new Tauri capability, no change to the Content-Security-Policy.

Limitations:

- **The same connection only.** A backup is restored onto the server it came from.
- **No time limit**, and for an archive no progress; Stop ends the client.
- **Users, privileges and other databases are not in a dump**, so they are not restored.
- **The size check is not a signature.** Someone who can write to the backup folder can change a backup and keep its size; the dump tool's closing line is not a secret either.
- **A cut in the data of an archive is found only while loading**: `pg_restore --list` reads the table of contents at the front of the file. Nothing is loaded then, because the load is one transaction. (The same check names an archive after a backup; there it follows a `pg_dump` that ended without error.)
