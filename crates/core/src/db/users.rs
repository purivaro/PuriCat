//! Accounts of a server: who they are, what they may do, and three forms — a new user from
//! a template, another password, dropping one.
//!
//! As in `ddl`, the statements are built here and nowhere else, so the text a form shows is
//! the text that runs. What is different is what the text holds:
//!
//! - **A password.** It can only be written as a string literal. A preview shows [`MASK`] in
//!   its place unless the user asked to see it; the statement that runs is the same text
//!   with the real literal there. A password is never part of an error message or of a
//!   `Debug` output, and nothing here logs.
//! - **MySQL account names.** `'user'@'host'` is two string literals, not identifiers.
//!
//! String literals are the delicate part. PostgreSQL: the quote is doubled, which is all
//! there is to it while `standard_conforming_strings` is on — and the forms refuse to run
//! when the server says it is off. MySQL: the quote is doubled, and a backslash is an escape
//! character unless the session's `sql_mode` has `NO_BACKSLASH_ESCAPES`, so no single
//! spelling of a backslash is right for every server. Names and hosts never need one and
//! are refused when they hold one, which makes their spelling the same in both modes. A
//! password may hold anything, so its backslashes are written for the mode read from the
//! very connection the statement then runs on (see `Session::create_user`). All of this
//! rests on the connection's character set being utf8mb4, as `mysql.rs` opens it: no byte
//! of a multibyte UTF-8 character is a quote or a backslash, which is not so in GBK or SJIS.

use serde::{Deserialize, Serialize};
use sqlx::Executor;

use super::{ident, Session};
use crate::model::Engine;
use crate::{Error, Result};

/// What a preview shows where the password literal is.
pub const MASK: &str = "'••••••••'";
/// Longer than any password manager makes; a bound on the size of the statement.
pub const MAX_PASSWORD_CHARS: usize = 256;

/// An account as the server names it: a MySQL user at a host, or a PostgreSQL role.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountRef {
    pub name: String,
    /// MySQL only. Left out for a new user: `%`, any host.
    #[serde(default)]
    pub host: Option<String>,
}

/// What a new user is for. The point of the form: most accounts are one of these.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Template {
    /// Everything inside one database, nothing outside it.
    App,
    /// May read every table and view of one database.
    ReadOnly,
    /// The account alone.
    None,
}

/// A user to create. No `Debug`: it holds a password.
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UserSpec {
    pub account: AccountRef,
    pub template: Template,
    /// Required by `App` and `ReadOnly`.
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub password: String,
}

/// One statement of a form, in the order it runs.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub sql: String,
    /// PostgreSQL: the database this statement runs inside. `None`: the one the session is
    /// opened on (roles and database privileges belong to the whole server).
    pub database: Option<String>,
}

/// What the builder has to be told about the server it writes for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// MySQL: the session's `sql_mode` has `NO_BACKSLASH_ESCAPES`.
    pub no_backslash_escapes: bool,
    /// MySQL: `partial_revokes` is on, which makes `_` and `%` in the database name of a
    /// grant ordinary characters. Otherwise they are wildcards and have to be escaped.
    pub literal_wildcards: bool,
    /// PostgreSQL: `standard_conforming_strings` is on (a backslash in a literal is itself).
    pub standard_strings: bool,
    /// PostgreSQL: the schemas of the target database that are not the server's own.
    pub schemas: Vec<String>,
}

/// Where the password goes in a statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    /// The statement that runs: the password must be there.
    Run,
    /// A preview with the mask in its place. An empty password is not a mistake yet.
    Masked,
    /// A preview with the password, because the user asked to see it.
    Revealed,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub host: Option<String>,
    /// The account this connection is logged in as.
    pub you: bool,
    /// One of the server's own accounts: shown, not changed.
    pub system: bool,
    /// MySQL: the account is locked. PostgreSQL: the role cannot log in.
    pub locked: bool,
    pub superuser: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoleAttributes {
    pub login: bool,
    pub superuser: bool,
    pub create_database: bool,
    pub create_role: bool,
    pub replication: bool,
    pub bypass_rls: bool,
    /// -1: no limit.
    pub connection_limit: i64,
    pub valid_until: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseAccess {
    pub name: String,
    pub owner: bool,
    pub connect: bool,
    pub create: bool,
    pub temporary: bool,
}

/// What one account may do, as far as this version reads it.
#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccountDetails {
    /// MySQL: `SHOW GRANTS`, one statement each — the server's own account of it.
    pub grants: Vec<String>,
    /// MySQL: the authentication plugin, and whether the password has expired.
    pub plugin: Option<String>,
    pub password_expired: bool,
    /// PostgreSQL.
    pub attributes: Option<RoleAttributes>,
    pub member_of: Vec<String>,
    pub members: Vec<String>,
    /// PostgreSQL: per database. Privileges on the tables inside are not read.
    pub databases: Vec<DatabaseAccess>,
}

fn supported(engine: Engine) -> Result<()> {
    match engine {
        Engine::Mysql | Engine::Postgres => Ok(()),
        Engine::Sqlite => Err(Error::Unsupported(
            "A SQLite file has no users: whoever can read the file can read the database".into(),
        )),
        Engine::Sqlserver => Err(Error::Unsupported(
            "SQL Server connections are not implemented yet".into(),
        )),
    }
}

/// Does the session's `sql_mode` (as `@@sql_mode` prints it) have `NO_BACKSLASH_ESCAPES`?
pub fn no_backslash_escapes(sql_mode: &str) -> bool {
    sql_mode
        .split(',')
        .any(|mode| mode.trim().eq_ignore_ascii_case("NO_BACKSLASH_ESCAPES"))
}

/// The server's own accounts. The forms show them and change none of them.
pub fn is_system(engine: Engine, account: &AccountRef) -> bool {
    match engine {
        // `mysql.sys`, `mysql.session`, `mysql.infoschema`: a dot cannot be typed by mistake.
        Engine::Mysql => account.name.starts_with("mysql."),
        Engine::Postgres => account.name.starts_with("pg_"),
        _ => false,
    }
}

