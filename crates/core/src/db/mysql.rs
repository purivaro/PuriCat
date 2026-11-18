use std::time::Duration;

use sqlx::mysql::{MySqlConnectOptions, MySqlPool, MySqlPoolOptions, MySqlRow, MySqlSslMode};
use sqlx::{ConnectOptions, Executor, Row};

use super::console::Console;
use super::filter::Where;
use super::smart::UniqueKey;
use super::write::{self, Statement};
use super::{
    ident, Capabilities, ColumnMeta, DbObject, Namespace, ObjectKind, Order, PageRequest,
    TableMeta, TableRef, ValueKind, MAX_BINARY_BYTES, MAX_CELL_CHARS,
};
use crate::model::{ConnectionProfile, TlsMode};
use crate::{Error, Result};

pub const CAPABILITIES: Capabilities = Capabilities {
    schemas: false,
    transactional_ddl: false,
    returning: false,
};

const SYSTEM_DATABASES: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];
const SPATIAL_TYPES: &[&str] = &[
    "geometry",
    "point",
    "linestring",
    "polygon",
    "multipoint",
    "multilinestring",
    "multipolygon",
    "geometrycollection",
    "geomcollection",
];

pub struct MySqlSession {
    pool: MySqlPool,
}

impl MySqlSession {
    pub async fn connect(profile: &ConnectionProfile, password: Option<&str>) -> Result<Self> {
        let mut opts = MySqlConnectOptions::new()
            .host(&profile.host)
            .port(profile.port)
            .username(&profile.username)
            .charset("utf8mb4")
            // Keep the server's own session defaults. The driver would otherwise force the
            // time zone to UTC (changing what CURRENT_TIMESTAMP stores and how TIMESTAMP reads)
            // and turn `||` into string concatenation.
            .timezone(None::<String>)
            .pipes_as_concat(false)
            .ssl_mode(match profile.tls {
                TlsMode::Disable => MySqlSslMode::Disabled,
                TlsMode::Prefer => MySqlSslMode::Preferred,
                TlsMode::Require => MySqlSslMode::Required,
                TlsMode::VerifyCa => MySqlSslMode::VerifyCa,
                TlsMode::VerifyFull => MySqlSslMode::VerifyIdentity,
            });
        if let Some(pw) = password {
            opts = opts.password(pw);
        }
        if let Some(db) = &profile.database {
            opts = opts.database(db);
        }
        if let Some(ca) = &profile.tls_ca_path {
            opts = opts.ssl_ca(ca);
        }

        // Connect once directly: a pool would retry until its timeout and then report only
        // "pool timed out", hiding the real reason (refused, bad password, unknown database).
        let timeout = Duration::from_secs(profile.connect_timeout_secs as u64);
        super::probe(timeout, opts.connect()).await?;

        let read_only = profile.read_only;
        let pool = MySqlPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(timeout)
            .idle_timeout(Some(Duration::from_secs(600)))
            .after_connect(move |conn, _meta| {
                Box::pin(async move {
                    // Reject invalid values instead of silently truncating or zeroing them,
                    // whatever the server's default sql_mode is.
                    conn.execute(
                        "SET SESSION sql_mode = IF(@@SESSION.sql_mode = '', 'STRICT_ALL_TABLES', \
                         CONCAT(@@SESSION.sql_mode, ',STRICT_ALL_TABLES'))",
                    )
                    .await?;
                    if read_only {
                        // Applies to every later transaction, including autocommit statements.
                        conn.execute("SET SESSION TRANSACTION READ ONLY").await?;
                    }
                    Ok(())
                })
            })
            .connect_lazy_with(opts);
        Ok(MySqlSession { pool })
    }

    pub async fn version(&self) -> Result<String> {
        let row = sqlx::query("SELECT VERSION()")
            .fetch_one(&self.pool)
            .await
            .map_err(Error::database)?;
        Ok(text(&row, 0)?.unwrap_or_default())
    }

    pub async fn tls(&self) -> Result<Option<bool>> {
        let row = sqlx::query("SHOW SESSION STATUS LIKE 'Ssl_cipher'")
            .fetch_optional(&self.pool)
            .await
            .map_err(Error::database)?;
        Ok(match row {
            Some(r) => Some(!text(&r, 1)?.unwrap_or_default().is_empty()),
            None => None,
        })
    }

    pub async fn read_only(&self) -> Result<bool> {
        let row = sqlx::query("SELECT CAST(@@session.transaction_read_only AS CHAR)")
            .fetch_one(&self.pool)
            .await
            .map_err(Error::database)?;
        Ok(text(&row, 0)?.as_deref() == Some("1"))
    }

