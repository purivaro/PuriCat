<p align="center">
  <a href="README.md"><b>🇹🇭 อ่านหน้านี้เป็นภาษาไทย</b></a> &nbsp;·&nbsp; English
</p>

<p align="center">
  <img src="branding/app-icon.png" width="112" alt="PuriCat icon">
</p>

<h1 align="center">PuriCat</h1>

<p align="center">
  <b>A free database client that treats your data carefully.</b><br>
  Browse and edit MySQL, PostgreSQL and SQLite like a spreadsheet, see the exact SQL before anything is saved,<br>
  and let an AI look things up — while every change still waits for your approval.
</p>

<p align="center">
  <a href="https://github.com/purivaro/PuriCat/releases/latest/download/PuriCat-mac.dmg"><b>Download for macOS</b></a>
  &nbsp;·&nbsp; <a href="STATUS.md">What works today</a>
  &nbsp;·&nbsp; <a href="LICENSE">MIT licence</a>
</p>

![PuriCat: a PostgreSQL table next to the AI agent, which has read the data and proposed a change that waits for approval](docs/images/hero.png)

> **Early development (v0.4).** PuriCat connects to **MySQL** and **PostgreSQL** servers and opens **SQLite** files, and has been tested on **macOS on Apple Silicon** only. There is no Windows or Linux build yet. Until you trust it, use it on data you can afford to lose or with a read-only account. [STATUS.md](STATUS.md) lists every feature and how it was tested.

## Download