/// The account as people write it, and as the drop form asks for it to be typed.
pub fn display_name(engine: Engine, account: &AccountRef) -> String {
    match (engine, &account.host) {
        (Engine::Mysql, Some(host)) => format!("{}@{host}", account.name),
        _ => account.name.clone(),
    }
}

/// A MySQL account name or host as a string literal. A backslash is refused: nothing needs
/// one there, and without it the text reads the same whatever the server's `sql_mode`.
fn mysql_name(what: &str, value: &str) -> Result<String> {
    if value.contains('\0') {
        return Err(Error::Invalid(format!("The {what} contains a NUL byte")));
    }
    if value.contains('\\') {
        return Err(Error::Invalid(format!(
            "The {what} contains a backslash, which PuriCat does not write into an account name"
        )));
    }
    Ok(format!("'{}'", value.replace('\'', "''")))
}

/// The account as it stands in a statement. Taken exactly as given: an existing account is
/// named as the server lists it.
fn account_sql(engine: Engine, account: &AccountRef) -> Result<String> {
    supported(engine)?;
    if account.name.is_empty() {
        return Err(Error::Invalid("Which account?".into()));
    }
    match engine {
        Engine::Mysql => {
            let host = account
                .host
                .as_deref()
                .filter(|h| !h.is_empty())
                .ok_or_else(|| Error::Invalid("A MySQL account needs a host".into()))?;
            Ok(format!(
                "{}@{}",
                mysql_name("user name", &account.name)?,
                mysql_name("host", host)?
            ))
        }
        _ => {
            if account.host.as_deref().is_some_and(|h| !h.is_empty()) {
                return Err(Error::Invalid(
                    "A PostgreSQL role has no host: who may connect from where is in pg_hba.conf"
                        .into(),
                ));
            }
            ident::postgres(&account.name)
        }
    }
}

/// The account of a new user: trimmed, with MySQL's host defaulting to any (`%`), and no
/// longer than the server stores. Longer names are not left to the server: PostgreSQL would
/// shorten the role's name without saying so.
fn new_account(engine: Engine, account: &AccountRef) -> Result<AccountRef> {
    supported(engine)?;
    let name = account.name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("The user needs a name".into()));
    }
    let too_long = |what: &str, most: usize, unit: &str, length: usize| {
        Error::Invalid(format!(
            "A {what} can be at most {most} {unit} long; this one is {length}"
        ))
    };
    match engine {
        Engine::Mysql => {
            let length = name.chars().count();
            if length > 32 {
                return Err(too_long("MySQL user name", 32, "characters", length));
            }
            let host = account.host.as_deref().map(str::trim).unwrap_or_default();
            let host = if host.is_empty() { "%" } else { host };
            let length = host.chars().count();
            if length > 255 {
                return Err(too_long("host", 255, "characters", length));
            }
            Ok(AccountRef {
                name: name.to_string(),
                host: Some(host.to_string()),
            })
        }
        _ => {
            if name.len() > 63 {
                return Err(too_long("PostgreSQL role name", 63, "bytes", name.len()));
            }
            Ok(AccountRef {
                name: name.to_string(),
                host: account.host.clone(),
            })
        }
    }
}

/// The password as a string literal. Never trimmed: spaces are part of a password.
fn password_literal(engine: Engine, facts: &Facts, password: &str) -> Result<String> {
    if password.is_empty() {
        return Err(Error::Invalid("The password is empty".into()));
    }
    if password.contains('\0') {
        return Err(Error::Invalid("The password contains a NUL byte".into()));
    }
    let length = password.chars().count();
    if length > MAX_PASSWORD_CHARS {
        return Err(Error::Invalid(format!(
            "A password can be at most {MAX_PASSWORD_CHARS} characters long here; this one is {length}"
        )));
    }
    let doubled = password.replace('\'', "''");
    match engine {
        // A backslash starts an escape sequence unless the session says otherwise.
        Engine::Mysql if !facts.no_backslash_escapes => {
            Ok(format!("'{}'", doubled.replace('\\', "\\\\")))
        }
        Engine::Mysql => Ok(format!("'{doubled}'")),
        _ => {
            if !facts.standard_strings {
                return Err(Error::Unsupported(
                    "This server has standard_conforming_strings off, so a password cannot be written into a statement safely. Turn it on (it is the default) to use this form."
                        .into(),
                ));
            }
            Ok(format!("'{doubled}'"))
        }
    }
}

fn shown_password(engine: Engine, facts: &Facts, password: &str, show: Show) -> Result<String> {
    match show {
        // Nothing typed yet: the preview still shows what will run around it.
        Show::Masked if password.is_empty() => Ok(MASK.to_string()),
        // A password that cannot be written is said now, not after "Create".
        Show::Masked => password_literal(engine, facts, password).map(|_| MASK.to_string()),
        Show::Run | Show::Revealed => password_literal(engine, facts, password),
    }
}

/// The database of a MySQL grant, `` `db`.* ``. There `_` and `%` are wildcards (a grant on
/// `shop_2026` would also cover `shopX2026`) unless `partial_revokes` is on, so they are
/// escaped — and must not be when it is on, where the backslash would be taken literally.
fn mysql_grant_target(database: &str, facts: &Facts) -> Result<String> {
    if facts.literal_wildcards {
        return Ok(format!("{}.*", ident::mysql(database)?));
    }
    if database.contains('\\') {
        return Err(Error::Invalid(format!(
            "The name of database {database} contains a backslash; PuriCat cannot write a grant for it"
        )));
    }
    let escaped = database.replace('_', "\\_").replace('%', "\\%");
    Ok(format!("{}.*", ident::mysql(&escaped)?))
}

fn target_database(spec: &UserSpec) -> Result<Option<&str>> {
    let database = spec.database.as_deref().filter(|d| !d.is_empty());
    match (spec.template, database) {
        (Template::None, Some(_)) => Err(Error::Invalid(
            "A user without privileges is not tied to a database".into(),
        )),
        (Template::None, None) => Ok(None),
        (_, None) => Err(Error::Invalid("Choose the database the user is for".into())),
        (_, Some(database)) => Ok(Some(database)),
    }
}

