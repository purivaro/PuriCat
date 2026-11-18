//! Row filters for table browsing -> a parameterised WHERE clause.
//!
//! Values always travel as bind parameters, and a column name must exist in the catalogue
//! before `ident` quotes it, so a filter cannot carry SQL.
//!
//! What is compared:
//! - numbers, booleans, dates and text are compared as their own type, so `1.5` finds a stored
//!   `1.50`, an index on the column can be used, and 64-bit integers are never compared as
//!   floating point;
//! - JSON and engine-specific types (uuid, arrays, spatial, ...) are compared as the text the
//!   grid shows, for equality only;
//! - "contains", "starts with" and "ends with" look at the text the grid shows;
//! - binary columns, and MySQL's spatial columns, can only be tested for NULL.
//!
//! SQLite compares by the column's affinity: text sent for an INTEGER, REAL or NUMERIC
//! column meets it as a number when it is one. Dates there are text, and compare as text.
//!
//! "Is not" and "does not contain" keep the rows whose value is NULL: to a person reading the
//! grid an empty cell is "not X", whatever three-valued logic says.

use serde::Deserialize;

use super::{write, ColumnMeta, TableMeta, ValueKind};
use crate::model::Engine;
use crate::{Error, Result};

/// More conditions than anyone builds by hand; a bound on the size of the statement.
pub const MAX_FILTERS: usize = 20;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    IsNull,
    IsNotNull,
}

/// One condition. All conditions of a request must hold (AND).
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    /// Ignored by `IsNull` and `IsNotNull`; required by every other operator.
    #[serde(default)]
    pub value: Option<String>,
}

/// A WHERE clause (with its leading space, or empty) and its bind parameters, in order.
/// PostgreSQL placeholders are numbered from `$1`.
#[derive(Debug, Default, PartialEq)]
pub struct Where {
    pub sql: String,
    pub params: Vec<String>,
}

pub(crate) fn where_clause(engine: Engine, meta: &TableMeta, filters: &[Filter]) -> Result<Where> {
    if filters.len() > MAX_FILTERS {
        return Err(Error::Invalid(format!(
            "{} filter conditions exceeds the limit of {MAX_FILTERS}",
            filters.len()
        )));
    }
    let mut params = Vec::new();
    let mut parts = Vec::with_capacity(filters.len());
    for filter in filters {
        let col = write::column(meta, &filter.column)?;
        parts.push(condition(engine, col, filter, &mut params)?);
    }
    let sql = if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    };
    Ok(Where { sql, params })
}

