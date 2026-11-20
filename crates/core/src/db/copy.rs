//! Copying rows from one table into another: on the same connection or another, of the same
//! engine or not. The target table exists; its columns are paired with the source's by the
//! user (by name to begin with).
//!
//! The source is read a page at a time, each column as the text the server renders for it,
//! whole (a grid page shortens long values; this does not) and with NULL kept. Each page is
//! then what a pasted block is to Smart Paste: it goes through the target's
//! [`Session::import_batch`], so the matching on a chosen key, the per-row statuses, the
//! choice between stopping and leaving a row out, and the report of what was not written are
//! those of a large import (ADR 0014, ADR 0027).

use serde::Serialize;

use super::smart::{Analysis, BatchOutcome, OnProblem, PasteRequest};
use super::{write, ColumnMeta, Session, TableMeta, TableRef, ValueKind};
use crate::model::Engine;
use crate::{Error, Result};

/// The most rows read in one page.
pub const MAX_PAGE: usize = 5_000;

/// What one page of a copy did.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CopyBatch {
    /// Rows read from the source; fewer than asked for means the source has no more.
    pub read: usize,
    pub outcome: BatchOutcome,
    /// The source values of each row in `outcome.failed`, in the same order.
    pub failed_rows: Vec<Vec<Option<String>>>,
}

/// The first page of a copy, analysed against the target without writing.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CopyPreview {
    pub analysis: Analysis,
    /// The source rows the analysis is about, in its order.
    pub rows: Vec<Vec<Option<String>>>,
}

/// A column whose value can be copied as text: not binary data, and not a MySQL spatial value
/// (its text is not what the column takes back).
pub fn copyable(engine: Engine, col: &ColumnMeta) -> bool {
    !(col.kind == ValueKind::Binary || (engine == Engine::Mysql && col.kind == ValueKind::Other))
}

/// One page of the source: the columns rendered as text, ordered by the primary key (or by
/// every column read, when there is none).
pub(crate) fn page_sql(
    engine: Engine,
    table: &TableRef,
    meta: &TableMeta,
    columns: &[String],
    offset: u64,
    limit: usize,
) -> Result<String> {
    if columns.is_empty() {
        return Err(Error::Invalid("Choose at least one column to copy".into()));
    }
    if limit == 0 || limit > MAX_PAGE {
        return Err(Error::Invalid(format!(
            "A page of a copy holds 1 to {MAX_PAGE} rows"
        )));
    }
    let mut select = Vec::with_capacity(columns.len());
    for name in columns {
        let col = write::column(meta, name)?;
        if !copyable(engine, col) {
            return Err(Error::Invalid(format!(
                "Column {name} ({}) holds binary data, which is not copied",
                col.data_type
            )));
        }
        select.push(write::rendered(
            engine,
            col,
            &write::quote(engine, &col.name)?,
        ));
    }
    let order = if meta.primary_key.is_empty() {
        (1..=select.len())
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
    } else {
        meta.primary_key
            .iter()
            .map(|k| write::quote(engine, k))
            .collect::<Result<Vec<_>>>()?
    };
    Ok(format!(
        "SELECT {} FROM {} ORDER BY {} LIMIT {limit} OFFSET {offset}",
        select.join(", "),
        write::table_name(engine, table)?,
        order.join(", ")
    ))
}

/// The request for the target, with the rows read from the source.
fn with_rows(
    request: &PasteRequest,
    columns: &[String],
    rows: Vec<Vec<Option<String>>>,
) -> Result<PasteRequest> {
    if request.columns.len() != columns.len() {
        return Err(Error::Invalid(
            "Each source column must be paired with one target column".into(),
        ));
    }
    Ok(PasteRequest {
        rows,
        skip_problems: false,
        ..request.clone()
    })
}

/// The first `limit` rows of the source, analysed against the target as a paste. Writes nothing.
#[allow(clippy::too_many_arguments)]
pub async fn preview(
    source: &Session,
    from: &TableRef,
    columns: &[String],
    target: &Session,
    to: &TableRef,
    request: &PasteRequest,
    limit: usize,
) -> Result<CopyPreview> {
    let rows = source.read_text_page(from, columns, 0, limit).await?;
    if rows.is_empty() {
        return Err(Error::Invalid(format!(
            "Table {} has no rows to copy",
            from.name
        )));
    }
    let analysis = target
        .analyze_paste(to, &with_rows(request, columns, rows.clone())?)
        .await?;
    Ok(CopyPreview { analysis, rows })
}

