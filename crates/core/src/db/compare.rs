//! Structure compare: what one database (or PostgreSQL schema) has that another has not, and
//! how the tables both have differ — columns, indexes, foreign keys. Read-only.
//!
//! Each side is read with a few catalogue queries for the whole schema (`Session::structure`),
//! then compared here, without the server:
//! - tables and views by name;
//! - columns by name, on their type, NULL, default, extras (MySQL `EXTRA`, PostgreSQL identity
//!   or generated), comment and collation;
//! - indexes and foreign keys by what they are (columns, uniqueness, the referenced table and
//!   actions), not by name: servers name them themselves (`orders_ibfk_1`), and two indexes on
//!   the same columns are the same index whatever they are called.
//!
//! MySQL before 8.0.19 writes a display width on integer types (`int(11)`) that later
//! versions leave out; it is ignored, except `tinyint(1)`, which tools read as a boolean.

use serde::Serialize;

/// One side of a compare.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub tables: Vec<TableShape>,
    pub views: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableShape {
    pub name: String,
    pub columns: Vec<ColumnShape>,
    pub indexes: Vec<IndexShape>,
    pub foreign_keys: Vec<KeyShape>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnShape {
    pub name: String,
    pub column_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    /// MySQL's `EXTRA` (auto_increment, on update …); PostgreSQL identity or generated.
    pub extra: String,
    pub comment: Option<String>,
    pub collation: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexShape {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
    /// PostgreSQL: the WHERE of a partial index.
    pub predicate: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyShape {
    pub name: String,
    pub columns: Vec<String>,
    /// The referenced table; with its schema (or database) when that is another one.
    pub references: String,
    pub ref_columns: Vec<String>,
    pub on_delete: String,
    pub on_update: String,
}

/// What differs between side A and side B.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
    /// Tables both have, alike.
    pub same: Vec<String>,
    pub changed: Vec<TableChanges>,
    pub views_only_a: Vec<String>,
    pub views_only_b: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableChanges {
    pub name: String,
    pub columns: Vec<ColumnChange>,
    pub indexes: Vec<OneSide<IndexShape>>,
    pub foreign_keys: Vec<OneSide<KeyShape>>,
}

/// A column on one side only (`a` or `b` is `None`), or on both with `fields` differing.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnChange {
    pub name: String,
    pub a: Option<ColumnShape>,
    pub b: Option<ColumnShape>,
    pub fields: Vec<&'static str>,
}

/// An index or a foreign key that only one side has.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OneSide<T> {
    pub a: Option<T>,
    pub b: Option<T>,
}

/// `int(11)` → `int`; `tinyint(1)` stays.
pub(crate) fn normal_type(column_type: &str) -> String {
    let lower = column_type.trim().to_ascii_lowercase();
    for base in [
        "tinyint",
        "smallint",
        "mediumint",
        "int",
        "integer",
        "bigint",
    ] {
        if let Some(rest) = lower.strip_prefix(base) {
            if let Some(close) = rest
                .strip_prefix('(')
                .and_then(|r| r.find(')').map(|at| (r, at)))
            {
                let (inside, at) = close;
                let width = &inside[..at];
                if width.chars().all(|c| c.is_ascii_digit()) && !(base == "tinyint" && width == "1")
                {
                    return format!("{base}{}", &inside[at + 1..]);
                }
            }
        }
    }
    lower
}

fn column_fields(a: &ColumnShape, b: &ColumnShape) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if normal_type(&a.column_type) != normal_type(&b.column_type) {
        fields.push("type");
    }
    if a.nullable != b.nullable {
        fields.push("nullable");
    }
    if a.default != b.default {
        fields.push("default");
    }
    if !a.extra.eq_ignore_ascii_case(&b.extra) {
        fields.push("extra");
    }
    if a.comment.as_deref().unwrap_or("") != b.comment.as_deref().unwrap_or("") {
        fields.push("comment");
    }
    if a.collation != b.collation {
        fields.push("collation");
    }
    fields
}

fn index_key(i: &IndexShape) -> (Vec<String>, bool, bool, Option<String>) {
    (i.columns.clone(), i.unique, i.primary, i.predicate.clone())
}

fn foreign_key(k: &KeyShape) -> (Vec<String>, String, Vec<String>, String, String) {
    (
        k.columns.clone(),
        k.references.clone(),
        k.ref_columns.clone(),
        k.on_delete.to_ascii_uppercase(),
        k.on_update.to_ascii_uppercase(),
    )
}

