# 0023 — Users and privileges: accounts from templates, the statements shown with the password masked

## Status

Accepted. Implemented on 2026-11-20 for MySQL and PostgreSQL connections: "Users and privileges…" in the menu of a connected connection lists the server's accounts, shows what one of them may do, and has three forms — a new user from a template, another password, a drop. It follows the rule of [ADR 0019](0019-schema-changes-through-forms.md) (one builder for the preview and for the run, a `plan_…` command and a command that runs) and adds what a password and an account name need. Verification is recorded in [TESTING.md](../../TESTING.md), "Results — 2026-11-20 (Users and privileges)".

**Verified in the backend on MySQL 9.3.0 and PostgreSQL 16.13; through the UI on MySQL only.** The live tests create one account per template on each server and then use it; the end-to-end test creates a read-only user on MySQL, reads its grants and drops it. Not verified: PostgreSQL through the UI; a password change and PostgreSQL's refusal of a drop through the UI; a read-only or Production-labelled connection through the UI; `partial_revokes` on and `standard_conforming_strings` off (unit tests only); MariaDB and other server versions; an account that may not read `mysql.user`.

## Context

Making an account for an application is routine work on a server behind a web application, and until now PuriCat offered it only as SQL typed in a query tab (or proposed by the AI and approved). There the password is part of the tab's text, which `puricat.db` keeps as typed while the tab is open, and of the SQL history, where a heuristic masks it ([SECURITY.md](../../SECURITY.md), "SQL console"). Database clients usually offer an editor of single privileges instead: global privileges, then object privileges in a tree, each a checkbox. Most accounts are one of a few kinds — an application's account that may do everything in one database and nothing outside it, an account that only reads, an account whose privileges come later — and such an editor turns each of them into many decisions, with two classic mistakes left to the user: on MySQL a grant on a database whose name holds `_` also covers every database that name matches as a pattern; on PostgreSQL an account that can read today's tables cannot read tomorrow's.

Constraints:

- The UI must not build SQL ([ADR 0002](0002-core-crate-and-thin-ipc.md)), and the statement shown must be the statement run (ADR 0019).
- A password can only be written into these statements as a string literal. A MySQL account is named by two string literals, `'user'@'host'`, not by identifiers, so `db::ident` does not apply to it either.
- How a string literal is read depends on the session: on MySQL a backslash is an escape character unless `sql_mode` has `NO_BACKSLASH_ESCAPES`; on PostgreSQL it is an ordinary character only while `standard_conforming_strings` is on.
- A password must not reach anything PuriCat keeps or shows: an error message, a log, the SQL history, the AI activity log — or the screen, unless the user asks.
- MySQL commits an account statement at once: several of them cannot be one transaction there.
- Some accounts keep the server, or PuriCat's own connection, working.

## Decision

### 1. Templates, not a privilege editor

The form asks what the account is for, and offers three answers:

| Template | MySQL | PostgreSQL |
|---|---|---|
| Application user, for one database | ``GRANT ALL PRIVILEGES ON `db`.*`` | `GRANT CONNECT ON DATABASE`; then, inside that database, for each schema that is not the server's own: `GRANT USAGE, CREATE ON SCHEMA`, `GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA` and `… ON ALL SEQUENCES IN SCHEMA`, `ALTER DEFAULT PRIVILEGES IN SCHEMA … GRANT ALL PRIVILEGES ON TABLES` and `… ON SEQUENCES` |
| Read-only user, for one database | ``GRANT SELECT, SHOW VIEW ON `db`.*`` | `GRANT CONNECT ON DATABASE`; then per schema `GRANT USAGE ON SCHEMA`, `GRANT SELECT ON ALL TABLES IN SCHEMA` and `ALTER DEFAULT PRIVILEGES IN SCHEMA … GRANT SELECT ON TABLES` |
| No privileges yet | `CREATE USER` alone | `CREATE ROLE … LOGIN` alone |

