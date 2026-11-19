# Backup and restore

**Status: backups are built; a restore into a new database is built; a restore over an existing database is not.** PuriCat makes a backup of one MySQL or PostgreSQL database with the vendor's own dump tool, when you ask and on a schedule, into a folder you choose. It can load one of its backups into a **new** database on the same connection, so that you can look at it beside the original. Replacing an existing database is done with the vendor's tools, outside PuriCat: the commands are below. The decision records are [ADR 0021](docs/adr/0021-backups-with-dump-tools.md) (backups, which builds on [ADR 0010](docs/adr/0010-backup-scheduler.md)) and [ADR 0022](docs/adr/0022-restore-into-a-new-database.md) (restore).

**Verified on one pair of versions**: `mysqldump` and `mysql` 9.3.0 with MySQL 9.3.0, and `pg_dump`, `pg_restore` and `psql` 16.13 with PostgreSQL 16.13, on macOS. Until you have restored one of your own backups, treat it as untested: only a restore proves a backup. [TESTING.md](TESTING.md) says what was run.

A dump is a snapshot of one database at the time it ran. It is not a replacement for server-side physical backups or point-in-time recovery.

## What you need

| Engine | Tool | Where it comes from |
|---|---|---|
| MySQL | `mysqldump`; to restore, `mysql` next to it | MySQL's client tools, for example `brew install mysql-client` |
| MariaDB | `mariadb-dump` (or MariaDB's `mysqldump`); to restore, `mariadb` or `mysql` next to it | MariaDB's client tools. MySQL's `mysqldump` is used with a warning when it is the only one found |
| PostgreSQL | `pg_dump`, and `pg_restore` next to it; to restore a SQL dump, `psql` next to it | PostgreSQL's client tools, for example `brew install libpq`. **`pg_dump` must not be older than the server** |
| SQLite, SQL Server | — | No backup in PuriCat. A SQLite database is one file: copy it while nothing writes to it |

The tools are not shipped with PuriCat. The backup dialog shows which tool it found, its version and the server's, and says what is missing.

## Making a backup

Right-click a database → **Back up…**

- **Folder.** "Choose…" opens the system's folder dialog. The first offer is `Documents/PuriCat Backups`. The choice is remembered for that database.
- **File.** `<connection>_<database>_<date>_<time>.sql.gz`, or `.dump` for a PostgreSQL archive. Readable by you only. An existing file is never overwritten.
- **Format.** SQL statements, gzipped unless you switch that off. For PostgreSQL the default is its own archive format, which is compressed and can be restored in parts.
- **Back up now** shows the size so far and can be stopped. A backup that fails or is stopped leaves no file.

A file is given its name only after the tool finished without error and the dump was checked: a SQL dump must end with the tool's closing line, an archive must be listed by `pg_restore`.

## What a backup contains

| Included | Not included |
|---|---|
| Table definitions and data | Users, roles and their passwords |
| Indexes, constraints, views | Server configuration |
| Routines, triggers and events (MySQL); functions and sequences with their values (PostgreSQL) | Other databases on the server |
| | Privileges granted to roles that do not exist on the target |
| | Binary logs / WAL: no point-in-time recovery |

MySQL's transactional (InnoDB) tables are dumped as one consistent snapshot, without locking them. Tables of other storage engines (MyISAM) are read without a lock and may not match each other if something writes during the dump.

## Scheduled backups

In the same dialog: **Every day** or **Weekly**, a time, and how many scheduled backups to keep.

- Saving a schedule installs a small background runner for your user in macOS (a LaunchAgent, `~/Library/LaunchAgents/com.purivaro.puricat.backups.plist`). macOS tells you that a background item was added. It starts PuriCat's own program, without a window, at the scheduled times, **also while PuriCat is closed**. Removing the last schedule removes the runner.
- The dialog shows the next time only when the runner is really installed. Otherwise it says that the schedule will not run.
- A failed run is in the history with its reason, and macOS shows a notification.
- After each scheduled backup that was kept, older scheduled backups of that database beyond the number to keep are deleted (7 unless you change it; 0 keeps all). **Backups you made by hand are never deleted.**

### When a scheduled backup does not run

| Situation | What happens |
|---|---|
| Mac asleep or powered off at the scheduled time | Nothing runs. After it wakes, the missed backup runs once, not once per missed day |
| You are logged out, or the login keychain is locked | The runner cannot read the password; the run fails and is recorded |
| The dump tool is missing or older than the server | The run fails with that reason |
| The server cannot be reached (VPN down) | The run fails; nothing is tried again before the next scheduled time |
| The folder is missing (an external disk that is not plugged in) | The run fails; nothing is deleted |
| PuriCat was moved or deleted | Open PuriCat once after moving it: it points the runner at the new place. After deleting it the runner starts nothing; remove the file in `~/Library/LaunchAgents` by hand |

The runner's own log is `backup-runner.log` in PuriCat's settings folder (`~/Library/Application Support/com.purivaro.puricat/`).

## Restoring a backup into a new database

In the backup dialog, every backup in the history that still has its file has **Restore…**

- **The only thing to choose is the name of a new database.** The name offered is the source's with when the backup was made, for example `shop_restored_20261119_1405`. A name that is taken is refused: nothing that exists is replaced, on any server, by this feature.
- **What will run is shown first**: the `CREATE DATABASE` statement, and the program that loads the backup (`mysql`, `psql` or `pg_restore`, the one installed next to the dump tool) with its version.
- On MySQL the new database gets the default character set and collation of the database the backup came from, while that one is still on the server.
- A SQL dump shows how much of the file was read; an archive is read by `pg_restore` itself and shows the time only. **Stop** ends the load.
- Afterwards the new database is in the list on the left. Look at it before you rely on it.

What is restored is the file PuriCat wrote, and only that:

- A backup of the history, from this connection, whose file is still there and still has the size that was recorded. A file that changed size since it was made is refused.
- A SQL dump is read to its end first and must end with the dump tool's closing line; an archive is listed by `pg_restore` first. A file that is not whole loads nothing.

When a restore fails or is stopped:

| Engine | What the new database holds |
|---|---|
| PostgreSQL | Nothing. The load is one transaction (`--single-transaction`), ended by the first error |
| MySQL | What was loaded up to then. MySQL commits each `CREATE TABLE` as it goes; a load cannot be undone |

In both cases the database that was created for the restore is still there, and the message says so. PuriCat does not drop it by itself: drop it from its menu (**Drop database…**) before trying again with the same name.

Not possible in PuriCat: restoring over an existing database, restoring onto another connection, restoring a file PuriCat did not make. A read-only connection cannot restore. A restore is not recorded in the history.

## Restoring a backup by hand

For what PuriCat does not do, above all replacing an existing database. Restore into a **new, empty database** first, and look at it, before anything else. Replace the names in capitals.

MySQL (`.sql.gz`, or `.sql` without the `gunzip`):

```sh
mysql -h HOST -u USER -p -e 'CREATE DATABASE restored_copy'
gunzip -c BACKUP.sql.gz | mysql -h HOST -u USER -p restored_copy
```

PostgreSQL archive (`.dump`):

```sh
createdb -h HOST -U USER restored_copy
pg_restore -h HOST -U USER --no-owner -d restored_copy BACKUP.dump
```

PostgreSQL SQL dump (`.sql.gz`):

```sh
createdb -h HOST -U USER restored_copy
gunzip -c BACKUP.sql.gz | psql -h HOST -U USER -d restored_copy --set ON_ERROR_STOP=1
```

These are the commands PuriCat's own tests use to check its backups. A restore is not atomic: if it stops half way, the target holds what was restored up to then.

## Where things are kept

| What | Where |
|---|---|
| Backup files | The folder you chose. PuriCat keeps no copy |
| The settings of each database's backups, and the history of runs | PuriCat's settings database (`backup_jobs`, `backup_runs`). The history holds the last 2,000 runs; dropping an entry does not delete its file |
| Passwords | The macOS Keychain, as for connecting. During a MySQL backup the password is in a file only you can read, in a private temporary folder, and is removed when the run ends |

Backup files are **not encrypted**. Anyone who can read the folder can read the data: choose the folder accordingly, especially on an external disk or in a folder that syncs to a cloud service.

## Planned, not built

- **Restore over an existing database**, with these rules: the default target is never the source database; it requires typing the database's name, and on a Production connection a fresh backup is offered first; an AI agent cannot start a restore without the same approval as any other change; a file PuriCat did not create is treated as an untrusted SQL script. (Built so far: a restore into a new database, above.)
- **A record of restores**, and a checksum of every backup in the history, so that a changed file is known by more than its size.
- **Encryption** of backup files, with the key in the Keychain.
- **A restore test as an action**: restore into a scratch database and compare, as PuriCat's own tests do.
- **SQL Server**: `BACKUP DATABASE` writes to a path on the server, not on this computer. It will be offered as a separate, clearly labelled strategy.
- **Windows**: Task Scheduler with the same runner.
- Backups of several databases at once, a time limit for a scheduled run, retention by age.
