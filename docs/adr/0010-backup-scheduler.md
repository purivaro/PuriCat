# 0010 — Backups: vendor dump tools and a per-user LaunchAgent helper

## Status

Proposed (Phase 6). Nothing is implemented.

## Context

The brief requires manual and scheduled backups, retention, optional encryption, restore, and honest reporting of when unattended runs cannot happen. A desktop app cannot run a schedule while it is closed.

## Decision

**Manual backups** run inside the app by invoking the vendor tools found on the machine — `mysqldump` for MySQL, `pg_dump` / `pg_restore` for PostgreSQL — after checking the tool's version against the server's. Fixed argument vectors, no shell. Passwords are passed through the tool's supported non-argument channel, never on the command line.

**Scheduled backups** need a process that outlives the app:

- macOS: a per-user LaunchAgent (launchd) that starts a small `puricat-backup` helper binary built on `puricat-core`. The helper reads job definitions from the settings store and credentials from the Keychain.
- Windows (later): Task Scheduler with the same helper.
- No privileged services. Installing or removing the agent requires explicit consent in the UI.

**SQL Server** is a separate, clearly labelled strategy: `BACKUP DATABASE` writes to a path **on the server**, not on the client. PuriCat will not present a local destination for it.

## Limitations to state in the UI

- Nothing runs while the Mac is asleep or powered off. launchd runs a missed calendar job once after wake, not once per missed interval.
- The login keychain must be unlocked, so jobs cannot run before the user logs in.
- The vendor tools must be installed and compatible with the server version.
- A database reachable only over VPN or SSH is skipped when that path is down; the run is recorded as failed.

## Alternatives considered

| Option | Why not |
|---|---|
| In-app timer only | Silently skips every run while the app is closed |
| Root LaunchDaemon | Privileged install, and it cannot read the user's login keychain |
| Native dump implemented in PuriCat | Re-implements mature tools; high risk of subtly incomplete dumps |
| Bundle the vendor tools | Licensing (MySQL client tools are GPL) and version-matching problems |

## Consequences

- Backups depend on tools PuriCat does not ship; detection and clear guidance are required.
- The helper needs a stable code signature to read Keychain items without prompting, which ties scheduled backups to the signing work of Phase 7.
- Schedule calculation, retention and the "missed run" logic need unit tests; restore needs an isolated target.

## Licensing

`mysqldump`, `pg_dump` and `pg_restore` are invoked as separate programs installed by the user and are not redistributed. No new library is chosen by this record; an encryption library will be selected in Phase 6.
