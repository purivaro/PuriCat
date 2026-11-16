//! Capability-aware database adapters.
//!
//! Each engine gets its own module with its own SQL; nothing here rewrites one dialect
//! into another. `Session` is a plain enum so adding an engine is a compile-time checklist.

pub mod ident;
mod mysql;
mod postgres;
pub mod write;

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::model::{ConnectionProfile, Engine};
use crate::{Error, Result};

/// Longest text prefix sent to the grid for one cell.
pub const MAX_CELL_CHARS: usize = 1000;
/// Longest binary prefix (bytes) sent to the grid for one cell, shown as hex.
pub const MAX_BINARY_BYTES: usize = 24;
pub const MAX_PAGE_SIZE: u32 = 1000;

/// What an engine can do. The UI asks instead of guessing from the engine name.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Objects live in schemas inside a database (PostgreSQL) rather than directly in it.
    pub schemas: bool,
    /// DDL participates in transactions and can be rolled back.
    pub transactional_ddl: bool,
    /// `INSERT/UPDATE/DELETE ... RETURNING` is available.
    pub returning: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub engine: Engine,
    pub version: String,
    pub capabilities: Capabilities,
    /// Whether this session is encrypted, when the engine can report it.
    pub tls: Option<bool>,
    /// The server's own answer to "does this session reject writes?".
    pub read_only: bool,
    pub connect_ms: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Namespace {
    pub name: String,
    /// Engine-owned catalogue (information_schema, pg_catalog, ...).
    pub system: bool,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ObjectKind {
    Table,
    View,
    Matview,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbObject {
    pub name: String,
    pub kind: ObjectKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableRef {
    pub database: String,
    /// Required for engines with `capabilities.schemas`.
    #[serde(default)]
    pub schema: Option<String>,
    pub name: String,
}

/// Coarse value family, used for alignment and (later) editors.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ValueKind {
    Number,
    Text,
    Bool,
    Temporal,
    Binary,
    Json,
    Other,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    pub name: String,
    /// Type as the server spells it, e.g. `varchar(255)` or `numeric(12,2)`.
    pub data_type: String,
    pub kind: ValueKind,
    pub nullable: bool,
    pub default: Option<String>,
    pub primary_key: bool,
    /// Value is produced by the server (identity, auto-increment, generated column).
    pub generated: bool,
    /// The grid may write this column (not generated, binary or spatial).
    pub editable: bool,
    /// Type name without length/precision (`character varying`, `numeric`, `bigint`).
    /// Used for casts: casting to `varchar(5)` would silently truncate, whereas casting to
    /// the bare type lets the column assignment enforce its own limits.
    #[serde(skip_serializing)]
    pub base_type: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TableMeta {
    pub columns: Vec<ColumnMeta>,
    /// Primary key columns in key order. Empty when the table has none.
    pub primary_key: Vec<String>,
    /// Planner statistics, not an exact count.
    pub estimated_rows: Option<i64>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SortSpec {
    pub column: String,
    #[serde(default)]
    pub descending: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PageRequest {
    pub offset: u64,
    pub limit: u32,
    #[serde(default)]
    pub sort: Option<SortSpec>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RowPage {
    pub columns: Vec<ColumnMeta>,
    /// Row-major cells as text exactly as the server renders them; `null` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// `[row, column]` of cells whose value was cut to the preview length.
    pub truncated: Vec<[u32; 2]>,
    pub offset: u64,
    pub has_more: bool,
    /// Primary key columns in key order. Empty when the table has none.
    pub primary_key: Vec<String>,
    /// Planner statistics, not an exact count.
    pub estimated_rows: Option<i64>,
    /// Columns the page is ordered by. Empty means the server's arbitrary order.
    pub ordered_by: Vec<String>,
    pub elapsed_ms: u64,
}

/// An open, pooled connection to one server.
pub enum Session {
    MySql(mysql::MySqlSession),
    Postgres(Box<postgres::PgSession>),
}

impl Session {
    /// Connect and verify the credentials by asking the server for its version.
    pub async fn connect(profile: &ConnectionProfile, password: Option<&str>) -> Result<Session> {
        match profile.engine {
            Engine::Mysql => Ok(Session::MySql(
                mysql::MySqlSession::connect(profile, password).await?,
            )),
            Engine::Postgres => Ok(Session::Postgres(Box::new(
                postgres::PgSession::connect(profile, password).await?,
            ))),
            Engine::Sqlserver => Err(Error::Unsupported(
                "SQL Server connections are not implemented yet".into(),
            )),
        }
    }

    pub fn capabilities(&self) -> Capabilities {
        match self {
            Session::MySql(_) => mysql::CAPABILITIES,
            Session::Postgres(_) => postgres::CAPABILITIES,
        }
    }

    pub async fn server_info(&self) -> Result<ServerInfo> {
        let started = Instant::now();
        let (engine, version, tls, read_only) = match self {
            Session::MySql(s) => (
                Engine::Mysql,
                s.version().await?,
                s.tls().await?,
                s.read_only().await?,
            ),
            Session::Postgres(s) => (
                Engine::Postgres,
                s.version().await?,
                s.tls().await?,
                s.read_only().await?,
            ),
        };
        Ok(ServerInfo {
            engine,
            version,
            capabilities: self.capabilities(),
            tls,
            read_only,
            connect_ms: started.elapsed().as_millis() as u64,
        })
    }

    pub async fn list_databases(&self) -> Result<Vec<Namespace>> {
        match self {
            Session::MySql(s) => s.list_databases().await,
            Session::Postgres(s) => s.list_databases().await,
        }
    }

    pub async fn list_schemas(&self, database: &str) -> Result<Vec<Namespace>> {
        match self {
            Session::MySql(_) => Err(Error::Unsupported(
                "MySQL has no schemas inside a database".into(),
            )),
            Session::Postgres(s) => s.list_schemas(database).await,
        }
    }

    pub async fn list_objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<DbObject>> {
        match self {
            Session::MySql(s) => s.list_objects(database).await,
            Session::Postgres(s) => s.list_objects(database, require_schema(schema)?).await,
        }
    }

    pub async fn describe_table(&self, table: &TableRef) -> Result<TableMeta> {
        match self {
            Session::MySql(s) => s.describe_table(table).await,
            Session::Postgres(s) => s.describe_table(table).await,
        }
    }

    /// Read one page of a table. Read-only by construction: only `SELECT` is issued.
    pub async fn fetch_rows(&self, table: &TableRef, req: &PageRequest) -> Result<RowPage> {
        if req.limit == 0 || req.limit > MAX_PAGE_SIZE {
            return Err(Error::Invalid(format!(
                "Page size must be between 1 and {MAX_PAGE_SIZE}"
            )));
        }
        let started = Instant::now();
        let meta = self.describe_table(table).await?;
        if meta.columns.is_empty() {
            return Err(Error::NotFound(format!("Table {}", table.name)));
        }
        let order = resolve_order(&meta, req.sort.as_ref())?;
        let raw = match self {
            Session::MySql(s) => s.select_page(table, &meta, &order, req).await?,
            Session::Postgres(s) => s.select_page(table, &meta, &order, req).await?,
        };
        Ok(finish_page(meta, raw, order, req, started))
    }

    /// The statements a change set would run, without running them.
    pub async fn plan_changes(
        &self,
        table: &TableRef,
        changes: &write::ChangeSet,
    ) -> Result<Vec<write::Statement>> {
        let meta = self.describe_table(table).await?;
        if meta.columns.is_empty() {
            return Err(Error::NotFound(format!("Table {}", table.name)));
        }
        let engine = match self {
            Session::MySql(_) => Engine::Mysql,
            Session::Postgres(_) => Engine::Postgres,
        };
        write::plan(engine, table, &meta, changes)
    }

    /// Run a change set in one transaction. Either every row change is applied, or none.
    pub async fn apply_changes(
        &self,
        table: &TableRef,
        changes: &write::ChangeSet,
    ) -> Result<write::ApplyResult> {
        let started = Instant::now();
        let statements = self.plan_changes(table, changes).await?;
        match self {
            Session::MySql(s) => s.apply(&statements).await?,
            Session::Postgres(s) => s.apply(&table.database, &statements).await?,
        }
        Ok(write::tally(
            &statements,
            started.elapsed().as_millis() as u64,
        ))
    }

    pub async fn close(&self) {
        match self {
            Session::MySql(s) => s.close().await,
            Session::Postgres(s) => s.close().await,
        }
    }
}

/// Open one connection within `timeout`, report the driver's own error, and close it.
async fn probe<C, F>(timeout: std::time::Duration, connecting: F) -> Result<()>
where
    C: sqlx::Connection,
    F: std::future::Future<Output = std::result::Result<C, sqlx::Error>>,
{
    match tokio::time::timeout(timeout, connecting).await {
        Ok(Ok(conn)) => {
            let _ = conn.close().await;
            Ok(())
        }
        Ok(Err(e)) => Err(Error::database(e)),
        Err(_) => Err(Error::Database(format!(
            "No response from the server after {} seconds",
            timeout.as_secs()
        ))),
    }
}

fn require_schema(schema: Option<&str>) -> Result<&str> {
    schema
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Invalid("A schema is required for this engine".into()))
}

/// `(column, descending)` pairs for ORDER BY.
pub(crate) type Order = Vec<(String, bool)>;

/// A caller-chosen sort column must exist; otherwise page by primary key so that
/// OFFSET paging is stable. Without either, the order is the server's.
fn resolve_order(meta: &TableMeta, sort: Option<&SortSpec>) -> Result<Order> {
    let mut order: Order = Vec::new();
    if let Some(sort) = sort {
        if !meta.columns.iter().any(|c| c.name == sort.column) {
            return Err(Error::Invalid(format!(
                "Unknown sort column: {}",
                sort.column
            )));
        }
        order.push((sort.column.clone(), sort.descending));
    }
    for pk in &meta.primary_key {
        if !order.iter().any(|(c, _)| c == pk) {
            order.push((pk.clone(), false));
        }
    }
    Ok(order)
}

/// Apply the preview limits and detect whether another page exists.
fn finish_page(
    meta: TableMeta,
    mut rows: Vec<Vec<Option<String>>>,
    order: Order,
    req: &PageRequest,
    started: Instant,
) -> RowPage {
    // Adapters fetch `limit + 1` rows.
    let has_more = rows.len() > req.limit as usize;
    rows.truncate(req.limit as usize);

    let mut truncated = Vec::new();
    for (r, row) in rows.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            let Some(text) = cell else { continue };
            let binary = meta.columns[c].kind == ValueKind::Binary;
            // Adapters fetch one unit more than the limit so truncation is detectable.
            let limit = if binary {
                MAX_BINARY_BYTES * 2
            } else {
                MAX_CELL_CHARS
            };
            if let Some((cut, _)) = text.char_indices().nth(limit) {
                text.truncate(cut);
                truncated.push([r as u32, c as u32]);
            }
            if binary {
                text.insert_str(0, "0x");
            }
        }
    }

    RowPage {
        columns: meta.columns,
        rows,
        truncated,
        primary_key: meta.primary_key,
        estimated_rows: meta.estimated_rows,
        offset: req.offset,
        has_more,
        ordered_by: order.into_iter().map(|(c, _)| c).collect(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, kind: ValueKind, pk: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            data_type: "x".into(),
            kind,
            nullable: true,
            default: None,
            primary_key: pk,
            generated: false,
            editable: true,
            base_type: "x".into(),
        }
    }

    fn meta() -> TableMeta {
        TableMeta {
            columns: vec![
                col("id", ValueKind::Number, true),
                col("note", ValueKind::Text, false),
                col("blob", ValueKind::Binary, false),
            ],
            primary_key: vec!["id".into()],
            estimated_rows: None,
        }
    }

    #[test]
    fn order_defaults_to_primary_key_and_validates_sort() {
        assert_eq!(
            resolve_order(&meta(), None).unwrap(),
            vec![("id".into(), false)]
        );
        let sort = SortSpec {
            column: "note".into(),
            descending: true,
        };
        assert_eq!(
            resolve_order(&meta(), Some(&sort)).unwrap(),
            vec![("note".into(), true), ("id".into(), false)]
        );
        let bad = SortSpec {
            column: "note; DROP TABLE x".into(),
            descending: false,
        };
        assert!(matches!(
            resolve_order(&meta(), Some(&bad)),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn page_limits_and_truncation() {
        let long = "ก".repeat(MAX_CELL_CHARS + 1); // multi-byte: must cut on a char boundary
        let rows = vec![
            vec![
                Some("1".into()),
                Some(long),
                Some("ab".repeat(MAX_BINARY_BYTES + 1)),
            ],
            vec![Some("2".into()), None, Some("00ff".into())],
            vec![Some("3".into()), Some("x".into()), None],
        ];
        let req = PageRequest {
            offset: 40,
            limit: 2,
            sort: None,
        };
        let page = finish_page(
            meta(),
            rows,
            vec![("id".into(), false)],
            &req,
            Instant::now(),
        );

        assert!(page.has_more);
        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.offset, 40);
        assert_eq!(page.ordered_by, vec!["id"]);
        assert_eq!(page.truncated, vec![[0, 1], [0, 2]]);
        assert_eq!(
            page.rows[0][1].as_ref().unwrap().chars().count(),
            MAX_CELL_CHARS
        );
        assert_eq!(
            page.rows[0][2].as_ref().unwrap().len(),
            2 + MAX_BINARY_BYTES * 2
        );
        assert_eq!(page.rows[1][1], None);
        assert_eq!(page.rows[1][2].as_deref(), Some("0x00ff"));
    }
}
