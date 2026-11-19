//! Schema changes made through a form rather than typed SQL.
//!
//! One statement per form, built here and nowhere else: the text the user reads in the
//! form's preview is the text that runs. Names go through `ident`; the few values that SQL
//! does not let us quote as identifiers (character sets, collations, encodings) must match
//! a strict pattern before they reach the statement.

use serde::{Deserialize, Serialize};

use super::ident;
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

/// MySQL allows 64 characters, PostgreSQL 63 bytes; the server has the last word.
const MAX_NAME_CHARS: usize = 64;

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

/// The `CREATE DATABASE` statement for `spec`.
pub fn create_database(engine: Engine, spec: &NewDatabase) -> Result<String> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("The database needs a name".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::Invalid(format!(
            "A database name can be at most {MAX_NAME_CHARS} characters long"
        )));
    }
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
        Engine::Sqlite => Err(Error::Unsupported(
            "A SQLite connection is one database file; there is no database to create inside it"
                .into(),
        )),
        Engine::Sqlserver => Err(Error::Unsupported(
            "SQL Server connections are not implemented yet".into(),
        )),
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
            assert!(create_database(engine, &named(&"n".repeat(65))).is_err());
            assert!(create_database(engine, &named(&"ก".repeat(64))).is_ok());
            assert!(create_database(engine, &named("a\0b")).is_err());
        }
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
}