/// What each side has that the other has not, by `key`; matched one for one, so two equal
/// indexes on one side and one on the other leave one over.
fn unmatched<T: Clone, K: PartialEq>(a: &[T], b: &[T], key: impl Fn(&T) -> K) -> Vec<OneSide<T>> {
    let mut taken = vec![false; b.len()];
    let mut out = Vec::new();
    for x in a {
        match b
            .iter()
            .enumerate()
            .position(|(i, y)| !taken[i] && key(x) == key(y))
        {
            Some(i) => taken[i] = true,
            None => out.push(OneSide {
                a: Some(x.clone()),
                b: None,
            }),
        }
    }
    out.extend(
        b.iter()
            .zip(&taken)
            .filter(|(_, t)| !**t)
            .map(|(y, _)| OneSide {
                a: None,
                b: Some(y.clone()),
            }),
    );
    out
}

fn table(a: &TableShape, b: &TableShape) -> TableChanges {
    let mut columns = Vec::new();
    for ca in &a.columns {
        match b.columns.iter().find(|cb| cb.name == ca.name) {
            None => columns.push(ColumnChange {
                name: ca.name.clone(),
                a: Some(ca.clone()),
                b: None,
                fields: Vec::new(),
            }),
            Some(cb) => {
                let fields = column_fields(ca, cb);
                if !fields.is_empty() {
                    columns.push(ColumnChange {
                        name: ca.name.clone(),
                        a: Some(ca.clone()),
                        b: Some(cb.clone()),
                        fields,
                    });
                }
            }
        }
    }
    columns.extend(
        b.columns
            .iter()
            .filter(|cb| !a.columns.iter().any(|ca| ca.name == cb.name))
            .map(|cb| ColumnChange {
                name: cb.name.clone(),
                a: None,
                b: Some(cb.clone()),
                fields: Vec::new(),
            }),
    );
    TableChanges {
        name: a.name.clone(),
        columns,
        indexes: unmatched(&a.indexes, &b.indexes, index_key),
        foreign_keys: unmatched(&a.foreign_keys, &b.foreign_keys, foreign_key),
    }
}