/// The statements that create a user from a template, in the order they run.
pub fn create_user(
    engine: Engine,
    spec: &UserSpec,
    facts: &Facts,
    show: Show,
) -> Result<Vec<Step>> {
    let account = new_account(engine, &spec.account)?;
    let who = account_sql(engine, &account)?;
    let database = target_database(spec)?;
    let password = shown_password(engine, facts, &spec.password, show)?;
    let step = |sql: String, database: Option<&str>| Step {
        sql,
        database: database.map(str::to_string),
    };
    let mut steps = Vec::new();
    match engine {
        Engine::Mysql => {
            steps.push(step(
                format!("CREATE USER {who} IDENTIFIED BY {password}"),
                None,
            ));
            if let Some(database) = database {
                let privileges = match spec.template {
                    Template::App => "ALL PRIVILEGES",
                    _ => "SELECT, SHOW VIEW",
                };
                let target = mysql_grant_target(database, facts)?;
                steps.push(step(
                    format!("GRANT {privileges} ON {target} TO {who}"),
                    None,
                ));
            }
        }
        _ => {
            steps.push(step(
                format!("CREATE ROLE {who} LOGIN PASSWORD {password}"),
                None,
            ));
            let Some(database) = database else {
                return Ok(steps);
            };
            steps.push(step(
                format!(
                    "GRANT CONNECT ON DATABASE {} TO {who}",
                    ident::postgres(database)?
                ),
                None,
            ));
            let inside = |sql: String| step(sql, Some(database));
            for schema in &facts.schemas {
                let schema = ident::postgres(schema)?;
                if spec.template == Template::App {
                    steps.push(inside(format!(
                        "GRANT USAGE, CREATE ON SCHEMA {schema} TO {who}"
                    )));
                    for what in ["TABLES", "SEQUENCES"] {
                        steps.push(inside(format!(
                            "GRANT ALL PRIVILEGES ON ALL {what} IN SCHEMA {schema} TO {who}"
                        )));
                    }
                    for what in ["TABLES", "SEQUENCES"] {
                        steps.push(inside(format!(
                            "ALTER DEFAULT PRIVILEGES IN SCHEMA {schema} GRANT ALL PRIVILEGES ON {what} TO {who}"
                        )));
                    }
                } else {
                    steps.push(inside(format!("GRANT USAGE ON SCHEMA {schema} TO {who}")));
                    steps.push(inside(format!(
                        "GRANT SELECT ON ALL TABLES IN SCHEMA {schema} TO {who}"
                    )));
                    steps.push(inside(format!(
                        "ALTER DEFAULT PRIVILEGES IN SCHEMA {schema} GRANT SELECT ON TABLES TO {who}"
                    )));
                }
            }
        }
    }
    Ok(steps)
}

/// The statement that gives an existing account another password.
pub fn change_password(
    engine: Engine,
    account: &AccountRef,
    password: &str,
    facts: &Facts,
    show: Show,
) -> Result<String> {
    let who = account_sql(engine, account)?;
    let password = shown_password(engine, facts, password, show)?;
    Ok(match engine {
        Engine::Mysql => format!("ALTER USER {who} IDENTIFIED BY {password}"),
        _ => format!("ALTER ROLE {who} PASSWORD {password}"),
    })
}

/// The statement that drops an account.
pub fn drop_user(engine: Engine, account: &AccountRef) -> Result<String> {
    let who = account_sql(engine, account)?;
    Ok(match engine {
        Engine::Mysql => format!("DROP USER {who}"),
        _ => format!("DROP ROLE {who}"),
    })
}

/// What undoes the first steps of `create_user` when a later one failed.
fn undo_create(engine: Engine, account: &AccountRef, database: Option<&str>) -> Result<String> {
    let who = account_sql(engine, account)?;
    Ok(match (engine, database) {
        // A role that holds a privilege cannot be dropped: the one granted goes first.
        (Engine::Postgres, Some(database)) => format!(
            "REVOKE ALL PRIVILEGES ON DATABASE {} FROM {who}; DROP ROLE {who}",
            ident::postgres(database)?
        ),
        (Engine::Postgres, None) => format!("DROP ROLE {who}"),
        _ => format!("DROP USER {who}"),
    })
}

/// A server's message, without the password should the server have quoted the statement:
/// the literal as it was written (quotes doubled; on MySQL backslashes doubled too) and the
/// password as typed. A password shorter than four characters is only taken out where it
/// stands in its quotes: anywhere else every short word that happened to match would go.
fn without_password(error: Error, password: &str) -> Error {
    let Error::Database(mut message) = error else {
        return error;
    };
    if password.is_empty() {
        return Error::Database(message);
    }
    let doubled = password.replace('\'', "''");
    let escaped = doubled.replace('\\', "\\\\");
    let mut forms = vec![format!("'{escaped}'"), format!("'{doubled}'")];
    if password.chars().count() >= 4 {
        forms.extend([escaped, doubled, password.to_string()]);
    }
    for form in forms {
        if message.contains(&form) {
            message = message.replace(&form, "••••••••");
        }
    }
    Error::Database(message)
}

fn flag(cell: Option<&Option<String>>) -> bool {
    matches!(
        cell.and_then(|c| c.as_deref()),
        Some("t" | "true" | "Y" | "1")
    )
}

fn cell(row: &[Option<String>], index: usize) -> Option<String> {
    row.get(index).cloned().flatten()
}

impl Session {
    /// What the statements depend on, read from the server: for a preview. `database` is the
    /// one a new user is for.
    async fn user_facts(&self, database: Option<&str>) -> Result<Facts> {
        match self {
            Session::MySql(s) => s.account_facts().await,
            Session::Postgres(s) => {
                let on = s
                    .query_text(
                        s.home(),
                        "SELECT current_setting('standard_conforming_strings')",
                        &[],
                    )
                    .await?;
                let mut schemas = Vec::new();
                if let Some(database) = database {
                    schemas = s
                        .list_schemas(database)
                        .await?
                        .into_iter()
                        .filter(|schema| !schema.system)
                        .map(|schema| schema.name)
                        .collect();
                }
                Ok(Facts {
                    standard_strings: on.first().and_then(|r| cell(r, 0)).as_deref() == Some("on"),
                    schemas,
                    ..Default::default()
                })
            }
            Session::Sqlite(_) => Err(supported(Engine::Sqlite).unwrap_err()),
        }
    }

