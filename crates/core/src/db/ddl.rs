//! Schema changes made through a form rather than typed SQL.
//!
//! One statement per form, built here and nowhere else: the text the user reads in the
//! form's preview is the text that runs. Names go through `ident`; the few values that SQL
//! does not let us quote as identifiers (character sets, collations, encodings) must match
//! a strict pattern before they reach the statement.

use serde::{Deserialize, Serialize};

use super::{ident, ObjectKind, TableRef};
use crate::model::Engine;
use crate::{Error, Result};

/// A database to create. Fields that do not belong to the engine must be left out.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewDatabase {
    pub name: String,
    /// MySQL: `CHARACTER SET`. Left out: the server's default.
    #[serde(default)]
    pub charset: Option<String>,
    /// MySQL: `COLLATE`. Left out: the default collation of the character set.
    #[serde(default)]
    pub collation: Option<String>,
    /// PostgreSQL: `OWNER`. Left out: the connected role.
    #[serde(default)]
    pub owner: Option<String>,
    /// PostgreSQL: `ENCODING`. Left out: the template's encoding.
    #[serde(default)]
    pub encoding: Option<String>,
    /// PostgreSQL: `TEMPLATE`. Left out: `template1`.
    #[serde(default)]
    pub template: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Charset {
    pub name: String,
    pub default_collation: String,
    pub description: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Collation {
    pub name: String,
    pub charset: String,
}

/// What the server offers for a new database, to fill the form's lists.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseOptions {
    pub charsets: Vec<Charset>,
    pub collations: Vec<Collation>,
    /// MySQL: `@@character_set_server` / `@@collation_server`.
    pub default_charset: Option<String>,
    pub default_collation: Option<String>,
    /// PostgreSQL: roles that can own a database, and the connected one.
    pub owners: Vec<String>,
    pub current_user: Option<String>,
    pub templates: Vec<String>,
    /// PostgreSQL: encodings a database can be stored in, and `template1`'s.
    pub encodings: Vec<String>,
    pub default_encoding: Option<String>,
}

/// How long the name of a database or a table may be. MySQL counts characters. PostgreSQL
/// counts bytes, and silently shortens a longer name: the object would get another name than
/// the statement shows, so the limit is enforced here and not left to the server.
fn check_length(engine: Engine, what: &str, name: &str) -> Result<()> {
    let (length, most, unit) = match engine {
        Engine::Postgres => (name.len(), 63, "bytes"),
        _ => (name.chars().count(), 64, "characters"),
    };
    if length > most {
        return Err(Error::Invalid(format!(
            "A {what} name can be at most {most} {unit} long; this one is {length}"
        )));
    }
    Ok(())
}

fn clean(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// A character set, collation or encoding name: these are keywords to the server, not
/// identifiers, so they cannot be quoted — only letters, digits, `_` and `-` are let through.
fn keyword<'a>(what: &str, value: &'a str) -> Result<&'a str> {
    let plain = value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if plain {
        Ok(value)
    } else {
        Err(Error::Invalid(format!("“{value}” is not a valid {what}")))
    }
}

fn refuse(given: &Option<String>, option: &str, engine: &str) -> Result<()> {
    match clean(given) {
        Some(_) => Err(Error::Invalid(format!(
            "{option} is not an option of a {engine} database"
        ))),
        None => Ok(()),
    }
}

fn unsupported(engine: Engine) -> Error {
    Error::Unsupported(
        match engine {
            Engine::Sqlite => {
                "A SQLite connection is one database file; there are no databases to manage inside it"
            }
            _ => "SQL Server connections are not implemented yet",
        }
        .into(),
    )
}

/// Database forms exist for servers that hold several databases.
pub fn supported(engine: Engine) -> Result<()> {
    match engine {
        Engine::Mysql | Engine::Postgres => Ok(()),
        _ => Err(unsupported(engine)),
    }
}

