//! Staged grid edits -> parameterised statements.
//!
//! The same plan is shown to the user for review and then executed, so what is reviewed is
//! what runs. Values always travel as bind parameters; identifiers are quoted by `ident`.
//!
//! Safety rules enforced here, independent of the UI:
//! - a row is addressed by its complete primary key, never by position;
//! - every UPDATE also requires the edited columns to still hold the values the user saw
//!   (optimistic concurrency), so a change made elsewhere is detected instead of overwritten;
//! - each statement must affect exactly one row, otherwise the whole save is rolled back.

use serde::{Deserialize, Serialize};

use super::{ident, ColumnMeta, TableMeta, TableRef};
use crate::model::Engine;
use crate::{Error, Result};

/// Upper bound for one save. Larger jobs belong to the import tools, which batch.
pub const MAX_STATEMENTS: usize = 10_000;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CellValue {
    pub column: String,
    /// `None` is SQL NULL.
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CellChange {
    pub column: String,
    /// The value the user was looking at when they edited the cell.
    #[serde(default)]
    pub original: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowUpdate {
    pub key: Vec<CellValue>,
    pub changes: Vec<CellChange>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowInsert {
    /// Columns left out take the server default.
    pub values: Vec<CellValue>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowDelete {
    pub key: Vec<CellValue>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    #[serde(default)]
    pub updates: Vec<RowUpdate>,
    #[serde(default)]
    pub inserts: Vec<RowInsert>,
    #[serde(default)]
    pub deletes: Vec<RowDelete>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StatementKind {
    Update,
    Insert,
    Delete,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    pub kind: StatementKind,
    pub sql: String,
    pub params: Vec<Option<String>>,
    /// Human label of the affected row, e.g. `id=5`, for messages.
    pub row: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub updated: u32,
    pub inserted: u32,
    pub deleted: u32,
    pub elapsed_ms: u64,
}

/// Collects bind parameters and hands out the engine's placeholder for each.
struct Params {
    engine: Engine,
    values: Vec<Option<String>>,
}

impl Params {
    fn push(&mut self, value: &Option<String>) -> String {
        self.values.push(value.clone());
        match self.engine {
            Engine::Postgres => format!("${}", self.values.len()),
            _ => "?".to_string(),
        }
    }
}

pub(crate) fn quote(engine: Engine, name: &str) -> Result<String> {
    match engine {
        Engine::Mysql => ident::mysql(name),
        Engine::Postgres => ident::postgres(name),
        Engine::Sqlserver => ident::sqlserver(name),
    }
}

pub(crate) fn table_name(engine: Engine, table: &TableRef) -> Result<String> {
    let parent = match engine {
        Engine::Postgres => super::require_schema(table.schema.as_deref())?,
        _ => &table.database,
    };
    Ok(format!(
        "{}.{}",
        quote(engine, parent)?,
        quote(engine, &table.name)?
    ))
}

/// A value being written. PostgreSQL parameters are sent as text and cast to the column's
/// bare type; the assignment then applies length and precision with assignment rules, which
/// reject an over-long string instead of truncating it. MySQL converts strings itself, and
/// the session's strict mode rejects bad input.
fn value_expr(engine: Engine, col: &ColumnMeta, placeholder: &str) -> String {
    match engine {
        Engine::Postgres => format!("{placeholder}::{}", col.base_type),
        // A string assigned to BIT would store its character codes.
        _ if col.base_type == "bit" => format!("CAST({placeholder} AS UNSIGNED)"),
        _ => placeholder.to_string(),
    }
}

/// A key value in WHERE. Typed, so the primary-key index is used and 64-bit integers
/// are never compared as floating point.
pub(crate) fn key_expr(engine: Engine, col: &ColumnMeta, placeholder: &str) -> String {
    if engine == Engine::Postgres {
        return format!("{placeholder}::{}", col.base_type);
    }
    match col.base_type.as_str() {
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint" | "bit" | "year" => {
            let sign = if col.data_type.to_ascii_lowercase().contains("unsigned") {
                "UNSIGNED"
            } else {
                "SIGNED"
            };
            format!("CAST({placeholder} AS {sign})")
        }
        "decimal" => format!("CAST({placeholder} AS DECIMAL(65,30))"),
        _ => placeholder.to_string(),
    }
}

/// The server-side text rendering of a column: what the grid shows and compares.
pub(crate) fn rendered(engine: Engine, col: &ColumnMeta, quoted: &str) -> String {
    match engine {
        Engine::Postgres => format!("{quoted}::text"),
        _ if col.base_type == "bit" => format!("CAST({quoted} + 0 AS CHAR)"),
        _ => format!("CAST({quoted} AS CHAR)"),
    }
}

/// "The column still shows what the user saw": compares the server's text rendering,
/// byte for byte and NULL-safe, with the original cell text.
fn unchanged_expr(engine: Engine, col: &ColumnMeta, quoted: &str, placeholder: &str) -> String {
    match engine {
        Engine::Postgres => format!("{quoted}::text IS NOT DISTINCT FROM {placeholder}"),
        _ => {
            let rendered = if col.base_type == "bit" {
                format!("CAST({quoted} + 0 AS CHAR)")
            } else {
                format!("CAST({quoted} AS CHAR)")
            };
            format!("CAST({rendered} AS BINARY) <=> CAST({placeholder} AS BINARY)")
        }
    }
}

pub(crate) fn column<'a>(meta: &'a TableMeta, name: &str) -> Result<&'a ColumnMeta> {
    meta.columns
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| Error::Invalid(format!("Unknown column: {name}")))
}

fn writable<'a>(meta: &'a TableMeta, name: &str) -> Result<&'a ColumnMeta> {
    let col = column(meta, name)?;
    if !col.editable {
        return Err(Error::Invalid(format!(
            "Column {name} cannot be edited here (generated by the server, binary or spatial)"
        )));
    }
    Ok(col)
}

