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
//! "Is not", "is not in the list" and "does not contain" keep the rows whose value is NULL: to
//! a person reading the grid an empty cell is "not X", whatever three-valued logic says.
//!
//! A list ("is one of") is so many equality tests: each value is checked and bound exactly as
//! "is" would bind it. "Is between" is two ordered comparisons, both ends included. "Is empty"
//! is about the empty text of a text column, which is not NULL.
//!
//! The conditions of a request must all hold, or — when the request says so — any one of them.

use serde::Deserialize;

use super::{write, ColumnMeta, TableMeta, ValueKind};
use crate::model::Engine;
use crate::{Error, Result};

/// More conditions than anyone builds by hand; a bound on the size of the statement.
pub const MAX_FILTERS: usize = 20;
/// The longest list one condition takes: a column pasted from a spreadsheet, not a table.
pub const MAX_VALUES: usize = 200;
/// Values bound in one statement, over all its conditions. Servers take more; a filter that
/// needs more is a join, and belongs in a query tab.
pub const MAX_PARAMS: usize = 1000;

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
    /// The value is one of `values`.
    In,
    NotIn,
    /// Between the two `values`, both included.
    Between,
    /// The empty text. NULL is not empty text: `IsNotEmpty` leaves NULL rows out too.
    IsEmpty,
    IsNotEmpty,
}

/// One condition. The conditions of a request must all hold, or any one of them.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub op: FilterOp,
    /// What the operators that compare with one value compare with.
    #[serde(default)]
    pub value: Option<String>,
    /// The list of `In` / `NotIn` (1 to [`MAX_VALUES`]) and the two ends of `Between`.
    #[serde(default)]
    pub values: Vec<String>,
}

/// A WHERE clause (with its leading space, or empty) and its bind parameters, in order.
/// PostgreSQL placeholders are numbered from `$1`.
#[derive(Debug, Default, PartialEq)]
pub struct Where {
    pub sql: String,
    pub params: Vec<String>,
}

/// `match_any`: a row passes when any one condition holds, instead of all of them.
pub(crate) fn where_clause(
    engine: Engine,
    meta: &TableMeta,
    filters: &[Filter],
    match_any: bool,
) -> Result<Where> {
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
    if params.len() > MAX_PARAMS {
        return Err(Error::Invalid(format!(
            "These conditions hold {} values in all; one filter takes at most {MAX_PARAMS}",
            params.len()
        )));
    }
    let sql = if parts.is_empty() {
        String::new()
    } else if match_any && parts.len() > 1 {
        // Each condition in its own parentheses: what "or" joins never depends on what a
        // condition happens to contain.
        let each: Vec<String> = parts.iter().map(|p| format!("({p})")).collect();
        format!(" WHERE {}", each.join(" OR "))
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    };
    Ok(Where { sql, params })
}

fn bind(engine: Engine, params: &mut Vec<String>, value: String) -> String {
    params.push(value);
    match engine {
        Engine::Postgres => format!("${}", params.len()),
        _ => "?".to_string(),
    }
}

/// The two sides of a comparison of `col` with one value: the column, or the text the grid
/// shows of it, and the bound value. `equality`: the comparison is "is" (or a list of them),
/// which every column that can be compared at all allows.
fn sides(
    engine: Engine,
    col: &ColumnMeta,
    quoted: &str,
    value: &str,
    equality: bool,
    params: &mut Vec<String>,
) -> Result<(String, String)> {
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
    let placeholder = bind(engine, params, value.to_string());
    Ok(if typed {
        (
            quoted.to_string(),
            write::key_expr(engine, col, &placeholder),
        )
    } else {
        (write::rendered(engine, col, quoted), placeholder)
    })
}