/// The `CREATE DATABASE` statement for `spec`.
pub fn create_database(engine: Engine, spec: &NewDatabase) -> Result<String> {
    supported(engine)?;
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("The database needs a name".into()));
    }
    check_length(engine, "database", name)?;
    match engine {
        Engine::Mysql => {
            refuse(&spec.owner, "An owner", "MySQL")?;
            refuse(&spec.encoding, "An encoding", "MySQL")?;
            refuse(&spec.template, "A template", "MySQL")?;
            let mut sql = format!("CREATE DATABASE {}", ident::mysql(name)?);
            if let Some(charset) = clean(&spec.charset) {
                sql.push_str(&format!(
                    " CHARACTER SET {}",
                    keyword("character set", charset)?
                ));
            }
            if let Some(collation) = clean(&spec.collation) {
                sql.push_str(&format!(" COLLATE {}", keyword("collation", collation)?));
            }
            Ok(sql)
        }
        Engine::Postgres => {
            refuse(&spec.charset, "A character set", "PostgreSQL")?;
            refuse(&spec.collation, "A collation", "PostgreSQL")?;
            let mut sql = format!("CREATE DATABASE {}", ident::postgres(name)?);
            if let Some(owner) = clean(&spec.owner) {
                sql.push_str(&format!(" OWNER {}", ident::postgres(owner)?));
            }
            if let Some(template) = clean(&spec.template) {
                sql.push_str(&format!(" TEMPLATE {}", ident::postgres(template)?));
            }
            if let Some(encoding) = clean(&spec.encoding) {
                sql.push_str(&format!(" ENCODING '{}'", keyword("encoding", encoding)?));
            }
            Ok(sql)
        }
        Engine::Sqlite | Engine::Sqlserver => Err(unsupported(engine)),
    }
}

/// Databases the server itself needs. PuriCat's forms do not drop or alter them.
pub fn is_protected(engine: Engine, name: &str) -> bool {
    match engine {
        Engine::Mysql => matches!(
            name.to_ascii_lowercase().as_str(),
            "information_schema" | "mysql" | "performance_schema" | "sys"
        ),
        Engine::Postgres => matches!(name, "postgres" | "template0" | "template1"),
        _ => false,
    }
}

fn existing(engine: Engine, name: &str) -> Result<&str> {
    if name.is_empty() {
        return Err(Error::Invalid("Which database?".into()));
    }
    if is_protected(engine, name) {
        return Err(Error::Invalid(format!(
            "“{name}” belongs to the server itself; PuriCat does not drop or alter it"
        )));
    }
    Ok(name)
}

/// The `DROP DATABASE` statement for `name`. The name is taken exactly as the server lists it.
pub fn drop_database(engine: Engine, name: &str) -> Result<String> {
    supported(engine)?;
    let name = existing(engine, name)?;
    match engine {
        Engine::Mysql => Ok(format!("DROP DATABASE {}", ident::mysql(name)?)),
        Engine::Postgres => Ok(format!("DROP DATABASE {}", ident::postgres(name)?)),
        _ => Err(unsupported(engine)),
    }
}

/// Changes to an existing database. Only what the engine can change is accepted.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlterDatabase {
    /// The database as the server lists it now.
    pub name: String,
    /// MySQL: the default character set and collation of new tables.
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
    /// PostgreSQL: a new name, a new owner.
    #[serde(default)]
    pub new_name: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
}

/// The `ALTER DATABASE` statements for `spec`, in the order they run.
pub fn alter_database(engine: Engine, spec: &AlterDatabase) -> Result<Vec<String>> {
    supported(engine)?;
    let name = existing(engine, &spec.name)?;
    let nothing = || Error::Invalid("Nothing was changed".into());
    match engine {
        Engine::Mysql => {
            refuse(&spec.new_name, "A new name", "MySQL")?;
            refuse(&spec.owner, "An owner", "MySQL")?;
            let mut sql = format!("ALTER DATABASE {}", ident::mysql(name)?);
            let before = sql.len();
            if let Some(charset) = clean(&spec.charset) {
                sql.push_str(&format!(
                    " CHARACTER SET {}",
                    keyword("character set", charset)?
                ));
            }
            if let Some(collation) = clean(&spec.collation) {
                sql.push_str(&format!(" COLLATE {}", keyword("collation", collation)?));
            }
            if sql.len() == before {
                return Err(nothing());
            }
            Ok(vec![sql])
        }
        Engine::Postgres => {
            refuse(&spec.charset, "A character set", "PostgreSQL")?;
            refuse(&spec.collation, "A collation", "PostgreSQL")?;
            let mut statements = Vec::new();
            // Owner first, while the database still has the name the user is looking at.
            if let Some(owner) = clean(&spec.owner) {
                statements.push(format!(
                    "ALTER DATABASE {} OWNER TO {}",
                    ident::postgres(name)?,
                    ident::postgres(owner)?
                ));
            }
            if let Some(new_name) = clean(&spec.new_name).filter(|n| *n != name) {
                check_length(engine, "database", new_name)?;
                statements.push(format!(
                    "ALTER DATABASE {} RENAME TO {}",
                    ident::postgres(name)?,
                    ident::postgres(new_name)?
                ));
            }
            if statements.is_empty() {
                return Err(nothing());
            }
            Ok(statements)
        }
        _ => Err(unsupported(engine)),
    }
}