Every template starts with `CREATE USER 'u'@'h' IDENTIFIED BY …` or `CREATE ROLE "u" LOGIN PASSWORD …`. The form asks for a name, on MySQL a host (any, `%`; `localhost`; or another, typed), the database for the first two templates, and a password. What a template grants is not only described: it is on screen as the statements before they run.

On MySQL a grant can name a database as a whole. On PostgreSQL privileges belong to each object, so the template grants on the schemas, tables and sequences the database holds when the statements are built, and adds `ALTER DEFAULT PRIVILEGES` for what comes later. That covers only objects created later by the role that runs it — the connection's own — and the form says so, naming that role. It also says that nothing is taken away from `PUBLIC`, which on most servers may still connect to every database.

### 2. One builder; the statements shown are the statements run, with the password masked

`db::users` writes every statement, as `db::ddl` does for the schema forms. `plan_user`, `plan_password` and `plan_drop_user` return the text and run nothing; `create_user`, `change_password` and `drop_user` call the same functions again and run what they return. The web view sends choices and the password, never SQL.

- **The preview shows `'••••••••'` where the password literal goes**, unless the user turns "show" on; then it shows the literal as it will be written (quotes doubled, on MySQL backslashes too). The statement that runs is the masked text with the literal in place of the mask (unit test). The commands answer with the statements that ran, masked.
- **The password crosses IPC from the web view to the backend.** It comes back in one case only: inside a preview the user asked to see. "Generate" makes 24 letters and digits from `crypto.getRandomValues` in the web view, without bias, so nothing in it needs escaping in a statement, a shell or a `.env` file — and turns "show" on, because a password nobody chose has to be seen to be kept.
- **After a creation or a change the password is shown once more, to be copied.** PuriCat keeps it nowhere.
- **Nothing here goes to the SQL history or the AI activity log, and nothing in `db::users` logs.** A server's error message is scrubbed of the password before it is passed on: of the literal as it was written and of the password as typed — a password shorter than four characters only where it stands in its quotes, so that short words elsewhere in the message survive. `UserSpec`, which holds the password, has no `Debug`.

### 3. Each piece of text has one way into the statement

| What | How it is written | Refused |
|---|---|---|
| A MySQL user name or host | A string literal, `'` doubled | A backslash, a NUL; more than 32 characters (user) or 255 (host) |
| A MySQL password | A string literal, `'` doubled, and `\` doubled unless the session's `sql_mode` has `NO_BACKSLASH_ESCAPES` | Empty, a NUL, more than 256 characters |
| The database of a MySQL grant | A quoted identifier, with `_` and `%` escaped as `\_` and `\%`, which are wildcards there — unless `partial_revokes` is on, which makes them ordinary characters | A backslash in the name, when escaping |
| A PostgreSQL role, database or schema | A quoted identifier (`db::ident`) | A NUL; a new role's name over 63 bytes |
| A PostgreSQL password | A string literal, `'` doubled | Empty, a NUL, more than 256 characters; any password while the server reports `standard_conforming_strings` off |

- **Why a backslash is refused in a name.** No user name or host needs one, and without one a name is spelled the same in every `sql_mode`: its text never depends on a setting. A password may hold anything, so it is the one literal written for the mode — and the mode is read with `SELECT @@SESSION.sql_mode` on a connection taken out of the pool for the purpose, which then runs the statements and is closed. The mode the literal is written for is the mode it is read in.
- **That rests on the connection's character set being utf8mb4**, as PuriCat opens MySQL connections: no byte of a multibyte UTF-8 character is a quote or a backslash, which is not so in GBK or SJIS.
- **On PostgreSQL there is no second spelling.** With `standard_conforming_strings` on, doubling the quote is all there is; with it off a backslash would be an escape, and the forms refuse to run rather than write `E''` strings for one rare setting.
- **A password is never trimmed.** A new account's name and host are, and the host defaults to `%`. An account that exists is named exactly as the server lists it.
- **A name is not left for the server to shorten.** PostgreSQL would cut a longer role name to 63 bytes with only a notice, and create a role the statement did not name.
- `partial_revokes` is read with `SHOW VARIABLES LIKE 'partial_revokes'`; a server older than MySQL 8.0.16 answers with no row, which is read as off.

