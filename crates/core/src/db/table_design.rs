//! A new table from the table form ([ADR 0025]).
//!
//! The rules of [`super::ddl`] hold here too: one builder for the preview and the run, names
//! through [`super::ident`], and typed text nowhere else but in a string literal (a default
//! value, a comment) that refuses what could change how it reads. A column's type is chosen
//! from a fixed list per engine, with numbers for its length or precision: no typed type.
//!
//! [ADR 0025]: ../../../../docs/adr/0025-new-table-form.md

use serde::{Deserialize, Serialize};

use super::{ident, TableRef};
use crate::model::Engine;
use crate::{Error, Result};

/// What a type takes in parentheses.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Params {
    None,
    /// `VARCHAR(n)`: a length, with the form's first offer.
    Length {
        required: bool,
        default: u32,
        max: u32,
    },
    /// `DECIMAL(p, s)`.
    Precision {
        default_precision: u32,
        default_scale: u32,
        max: u32,
    },
}

/// What a type is for, which decides which column options make sense.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Family {
    Integer,
    Number,
    Text,
    Time,
    Other,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TypeChoice {
    /// As it is written in the statement.
    pub name: &'static str,
    pub params: Params,
    pub family: Family,
    /// `DEFAULT CURRENT_TIMESTAMP` is allowed.
    pub timestamp: bool,
}

const fn t(name: &'static str, params: Params, family: Family, timestamp: bool) -> TypeChoice {
    TypeChoice {
        name,
        params,
        family,
        timestamp,
    }
}

const NONE: Params = Params::None;
const fn len(required: bool, default: u32, max: u32) -> Params {
    Params::Length {
        required,
        default,
        max,
    }
}
const DECIMAL: Params = Params::Precision {
    default_precision: 12,
    default_scale: 2,
    max: 65,
};
const NUMERIC: Params = Params::Precision {
    default_precision: 12,
    default_scale: 2,
    max: 1000,
};

/// The types the form offers, most used first.
pub fn types(engine: Engine) -> Result<Vec<TypeChoice>> {
    use Family::*;
    Ok(match engine {
        Engine::Mysql => vec![
            t("INT", NONE, Integer, false),
            t("BIGINT", NONE, Integer, false),
            t("VARCHAR", len(true, 255, 16383), Text, false),
            t("TEXT", NONE, Text, false),
            t("DECIMAL", DECIMAL, Number, false),
            t("DATETIME", NONE, Time, true),
            t("DATE", NONE, Time, false),
            t("TIMESTAMP", NONE, Time, true),
            t("TINYINT", NONE, Integer, false),
            t("SMALLINT", NONE, Integer, false),
            t("MEDIUMINT", NONE, Integer, false),
            t("BOOLEAN", NONE, Other, false),
            t("CHAR", len(true, 1, 255), Text, false),
            t("MEDIUMTEXT", NONE, Text, false),
            t("LONGTEXT", NONE, Text, false),
            t("DOUBLE", NONE, Number, false),
            t("FLOAT", NONE, Number, false),
            t("TIME", NONE, Time, false),
            t("YEAR", NONE, Time, false),
            t("JSON", NONE, Other, false),
            t("BLOB", NONE, Other, false),
            t("LONGBLOB", NONE, Other, false),
            t("VARBINARY", len(true, 255, 65535), Other, false),
        ],
        Engine::Postgres => vec![
            t("integer", NONE, Integer, false),
            t("bigint", NONE, Integer, false),
            t("text", NONE, Text, false),
            t("varchar", len(false, 255, 10_485_760), Text, false),
            t("numeric", NUMERIC, Number, false),
            t("timestamptz", NONE, Time, true),
            t("timestamp", NONE, Time, true),
            t("date", NONE, Time, false),
            t("boolean", NONE, Other, false),
            t("smallint", NONE, Integer, false),
            t("char", len(true, 1, 10_485_760), Text, false),
            t("double precision", NONE, Number, false),
            t("real", NONE, Number, false),
            t("time", NONE, Time, false),
            t("uuid", NONE, Other, false),
            t("jsonb", NONE, Other, false),
            t("json", NONE, Other, false),
            t("bytea", NONE, Other, false),
        ],
        _ => return Err(unsupported(engine)),
    })
}

/// A column's default.
#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum DefaultValue {
    #[default]
    None,
    Null,
    /// A value, written as a string literal: the server converts it to the column's type.
    Value(String),
    CurrentTimestamp,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    /// One of [`types`]' names.
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub scale: Option<u32>,
    /// MySQL's `UNSIGNED`.
    #[serde(default)]
    pub unsigned: bool,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default)]
    pub default: DefaultValue,
    /// MySQL's `AUTO_INCREMENT`, PostgreSQL's identity.
    #[serde(default)]
    pub auto: bool,
    /// Part of the primary key, in the order of the columns.
    #[serde(default)]
    pub key: bool,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewTable {
    /// Where, and the table's name.
    pub table: TableRef,
    pub columns: Vec<ColumnSpec>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// The longest default value or comment the form writes.
const MAX_LITERAL_CHARS: usize = 1024;

/// Text as a string literal. NUL and backslash are refused: without them the literal reads
/// the same whatever MySQL's `sql_mode` or PostgreSQL's `standard_conforming_strings`.
fn literal(what: &str, value: &str) -> Result<String> {
    if value.contains('\0') {
        return Err(Error::Invalid(format!("The {what} contains a NUL byte")));
    }
    if value.contains('\\') {
        return Err(Error::Invalid(format!(
            "The {what} contains a backslash, which the table form does not write; use a query tab for it"
        )));
    }
    let n = value.chars().count();
    if n > MAX_LITERAL_CHARS {
        return Err(Error::Invalid(format!(
            "The {what} can be at most {MAX_LITERAL_CHARS} characters long; this one is {n}"
        )));
    }
    Ok(format!("'{}'", value.replace('\'', "''")))
}

fn quote(engine: Engine, name: &str) -> Result<String> {
    match engine {
        Engine::Mysql => ident::mysql(name),
        _ => ident::postgres(name),
    }
}

fn check_name(engine: Engine, what: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Invalid(format!("The {what} needs a name")));
    }
    let (length, most, unit) = match engine {
        Engine::Postgres => (name.len(), 63, "bytes"),
        _ => (name.chars().count(), 64, "characters"),
    };
    if length > most {
        return Err(Error::Invalid(format!(
            "A {what} name can be at most {most} {unit} long; “{name}” is {length}"
        )));
    }
    Ok(())
}

