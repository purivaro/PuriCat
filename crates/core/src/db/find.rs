//! Find in database: which tables hold a value, and in which columns; and which tables and
//! columns have a name that contains some text.
//!
//! A table is searched with one statement that reads it once and counts, per column, the rows
//! in which that column holds the value. Each column's test is the row filter's own condition
//! ([`filter::condition`]), so a table opened with the same filter shows exactly the rows that
//! were counted. Values travel as bind parameters, and column names come from the catalogue.
//!
//! A column the value cannot be in is left out rather than failing the table: binary and
//! spatial columns; for an exact value, dates, times and booleans (a cast of text that is not
//! one would fail the statement on PostgreSQL), and numbers the text is not (an integer column
//! only when the text is an integer it can hold: MySQL would turn `1.5` into 1).

use serde::{Deserialize, Serialize};

use super::filter::{self, Filter, FilterOp};
use super::{write, ObjectKind, TableMeta, TableRef, ValueKind};
use crate::model::Engine;
use crate::{Error, Result};

/// The longest text looked for, in characters.
pub const MAX_TEXT: usize = 1000;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FindMode {
    Contains,
    /// The whole value, compared as the column's own type.
    Equals,
    StartsWith,
}

impl FindMode {
    fn op(self) -> FilterOp {
        match self {
            FindMode::Contains => FilterOp::Contains,
            FindMode::Equals => FilterOp::Equals,
            FindMode::StartsWith => FilterOp::StartsWith,
        }
    }
}

/// What one table holds of the value.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableFind {
    /// The columns the value was looked for in, in the table's order.
    pub searched: Vec<String>,
    /// The columns that hold it, with the number of rows each: only those with one or more.
    pub hits: Vec<ColumnHit>,
    /// The rows in which any searched column holds it.
    pub rows: u64,
    pub elapsed_ms: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnHit {
    pub column: String,
    pub rows: u64,
}

/// A table, view or column whose name contains the text.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NameHit {
    /// PostgreSQL only.
    pub schema: Option<String>,
    pub table: String,
    pub kind: ObjectKind,
    /// `None`: the table's own name matched.
    pub column: Option<String>,
    pub data_type: Option<String>,
}

fn unsupported() -> Error {
    Error::Unsupported("Find in database is for MySQL, PostgreSQL and SQLite".into())
}

pub(crate) fn check_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Err(Error::Invalid("Type something to look for".into()));
    }
    if text.chars().count() > MAX_TEXT {
        return Err(Error::Invalid(format!(
            "The text to look for is longer than {MAX_TEXT} characters"
        )));
    }
    if text.contains('\0') {
        return Err(Error::Invalid(
            "The text to look for holds a NUL character".into(),
        ));
    }
    Ok(())
}

/// The statement of one table's search, its parameters, and the columns it tests, in the
/// order of the counts it answers with.
#[derive(Debug, PartialEq)]
pub(crate) struct TableSearch {
    pub sql: String,
    pub params: Vec<String>,
    pub columns: Vec<String>,
}

