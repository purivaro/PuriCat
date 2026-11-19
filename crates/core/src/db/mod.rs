//! Capability-aware database adapters.
//!
//! Each engine gets its own module with its own SQL; nothing here rewrites one dialect
//! into another. `Session` is a plain enum so adding an engine is a compile-time checklist.

pub mod console;
pub mod ddl;
pub mod export;
pub mod filter;
pub mod ident;
mod mysql;
mod postgres;
pub mod script;
pub mod smart;
mod sqlite;
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
/// Longest text sent for one cell when its whole value is asked for.
pub const MAX_FULL_CHARS: usize = 1_000_000;
/// Longest binary prefix (bytes) sent for one cell when its whole value is asked for.
pub const MAX_FULL_BYTES: usize = 65_536;

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
    /// Conditions a row must meet, all of them. Empty shows every row.
    #[serde(default)]
    pub filters: Vec<filter::Filter>,
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

/// How many rows a table has, or how many of them pass a filter: counted, not estimated.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RowCount {
    pub count: u64,
    pub elapsed_ms: u64,
}

/// `SELECT COUNT(*)` with the number rendered to text by the server, like every other value.
fn count_sql(engine: Engine, table: &TableRef, filter: &filter::Where) -> Result<String> {
    let counted = match engine {
        Engine::Postgres => "COUNT(*)::text",
        Engine::Sqlite => "CAST(COUNT(*) AS TEXT)",
        _ => "CAST(COUNT(*) AS CHAR)",
    };
    Ok(format!(
        "SELECT {counted} FROM {}{}",
        write::table_name(engine, table)?,
        filter.sql
    ))
}

/// The whole value of one cell, for the cells a page shows shortened.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CellContent {
    /// As the server renders it; binary as `0x` and hex. `null` is SQL NULL.
    pub value: Option<String>,
    /// Even this was cut: the value is longer than `MAX_FULL_CHARS` / `MAX_FULL_BYTES`.
    pub cut: bool,
}

