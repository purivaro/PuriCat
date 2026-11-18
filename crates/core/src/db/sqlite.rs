//! SQLite: a database file on this computer.
//!
//! Three things set it apart from the server engines and shape this adapter:
//!
//! - **A column's type is a suggestion.** A declared type only gives the column an
//!   *affinity* (<https://www.sqlite.org/datatype3.html>); any value can sit in any column of
//!   a table that is not `STRICT`. Values are still read as text rendered by SQLite itself
//!   and written as text, which the column's affinity turns into a number where it is one.
//!   The write planner and the filter refuse text that is not a number for a column declared
//!   to hold numbers, since SQLite would store it without a word.
//! - **There is no server to reject writes.** A read-only connection opens the file with
//!   `SQLITE_OPEN_READONLY`: the handle itself cannot write.
//! - **The file may belong to a running program.** Nothing of the file's own settings is
//!   changed (journal mode, page size, ...); a lock held by another program is waited for,
//!   for as long as the connection timeout, and then reported.

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::Row;

use super::console::Console;
use super::filter::Where;
use super::smart::UniqueKey;
use super::write::{self, Statement};
use super::{
    ident, Capabilities, ColumnMeta, DbObject, Namespace, ObjectKind, Order, PageRequest,
    TableMeta, TableRef, ValueKind, MAX_BINARY_BYTES, MAX_CELL_CHARS,
};
use crate::model::ConnectionProfile;
use crate::{Error, Result};

pub const CAPABILITIES: Capabilities = Capabilities {
    schemas: false,
    transactional_ddl: true,
    returning: true,
};

/// The database a file holds. Attached and temporary databases are not browsed.
pub const MAIN: &str = "main";

pub struct SqliteSession {
    options: SqliteConnectOptions,
    pool: SqlitePool,
    read_only: bool,
}