### 4. All of it or none

- **MySQL** runs the steps on that one connection. When the `GRANT` fails after `CREATE USER` succeeded, `DROP USER` runs on the same connection, and the error says that the account was dropped again — or that it could not be, why, and that it can be dropped from the list.
- **PostgreSQL** sends the role and its `CONNECT` as one simple query on the database the session is opened on, and the statements inside the target database as another, on a connection to that database. Each is one implicit transaction, which the server rolls back on a failure. When the second fails, `REVOKE ALL PRIVILEGES ON DATABASE … FROM …; DROP ROLE …` undoes the first (a role that holds a privilege cannot be dropped), and the error says so.
- **When the first step fails, nothing is undone.** So an account that already has the name is never dropped by an attempt to create it again (live tests on both servers: the second attempt is refused by the server, and the first account still works).

### 5. The accounts the forms leave alone

The backend refuses to plan or to run a password change or a drop for:

- **the server's own accounts**: on MySQL the names that begin with `mysql.` (`mysql.sys`, `mysql.session`, `mysql.infoschema`); on PostgreSQL the roles that begin with `pg_`, which the list leaves out;
- **the account the session is logged in as** (`CURRENT_USER()`, `current_user`): after a new password or a drop, PuriCat's saved connection could not log in again;
- **a PostgreSQL superuser.**

The list marks these and the two buttons are disabled with the reason, but the refusal does not depend on the buttons. A drop needs the account typed as it is written — `user@host` on MySQL — and `drop_user` compares the typed text again. On PostgreSQL the server refuses to drop a role that owns objects or still holds privileges; the form says to run `REASSIGN OWNED BY` and `DROP OWNED BY` for it in a query tab of each database first, and does not run them itself: `DROP OWNED BY` drops what the role owns, and works on one database at a time.

A connection saved as read-only is refused by `create_user`, `change_password` and `drop_user` before anything is sent, and the forms disable their buttons. A Production-labelled connection gets a warning in each form and danger-styled buttons; there is no typed confirmation beyond the drop's.

### 6. What an account may do is read, not reconstructed

MySQL: `SHOW GRANTS`, the server's own text, one statement per line, beside the authentication plugin, "locked" and "password expired" from `mysql.user`. PostgreSQL has no such statement, and PuriCat does not write one: the pane shows the role's attributes, the roles it is a member of and its members, and for each database that is not a template whether it owns it and has `CONNECT`, `CREATE` and `TEMP` (`has_database_privilege`). Privileges on tables and other objects inside a database are not read in this version, and the pane says so.

### 7. Not built

- A privilege editor (a tree or a grid of checkboxes), revoking single privileges, editing role membership.
- On MySQL: SSL requirements, resource limits, the authentication plugin, password expiry and locking. On PostgreSQL the attributes (superuser, `CREATEDB`, `CREATEROLE`, connection limit, `VALID UNTIL`) are shown, not set.
- An account without a password.
- Privileges on tables inside a PostgreSQL database, in the pane.
- SQLite: a file has no users; whoever can read the file can read the database.
- A record of what was created, changed or dropped.
- A PostgreSQL password sent as a SCRAM verifier (see "Consequences").

## Alternatives considered