    /// The account this session is logged in as.
    async fn own_account(&self) -> Result<AccountRef> {
        match self {
            Session::MySql(s) => {
                let rows = s.query_text("SELECT CURRENT_USER()", &[]).await?;
                let own = rows.first().and_then(|r| cell(r, 0)).unwrap_or_default();
                // `user@host`; the host cannot hold an `@`, the user can.
                let (name, host) = own.rsplit_once('@').unwrap_or((&own, ""));
                Ok(AccountRef {
                    name: name.to_string(),
                    host: Some(host.to_string()),
                })
            }
            Session::Postgres(s) => {
                let rows = s
                    .query_text(s.home(), "SELECT current_user::text", &[])
                    .await?;
                Ok(AccountRef {
                    name: rows.first().and_then(|r| cell(r, 0)).unwrap_or_default(),
                    host: None,
                })
            }
            Session::Sqlite(_) => Err(supported(Engine::Sqlite).unwrap_err()),
        }
    }

    /// The accounts of the server.
    pub async fn list_accounts(&self) -> Result<Vec<Account>> {
        let engine = self.engine();
        supported(engine)?;
        let own = self.own_account().await?;
        let rows = match self {
            Session::MySql(s) => {
                s.query_text(
                    "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), CAST(account_locked AS CHAR)
                     FROM mysql.user ORDER BY User, Host",
                    &[],
                )
                .await?
            }
            Session::Postgres(s) => {
                // Roles that can log in first: they are the users.
                s.query_text(
                    s.home(),
                    "SELECT rolname::text, NULL::text, (NOT rolcanlogin)::text, rolsuper::text
                     FROM pg_roles WHERE rolname !~ '^pg_'
                     ORDER BY rolcanlogin DESC, rolname",
                    &[],
                )
                .await?
            }
            Session::Sqlite(_) => unreachable!("refused above"),
        };
        Ok(rows
            .iter()
            .map(|row| {
                let account = AccountRef {
                    name: cell(row, 0).unwrap_or_default(),
                    host: cell(row, 1),
                };
                Account {
                    you: account == own,
                    system: is_system(engine, &account),
                    locked: flag(row.get(2)),
                    superuser: flag(row.get(3)),
                    name: account.name,
                    host: account.host,
                }
            })
            .collect())
    }

    /// What one account may do. Reads only.
    pub async fn account_details(&self, account: &AccountRef) -> Result<AccountDetails> {
        let engine = self.engine();
        let who = account_sql(engine, account)?;
        let missing = || Error::NotFound(format!("Account {}", display_name(engine, account)));
        match self {
            Session::MySql(s) => {
                let rows = s
                    .query_text(
                        "SELECT CAST(plugin AS CHAR), CAST(password_expired AS CHAR)
                         FROM mysql.user WHERE User = ? AND Host = ?",
                        &[Some(account.name.clone()), account.host.clone()],
                    )
                    .await?;
                let row = rows.first().ok_or_else(missing)?;
                Ok(AccountDetails {
                    grants: s.show_grants(&who).await?,
                    plugin: cell(row, 0),
                    password_expired: flag(row.get(1)),
                    ..Default::default()
                })
            }
            Session::Postgres(s) => {
                let name = [Some(account.name.clone())];
                let rows = s
                    .query_text(
                        s.home(),
                        "SELECT rolcanlogin::text, rolsuper::text, rolcreatedb::text,
                                rolcreaterole::text, rolreplication::text, rolbypassrls::text,
                                rolconnlimit::text, rolvaliduntil::text
                         FROM pg_roles WHERE rolname = $1",
                        &name,
                    )
                    .await?;
                let row = rows.first().ok_or_else(missing)?;
                let attributes = RoleAttributes {
                    login: flag(row.first()),
                    superuser: flag(row.get(1)),
                    create_database: flag(row.get(2)),
                    create_role: flag(row.get(3)),
                    replication: flag(row.get(4)),
                    bypass_rls: flag(row.get(5)),
                    connection_limit: cell(row, 6).and_then(|n| n.parse().ok()).unwrap_or(-1),
                    valid_until: cell(row, 7),
                };
                let names = |rows: Vec<Vec<Option<String>>>| {
                    rows.iter().filter_map(|r| cell(r, 0)).collect::<Vec<_>>()
                };
                let member_of = s
                    .query_text(
                        s.home(),
                        "SELECT g.rolname::text FROM pg_auth_members m
                         JOIN pg_roles g ON g.oid = m.roleid
                         JOIN pg_roles r ON r.oid = m.member
                         WHERE r.rolname = $1 ORDER BY 1",
                        &name,
                    )
                    .await?;
                let members = s
                    .query_text(
                        s.home(),
                        "SELECT r.rolname::text FROM pg_auth_members m
                         JOIN pg_roles g ON g.oid = m.roleid
                         JOIN pg_roles r ON r.oid = m.member
                         WHERE g.rolname = $1 ORDER BY 1",
                        &name,
                    )
                    .await?;
                let databases = s
                    .query_text(
                        s.home(),
                        "SELECT d.datname::text, (d.datdba = r.oid)::text,
                                has_database_privilege(r.oid, d.oid, 'CONNECT')::text,
                                has_database_privilege(r.oid, d.oid, 'CREATE')::text,
                                has_database_privilege(r.oid, d.oid, 'TEMPORARY')::text
                         FROM pg_database d, pg_roles r
                         WHERE r.rolname = $1 AND NOT d.datistemplate ORDER BY 1",
                        &name,
                    )
                    .await?;
                Ok(AccountDetails {
                    attributes: Some(attributes),
                    member_of: names(member_of),
                    members: names(members),
                    databases: databases
                        .iter()
                        .map(|row| DatabaseAccess {
                            name: cell(row, 0).unwrap_or_default(),
                            owner: flag(row.get(1)),
                            connect: flag(row.get(2)),
                            create: flag(row.get(3)),
                            temporary: flag(row.get(4)),
                        })
                        .collect(),
                    ..Default::default()
                })
            }
            Session::Sqlite(_) => unreachable!("refused by account_sql"),
        }
    }