impl SqliteSession {
    pub async fn connect(profile: &ConnectionProfile) -> Result<Self> {
        let file = profile
            .file
            .as_deref()
            .ok_or_else(|| Error::Invalid("Choose the SQLite file to open".into()))?;
        // SQLite itself would only say "unable to open database file".
        match std::fs::metadata(file) {
            Ok(found) if found.is_file() => {}
            Ok(_) => {
                return Err(Error::Invalid(format!(
                    "{file} is a folder, not a SQLite file"
                )))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NotFound(format!("The file {file}")))
            }
            Err(e) => return Err(Error::Database(format!("Cannot open {file}: {e}"))),
        }
        let timeout = Duration::from_secs(profile.connect_timeout_secs as u64);
        // An existing file only: a mistyped path must not leave an empty database behind.
        let options = SqliteConnectOptions::new()
            .filename(Path::new(file))
            .create_if_missing(false)
            .read_only(profile.read_only)
            // How long a statement waits for another program's lock on the file.
            .busy_timeout(timeout);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .acquire_timeout(timeout)
            .idle_timeout(Some(Duration::from_secs(600)))
            .connect_lazy_with(options.clone());
        // Opening succeeds for any file; reading the schema is what proves it is a database.
        if let Err(e) = sqlx::query("SELECT count(*) FROM main.sqlite_master")
            .fetch_one(&pool)
            .await
        {
            pool.close().await;
            return Err(not_a_database(&e).unwrap_or_else(|| Error::database(e)));
        }
        Ok(SqliteSession {
            options,
            pool,
            read_only: profile.read_only,
        })
    }

    pub async fn version(&self) -> Result<String> {
        let row = sqlx::query("SELECT sqlite_version()")
            .fetch_one(&self.pool)
            .await
            .map_err(Error::database)?;
        Ok(text(&row, 0)?.unwrap_or_default())
    }

    /// Whether the file was opened so that it cannot be written.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    pub fn list_databases(&self) -> Vec<Namespace> {
        vec![Namespace {
            name: MAIN.into(),
            system: false,
        }]
    }

    pub async fn list_objects(&self, database: &str) -> Result<Vec<DbObject>> {
        only_main(database)?;
        // SQLite's own tables (sqlite_sequence, sqlite_stat1, ...) are left out.
        let rows = sqlx::query(
            "SELECT name, type FROM main.sqlite_master
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite!_%' ESCAPE '!'
             ORDER BY name COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::database)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            out.push(DbObject {
                name: text(r, 0)?.unwrap_or_default(),
                kind: match text(r, 1)?.as_deref() {
                    Some("view") => ObjectKind::View,
                    _ => ObjectKind::Table,
                },
            });
        }
        Ok(out)
    }

    pub async fn describe_table(&self, table: &TableRef) -> Result<TableMeta> {
        only_main(&table.database)?;
        // `wr`: WITHOUT ROWID. `strict`: the declared types are enforced.
        let shape = sqlx::query(
            "SELECT wr, strict FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
        )
        .bind(&table.name)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::database)?;
        let Some(shape) = shape else {
            return Ok(TableMeta {
                columns: Vec::new(),
                primary_key: Vec::new(),
                estimated_rows: None,
            });
        };
        let without_rowid = text(&shape, 0)?.as_deref() == Some("1");
        let strict = text(&shape, 1)?.as_deref() == Some("1");

        // `hidden`: 1 for a virtual table's hidden column, 2 and 3 for generated columns.
        let rows = sqlx::query(
            "SELECT name, type, \"notnull\", dflt_value, pk, hidden
             FROM pragma_table_xinfo(?1, 'main') ORDER BY cid",
        )
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::database)?;

        let mut keyed: Vec<(u32, String)> = Vec::new();
        let mut columns = Vec::with_capacity(rows.len());
        for r in &rows {
            let hidden = text(r, 5)?.unwrap_or_default();
            if hidden == "1" {
                continue;
            }
            let name = text(r, 0)?.unwrap_or_default();
            let declared = text(r, 1)?.unwrap_or_default();
            let position: u32 = text(r, 4)?.and_then(|p| p.parse().ok()).unwrap_or(0);
            if position > 0 {
                keyed.push((position, name.clone()));
            }
            let affinity = Affinity::of(&declared, strict);
            let kind = value_kind(&declared, affinity);
            columns.push(ColumnMeta {
                kind,
                nullable: text(r, 2)?.as_deref() != Some("1"),
                default: text(r, 3)?,
                primary_key: position > 0,
                generated: matches!(hidden.as_str(), "2" | "3"),
                // Decided below, once the primary key is known.
                editable: false,
                base_type: affinity.name().into(),
                data_type: declared,
                name,
            });
        }
        keyed.sort();
        let primary_key: Vec<String> = keyed.into_iter().map(|(_, name)| name).collect();

        for c in &mut columns {
            // `INTEGER PRIMARY KEY` is the row id: SQLite assigns it when it is left out.
            let rowid = !without_rowid
                && primary_key.len() == 1
                && c.primary_key
                && c.data_type.eq_ignore_ascii_case("integer");
            c.generated |= rowid;
            // A column without a type keeps whatever it is given: a number typed into the
            // grid would be stored as text. Such a column is shown, not edited.
            c.editable = !c.generated && c.kind != ValueKind::Binary && c.base_type != ANY;
        }

        Ok(TableMeta {
            columns,
            primary_key,
            // SQLite keeps no row count; `sqlite_stat1` exists only after ANALYZE.
            estimated_rows: None,
        })
    }

    pub async fn select_page(
        &self,
        table: &TableRef,
        meta: &TableMeta,
        filter: &Where,
        order: &Order,
        req: &PageRequest,
    ) -> Result<Vec<Vec<Option<String>>>> {
        let sql = page_sql(table, meta, filter, order)?;
        let mut query = sqlx::query(&sql);
        for param in &filter.params {
            query = query.bind(param);
        }
        let rows = query
            .bind(req.limit as i64 + 1)
            .bind(req.offset as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(Error::database)?;
        let width = meta.columns.len();
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let mut cells = Vec::with_capacity(width);
            for i in 0..width {
                cells.push(text(r, i)?);
            }
            out.push(cells);
        }
        Ok(out)
    }

    /// The primary key and every unique index a row can be matched on. Partial indexes and
    /// indexes over expressions are left out: equality on them is not row identity.
    pub async fn unique_keys(&self, table: &TableRef) -> Result<Vec<UniqueKey>> {
        let meta = self.describe_table(table).await?;
        let mut keys = Vec::new();
        if !meta.primary_key.is_empty() {
            keys.push(UniqueKey {
                name: "PRIMARY".into(),
                primary: true,
                columns: meta.primary_key.clone(),
            });
        }
        // `origin`: `pk` for the primary key's own index, already listed above.
        // `key`: 1 for the indexed columns, 0 for the columns an index carries along.
        // `cid`: negative for the row id and for an expression.
        let rows = sqlx::query(
            "SELECT l.name, l.partial, x.cid, x.name
             FROM pragma_index_list(?1, 'main') AS l, pragma_index_xinfo(l.name, 'main') AS x
             WHERE l.\"unique\" = 1 AND l.origin <> 'pk' AND x.key = 1
             ORDER BY l.name, x.seqno",
        )
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::database)?;
        let mut unusable = std::collections::HashSet::new();
        let mut found: Vec<UniqueKey> = Vec::new();
        for r in &rows {
            let name = text(r, 0)?.unwrap_or_default();
            let partial = text(r, 1)?.as_deref() == Some("1");
            let plain = text(r, 2)?.is_some_and(|cid| !cid.starts_with('-'));
            let Some(column) = text(r, 3)?.filter(|_| plain && !partial) else {
                unusable.insert(name);
                continue;
            };
            match found.iter_mut().find(|k| k.name == name) {
                Some(key) => key.columns.push(column),
                None => found.push(UniqueKey {
                    primary: false,
                    name,
                    columns: vec![column],
                }),
            }
        }
        found.retain(|k| !unusable.contains(&k.name));
        keys.extend(found);
        Ok(keys)
    }

    /// Run planned statements in one transaction; each must touch exactly one row.
    /// `keep_going` is called before every statement with the number already done;
    /// returning `false` cancels and rolls everything back.
    pub async fn apply(
        &self,
        statements: &[Statement],
        keep_going: &(dyn Fn(usize) -> bool + Send + Sync),
    ) -> Result<()> {
        let pool = self.pool.clone();
        // The write lock is taken at once: a lock held by another program is then waited
        // for here, not met half-way through the save.
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(Error::database)?;
        for (done, statement) in statements.iter().enumerate() {
            if !keep_going(done) {
                let _ = tx.rollback().await;
                return Err(Error::Invalid(
                    "Cancelled. Nothing was saved — the transaction was rolled back.".into(),
                ));
            }
            let mut query = sqlx::query(&statement.sql);
            for param in &statement.params {
                query = query.bind(param.as_deref());
            }
            let affected = match query.execute(&mut *tx).await {
                Ok(done) => done.rows_affected(),
                Err(e) => {
                    let _ = tx.rollback().await;
                    return Err(write::failed(statement, e));
                }
            };
            if affected != 1 {
                let _ = tx.rollback().await;
                return Err(write::conflict(statement, affected));
            }
        }
        tx.commit().await.map_err(Error::database)
    }

    /// Run a read-only query whose columns are all text.
    pub async fn query_text(
        &self,
        sql: &str,
        params: &[Option<String>],
    ) -> Result<Vec<Vec<Option<String>>>> {
        let pool = self.pool.clone();
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param.as_deref());
        }
        let rows = query.fetch_all(&pool).await.map_err(Error::database)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let mut cells = Vec::with_capacity(r.len());
            for i in 0..r.len() {
                cells.push(text(r, i)?);
            }
            out.push(cells);
        }
        Ok(out)
    }

    /// A connection of its own for a query tab. `reader` opens the file so that the handle
    /// cannot write, whatever the session may do: that is what the AI's query tool gets.
    pub async fn open_console(&self, reader: bool) -> Result<Console> {
        let options = self.options.clone().read_only(self.read_only || reader);
        Console::sqlite(options).await
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

fn only_main(database: &str) -> Result<()> {
    if database == MAIN {
        Ok(())
    } else {
        Err(Error::NotFound(format!("Database {database}")))
    }
}

/// SQLITE_NOTADB: the file is something else, or an encrypted database.
fn not_a_database(e: &sqlx::Error) -> Option<Error> {
    let code = e.as_database_error()?.code()?;
    (code == "26").then(|| {
        Error::Database(
            "This file is not a SQLite database, or it is encrypted (encrypted databases cannot be opened)"
                .into(),
        )
    })
}

/// What a column without a declared type — or `ANY` in a STRICT table — is called here:
/// it has no affinity, so a value keeps the storage class it arrives with.
pub(super) const ANY: &str = "any";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Affinity {
    Integer,
    Text,
    Blob,
    Real,
    Numeric,
    None,
}

impl Affinity {
    /// SQLite's own rule, in its order: <https://www.sqlite.org/datatype3.html#determination_of_column_affinity>.
    fn of(declared: &str, strict: bool) -> Affinity {
        let t = declared.to_ascii_uppercase();
        let has = |needle: &str| t.contains(needle);
        if t.is_empty() || (strict && t == "ANY") {
            Affinity::None
        } else if has("INT") {
            Affinity::Integer
        } else if has("CHAR") || has("CLOB") || has("TEXT") {
            Affinity::Text
        } else if has("BLOB") {
            Affinity::Blob
        } else if has("REAL") || has("FLOA") || has("DOUB") {
            Affinity::Real
        } else {
            Affinity::Numeric
        }
    }

    fn name(self) -> &'static str {
        match self {
            Affinity::Integer => "integer",
            Affinity::Text => "text",
            Affinity::Blob => "blob",
            Affinity::Real => "real",
            Affinity::Numeric => "numeric",
            Affinity::None => ANY,
        }
    }
}