/// The statement that counts, in one pass over the table, the rows each column holds the
/// value in, then the rows any of them does; its one text value is the counts joined by
/// commas. `None` when no column can hold the value. Also the columns, in the counts' order.
pub(crate) fn table_sql(
    engine: Engine,
    table: &TableRef,
    meta: &TableMeta,
    text: &str,
    mode: FindMode,
) -> Result<Option<TableSearch>> {
    check_text(text)?;
    let mut params = Vec::new();
    let mut columns = Vec::new();
    let mut tests = Vec::new();
    for col in &meta.columns {
        if mode == FindMode::Equals && !can_equal(engine, col, text) {
            continue;
        }
        let f = Filter {
            column: col.name.clone(),
            op: mode.op(),
            value: Some(text.to_string()),
            values: Vec::new(),
        };
        let before = params.len();
        match filter::condition(engine, col, &f, &mut params) {
            Ok(test) => {
                columns.push(col.name.clone());
                tests.push(test);
            }
            // The value cannot be in this column: it is not searched.
            Err(_) => params.truncate(before),
        }
    }
    if columns.is_empty() {
        return Ok(None);
    }
    // Each test once, as a column of a derived table; the counts are taken over those.
    let marks: Vec<String> = tests
        .iter()
        .enumerate()
        .map(|(i, t)| format!("({t}) AS m{i}"))
        .collect();
    let mut counts: Vec<String> = (0..tests.len())
        .map(|i| format!("COUNT(CASE WHEN m{i} THEN 1 END)"))
        .collect();
    let any: Vec<String> = (0..tests.len()).map(|i| format!("m{i}")).collect();
    counts.push(format!("COUNT(CASE WHEN {} THEN 1 END)", any.join(" OR ")));
    let joined = match engine {
        Engine::Mysql => format!("CONCAT_WS(',', {})", counts.join(", ")),
        Engine::Postgres => counts
            .iter()
            .map(|c| format!("{c}::text"))
            .collect::<Vec<_>>()
            .join(" || ',' || "),
        Engine::Sqlite => counts
            .iter()
            .map(|c| format!("CAST({c} AS TEXT)"))
            .collect::<Vec<_>>()
            .join(" || ',' || "),
        Engine::Sqlserver => return Err(unsupported()),
    };
    let sql = format!(
        "SELECT {joined} FROM (SELECT {} FROM {}) AS found",
        marks.join(", "),
        write::table_name(engine, table)?
    );
    Ok(Some(TableSearch {
        sql,
        params,
        columns,
    }))
}

/// Reads the answer of [`table_sql`].
pub(crate) fn read_counts(
    columns: Vec<String>,
    answer: Option<&str>,
) -> Result<(Vec<ColumnHit>, u64)> {
    let bad = || Error::Database("The server did not answer the search with counts".into());
    let numbers = answer
        .ok_or_else(bad)?
        .split(',')
        .map(|n| n.trim().parse::<u64>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| bad())?;
    if numbers.len() != columns.len() + 1 {
        return Err(bad());
    }
    let hits = columns
        .into_iter()
        .zip(&numbers)
        .filter(|(_, n)| **n > 0)
        .map(|(column, n)| ColumnHit { column, rows: *n })
        .collect();
    Ok((hits, numbers[numbers.len() - 1]))
}

/// Whether "is exactly" can be tested on `col` with `text` without a cast that fails or
/// changes the value.
fn can_equal(engine: Engine, col: &super::ColumnMeta, text: &str) -> bool {
    match col.kind {
        ValueKind::Text => true,
        ValueKind::Temporal | ValueKind::Bool | ValueKind::Binary => false,
        ValueKind::Number => {
            if engine == Engine::Sqlite {
                return filter::is_number(text);
            }
            match integer_range(engine, col) {
                Some((low, high)) => text
                    .trim()
                    .parse::<i128>()
                    .is_ok_and(|n| n >= low && n <= high),
                // MySQL's BIT and YEAR are numbers of their own; not looked for.
                None if engine == Engine::Mysql
                    && matches!(col.base_type.as_str(), "bit" | "year") =>
                {
                    false
                }
                None => filter::is_number(text),
            }
        }
        // Compared as the text the grid shows.
        _ => true,
    }
}

/// The values an integer column holds; `None` for other numbers.
fn integer_range(engine: Engine, col: &super::ColumnMeta) -> Option<(i128, i128)> {
    let bits = match (engine, col.base_type.as_str()) {
        (Engine::Mysql, "tinyint") => 8,
        (Engine::Mysql, "smallint") | (Engine::Postgres, "smallint") => 16,
        (Engine::Mysql, "mediumint") => 24,
        (Engine::Mysql, "int") | (Engine::Postgres, "integer") => 32,
        (Engine::Mysql, "bigint") | (Engine::Postgres, "bigint") => 64,
        _ => return None,
    };
    let unsigned =
        engine == Engine::Mysql && col.data_type.to_ascii_lowercase().contains("unsigned");
    Some(if unsigned {
        (0, (1i128 << bits) - 1)
    } else {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    })
}