pub(crate) fn condition(
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
        IsEmpty | IsNotEmpty => {
            if col.kind != ValueKind::Text {
                return Err(Error::Invalid(format!(
                    "Column {} ({}) does not hold text, so it has no empty text to look for",
                    col.name, col.data_type
                )));
            }
            let test = if filter.op == IsEmpty { "=" } else { "<>" };
            return Ok(format!("{quoted} {test} ''"));
        }
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

    if matches!(filter.op, In | NotIn) {
        if filter.values.is_empty() {
            return Err(Error::Invalid(format!(
                "The list of values for {} is empty",
                col.name
            )));
        }
        if filter.values.len() > MAX_VALUES {
            return Err(Error::Invalid(format!(
                "The list for {} holds {} values; a list takes at most {MAX_VALUES}",
                col.name,
                filter.values.len()
            )));
        }
        let mut left = String::new();
        let mut list = Vec::with_capacity(filter.values.len());
        for value in &filter.values {
            let (column, bound) = sides(engine, col, &quoted, value, true, params)?;
            left = column;
            list.push(bound);
        }
        let list = list.join(", ");
        return Ok(if filter.op == In {
            format!("{left} IN ({list})")
        } else {
            format!("({left} NOT IN ({list}) OR {quoted} IS NULL)")
        });
    }
    if filter.op == Between {
        if !matches!(
            col.kind,
            ValueKind::Number | ValueKind::Temporal | ValueKind::Text
        ) {
            return Err(Error::Invalid(format!(
                "Column {} ({}) has no order for a value to be between two others",
                col.name, col.data_type
            )));
        }
        let [from, to] = filter.values.as_slice() else {
            return Err(Error::Invalid(format!(
                "“is between” on {} needs two values, a first and a last",
                col.name
            )));
        };
        let (left, from) = sides(engine, col, &quoted, from, false, params)?;
        let (_, to) = sides(engine, col, &quoted, to, false, params)?;
        return Ok(format!("{left} BETWEEN {from} AND {to}"));
    }

    let Some(value) = &filter.value else {
        return Err(Error::Invalid(format!(
            "The filter on {} needs a value",
            col.name
        )));
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
        let placeholder = bind(engine, params, pattern);
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
    let (left, right) = sides(engine, col, &quoted, value, equality, params)?;
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
            values: Vec::new(),
        }
    }

    fn list(column: &str, op: FilterOp, values: &[&str]) -> Filter {
        Filter {
            column: column.into(),
            op,
            value: None,
            values: values.iter().map(|v| v.to_string()).collect(),
        }
    }

    fn mysql(filters: &[Filter]) -> Result<Where> {
        where_clause(Engine::Mysql, &meta(), filters, false)
    }

    fn postgres(filters: &[Filter]) -> Result<Where> {
        where_clause(Engine::Postgres, &meta(), filters, false)
    }

    #[test]
    fn sqlite_compares_by_affinity() {
        use FilterOp::*;
        let sqlite = |filters: &[Filter]| where_clause(Engine::Sqlite, &meta(), filters, false);
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
    fn lists_are_so_many_equality_tests() {
        use FilterOp::*;
        let w = mysql(&[
            list("id", In, &["1", " 2 "]),
            list("na`me", NotIn, &["a", "x' OR 1=1 --"]),
            // Equality on a FLOAT, and on JSON, compares the text the grid shows.
            list("ratio", In, &["1.1", "2"]),
            list("doc", NotIn, &["{}"]),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE `id` IN (CAST(? AS UNSIGNED), CAST(? AS UNSIGNED)) \
             AND (`na``me` NOT IN (?, ?) OR `na``me` IS NULL) \
             AND CAST(`ratio` AS CHAR) IN (?, ?) \
             AND (CAST(`doc` AS CHAR) NOT IN (?) OR `doc` IS NULL)"
        );
        assert_eq!(
            w.params,
            ["1", " 2 ", "a", "x' OR 1=1 --", "1.1", "2", "{}"]
        );

        // PostgreSQL numbers its placeholders through every condition and list.
        let w = postgres(&[
            filter("price", Greater, Some("1")),
            list("id", In, &["7", "8", "9"]),
            list("doc", In, &["{}"]),
            list("na`me", NotIn, &["a"]),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE \"price\" > $1::decimal AND \"id\" IN ($2::bigint, $3::bigint, $4::bigint) \
             AND \"doc\"::text IN ($5) \
             AND (\"na`me\" NOT IN ($6::varchar) OR \"na`me\" IS NULL)"
        );
        assert_eq!(w.params, ["1", "7", "8", "9", "{}", "a"]);

        let w = where_clause(
            Engine::Sqlite,
            &meta(),
            &[list("id", In, &["1", "2"])],
            false,
        )
        .unwrap();
        assert_eq!(w.sql, " WHERE \"id\" IN (?, ?)");
    }

    #[test]
    fn between_and_the_empty_text() {
        use FilterOp::*;
        let w = mysql(&[
            list("price", Between, &["0.5", "10"]),
            list("made", Between, &["2026-01-01", "2026-01-31"]),
            list("na`me", Between, &["a", "m"]),
            filter("na`me", IsEmpty, None),
            filter("na`me", IsNotEmpty, Some("ignored")),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE `price` BETWEEN CAST(? AS DECIMAL(65,30)) AND CAST(? AS DECIMAL(65,30)) \
             AND `made` BETWEEN ? AND ? AND `na``me` BETWEEN ? AND ? \
             AND `na``me` = '' AND `na``me` <> ''"
        );
        assert_eq!(
            w.params,
            ["0.5", "10", "2026-01-01", "2026-01-31", "a", "m"]
        );

        let w = postgres(&[
            list("made", Between, &["2026-01-01", "2026-01-31"]),
            filter("na`me", IsEmpty, None),
        ])
        .unwrap();
        assert_eq!(
            w.sql,
            " WHERE \"made\" BETWEEN $1::date AND $2::date AND \"na`me\" = ''"
        );
    }

    #[test]
    fn any_joins_with_or_each_condition_in_parentheses() {
        use FilterOp::*;
        let conditions = [
            list("id", In, &["1", "2"]),
            filter("doc", NotContains, Some("x")),
            filter("na`me", IsNull, None),
        ];
        let w = where_clause(Engine::Postgres, &meta(), &conditions, true).unwrap();
        assert_eq!(
            w.sql,
            " WHERE (\"id\" IN ($1::bigint, $2::bigint)) \
             OR ((\"doc\"::text NOT ILIKE $3 ESCAPE '!' OR \"doc\" IS NULL)) \
             OR (\"na`me\" IS NULL)"
        );
        assert_eq!(w.params, ["1", "2", "%x%"]);
        let w = where_clause(Engine::Mysql, &meta(), &conditions[..2], true).unwrap();
        assert_eq!(
            w.sql,
            " WHERE (`id` IN (CAST(? AS UNSIGNED), CAST(? AS UNSIGNED))) \
             OR ((CAST(`doc` AS CHAR) NOT LIKE ? ESCAPE '!' OR `doc` IS NULL))"
        );
        // One condition is the same either way.
        let one = where_clause(Engine::Mysql, &meta(), &conditions[2..], true).unwrap();
        assert_eq!(one.sql, " WHERE `na``me` IS NULL");
    }

    #[test]
    fn refusals_of_lists_and_ranges() {
        use FilterOp::*;
        let refused = |f: Filter| match mysql(&[f]) {
            Err(Error::Invalid(message)) => message,
            other => panic!("not refused: {other:?}"),
        };
        refused(list("id", In, &[]));
        refused(list("id", NotIn, &[]));
        let many: Vec<String> = (0..=MAX_VALUES).map(|n| n.to_string()).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(mysql(&[list("id", In, &many[..MAX_VALUES])]).is_ok());
        assert!(refused(list("id", In, &many)).contains("201 values"));
        // The value that is not a number, or not a date, is named.
        assert!(refused(list("id", In, &["1", "two", "3"])).contains("“two”"));
        assert!(refused(list("made", NotIn, &["2026-01-02", "2026"])).contains("“2026”"));
        assert!(refused(list("id", Between, &["1", "x"])).contains("“x”"));
        // A list is of the values a column can be compared with at all.
        refused(list("photo", In, &["00"]));
        refused(list("spot", In, &["POINT(1 2)"]));
        // Between takes two values, of a column that has an order.
        for values in [&[][..], &["1"], &["1", "2", "3"]] {
            refused(list("id", Between, values));
        }
        let mut flags = meta();
        flags.columns.push(col("on", "tinyint(1)", ValueKind::Bool));
        assert!(matches!(
            where_clause(
                Engine::Mysql,
                &flags,
                &[list("on", Between, &["0", "1"])],
                false
            ),
            Err(Error::Invalid(_))
        ));
        refused(list("doc", Between, &["a", "b"]));
        refused(list("photo", Between, &["a", "b"]));
        // The empty text is a matter for text columns.
        refused(filter("id", IsEmpty, None));
        refused(filter("made", IsNotEmpty, None));
        refused(filter("photo", IsEmpty, None));

        // All the values of a filter, over its conditions, are bounded too.
        let full = vec![list("id", In, &many[..MAX_VALUES]); 5];
        assert_eq!(mysql(&full).unwrap().params.len(), MAX_PARAMS);
        let mut over = full.clone();
        over.push(filter("id", Equals, Some("1")));
        assert!(refused_all(&over).contains("1001 values"));
    }

    fn refused_all(filters: &[Filter]) -> String {
        match mysql(filters) {
            Err(Error::Invalid(message)) => message,
            other => panic!("not refused: {other:?}"),
        }
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