/// An open, pooled connection to one server, or to one SQLite file.
pub enum Session {
    MySql(mysql::MySqlSession),
    Postgres(Box<postgres::PgSession>),
    Sqlite(Box<sqlite::SqliteSession>),
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
            // A file has no password: nothing is read from the keychain for it.
            Engine::Sqlite => Ok(Session::Sqlite(Box::new(
                sqlite::SqliteSession::connect(profile).await?,
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
            Session::Sqlite(_) => sqlite::CAPABILITIES,
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
            Session::Sqlite(s) => (Engine::Sqlite, s.version().await?, None, s.read_only()),
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
            Session::Sqlite(s) => Ok(s.list_databases()),
        }
    }

    /// What the server offers for a new database, to fill the form's lists.
    pub async fn database_options(&self) -> Result<ddl::DatabaseOptions> {
        let text =
            |row: &[Option<String>], i: usize| row.get(i).cloned().flatten().unwrap_or_default();
        let mut options = ddl::DatabaseOptions::default();
        match self {
            Session::MySql(s) => {
                let rows = s
                    .query_text(
                        "SELECT CHARACTER_SET_NAME, DEFAULT_COLLATE_NAME, DESCRIPTION
                         FROM information_schema.CHARACTER_SETS ORDER BY CHARACTER_SET_NAME",
                        &[],
                    )
                    .await?;
                options.charsets = rows
                    .iter()
                    .map(|r| ddl::Charset {
                        name: text(r, 0),
                        default_collation: text(r, 1),
                        description: text(r, 2),
                    })
                    .collect();
                let rows = s
                    .query_text(
                        "SELECT COLLATION_NAME, CHARACTER_SET_NAME FROM information_schema.COLLATIONS
                         WHERE CHARACTER_SET_NAME IS NOT NULL ORDER BY COLLATION_NAME",
                        &[],
                    )
                    .await?;
                options.collations = rows
                    .iter()
                    .map(|r| ddl::Collation {
                        name: text(r, 0),
                        charset: text(r, 1),
                    })
                    .collect();
                let rows = s
                    .query_text("SELECT @@character_set_server, @@collation_server", &[])
                    .await?;
                if let Some(r) = rows.first() {
                    options.default_charset = r.first().cloned().flatten();
                    options.default_collation = r.get(1).cloned().flatten();
                }
            }
            Session::Postgres(s) => {
                let home = s.home().to_string();
                let column = |rows: Vec<Vec<Option<String>>>| -> Vec<String> {
                    rows.iter().map(|r| text(r, 0)).collect()
                };
                options.owners = column(
                    s.query_text(
                        &home,
                        "SELECT rolname::text FROM pg_roles WHERE rolname !~ '^pg_' ORDER BY rolname",
                        &[],
                    )
                    .await?,
                );
                options.current_user = column(
                    s.query_text(&home, "SELECT current_user::text", &[])
                        .await?,
                )
                .into_iter()
                .next();
                options.templates = column(
                    s.query_text(
                        &home,
                        "SELECT datname::text FROM pg_database WHERE datistemplate ORDER BY datname",
                        &[],
                    )
                    .await?,
                );
                // Encoding ids 0..=34 are the ones a database can be stored in; the ids above
                // (SJIS, BIG5, ...) exist for clients only. The server refuses any other.
                options.encodings = column(
                    s.query_text(
                        &home,
                        "SELECT pg_encoding_to_char(e)::text FROM generate_series(0, 34) AS e
                         WHERE pg_encoding_to_char(e) <> '' ORDER BY 1",
                        &[],
                    )
                    .await?,
                );
                options.default_encoding = column(
                    s.query_text(
                        &home,
                        "SELECT pg_encoding_to_char(encoding)::text FROM pg_database WHERE datname = 'template1'",
                        &[],
                    )
                    .await?,
                )
                .into_iter()
                .next();
            }
            Session::Sqlite(_) => {
                // Refuse with the same explanation the statement builder gives.
                ddl::supported(Engine::Sqlite)?;
            }
        }
        Ok(options)
    }

    /// The exact statement `create_database` would run, for the form's preview.
    pub fn create_database_sql(&self, spec: &ddl::NewDatabase) -> Result<String> {
        ddl::create_database(self.engine(), spec)
    }

    /// Create a database. Returns the statement that ran.
    pub async fn create_database(&self, spec: &ddl::NewDatabase) -> Result<String> {
        let sql = self.create_database_sql(spec)?;
        match self {
            Session::MySql(s) => s.execute_statement(&sql).await?,
            Session::Postgres(s) => s.execute_statement(&sql).await?,
            Session::Sqlite(_) => unreachable!("refused by create_database_sql"),
        }
        Ok(sql)
    }

    /// What a database is set to now, to fill the edit form.
    pub async fn database_details(&self, name: &str) -> Result<ddl::DatabaseDetails> {
        let mut details = ddl::DatabaseDetails {
            name: name.to_string(),
            protected: ddl::is_protected(self.engine(), name),
            ..Default::default()
        };
        let wanted = [Some(name.to_string())];
        let rows = match self {
            Session::MySql(s) => {
                s.query_text(
                    "SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME
                     FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
                    &wanted,
                )
                .await?
            }
            Session::Postgres(s) => {
                s.query_text(
                    s.home(),
                    "SELECT pg_get_userbyid(datdba)::text, pg_encoding_to_char(encoding)::text
                     FROM pg_database WHERE datname = $1",
                    &wanted,
                )
                .await?
            }
            Session::Sqlite(_) => {
                ddl::supported(Engine::Sqlite)?;
                Vec::new()
            }
        };
        let row = rows
            .first()
            .ok_or_else(|| Error::NotFound(format!("Database {name}")))?;
        let cell = |i: usize| row.get(i).cloned().flatten();
        match self {
            Session::MySql(_) => (details.charset, details.collation) = (cell(0), cell(1)),
            _ => (details.owner, details.encoding) = (cell(0), cell(1)),
        }
        // What a drop would take with it. Best effort: a database that cannot be looked
        // into (PostgreSQL's `template0`, no privilege) is still shown, without the number.
        let counted = match self {
            Session::MySql(s) => {
                s.query_text(
                    "SELECT CAST(COUNT(*) AS CHAR) FROM information_schema.TABLES
                     WHERE TABLE_SCHEMA = ?",
                    &wanted,
                )
                .await
            }
            Session::Postgres(s) => {
                s.query_text(
                    name,
                    "SELECT count(*)::text FROM pg_class c
                     JOIN pg_namespace n ON n.oid = c.relnamespace
                     WHERE c.relkind IN ('r', 'p', 'v', 'm')
                       AND n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'",
                    &[],
                )
                .await
            }
            Session::Sqlite(_) => return Ok(details),
        };
        details.tables = counted
            .ok()
            .and_then(|rows| rows.first()?.first()?.as_deref()?.parse().ok());
        Ok(details)
    }

    /// The database this session itself is connected through, which the server will not
    /// drop or rename from here.
    fn refuse_own_database(&self, name: &str, action: &str) -> Result<()> {
        match self {
            Session::Postgres(s) if s.home() == name => Err(Error::Invalid(format!(
                "This connection is opened on “{name}”, so it cannot {action} it. Use a connection whose default database is another one."
            ))),
            _ => Ok(()),
        }
    }

    /// The exact statement `drop_database` would run.
    pub fn drop_database_sql(&self, name: &str) -> Result<String> {
        self.refuse_own_database(name, "drop")?;
        ddl::drop_database(self.engine(), name)
    }

    /// Drop a database and everything in it. Returns the statement that ran.
    pub async fn drop_database(&self, name: &str) -> Result<String> {
        let sql = self.drop_database_sql(name)?;
        match self {
            Session::MySql(s) => s.execute_statement(&sql).await?,
            Session::Postgres(s) => {
                s.release(name).await;
                s.execute_statement(&sql).await?
            }
            Session::Sqlite(_) => unreachable!("refused by drop_database_sql"),
        }
        Ok(sql)
    }

    /// The exact statements `alter_database` would run, in order.
    pub fn alter_database_sql(&self, spec: &ddl::AlterDatabase) -> Result<Vec<String>> {
        let statements = ddl::alter_database(self.engine(), spec)?;
        if statements.iter().any(|s| s.contains(" RENAME TO ")) {
            self.refuse_own_database(&spec.name, "rename")?;
        }
        Ok(statements)
    }

    /// Change a database. On PostgreSQL the statements run as one unit: all or none.
    pub async fn alter_database(&self, spec: &ddl::AlterDatabase) -> Result<Vec<String>> {
        let statements = self.alter_database_sql(spec)?;
        match self {
            Session::MySql(s) => {
                for sql in &statements {
                    s.execute_statement(sql).await?;
                }
            }
            Session::Postgres(s) => {
                s.release(&spec.name).await;
                // Several statements sent as one simple query are one implicit transaction.
                s.execute_statement(&statements.join("; ")).await?
            }
            Session::Sqlite(_) => unreachable!("refused by alter_database_sql"),
        }
        Ok(statements)
    }

    /// MySQL server maintenance from the connection's menu. Returns the statement that ran.
    pub async fn flush(&self, what: ddl::Flush) -> Result<&'static str> {
        let sql = ddl::flush(self.engine(), what)?;
        match self {
            Session::MySql(s) => s.execute_statement(sql).await?,
            _ => unreachable!("refused by ddl::flush"),
        }
        Ok(sql)
    }

