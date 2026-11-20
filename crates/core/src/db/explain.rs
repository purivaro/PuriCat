//! EXPLAIN for one statement: the server's own plan, as text, with the places where it reads
//! a whole table pointed out.
//!
//! The plan is the server's: MySQL's `EXPLAIN FORMAT=TREE` (the classic table on a server
//! without it), PostgreSQL's text plan, SQLite's `EXPLAIN QUERY PLAN` as an indented tree.
//! "Run and measure" (`EXPLAIN ANALYZE`) runs the statement, so it is offered for a statement
//! that only reads, and not on SQLite, which has none.

use serde::Serialize;

use super::console::ResultSet;
use super::script::{self, Kind};
use crate::model::Engine;
use crate::{Error, Result};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The statement that was explained.
    pub statement: String,
    /// The plan as the server gave it, one step a line.
    pub text: String,
    /// The lines (or, for MySQL's table, the tables) where a whole table is read.
    pub scans: Vec<String>,
    /// The statement ran: the plan carries real times and rows.
    pub analyzed: bool,
    pub elapsed_ms: u64,
}

/// The one statement to explain, without its closing semicolon.
pub(crate) fn statement(engine: Engine, sql: &str, analyze: bool) -> Result<String> {
    let statements = script::split(engine, sql);
    let [only] = statements.as_slice() else {
        return Err(Error::Invalid(if statements.is_empty() {
            "There is no statement to explain".into()
        } else {
            "Explain one statement at a time: put the cursor in it, or select it".into()
        }));
    };
    if analyze {
        if engine == Engine::Sqlite {
            return Err(Error::Unsupported(
                "SQLite gives a plan without running the statement only".into(),
            ));
        }
        if only.kind != Kind::Read {
            return Err(Error::Invalid(
                "Only a statement that reads can be run and measured: EXPLAIN ANALYZE would make its changes too".into(),
            ));
        }
    }
    Ok(only
        .text
        .trim()
        .trim_end_matches(';')
        .trim_end()
        .to_string())
}

/// The EXPLAIN statement. `classic`: MySQL's table, for a server without `FORMAT=TREE`.
pub(crate) fn explain_sql(engine: Engine, statement: &str, analyze: bool, classic: bool) -> String {
    match (engine, analyze) {
        (Engine::Mysql, true) => format!("EXPLAIN ANALYZE {statement}"),
        (Engine::Mysql, false) if classic => format!("EXPLAIN {statement}"),
        (Engine::Mysql, false) => format!("EXPLAIN FORMAT=TREE {statement}"),
        (Engine::Postgres, true) => format!("EXPLAIN (ANALYZE, BUFFERS) {statement}"),
        (Engine::Sqlite, _) => format!("EXPLAIN QUERY PLAN {statement}"),
        _ => format!("EXPLAIN {statement}"),
    }
}

