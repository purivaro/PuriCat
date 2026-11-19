# 0021 — Backups as built: the vendors' dump tools, a checked file, a LaunchAgent that starts the app's own executable

## Status

Accepted. It builds what [ADR 0010](0010-backup-scheduler.md) proposed and departs from it in three places, named under "Decision". Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-19 (backups)".

**Verified on one pair of versions only**: `mysqldump` 9.3.0 against MySQL 9.3.0, and `pg_dump` 16.13 against PostgreSQL 16.13, all on the development Mac. Every backup made in the tests was restored into a second database and compared row for row. MariaDB (as a server or as a tool), older MySQL servers and tools, TLS-enforcing servers and servers that need a password are handled in the code and its unit tests but **no backup of any of them has been made**. The scheduled path was run once for real: macOS started the runner at the scheduled minute with the app closed, from a development build with a label of its own. No scheduled backup has run from an installed release.

## Context

The owner asked for backups of a database: made by hand, made on a schedule, and into a folder of their choice (2026-11-19). ADR 0010 had settled the direction: the vendors' tools, and the operating system's scheduler for runs while the app is closed.

Constraints:

- A backup that is silently incomplete is worse than none. Whatever is written must be known to be whole, and a failure must leave nothing that looks like a backup.
- The password must not be visible to other processes (an argument is, through `ps`).
- A schedule must not be shown as working when nothing will run it.
- The app is used against production servers: a backup must not lock tables or change anything.
- Deleting old backups is deleting the user's files.

## Decision

### 1. The vendors' tools, found on this computer

- MySQL: `mysqldump`, or MariaDB's `mysqldump` / `mariadb-dump`. PostgreSQL: `pg_dump`. They are looked for in `PATH`, in Homebrew's directories (including the versioned kegs that Homebrew keeps out of `PATH`), in Postgres.app and in the vendors' installer locations. Each is asked for its version.
- The tool is chosen for what the server reports. `pg_dump` must not be older than the server (it refuses); the same major version is preferred, else the newest. For MySQL the vendor's own client is preferred; the other vendor's is used with a warning; an older `mysqldump` than the server gets a warning.
- When no tool fits, the dialog says which is missing and how it is usually installed. Nothing is downloaded or bundled.

### 2. A fixed argument vector; the password is never an argument

- No shell. The database name is the last argument, after `--`.
- `mysqldump` is given `--defaults-file=<file>`: an option file in a directory only this user can enter (mode 0700, the file 0600), holding the password and nothing else, removed when the run ends. It replaces the user's own option files, so nothing in `~/.my.cnf` changes which server is dumped.
- `pg_dump` gets the password in `PGPASSWORD`, and the TLS policy in `PGSSLMODE` and `PGSSLROOTCERT`. Its environment is cleared first: none of the user's `PG*` variables reaches it.
- The connection's TLS setting is passed to the tool in the tool's own terms.
- `mysqldump` runs with `--single-transaction --quick --routines --triggers --events --hex-blob --set-gtid-purged=OFF`, and from version 8 `--no-tablespaces --column-statistics=0`. One consistent snapshot of the transactional tables, no table locks, nothing written.

### 3. A file gets its name only when it is known to be whole

- The dump is written as `.<name>.partial` in the destination, created readable by its owner only.
- A SQL dump must end with the tool's own closing line (`-- Dump completed`, `PostgreSQL database dump complete`). A PostgreSQL archive must be listed by `pg_restore --list`.
- Only then is it renamed. After a failure, a stop or a missing closing line the partial file is removed: the destination never holds something that only looks like a backup.
- Names are `<connection>_<database>_<YYYY-MM-DD_HHMMSS>.<ext>`, with spaces and the characters a path or a shell gives a meaning replaced; an existing name is never overwritten.
- Formats: SQL, gzipped by PuriCat unless switched off; for PostgreSQL also its custom archive, which is the default there.

### 4. One job per database; the folder is the user's choice

- A job (`backup_jobs`) holds the folder, the format, the schedule and how many scheduled backups to keep. It is also where a backup made by hand remembers its folder.
- The folder is chosen in the native dialog, shown by the backend. The first offer is `~/Documents/PuriCat Backups`; it is created only when a backup is made or a schedule is saved.
- Every run is a row of `backup_runs`: by hand or scheduled, kept, failed or stopped, with the tool, the size, the file and the reason. A failed run is never shown as anything else.

### 5. Scheduled backups: launchd starts the app's own executable

Departure from ADR 0010, which proposed a separate `puricat-backup` helper: the runner is the app's executable, started as `puricat --run-backups --data-dir <dir>`. It opens no window. The reasons are those of [ADR 0017](0017-ai-database-tools.md) for the MCP bridge: one program to build, sign and find, and it is the program whose signature the Keychain already knows.