fn condition(
    engine: Engine,
    col: &ColumnMeta,
    filter: &Filter,
    params: &mut Vec<String>,
) -> Result<String> {
    use FilterOp::*;
    let quoted = write::quote(engine, &col.name)?;
    match filter.op {
        IsNull => return Ok(format!("{quoted} IS NULL")),
        IsNotNull => return Ok(format!("{quoted} IS NOT NULL")),
        _ => {}
    }
    if col.kind == ValueKind::Binary {
        return Err(Error::Invalid(format!(
            "Column {} holds binary data: it can only be filtered on NULL",
            col.name
        )));
    }
    // The grid shows a MySQL spatial value as its WKT text; the column itself is binary.
    if engine == Engine::Mysql && col.kind == ValueKind::Other {
        return Err(Error::Invalid(format!(
            "Column {} ({}) can only be filtered on NULL",
            col.name, col.data_type
        )));
    }
    let Some(value) = &filter.value else {
        return Err(Error::Invalid(format!(
            "The filter on {} needs a value",
            col.name
        )));
    };
    let mut bind = |value: String| {
        params.push(value);
        match engine {
            Engine::Postgres => format!("${}", params.len()),
            _ => "?".to_string(),
        }
    };

    if matches!(filter.op, Contains | NotContains | StartsWith | EndsWith) {
        // A text column is matched as itself, under its own collation; anything else as
        // the text the grid shows.
        let target = if col.kind == ValueKind::Text {
            quoted.clone()
        } else {
            write::rendered(engine, col, &quoted)
        };
        let escaped = like_escape(value);
        let pattern = match filter.op {
            StartsWith => format!("{escaped}%"),
            EndsWith => format!("%{escaped}"),
            _ => format!("%{escaped}%"),
        };
        let placeholder = bind(pattern);
        // PostgreSQL's LIKE is case-sensitive; MySQL's follows the collation, which is
        // case-insensitive unless the column says otherwise. ILIKE brings the two together.
        let like = if engine == Engine::Postgres {
            "ILIKE"
        } else {
            "LIKE"
        };
        // An explicit escape character: the default (a backslash) depends on server settings.
        return Ok(if filter.op == NotContains {
            format!("({target} NOT {like} {placeholder} ESCAPE '!' OR {quoted} IS NULL)")
        } else {
            format!("{target} {like} {placeholder} ESCAPE '!'")
        });
    }

    let equality = matches!(filter.op, Equals | NotEquals);
    let typed = match col.kind {
        ValueKind::Number | ValueKind::Bool | ValueKind::Temporal | ValueKind::Text => {
            // A MySQL FLOAT widened to double never equals the double parsed from the same
            // digits, so equality on it compares what the grid shows instead.
            !(engine == Engine::Mysql && col.base_type == "float" && equality)
        }
        _ => false,
    };
    if !typed && !equality {
        return Err(Error::Invalid(format!(
            "Column {} ({}) can be filtered for equality, text or NULL only",
            col.name, col.data_type
        )));
    }
    // MySQL turns text that is not a number into 0 with a warning, which would silently
    // show the rows that hold 0. PostgreSQL refuses the cast by itself. SQLite compares
    // such text as text, after every number: "greater than abc" would quietly show nothing.
    let numbers_only = match engine {
        Engine::Mysql => matches!(col.kind, ValueKind::Number | ValueKind::Bool),
        Engine::Sqlite => col.kind == ValueKind::Number,
        _ => false,
    };
    if numbers_only && !is_number(value) {
        return Err(Error::Invalid(format!(
            "“{value}” is not a number, and column {} holds numbers",
            col.name
        )));
    }
    // MySQL reads text that is not a date as NULL with a warning: the condition would hold
    // for no row at all, and "after 2026" would look like an empty table.
    if engine == Engine::Mysql && col.kind == ValueKind::Temporal && !is_temporal(col, value) {
        return Err(Error::Invalid(format!(
            "“{value}” is not a complete {} value, and column {} holds those. \
             To find part of a date or time, use “starts with” or “contains”.",
            col.base_type, col.name
        )));
    }
    let placeholder = bind(value.clone());
    let (left, right) = if typed {
        (quoted, write::key_expr(engine, col, &placeholder))
    } else {
        (write::rendered(engine, col, &quoted), placeholder)
    };
    Ok(match filter.op {
        Equals => format!("{left} = {right}"),
        NotEquals if engine == Engine::Postgres => format!("{left} IS DISTINCT FROM {right}"),
        NotEquals if engine == Engine::Sqlite => format!("{left} IS NOT {right}"),
        NotEquals => format!("NOT ({left} <=> {right})"),
        Greater => format!("{left} > {right}"),
        GreaterOrEqual => format!("{left} >= {right}"),
        Less => format!("{left} < {right}"),
        LessOrEqual => format!("{left} <= {right}"),
        _ => unreachable!("handled above"),
    })
}