/// What the column is *meant* to hold, read from its declared type. NUMERIC affinity covers
/// very different intentions (`DECIMAL(10,2)`, `DATETIME`, `BOOLEAN`, `JSON`, `UUID`): only
/// the names that plainly mean a number are treated as numbers.
fn value_kind(declared: &str, affinity: Affinity) -> ValueKind {
    let t = declared.to_ascii_uppercase();
    match affinity {
        Affinity::Integer | Affinity::Real => ValueKind::Number,
        Affinity::Text => ValueKind::Text,
        Affinity::Blob => ValueKind::Binary,
        Affinity::None => ValueKind::Other,
        Affinity::Numeric => {
            if t.contains("BOOL") {
                ValueKind::Bool
            } else if t.contains("DATE") || t.contains("TIME") {
                ValueKind::Temporal
            } else if t.contains("JSON") {
                ValueKind::Json
            } else if ["NUMERIC", "DECIMAL", "DEC", "NUMBER"]
                .iter()
                .any(|name| t.split(['(', ' ']).next() == Some(name))
            {
                ValueKind::Number
            } else {
                ValueKind::Other
            }
        }
    }
}

/// Every projected value is rendered to text by SQLite, so 64-bit integers reach the grid
/// whole. A REAL is shown the way SQLite itself converts it: 15 significant digits.
fn page_sql(table: &TableRef, meta: &TableMeta, filter: &Where, order: &Order) -> Result<String> {
    let mut exprs = Vec::with_capacity(meta.columns.len());
    for c in &meta.columns {
        // One unit more than the preview shows, so a cut value can be told from a short one.
        exprs.push(cell_expr(c, MAX_CELL_CHARS + 1, MAX_BINARY_BYTES + 1)?);
    }
    let mut sql = format!(
        "SELECT {} FROM {}.{}",
        exprs.join(", "),
        ident::sqlite(&table.database)?,
        ident::sqlite(&table.name)?
    );
    sql.push_str(&filter.sql);
    if !order.is_empty() {
        let mut parts = Vec::with_capacity(order.len());
        for (col, desc) in order {
            parts.push(format!(
                "{}{}",
                ident::sqlite(col)?,
                if *desc { " DESC" } else { "" }
            ));
        }
        sql.push_str(" ORDER BY ");
        sql.push_str(&parts.join(", "));
    }
    sql.push_str(" LIMIT ? OFFSET ?");
    Ok(sql)
}