- Saving a schedule writes `~/Library/LaunchAgents/com.purivaro.puricat.backups.plist` (a per-user LaunchAgent) with one `StartCalendarInterval` entry for every time some job is scheduled for, and loads it with `launchctl bootstrap`. Removing the last schedule unloads and deletes it. The dialog says this before the schedule is saved; saving is the consent.
- The runner takes a lock (one at a time), reads the jobs, and runs those that are due: a scheduled time has passed that is later than the job's last scheduled run and than the moment the job was saved. So a job never runs for a time before it existed, and a time missed while the Mac slept is made up for once, not once per night.
- It connects with the password from the Keychain to learn the server's version, chooses the tool, makes the backup, writes the run into the history and a line into `backup-runner.log`. A failure is also shown as a system notification.
- The app shows a "next backup" time only when launchd has the agent loaded and the agent starts this copy of the app. Otherwise it says that the schedule will not run. At start-up the app re-installs the agent if schedules exist and the agent points elsewhere (the app was moved).
- A development build installs nothing unless `PURICAT_BACKUP_AGENT=1`, and then under a label of its own (`….backups.dev`).

### 6. Retention deletes only what the schedule made, and only after a newer one was kept

After a scheduled backup that was kept, the job's older **scheduled** backups beyond the number to keep are deleted (default 7; 0 keeps all). Backups made by hand are never deleted and do not count. A failed run deletes nothing. The history keeps the entry and says the file was removed.

### 7. Not built

Departures from ADR 0010 and `BACKUP_RESTORE.md`, which remain the plan:

- **Restore.** There is no restore in the app. `BACKUP_RESTORE.md` gives the commands. A restore through the app needs the safety rules written there, and is a feature of its own.
- **Encryption.** Backup files are not encrypted. They are readable by their owner only, and as safe as the folder they are in.
- **SQL Server, SQLite.** Neither has a backup here.

## Alternatives considered

| Option | Why not |
|---|---|
| A dump written by PuriCat itself, through its own connection | Re-implements mature tools. A dump that misses a trigger or mis-escapes a value is found out on the day of the restore (ADR 0010) |
| The password in `MYSQL_PWD` | Deprecated by MySQL. The option file is the documented way |
| `--defaults-extra-file`, keeping the user's option files | A `[client]` section in `~/.my.cnf` with another host would then decide what is backed up |
| Write straight to the final name | A crash or a full disk leaves a file that looks like a backup |
| A separate helper binary for the runner | A second executable to build, sign, notarize and locate; and the Keychain would ask for it separately |
| A timer inside the app | Skips every run while the app is closed, which is when backups are wanted (ADR 0010) |
| One LaunchAgent per job | More files to keep in step; the runner decides what is due anyway |
| Run a missed backup once per missed time | Several dumps in a row after a long sleep, each of the same data |
| Retention by age | A Mac that was off for a month would delete everything on its first run. A count keeps the newest whatever their age |
| Count backups made by hand in the retention | A backup someone made on purpose before a risky change would be deleted by a schedule |
| Bundle the tools | Licensing (MySQL's client tools are GPL) and the version rule of `pg_dump` (ADR 0010) |

## Consequences

- A user can back a database up in two clicks, into any folder, and have it done every night with the app closed.
- Backups depend on tools PuriCat does not ship. Without them the dialog explains and offers nothing.
- A LaunchAgent in the user's account is a new kind of footprint: macOS announces it ("Background Items Added"), and it stays until the last schedule is removed or the connection deleted. Deleting the app does not remove it; the agent then fails to start and does nothing.
- The settings database is now also opened by the runner while the app may be open. Both use SQLite's own locking.
- New: migration 5 (`backup_jobs`, `backup_runs`); six IPC commands; `chrono` and `flate2` as direct dependencies of the core; the workspace's minimum Rust version is 1.89 (for `File::try_lock`, the runner's lock).
- No new Tauri capability, no change to the Content-Security-Policy.

Limitations:

- **macOS only** for the schedule; the manual backup is not tested elsewhere either.
- **One database per job.** No "all databases", no selection of tables.
- **Nothing runs while the Mac is asleep, off, or the user is logged out.** The Keychain must be unlocked.
- **A database behind a VPN is not backed up while the VPN is down.** The run fails and says so; nothing is retried before the next scheduled time.
- **No progress in percent.** The size of the dump so far is shown; the tools do not say how much is left.
- **MyISAM and other non-transactional tables** are dumped without a lock, so they may be inconsistent with each other if written to during the dump.
- **Users, privileges, other databases and point-in-time recovery are not in a dump** (`BACKUP_RESTORE.md`).
- **The folder is a path the web view names.** The backend requires an absolute path and creates the folder; it does not confine where that is.
- **A scheduled run has no time limit.** A dump that hangs holds the runner's lock until it is killed.
- **The history is capped at 2,000 runs**; older entries are dropped, their files are not.

## Licensing

`mysqldump`, `mariadb-dump`, `pg_dump` and `pg_restore` are started as separate programs that the user installed; none is redistributed. `chrono` (MIT OR Apache-2.0) and `flate2` (MIT OR Apache-2.0) are listed in [LICENSES.md](../../LICENSES.md).