    /// The statements `create_user` would run, for the form's preview: with the mask where
    /// the password is, or with the password when `reveal` asks for it.
    pub async fn plan_user(&self, spec: &UserSpec, reveal: bool) -> Result<Vec<Step>> {
        let engine = self.engine();
        supported(engine)?;
        let database = target_database(spec)?;
        let facts = self.user_facts(database).await?;
        let show = if reveal { Show::Revealed } else { Show::Masked };
        create_user(engine, spec, &facts, show)
    }

    /// Create a user from a template. All of it or none: when a later step fails, the
    /// account that was just made is dropped again, and the error says so. Returns the
    /// statements that ran, with the mask where the password is.
    pub async fn create_user(&self, spec: &UserSpec) -> Result<Vec<Step>> {
        let engine = self.engine();
        supported(engine)?;
        let account = new_account(engine, &spec.account)?;
        let database = target_database(spec)?;
        let undone = |error: Error, undo: Result<()>| {
            let error = without_password(error, &spec.password);
            let name = display_name(engine, &account);
            Error::Database(match undo {
                Ok(()) => format!(
                    "{error}\n\nNothing was kept: the account {name} that had just been created was dropped again."
                ),
                Err(undo) => format!(
                    "{error}\n\nThe account {name} had been created and could not be removed again ({undo}). Drop it from this list."
                ),
            })
        };
        match self {
            Session::MySql(s) => {
                // One connection for all of it: the mode its literals are written for is
                // the mode they are read in.
                let (mut conn, facts) = s.account_connection().await?;
                let steps = create_user(engine, spec, &facts, Show::Run)?;
                let mut result = Ok(());
                for (index, step) in steps.iter().enumerate() {
                    if let Err(e) = conn.execute(step.sql.as_str()).await {
                        let error = Error::database(e);
                        result = Err(if index == 0 {
                            without_password(error, &spec.password)
                        } else {
                            let undo = undo_create(engine, &account, None)?;
                            let undo = conn.execute(undo.as_str()).await;
                            undone(error, undo.map(|_| ()).map_err(Error::database))
                        });
                        break;
                    }
                }
                let _ = sqlx::Connection::close(conn).await;
                result?;
                create_user(engine, spec, &facts, Show::Masked)
            }
            Session::Postgres(s) => {
                let facts = self.user_facts(database).await?;
                let steps = create_user(engine, spec, &facts, Show::Run)?;
                // Several statements sent as one simple query are one implicit transaction:
                // the role and its right to connect, then everything inside the database.
                let sent = |inside: bool| {
                    steps
                        .iter()
                        .filter(|step| step.database.is_some() == inside)
                        .map(|step| step.sql.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                s.execute_statement(&sent(false))
                    .await
                    .map_err(|e| without_password(e, &spec.password))?;
                if let (Some(database), false) = (database, sent(true).is_empty()) {
                    if let Err(error) = s.execute_on(database, &sent(true)).await {
                        let undo = undo_create(engine, &account, Some(database))?;
                        return Err(undone(error, s.execute_statement(&undo).await));
                    }
                }
                create_user(engine, spec, &facts, Show::Masked)
            }
            Session::Sqlite(_) => unreachable!("refused above"),
        }
    }

    /// The accounts the forms leave alone: the server's own, the one this session is logged
    /// in as (changing it would cut the connection off from its saved password), and a
    /// PostgreSQL superuser.
    async fn refuse_untouchable(&self, account: &AccountRef, action: &str) -> Result<()> {
        let engine = self.engine();
        supported(engine)?;
        let name = display_name(engine, account);
        if is_system(engine, account) {
            return Err(Error::Invalid(format!(
                "{name} is one of the server's own accounts; PuriCat does not {action} it"
            )));
        }
        if *account == self.own_account().await? {
            return Err(Error::Invalid(format!(
                "{name} is the account this connection is logged in as; PuriCat does not {action} it from here"
            )));
        }
        if let Session::Postgres(s) = self {
            let rows = s
                .query_text(
                    s.home(),
                    "SELECT rolsuper::text FROM pg_roles WHERE rolname = $1",
                    &[Some(account.name.clone())],
                )
                .await?;
            if flag(rows.first().and_then(|r| r.first())) {
                return Err(Error::Invalid(format!(
                    "{name} is a superuser; PuriCat does not {action} it"
                )));
            }
        }
        Ok(())
    }

    /// The statement `change_password` would run, for the form's preview.
    pub async fn plan_password(
        &self,
        account: &AccountRef,
        password: &str,
        reveal: bool,
    ) -> Result<String> {
        self.refuse_untouchable(account, "change the password of")
            .await?;
        let facts = self.user_facts(None).await?;
        let show = if reveal { Show::Revealed } else { Show::Masked };
        change_password(self.engine(), account, password, &facts, show)
    }

    /// Give an account another password. Returns the statement that ran, masked.
    pub async fn change_password(&self, account: &AccountRef, password: &str) -> Result<String> {
        let engine = self.engine();
        self.refuse_untouchable(account, "change the password of")
            .await?;
        let facts = match self {
            Session::MySql(s) => {
                let (mut conn, facts) = s.account_connection().await?;
                let sql = change_password(engine, account, password, &facts, Show::Run)?;
                let result = conn.execute(sql.as_str()).await;
                let _ = sqlx::Connection::close(conn).await;
                result.map_err(|e| without_password(Error::database(e), password))?;
                facts
            }
            Session::Postgres(s) => {
                let facts = self.user_facts(None).await?;
                let sql = change_password(engine, account, password, &facts, Show::Run)?;
                s.execute_statement(&sql)
                    .await
                    .map_err(|e| without_password(e, password))?;
                facts
            }
            Session::Sqlite(_) => unreachable!("refused above"),
        };
        change_password(engine, account, password, &facts, Show::Masked)
    }

    /// The exact statement `drop_user` would run.
    pub async fn plan_drop_user(&self, account: &AccountRef) -> Result<String> {
        self.refuse_untouchable(account, "drop").await?;
        drop_user(self.engine(), account)
    }

    /// Drop an account. Returns the statement that ran.
    pub async fn drop_user(&self, account: &AccountRef) -> Result<String> {
        let sql = self.plan_drop_user(account).await?;
        match self {
            Session::MySql(s) => s.execute_statement(&sql).await?,
            Session::Postgres(s) => s.execute_statement(&sql).await?,
            Session::Sqlite(_) => unreachable!("refused above"),
        }
        Ok(sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(name: &str, host: Option<&str>) -> AccountRef {
        AccountRef {
            name: name.into(),
            host: host.map(str::to_string),
        }
    }

    fn user(
        name: &str,
        host: Option<&str>,
        template: Template,
        database: Option<&str>,
    ) -> UserSpec {
        UserSpec {
            account: at(name, host),
            template,
            database: database.map(str::to_string),
            password: "s3cret pass".into(),
        }
    }

    fn pg() -> Facts {
        Facts {
            standard_strings: true,
            schemas: vec!["public".into(), "shop".into()],
            ..Default::default()
        }
    }

    fn texts(steps: &[Step]) -> Vec<&str> {
        steps.iter().map(|s| s.sql.as_str()).collect()
    }

    #[test]
    fn mysql_templates() {
        let facts = Facts::default();
        let app = user(" app ", None, Template::App, Some("shop_2026"));
        assert_eq!(
            texts(&create_user(Engine::Mysql, &app, &facts, Show::Run).unwrap()),
            [
                "CREATE USER 'app'@'%' IDENTIFIED BY 's3cret pass'",
                // `_` would match any character in the database of a grant.
                "GRANT ALL PRIVILEGES ON `shop\\_2026`.* TO 'app'@'%'",
            ]
        );
        let reader = user(
            "reader",
            Some("localhost"),
            Template::ReadOnly,
            Some("shop"),
        );
        assert_eq!(
            texts(&create_user(Engine::Mysql, &reader, &facts, Show::Masked).unwrap()),
            [
                "CREATE USER 'reader'@'localhost' IDENTIFIED BY '••••••••'",
                "GRANT SELECT, SHOW VIEW ON `shop`.* TO 'reader'@'localhost'",
            ]
        );
        let bare = user("bare", Some("10.0.%"), Template::None, None);
        assert_eq!(
            texts(&create_user(Engine::Mysql, &bare, &facts, Show::Revealed).unwrap()),
            ["CREATE USER 'bare'@'10.0.%' IDENTIFIED BY 's3cret pass'"]
        );
        // With partial_revokes the server reads `_` and `%` as themselves.
        let literal = Facts {
            literal_wildcards: true,
            ..Default::default()
        };
        assert_eq!(
            create_user(Engine::Mysql, &app, &literal, Show::Run).unwrap()[1].sql,
            "GRANT ALL PRIVILEGES ON `shop_2026`.* TO 'app'@'%'"
        );
        // No statement runs inside a database on MySQL.
        assert!(create_user(Engine::Mysql, &app, &facts, Show::Run)
            .unwrap()
            .iter()
            .all(|s| s.database.is_none()));
    }

    #[test]
    fn postgres_templates() {
        let app = user("app", None, Template::App, Some("shop db"));
        let steps = create_user(Engine::Postgres, &app, &pg(), Show::Masked).unwrap();
        assert_eq!(
            texts(&steps),
            [
                "CREATE ROLE \"app\" LOGIN PASSWORD '••••••••'",
                "GRANT CONNECT ON DATABASE \"shop db\" TO \"app\"",
                "GRANT USAGE, CREATE ON SCHEMA \"public\" TO \"app\"",
                "GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA \"public\" TO \"app\"",
                "GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA \"public\" TO \"app\"",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"public\" GRANT ALL PRIVILEGES ON TABLES TO \"app\"",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"public\" GRANT ALL PRIVILEGES ON SEQUENCES TO \"app\"",
                "GRANT USAGE, CREATE ON SCHEMA \"shop\" TO \"app\"",
                "GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA \"shop\" TO \"app\"",
                "GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA \"shop\" TO \"app\"",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"shop\" GRANT ALL PRIVILEGES ON TABLES TO \"app\"",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"shop\" GRANT ALL PRIVILEGES ON SEQUENCES TO \"app\"",
            ]
        );
        // The role and its right to connect belong to the server; the rest runs inside.
        let inside: Vec<_> = steps.iter().map(|s| s.database.as_deref()).collect();
        assert_eq!(inside[..2], [None, None]);
        assert!(inside[2..].iter().all(|d| *d == Some("shop db")));

        let reader = user("reader", None, Template::ReadOnly, Some("shop"));
        let facts = Facts {
            schemas: vec!["public".into()],
            ..pg()
        };
        assert_eq!(
            texts(&create_user(Engine::Postgres, &reader, &facts, Show::Run).unwrap()),
            [
                "CREATE ROLE \"reader\" LOGIN PASSWORD 's3cret pass'",
                "GRANT CONNECT ON DATABASE \"shop\" TO \"reader\"",
                "GRANT USAGE ON SCHEMA \"public\" TO \"reader\"",
                "GRANT SELECT ON ALL TABLES IN SCHEMA \"public\" TO \"reader\"",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"public\" GRANT SELECT ON TABLES TO \"reader\"",
            ]
        );
        let bare = user("bare", None, Template::None, None);
        assert_eq!(
            texts(&create_user(Engine::Postgres, &bare, &pg(), Show::Run).unwrap()),
            ["CREATE ROLE \"bare\" LOGIN PASSWORD 's3cret pass'"]
        );
    }

    #[test]
    fn password_and_drop_statements() {
        let facts = Facts::default();
        let who = at("app", Some("%"));
        assert_eq!(
            change_password(Engine::Mysql, &who, "new", &facts, Show::Run).unwrap(),
            "ALTER USER 'app'@'%' IDENTIFIED BY 'new'"
        );
        assert_eq!(
            drop_user(Engine::Mysql, &who).unwrap(),
            "DROP USER 'app'@'%'"
        );
        let role = at("app \"one\"", None);
        assert_eq!(
            change_password(Engine::Postgres, &role, "new", &pg(), Show::Masked).unwrap(),
            "ALTER ROLE \"app \"\"one\"\"\" PASSWORD '••••••••'"
        );
        assert_eq!(
            drop_user(Engine::Postgres, &role).unwrap(),
            "DROP ROLE \"app \"\"one\"\"\""
        );
        assert_eq!(display_name(Engine::Mysql, &who), "app@%");
        assert_eq!(display_name(Engine::Postgres, &role), "app \"one\"");
    }

    /// What was typed cannot end the literal or the identifier it is written into: after
    /// the pieces we wrote are taken away, no unpaired quote is left.
    #[test]
    fn nothing_typed_can_leave_its_place() {
        let hostile = [
            "'; DROP USER root; --",
            "a\\' OR 1=1 --",
            "\\",
            "`x`; \"y\"",
            "line\nbreak\r\n",
            "ผู้ใช้ 'หนึ่ง' 🐈",
            "''",
            "%_",
        ];
        for text in hostile {
            // Passwords, in both MySQL modes and on PostgreSQL.
            for (engine, facts) in [
                (Engine::Mysql, Facts::default()),
                (
                    Engine::Mysql,
                    Facts {
                        no_backslash_escapes: true,
                        ..Default::default()
                    },
                ),
                (Engine::Postgres, pg()),
            ] {
                let literal = password_literal(engine, &facts, text).unwrap();
                let inner = &literal[1..literal.len() - 1];
                let escapes = engine == Engine::Mysql && !facts.no_backslash_escapes;
                assert_eq!(
                    read_literal(inner, escapes).as_deref(),
                    Some(text),
                    "{literal}"
                );
            }
            // PostgreSQL role names are identifiers.
            let role = account_sql(Engine::Postgres, &at(text, None)).unwrap();
            assert_eq!(role, format!("\"{}\"", text.replace('"', "\"\"")));
            // MySQL names: the same text in both modes, or refused.
            match account_sql(Engine::Mysql, &at(text, Some(text))) {
                Ok(sql) => {
                    assert!(!text.contains('\\'));
                    let quoted = format!("'{}'", text.replace('\'', "''"));
                    assert_eq!(sql, format!("{quoted}@{quoted}"));
                }
                Err(e) => assert!(
                    text.contains('\\') && matches!(e, Error::Invalid(_)),
                    "{text}"
                ),
            }
        }
    }

    /// Read a literal's inside back the way the server does: `None` if a quote is left open
    /// or closes early.
    fn read_literal(inner: &str, backslash_escapes: bool) -> Option<String> {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '\'' => match chars.next() {
                    Some('\'') => out.push('\''),
                    _ => return None,
                },
                '\\' if backslash_escapes => match chars.next() {
                    Some('\\') => out.push('\\'),
                    // Only the backslash itself is ever escaped by the builder.
                    _ => return None,
                },
                c => out.push(c),
            }
        }
        Some(out)
    }

    #[test]
    fn backslashes_follow_the_mode() {
        let plain = Facts::default();
        let no_escapes = Facts {
            no_backslash_escapes: true,
            ..Default::default()
        };
        assert_eq!(
            password_literal(Engine::Mysql, &plain, "a\\b'c").unwrap(),
            "'a\\\\b''c'"
        );
        assert_eq!(
            password_literal(Engine::Mysql, &no_escapes, "a\\b'c").unwrap(),
            "'a\\b''c'"
        );
        assert_eq!(
            password_literal(Engine::Postgres, &pg(), "a\\b'c").unwrap(),
            "'a\\b''c'"
        );
        assert!(no_backslash_escapes(
            "STRICT_TRANS_TABLES,NO_BACKSLASH_ESCAPES,ANSI_QUOTES"
        ));
        assert!(no_backslash_escapes("no_backslash_escapes"));
        assert!(!no_backslash_escapes("STRICT_TRANS_TABLES,NO_ZERO_DATE"));
        assert!(!no_backslash_escapes(""));
        // Without standard_conforming_strings a backslash would be an escape: refused.
        let off = Facts::default();
        assert!(matches!(
            password_literal(Engine::Postgres, &off, "plain"),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn the_mask_never_holds_the_password() {
        let secret = "Tr0ub4dor&3 'quoted' \\ ผ่าน";
        for (engine, facts, host) in [
            (Engine::Mysql, Facts::default(), Some("%")),
            (Engine::Postgres, pg(), None),
        ] {
            let mut spec = user("app", host, Template::App, Some("shop"));
            spec.password = secret.into();
            let masked = create_user(engine, &spec, &facts, Show::Masked).unwrap();
            let real = create_user(engine, &spec, &facts, Show::Run).unwrap();
            assert_eq!(masked.len(), real.len());
            for (m, r) in masked.iter().zip(&real) {
                assert!(
                    !m.sql.contains("Tr0ub4dor") && !m.sql.contains("ผ่าน"),
                    "{}",
                    m.sql
                );
                // The same statement, but for the literal.
                let literal = password_literal(engine, &facts, secret).unwrap();
                assert_eq!(m.sql.replace(MASK, &literal), r.sql);
                assert_eq!(m.database, r.database);
            }
            assert!(real[0].sql.contains("Tr0ub4dor"));
            let who = at("app", host);
            let masked = change_password(engine, &who, secret, &facts, Show::Masked).unwrap();
            assert!(masked.ends_with(MASK) && !masked.contains("Tr0ub4dor"));
        }
        // A server's message that quotes the statement loses the password too: as typed, as
        // the literal was written for either server, and a short one where it stands quoted.
        let scrubbed = without_password(
            Error::Database(format!("syntax error near '{secret}'")),
            secret,
        );
        assert!(!scrubbed.to_string().contains("Tr0ub4dor"));
        let written = password_literal(Engine::Mysql, &Facts::default(), "a\\b'c d").unwrap();
        assert_eq!(written, "'a\\\\b''c d'");
        let scrubbed = without_password(
            Error::Database(format!("You have an error near {written} at line 1")),
            "a\\b'c d",
        )
        .to_string();
        assert!(
            !scrubbed.contains("c d") && scrubbed.contains("at line 1"),
            "{scrubbed}"
        );
        let scrubbed = without_password(
            Error::Database("syntax error at or near \"'x''y'\"".into()),
            "x'y",
        )
        .to_string();
        assert!(!scrubbed.contains("x''y"), "{scrubbed}");
        // A short one is not hunted through the rest of the message.
        let kept = without_password(Error::Database("role ab exists".into()), "ab").to_string();
        assert!(kept.contains("role ab exists"));
        // Other errors pass unchanged.
        assert!(matches!(
            without_password(Error::Invalid("x".into()), "x"),
            Error::Invalid(_)
        ));
        // No Debug for the spec: it would print the password. (A compile-time property:
        // `UserSpec` does not derive it.)
    }

    #[test]
    fn refuses_what_cannot_be_an_account() {
        let facts = Facts::default();
        let invalid = |r: Result<Vec<Step>>| matches!(r, Err(Error::Invalid(_)));
        for engine in [Engine::Mysql, Engine::Postgres] {
            let facts = if engine == Engine::Postgres {
                pg()
            } else {
                facts.clone()
            };
            assert!(invalid(create_user(
                engine,
                &user("  ", None, Template::None, None),
                &facts,
                Show::Run
            )));
            assert!(invalid(create_user(
                engine,
                &user("a\0b", None, Template::None, None),
                &facts,
                Show::Run
            )));
            // A template for a database needs one, and "none" takes none.
            assert!(invalid(create_user(
                engine,
                &user("a", None, Template::App, None),
                &facts,
                Show::Run
            )));
            assert!(invalid(create_user(
                engine,
                &user("a", None, Template::ReadOnly, Some("")),
                &facts,
                Show::Run
            )));
            assert!(invalid(create_user(
                engine,
                &user("a", None, Template::None, Some("shop")),
                &facts,
                Show::Run
            )));
            // Passwords: empty only in a masked preview; NUL and over-long never.
            let mut spec = user("a", None, Template::None, None);
            spec.password = String::new();
            assert!(invalid(create_user(engine, &spec, &facts, Show::Run)));
            assert!(invalid(create_user(engine, &spec, &facts, Show::Revealed)));
            assert_eq!(
                create_user(engine, &spec, &facts, Show::Masked)
                    .unwrap()
                    .len(),
                1
            );
            spec.password = "a\0b".into();
            assert!(invalid(create_user(engine, &spec, &facts, Show::Masked)));
            spec.password = "ก".repeat(MAX_PASSWORD_CHARS);
            assert!(create_user(engine, &spec, &facts, Show::Run).is_ok());
            spec.password = "x".repeat(MAX_PASSWORD_CHARS + 1);
            assert!(invalid(create_user(engine, &spec, &facts, Show::Masked)));
            // An error never repeats the password.
            let error = create_user(engine, &spec, &facts, Show::Run).unwrap_err();
            assert!(!error.to_string().contains("xxxx"));
            // An existing account is named exactly: nothing is trimmed, an empty one refused.
            assert!(drop_user(engine, &at("", Some("%"))).is_err());
        }
        // Lengths: MySQL counts characters, PostgreSQL bytes.
        let ok = |engine, name: &str| {
            let facts = if engine == Engine::Postgres {
                pg()
            } else {
                Facts::default()
            };
            create_user(
                engine,
                &user(name, None, Template::None, None),
                &facts,
                Show::Run,
            )
            .is_ok()
        };
        assert!(ok(Engine::Mysql, &"ก".repeat(32)) && !ok(Engine::Mysql, &"n".repeat(33)));
        assert!(ok(Engine::Postgres, &"n".repeat(63)) && !ok(Engine::Postgres, &"n".repeat(64)));
        assert!(ok(Engine::Postgres, &"ก".repeat(21)) && !ok(Engine::Postgres, &"ก".repeat(22)));
        let long_host = "h".repeat(256);
        assert!(invalid(create_user(
            Engine::Mysql,
            &user("a", Some(&long_host), Template::None, None),
            &facts,
            Show::Run
        )));
        // Backslashes: never in a MySQL name or host, nor in the database of a grant.
        assert!(invalid(create_user(
            Engine::Mysql,
            &user("a\\b", None, Template::None, None),
            &facts,
            Show::Run
        )));
        assert!(invalid(create_user(
            Engine::Mysql,
            &user("a", Some("h\\"), Template::None, None),
            &facts,
            Show::Run
        )));
        assert!(invalid(create_user(
            Engine::Mysql,
            &user("a", None, Template::App, Some("d\\b")),
            &facts,
            Show::Run
        )));
        // A host belongs to MySQL only, and an existing MySQL account needs one.
        assert!(invalid(create_user(
            Engine::Postgres,
            &user("a", Some("%"), Template::None, None),
            &pg(),
            Show::Run
        )));
        assert!(drop_user(Engine::Mysql, &at("a", None)).is_err());
        // SQLite has no users.
        assert!(matches!(
            create_user(
                Engine::Sqlite,
                &user("a", None, Template::None, None),
                &facts,
                Show::Run
            ),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            drop_user(Engine::Sqlite, &at("a", None)),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn the_servers_own_accounts_and_the_undo() {
        assert!(is_system(
            Engine::Mysql,
            &at("mysql.sys", Some("localhost"))
        ));
        assert!(!is_system(Engine::Mysql, &at("root", Some("localhost"))));
        assert!(is_system(Engine::Postgres, &at("pg_monitor", None)));
        assert!(!is_system(Engine::Postgres, &at("postgres", None)));
        assert_eq!(
            undo_create(Engine::Mysql, &at("app", Some("%")), None).unwrap(),
            "DROP USER 'app'@'%'"
        );
        assert_eq!(
            undo_create(Engine::Postgres, &at("app", None), Some("shop")).unwrap(),
            "REVOKE ALL PRIVILEGES ON DATABASE \"shop\" FROM \"app\"; DROP ROLE \"app\""
        );
        assert_eq!(
            undo_create(Engine::Postgres, &at("app", None), None).unwrap(),
            "DROP ROLE \"app\""
        );
    }
}