fn unsupported(engine: Engine) -> Error {
    Error::Unsupported(format!(
        "The table form is for MySQL and PostgreSQL; this is a {} connection",
        match engine {
            Engine::Sqlite => "SQLite",
            _ => "SQL Server",
        }
    ))
}

/// A column type from the list, with what it takes in parentheses, and MySQL's `UNSIGNED`.
fn type_sql<'a>(
    engine: Engine,
    choices: &'a [TypeChoice],
    type_name: &str,
    length: Option<u32>,
    scale: Option<u32>,
    unsigned: bool,
    name: &str,
) -> Result<(String, &'a TypeChoice)> {
    let choice = choices
        .iter()
        .find(|t| t.name == type_name)
        .ok_or_else(|| Error::Invalid(format!("“{type_name}” is not a type the form offers")))?;
    let mut sql = choice.name.to_string();
    match choice.params {
        Params::None => {
            if length.is_some() || scale.is_some() {
                return Err(Error::Invalid(format!("{} takes no length", choice.name)));
            }
        }
        Params::Length { required, max, .. } => match length {
            Some(n) if n == 0 || n > max => {
                return Err(Error::Invalid(format!(
                    "The length of {} is from 1 to {max}; {n} is not",
                    choice.name
                )))
            }
            Some(n) => sql.push_str(&format!("({n})")),
            None if required => {
                return Err(Error::Invalid(format!(
                    "{} needs a length (column “{name}”)",
                    choice.name
                )))
            }
            None => {}
        },
        Params::Precision { max, .. } => {
            let p = length.ok_or_else(|| {
                Error::Invalid(format!(
                    "{} needs a precision (column “{name}”)",
                    choice.name
                ))
            })?;
            let s = scale.unwrap_or(0);
            if p == 0 || p > max || s > p {
                return Err(Error::Invalid(format!(
                    "{}({p}, {s}) is not possible: the precision is from 1 to {max}, and the scale at most the precision",
                    choice.name
                )));
            }
            sql.push_str(&format!("({p}, {s})"));
        }
    }
    if unsigned {
        if engine != Engine::Mysql || !matches!(choice.family, Family::Integer | Family::Number) {
            return Err(Error::Invalid(format!(
                "Only a MySQL number can be unsigned (column “{name}”)"
            )));
        }
        sql.push_str(" UNSIGNED");
    }
    Ok((sql, choice))
}

/// One column's definition, after the name.
fn column_sql(
    engine: Engine,
    choices: &[TypeChoice],
    c: &ColumnSpec,
    name: &str,
) -> Result<String> {
    let (mut sql, choice) = type_sql(
        engine,
        choices,
        &c.type_name,
        c.length,
        c.scale,
        c.unsigned,
        name,
    )?;
    if c.auto {
        if choice.family != Family::Integer {
            return Err(Error::Invalid(format!(
                "Only an integer column can number itself (column “{name}”)"
            )));
        }
        if engine == Engine::Postgres {
            sql.push_str(" GENERATED BY DEFAULT AS IDENTITY");
        }
    }
    // A key column, and an identity, are NOT NULL whatever the form says.
    sql.push_str(if c.nullable && !c.key && !c.auto {
        " NULL"
    } else {
        " NOT NULL"
    });
    match &c.default {
        DefaultValue::None => {}
        _ if c.auto => {
            return Err(Error::Invalid(format!(
                "A column that numbers itself has no default (column “{name}”)"
            )))
        }
        DefaultValue::Null => {
            if !c.nullable || c.key {
                return Err(Error::Invalid(format!(
                    "NULL is not a default of a NOT NULL column (column “{name}”)"
                )));
            }
            sql.push_str(" DEFAULT NULL");
        }
        DefaultValue::Value(v) => {
            sql.push_str(&format!(" DEFAULT {}", literal("default value", v)?))
        }
        DefaultValue::CurrentTimestamp => {
            if !choice.timestamp {
                return Err(Error::Invalid(format!(
                    "CURRENT_TIMESTAMP is a default of a date-and-time column, not of {} (column “{name}”)",
                    choice.name
                )));
            }
            sql.push_str(" DEFAULT CURRENT_TIMESTAMP");
        }
    }
    if c.auto && engine == Engine::Mysql {
        sql.push_str(" AUTO_INCREMENT");
    }
    if engine == Engine::Mysql {
        if let Some(comment) = clean(&c.comment) {
            sql.push_str(&format!(" COMMENT {}", literal("column comment", comment)?));
        }
    }
    Ok(sql)
}

fn clean(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// The statements that create the table, in the order they run: one on MySQL; on PostgreSQL
/// the `CREATE TABLE` and then a `COMMENT ON` for each comment, run together in one
/// transaction.
pub fn create_table(engine: Engine, spec: &NewTable) -> Result<Vec<String>> {
    let choices = types(engine)?;
    let name = spec.table.name.trim();
    check_name(engine, "table", name)?;
    let qualified = match engine {
        Engine::Mysql => format!(
            "{}.{}",
            ident::mysql(&spec.table.database)?,
            ident::mysql(name)?
        ),
        _ => format!(
            "{}.{}",
            ident::postgres(super::require_schema(spec.table.schema.as_deref())?)?,
            ident::postgres(name)?
        ),
    };
    if spec.columns.is_empty() {
        return Err(Error::Invalid("The table needs at least one column".into()));
    }
    let mut seen = std::collections::HashSet::new();
    let mut lines = Vec::new();
    let mut keys = Vec::new();
    let mut comments = Vec::new();
    for c in &spec.columns {
        let column = c.name.trim();
        check_name(engine, "column", column)?;
        // MySQL compares column names without case; PostgreSQL as written.
        let folded = if engine == Engine::Mysql {
            column.to_lowercase()
        } else {
            column.to_string()
        };
        if !seen.insert(folded) {
            return Err(Error::Invalid(format!("Two columns are named “{column}”")));
        }
        lines.push(format!(
            "  {} {}",
            quote(engine, column)?,
            column_sql(engine, &choices, c, column)?
        ));
        if c.key {
            keys.push(quote(engine, column)?);
        }
        if engine == Engine::Postgres {
            if let Some(comment) = clean(&c.comment) {
                comments.push(format!(
                    "COMMENT ON COLUMN {qualified}.{} IS {}",
                    quote(engine, column)?,
                    literal("column comment", comment)?
                ));
            }
        }
    }
    let autos: Vec<&ColumnSpec> = spec.columns.iter().filter(|c| c.auto).collect();
    if autos.len() > 1 {
        return Err(Error::Invalid("Only one column can number itself".into()));
    }
    if engine == Engine::Mysql {
        if let Some(auto) = autos.first() {
            // MySQL wants the self-numbering column first in a key.
            if !auto.key || keys.first() != Some(&quote(engine, auto.name.trim())?) {
                return Err(Error::Invalid(format!(
                    "On MySQL the column that numbers itself must be the (first column of the) primary key: “{}”",
                    auto.name.trim()
                )));
            }
        }
    }
    if !keys.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", keys.join(", ")));
    }
    let mut sql = format!("CREATE TABLE {qualified} (\n{}\n)", lines.join(",\n"));
    let mut statements = Vec::new();
    match engine {
        Engine::Mysql => {
            if let Some(comment) = clean(&spec.comment) {
                sql.push_str(&format!(
                    " COMMENT = {}",
                    literal("table comment", comment)?
                ));
            }
            statements.push(sql);
        }
        _ => {
            statements.push(sql);
            if let Some(comment) = clean(&spec.comment) {
                statements.push(format!(
                    "COMMENT ON TABLE {qualified} IS {}",
                    literal("table comment", comment)?
                ));
            }
            statements.extend(comments);
        }
    }
    Ok(statements)
}