| System | File |
|---|---|
| **macOS** 12 or newer | [**PuriCat-mac.dmg**](https://github.com/purivaro/PuriCat/releases/latest/download/PuriCat-mac.dmg) — a universal app. Tested on Apple Silicon; the Intel half has not been run |
| Windows, Linux | Not built yet |

All versions and checksums are on the [Releases](https://github.com/purivaro/PuriCat/releases) page.

### Install on a Mac

1. Open `PuriCat-mac.dmg` and drag **PuriCat** into Applications.
2. Open PuriCat. The app is **signed with a Developer ID and notarized by Apple**, so it opens like any other downloaded app: macOS asks once whether to open an app downloaded from the internet.
3. Passwords you save are kept in the **macOS Keychain**, never in a file.

**Updates.** From version 0.3.1, PuriCat looks for a newer version when it starts and shows it in the status bar; you choose when to download it and when to restart. The check is one request to github.com that says nothing about you or your databases, the download is installed only if it carries PuriCat's release signature, and the check can be switched off in the same dialog. You can also ask at any time: click the version number in the status bar, or choose **Check for Updates…** in the PuriCat menu. Version 0.3.0 and older are updated by downloading the file above again.

## What it does

### Edit tables like a spreadsheet, and read the SQL before it runs

Type in cells, add and delete rows, paste a block copied from Excel or Google Sheets, undo and redo. A long text or a JSON document opens whole in its own dialog (double-click the cell), where it can be read, copied and edited; JSON is laid out one member per line, with every number exactly as stored. Find rows with a **filter**: conditions on any column (⌘F), of which a row must meet all, or any one. A condition can compare with one value, a range (*is between*) or a list (*is one of*) — a column copied from a spreadsheet can be pasted as the list — and can be switched off without deleting it. Or right-click a cell and choose *Filter by this value* or *Hide rows with this value*. The footer shows the server's estimate of the rows; the exact number, of the table or of the rows that match, is counted when you ask for it. Nothing touches the database until you press Save. For deletes, larger changes and Production connections, a review shows **every statement with its values** first. Everything is saved in **one transaction**: if a row was changed by someone else in the meantime, nothing is saved.

![The review dialog: each DELETE, UPDATE and INSERT with its values, saved in one transaction](docs/images/edit-review.png)

### Smart Paste: update a table from a spreadsheet or a file

Copy rows from a spreadsheet — or open a **CSV or Excel file** — and choose INSERT, UPDATE or UPSERT. PuriCat matches the rows on a unique key and shows what will happen to each one — insert, update, already identical, or a problem — before anything runs. Rows with a problem can be left out and copied back to the spreadsheet to be fixed.

![Smart Paste: columns mapped to the table, and a preview of which rows will be inserted or updated](docs/images/smart-paste.png)

### A SQL console with guard rails

Query tabs with syntax highlighting, completion, history and export to CSV, JSON or XLSX. Open one with the **+** button after the last tab, as in a browser, or with ⌘T: it starts on the connection and the database of the tab in front. Run the whole script, the selection, or only the statement the cursor is in (⇧⌘↵). Save a script under a name (⌘S) and open it again later from the connection's saved scripts. Each tab has its own connection. A statement that would change more rows than the connection's limit (10 by default) is **rolled back** and you are asked again; destructive statements are confirmed first; an open transaction is always visible.

![A query tab with two statements and one result tab for each](docs/images/sql-console.png)

### An AI agent that asks before it changes anything

PuriCat talks to the **Claude Code** or **Codex** command-line tool that you have installed and signed in to. It never asks for an API key or a password.

- **Off by default.** Out of the box the AI only sees table and column names, and only writes text.
- **Structure**, or **Structure and rows**: you choose whether it may look at the catalogue, or also run read-only queries (one statement at a time, in a read-only transaction, at most 200 rows).
- **It cannot write.** A change is a *proposal*: you read the SQL and see which connection and database it will run on, and PuriCat runs it only after you approve it. The row limit of the SQL console applies here too.
- **Everything is visible.** Each step opens to show exactly what the AI was given, and an activity log on your computer records every read, proposal and decision.

![The approval dialog: the proposed SQL, the connection it runs on and the guards that apply](docs/images/ai-review.png)

![The activity log: the queries the AI ran, what it proposed and what was approved](docs/images/ai-activity-log.png)

To use the agent, install [Claude Code](https://claude.com/claude-code) and sign in (`claude auth login`); PuriCat finds it by itself. Codex is supported the same way, but its tool calls have not been verified yet. What you send to the AI goes to that provider under your own account: [AI_INTEGRATION.md](AI_INTEGRATION.md) says exactly what is sent.

### A new database, with its SQL in view

Right-click a MySQL or PostgreSQL connection and choose *New database…*: give it a name, pick the character set and collation (MySQL) or the owner, template and encoding (PostgreSQL) from what that server offers, and read the exact `CREATE DATABASE` statement before it runs.

The same right-click menus look after a database later on. *Edit database…* changes the default character set and collation (MySQL) or the name and the owner (PostgreSQL). *Drop database…* tells you how many tables and views are inside and stays locked until you type the database's name. *Open SQL file…* puts a `.sql` file into a new query tab without running it, and a MySQL connection has *Flush* (Privileges, Tables, Logs, Status). In each case you read the exact statement before it runs.

A table or a view has a menu of its own. *Definition…* shows the statements that would create it as it is now, to copy or to open in a query tab: MySQL's and SQLite's own text, and on PostgreSQL, which has no such statement, text PuriCat puts together from the catalogue and labels as such (`pg_dump` remains the authority). *Rename…*, *Empty table…* and *Drop table…* / *Drop view…* each show the one statement they will run; emptying and dropping stay locked until you type the name.

A view is made and changed in a form: *New view…* on a database (on PostgreSQL, on a schema) and *Edit view…* on a view. You write only the query — a `SELECT` — in the same editor a query tab has; PuriCat writes `CREATE VIEW … AS` around it and shows the whole statement before it runs. *Preview rows* runs the query read-only and shows its first 100 rows, so you can see that it is the right query before the view exists. The query must be a single statement that reads; anything else is refused. When you edit a view on MySQL, the form shows the query as MySQL keeps it, which is the server's own rewriting and not the text you typed. Creating a table or changing its columns is still done as SQL in a query tab.

A table is made in a form too: *New table…* on a database (on PostgreSQL, on a schema). Add its columns — name, type from a list, length or precision, NULL, a self-numbering id, a default, a comment — and tick the primary key. The `CREATE TABLE` statement is shown as you go and is what runs; the new table opens in a tab. *Change table…* in a table's menu renames, drops and adds columns and drops or makes indexes, all in one go; dropping a column asks you to type the table's name. *Change* on a column sets a new type, NULL, default or comment (what you leave alone stays as it is), and the same form lists the table's foreign keys, drops them, and makes new ones to another table's columns with their delete and update actions.

A MySQL or PostgreSQL connection's menu also has *Users and privileges…*: the server's accounts, and what the one you select may do — MySQL's own `SHOW GRANTS`, or a PostgreSQL role's attributes and its access to each database. *New user…* starts from what the account is for: an application user for one database, a read-only user for one database, or no privileges yet. It can generate the password, and shows the statements before they run, with the password masked unless you ask to see it; afterwards the password is shown once more to be copied, and PuriCat does not keep it. *Change password…* and *Drop user…* (locked until you type the account's name) are there too. The server's own accounts, the account you are connected as and a PostgreSQL superuser are left alone. Privileges cannot yet be granted one by one.

*Server sessions…* in the same menu lists who is connected to the server and what each session runs, for how long. A statement that runs too long can be stopped, or a forgotten session ended, after the statement that does it (`KILL QUERY`, `pg_cancel_backend`, …) is shown. A read-only connection only lists.

*Find in database…* in a database's menu (or a PostgreSQL schema's) looks for a value in every table — contains, is exactly, or starts with — reading each table once on the server and showing, as it goes, the tables and columns that hold it and in how many rows. A result opens the table showing just those rows; *Stop* ends the search at once. It can also find the tables and columns whose *name* contains some text. Nothing is changed.

### Backups, by hand and on a schedule, and a restore into a new database

Right-click a database → **Back up…** makes a backup with the database's own tool (`mysqldump` or `pg_dump`, which must be installed) into **a folder you choose**, and shows which tool it found and whether it fits the server. A file is kept only after it was checked to be complete; a backup that fails or is stopped leaves nothing behind. In the same dialog you can have the backup made **every day or on chosen days**, also while PuriCat is closed, and say how many to keep. Every backup in the dialog's history has **Restore…**, which loads it into a **new database** on the same connection, so that you can look at it beside the original: nothing that exists is replaced. Replacing an existing database is done with the same tools, outside PuriCat: [BACKUP_RESTORE.md](BACKUP_RESTORE.md) has the commands and the limits. Backup files are not encrypted.

### SQLite files, opened carefully

Open a `.db`, `.sqlite` or `.sqlite3` file — an app's data, a project's database — without a server or an account. A new SQLite connection is **read-only**: the file is opened so that it cannot be written, until you decide to edit. PuriCat also makes the checks SQLite does not: text that is not a number is refused for a column of numbers, instead of being stored without a word. [What works for SQLite, and its limits](DATABASE_COMPATIBILITY.md#sqlite--a-database-file).

### English and Thai, light and dark

![The same screen in Thai with the dark theme](docs/images/thai-dark.png)

## Built to be safe

- **Passwords stay in the macOS Keychain.** They are never written to PuriCat's settings or logs, and never sent to the AI.
- **No telemetry.** PuriCat's own code opens no connection except to your databases. The AI panel starts the CLI you installed, and that tool talks to its provider.
- **Environment labels.** Mark a connection as Development, Staging or Production; Production adds confirmations. A connection can also be opened read-only, enforced by the server.
- **Exact values.** Numbers, dates and JSON are shown as the server renders them: no rounding of a `DECIMAL`, no loss beyond 2^53.
- **The AI never holds your credentials.** Its tools run inside PuriCat on the connection you already opened, for the length of one answer.

The threat model, and the limits of each of these, are in [SECURITY.md](SECURITY.md).

## Not there yet

SQL Server, SSH tunnels, creating and altering tables, imports of more than 10,000 rows, restoring a backup over an existing database, encrypted backups, and Windows or Linux builds are planned but not built. The [roadmap](ROADMAP.md) has the order.

## Build from source

You need Rust 1.85 or newer, Node.js 22 or newer and pnpm 10, on macOS 12 or newer with the Xcode Command Line Tools.

```sh
pnpm install
pnpm tauri dev      # run the app (the first build takes several minutes)
pnpm tauri build    # make PuriCat.app and a .dmg
pnpm check          # type-check, unit tests, cargo fmt, clippy, cargo test
```

Tests against real MySQL and PostgreSQL servers and real SQLite files, and the end-to-end test that drives the real window, are described in [TESTING.md](TESTING.md).

A release is made by pushing a tag such as `v0.2.0`: [`.github/workflows/release.yml`](.github/workflows/release.yml) builds the universal macOS app and attaches it to the release. It also publishes the package and the manifest that installed apps update from, signed with the key in the `TAURI_SIGNING_PRIVATE_KEY` secret ([ADR 0020](docs/adr/0020-in-app-updates.md)); a tagged build fails without it. If the Apple signing secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) are set on the repository, the app is signed and notarized; without them (in a fork, for example) it is only ad-hoc signed. [`scripts/release-mac.sh`](scripts/release-mac.sh) makes the same download by hand on a Mac that has the Developer ID in its keychain; v0.1.0 was published that way, before the secrets were set.

```
crates/core/   puricat-core: database adapters, the write planner, Smart Paste, the file reader, the SQL console,
               the AI chat runner and the tools it serves (no Tauri dependency)
src-tauri/     the Tauri app: IPC commands, app state, bundle configuration
src/           the React frontend
e2e/           the end-to-end test: runner, script, SQL fixtures, stand-in AI CLIs
docs/          decision records (docs/adr) and the screenshots on this page
```

## Documentation

| Document | Contents |
|---|---|
| [STATUS.md](STATUS.md) | Every feature, and how it is known to work |
| [ROADMAP.md](ROADMAP.md) | What comes next |
| [SECURITY.md](SECURITY.md) | Threat model, credentials, TLS, the SQL console, the AI |
| [AI_INTEGRATION.md](AI_INTEGRATION.md) | How the AI CLIs are run, what is sent, the database tools |
| [DATABASE_COMPATIBILITY.md](DATABASE_COMPATIBILITY.md) | Engines, versions and features |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Components, data flow, IPC surface |
| [TESTING.md](TESTING.md) | Test layers, commands, results and gaps |
| [REQUIREMENTS.md](REQUIREMENTS.md) | Requirements and acceptance criteria |
| [docs/adr/](docs/adr/README.md) | Architecture decision records |
| [LICENSES.md](LICENSES.md) | Licences of the libraries PuriCat uses |

## Licence

PuriCat is free and open source under the [MIT licence](LICENSE). Use it, share it, change it.

The screenshots on this page were taken from a debug build running against a demo database. The AI answers in them are real answers from Claude Code.
