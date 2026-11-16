# 0005 — SQLite settings store and OS credential store

## Status

Accepted. Implemented. Verified on macOS; the Windows path is compiled in but has never been run.

## Context

PuriCat stores connection profiles, UI settings and open tabs, and later history, backup configuration and an audit log. Passwords must not be stored in plain files, logs or anything the web view can read back.

## Decision

**Settings:** one SQLite file, `puricat.db`, in the OS app-data directory for the bundle identifier (`com.purivaro.puricat` since the first public release on 2026-11-16; before that the placeholder `app.puricat.desktop`).

- Tables: `connections` (id, JSON profile, timestamps) and `settings` (key, value; current keys `ui` and `workspace`).
- Migrations are an append-only list in code, tracked with `PRAGMA user_version`. A database from a newer build is refused rather than downgraded.
- File mode is set to 0600 on Unix after opening.
- Accessed through sqlx, which the adapters already use.

**Secrets:** OS credential store through `keyring` 3 (`apple-native`, `windows-native`).

- Service `PuriCat`, key `connection/<id>/password`.
- IPC never returns a stored secret. The UI learns only `hasPassword`, and updates are `keep`, `set` or `clear`.
- Duplicating a connection copies the password inside the backend; deleting a connection deletes its password.
- Debug builds accept `PURICAT_SECRET_STORE=memory` to use a process-local store. It is compiled out of release builds.

## Alternatives considered

| Option | Why not |
|---|---|
| JSON or TOML config file | No transactions or migrations; grows badly once history and audit data arrive |
| Passwords encrypted in SQLite with an app key | The key has to live somewhere; this reinvents the credential store less safely |
| Tauri Stronghold plugin | Needs its own vault password and adds a plugin to the web view's capability surface |
| `sqlx::migrate!` with SQL files | Fine alternative; the inline list is smaller while there is one migration |

## Consequences

- Host names, user names and CA paths are stored unencrypted in `puricat.db`, protected by file permissions only.
- Unsigned development builds change code signature on each rebuild, so macOS may prompt for Keychain access again; hence the debug-only memory store.
- Changing the service name or the bundle identifier orphans saved passwords or settings. Both must be fixed before the first public build.
- Unattended backups (Phase 6) will read the same Keychain items from a helper binary, which requires an unlocked login keychain and a stable code signature.
- Profiles are opaque JSON, so they cannot be queried by column; acceptable for a short list.

## Licensing

keyring MIT/Apache-2.0; sqlx MIT/Apache-2.0; SQLite public domain.