// ---- changing a table ---------------------------------------------------------------------

/// An index of a table, as the server lists it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub name: String,
    /// The columns in index order; an expression as the server prints it.
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

/// A column as the server keeps it: everything a MySQL `MODIFY COLUMN` must say again so
/// that what is not being changed stays as it was.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColumnDefinition {
    pub name: String,
    /// MySQL's `COLUMN_TYPE` (`int unsigned`, `varchar(120)`); PostgreSQL's `format_type`.
    pub column_type: String,
    pub nullable: bool,
    /// As the server shows it; `None` when there is none.
    pub default: Option<String>,
    /// The default is an expression (MySQL `DEFAULT_GENERATED`; on PostgreSQL always).
    pub default_expression: bool,
    /// MySQL's `EXTRA`.
    pub extra: String,
    pub comment: Option<String>,
    pub charset: Option<String>,
    pub collation: Option<String>,
    /// A generated or (PostgreSQL) identity column.
    pub generated: bool,
}

/// What happens to referencing rows when the referenced row goes or changes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum FkAction {
    #[default]
    NoAction,
    Restrict,
    Cascade,
    SetNull,
}

impl FkAction {
    fn sql(self) -> &'static str {
        match self {
            FkAction::NoAction => "NO ACTION",
            FkAction::Restrict => "RESTRICT",
            FkAction::Cascade => "CASCADE",
            FkAction::SetNull => "SET NULL",
        }
    }

    /// From MySQL's `DELETE_RULE` text or PostgreSQL's one-letter code.
    pub fn parse(text: &str) -> Option<FkAction> {
        match text.trim().to_ascii_uppercase().as_str() {
            "NO ACTION" | "A" => Some(FkAction::NoAction),
            "RESTRICT" | "R" => Some(FkAction::Restrict),
            "CASCADE" | "C" => Some(FkAction::Cascade),
            "SET NULL" | "N" => Some(FkAction::SetNull),
            _ => None,
        }
    }
}

/// A foreign key of a table, as the server lists it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub references: TableRef,
    pub ref_columns: Vec<String>,
    /// `None`: an action the form does not offer (SET DEFAULT).
    pub on_delete: Option<FkAction>,
    pub on_update: Option<FkAction>,
}

/// A column type chosen in the form for a column that changes.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewType {
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub scale: Option<u32>,
    #[serde(default)]
    pub unsigned: bool,
}