/// One page of a copy: rows `offset..offset + limit` of the source, written to the target as
/// one batch of a large import. `stop` stops it as it stops a batch.
#[allow(clippy::too_many_arguments)]
pub async fn batch(
    source: &Session,
    from: &TableRef,
    columns: &[String],
    target: &Session,
    to: &TableRef,
    request: &PasteRequest,
    on_problem: OnProblem,
    offset: u64,
    limit: usize,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<CopyBatch> {
    let rows = source.read_text_page(from, columns, offset, limit).await?;
    if rows.is_empty() {
        return Ok(CopyBatch {
            read: 0,
            outcome: BatchOutcome::default(),
            failed_rows: Vec::new(),
        });
    }
    let request = with_rows(request, columns, rows.clone())?;
    let outcome = target.import_batch(to, &request, on_problem, stop).await?;
    let failed_rows = outcome
        .failed
        .iter()
        .map(|f| rows.get(f.row).cloned().unwrap_or_default())
        .collect();
    Ok(CopyBatch {
        read: rows.len(),
        outcome,
        failed_rows,
    })
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

    fn meta(key: &[&str]) -> TableMeta {
        TableMeta {
            columns: vec![
                col("id", "int", ValueKind::Number),
                col("name", "varchar(20)", ValueKind::Text),
                col("photo", "blob", ValueKind::Binary),
                col("spot", "point", ValueKind::Other),
            ],
            primary_key: key.iter().map(|k| k.to_string()).collect(),
            estimated_rows: None,
        }
    }

    fn table(schema: Option<&str>) -> TableRef {
        TableRef {
            database: "shop".into(),
            schema: schema.map(str::to_string),
            name: "people".into(),
        }
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_page_is_text_in_key_order() {
        assert_eq!(
            page_sql(Engine::Mysql, &table(None), &meta(&["id"]), &names(&["name", "id"]), 2000, 1000).unwrap(),
            "SELECT CAST(`name` AS CHAR), CAST(`id` AS CHAR) FROM `shop`.`people` ORDER BY `id` LIMIT 1000 OFFSET 2000"
        );
        assert_eq!(
            page_sql(
                Engine::Postgres,
                &table(Some("public")),
                &meta(&["id"]),
                &names(&["name"]),
                0,
                10
            )
            .unwrap(),
            "SELECT \"name\"::text FROM \"public\".\"people\" ORDER BY \"id\" LIMIT 10 OFFSET 0"
        );
    }

    #[test]
    fn without_a_key_every_column_read_orders_the_pages() {
        assert_eq!(
            page_sql(Engine::Sqlite, &table(None), &meta(&[]), &names(&["id", "name"]), 0, 5).unwrap(),
            "SELECT CAST(\"id\" AS TEXT), CAST(\"name\" AS TEXT) FROM \"shop\".\"people\" ORDER BY 1, 2 LIMIT 5 OFFSET 0"
        );
    }

    #[test]
    fn what_a_page_cannot_be() {
        let m = meta(&["id"]);
        let t = table(None);
        assert!(page_sql(Engine::Mysql, &t, &m, &[], 0, 10).is_err());
        assert!(page_sql(Engine::Mysql, &t, &m, &names(&["id"]), 0, 0).is_err());
        assert!(page_sql(Engine::Mysql, &t, &m, &names(&["id"]), 0, MAX_PAGE + 1).is_err());
        assert!(page_sql(Engine::Mysql, &t, &m, &names(&["nope"]), 0, 10).is_err());
        let binary = page_sql(Engine::Mysql, &t, &m, &names(&["photo"]), 0, 10)
            .unwrap_err()
            .to_string();
        assert!(binary.contains("binary"), "{binary}");
        assert!(page_sql(Engine::Mysql, &t, &m, &names(&["spot"]), 0, 10).is_err());
        // A PostgreSQL value of another type is text the target can read.
        let pg = TableMeta {
            columns: vec![col("u", "uuid", ValueKind::Other)],
            primary_key: vec![],
            estimated_rows: None,
        };
        assert!(page_sql(
            Engine::Postgres,
            &table(Some("public")),
            &pg,
            &names(&["u"]),
            0,
            10
        )
        .is_ok());
    }
}
