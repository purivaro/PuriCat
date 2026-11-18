# 0018 — SQLite files as a third engine

## Status

Accepted. Implemented: a connection can be a SQLite database file, with browsing, filtering, editing, Smart Paste, the SQL console and the AI's tools. Verification is recorded in [TESTING.md](../../TESTING.md); what was not tested is listed there and in [DATABASE_COMPATIBILITY.md](../../DATABASE_COMPATIBILITY.md), "SQLite".

Proposed by a user in [issue #10](https://github.com/purivaro/PuriCat/issues/10).

## Context

PuriCat connected to MySQL and PostgreSQL servers only. SQLite files are everywhere a developer looks (mobile apps, desktop apps, Django and Rails projects, browser profiles), and opening one needs no server, account or network.

Two things made it cheap to consider and one made it not trivial:

- `sqlx` was already built with its `sqlite` feature, for PuriCat's own settings store ([ADR 0005](0005-local-store-and-secrets.md)). The SQLite library (3.46.0, compiled in) is already part of the app. No new dependency.
- `Session` is an enum with one module of SQL per engine ([ADR 0003](0003-database-drivers.md)), so a third engine is a new module and a list of `match` arms.
- PuriCat's promises were written for servers. Three of them have no server to lean on in SQLite:
  1. **"The server rejects a value the column cannot hold."** A SQLite column's declared type is a suggestion (*type affinity*). Unless the table is `STRICT`, `abc` goes into an `INTEGER` column without a word.
  2. **"A read-only session is enforced by the server."** There is no server, and no read-only transaction.
  3. **"The AI's query runs in a read-only transaction, and the server confirms it is one statement."** SQLite has neither: `sqlx` prepares a whole script.

## Decision

### 1. A connection profile is a file path

`Engine::Sqlite`, and one new profile field, `file`: the absolute path of an existing file. Host, port, user name, default database and TLS do not apply and are cleared when the profile is saved. There is no password, so the keychain is not asked.

- **Existing files only.** The file is opened without `SQLITE_OPEN_CREATE`: a mistyped path is an error, not a new empty database. Creating a database is not part of this.
- **An absolute path.** A relative one would depend on where the app was started.
- The path is typed or pasted, or chosen in the native Open dialog. The dialog has no filter by extension: SQLite files are called anything (`.db`, `.sqlite`, `.storedata`, no extension). Connecting is what tells whether a file is a database: the schema is read, and a file that is something else — or an encrypted database — is refused in plain words.
- The file holds one database, `main`. Attached and temporary databases are not browsed.

### 2. Read-only means the handle cannot write, and a new file connection starts that way

"Read-only" opens the file with `SQLITE_OPEN_READONLY`. This is stronger than the server engines' session setting, which a statement can switch off ([SECURITY.md](../../SECURITY.md), Read-only sessions): nothing sent on that handle can write.

In the connection form, choosing SQLite on a **new** connection turns "Read-only" on. The files people open are often another program's live data; looking should be the default and editing a decision. The switch is one click, and an existing connection is never changed.

### 3. Nothing of the file's own settings is changed

No journal mode, page size or synchronous setting is sent. Two per-connection settings are, neither of which is stored in the file:

- `foreign_keys = ON` (the driver's default; SQLite's own is off). Writes made through PuriCat respect the foreign keys the schema declares.
- a busy timeout equal to the connection timeout: a lock held by another program is waited for, then reported as "database is locked".

A save takes the write lock at once (`BEGIN IMMEDIATE`), so the wait happens before the first statement, not half-way.

### 4. Values: text in, text out, as on the other engines — with the checks SQLite does not make

Reading follows [ADR 0004](0004-server-side-text-rendering.md): every value is rendered by SQLite (`CAST(col AS TEXT)`, `hex()` for a column declared `BLOB`). 64-bit integers arrive whole. A `REAL` is shown as SQLite converts it, with 15 significant digits.

Writing sends text; the column's affinity turns it into a number where it is one. A parameter compared with a column takes that column's affinity, so `id = ?` with the text `42` meets an `INTEGER` column as the integer 42, exactly, and uses its index. No cast is written.

A column's **kind** (what it is meant to hold) is read from its declared type: affinity first, by SQLite's own rule; then, within NUMERIC affinity, the names that plainly mean a number (`NUMERIC`, `DECIMAL`, …), a boolean, a date or time, or JSON. Anything else (`UUID`, `STRING`, …) is "other" and compared as the text shown.

What PuriCat adds because SQLite would not object:

- **Text that is not a number is refused for a column that holds numbers**, in the write plan, in Smart Paste and in the filter. The message says SQLite would have stored it and points to the SQL console for whoever means to.
- **A column without a declared type is shown but not edited** (also `ANY` in a `STRICT` table). It has no affinity: a number typed in the grid would be stored as text, silently changing the value's type. As a key, such a column is matched on the text shown.

`INTEGER PRIMARY KEY` in an ordinary table is the row id and is treated like an auto-increment column: SQLite assigns it, the grid does not write it, and a blank key in Smart Paste means "new row". In a `WITHOUT ROWID` table it is an ordinary column.

Optimistic concurrency ([ADR 0013](0013-staged-edits-and-optimistic-saves.md)) is unchanged: `CAST(col AS TEXT) IS ?` is the NULL-safe, byte-for-byte comparison with what the user saw.

### 5. The SQL console: an in-process connection, stopped by a flag

A query tab gets a connection of its own, opened directly (not taken from the pool). SQLite runs inside the app, so there is no second connection to send a "cancel" from: the tab's connection is given a progress handler that looks at a flag every 1,000 steps of the virtual machine and interrupts the statement when it is set. Stop, and the cut at the row cap, set that flag.

What differs from the servers, and is handled:

- **SQLite ends a transaction by itself when a write in it is interrupted.** After a statement that did not succeed inside the user's transaction, the console asks whether one is still open (a second `BEGIN` is refused inside a transaction) and, if not, says "SQLite rolled the open transaction back" instead of showing a transaction that is gone. Under the impact guard, a ROLLBACK that finds nothing to roll back is the outcome asked for, not an error.
- **A trigger's body has semicolons.** A `CREATE TRIGGER` statement runs on to the `END` that stands alone between two semicolons, the rule of SQLite's own `sqlite3_complete()`.
- The scanner reads SQLite's quoting: `"…"`, `` `…` `` and `[…]` around names, no backslash escapes, block comments that do not nest, `$` and `#` as ordinary characters.
- DDL is transactional and nothing commits implicitly. `PRAGMA`, `VACUUM`, `ATTACH` and the like are "other": run as typed, never under the guard, never accepted by the reading tool.

### 6. The AI's reading tool gets a handle that cannot write

`Session::open_reader` opens the file with `SQLITE_OPEN_READONLY` for the tool, whatever the session itself may do. On top of that, as first filters:

- the text must be one reading statement (the same check as on the servers), and **may hold no semicolon but a last one** — not even inside a string. SQLite needs a semicolon between two statements, so this does not depend on the scanner reading the text the way SQLite does;
- `PRAGMA query_only` is on while the statement runs.

`ATTACH` (which could read another file) is not a reading statement and cannot follow one.

## Alternatives considered

- **`rusqlite` instead of `sqlx`.** Direct access to `sqlite3_interrupt`, the authorizer and limits. Rejected for now: a second driver and a second copy of SQLite's bindings, for what the progress handler and the read-only open already give. An authorizer (to deny `ATTACH` outright) would be the reason to revisit.
- **Store the path in `database` or `host`.** No new field, but every place that shows a host or picks a default database would show a path. A field of its own keeps the two meanings apart; the profile is JSON, so nothing migrates.
- **Read-write by default**, like the servers. Rejected: see 2.
- **Leave SQLite's typing alone** and store whatever is typed. Rejected: the app's promise is that a value the column cannot hold is an error. The SQL console remains for anyone who wants SQLite's own behaviour.
- **Edit untyped columns, guessing the type** (a number if it looks like one). Rejected: a guess changes data without saying so.
- **Expose the row id as a key for tables without a primary key.** Would make every ordinary table editable. Left out: row ids can be reused and change on `VACUUM`, and the write rules assume a declared key. Possible later, with those caveats shown.
- **Cancel by dropping the query.** The driver's worker would run a long single step to its end. The progress handler stops inside the step.
- **A file filter in the Open dialog.** Hides the many SQLite files without one of the usual extensions.

## Consequences

- No server, account or network is needed to try PuriCat; the SQLite tests need none either and always run.
- The promises hold on SQLite by different means, and the documents say which: the handle for read-only, PuriCat's own check for numbers, the one-semicolon rule and the read-only handle for the AI.
- Limits that follow from SQLite, stated rather than hidden: no exact decimal type (a `DECIMAL` column stores a floating-point number), dates are text, a `REAL` is shown with 15 digits, there is no estimated row count, and a declared length (`VARCHAR(20)`) is not enforced.
- Not built: creating a database, encrypted databases (SQLCipher), attached databases, tables without a primary key as editable, the JSONB binary format (shown as raw bytes read as text), virtual-table specifics.
- A file used by a running program is read and written under SQLite's own locking. PuriCat adds no protection beyond waiting for locks; the read-only default is the protection.

## Licensing

No new dependency. SQLite is in the public domain; `sqlx` is MIT or Apache-2.0 ([LICENSES.md](../../LICENSES.md)).