/// One change of the table form's "Edit table…".
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TableEdit {
    AddColumn {
        column: ColumnSpec,
    },
    DropColumn {
        name: String,
    },
    RenameColumn {
        from: String,
        to: String,
    },
    AddIndex {
        #[serde(default)]
        name: Option<String>,
        columns: Vec<String>,
        #[serde(default)]
        unique: bool,
    },
    DropIndex {
        name: String,
    },
    /// What is `None` stays as it is.
    ChangeColumn {
        name: String,
        #[serde(default, rename = "type")]
        new_type: Option<NewType>,
        #[serde(default)]
        nullable: Option<bool>,
        /// `Some(DefaultValue::None)` removes the default.
        #[serde(default)]
        default: Option<DefaultValue>,
        /// `Some("")` removes the comment.
        #[serde(default)]
        comment: Option<String>,
    },
    AddForeignKey {
        #[serde(default)]
        name: Option<String>,
        columns: Vec<String>,
        references: TableRef,
        ref_columns: Vec<String>,
        #[serde(default)]
        on_delete: FkAction,
        #[serde(default)]
        on_update: FkAction,
    },
    DropForeignKey {
        name: String,
    },
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlterTable {
    pub table: TableRef,
    pub edits: Vec<TableEdit>,
}

impl AlterTable {
    /// The columns this change deletes, with their data.
    pub fn dropped_columns(&self) -> Vec<&str> {
        self.edits
            .iter()
            .filter_map(|e| match e {
                TableEdit::DropColumn { name } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// The statements that make the changes, in the order they run. MySQL: one `ALTER TABLE`
/// with a clause per change, which the server applies as one. PostgreSQL: one statement per
/// change (and a `COMMENT ON` for an added column's comment), run together in one
/// transaction. `current` is what the server says of the table's columns now: a MySQL
/// change of a column says again what is not being changed.
pub fn alter_table(
    engine: Engine,
    spec: &AlterTable,
    current: &[ColumnDefinition],
) -> Result<Vec<String>> {
    let choices = types(engine)?;
    if spec.edits.is_empty() {
        return Err(Error::Invalid("Nothing is changed yet".into()));
    }
    let table = &spec.table;
    let qualified = match engine {
        Engine::Mysql => format!(
            "{}.{}",
            ident::mysql(&table.database)?,
            ident::mysql(&table.name)?
        ),
        _ => format!(
            "{}.{}",
            ident::postgres(super::require_schema(table.schema.as_deref())?)?,
            ident::postgres(&table.name)?
        ),
    };
    let mut clauses = Vec::new(); // MySQL
    let mut statements = Vec::new(); // PostgreSQL
    let mut after = Vec::new(); // PostgreSQL: comments, after the columns exist
    let mut touched = std::collections::HashSet::new();
    let mut mark = |name: &str| -> Result<()> {
        let key = if engine == Engine::Mysql {
            name.to_lowercase()
        } else {
            name.to_string()
        };
        if touched.insert(key) {
            Ok(())
        } else {
            Err(Error::Invalid(format!(
                "“{name}” is changed twice; make one change per column"
            )))
        }
    };
    for edit in &spec.edits {
        match edit {
            TableEdit::AddColumn { column } => {
                let name = column.name.trim();
                check_name(engine, "column", name)?;
                mark(name)?;
                if column.key || column.auto {
                    return Err(Error::Invalid(format!(
                        "A column added to an existing table cannot be part of the primary key or number itself here (column “{name}”); use a query tab for that"
                    )));
                }
                let definition = format!(
                    "{} {}",
                    quote(engine, name)?,
                    column_sql(engine, &choices, column, name)?
                );
                match engine {
                    Engine::Mysql => clauses.push(format!("ADD COLUMN {definition}")),
                    _ => {
                        statements.push(format!("ALTER TABLE {qualified} ADD COLUMN {definition}"));
                        if let Some(comment) = clean(&column.comment) {
                            after.push(format!(
                                "COMMENT ON COLUMN {qualified}.{} IS {}",
                                quote(engine, name)?,
                                literal("column comment", comment)?
                            ));
                        }
                    }
                }
            }
            TableEdit::DropColumn { name } => {
                mark(name)?;
                let column = quote(engine, name)?;
                match engine {
                    Engine::Mysql => clauses.push(format!("DROP COLUMN {column}")),
                    _ => statements.push(format!("ALTER TABLE {qualified} DROP COLUMN {column}")),
                }
            }
            TableEdit::RenameColumn { from, to } => {
                let to = to.trim();
                check_name(engine, "column", to)?;
                if to == from {
                    return Err(Error::Invalid(format!("“{from}” already has that name")));
                }
                mark(from)?;
                mark(to)?;
                let clause = format!(
                    "RENAME COLUMN {} TO {}",
                    quote(engine, from)?,
                    quote(engine, to)?
                );
                match engine {
                    Engine::Mysql => clauses.push(clause),
                    _ => statements.push(format!("ALTER TABLE {qualified} {clause}")),
                }
            }
            TableEdit::AddIndex {
                name,
                columns,
                unique,
            } => {
                if columns.is_empty() {
                    return Err(Error::Invalid("An index needs at least one column".into()));
                }
                let mut seen = std::collections::HashSet::new();
                let mut quoted = Vec::new();
                for c in columns {
                    if !seen.insert(c.as_str()) {
                        return Err(Error::Invalid(format!("“{c}” is in the index twice")));
                    }
                    quoted.push(quote(engine, c)?);
                }
                let name = clean(name);
                if let Some(n) = name {
                    check_name(engine, "index", n)?;
                }
                let unique = if *unique { "UNIQUE " } else { "" };
                let named = name
                    .map(|n| quote(engine, n))
                    .transpose()?
                    .map(|n| format!("{n} "))
                    .unwrap_or_default();
                match engine {
                    Engine::Mysql => {
                        clauses.push(format!("ADD {unique}INDEX {named}({})", quoted.join(", ")))
                    }
                    _ => statements.push(format!(
                        "CREATE {unique}INDEX {named}ON {qualified} ({})",
                        quoted.join(", ")
                    )),
                }
            }
            TableEdit::DropIndex { name } => {
                if engine == Engine::Mysql && name.eq_ignore_ascii_case("PRIMARY")
                    || name.is_empty()
                {
                    return Err(Error::Invalid("The primary key is not dropped here".into()));
                }
                match engine {
                    Engine::Mysql => clauses.push(format!("DROP INDEX {}", quote(engine, name)?)),
                    // An index lives in the table's schema.
                    _ => statements.push(format!(
                        "DROP INDEX {}.{}",
                        ident::postgres(super::require_schema(table.schema.as_deref())?)?,
                        ident::postgres(name)?
                    )),
                }
            }
            TableEdit::ChangeColumn {
                name,
                new_type,
                nullable,
                default,
                comment,
            } => {
                mark(name)?;
                let def = current
                    .iter()
                    .find(|c| &c.name == name)
                    .ok_or_else(|| Error::Invalid(format!("The table has no column “{name}”")))?;
                if new_type.is_none()
                    && nullable.is_none()
                    && default.is_none()
                    && comment.is_none()
                {
                    return Err(Error::Invalid(format!("Nothing is changed in “{name}”")));
                }
                match engine {
                    Engine::Mysql => clauses.push(format!(
                        "MODIFY COLUMN {} {}",
                        quote(engine, name)?,
                        mysql_modify(
                            &choices,
                            def,
                            new_type.as_ref(),
                            *nullable,
                            default.as_ref(),
                            comment.as_deref()
                        )?
                    )),
                    _ => {
                        let column = quote(engine, name)?;
                        // PostgreSQL will not convert a default to another type: it goes
                        // first, and the change must say what the default becomes.
                        let default_dropped = new_type.is_some() && def.default.is_some();
                        if default_dropped {
                            if default.is_none() {
                                return Err(Error::Invalid(format!(
                                    "“{name}” has a default ({}); say what its default becomes when its type changes",
                                    def.default.as_deref().unwrap_or_default()
                                )));
                            }
                            statements.push(format!(
                                "ALTER TABLE {qualified} ALTER COLUMN {column} DROP DEFAULT"
                            ));
                        }
                        if let Some(t) = new_type {
                            let (sql, _) = type_sql(
                                engine,
                                &choices,
                                &t.type_name,
                                t.length,
                                t.scale,
                                t.unsigned,
                                name,
                            )?;
                            statements.push(format!("ALTER TABLE {qualified} ALTER COLUMN {column} TYPE {sql} USING {column}::{sql}"));
                        }
                        if let Some(n) = nullable {
                            statements.push(format!(
                                "ALTER TABLE {qualified} ALTER COLUMN {column} {} NOT NULL",
                                if *n { "DROP" } else { "SET" }
                            ));
                        }
                        if let Some(d) = default {
                            if def.generated {
                                return Err(Error::Invalid(format!("“{name}” is generated by the server and has no default to change")));
                            }
                            let set = match d {
                                // Already dropped before the new type.
                                DefaultValue::None if default_dropped => None,
                                DefaultValue::None => Some("DROP DEFAULT".to_string()),
                                DefaultValue::Null => Some("SET DEFAULT NULL".to_string()),
                                DefaultValue::Value(v) => {
                                    Some(format!("SET DEFAULT {}", literal("default value", v)?))
                                }
                                DefaultValue::CurrentTimestamp => {
                                    Some("SET DEFAULT CURRENT_TIMESTAMP".to_string())
                                }
                            };
                            if let Some(set) = set {
                                statements.push(format!(
                                    "ALTER TABLE {qualified} ALTER COLUMN {column} {set}"
                                ));
                            }
                        }
                        if let Some(c) = comment {
                            after.push(format!(
                                "COMMENT ON COLUMN {qualified}.{column} IS {}",
                                if c.trim().is_empty() {
                                    "NULL".to_string()
                                } else {
                                    literal("column comment", c.trim())?
                                }
                            ));
                        }
                    }
                }
            }
            TableEdit::AddForeignKey {
                name,
                columns,
                references,
                ref_columns,
                on_delete,
                on_update,
            } => {
                if columns.is_empty() || columns.len() != ref_columns.len() {
                    return Err(Error::Invalid("A foreign key pairs each of its columns with one column of the other table".into()));
                }
                if engine == Engine::Mysql && references.database != table.database {
                    return Err(Error::Invalid(
                        "The form makes a foreign key to a table of the same database".into(),
                    ));
                }
                let quoted = |list: &[String]| -> Result<String> {
                    Ok(list
                        .iter()
                        .map(|c| quote(engine, c))
                        .collect::<Result<Vec<_>>>()?
                        .join(", "))
                };
                let target = match engine {
                    Engine::Mysql => format!(
                        "{}.{}",
                        ident::mysql(&references.database)?,
                        ident::mysql(&references.name)?
                    ),
                    _ => format!(
                        "{}.{}",
                        ident::postgres(super::require_schema(references.schema.as_deref())?)?,
                        ident::postgres(&references.name)?
                    ),
                };
                let named = match clean(name) {
                    Some(n) => {
                        check_name(engine, "foreign key", n)?;
                        format!("CONSTRAINT {} ", quote(engine, n)?)
                    }
                    None => String::new(),
                };
                let clause = format!(
                    "ADD {named}FOREIGN KEY ({}) REFERENCES {target} ({}) ON DELETE {} ON UPDATE {}",
                    quoted(columns)?,
                    quoted(ref_columns)?,
                    on_delete.sql(),
                    on_update.sql()
                );
                match engine {
                    Engine::Mysql => clauses.push(clause),
                    _ => statements.push(format!("ALTER TABLE {qualified} {clause}")),
                }
            }
            TableEdit::DropForeignKey { name } => match engine {
                Engine::Mysql => clauses.push(format!("DROP FOREIGN KEY {}", quote(engine, name)?)),
                _ => statements.push(format!(
                    "ALTER TABLE {qualified} DROP CONSTRAINT {}",
                    quote(engine, name)?
                )),
            },
        }
    }
    Ok(match engine {
        Engine::Mysql => vec![format!(
            "ALTER TABLE {qualified}\n  {}",
            clauses.join(",\n  ")
        )],
        _ => {
            statements.extend(after);
            statements
        }
    })
}

/// A MySQL `EXTRA` that the form can say again: nothing else is kept on a changed column.
fn mysql_extra(def: &ColumnDefinition) -> Result<(bool, Option<String>)> {
    let mut rest = def.extra.to_string();
    let auto = rest.to_ascii_lowercase().contains("auto_increment");
    let mut on_update = None;
    let lower = rest.to_ascii_lowercase();
    if let Some(at) = lower.find("on update current_timestamp") {
        let tail = &rest[at + "on update ".len()..];
        let end = tail
            .find(')')
            .map(|i| i + 1)
            .filter(|_| tail["current_timestamp".len()..].starts_with('('))
            .unwrap_or("current_timestamp".len());
        on_update = Some(tail[..end].to_ascii_uppercase());
        rest.replace_range(at..at + "on update ".len() + end, "");
    }
    let rest = rest
        .split_whitespace()
        .filter(|w| {
            !w.eq_ignore_ascii_case("auto_increment")
                && !w.eq_ignore_ascii_case("DEFAULT_GENERATED")
        })
        .collect::<Vec<_>>();
    if !rest.is_empty() || def.generated {
        return Err(Error::Invalid(format!(
            "“{}” has {} which the form cannot keep; change it in a query tab",
            def.name,
            if def.generated {
                "a generated value".to_string()
            } else {
                format!("“{}”", rest.join(" "))
            }
        )));
    }
    if let Some(u) = &on_update {
        let ok = u == "CURRENT_TIMESTAMP"
            || (u.starts_with("CURRENT_TIMESTAMP(")
                && u[18..u.len() - 1].bytes().all(|b| b.is_ascii_digit())
                && u.ends_with(')'));
        if !ok {
            return Err(Error::Invalid(format!(
                "“{}” has an ON UPDATE the form does not read",
                def.name
            )));
        }
    }
    Ok((auto, on_update))
}

/// MySQL's `MODIFY COLUMN` rewrites a column whole: what is not changed is said again from
/// what the server reported, or the change is refused.
fn mysql_modify(
    choices: &[TypeChoice],
    def: &ColumnDefinition,
    new_type: Option<&NewType>,
    nullable: Option<bool>,
    default: Option<&DefaultValue>,
    comment: Option<&str>,
) -> Result<String> {
    let name = def.name.as_str();
    let (auto, on_update) = mysql_extra(def)?;
    let (mut sql, text, timestamp) = match new_type {
        Some(t) => {
            let (sql, choice) = type_sql(
                Engine::Mysql,
                choices,
                &t.type_name,
                t.length,
                t.scale,
                t.unsigned,
                name,
            )?;
            if auto && choice.family != Family::Integer {
                return Err(Error::Invalid(format!(
                    "“{name}” numbers itself, so it stays an integer"
                )));
            }
            (sql, choice.family == Family::Text, choice.timestamp)
        }
        None => {
            // The server's own text for the type: kept as it is.
            if def.column_type.contains(['\0', ';']) || def.column_type.trim().is_empty() {
                return Err(Error::Invalid(format!(
                    "The type of “{name}” cannot be said again; change it in a query tab"
                )));
            }
            let lower = def.column_type.to_ascii_lowercase();
            (
                def.column_type.clone(),
                def.charset.is_some(),
                lower.starts_with("datetime") || lower.starts_with("timestamp"),
            )
        }
    };
    if text {
        if let (Some(cs), Some(co)) = (&def.charset, &def.collation) {
            sql.push_str(&format!(
                " CHARACTER SET {} COLLATE {}",
                charset_word(cs)?,
                charset_word(co)?
            ));
        }
    }
    let nullable = nullable.unwrap_or(def.nullable);
    sql.push_str(if nullable { " NULL" } else { " NOT NULL" });
    match default {
        Some(DefaultValue::None) => {}
        Some(_) if auto => {
            return Err(Error::Invalid(format!(
                "“{name}” numbers itself and has no default"
            )))
        }
        Some(DefaultValue::Null) => {
            if !nullable {
                return Err(Error::Invalid(format!(
                    "NULL is not a default of a NOT NULL column (column “{name}”)"
                )));
            }
            sql.push_str(" DEFAULT NULL");
        }
        Some(DefaultValue::Value(v)) => {
            sql.push_str(&format!(" DEFAULT {}", literal("default value", v)?))
        }
        Some(DefaultValue::CurrentTimestamp) => {
            if !timestamp {
                return Err(Error::Invalid(format!(
                    "CURRENT_TIMESTAMP is a default of a date-and-time column (column “{name}”)"
                )));
            }
            sql.push_str(" DEFAULT CURRENT_TIMESTAMP");
        }
        // Kept: as the server shows it.
        None => match &def.default {
            None => {}
            Some(d) if def.default_expression => {
                let upper = d.trim().to_ascii_uppercase();
                let timestamp_now = upper == "CURRENT_TIMESTAMP"
                    || (upper.starts_with("CURRENT_TIMESTAMP(")
                        && upper.ends_with(')')
                        && upper[18..upper.len() - 1]
                            .bytes()
                            .all(|b| b.is_ascii_digit()));
                if timestamp_now {
                    sql.push_str(&format!(" DEFAULT {upper}"));
                } else {
                    return Err(Error::Invalid(format!(
                        "The default of “{name}” is an expression the form does not repeat ({d}); change the column in a query tab"
                    )));
                }
            }
            Some(d) => sql.push_str(&format!(" DEFAULT {}", literal("current default", d)?)),
        },
    }
    if auto {
        sql.push_str(" AUTO_INCREMENT");
    }
    if let Some(u) = on_update {
        sql.push_str(&format!(" ON UPDATE {u}"));
    }
    let comment = match comment {
        Some(c) => clean(&Some(c.to_string())).map(str::to_string),
        None => def.comment.clone().filter(|c| !c.is_empty()),
    };
    if let Some(c) = comment {
        sql.push_str(&format!(" COMMENT {}", literal("column comment", &c)?));
    }
    Ok(sql)
}

/// A character set or collation name, as the server reported it.
fn charset_word(value: &str) -> Result<&str> {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        Ok(value)
    } else {
        Err(Error::Invalid(format!(
            "“{value}” is not a character set or collation the form writes"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(engine: Engine, columns: serde_json::Value, comment: Option<&str>) -> NewTable {
        let schema = (engine == Engine::Postgres).then_some("public");
        serde_json::from_value(json!({
            "table": { "database": "shop", "schema": schema, "name": "orders" },
            "columns": columns,
            "comment": comment,
        }))
        .unwrap()
    }

    #[test]
    fn the_type_list_reads_as_the_form_expects() {
        let decimal = types(Engine::Mysql)
            .unwrap()
            .into_iter()
            .find(|t| t.name == "DECIMAL")
            .unwrap();
        assert_eq!(
            serde_json::to_value(decimal).unwrap(),
            json!({ "name": "DECIMAL", "params": { "kind": "precision", "defaultPrecision": 12, "defaultScale": 2, "max": 65 }, "family": "number", "timestamp": false })
        );
    }

    #[test]
    fn mysql_table() {
        let s = spec(
            Engine::Mysql,
            json!([
                { "name": "id", "type": "INT", "unsigned": true, "auto": true, "key": true },
                { "name": "order_no", "type": "VARCHAR", "length": 20, "comment": "เลขที่ใบสั่ง" },
                { "name": "total", "type": "DECIMAL", "length": 12, "scale": 2, "default": { "kind": "value", "value": "0" } },
                { "name": "note", "type": "TEXT", "nullable": true, "default": { "kind": "null" } },
                { "name": "placed", "type": "DATETIME", "default": { "kind": "currentTimestamp" } },
                { "name": "who's", "type": "VARCHAR", "length": 50, "nullable": true, "default": { "kind": "value", "value": "it's" } }
            ]),
            Some("ใบสั่งซื้อ"),
        );
        assert_eq!(
            create_table(Engine::Mysql, &s).unwrap(),
            ["CREATE TABLE `shop`.`orders` (\n  `id` INT UNSIGNED NOT NULL AUTO_INCREMENT,\n  `order_no` VARCHAR(20) NOT NULL COMMENT 'เลขที่ใบสั่ง',\n  `total` DECIMAL(12, 2) NOT NULL DEFAULT '0',\n  `note` TEXT NULL DEFAULT NULL,\n  `placed` DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,\n  `who's` VARCHAR(50) NULL DEFAULT 'it''s',\n  PRIMARY KEY (`id`)\n) COMMENT = 'ใบสั่งซื้อ'"]
        );
    }

    #[test]
    fn postgres_table() {
        let s = spec(
            Engine::Postgres,
            json!([
                { "name": "id", "type": "bigint", "auto": true, "key": true },
                { "name": "Order No", "type": "varchar", "comment": "x" },
                { "name": "amount", "type": "numeric", "length": 10, "scale": 0 },
                { "name": "at", "type": "timestamptz", "default": { "kind": "currentTimestamp" } }
            ]),
            Some("t"),
        );
        assert_eq!(
            create_table(Engine::Postgres, &s).unwrap(),
            [
                "CREATE TABLE \"public\".\"orders\" (\n  \"id\" bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL,\n  \"Order No\" varchar NOT NULL,\n  \"amount\" numeric(10, 0) NOT NULL,\n  \"at\" timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,\n  PRIMARY KEY (\"id\")\n)",
                "COMMENT ON TABLE \"public\".\"orders\" IS 't'",
                "COMMENT ON COLUMN \"public\".\"orders\".\"Order No\" IS 'x'",
            ]
        );
    }

    #[test]
    fn what_the_form_cannot_write_is_refused() {
        let one = |engine: Engine, column: serde_json::Value| {
            create_table(engine, &spec(engine, json!([column]), None))
                .unwrap_err()
                .to_string()
        };
        // A type that is not offered: typed SQL never becomes a type.
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "INT); DROP TABLE x; --" })
        )
        .contains("not a type"));
        assert!(
            one(Engine::Mysql, json!({ "name": "a", "type": "VARCHAR" }))
                .contains("needs a length")
        );
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "VARCHAR", "length": 0 })
        )
        .contains("from 1 to"));
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "INT", "length": 11 })
        )
        .contains("takes no length"));
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "DECIMAL", "length": 5, "scale": 6 })
        )
        .contains("not possible"));
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "TEXT", "auto": true, "key": true })
        )
        .contains("integer"));
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "INT", "auto": true })
        )
        .contains("primary key"));
        assert!(one(
            Engine::Postgres,
            json!({ "name": "a", "type": "text", "unsigned": true })
        )
        .contains("unsigned"));
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "DATE", "default": { "kind": "currentTimestamp" } })
        )
        .contains("CURRENT_TIMESTAMP"));
        assert!(one(
            Engine::Mysql,
            json!({ "name": "a", "type": "INT", "default": { "kind": "null" } })
        )
        .contains("NOT NULL"));
        assert!(one(Engine::Mysql, json!({ "name": "a", "type": "TEXT", "nullable": true, "default": { "kind": "value", "value": "a\\'b" } })).contains("backslash"));
        assert!(
            one(Engine::Postgres, json!({ "name": "", "type": "text" })).contains("needs a name")
        );
        assert!(one(
            Engine::Postgres,
            json!({ "name": "x".repeat(64), "type": "text" })
        )
        .contains("at most 63 bytes"));
        let twice = create_table(
            Engine::Mysql,
            &spec(
                Engine::Mysql,
                json!([{ "name": "A", "type": "INT" }, { "name": "a", "type": "INT" }]),
                None,
            ),
        );
        assert!(twice.unwrap_err().to_string().contains("Two columns"));
        let none = create_table(Engine::Mysql, &spec(Engine::Mysql, json!([]), None));
        assert!(none
            .unwrap_err()
            .to_string()
            .contains("at least one column"));
        assert!(create_table(
            Engine::Sqlite,
            &spec(Engine::Mysql, json!([{ "name": "a", "type": "INT" }]), None)
        )
        .is_err());
    }

    #[test]
    fn a_key_column_is_not_null_whatever_the_form_says() {
        let s = spec(
            Engine::Postgres,
            json!([{ "name": "code", "type": "text", "nullable": true, "key": true }]),
            None,
        );
        assert_eq!(create_table(Engine::Postgres, &s).unwrap()[0], "CREATE TABLE \"public\".\"orders\" (\n  \"code\" text NOT NULL,\n  PRIMARY KEY (\"code\")\n)");
    }

    fn alter(engine: Engine, edits: serde_json::Value) -> Result<Vec<String>> {
        let schema = (engine == Engine::Postgres).then_some("public");
        let spec: AlterTable = serde_json::from_value(json!({
            "table": { "database": "shop", "schema": schema, "name": "orders" },
            "edits": edits,
        }))
        .unwrap();
        alter_table(engine, &spec, &current())
    }

    fn current() -> Vec<ColumnDefinition> {
        vec![
            ColumnDefinition {
                name: "id".into(),
                column_type: "int unsigned".into(),
                extra: "auto_increment".into(),
                ..Default::default()
            },
            ColumnDefinition {
                name: "title".into(),
                column_type: "varchar(120)".into(),
                charset: Some("utf8mb4".into()),
                collation: Some("utf8mb4_unicode_ci".into()),
                default: Some("ไม่มีชื่อ".into()),
                comment: Some("ชื่อ".into()),
                ..Default::default()
            },
            ColumnDefinition {
                name: "at".into(),
                column_type: "datetime(3)".into(),
                default: Some("CURRENT_TIMESTAMP(3)".into()),
                default_expression: true,
                extra: "DEFAULT_GENERATED on update CURRENT_TIMESTAMP(3)".into(),
                ..Default::default()
            },
            ColumnDefinition {
                name: "v".into(),
                column_type: "int".into(),
                extra: "VIRTUAL GENERATED".into(),
                generated: true,
                ..Default::default()
            },
            ColumnDefinition {
                name: "r".into(),
                column_type: "double".into(),
                default: Some("rand()".into()),
                default_expression: true,
                nullable: true,
                ..Default::default()
            },
        ]
    }

    #[test]
    fn mysql_changes_are_one_statement() {
        let edits = json!([
            { "kind": "addColumn", "column": { "name": "หมายเหตุ", "type": "VARCHAR", "length": 200, "nullable": true, "comment": "x" } },
            { "kind": "dropColumn", "name": "old" },
            { "kind": "renameColumn", "from": "a", "to": "b" },
            { "kind": "addIndex", "columns": ["b", "c"], "unique": true, "name": "ux_bc" },
            { "kind": "addIndex", "columns": ["d"] },
            { "kind": "dropIndex", "name": "ix_old" }
        ]);
        assert_eq!(
            alter(Engine::Mysql, edits).unwrap(),
            ["ALTER TABLE `shop`.`orders`\n  ADD COLUMN `หมายเหตุ` VARCHAR(200) NULL COMMENT 'x',\n  DROP COLUMN `old`,\n  RENAME COLUMN `a` TO `b`,\n  ADD UNIQUE INDEX `ux_bc` (`b`, `c`),\n  ADD INDEX (`d`),\n  DROP INDEX `ix_old`"]
        );
    }

    #[test]
    fn postgres_changes_are_statements_in_order() {
        let edits = json!([
            { "kind": "addColumn", "column": { "name": "note", "type": "text", "nullable": true, "comment": "c" } },
            { "kind": "renameColumn", "from": "a", "to": "B" },
            { "kind": "addIndex", "columns": ["B"], "unique": false },
            { "kind": "dropIndex", "name": "ix old" }
        ]);
        assert_eq!(
            alter(Engine::Postgres, edits).unwrap(),
            [
                "ALTER TABLE \"public\".\"orders\" ADD COLUMN \"note\" text NULL",
                "ALTER TABLE \"public\".\"orders\" RENAME COLUMN \"a\" TO \"B\"",
                "CREATE INDEX ON \"public\".\"orders\" (\"B\")",
                "DROP INDEX \"public\".\"ix old\"",
                "COMMENT ON COLUMN \"public\".\"orders\".\"note\" IS 'c'",
            ]
        );
    }

    #[test]
    fn what_a_change_cannot_be() {
        let err = |engine, edits| alter(engine, edits).unwrap_err().to_string();
        assert!(err(Engine::Mysql, json!([])).contains("Nothing"));
        assert!(err(
            Engine::Mysql,
            json!([{ "kind": "addColumn", "column": { "name": "k", "type": "INT", "key": true } }])
        )
        .contains("primary key"));
        assert!(err(Engine::Mysql, json!([{ "kind": "dropColumn", "name": "a" }, { "kind": "renameColumn", "from": "A", "to": "z" }])).contains("twice"));
        assert!(err(
            Engine::Postgres,
            json!([{ "kind": "renameColumn", "from": "a", "to": "a" }])
        )
        .contains("already has"));
        assert!(err(
            Engine::Postgres,
            json!([{ "kind": "addIndex", "columns": [] }])
        )
        .contains("at least one column"));
        assert!(err(
            Engine::Postgres,
            json!([{ "kind": "addIndex", "columns": ["a", "a"] }])
        )
        .contains("twice"));
        assert!(err(
            Engine::Mysql,
            json!([{ "kind": "dropIndex", "name": "PRIMARY" }])
        )
        .contains("primary key"));
        assert!(err(Engine::Mysql, json!([{ "kind": "addColumn", "column": { "name": "a", "type": "INT); DROP TABLE x" } }])).contains("not a type"));
        let dropped: AlterTable = serde_json::from_value(json!({ "table": { "database": "d", "schema": null, "name": "t" }, "edits": [{ "kind": "dropColumn", "name": "x" }, { "kind": "dropIndex", "name": "i" }] })).unwrap();
        assert_eq!(dropped.dropped_columns(), ["x"]);
    }

    #[test]
    fn a_mysql_change_says_again_what_it_does_not_change() {
        // Only NULL changes: the type, charset, default and comment are said again.
        assert_eq!(
            alter(Engine::Mysql, json!([{ "kind": "changeColumn", "name": "title", "nullable": true }])).unwrap(),
            ["ALTER TABLE `shop`.`orders`\n  MODIFY COLUMN `title` varchar(120) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NULL DEFAULT 'ไม่มีชื่อ' COMMENT 'ชื่อ'"]
        );
        // A new type, the comment removed: the default stays.
        assert_eq!(
            alter(Engine::Mysql, json!([{ "kind": "changeColumn", "name": "title", "type": { "type": "VARCHAR", "length": 200 }, "comment": "" }])).unwrap()[0],
            "ALTER TABLE `shop`.`orders`\n  MODIFY COLUMN `title` VARCHAR(200) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'ไม่มีชื่อ'"
        );
        // AUTO_INCREMENT and ON UPDATE are kept.
        assert_eq!(
            alter(Engine::Mysql, json!([{ "kind": "changeColumn", "name": "id", "type": { "type": "BIGINT", "unsigned": true } }, { "kind": "changeColumn", "name": "at", "nullable": true }])).unwrap()[0],
            "ALTER TABLE `shop`.`orders`\n  MODIFY COLUMN `id` BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,\n  MODIFY COLUMN `at` datetime(3) NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3)"
        );
        // What cannot be said again is refused, not dropped.
        let err = |edit| alter(Engine::Mysql, json!([edit])).unwrap_err().to_string();
        assert!(
            err(json!({ "kind": "changeColumn", "name": "v", "nullable": true }))
                .contains("generated")
        );
        assert!(
            err(json!({ "kind": "changeColumn", "name": "r", "nullable": false }))
                .contains("expression")
        );
        assert!(err(json!({ "kind": "changeColumn", "name": "id", "type": { "type": "VARCHAR", "length": 5 } })).contains("integer"));
        assert!(err(json!({ "kind": "changeColumn", "name": "title" })).contains("Nothing"));
        assert!(
            err(json!({ "kind": "changeColumn", "name": "nope", "nullable": true }))
                .contains("no column")
        );
    }

    #[test]
    fn a_postgres_change_is_one_statement_per_property() {
        assert_eq!(
            alter(Engine::Postgres, json!([{ "kind": "changeColumn", "name": "title", "type": { "type": "varchar", "length": 200 }, "nullable": false, "default": { "kind": "none" }, "comment": "" }])).unwrap(),
            [
                "ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"title\" DROP DEFAULT",
                "ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"title\" TYPE varchar(200) USING \"title\"::varchar(200)",
                "ALTER TABLE \"public\".\"orders\" ALTER COLUMN \"title\" SET NOT NULL",
                "COMMENT ON COLUMN \"public\".\"orders\".\"title\" IS NULL",
            ]
        );
        // A new type for a column with a default: the form must say what the default becomes.
        let quiet = alter(
            Engine::Postgres,
            json!([{ "kind": "changeColumn", "name": "title", "type": { "type": "text" } }]),
        );
        assert!(quiet
            .unwrap_err()
            .to_string()
            .contains("say what its default becomes"));
    }

    #[test]
    fn foreign_keys() {
        let fk = json!([{ "kind": "addForeignKey", "name": "fk_customer", "columns": ["customer_id"], "references": { "database": "shop", "schema": "public", "name": "customers" }, "refColumns": ["id"], "onDelete": "cascade", "onUpdate": "noAction" }, { "kind": "dropForeignKey", "name": "fk_old" }]);
        assert_eq!(
            alter(Engine::Postgres, fk.clone()).unwrap(),
            [
                "ALTER TABLE \"public\".\"orders\" ADD CONSTRAINT \"fk_customer\" FOREIGN KEY (\"customer_id\") REFERENCES \"public\".\"customers\" (\"id\") ON DELETE CASCADE ON UPDATE NO ACTION",
                "ALTER TABLE \"public\".\"orders\" DROP CONSTRAINT \"fk_old\"",
            ]
        );
        assert_eq!(
            alter(Engine::Mysql, fk).unwrap()[0],
            "ALTER TABLE `shop`.`orders`\n  ADD CONSTRAINT `fk_customer` FOREIGN KEY (`customer_id`) REFERENCES `shop`.`customers` (`id`) ON DELETE CASCADE ON UPDATE NO ACTION,\n  DROP FOREIGN KEY `fk_old`"
        );
        let uneven = json!([{ "kind": "addForeignKey", "columns": ["a", "b"], "references": { "database": "shop", "schema": null, "name": "c" }, "refColumns": ["id"] }]);
        assert!(alter(Engine::Mysql, uneven)
            .unwrap_err()
            .to_string()
            .contains("pairs"));
        let elsewhere = json!([{ "kind": "addForeignKey", "columns": ["a"], "references": { "database": "other", "schema": null, "name": "c" }, "refColumns": ["id"] }]);
        assert!(alter(Engine::Mysql, elsewhere)
            .unwrap_err()
            .to_string()
            .contains("same database"));
        assert_eq!(FkAction::parse("SET NULL"), Some(FkAction::SetNull));
        assert_eq!(FkAction::parse("c"), Some(FkAction::Cascade));
        assert_eq!(FkAction::parse("d"), None);
    }
}
