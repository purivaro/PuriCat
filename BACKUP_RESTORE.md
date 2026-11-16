# Backup and restore

**Status: nothing is implemented.** Backup and restore are Planned (Phase 6). There is no backup code, UI, helper binary or scheduler in the repository. This document records the design and the limits PuriCat will state to users. The decision record is [ADR 0010](docs/adr/0010-backup-scheduler.md).

Until Phase 6 ships, PuriCat must not be relied on for backups in any way.

## Plan per engine — Planned (Phase 6)

| Engine | Tool | Notes |
|---|---|---|
| MySQL | `mysqldump` | Consistent dump of transactional (InnoDB) tables taken inside a single transaction; non-transactional tables need locking, which will be stated. Client and server versions are checked before running |
| PostgreSQL | `pg_dump`, `pg_restore` | Custom format for selective restore, or plain SQL. `pg_dump` must not be older than the server; checked before running |
| SQL Server | `BACKUP DATABASE` (server-side) | **Writes to a path on the server, not on the client.** Needs server privileges and a server-visible path. Offered as a separate, clearly labelled strategy; an alternative client-side export, if any, will be documented as an export, not a backup |

The vendor tools are not bundled. PuriCat will detect them, show their versions, and explain what is missing. Exact flags and version rules will be confirmed against each tool's documentation in Phase 6.

Tools run from Rust with fixed argument vectors and no shell. Passwords are never placed on the command line.

## What a logical backup contains

A single-database dump from `mysqldump` or `pg_dump`:

| Included | Not included |
|---|---|
| Table definitions and data | Users, roles and their passwords |
| Indexes, constraints, views | Server configuration |
| Routines, triggers and events when the matching options are enabled | Other databases on the server |
| Sequences and their values (PostgreSQL) | Tablespaces and replication settings |
| | Privileges granted to roles that do not exist on the target |
| | Binary logs / WAL — no point-in-time recovery |

A dump is a snapshot at the time it ran. It is not a replacement for server-side physical backups or point-in-time recovery. The UI will say this where backups are configured.

## Scheduler design — Planned (Phase 6)

- **Manual backups** run inside the app while it is open.
- **Scheduled backups** on macOS use a per-user LaunchAgent (launchd) that starts a small `puricat-backup` helper built on `puricat-core`. The helper reads job definitions from the settings store and passwords from the Keychain, runs the job, applies retention and records the result.
- **Windows** will use Task Scheduler with the same helper (after the macOS beta).
- No privileged service is installed. Creating or removing the LaunchAgent requires explicit consent in the UI, and the UI shows whether it is installed.

### Limitations PuriCat will show

| Situation | What happens |
|---|---|
| Mac asleep or powered off at the scheduled time | Nothing runs. After wake, launchd runs a missed calendar job once, not once per missed interval |
| User not logged in, or login keychain locked | The helper cannot read credentials; the run fails and is recorded |
| LaunchAgent not installed or disabled | No scheduled runs; the UI shows schedules as inactive rather than pending |
| Vendor tool missing or incompatible with the server | The run fails with that reason |
| Server unreachable (VPN down, SSH tunnel needed) | The run fails; nothing is retried silently |
| Destination missing or full | The run fails; older backups are not deleted to make room |

PuriCat will not display a "next run" time unless the scheduler is actually installed and enabled, and will not promise unattended runs it cannot perform.

## Encryption, retention and verification — Planned (Phase 6)

- **Permissions:** backup files are created readable by the owner only.
- **Encryption:** optional, with the key or passphrase held in the OS credential store. The format and library are not chosen. A lost key means an unrecoverable backup; the UI will say so.
- **Compression:** optional; the format is not chosen.
- **Retention:** by count or by age. Retention deletes only files PuriCat created and only after a newer backup has completed successfully.
- **Destination:** a local folder. No cloud upload by default.
- **History:** each run records start, end, size, tool versions, exit status and a log without secrets.
- **Notifications:** failures are surfaced in the app and through an OS notification; a failed run is never shown as successful.
- **Verification:** a completed dump is checked for a clean exit and a readable archive (for the PostgreSQL custom format, by having `pg_restore` list its contents). A real restore test into a scratch database is offered as an explicit action, because only a restore proves a backup.

## Restore safety rules — Planned (Phase 6)

1. Restore targets a new or explicitly chosen database; the default is never the source database.
2. Restoring over an existing database requires typing its name, and for Production-labelled connections a statement that a recovery path exists (a fresh backup taken first is offered).
3. Tool and server versions are checked before starting.
4. Progress and the tool's own output are shown. On failure the UI states what was applied, because a logical restore is not atomic unless run in a single transaction.
5. AI agents cannot start a restore without the same approval as any other mutation ([AI_INTEGRATION.md](AI_INTEGRATION.md)).
6. A restore from a file PuriCat did not create is treated as running an untrusted SQL script and is labelled as such.

## Testing plan — Planned (Phase 6)

- Unit tests: schedule calculation, missed-run handling, retention selection.
- Integration tests: dump and restore round trips against disposable MySQL and PostgreSQL databases, comparing schema and row checksums.
- The scheduler itself needs a manual test procedure on a real login session (sleep, wake, locked keychain).

None of these tests exist yet.
