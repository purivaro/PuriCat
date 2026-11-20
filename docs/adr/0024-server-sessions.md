# 0024 — Server sessions: the server's own list, and a statement built from a number

## Status

Accepted. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-20 (server sessions)".

Verified on MySQL 9.3.0 and PostgreSQL 16.13 only (live tests on both; through the window on MySQL). MariaDB, MySQL before 8.0.22 (the fallback table), an account without the `PROCESS` privilege and a PostgreSQL account that is not a superuser have not been tried.

## Context

When a statement runs long on a server, the question is which one, from whom, and how to stop it. Until now PuriCat could stop only the statements of its own query tabs. A database tool the owner uses has a server monitor with the list of sessions and a way to end one (survey of 2026-11-20, kept locally). The owner asked for the things that tool has and PuriCat lacks; this is the first of them.

Constraints:

- Ending another client's session is an action against someone else's work. It must be visible before it happens, and refused on a read-only connection like any other change.
- What the server says about its sessions is the source; PuriCat keeps nothing.
- The statement shown must be the one that runs.

## Decision

1. **"Server sessions…"** in the menu of a connected MySQL or PostgreSQL connection opens a dialog with the server's sessions: id, user, host (and PostgreSQL's `application_name`), database, command or state, time, and the statement.
2. **The list is one query on the pooled session.** MySQL: `performance_schema.processlist` (8.0.22 and later), falling back to `information_schema.PROCESSLIST` (MariaDB, older servers). PostgreSQL: `pg_stat_activity`, client backends only. The session that reads the list marks itself (`ID = CONNECTION_ID()`, `pid = pg_backend_pid()`). A statement is read up to 4,000 characters.
3. **Two actions for one session**: stop its query (`KILL QUERY n`, `pg_cancel_backend(n)`) or end it (`KILL CONNECTION n`, `pg_terminate_backend(n)`). The statement is built in the core from a number (`monitor::end_sql`); the IPC takes a `u64`, so no text from the web view reaches it. A separate command returns the statement for the confirmation, from the same function.
4. **A confirmation in the dialog**, with the statement, what happens to the session and its transaction, and a warning on a Production-labelled connection. The read-only setting of a connection refuses both actions in the backend. The session reading the list has no actions.
5. **Idle sessions are hidden by default** (MySQL `Sleep`, PostgreSQL `idle`), with a switch; the list is read again every 5 seconds by default (2, 5, 10 seconds, or by hand), and not while a confirmation is open.
6. PostgreSQL's `pg_cancel_backend` and `pg_terminate_backend` answer `false` instead of failing when the session is gone or the account may not signal it; that answer is an error with that sentence.

## Alternatives considered

| Option | Why not |
|---|---|
| A tab instead of a dialog | The list is looked at and closed; a tab would stay among the table and query tabs. A dialog can become a tab later |
| `SHOW FULL PROCESSLIST` | Its columns are typed per server (unsigned ids, nullable integers); the tables give text with `CAST` and the own-session flag in the same query |
| Building the statement in the web view | The statement shown and the statement run would be two pieces of code |
| End sessions without a confirmation | Another client loses its work and its open transaction |
| Server variables and status counters in the same dialog | More than the first slice needs; noted as a follow-up |

## Consequences

- A long or stuck statement on a server can be found and stopped from PuriCat, and a forgotten session closed.
- Three new IPC commands (`list_processes`, `plan_end_process`, `end_process`). No new dependency, capability or CSP change.

Limitations:

- **What is listed depends on the account.** MySQL without `PROCESS` lists only the account's own sessions; PostgreSQL hides other accounts' statements (shown as `<insufficient privilege>`) unless the account is a superuser or in `pg_read_all_stats`, and signals them only with `pg_signal_backend`. The dialog says so.
- **PuriCat's own pooled connections are listed** like any other client's, unmarked except the one that read the list. Ending one makes PuriCat reconnect on its next use.
- **MySQL's `TIME` is the time in the current state; PostgreSQL's column is the time since the current or last statement started.**
- **Nothing is recorded** of what was stopped or ended; the toast says which statement ran.
- Not built: server variables and status counters, sorting by column, several sessions at once, an automatic warning for long-running statements.