fn no_duplicates<'a>(names: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(Error::Invalid(format!("Column {name} is listed twice")));
        }
    }
    Ok(())
}

/// WHERE fragment and label for a row, in primary-key order. The key must be complete.
fn key_clause(
    engine: Engine,
    meta: &TableMeta,
    key_columns: &[String],
    key: &[CellValue],
    params: &mut Params,
) -> Result<(String, String)> {
    if key.len() != key_columns.len() {
        return Err(Error::Invalid(
            "A row must be identified by its complete primary key".into(),
        ));
    }
    let mut parts = Vec::with_capacity(key.len());
    let mut label = Vec::with_capacity(key.len());
    for pk in key_columns {
        let cell = key
            .iter()
            .find(|k| &k.column == pk)
            .ok_or_else(|| Error::Invalid(format!("Primary key column {pk} is missing")))?;
        let Some(value) = &cell.value else {
            return Err(Error::Invalid(format!(
                "Primary key column {pk} has no value"
            )));
        };
        let col = column(meta, pk)?;
        let placeholder = params.push(&cell.value);
        parts.push(format!(
            "{} = {}",
            quote(engine, pk)?,
            key_expr(engine, col, &placeholder)
        ));
        label.push(format!("{pk}={value}"));
    }
    let mut label = label.join(", ");
    if let Some((cut, _)) = label.char_indices().nth(80) {
        label.truncate(cut);
        label.push('…');
    }
    Ok((parts.join(" AND "), label))
}

/// Turn a change set into statements, in execution order: deletes, updates, inserts
/// (so a deleted row's unique values can be reused by an inserted one in the same save).
pub fn plan(
    engine: Engine,
    table: &TableRef,
    meta: &TableMeta,
    changes: &ChangeSet,
) -> Result<Vec<Statement>> {
    plan_keyed(engine, table, meta, &meta.primary_key, changes)
}