/// The pattern for a name that contains `text`, in lower case (`!` escapes).
pub(crate) fn name_pattern(text: &str) -> Result<String> {
    check_text(text)?;
    let mut out = String::from("%");
    for ch in text.to_lowercase().chars() {
        if matches!(ch, '!' | '%' | '_') {
            out.push('!');
        }
        out.push(ch);
    }
    out.push('%');
    Ok(out)
}

/// The catalogue query for names, its parameters taken from `pattern` and the place.
/// Rows: schema, table, kind, column, column type.
pub(crate) fn names_sql(engine: Engine) -> Result<&'static str> {
    Ok(match engine {
        Engine::Mysql => {
            "SELECT NULL, TABLE_NAME, TABLE_TYPE, NULL, NULL FROM information_schema.TABLES \
               WHERE TABLE_SCHEMA = ? AND LOWER(TABLE_NAME) LIKE ? ESCAPE '!' \
             UNION ALL \
             SELECT NULL, c.TABLE_NAME, t.TABLE_TYPE, c.COLUMN_NAME, c.COLUMN_TYPE \
               FROM information_schema.COLUMNS c JOIN information_schema.TABLES t \
                 ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME \
               WHERE c.TABLE_SCHEMA = ? AND LOWER(c.COLUMN_NAME) LIKE ? ESCAPE '!'"
        }
        // $1: the schema, or NULL for every schema that is not the server's own.
        Engine::Postgres => {
            "SELECT n.nspname::text, c.relname::text, c.relkind::text, NULL::text, NULL::text \
               FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
               WHERE c.relkind IN ('r', 'p', 'v', 'm') \
                 AND (n.nspname = $1 OR ($1 IS NULL AND n.nspname <> 'information_schema' AND n.nspname NOT LIKE 'pg!_%' ESCAPE '!')) \
                 AND lower(c.relname) LIKE $2 ESCAPE '!' \
             UNION ALL \
             SELECT n.nspname::text, c.relname::text, c.relkind::text, a.attname::text, format_type(a.atttypid, a.atttypmod) \
               FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
               WHERE c.relkind IN ('r', 'p', 'v', 'm') AND a.attnum > 0 AND NOT a.attisdropped \
                 AND (n.nspname = $1 OR ($1 IS NULL AND n.nspname <> 'information_schema' AND n.nspname NOT LIKE 'pg!_%' ESCAPE '!')) \
                 AND lower(a.attname) LIKE $2 ESCAPE '!'"
        }
        Engine::Sqlite => {
            "SELECT NULL, m.name, m.type, NULL, NULL FROM sqlite_master m \
               WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite!_%' ESCAPE '!' \
                 AND lower(m.name) LIKE ? ESCAPE '!' \
             UNION ALL \
             SELECT NULL, m.name, m.type, p.name, p.type FROM sqlite_master m JOIN pragma_table_info(m.name) p \
               WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite!_%' ESCAPE '!' \
                 AND lower(p.name) LIKE ? ESCAPE '!'"
        }
        Engine::Sqlserver => return Err(unsupported()),
    })
}