/// One column as the text the grid shows: at most `chars` characters, or `bytes` bytes in hex.
pub(super) fn cell_expr(c: &ColumnMeta, chars: usize, bytes: usize) -> Result<String> {
    let col = ident::sqlite(&c.name)?;
    Ok(if c.kind == ValueKind::Binary {
        // `hex(NULL)` is an empty string, not NULL.
        format!("CASE WHEN {col} IS NULL THEN NULL ELSE hex(substr({col}, 1, {bytes})) END")
    } else {
        format!("substr(CAST({col} AS TEXT), 1, {chars})")
    })
}

/// Any value as the text SQLite renders it. A value that is not valid UTF-8 (a BLOB in a
/// column that is not declared as one) is shown with the bad bytes replaced.
fn text(row: &SqliteRow, idx: usize) -> Result<Option<String>> {
    let bytes: Option<Vec<u8>> = row.try_get_unchecked(idx).map_err(Error::database)?;
    Ok(bytes.map(|b| match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinity_follows_sqlite_rules() {
        let of = |declared: &str| Affinity::of(declared, false);
        assert_eq!(of("INTEGER"), Affinity::Integer);
        assert_eq!(of("bigint unsigned"), Affinity::Integer);
        // The rules are substring tests, in order: "INT" wins over everything.
        assert_eq!(of("POINT"), Affinity::Integer);
        assert_eq!(of("CHARINT"), Affinity::Integer);
        assert_eq!(of("VARCHAR(20)"), Affinity::Text);
        assert_eq!(of("nvarchar(10)"), Affinity::Text);
        assert_eq!(of("CLOB"), Affinity::Text);
        assert_eq!(of("BLOB"), Affinity::Blob);
        assert_eq!(of("DOUBLE PRECISION"), Affinity::Real);
        assert_eq!(of("FLOAT"), Affinity::Real);
        assert_eq!(of("DECIMAL(10,2)"), Affinity::Numeric);
        assert_eq!(of("DATETIME"), Affinity::Numeric);
        assert_eq!(of("STRING"), Affinity::Numeric);
        assert_eq!(of(""), Affinity::None);
        // `ANY` keeps values as they are only in a STRICT table.
        assert_eq!(of("ANY"), Affinity::Numeric);
        assert_eq!(Affinity::of("ANY", true), Affinity::None);
    }

    #[test]
    fn kind_is_what_the_declared_type_means() {
        let kind = |declared: &str| value_kind(declared, Affinity::of(declared, false));
        assert_eq!(kind("INTEGER"), ValueKind::Number);
        assert_eq!(kind("REAL"), ValueKind::Number);
        assert_eq!(kind("NUMERIC"), ValueKind::Number);
        assert_eq!(kind("decimal(12,2)"), ValueKind::Number);
        assert_eq!(kind("TEXT"), ValueKind::Text);
        assert_eq!(kind("BLOB"), ValueKind::Binary);
        assert_eq!(kind("BOOLEAN"), ValueKind::Bool);
        assert_eq!(kind("DATETIME"), ValueKind::Temporal);
        assert_eq!(kind("DATE"), ValueKind::Temporal);
        assert_eq!(kind("TIMESTAMP"), ValueKind::Temporal);
        assert_eq!(kind("JSON"), ValueKind::Json);
        // NUMERIC affinity, but nothing says these hold numbers.
        assert_eq!(kind("UUID"), ValueKind::Other);
        assert_eq!(kind("STRING"), ValueKind::Other);
        assert_eq!(kind(""), ValueKind::Other);
    }

    #[test]
    fn builds_a_quoted_text_projection() {
        let col = |name: &str, declared: &str| {
            let affinity = Affinity::of(declared, false);
            ColumnMeta {
                name: name.into(),
                data_type: declared.into(),
                kind: value_kind(declared, affinity),
                nullable: true,
                default: None,
                primary_key: false,
                generated: false,
                editable: true,
                base_type: affinity.name().into(),
            }
        };
        let meta = TableMeta {
            columns: vec![
                col("id", "INTEGER"),
                col("we\"ird", "TEXT"),
                col("photo", "BLOB"),
            ],
            primary_key: vec!["id".into()],
            estimated_rows: None,
        };
        let table = TableRef {
            database: "main".into(),
            schema: None,
            name: "items".into(),
        };
        let sql = page_sql(&table, &meta, &Where::default(), &vec![("id".into(), true)]).unwrap();
        assert_eq!(
            sql,
            "SELECT substr(CAST(\"id\" AS TEXT), 1, 1001), substr(CAST(\"we\"\"ird\" AS TEXT), 1, 1001), \
             CASE WHEN \"photo\" IS NULL THEN NULL ELSE hex(substr(\"photo\", 1, 25)) END \
             FROM \"main\".\"items\" ORDER BY \"id\" DESC LIMIT ? OFFSET ?"
        );
    }
}