pub fn compare(a: &Shape, b: &Shape) -> Comparison {
    let mut out = Comparison::default();
    for ta in &a.tables {
        match b.tables.iter().find(|tb| tb.name == ta.name) {
            None => out.only_a.push(ta.name.clone()),
            Some(tb) => {
                let changes = table(ta, tb);
                if changes.columns.is_empty()
                    && changes.indexes.is_empty()
                    && changes.foreign_keys.is_empty()
                {
                    out.same.push(ta.name.clone());
                } else {
                    out.changed.push(changes);
                }
            }
        }
    }
    out.only_b = b
        .tables
        .iter()
        .filter(|tb| !a.tables.iter().any(|ta| ta.name == tb.name))
        .map(|t| t.name.clone())
        .collect();
    out.views_only_a = a
        .views
        .iter()
        .filter(|v| !b.views.contains(v))
        .cloned()
        .collect();
    out.views_only_b = b
        .views
        .iter()
        .filter(|v| !a.views.contains(v))
        .cloned()
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, column_type: &str) -> ColumnShape {
        ColumnShape {
            name: name.into(),
            column_type: column_type.into(),
            nullable: true,
            ..Default::default()
        }
    }

    fn index(name: &str, columns: &[&str], unique: bool) -> IndexShape {
        IndexShape {
            name: name.into(),
            columns: columns.iter().map(|c| c.to_string()).collect(),
            unique,
            primary: name == "PRIMARY",
            predicate: None,
        }
    }

    fn shape(tables: Vec<TableShape>, views: &[&str]) -> Shape {
        Shape {
            tables,
            views: views.iter().map(|v| v.to_string()).collect(),
        }
    }

    #[test]
    fn integer_display_widths_are_not_a_difference() {
        assert_eq!(normal_type("int(11)"), "int");
        assert_eq!(normal_type("INT(10) UNSIGNED"), "int unsigned");
        assert_eq!(
            normal_type("bigint(20) unsigned zerofill"),
            "bigint unsigned zerofill"
        );
        assert_eq!(normal_type("tinyint(1)"), "tinyint(1)");
        assert_eq!(normal_type("tinyint(4)"), "tinyint");
        assert_eq!(normal_type("varchar(20)"), "varchar(20)");
        assert_eq!(normal_type("integer"), "integer");
        assert_eq!(normal_type("interval"), "interval");
    }

    #[test]
    fn tables_columns_indexes_and_keys_are_compared() {
        let key = |name: &str, refs: &str| KeyShape {
            name: name.into(),
            columns: vec!["customer_id".into()],
            references: refs.into(),
            ref_columns: vec!["id".into()],
            on_delete: "CASCADE".into(),
            on_update: "NO ACTION".into(),
        };
        let a = shape(
            vec![
                TableShape {
                    name: "orders".into(),
                    columns: vec![
                        col("id", "int(11)"),
                        ColumnShape {
                            nullable: false,
                            ..col("total", "decimal(12,2)")
                        },
                        ColumnShape {
                            default: Some("new".into()),
                            ..col("status", "varchar(20)")
                        },
                        col("only_here", "text"),
                    ],
                    indexes: vec![
                        index("PRIMARY", &["id"], true),
                        index("by_status", &["status"], false),
                    ],
                    foreign_keys: vec![key("orders_ibfk_1", "customers")],
                },
                TableShape {
                    name: "same".into(),
                    columns: vec![col("id", "int")],
                    ..Default::default()
                },
                TableShape {
                    name: "only_a".into(),
                    ..Default::default()
                },
            ],
            &["v_both", "v_a"],
        );
        let b = shape(
            vec![
                TableShape {
                    name: "orders".into(),
                    columns: vec![
                        col("id", "int"),
                        col("total", "decimal(14,2)"),
                        ColumnShape {
                            default: Some("new".into()),
                            comment: Some("".into()),
                            ..col("status", "varchar(20)")
                        },
                        col("only_there", "int"),
                    ],
                    // The same index under another name is the same index.
                    indexes: vec![
                        index("PRIMARY", &["id"], true),
                        index("status_idx", &["status"], false),
                        index("u_total", &["total"], true),
                    ],
                    foreign_keys: vec![key("fk_customer", "customers")],
                },
                TableShape {
                    name: "same".into(),
                    columns: vec![col("id", "int")],
                    ..Default::default()
                },
                TableShape {
                    name: "only_b".into(),
                    ..Default::default()
                },
            ],
            &["v_both", "v_b"],
        );
        let c = compare(&a, &b);
        assert_eq!(
            (c.only_a.as_slice(), c.only_b.as_slice(), c.same.as_slice()),
            (
                &["only_a".to_string()][..],
                &["only_b".to_string()][..],
                &["same".to_string()][..]
            )
        );
        assert_eq!(
            (c.views_only_a.clone(), c.views_only_b.clone()),
            (vec!["v_a".to_string()], vec!["v_b".to_string()])
        );
        let [orders] = c.changed.as_slice() else {
            panic!("{c:?}")
        };
        let cols: Vec<(&str, bool, bool, Vec<&str>)> = orders
            .columns
            .iter()
            .map(|x| {
                (
                    x.name.as_str(),
                    x.a.is_some(),
                    x.b.is_some(),
                    x.fields.clone(),
                )
            })
            .collect();
        assert_eq!(
            cols,
            [
                ("total", true, true, vec!["type", "nullable"]),
                ("only_here", true, false, vec![]),
                ("only_there", false, true, vec![])
            ]
        );
        assert_eq!(orders.indexes.len(), 1);
        assert_eq!(
            orders.indexes[0].b.as_ref().map(|i| i.name.as_str()),
            Some("u_total")
        );
        assert!(orders.foreign_keys.is_empty(), "{:?}", orders.foreign_keys);
    }

    #[test]
    fn a_key_with_another_action_is_another_key() {
        let k = |on_delete: &str| KeyShape {
            columns: vec!["c".into()],
            references: "t".into(),
            ref_columns: vec!["id".into()],
            on_delete: on_delete.into(),
            on_update: "NO ACTION".into(),
            ..Default::default()
        };
        let a = shape(
            vec![TableShape {
                name: "x".into(),
                foreign_keys: vec![k("CASCADE")],
                ..Default::default()
            }],
            &[],
        );
        let b = shape(
            vec![TableShape {
                name: "x".into(),
                foreign_keys: vec![k("SET NULL")],
                ..Default::default()
            }],
            &[],
        );
        let c = compare(&a, &b);
        assert_eq!(c.changed[0].foreign_keys.len(), 2);
        assert!(compare(&a, &a).changed.is_empty());
    }
}