/// `plan`, addressing rows by `key_columns`. The caller guarantees these columns form a
/// unique key of the table (the primary key, or a unique index validated against the catalogue).
pub(crate) fn plan_keyed(
    engine: Engine,
    table: &TableRef,
    meta: &TableMeta,
    key_columns: &[String],
    changes: &ChangeSet,
) -> Result<Vec<Statement>> {
    let total = changes.updates.len() + changes.inserts.len() + changes.deletes.len();
    if total == 0 {
        return Err(Error::Invalid("There is nothing to save".into()));
    }
    if total > MAX_STATEMENTS {
        return Err(Error::Invalid(format!(
            "{total} row changes in one save exceeds the limit of {MAX_STATEMENTS}"
        )));
    }
    if key_columns.is_empty() && (!changes.updates.is_empty() || !changes.deletes.is_empty()) {
        return Err(Error::Unsupported(
            "This table has no primary key, so existing rows cannot be changed or deleted safely"
                .into(),
        ));
    }
    let target = table_name(engine, table)?;
    let mut out = Vec::with_capacity(total);

    for delete in &changes.deletes {
        let mut params = Params {
            engine,
            values: Vec::new(),
        };
        let (where_key, row) = key_clause(engine, meta, key_columns, &delete.key, &mut params)?;
        out.push(Statement {
            kind: StatementKind::Delete,
            sql: format!("DELETE FROM {target} WHERE {where_key}"),
            params: params.values,
            row,
        });
    }

    for update in &changes.updates {
        if update.changes.is_empty() {
            return Err(Error::Invalid("An update without changes".into()));
        }
        no_duplicates(update.changes.iter().map(|c| c.column.as_str()))?;
        let mut params = Params {
            engine,
            values: Vec::new(),
        };
        let mut sets = Vec::with_capacity(update.changes.len());
        for change in &update.changes {
            let col = writable(meta, &change.column)?;
            let placeholder = params.push(&change.value);
            sets.push(format!(
                "{} = {}",
                quote(engine, &col.name)?,
                value_expr(engine, col, &placeholder)
            ));
        }
        let (where_key, row) = key_clause(engine, meta, key_columns, &update.key, &mut params)?;
        let mut conditions = vec![where_key];
        for change in &update.changes {
            let col = column(meta, &change.column)?;
            let placeholder = params.push(&change.original);
            conditions.push(unchanged_expr(
                engine,
                col,
                &quote(engine, &col.name)?,
                &placeholder,
            ));
        }
        out.push(Statement {
            kind: StatementKind::Update,
            sql: format!(
                "UPDATE {target} SET {} WHERE {}",
                sets.join(", "),
                conditions.join(" AND ")
            ),
            params: params.values,
            row,
        });
    }

    for insert in &changes.inserts {
        no_duplicates(insert.values.iter().map(|c| c.column.as_str()))?;
        let mut params = Params {
            engine,
            values: Vec::new(),
        };
        let mut names = Vec::with_capacity(insert.values.len());
        let mut exprs = Vec::with_capacity(insert.values.len());
        for cell in &insert.values {
            let col = writable(meta, &cell.column)?;
            let placeholder = params.push(&cell.value);
            names.push(quote(engine, &col.name)?);
            exprs.push(value_expr(engine, col, &placeholder));
        }
        let sql = if names.is_empty() {
            match engine {
                Engine::Mysql => format!("INSERT INTO {target} () VALUES ()"),
                _ => format!("INSERT INTO {target} DEFAULT VALUES"),
            }
        } else {
            format!(
                "INSERT INTO {target} ({}) VALUES ({})",
                names.join(", "),
                exprs.join(", ")
            )
        };
        out.push(Statement {
            kind: StatementKind::Insert,
            sql,
            params: params.values,
            row: "new row".into(),
        });
    }
    Ok(out)
}

/// The message for a statement that matched no row (or, impossibly, several).
pub(crate) fn conflict(statement: &Statement, affected: u64) -> Error {
    let what = match statement.kind {
        StatementKind::Update => "was changed or removed by someone else after you loaded it",
        StatementKind::Delete => "no longer exists",
        StatementKind::Insert => "could not be inserted",
    };
    Error::Conflict(format!(
        "Row {} {what} ({affected} rows matched). Nothing was saved — reload to see the current data.",
        statement.row
    ))
}