| Option | Why not |
|---|---|
| An editor of single privileges, as most database clients have | Many decisions for the accounts people make most, with the wildcard and default-privilege mistakes left to the user. Not ruled out for later; it would use the same builder and preview |
| The statements typed in a query tab, as before | The password would be in the tab's text in `puricat.db` and in the history, behind a heuristic mask |
| The password as a bind parameter | The statements take a literal |
| Backslashes in names escaped for the mode, like the password's | A name's text would then depend on a setting read at run time; refusing a character no account name needs costs nothing |
| The mode read once per session, or on any pooled connection | `sql_mode` is a session variable; only the connection that runs the statement can say how it will read it. The preview is built with a pooled connection's mode: it shows the mask, or a literal that the run writes again for its own connection |
| `E''` strings when `standard_conforming_strings` is off | A second way to write a password, with tests of its own, for a setting that has been on by default since PostgreSQL 9.1. Refused instead, with the reason |
| On PostgreSQL, everything in one transaction on a connection to the target database | `CREATE ROLE` is transactional and a role belongs to the whole server wherever it is created, so no compensating statement would be needed. Not what was built: the role and its `CONNECT` run on the session's own database, the rest inside the target, and the undo of section 4 covers the gap |
| A SCRAM verifier instead of the password, as `psql`'s `\password` sends | Keeps the password out of the server's log (see "Consequences"). Needs PuriCat to compute the verifier; not built |
| Refusing every MySQL account with global privileges | MySQL has no superuser attribute like PostgreSQL's to read. The server's own accounts and the account in use are refused; for the rest, the typed name and the connected account's privileges decide |

## Consequences

- The accounts people make most are one form each, and what they may do is on screen as SQL before they exist.
- New: the module `db::users`, eight IPC commands (84 registered in all), `UsersDialog.tsx` with its helpers in `users.ts`. No migration, no new dependency, no new Tauri capability, no change to the Content-Security-Policy; "Copy" uses the clipboard permission the app already had.
- **The password passes through the web view**: typed or generated there, held in the form's memory until the dialog moves on, and on the system clipboard once copied, where PuriCat does not clear it.
- **Known gap: the PostgreSQL server log.** `CREATE ROLE … PASSWORD '…'` and `ALTER ROLE … PASSWORD '…'` carry the password in clear, and a server that logs statements (`log_statement` set to `ddl` or `all`) writes it into its log. By PostgreSQL's default `log_min_error_statement`, a statement that fails is logged with its text too (PostgreSQL's documentation; not tried). MySQL rewrites passwords out of the statements it logs. Sending a verifier computed by PuriCat, as `psql`'s `\password` does, would close the gap; it is not built. Other places where a server keeps statement texts were not looked at.
- **On PostgreSQL a template covers what exists when the user is made**, and later only what the connection's role creates. A schema created later, and tables another role creates, need grants of their own; the form says the second.
- **On PostgreSQL a user made from the application or read-only template holds privileges**, so "Drop user…" is refused by the server until `DROP OWNED BY` (and `REASSIGN OWNED BY` for what it owns) has been run in the database it was made for; the form says what to run. Verified for a role that owns a table; a role that only holds privileges was not tried.
- **On MySQL ``ALL PRIVILEGES ON `db`.*`` is everything at the level of that database**, `DROP` included, so an application user can drop its own database (MySQL's rules; not tried).
- **`PUBLIC` keeps what it has**: on most servers a new role may connect to every database (live test: a new role connects to a second scratch database and reads nothing there).
- **The typed name stops a slip, not a script in the web view**, which can send it twice — and can already run the same statements through `run_sql`.
- **Nothing else is protected by name on MySQL**: an account with the privilege can change or drop another administrator's account, `root` at another host included.
- **Some existing accounts cannot be handled here**: one whose name or host holds a backslash, and MySQL's anonymous account (an empty user name), cannot be read, changed or dropped from the dialog, because the builder will not name them (read from the code; not tried).
- **The screen that shows a new password stays until "Done"**: Escape and a click outside do nothing there, so a slip of a key does not throw away the one display of it (end-to-end check of Escape). A password that was not copied before "Done" has to be replaced with "Change password…". In a form, Escape returns to the list, as its Back button does, and what was typed is gone (read from the code).
- **Not recorded, no Stop.** Neither the SQL history nor any log of PuriCat's has what was created, changed or dropped, and the statements run outside a console.
- Not verified: PostgreSQL through the UI; a password change and PostgreSQL's drop refusal through the UI; the dialog in Thai and in the dark theme; the bundled app under the production CSP; read-only and Production-labelled connections through the UI; the commands' own read-only refusal and typed-name check, on their own; `partial_revokes` on and `standard_conforming_strings` off, which only unit tests see; MariaDB and other server versions; an account that may not read `mysql.user`.

## Licensing

No new dependency.