/// Make `%`, `_` and the escape character itself literal in a LIKE pattern.
fn like_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '!' | '%' | '_') {
            out.push('!');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn is_number(text: &str) -> bool {
    let text = text.trim();
    // `parse` alone also accepts "inf" and "nan".
    text.bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
        && text.parse::<f64>().is_ok()
}

/// Whether MySQL can read `text` as a value of the temporal column: the forms the grid
/// itself shows (`2026-01-31`, `2026-01-31 13:45:00.5`, `13:45:00`), nothing looser.
fn is_temporal(col: &ColumnMeta, text: &str) -> bool {
    fn digits(part: &str, min: usize, max: usize, limit: u32) -> bool {
        (min..=max).contains(&part.len())
            && part.bytes().all(|b| b.is_ascii_digit())
            && part.parse::<u32>().is_ok_and(|n| n <= limit)
    }
    fn date(text: &str) -> bool {
        let parts: Vec<&str> = text.split('-').collect();
        parts.len() == 3
            && digits(parts[0], 4, 4, 9999)
            && digits(parts[1], 1, 2, 12)
            && digits(parts[2], 1, 2, 31)
    }
    // `hours` is 23 for a time of day; a TIME column also holds durations up to 838 hours.
    fn time(text: &str, hours: u32) -> bool {
        let (whole, fraction) = text.split_once('.').unwrap_or((text, "0"));
        let parts: Vec<&str> = whole.split(':').collect();
        (2..=3).contains(&parts.len())
            && digits(parts[0], 1, 3, hours)
            && parts[1..].iter().all(|p| digits(p, 2, 2, 59))
            && digits(fraction, 1, 6, 999_999)
    }
    let text = text.trim();
    match col.base_type.as_str() {
        "date" => date(text),
        "time" => time(text.strip_prefix('-').unwrap_or(text), 838),
        // datetime, timestamp
        _ => match text.split_once([' ', 'T']) {
            Some((day, clock)) => date(day) && time(clock, 23),
            None => date(text),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn meta() -> TableMeta {
        TableMeta {
            columns: vec![
                col("id", "bigint unsigned", ValueKind::Number),
                col("na`me", "varchar(20)", ValueKind::Text),
                col("price", "decimal(12,2)", ValueKind::Number),
                col("ratio", "float", ValueKind::Number),
                col("made", "date", ValueKind::Temporal),
                col("seen", "datetime(6)", ValueKind::Temporal),
                col("took", "time", ValueKind::Temporal),
                col("doc", "json", ValueKind::Json),
                col("photo", "blob", ValueKind::Binary),
                col("spot", "point", ValueKind::Other),
            ],
            primary_key: vec!["id".into()],
            estimated_rows: None,
        }
    }

    fn filter(column: &str, op: FilterOp, value: Option<&str>) -> Filter {
        Filter {
            column: column.into(),
            op,
            value: value.map(str::to_string),
        }
    }

    fn mysql(filters: &[Filter]) -> Result<Where> {
        where_clause(Engine::Mysql, &meta(), filters)
    }

    fn postgres(filters: &[Filter]) -> Result<Where> {
        where_clause(Engine::Postgres, &meta(), filters)
    }

    #[test]
    fn sqlite_compares_by_affinity() {
        use FilterOp::*;
        let sqlite = |filters: &[Filter]| where_clause(Engine::Sqlite, &meta(), filters);
        let w = sqlite(&[
            filter("id", Greater, Some("5")),
            filter("na`me", NotEquals, Some("x")),
            filter("na`me", Contains, Some("50%")),
            filter("price", NotContains, Some("9")),
            filter("made", LessOrEqual, Some("2026-01")),
            filter("doc", Equals, Some("{}")),
            filter("spot", Equals, Some("1")),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE \"id\" > ? AND \"na`me\" IS NOT ? AND \"na`me\" LIKE ? ESCAPE '!' \
             AND (CAST(\"price\" AS TEXT) NOT LIKE ? ESCAPE '!' OR \"price\" IS NULL) \
             AND \"made\" <= ? AND CAST(\"doc\" AS TEXT) = ? AND CAST(\"spot\" AS TEXT) = ?"
        );
        assert_eq!(w.params, ["5", "x", "%50!%%", "%9%", "2026-01", "{}", "1"]);
        // Dates are text there: part of one is a value like any other. A number is not.
        assert!(sqlite(&[filter("id", Equals, Some("five"))]).is_err());
        assert!(sqlite(&[filter("spot", Greater, Some("1"))]).is_err());
        assert!(sqlite(&[filter("photo", Equals, Some("0x00"))]).is_err());
    }

    #[test]
    fn no_filters_no_clause() {
        assert_eq!(mysql(&[]).unwrap(), Where::default());
    }

    #[test]
    fn typed_comparisons_bind_the_value() {
        use FilterOp::*;
        let w = mysql(&[
            filter("id", Equals, Some("9007199254740993")),
            filter("price", GreaterOrEqual, Some("1.5")),
            filter("na`me", NotEquals, Some("x' OR 1=1 --")),
            filter("made", Less, Some("2026-01-01")),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE `id` = CAST(? AS UNSIGNED) AND `price` >= CAST(? AS DECIMAL(65,30)) \
             AND NOT (`na``me` <=> ?) AND `made` < ?"
        );
        assert_eq!(
            w.params,
            ["9007199254740993", "1.5", "x' OR 1=1 --", "2026-01-01"]
        );

        let w = postgres(&[
            filter("id", Equals, Some("7")),
            filter("na`me", NotEquals, Some("a")),
            filter("made", LessOrEqual, Some("2026-01-01")),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE \"id\" = $1::bigint AND \"na`me\" IS DISTINCT FROM $2::varchar \
             AND \"made\" <= $3::date"
        );
        assert_eq!(w.params, ["7", "a", "2026-01-01"]);
    }

    #[test]
    fn text_search_escapes_wildcards_and_keeps_nulls_out_of_the_way() {
        use FilterOp::*;
        let w = mysql(&[
            filter("na`me", Contains, Some("50%_off!")),
            filter("id", StartsWith, Some("20")),
            filter("made", EndsWith, Some("-01")),
            filter("doc", NotContains, Some("x")),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE `na``me` LIKE ? ESCAPE '!' AND CAST(`id` AS CHAR) LIKE ? ESCAPE '!' \
             AND CAST(`made` AS CHAR) LIKE ? ESCAPE '!' \
             AND (CAST(`doc` AS CHAR) NOT LIKE ? ESCAPE '!' OR `doc` IS NULL)"
        );
        assert_eq!(w.params, ["%50!%!_off!!%", "20%", "%-01", "%x%"]);

        let w = postgres(&[filter("price", Contains, Some("9"))]).unwrap();
        assert_eq!(w.sql, " WHERE \"price\"::text ILIKE $1 ESCAPE '!'");
    }

    #[test]
    fn null_tests_take_no_value_and_work_on_binary() {
        let w = postgres(&[
            filter("photo", FilterOp::IsNull, Some("ignored")),
            filter("doc", FilterOp::IsNotNull, None),
        ])
        .unwrap();
        assert_eq!(w.sql, " WHERE \"photo\" IS NULL AND \"doc\" IS NOT NULL");
        assert!(w.params.is_empty());
    }

    #[test]
    fn untyped_columns_compare_the_shown_text() {
        let w = mysql(&[
            filter("doc", FilterOp::Equals, Some("{\"a\": 1}")),
            filter("ratio", FilterOp::Equals, Some("1.1")),
            filter("ratio", FilterOp::Greater, Some("1.1")),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE CAST(`doc` AS CHAR) = ? AND CAST(`ratio` AS CHAR) = ? AND `ratio` > ?"
        );
        let w = postgres(&[filter("doc", FilterOp::NotEquals, Some("{}"))]).unwrap();
        assert_eq!(w.sql, " WHERE \"doc\"::text IS DISTINCT FROM $1");
    }

    #[test]
    fn refusals() {
        use FilterOp::*;
        let refused = |f: Filter| matches!(mysql(&[f]), Err(Error::Invalid(_)));
        assert!(refused(filter("nope", Equals, Some("1"))));
        assert!(refused(filter("id; DROP TABLE x", IsNull, None)));
        assert!(refused(filter("id", Equals, None)));
        assert!(refused(filter("photo", Equals, Some("00"))));
        assert!(refused(filter("photo", Contains, Some("00"))));
        assert!(refused(filter("doc", Greater, Some("1"))));
        assert!(refused(filter("spot", Equals, Some("POINT(1 2)"))));
        assert!(refused(filter("spot", Contains, Some("POINT"))));
        assert!(!refused(filter("spot", IsNotNull, None)));
        // MySQL would read these as 0 and show the rows holding 0.
        for text in ["abc", "", "1 OR 1", "nan", "inf", "0x10"] {
            assert!(refused(filter("id", Equals, Some(text))), "{text}");
        }
        for text in ["5", " 5 ", "-1.50", ".5", "1e3"] {
            assert!(!refused(filter("id", Equals, Some(text))), "{text}");
        }
        // MySQL would read these as NULL and show no rows at all.
        for text in [
            "garbage",
            "2026",
            "2026-01",
            "2026-13-01",
            "2026-01-02 25:00",
            "",
        ] {
            assert!(refused(filter("made", Greater, Some(text))), "{text}");
            assert!(refused(filter("seen", Equals, Some(text))), "{text}");
        }
        for text in ["2026-01-02", "2026-1-2", " 2026-01-02 "] {
            assert!(!refused(filter("made", Greater, Some(text))), "{text}");
            assert!(!refused(filter("seen", Greater, Some(text))), "{text}");
        }
        for text in [
            "2026-01-02 03:04",
            "2026-01-02T03:04:05",
            "2026-01-02 03:04:05.123456",
        ] {
            assert!(!refused(filter("seen", Less, Some(text))), "{text}");
            assert!(refused(filter("made", Less, Some(text))), "{text}");
        }
        for text in ["03:04", "03:04:05.5", "-838:59:59"] {
            assert!(!refused(filter("took", Less, Some(text))), "{text}");
        }
        assert!(refused(filter("took", Less, Some("2026-01-02"))));
        // Looking for part of a date is a text search, and stays possible.
        assert!(!refused(filter("made", StartsWith, Some("2026-01"))));
        // PostgreSQL refuses a bad cast itself, so the text is passed on.
        assert!(postgres(&[filter("id", Equals, Some("abc"))]).is_ok());
        let many = vec![filter("id", IsNull, None); MAX_FILTERS + 1];
        assert!(matches!(mysql(&many), Err(Error::Invalid(_))));
    }
}