/// What a database is set to now, to fill the edit form.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseDetails {
    pub name: String,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub owner: Option<String>,
    pub encoding: Option<String>,
    /// The server's own database: shown, not changed.
    pub protected: bool,
    /// How many tables and views it holds, when that could be counted.
    pub tables: Option<u64>,
}

/// Server maintenance a MySQL connection's menu offers. Fixed statements, nothing typed.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Flush {
    Privileges,
    Tables,
    Logs,
    Status,
}

/// The `FLUSH` statement. (`FLUSH HOSTS` is not offered: MySQL 8.4 removed it.)
pub fn flush(engine: Engine, what: Flush) -> Result<&'static str> {
    if engine != Engine::Mysql {
        return Err(Error::Unsupported("FLUSH is a MySQL statement".into()));
    }
    Ok(match what {
        Flush::Privileges => "FLUSH PRIVILEGES",
        Flush::Tables => "FLUSH TABLES",
        Flush::Logs => "FLUSH LOGS",
        Flush::Status => "FLUSH STATUS",
    })
}

/// Where the text of a definition comes from, so the form can say how far to trust it.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionSource {
    /// The server's own statement (MySQL's `SHOW CREATE`).
    Server,
    /// Put together from the catalogue: PostgreSQL has no statement that prints a table.
    Catalogue,
    /// The text SQLite keeps in the file.
    File,
}

/// The statements that would create a table or a view as it is now. Read, never run.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub sql: String,
    pub source: DefinitionSource,
}

/// A change to one table or view, chosen in a form of the explorer's table menu.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum TableChange {
    #[serde(rename_all = "camelCase")]
    Rename {
        table: TableRef,
        kind: ObjectKind,
        new_name: String,
    },
    /// Remove every row. Tables only.
    Empty {
        table: TableRef,
    },
    Drop {
        table: TableRef,
        kind: ObjectKind,
    },
}

impl TableChange {
    /// The table or view the change is made to, as the server lists it now.
    pub fn table(&self) -> &TableRef {
        match self {
            TableChange::Rename { table, .. }
            | TableChange::Empty { table }
            | TableChange::Drop { table, .. } => table,
        }
    }
}

/// The table's name as the engine addresses it: with its database (MySQL) or its schema
/// (PostgreSQL). A SQLite file is one database, so the name stands alone.
fn table_name(engine: Engine, table: &TableRef) -> Result<String> {
    match engine {
        Engine::Mysql => Ok(format!(
            "{}.{}",
            ident::mysql(&table.database)?,
            ident::mysql(&table.name)?
        )),
        Engine::Postgres => Ok(format!(
            "{}.{}",
            ident::postgres(super::require_schema(table.schema.as_deref())?)?,
            ident::postgres(&table.name)?
        )),
        Engine::Sqlite => ident::sqlite(&table.name),
        Engine::Sqlserver => Err(unsupported(engine)),
    }
}

/// `TABLE`, `VIEW` or `MATERIALIZED VIEW`, for an engine that has that kind of object.
fn object_word(engine: Engine, kind: ObjectKind) -> Result<&'static str> {
    match (kind, engine) {
        (ObjectKind::Table, _) => Ok("TABLE"),
        (ObjectKind::View, _) => Ok("VIEW"),
        (ObjectKind::Matview, Engine::Postgres) => Ok("MATERIALIZED VIEW"),
        (ObjectKind::Matview, _) => Err(Error::Unsupported(
            "Only PostgreSQL has materialized views".into(),
        )),
    }
}