/// The rows of [`names_sql`], tables before their columns, in name order.
pub(crate) fn read_names(rows: Vec<Vec<Option<String>>>) -> Vec<NameHit> {
    let mut hits: Vec<NameHit> = rows
        .into_iter()
        .filter_map(|r| {
            let at = |i: usize| r.get(i).cloned().flatten();
            let kind = match at(2)?.to_ascii_lowercase().as_str() {
                "base table" | "table" | "r" | "p" => ObjectKind::Table,
                "view" | "system view" | "v" => ObjectKind::View,
                "m" => ObjectKind::Matview,
                _ => return None,
            };
            Some(NameHit {
                schema: at(0),
                table: at(1)?,
                kind,
                column: at(3),
                data_type: at(4),
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        (
            &a.schema,
            a.table.to_lowercase(),
            &a.table,
            a.column.is_some(),
            &a.column,
        )
            .cmp(&(
                &b.schema,
                b.table.to_lowercase(),
                &b.table,
                b.column.is_some(),
                &b.column,
            ))
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ColumnMeta;

    fn col(name: &str, data_type: &str, kind: ValueKind) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            data_type: data_type.into(),
            kind,
            nullable: true,
            default: None,
            primary_key: false,
            generated: false,
            editable: true,
            base_type: data_type.split(['(', ' ']).next().unwrap_or("").into(),
        }
    }

    fn meta(columns: Vec<ColumnMeta>) -> TableMeta {
        TableMeta {
            columns,
            primary_key: vec![],
            estimated_rows: None,
        }
    }

    fn table() -> TableRef {
        TableRef {
            database: "shop".into(),
            schema: None,
            name: "orders".into(),
        }
    }

    fn mysql_meta() -> TableMeta {
        meta(vec![
            col("id", "int unsigned", ValueKind::Number),
            col("small", "tinyint", ValueKind::Number),
            col("name", "varchar(20)", ValueKind::Text),
            col("price", "decimal(12,2)", ValueKind::Number),
            col("made", "date", ValueKind::Temporal),
            col("photo", "blob", ValueKind::Binary),
            col("spot", "point", ValueKind::Other),
        ])
    }

    #[test]
    fn one_pass_over_a_table_counts_each_column_and_the_rows() {
        let TableSearch {
            sql,
            params,
            columns,
        } = table_sql(
            Engine::Mysql,
            &table(),
            &mysql_meta(),
            "5%",
            FindMode::Contains,
        )
        .unwrap()
        .unwrap();
        // Binary and spatial columns cannot hold text.
        assert_eq!(columns, ["id", "small", "name", "price", "made"]);
        assert_eq!(params, vec!["%5!%%"; 5]);
        assert_eq!(
            sql,
            "SELECT CONCAT_WS(',', COUNT(CASE WHEN m0 THEN 1 END), COUNT(CASE WHEN m1 THEN 1 END), \
             COUNT(CASE WHEN m2 THEN 1 END), COUNT(CASE WHEN m3 THEN 1 END), COUNT(CASE WHEN m4 THEN 1 END), \
             COUNT(CASE WHEN m0 OR m1 OR m2 OR m3 OR m4 THEN 1 END)) FROM (SELECT \
             (CAST(`id` AS CHAR) LIKE ? ESCAPE '!') AS m0, (CAST(`small` AS CHAR) LIKE ? ESCAPE '!') AS m1, \
             (`name` LIKE ? ESCAPE '!') AS m2, (CAST(`price` AS CHAR) LIKE ? ESCAPE '!') AS m3, \
             (CAST(`made` AS CHAR) LIKE ? ESCAPE '!') AS m4 FROM `shop`.`orders`) AS found"
        );
    }

    #[test]
    fn an_exact_value_is_looked_for_only_where_it_can_be() {
        let columns = |engine, text| {
            table_sql(engine, &table(), &mysql_meta(), text, FindMode::Equals)
                .unwrap()
                .map(|t| t.columns)
                .unwrap_or_default()
        };
        // Text: in text columns only; never in a date.
        assert_eq!(columns(Engine::Mysql, "abc"), ["name"]);
        // An integer that fits: every number column; one too big for TINYINT skips it.
        assert_eq!(
            columns(Engine::Mysql, "100"),
            ["id", "small", "name", "price"]
        );
        assert_eq!(columns(Engine::Mysql, "300"), ["id", "name", "price"]);
        // Not an integer: no integer column (MySQL would make 1.5 into 1).
        assert_eq!(columns(Engine::Mysql, "1.5"), ["name", "price"]);
        // Negative: not in an unsigned column.
        assert_eq!(columns(Engine::Mysql, "-1"), ["small", "name", "price"]);
        let pg = meta(vec![
            col("n", "smallint", ValueKind::Number),
            col("big", "bigint", ValueKind::Number),
            col("ok", "boolean", ValueKind::Bool),
            col("at", "timestamp", ValueKind::Temporal),
            col("u", "uuid", ValueKind::Other),
        ]);
        let pg_table = TableRef {
            schema: Some("public".into()),
            ..table()
        };
        let pg_columns = |text| {
            table_sql(Engine::Postgres, &pg_table, &pg, text, FindMode::Equals)
                .unwrap()
                .map(|t| t.columns)
                .unwrap_or_default()
        };
        assert_eq!(pg_columns("40000"), ["big", "u"]);
        assert_eq!(pg_columns("true"), ["u"]);
        let TableSearch { sql, params, .. } =
            table_sql(Engine::Postgres, &pg_table, &pg, "7", FindMode::Equals)
                .unwrap()
                .unwrap();
        assert_eq!(params, ["7", "7", "7"]);
        assert!(
            sql.starts_with("SELECT COUNT(CASE WHEN m0 THEN 1 END)::text || ',' || "),
            "{sql}"
        );
        assert!(sql.contains("(\"n\" = $1::smallint) AS m0, (\"big\" = $2::bigint) AS m1, (\"u\"::text = $3) AS m2"), "{sql}");
    }

    #[test]
    fn a_table_with_nowhere_to_look_is_not_read() {
        let blobs = meta(vec![col("photo", "blob", ValueKind::Binary)]);
        assert_eq!(
            table_sql(Engine::Mysql, &table(), &blobs, "x", FindMode::Contains).unwrap(),
            None
        );
    }

    #[test]
    fn what_cannot_be_looked_for() {
        let m = mysql_meta();
        assert!(table_sql(Engine::Mysql, &table(), &m, "", FindMode::Contains).is_err());
        assert!(table_sql(Engine::Mysql, &table(), &m, "a\0b", FindMode::Contains).is_err());
        assert!(table_sql(
            Engine::Mysql,
            &table(),
            &m,
            &"x".repeat(MAX_TEXT + 1),
            FindMode::Contains
        )
        .is_err());
        assert!(name_pattern("").is_err());
    }

    #[test]
    fn counts_are_read_back_per_column() {
        let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let (hits, rows) = read_counts(columns.clone(), Some("2,0,5,6")).unwrap();
        assert_eq!(
            hits,
            vec![
                ColumnHit {
                    column: "a".into(),
                    rows: 2
                },
                ColumnHit {
                    column: "c".into(),
                    rows: 5
                }
            ]
        );
        assert_eq!(rows, 6);
        assert!(read_counts(columns.clone(), Some("1,2")).is_err());
        assert!(read_counts(columns.clone(), Some("1,x,2,3")).is_err());
        assert!(read_counts(columns, None).is_err());
    }

    #[test]
    fn names_are_matched_in_lower_case_with_wildcards_made_literal() {
        assert_eq!(name_pattern("Order_ID").unwrap(), "%order!_id%");
        assert_eq!(name_pattern("50%!").unwrap(), "%50!%!!%");
        let rows = vec![
            vec![
                None,
                Some("orders".into()),
                Some("o_id".into()),
                Some("order_id".into()),
                Some("int".into()),
            ],
            vec![
                None,
                Some("orders".into()),
                Some("BASE TABLE".into()),
                Some("order_id".into()),
                Some("int".into()),
            ],
            vec![
                None,
                Some("orders".into()),
                Some("BASE TABLE".into()),
                None,
                None,
            ],
            vec![
                None,
                Some("Order_view".into()),
                Some("VIEW".into()),
                None,
                None,
            ],
        ];
        let hits = read_names(rows);
        // An unknown kind is left out; names in order whatever their case; a table before its columns.
        assert_eq!(
            hits.iter()
                .map(|h| (h.table.as_str(), h.kind, h.column.as_deref()))
                .collect::<Vec<_>>(),
            [
                ("Order_view", ObjectKind::View, None),
                ("orders", ObjectKind::Table, None),
                ("orders", ObjectKind::Table, Some("order_id"))
            ]
        );
    }
}