/// Wrap a driver error with the row it happened on and the fact that nothing was kept.
pub(crate) fn failed(statement: &Statement, e: sqlx::Error) -> Error {
    let message = match &e {
        sqlx::Error::Database(db) => db.message().to_string(),
        other => other.to_string(),
    };
    Error::Database(format!(
        "{message} (row {}). Nothing was saved — the transaction was rolled back.",
        statement.row
    ))
}

pub(crate) fn tally(statements: &[Statement], elapsed_ms: u64) -> ApplyResult {
    let count = |kind| statements.iter().filter(|s| s.kind == kind).count() as u32;
    ApplyResult {
        updated: count(StatementKind::Update),
        inserted: count(StatementKind::Insert),
        deleted: count(StatementKind::Delete),
        elapsed_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::super::ValueKind;
    use super::*;

    fn col(name: &str, data_type: &str, pk: bool, editable: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            data_type: data_type.into(),
            kind: ValueKind::Text,
            nullable: true,
            default: None,
            primary_key: pk,
            generated: !editable,
            editable,
            base_type: data_type.split(['(', ' ']).next().unwrap_or("").into(),
        }
    }

    fn meta(types: [&str; 4]) -> TableMeta {
        TableMeta {
            columns: vec![
                col("id", types[0], true, false),
                col("name", types[1], false, true),
                col("price", types[2], false, true),
                col("flags", types[3], false, true),
            ],
            primary_key: vec!["id".into()],
            estimated_rows: None,
        }
    }

    fn cell(column: &str, value: Option<&str>) -> CellValue {
        CellValue {
            column: column.into(),
            value: value.map(Into::into),
        }
    }

    fn change(column: &str, original: Option<&str>, value: Option<&str>) -> CellChange {
        CellChange {
            column: column.into(),
            original: original.map(Into::into),
            value: value.map(Into::into),
        }
    }

    fn some(values: &[Option<&str>]) -> Vec<Option<String>> {
        values.iter().map(|v| v.map(String::from)).collect()
    }

    fn sample() -> ChangeSet {
        ChangeSet {
            updates: vec![RowUpdate {
                key: vec![cell("id", Some("7"))],
                changes: vec![
                    change("name", Some("old"), Some("new")),
                    change("price", None, Some("1.50")),
                    change("flags", Some("5"), None),
                ],
            }],
            inserts: vec![
                RowInsert {
                    values: vec![cell("name", Some("x")), cell("price", None)],
                },
                RowInsert { values: vec![] },
            ],
            deletes: vec![RowDelete {
                key: vec![cell("id", Some("9"))],
            }],
        }
    }

    #[test]
    fn mysql_plan() {
        let table = TableRef {
            database: "shop".into(),
            schema: None,
            name: "items".into(),
        };
        let meta = meta(["bigint unsigned", "varchar(20)", "decimal(12,2)", "bit(8)"]);
        let plan = plan(Engine::Mysql, &table, &meta, &sample()).unwrap();
        assert_eq!(plan.len(), 4);

        assert_eq!(plan[0].kind, StatementKind::Delete);
        assert_eq!(
            plan[0].sql,
            "DELETE FROM `shop`.`items` WHERE `id` = CAST(? AS UNSIGNED)"
        );
        assert_eq!(plan[0].params, some(&[Some("9")]));
        assert_eq!(plan[0].row, "id=9");

        assert_eq!(
            plan[1].sql,
            "UPDATE `shop`.`items` SET `name` = ?, `price` = ?, `flags` = CAST(? AS UNSIGNED) \
             WHERE `id` = CAST(? AS UNSIGNED) \
             AND CAST(CAST(`name` AS CHAR) AS BINARY) <=> CAST(? AS BINARY) \
             AND CAST(CAST(`price` AS CHAR) AS BINARY) <=> CAST(? AS BINARY) \
             AND CAST(CAST(`flags` + 0 AS CHAR) AS BINARY) <=> CAST(? AS BINARY)"
        );
        assert_eq!(
            plan[1].params,
            some(&[
                Some("new"),
                Some("1.50"),
                None,
                Some("7"),
                Some("old"),
                None,
                Some("5")
            ])
        );

        assert_eq!(
            plan[2].sql,
            "INSERT INTO `shop`.`items` (`name`, `price`) VALUES (?, ?)"
        );
        assert_eq!(plan[2].params, some(&[Some("x"), None]));
        assert_eq!(plan[3].sql, "INSERT INTO `shop`.`items` () VALUES ()");
    }

    #[test]
    fn postgres_plan() {
        let table = TableRef {
            database: "app".into(),
            schema: Some("shop".into()),
            name: "Items".into(),
        };
        let meta = meta(["bigint", "text", "numeric(12,2)", "shop.mood"]);
        let plan = plan(Engine::Postgres, &table, &meta, &sample()).unwrap();
        assert_eq!(
            plan[0].sql,
            "DELETE FROM \"shop\".\"Items\" WHERE \"id\" = $1::bigint"
        );
        assert_eq!(
            plan[1].sql,
            "UPDATE \"shop\".\"Items\" SET \"name\" = $1::text, \"price\" = $2::numeric, \
             \"flags\" = $3::shop.mood WHERE \"id\" = $4::bigint \
             AND \"name\"::text IS NOT DISTINCT FROM $5 \
             AND \"price\"::text IS NOT DISTINCT FROM $6 \
             AND \"flags\"::text IS NOT DISTINCT FROM $7"
        );
        assert_eq!(
            plan[2].sql,
            "INSERT INTO \"shop\".\"Items\" (\"name\", \"price\") VALUES ($1::text, $2::numeric)"
        );
        assert_eq!(plan[3].sql, "INSERT INTO \"shop\".\"Items\" DEFAULT VALUES");
    }

    #[test]
    fn refuses_unsafe_or_malformed_changes() {
        let table = TableRef {
            database: "shop".into(),
            schema: None,
            name: "items".into(),
        };
        let m = meta(["int", "text", "int", "int"]);
        let run = |c: ChangeSet| plan(Engine::Mysql, &table, &m, &c);
        let update = |key: Vec<CellValue>, changes: Vec<CellChange>| ChangeSet {
            updates: vec![RowUpdate { key, changes }],
            ..Default::default()
        };

        assert!(matches!(run(ChangeSet::default()), Err(Error::Invalid(_))));
        // incomplete / null / wrong key
        assert!(run(update(vec![], vec![change("name", None, Some("x"))])).is_err());
        assert!(run(update(
            vec![cell("id", None)],
            vec![change("name", None, Some("x"))]
        ))
        .is_err());
        assert!(run(update(
            vec![cell("name", Some("a"))],
            vec![change("name", None, Some("x"))]
        ))
        .is_err());
        // unknown, generated and duplicated columns
        assert!(run(update(
            vec![cell("id", Some("1"))],
            vec![change("nope", None, Some("x"))]
        ))
        .is_err());
        assert!(run(update(
            vec![cell("id", Some("1"))],
            vec![change("id", Some("1"), Some("2"))]
        ))
        .is_err());
        assert!(run(update(
            vec![cell("id", Some("1"))],
            vec![
                change("name", None, Some("x")),
                change("name", None, Some("y"))
            ]
        ))
        .is_err());
        assert!(run(update(vec![cell("id", Some("1"))], vec![])).is_err());

        // keyless tables: inserts only
        let keyless = TableMeta {
            primary_key: vec![],
            ..meta(["int", "text", "int", "int"])
        };
        let delete = ChangeSet {
            deletes: vec![RowDelete { key: vec![] }],
            ..Default::default()
        };
        assert!(matches!(
            plan(Engine::Mysql, &table, &keyless, &delete),
            Err(Error::Unsupported(_))
        ));
        let insert = ChangeSet {
            inserts: vec![RowInsert {
                values: vec![cell("name", Some("x"))],
            }],
            ..Default::default()
        };
        assert!(plan(Engine::Mysql, &table, &keyless, &insert).is_ok());

        // identifiers are quoted, values never reach the SQL text
        let nasty = update(
            vec![cell("id", Some("1 OR 1=1"))],
            vec![change("name", None, Some("'; DROP TABLE items; --"))],
        );
        let sql = &run(nasty).unwrap()[0].sql;
        assert!(!sql.contains("DROP") && !sql.contains("OR 1=1"));
    }
}