/// The plan as text, and where it reads whole tables.
pub(crate) fn read(engine: Engine, set: &ResultSet, classic: bool) -> (String, Vec<String>) {
    let cell =
        |row: &Vec<Option<String>>, i: usize| row.get(i).cloned().flatten().unwrap_or_default();
    match engine {
        Engine::Sqlite => {
            // id, parent, notused, detail: a tree by parent.
            let rows: Vec<(String, String, String)> = set
                .rows
                .iter()
                .map(|r| (cell(r, 0), cell(r, 1), cell(r, 3)))
                .collect();
            let mut text = Vec::new();
            fn walk(
                rows: &[(String, String, String)],
                parent: &str,
                depth: usize,
                out: &mut Vec<String>,
            ) {
                for (id, p, detail) in rows.iter().filter(|(_, p, _)| p == parent) {
                    out.push(format!("{}{detail}", "  ".repeat(depth)));
                    if id != p {
                        walk(rows, id, depth + 1, out);
                    }
                }
            }
            walk(&rows, "0", 0, &mut text);
            let scans = rows
                .iter()
                .filter(|(_, _, d)| d.starts_with("SCAN ") && !d.contains(" USING "))
                .map(|(_, _, d)| d.clone())
                .collect();
            (text.join("\n"), scans)
        }
        Engine::Mysql if classic => {
            let names: Vec<&str> = set.columns.iter().map(|c| c.name.as_str()).collect();
            let at = |name: &str| names.iter().position(|n| n.eq_ignore_ascii_case(name));
            let grid: Vec<Vec<String>> =
                std::iter::once(names.iter().map(|n| n.to_string()).collect())
                    .chain(
                        set.rows
                            .iter()
                            .map(|r| (0..names.len()).map(|i| cell(r, i)).collect()),
                    )
                    .collect();
            let widths: Vec<usize> = (0..names.len())
                .map(|i| grid.iter().map(|r| r[i].chars().count()).max().unwrap_or(0))
                .collect();
            let text = grid
                .iter()
                .map(|r| {
                    r.iter()
                        .zip(&widths)
                        .map(|(c, w)| format!("{c:<w$}"))
                        .collect::<Vec<_>>()
                        .join("  ")
                        .trim_end()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            let scans = match (at("type"), at("table")) {
                (Some(t), Some(name)) => set
                    .rows
                    .iter()
                    .filter(|r| cell(r, t).eq_ignore_ascii_case("ALL"))
                    .map(|r| cell(r, name))
                    .collect(),
                _ => Vec::new(),
            };
            (text, scans)
        }
        _ => {
            // MySQL's tree is one cell of lines; PostgreSQL's plan one line a row.
            let text = set
                .rows
                .iter()
                .map(|r| cell(r, 0))
                .collect::<Vec<_>>()
                .join("\n");
            let marker = if engine == Engine::Mysql {
                "Table scan on "
            } else {
                "Seq Scan on "
            };
            let scans = text
                .lines()
                .filter(|l| l.contains(marker))
                .map(|l| l.trim().trim_start_matches("->").trim().to_string())
                .collect();
            (text, scans)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::console::ResultColumn;
    use crate::db::ValueKind;

    fn set(columns: &[&str], rows: &[&[&str]]) -> ResultSet {
        ResultSet {
            columns: columns
                .iter()
                .map(|c| ResultColumn {
                    name: c.to_string(),
                    type_name: "TEXT".into(),
                    kind: ValueKind::Text,
                })
                .collect(),
            rows: rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| (*c != "NULL").then(|| c.to_string()))
                        .collect()
                })
                .collect(),
        }
    }

    #[test]
    fn one_statement_is_explained_and_only_a_read_is_run() {
        assert_eq!(
            statement(Engine::Mysql, "  SELECT 1 ;", false).unwrap(),
            "SELECT 1"
        );
        assert!(statement(Engine::Mysql, "SELECT 1; SELECT 2", false).is_err());
        assert!(statement(Engine::Mysql, "-- nothing", false).is_err());
        assert_eq!(
            statement(Engine::Postgres, "DELETE FROM t", false).unwrap(),
            "DELETE FROM t"
        );
        let analyze_delete = statement(Engine::Postgres, "DELETE FROM t", true)
            .unwrap_err()
            .to_string();
        assert!(analyze_delete.contains("changes"), "{analyze_delete}");
        assert!(statement(Engine::Sqlite, "SELECT 1", true).is_err());
        assert_eq!(
            explain_sql(Engine::Mysql, "SELECT 1", false, false),
            "EXPLAIN FORMAT=TREE SELECT 1"
        );
        assert_eq!(
            explain_sql(Engine::Mysql, "SELECT 1", false, true),
            "EXPLAIN SELECT 1"
        );
        assert_eq!(
            explain_sql(Engine::Mysql, "SELECT 1", true, false),
            "EXPLAIN ANALYZE SELECT 1"
        );
        assert_eq!(
            explain_sql(Engine::Postgres, "SELECT 1", true, false),
            "EXPLAIN (ANALYZE, BUFFERS) SELECT 1"
        );
        assert_eq!(
            explain_sql(Engine::Postgres, "SELECT 1", false, false),
            "EXPLAIN SELECT 1"
        );
        assert_eq!(
            explain_sql(Engine::Sqlite, "SELECT 1", false, false),
            "EXPLAIN QUERY PLAN SELECT 1"
        );
    }

    #[test]
    fn whole_table_reads_are_pointed_out() {
        let tree = set(&["EXPLAIN"], &[&["-> Filter: (o.total > 10)  (cost=128 rows=416)\n    -> Table scan on o  (cost=128 rows=1250)"]]);
        let (text, scans) = read(Engine::Mysql, &tree, false);
        assert!(text.starts_with("-> Filter"));
        assert_eq!(scans, ["Table scan on o  (cost=128 rows=1250)"]);

        let pg = set(
            &["QUERY PLAN"],
            &[
                &["Seq Scan on orders  (cost=0.00..25.00 rows=10 width=8)"],
                &["  Filter: (total > 10)"],
            ],
        );
        assert_eq!(
            read(Engine::Postgres, &pg, false).1,
            ["Seq Scan on orders  (cost=0.00..25.00 rows=10 width=8)"]
        );

        let lite = set(
            &["id", "parent", "notused", "detail"],
            &[
                &["2", "0", "0", "SCAN t"],
                &["5", "0", "0", "SEARCH u USING INDEX u_k (k=?)"],
                &["7", "5", "0", "USE TEMP B-TREE FOR ORDER BY"],
            ],
        );
        let (text, scans) = read(Engine::Sqlite, &lite, false);
        assert_eq!(
            text,
            "SCAN t\nSEARCH u USING INDEX u_k (k=?)\n  USE TEMP B-TREE FOR ORDER BY"
        );
        assert_eq!(scans, ["SCAN t"]);

        let classic = set(
            &["id", "select_type", "table", "type", "key"],
            &[
                &["1", "SIMPLE", "orders", "ALL", "NULL"],
                &["1", "SIMPLE", "c", "eq_ref", "PRIMARY"],
            ],
        );
        let (text, scans) = read(Engine::Mysql, &classic, true);
        assert_eq!(
            text.lines().next().unwrap(),
            "id  select_type  table   type    key"
        );
        assert_eq!(scans, ["orders"]);
    }
}