/// The statement for `change`. One statement: no `IF EXISTS`, no `CASCADE` — what stands in
/// the way is the server's to say.
pub fn table_change(engine: Engine, change: &TableChange) -> Result<String> {
    if engine == Engine::Sqlserver {
        return Err(unsupported(engine));
    }
    let name = table_name(engine, change.table())?;
    match change {
        TableChange::Rename {
            table,
            kind,
            new_name,
        } => {
            let word = object_word(engine, *kind)?;
            let new_name = new_name.trim();
            if new_name.is_empty() {
                return Err(Error::Invalid("The new name is empty".into()));
            }
            if new_name == table.name {
                return Err(Error::Invalid("That is its name already".into()));
            }
            check_length(engine, "table", new_name)?;
            match engine {
                // MySQL renames views with the same statement, and names the database on
                // both sides: without it the new name would go to the session's database.
                Engine::Mysql => Ok(format!(
                    "RENAME TABLE {name} TO {}.{}",
                    ident::mysql(&table.database)?,
                    ident::mysql(new_name)?
                )),
                Engine::Postgres => Ok(format!(
                    "ALTER {word} {name} RENAME TO {}",
                    ident::postgres(new_name)?
                )),
                _ if *kind == ObjectKind::View => Err(Error::Unsupported(
                    "SQLite cannot rename a view. Drop it and create it again under the new name."
                        .into(),
                )),
                _ => Ok(format!(
                    "ALTER TABLE {name} RENAME TO {}",
                    ident::sqlite(new_name)?
                )),
            }
        }
        // SQLite has no TRUNCATE: a DELETE without a condition is its way to empty a table.
        TableChange::Empty { .. } if engine == Engine::Sqlite => Ok(format!("DELETE FROM {name}")),
        TableChange::Empty { .. } => Ok(format!("TRUNCATE TABLE {name}")),
        TableChange::Drop { kind, .. } => {
            Ok(format!("DROP {} {name}", object_word(engine, *kind)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> NewDatabase {
        NewDatabase {
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn mysql_statement() {
        assert_eq!(
            create_database(Engine::Mysql, &named("  shop  ")).unwrap(),
            "CREATE DATABASE `shop`"
        );
        let full = NewDatabase {
            charset: Some("utf8mb4".into()),
            collation: Some("utf8mb4_0900_ai_ci".into()),
            ..named("ร้าน `ใหม่`")
        };
        assert_eq!(
            create_database(Engine::Mysql, &full).unwrap(),
            "CREATE DATABASE `ร้าน ``ใหม่``` CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci"
        );
        // An empty choice is "the server's default", not an empty keyword.
        let blank = NewDatabase {
            charset: Some("  ".into()),
            collation: Some(String::new()),
            ..named("a")
        };
        assert_eq!(
            create_database(Engine::Mysql, &blank).unwrap(),
            "CREATE DATABASE `a`"
        );
    }

    #[test]
    fn postgres_statement() {
        assert_eq!(
            create_database(Engine::Postgres, &named("shop")).unwrap(),
            "CREATE DATABASE \"shop\""
        );
        let full = NewDatabase {
            owner: Some("app \"owner\"".into()),
            template: Some("template0".into()),
            encoding: Some("UTF8".into()),
            ..named("Shop-2")
        };
        assert_eq!(
            create_database(Engine::Postgres, &full).unwrap(),
            "CREATE DATABASE \"Shop-2\" OWNER \"app \"\"owner\"\"\" TEMPLATE \"template0\" ENCODING 'UTF8'"
        );
    }

    #[test]
    fn nothing_typed_can_leave_its_place_in_the_statement() {
        // A name is always quoted as one identifier.
        let sql = create_database(Engine::Mysql, &named("x`; DROP DATABASE mysql; --")).unwrap();
        assert_eq!(sql, "CREATE DATABASE `x``; DROP DATABASE mysql; --`");
        let sql =
            create_database(Engine::Postgres, &named("x\"; DROP DATABASE postgres; --")).unwrap();
        assert_eq!(sql, "CREATE DATABASE \"x\"\"; DROP DATABASE postgres; --\"");
        // Keywords cannot be quoted, so anything but a plain word is refused.
        for bad in [
            "utf8mb4; DROP DATABASE mysql",
            "utf8 mb4",
            "utf8'",
            "a`b",
            "ไทย",
        ] {
            let spec = NewDatabase {
                charset: Some(bad.into()),
                ..named("a")
            };
            assert!(
                matches!(
                    create_database(Engine::Mysql, &spec),
                    Err(Error::Invalid(_))
                ),
                "{bad}"
            );
            let spec = NewDatabase {
                encoding: Some(bad.into()),
                ..named("a")
            };
            assert!(
                matches!(
                    create_database(Engine::Postgres, &spec),
                    Err(Error::Invalid(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn refuses_what_cannot_be_a_database() {
        for engine in [Engine::Mysql, Engine::Postgres] {
            assert!(matches!(
                create_database(engine, &named("   ")),
                Err(Error::Invalid(_))
            ));
            assert!(create_database(engine, &named("a\0b")).is_err());
        }
        // MySQL counts characters: 64 of any script.
        assert!(create_database(Engine::Mysql, &named(&"n".repeat(65))).is_err());
        assert!(create_database(Engine::Mysql, &named(&"ก".repeat(64))).is_ok());
        // PostgreSQL counts bytes and would shorten a longer name without saying so:
        // 63 bytes is 63 ASCII letters or 21 Thai ones.
        assert!(create_database(Engine::Postgres, &named(&"n".repeat(63))).is_ok());
        assert!(create_database(Engine::Postgres, &named(&"n".repeat(64))).is_err());
        assert!(create_database(Engine::Postgres, &named(&"ก".repeat(21))).is_ok());
        assert!(create_database(Engine::Postgres, &named(&"ก".repeat(22))).is_err());
        // An option of the other engine is a mistake, not something to drop silently.
        let owner = NewDatabase {
            owner: Some("app".into()),
            ..named("a")
        };
        assert!(create_database(Engine::Mysql, &owner).is_err());
        let charset = NewDatabase {
            charset: Some("utf8mb4".into()),
            ..named("a")
        };
        assert!(create_database(Engine::Postgres, &charset).is_err());
        assert!(matches!(
            create_database(Engine::Sqlite, &named("a")),
            Err(Error::Unsupported(_))
        ));
    }
    #[test]
    fn drop_statement_and_the_servers_own_databases() {
        assert_eq!(
            drop_database(Engine::Mysql, "shop `old`").unwrap(),
            "DROP DATABASE `shop ``old```"
        );
        assert_eq!(
            drop_database(Engine::Postgres, "Shop \"old\"").unwrap(),
            "DROP DATABASE \"Shop \"\"old\"\"\""
        );
        for (engine, name) in [
            (Engine::Mysql, "mysql"),
            (Engine::Mysql, "INFORMATION_SCHEMA"),
            (Engine::Mysql, "sys"),
            (Engine::Postgres, "postgres"),
            (Engine::Postgres, "template1"),
        ] {
            assert!(is_protected(engine, name), "{name}");
            assert!(drop_database(engine, name).is_err(), "{name}");
            let spec = AlterDatabase {
                name: name.into(),
                charset: Some("utf8mb4".into()),
                owner: Some("x".into()),
                ..Default::default()
            };
            assert!(alter_database(engine, &spec).is_err(), "{name}");
        }
        // "postgres" is an ordinary name on MySQL, "mysql" on PostgreSQL.
        assert!(drop_database(Engine::Mysql, "postgres").is_ok());
        assert!(drop_database(Engine::Postgres, "mysql").is_ok());
        assert!(drop_database(Engine::Mysql, "").is_err());
        assert!(matches!(
            drop_database(Engine::Sqlite, "main"),
            Err(Error::Unsupported(_))
        ));
    }

    #[test]
    fn alter_statements() {
        let mysql = |charset: Option<&str>, collation: Option<&str>| AlterDatabase {
            name: "shop".into(),
            charset: charset.map(Into::into),
            collation: collation.map(Into::into),
            ..Default::default()
        };
        assert_eq!(
            alter_database(Engine::Mysql, &mysql(Some("utf8mb4"), Some("utf8mb4_bin"))).unwrap(),
            vec!["ALTER DATABASE `shop` CHARACTER SET utf8mb4 COLLATE utf8mb4_bin"]
        );
        assert_eq!(
            alter_database(Engine::Mysql, &mysql(None, Some("utf8mb4_bin"))).unwrap(),
            vec!["ALTER DATABASE `shop` COLLATE utf8mb4_bin"]
        );
        assert!(alter_database(Engine::Mysql, &mysql(None, None)).is_err());
        assert!(alter_database(Engine::Mysql, &mysql(Some("utf8; DROP"), None)).is_err());
        // MySQL cannot rename a database, and has no owner.
        let rename = AlterDatabase {
            name: "shop".into(),
            new_name: Some("store".into()),
            ..Default::default()
        };
        assert!(alter_database(Engine::Mysql, &rename).is_err());

        let both = AlterDatabase {
            name: "shop".into(),
            new_name: Some("ร้าน \"ใหม่\"".into()),
            owner: Some("app".into()),
            ..Default::default()
        };
        assert_eq!(
            alter_database(Engine::Postgres, &both).unwrap(),
            vec![
                "ALTER DATABASE \"shop\" OWNER TO \"app\"",
                "ALTER DATABASE \"shop\" RENAME TO \"ร้าน \"\"ใหม่\"\"\"",
            ]
        );
        assert_eq!(
            alter_database(Engine::Postgres, &rename).unwrap(),
            vec!["ALTER DATABASE \"shop\" RENAME TO \"store\""]
        );
        // The same name again is not a change.
        let same = AlterDatabase {
            name: "shop".into(),
            new_name: Some("shop".into()),
            ..Default::default()
        };
        assert!(alter_database(Engine::Postgres, &same).is_err());
    }

    #[test]
    fn flush_is_a_fixed_mysql_statement() {
        assert_eq!(
            flush(Engine::Mysql, Flush::Privileges).unwrap(),
            "FLUSH PRIVILEGES"
        );
        assert_eq!(flush(Engine::Mysql, Flush::Tables).unwrap(), "FLUSH TABLES");
        assert!(flush(Engine::Postgres, Flush::Logs).is_err());
        let parsed: Flush = serde_json::from_str("\"status\"").unwrap();
        assert_eq!(parsed, Flush::Status);
        assert!(serde_json::from_str::<Flush>("\"hosts; DROP DATABASE x\"").is_err());
    }

    fn at(database: &str, schema: Option<&str>, name: &str) -> TableRef {
        TableRef {
            database: database.into(),
            schema: schema.map(Into::into),
            name: name.into(),
        }
    }

    fn rename(table: TableRef, kind: ObjectKind, new_name: &str) -> TableChange {
        TableChange::Rename {
            table,
            kind,
            new_name: new_name.into(),
        }
    }

    #[test]
    fn table_statements_per_engine() {
        let my = at("shop", None, "orders");
        let pg = at("app", Some("shop"), "orders");
        let lite = at("main", None, "orders");
        let all = [
            (Engine::Mysql, &my),
            (Engine::Postgres, &pg),
            (Engine::Sqlite, &lite),
        ];
        let run = |engine, change: TableChange| table_change(engine, &change).unwrap();

        // rename: the new name is trimmed
        let renamed = |engine, table: &TableRef, kind| {
            run(engine, rename(table.clone(), kind, "  orders_2026 "))
        };
        assert_eq!(
            renamed(Engine::Mysql, &my, ObjectKind::Table),
            "RENAME TABLE `shop`.`orders` TO `shop`.`orders_2026`"
        );
        assert_eq!(
            renamed(Engine::Mysql, &my, ObjectKind::View),
            "RENAME TABLE `shop`.`orders` TO `shop`.`orders_2026`"
        );
        assert_eq!(
            renamed(Engine::Postgres, &pg, ObjectKind::Table),
            r#"ALTER TABLE "shop"."orders" RENAME TO "orders_2026""#
        );
        assert_eq!(
            renamed(Engine::Postgres, &pg, ObjectKind::View),
            r#"ALTER VIEW "shop"."orders" RENAME TO "orders_2026""#
        );
        assert_eq!(
            renamed(Engine::Postgres, &pg, ObjectKind::Matview),
            r#"ALTER MATERIALIZED VIEW "shop"."orders" RENAME TO "orders_2026""#
        );
        assert_eq!(
            renamed(Engine::Sqlite, &lite, ObjectKind::Table),
            r#"ALTER TABLE "orders" RENAME TO "orders_2026""#
        );

        // empty
        let emptied: Vec<String> = all
            .iter()
            .map(|(engine, table)| {
                run(
                    *engine,
                    TableChange::Empty {
                        table: (*table).clone(),
                    },
                )
            })
            .collect();
        assert_eq!(
            emptied,
            [
                "TRUNCATE TABLE `shop`.`orders`",
                r#"TRUNCATE TABLE "shop"."orders""#,
                r#"DELETE FROM "orders""#
            ]
        );

        // drop
        let dropped = |engine, table: &TableRef, kind| {
            run(
                engine,
                TableChange::Drop {
                    table: table.clone(),
                    kind,
                },
            )
        };
        assert_eq!(
            dropped(Engine::Mysql, &my, ObjectKind::Table),
            "DROP TABLE `shop`.`orders`"
        );
        assert_eq!(
            dropped(Engine::Mysql, &my, ObjectKind::View),
            "DROP VIEW `shop`.`orders`"
        );
        assert_eq!(
            dropped(Engine::Postgres, &pg, ObjectKind::Matview),
            r#"DROP MATERIALIZED VIEW "shop"."orders""#
        );
        assert_eq!(
            dropped(Engine::Sqlite, &lite, ObjectKind::View),
            r#"DROP VIEW "orders""#
        );
    }

    #[test]
    fn nothing_typed_can_leave_a_table_statement() {
        // A table that already has a hostile name, and a hostile new name for it.
        let hostile = "x`\"; DROP TABLE users; --";
        assert_eq!(
            table_change(
                Engine::Mysql,
                &rename(at("a`b", None, hostile), ObjectKind::Table, hostile.trim_end_matches('-'))
            )
            .unwrap(),
            "RENAME TABLE `a``b`.`x``\"; DROP TABLE users; --` TO `a``b`.`x``\"; DROP TABLE users;`"
        );
        assert_eq!(
            table_change(
                Engine::Postgres,
                &TableChange::Drop {
                    table: at("app", Some("s\"x"), hostile),
                    kind: ObjectKind::Table
                }
            )
            .unwrap(),
            r#"DROP TABLE "s""x"."x`""; DROP TABLE users; --""#
        );
        assert_eq!(
            table_change(
                Engine::Sqlite,
                &TableChange::Empty {
                    table: at("main", None, hostile)
                }
            )
            .unwrap(),
            r#"DELETE FROM "x`""; DROP TABLE users; --""#
        );
    }

    #[test]
    fn refuses_table_changes_that_cannot_be() {
        let my = at("shop", None, "orders");
        let pg = at("app", Some("shop"), "orders");
        let invalid = |engine, change: TableChange| {
            matches!(table_change(engine, &change), Err(Error::Invalid(_)))
        };
        let unsupported = |engine, change: TableChange| {
            matches!(table_change(engine, &change), Err(Error::Unsupported(_)))
        };

        // a new name that is empty, unchanged, too long, or holds a NUL
        for (engine, table) in [(Engine::Mysql, &my), (Engine::Postgres, &pg)] {
            assert!(invalid(
                engine,
                rename(table.clone(), ObjectKind::Table, "   ")
            ));
            assert!(invalid(
                engine,
                rename(table.clone(), ObjectKind::Table, " orders ")
            ));
            assert!(invalid(
                engine,
                rename(table.clone(), ObjectKind::Table, "a\0b")
            ));
        }
        assert!(invalid(
            Engine::Mysql,
            rename(my.clone(), ObjectKind::Table, &"n".repeat(65))
        ));
        assert!(table_change(
            Engine::Mysql,
            &rename(my.clone(), ObjectKind::Table, &"ก".repeat(64))
        )
        .is_ok());
        // PostgreSQL would shorten a longer name without saying so.
        assert!(invalid(
            Engine::Postgres,
            rename(pg.clone(), ObjectKind::Table, &"n".repeat(64))
        ));
        assert!(table_change(
            Engine::Postgres,
            &rename(pg.clone(), ObjectKind::Table, &"ก".repeat(21))
        )
        .is_ok());

        // PostgreSQL needs the schema
        assert!(invalid(
            Engine::Postgres,
            TableChange::Empty {
                table: at("app", None, "orders")
            }
        ));
        // what an engine does not have
        assert!(unsupported(
            Engine::Mysql,
            TableChange::Drop {
                table: my.clone(),
                kind: ObjectKind::Matview
            }
        ));
        assert!(unsupported(
            Engine::Sqlite,
            rename(at("main", None, "v"), ObjectKind::View, "w")
        ));
        assert!(unsupported(
            Engine::Sqlserver,
            TableChange::Empty { table: my }
        ));
    }
}