    pub async fn list_schemas(&self, database: &str) -> Result<Vec<Namespace>> {
        match self {
            Session::MySql(_) => Err(Error::Unsupported(
                "MySQL has no schemas inside a database".into(),
            )),
            Session::Sqlite(_) => Err(Error::Unsupported(
                "SQLite has no schemas inside a database".into(),
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
            Session::Sqlite(s) => s.list_objects(database).await,
        }
    }

    pub async fn describe_table(&self, table: &TableRef) -> Result<TableMeta> {
        match self {
            Session::MySql(s) => s.describe_table(table).await,
            Session::Postgres(s) => s.describe_table(table).await,
            Session::Sqlite(s) => s.describe_table(table).await,
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
        let filter = filter::where_clause(self.engine(), &meta, &req.filters)?;
        let raw = match self {
            Session::MySql(s) => s.select_page(table, &meta, &filter, &order, req).await?,
            Session::Postgres(s) => s.select_page(table, &meta, &filter, &order, req).await?,
            Session::Sqlite(s) => s.select_page(table, &meta, &filter, &order, req).await?,
        };
        Ok(finish_page(meta, raw, order, req, started))
    }

    /// Count the rows of a table that pass `filters` (all of them, without filters). Asked
    /// for, never automatic: on a large table the server reads every row or a whole index.
    /// It runs on a connection of its own, so setting `stop` interrupts it on the server.
    pub async fn count_rows(
        &self,
        table: &TableRef,
        filters: &[filter::Filter],
        stop: &std::sync::atomic::AtomicBool,
    ) -> Result<RowCount> {
        use std::sync::atomic::Ordering;
        let started = Instant::now();
        let meta = self.describe_table(table).await?;
        if meta.columns.is_empty() {
            return Err(Error::NotFound(format!("Table {}", table.name)));
        }
        let engine = self.engine();
        let filter = filter::where_clause(engine, &meta, filters)?;
        let sql = count_sql(engine, table, &filter)?;
        let mut console = self.open_reader(Some(&table.database)).await?;
        let canceller = console.canceller();
        let query = console.text_scalar(&sql, &filter.params);
        tokio::pin!(query);
        let mut asked = false;
        let answer = loop {
            tokio::select! {
                answer = &mut query => break answer,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)), if !asked => {
                    if stop.load(Ordering::Relaxed) {
                        asked = true;
                        let _ = canceller.cancel().await;
                    }
                }
            }
        };
        let count = match answer {
            Ok(text) => text.and_then(|t| t.parse().ok()).ok_or_else(|| {
                Error::Database("The server did not answer the count with a number".into())
            })?,
            Err(e) if asked && console::is_cancellation(&e) => {
                return Err(Error::Conflict("Cancelled".into()))
            }
            Err(e) => return Err(Error::database(e)),
        };
        Ok(RowCount {
            count,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// Read one cell in full: the row addressed by its complete primary key. Read-only.
    pub async fn fetch_cell(
        &self,
        table: &TableRef,
        key: &[write::CellValue],
        column: &str,
    ) -> Result<CellContent> {
        let meta = self.describe_table(table).await?;
        if meta.columns.is_empty() {
            return Err(Error::NotFound(format!("Table {}", table.name)));
        }
        let col = write::column(&meta, column)?;
        let engine = self.engine();
        let expr = match self {
            Session::MySql(_) => mysql::cell_expr(col, MAX_FULL_CHARS + 1, MAX_FULL_BYTES + 1)?,
            Session::Postgres(_) => {
                postgres::cell_expr(col, MAX_FULL_CHARS + 1, MAX_FULL_BYTES + 1)?
            }
            Session::Sqlite(_) => sqlite::cell_expr(col, MAX_FULL_CHARS + 1, MAX_FULL_BYTES + 1)?,
        };
        let (clause, params) = write::row_clause(engine, &meta, key)?;
        let sql = format!(
            "SELECT {expr} FROM {} WHERE {clause}",
            write::table_name(engine, table)?
        );
        let mut rows = self.query_text(&table.database, &sql, &params).await?;
        if rows.len() != 1 {
            return Err(Error::Conflict(
                "This row is no longer there. Refresh the table.".into(),
            ));
        }
        let Some(mut text) = rows.remove(0).into_iter().next().flatten() else {
            return Ok(CellContent {
                value: None,
                cut: false,
            });
        };
        let binary = col.kind == ValueKind::Binary;
        let limit = if binary {
            MAX_FULL_BYTES * 2
        } else {
            MAX_FULL_CHARS
        };
        let cut = match text.char_indices().nth(limit) {
            Some((at, _)) => {
                text.truncate(at);
                true
            }
            None => false,
        };
        if binary {
            text.insert_str(0, "0x");
        }
        Ok(CellContent {
            value: Some(text),
            cut,
        })
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
        write::plan(self.engine(), table, &meta, changes)
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
            Session::MySql(s) => s.apply(&statements, &|_| true).await?,
            Session::Postgres(s) => s.apply(&table.database, &statements, &|_| true).await?,
            Session::Sqlite(s) => s.apply(&statements, &|_| true).await?,
        }
        Ok(write::tally(
            &statements,
            started.elapsed().as_millis() as u64,
        ))
    }

    pub fn engine(&self) -> Engine {
        match self {
            Session::MySql(_) => Engine::Mysql,
            Session::Postgres(_) => Engine::Postgres,
            Session::Sqlite(_) => Engine::Sqlite,
        }
    }

    /// A connection of its own for one query tab, on `database` (the default one if `None`).
    pub async fn open_console(&self, database: Option<&str>) -> Result<console::Console> {
        match self {
            Session::MySql(s) => s.open_console(database).await,
            Session::Postgres(s) => s.open_console(database).await,
            Session::Sqlite(s) => s.open_console(false).await,
        }
    }

    /// A connection of its own for a caller that must only read (the AI's query tool; see
    /// [`console::Console::read_only`]). On SQLite the file is opened so that the handle
    /// cannot write; the servers are held by a read-only transaction instead.
    pub async fn open_reader(&self, database: Option<&str>) -> Result<console::Console> {
        match self {
            Session::Sqlite(s) => s.open_console(true).await,
            _ => self.open_console(database).await,
        }
    }

    /// Keys a pasted row can be matched on: the primary key first, then unique indexes.
    pub async fn unique_keys(&self, table: &TableRef) -> Result<Vec<smart::UniqueKey>> {
        match self {
            Session::MySql(s) => s.unique_keys(table).await,
            Session::Postgres(s) => s.unique_keys(table).await,
            Session::Sqlite(s) => s.unique_keys(table).await,
        }
    }

    async fn query_text(
        &self,
        database: &str,
        sql: &str,
        params: &[Option<String>],
    ) -> Result<Vec<Vec<Option<String>>>> {
        match self {
            Session::MySql(s) => s.query_text(sql, params).await,
            Session::Postgres(s) => s.query_text(database, sql, params).await,
            Session::Sqlite(s) => s.query_text(sql, params).await,
        }
    }

    /// Smart Paste, read-only half: match the pasted rows against the table and say exactly
    /// what running it would do. Also returns the statements, for `apply_paste`.
    async fn prepare_paste(
        &self,
        table: &TableRef,
        request: &smart::PasteRequest,
    ) -> Result<(smart::Analysis, Vec<write::Statement>)> {
        let started = Instant::now();
        let meta = self.describe_table(table).await?;
        if meta.columns.is_empty() {
            return Err(Error::NotFound(format!("Table {}", table.name)));
        }
        let keys = self.unique_keys(table).await?;
        let engine = self.engine();
        let mut draft = smart::Draft::new(engine, &meta, &keys, request)?;
        for (sql, params, rows) in draft.lookups(table)? {
            let found = self.query_text(&table.database, &sql, &params).await?;
            draft.matched(&rows, found);
        }
        let (mut analysis, changes) = draft.finish();
        // Rows with a problem never reach the change set. They stop the run, unless the
        // request says to leave them out and run the rest.
        let blocked = !analysis.problems.is_empty()
            || (analysis.counts.problems > 0 && !request.skip_problems);
        let nothing = changes.updates.is_empty() && changes.inserts.is_empty();
        let statements = if blocked || nothing {
            Vec::new()
        } else {
            write::plan_keyed(engine, table, &meta, &request.key, &changes)?
        };
        analysis.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok((analysis, statements))
    }

    pub async fn analyze_paste(
        &self,
        table: &TableRef,
        request: &smart::PasteRequest,
    ) -> Result<smart::Analysis> {
        Ok(self.prepare_paste(table, request).await?.0)
    }

    /// Smart Paste, write half. The table is analysed again; if the outcome is not exactly the
    /// one the user approved (`expected`, the preview's fingerprint), nothing runs. Every UPDATE also carries the
    /// optimistic check on the values seen by this fresh analysis.
    pub async fn apply_paste(
        &self,
        table: &TableRef,
        request: &smart::PasteRequest,
        expected: &str,
        keep_going: &(dyn Fn(usize) -> bool + Send + Sync),
    ) -> Result<write::ApplyResult> {
        let started = Instant::now();
        let (analysis, statements) = self.prepare_paste(table, request).await?;
        if let Some(problem) = analysis.problems.first() {
            return Err(Error::Invalid(problem.clone()));
        }
        if analysis.fingerprint != expected {
            return Err(Error::Conflict(
                "The table changed since the preview, so the result would be different. Preview again before running.".into(),
            ));
        }
        if analysis.counts.problems > 0 && !request.skip_problems {
            return Err(Error::Invalid(
                "Some pasted rows have problems. Fix them and preview again.".into(),
            ));
        }
        if statements.is_empty() {
            return Err(Error::Invalid("There is nothing to save".into()));
        }
        match self {
            Session::MySql(s) => s.apply(&statements, keep_going).await?,
            Session::Postgres(s) => s.apply(&table.database, &statements, keep_going).await?,
            Session::Sqlite(s) => s.apply(&statements, keep_going).await?,
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
            Session::Sqlite(s) => s.close().await,
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
    fn the_count_statement_per_engine() {
        let filter = filter::Where {
            sql: " WHERE x = ?".into(),
            params: vec!["1".into()],
        };
        let table = TableRef {
            database: "shop".into(),
            schema: Some("pub\"lic".into()),
            name: "or`ders".into(),
        };
        assert_eq!(
            count_sql(Engine::Mysql, &table, &filter).unwrap(),
            "SELECT CAST(COUNT(*) AS CHAR) FROM `shop`.`or``ders` WHERE x = ?"
        );
        assert_eq!(
            count_sql(Engine::Postgres, &table, &filter::Where::default()).unwrap(),
            "SELECT COUNT(*)::text FROM \"pub\"\"lic\".\"or`ders\""
        );
        let main = TableRef {
            database: "main".into(),
            schema: None,
            name: "orders".into(),
        };
        assert_eq!(
            count_sql(Engine::Sqlite, &main, &filter).unwrap(),
            "SELECT CAST(COUNT(*) AS TEXT) FROM \"main\".\"orders\" WHERE x = ?"
        );
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
            filters: Vec::new(),
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