    pub async fn list_databases(&self) -> Result<Vec<Namespace>> {
        let rows = sqlx::query("SHOW DATABASES")
            .fetch_all(&self.pool)
            .await
            .map_err(Error::database)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let name = text(r, 0)?.unwrap_or_default();
            let system = SYSTEM_DATABASES.contains(&name.to_ascii_lowercase().as_str());
            out.push(Namespace { name, system });
        }
        out.sort_by(|a, b| {
            (a.system, a.name.to_lowercase()).cmp(&(b.system, b.name.to_lowercase()))
        });
        Ok(out)
    }

    pub async fn list_objects(&self, database: &str) -> Result<Vec<DbObject>> {
        let rows = sqlx::query(
            "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
        )
        .bind(database)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::database)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let kind = match text(r, 1)?.unwrap_or_default().as_str() {
                "BASE TABLE" => ObjectKind::Table,
                _ => ObjectKind::View, // VIEW, SYSTEM VIEW
            };
            out.push(DbObject {
                name: text(r, 0)?.unwrap_or_default(),
                kind,
            });
        }
        Ok(out)
    }

    pub async fn describe_table(&self, table: &TableRef) -> Result<TableMeta> {
        let pk_rows = sqlx::query(
            "SELECT COLUMN_NAME FROM information_schema.STATISTICS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY'
             ORDER BY SEQ_IN_INDEX",
        )
        .bind(&table.database)
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::database)?;
        let mut primary_key = Vec::with_capacity(pk_rows.len());
        for r in &pk_rows {
            primary_key.push(text(r, 0)?.unwrap_or_default());
        }

        let rows = sqlx::query(
            "SELECT COLUMN_NAME, COLUMN_TYPE, DATA_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA
             FROM information_schema.COLUMNS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
        )
        .bind(&table.database)
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::database)?;
        let mut columns = Vec::with_capacity(rows.len());
        for r in &rows {
            let name = text(r, 0)?.unwrap_or_default();
            let column_type = text(r, 1)?.unwrap_or_default();
            let data_type = text(r, 2)?.unwrap_or_default().to_ascii_lowercase();
            let extra = text(r, 5)?.unwrap_or_default().to_ascii_lowercase();
            let kind = value_kind(&data_type, &column_type);
            let generated = is_generated(&extra);
            columns.push(ColumnMeta {
                kind,
                editable: !generated
                    && kind != ValueKind::Binary
                    && !SPATIAL_TYPES.contains(&data_type.as_str()),
                nullable: text(r, 3)?.as_deref() == Some("YES"),
                default: text(r, 4)?,
                primary_key: primary_key.contains(&name),
                generated,
                base_type: data_type,
                data_type: column_type,
                name,
            });
        }

        let estimated_rows = sqlx::query(
            "SELECT CAST(TABLE_ROWS AS CHAR) FROM information_schema.TABLES
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
        )
        .bind(&table.database)
        .bind(&table.name)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::database)?
        .and_then(|r| text(&r, 0).ok().flatten())
        .and_then(|s| s.parse().ok());

        Ok(TableMeta {
            columns,
            primary_key,
            estimated_rows,
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
            .bind(req.limit as u64 + 1)
            .bind(req.offset)
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

    /// The primary key and every unique index a row can be matched on. Indexes over
    /// expressions or column prefixes are left out: equality on them is not row identity.
    pub async fn unique_keys(&self, table: &TableRef) -> Result<Vec<UniqueKey>> {
        let rows = sqlx::query(
            "SELECT INDEX_NAME, COLUMN_NAME, CAST(SUB_PART AS CHAR)
             FROM information_schema.STATISTICS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND NON_UNIQUE = 0
             ORDER BY INDEX_NAME, SEQ_IN_INDEX",
        )
        .bind(&table.database)
        .bind(&table.name)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::database)?;
        let mut keys: Vec<UniqueKey> = Vec::new();
        let mut unusable = std::collections::HashSet::new();
        for r in &rows {
            let name = text(r, 0)?.unwrap_or_default();
            let (column, prefix) = (text(r, 1)?, text(r, 2)?);
            let Some(column) = column.filter(|_| prefix.is_none()) else {
                unusable.insert(name);
                continue;
            };
            match keys.iter_mut().find(|k| k.name == name) {
                Some(key) => key.columns.push(column),
                None => keys.push(UniqueKey {
                    primary: name == "PRIMARY",
                    name,
                    columns: vec![column],
                }),
            }
        }
        keys.retain(|k| !unusable.contains(&k.name));
        keys.sort_by_key(|k| !k.primary);
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
        let mut tx = pool.begin().await.map_err(Error::database)?;
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

    pub async fn open_console(&self, database: Option<&str>) -> Result<Console> {
        Console::mysql(&self.pool, database).await
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// Every projected value is rendered to text by the server, so decimals, dates and
/// 64-bit integers reach the grid without a lossy trip through floats.
fn page_sql(table: &TableRef, meta: &TableMeta, filter: &Where, order: &Order) -> Result<String> {
    let mut exprs = Vec::with_capacity(meta.columns.len());
    for c in &meta.columns {
        // One unit more than the preview shows, so a cut value can be told from a short one.
        exprs.push(cell_expr(c, MAX_CELL_CHARS + 1, MAX_BINARY_BYTES + 1)?);
    }
    let mut sql = format!(
        "SELECT {} FROM {}.{}",
        exprs.join(", "),
        ident::mysql(&table.database)?,
        ident::mysql(&table.name)?
    );
    sql.push_str(&filter.sql);
    if !order.is_empty() {
        let mut parts = Vec::with_capacity(order.len());
        for (col, desc) in order {
            parts.push(format!(
                "{}{}",
                ident::mysql(col)?,
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
    let col = ident::mysql(&c.name)?;
    let base = c
        .data_type
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Ok(if c.kind == ValueKind::Binary {
        format!("HEX(LEFT({col}, {bytes}))")
    } else if base == "bit" {
        format!("CAST({col} + 0 AS CHAR)")
    } else if SPATIAL_TYPES.contains(&base.as_str()) {
        format!("LEFT(ST_AsText({col}), {chars})")
    } else {
        format!("LEFT(CAST({col} AS CHAR), {chars})")
    })
}

/// `EXTRA` also says `DEFAULT_GENERATED` for plain expression defaults such as
/// `DEFAULT CURRENT_TIMESTAMP`; those columns are still writable.
fn is_generated(extra: &str) -> bool {
    extra.contains("auto_increment")
        || extra.contains("stored generated")
        || extra.contains("virtual generated")
}

fn value_kind(data_type: &str, column_type: &str) -> ValueKind {
    if column_type.eq_ignore_ascii_case("tinyint(1)") {
        return ValueKind::Bool;
    }
    match data_type {
        "tinyint" | "smallint" | "mediumint" | "int" | "bigint" | "decimal" | "float"
        | "double" | "year" | "bit" => ValueKind::Number,
        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext" | "enum" | "set" => {
            ValueKind::Text
        }
        "date" | "time" | "datetime" | "timestamp" => ValueKind::Temporal,
        "binary" | "varbinary" | "tinyblob" | "blob" | "mediumblob" | "longblob" => {
            ValueKind::Binary
        }
        "json" => ValueKind::Json,
        _ => ValueKind::Other,
    }
}

/// Read a string-typed column. MySQL reports several catalogue columns as binary
/// strings, so decode the raw bytes instead of relying on the declared type.
fn text(row: &MySqlRow, idx: usize) -> Result<Option<String>> {
    let bytes: Option<Vec<u8>> = row.try_get_unchecked(idx).map_err(Error::database)?;
    Ok(bytes.map(|b| match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }))
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

    #[test]
    fn builds_a_quoted_text_projection() {
        let meta = TableMeta {
            columns: vec![
                col("id", "bigint unsigned", ValueKind::Number),
                col("we`ird", "varchar(20)", ValueKind::Text),
                col("photo", "blob", ValueKind::Binary),
                col("flags", "bit(8)", ValueKind::Number),
                col("area", "polygon", ValueKind::Other),
            ],
            primary_key: vec!["id".into()],
            estimated_rows: None,
        };
        let table = TableRef {
            database: "shop".into(),
            schema: None,
            name: "items".into(),
        };
        let sql = page_sql(&table, &meta, &Where::default(), &vec![("id".into(), true)]).unwrap();
        assert_eq!(
            sql,
            "SELECT LEFT(CAST(`id` AS CHAR), 1001), LEFT(CAST(`we``ird` AS CHAR), 1001), \
             HEX(LEFT(`photo`, 25)), CAST(`flags` + 0 AS CHAR), LEFT(ST_AsText(`area`), 1001) \
             FROM `shop`.`items` ORDER BY `id` DESC LIMIT ? OFFSET ?"
        );
    }

    #[test]
    fn default_expressions_are_not_generated_columns() {
        assert!(is_generated("auto_increment"));
        assert!(is_generated("stored generated"));
        assert!(is_generated("virtual generated"));
        assert!(!is_generated("default_generated"));
        assert!(!is_generated(
            "default_generated on update current_timestamp"
        ));
        assert!(!is_generated(""));
    }

    #[test]
    fn classifies_types() {
        assert_eq!(value_kind("tinyint", "tinyint(1)"), ValueKind::Bool);
        assert_eq!(value_kind("tinyint", "tinyint(4)"), ValueKind::Number);
        assert_eq!(value_kind("decimal", "decimal(12,2)"), ValueKind::Number);
        assert_eq!(value_kind("datetime", "datetime(6)"), ValueKind::Temporal);
        assert_eq!(value_kind("json", "json"), ValueKind::Json);
        assert_eq!(value_kind("point", "point"), ValueKind::Other);
    }
}
