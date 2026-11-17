use std::collections::HashMap;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
use sqlx::{ConnectOptions, Executor, Row};
use tokio::sync::Mutex;

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
    schemas: true,
    transactional_ddl: true,
    returning: true,
};

const DEFAULT_DATABASE: &str = "postgres";

/// A PostgreSQL connection is bound to one database, so browsing several databases on
/// one server needs one small pool per database. Pools are opened on first use.
pub struct PgSession {
    options: PgConnectOptions,
    read_only: bool,
    timeout: Duration,
    home: String,
    pools: Mutex<HashMap<String, PgPool>>,
}

impl PgSession {
    pub async fn connect(profile: &ConnectionProfile, password: Option<&str>) -> Result<Self> {
        let mut options = PgConnectOptions::new()
            .host(&profile.host)
            .port(profile.port)
            .username(&profile.username)
            .application_name("PuriCat")
            .ssl_mode(match profile.tls {
                TlsMode::Disable => PgSslMode::Disable,
                TlsMode::Prefer => PgSslMode::Prefer,
                TlsMode::Require => PgSslMode::Require,
                TlsMode::VerifyCa => PgSslMode::VerifyCa,
                TlsMode::VerifyFull => PgSslMode::VerifyFull,
            });
        if let Some(pw) = password {
            options = options.password(pw);
        }
        if let Some(ca) = &profile.tls_ca_path {
            options = options.ssl_root_cert(ca);
        }
        let home = profile
            .database
            .clone()
            .unwrap_or_else(|| DEFAULT_DATABASE.to_string());
        let session = PgSession {
            options,
            read_only: profile.read_only,
            timeout: Duration::from_secs(profile.connect_timeout_secs as u64),
            home,
            pools: Mutex::new(HashMap::new()),
        };
        // Fail now, not on first use, if the credentials or the database are wrong.
        session.pool(&session.home).await?;
        Ok(session)
    }

    async fn pool(&self, database: &str) -> Result<PgPool> {
        let mut pools = self.pools.lock().await;
        if let Some(p) = pools.get(database) {
            return Ok(p.clone());
        }
        let options = self.options.clone().database(database);
        // See the MySQL adapter: probe first so the real connection error is reported.
        super::probe(self.timeout, options.connect()).await?;

        let read_only = self.read_only;
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .acquire_timeout(self.timeout)
            .idle_timeout(Some(Duration::from_secs(600)))
            .after_connect(move |conn, _meta| {
                Box::pin(async move {
                    if read_only {
                        conn.execute("SET default_transaction_read_only = on")
                            .await?;
                    }
                    Ok(())
                })
            })
            .connect_lazy_with(options);
        pools.insert(database.to_string(), pool.clone());
        Ok(pool)
    }

    pub async fn version(&self) -> Result<String> {
        let row = sqlx::query("SELECT current_setting('server_version')")
            .fetch_one(&self.pool(&self.home).await?)
            .await
            .map_err(Error::database)?;
        row.try_get(0).map_err(Error::database)
    }

    pub async fn tls(&self) -> Result<Option<bool>> {
        let row = sqlx::query("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
            .fetch_optional(&self.pool(&self.home).await?)
            .await
            .map_err(Error::database)?;
        match row {
            Some(r) => Ok(Some(r.try_get(0).map_err(Error::database)?)),
            None => Ok(None),
        }
    }

    pub async fn read_only(&self) -> Result<bool> {
        let row = sqlx::query("SELECT current_setting('transaction_read_only')")
            .fetch_one(&self.pool(&self.home).await?)
            .await
            .map_err(Error::database)?;
        let value: String = row.try_get(0).map_err(Error::database)?;
        Ok(value == "on")
    }

    pub async fn list_databases(&self) -> Result<Vec<Namespace>> {
        let rows = sqlx::query(
            "SELECT datname::text FROM pg_database
             WHERE datallowconn AND NOT datistemplate ORDER BY datname",
        )
        .fetch_all(&self.pool(&self.home).await?)
        .await
        .map_err(Error::database)?;
        rows.iter()
            .map(|r| {
                Ok(Namespace {
                    name: r.try_get(0).map_err(Error::database)?,
                    system: false,
                })
            })
            .collect()
    }

    pub async fn list_schemas(&self, database: &str) -> Result<Vec<Namespace>> {
        let rows = sqlx::query(
            "SELECT nspname::text,
                    (nspname = 'information_schema' OR nspname LIKE 'pg\\_%') AS system
             FROM pg_namespace
             WHERE nspname NOT LIKE 'pg\\_toast%' AND nspname NOT LIKE 'pg\\_temp%'
             ORDER BY 2, 1",
        )
        .fetch_all(&self.pool(database).await?)
        .await
        .map_err(Error::database)?;
        rows.iter()
            .map(|r| {
                Ok(Namespace {
                    name: r.try_get(0).map_err(Error::database)?,
                    system: r.try_get(1).map_err(Error::database)?,
                })
            })
            .collect()
    }

    pub async fn list_objects(&self, database: &str, schema: &str) -> Result<Vec<DbObject>> {
        let rows = sqlx::query(
            "SELECT c.relname::text, c.relkind::text
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relkind IN ('r', 'p', 'f', 'v', 'm')
             ORDER BY c.relname",
        )
        .bind(schema)
        .fetch_all(&self.pool(database).await?)
        .await
        .map_err(Error::database)?;
        rows.iter()
            .map(|r| {
                let relkind: String = r.try_get(1).map_err(Error::database)?;
                Ok(DbObject {
                    name: r.try_get(0).map_err(Error::database)?,
                    kind: match relkind.as_str() {
                        "v" => ObjectKind::View,
                        "m" => ObjectKind::Matview,
                        _ => ObjectKind::Table, // ordinary, partitioned, foreign
                    },
                })
            })
            .collect()
    }

    pub async fn describe_table(&self, table: &TableRef) -> Result<TableMeta> {
        let schema = super::require_schema(table.schema.as_deref())?;
        let pool = self.pool(&table.database).await?;

        let pk_rows = sqlx::query(
            "SELECT a.attname::text
             FROM pg_index i
             JOIN pg_class c ON c.oid = i.indrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) ON true
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
             WHERE i.indisprimary AND n.nspname = $1 AND c.relname = $2
             ORDER BY k.ord",
        )
        .bind(schema)
        .bind(&table.name)
        .fetch_all(&pool)
        .await
        .map_err(Error::database)?;
        let mut primary_key: Vec<String> = Vec::with_capacity(pk_rows.len());
        for r in &pk_rows {
            primary_key.push(r.try_get(0).map_err(Error::database)?);
        }

        let rows = sqlx::query(
            "SELECT a.attname::text,
                    format_type(a.atttypid, a.atttypmod),
                    NOT a.attnotnull,
                    pg_get_expr(d.adbin, d.adrelid),
                    t.typcategory::text,
                    t.typname::text,
                    (a.attidentity <> '' OR a.attgenerated <> ''),
                    format_type(a.atttypid, NULL)
             FROM pg_attribute a
             JOIN pg_class c ON c.oid = a.attrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_type t ON t.oid = a.atttypid
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
             ORDER BY a.attnum",
        )
        .bind(schema)
        .bind(&table.name)
        .fetch_all(&pool)
        .await
        .map_err(Error::database)?;
        let mut columns = Vec::with_capacity(rows.len());
        for r in &rows {
            let name: String = r.try_get(0).map_err(Error::database)?;
            let default: Option<String> = r.try_get(3).map_err(Error::database)?;
            let category: String = r.try_get(4).map_err(Error::database)?;
            let typname: String = r.try_get(5).map_err(Error::database)?;
            let identity: bool = r.try_get(6).map_err(Error::database)?;
            let serial = default
                .as_deref()
                .is_some_and(|d| d.starts_with("nextval("));
            let kind = value_kind(&category, &typname);
            let generated = identity || serial;
            columns.push(ColumnMeta {
                data_type: r.try_get(1).map_err(Error::database)?,
                kind,
                editable: !generated && kind != ValueKind::Binary,
                nullable: r.try_get(2).map_err(Error::database)?,
                primary_key: primary_key.contains(&name),
                generated,
                base_type: r.try_get(7).map_err(Error::database)?,
                default,
                name,
            });
        }

        let estimated_rows = sqlx::query(
            "SELECT c.reltuples::bigint
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2",
        )
        .bind(schema)
        .bind(&table.name)
        .fetch_optional(&pool)
        .await
        .map_err(Error::database)?
        .and_then(|r| r.try_get::<i64, _>(0).ok())
        .filter(|n| *n >= 0); // -1: never analysed

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
            .bind(req.limit as i64 + 1)
            .bind(req.offset as i64)
            .fetch_all(&self.pool(&table.database).await?)
            .await
            .map_err(Error::database)?;
        let width = meta.columns.len();
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let mut cells = Vec::with_capacity(width);
            for i in 0..width {
                cells.push(r.try_get::<Option<String>, _>(i).map_err(Error::database)?);
            }
            out.push(cells);
        }
        Ok(out)
    }

    /// The primary key and every plain unique index a row can be matched on. Partial and
    /// expression indexes are left out: equality on them is not row identity.
    pub async fn unique_keys(&self, table: &TableRef) -> Result<Vec<UniqueKey>> {
        let schema = super::require_schema(table.schema.as_deref())?;
        let rows = sqlx::query(
            "SELECT ic.relname::text, i.indisprimary, a.attname::text
             FROM pg_index i
             JOIN pg_class c ON c.oid = i.indrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_class ic ON ic.oid = i.indexrelid
             JOIN unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) ON k.ord <= i.indnkeyatts
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
             WHERE n.nspname = $1 AND c.relname = $2
               AND i.indisunique AND i.indisvalid AND i.indpred IS NULL AND i.indexprs IS NULL
             ORDER BY i.indisprimary DESC, ic.relname, k.ord",
        )
        .bind(schema)
        .bind(&table.name)
        .fetch_all(&self.pool(&table.database).await?)
        .await
        .map_err(Error::database)?;
        let mut keys: Vec<UniqueKey> = Vec::new();
        for r in &rows {
            let name: String = r.try_get(0).map_err(Error::database)?;
            let column: String = r.try_get(2).map_err(Error::database)?;
            match keys.iter_mut().find(|k| k.name == name) {
                Some(key) => key.columns.push(column),
                None => keys.push(UniqueKey {
                    primary: r.try_get(1).map_err(Error::database)?,
                    name,
                    columns: vec![column],
                }),
            }
        }
        Ok(keys)
    }

    /// Run planned statements in one transaction; each must touch exactly one row.
    /// `keep_going` is called before every statement with the number already done;
    /// returning `false` cancels and rolls everything back.
    pub async fn apply(
        &self,
        database: &str,
        statements: &[Statement],
        keep_going: &(dyn Fn(usize) -> bool + Send + Sync),
    ) -> Result<()> {
        let pool = self.pool(database).await?;
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
        database: &str,
        sql: &str,
        params: &[Option<String>],
    ) -> Result<Vec<Vec<Option<String>>>> {
        let pool = self.pool(database).await?;
        let mut query = sqlx::query(sql);
        for param in params {
            query = query.bind(param.as_deref());
        }
        let rows = query.fetch_all(&pool).await.map_err(Error::database)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let mut cells = Vec::with_capacity(r.len());
            for i in 0..r.len() {
                cells.push(r.try_get::<Option<String>, _>(i).map_err(Error::database)?);
            }
            out.push(cells);
        }
        Ok(out)
    }

    pub async fn open_console(&self, database: Option<&str>) -> Result<Console> {
        Console::postgres(&self.pool(database.unwrap_or(&self.home)).await?).await
    }

    pub async fn close(&self) {
        for (_, pool) in self.pools.lock().await.drain() {
            pool.close().await;
        }
    }
}

/// Every projected value is cast to text by the server (`numeric`, `timestamptz`, arrays,
/// enums, ...), so nothing is reinterpreted on the way to the grid.
fn page_sql(table: &TableRef, meta: &TableMeta, filter: &Where, order: &Order) -> Result<String> {
    let schema = super::require_schema(table.schema.as_deref())?;
    let mut exprs = Vec::with_capacity(meta.columns.len());
    for c in &meta.columns {
        let col = ident::postgres(&c.name)?;
        exprs.push(if c.kind == ValueKind::Binary {
            format!(
                "encode(substring({col} from 1 for {}), 'hex')",
                MAX_BINARY_BYTES + 1
            )
        } else {
            format!("left({col}::text, {})", MAX_CELL_CHARS + 1)
        });
    }
    let mut sql = format!(
        "SELECT {} FROM {}.{}",
        exprs.join(", "),
        ident::postgres(schema)?,
        ident::postgres(&table.name)?
    );
    sql.push_str(&filter.sql);
    if !order.is_empty() {
        let mut parts = Vec::with_capacity(order.len());
        for (col, desc) in order {
            parts.push(format!(
                "{}{}",
                ident::postgres(col)?,
                if *desc { " DESC" } else { "" }
            ));
        }
        sql.push_str(" ORDER BY ");
        sql.push_str(&parts.join(", "));
    }
    // The filter's parameters come first; the page bounds follow them.
    let n = filter.params.len();
    sql.push_str(&format!(" LIMIT ${} OFFSET ${}", n + 1, n + 2));
    Ok(sql)
}

fn value_kind(typcategory: &str, typname: &str) -> ValueKind {
    match (typcategory, typname) {
        (_, "bytea") => ValueKind::Binary,
        (_, "json" | "jsonb") => ValueKind::Json,
        ("N", _) => ValueKind::Number,
        ("S", _) => ValueKind::Text,
        ("B", _) => ValueKind::Bool,
        ("D" | "T", _) => ValueKind::Temporal,
        _ => ValueKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_quoted_text_projection() {
        let col = |name: &str, kind| ColumnMeta {
            name: name.into(),
            data_type: "x".into(),
            kind,
            nullable: true,
            default: None,
            primary_key: false,
            generated: false,
            editable: true,
            base_type: "x".into(),
        };
        let meta = TableMeta {
            columns: vec![
                col("id", ValueKind::Number),
                col("da\"ta", ValueKind::Binary),
            ],
            primary_key: vec![],
            estimated_rows: None,
        };
        let table = TableRef {
            database: "app".into(),
            schema: Some("public".into()),
            name: "Files".into(),
        };
        assert_eq!(
            page_sql(&table, &meta, &Where::default(), &vec![]).unwrap(),
            "SELECT left(\"id\"::text, 1001), encode(substring(\"da\"\"ta\" from 1 for 25), 'hex') \
             FROM \"public\".\"Files\" LIMIT $1 OFFSET $2"
        );
        let no_schema = TableRef {
            schema: None,
            ..table.clone()
        };
        assert!(page_sql(&no_schema, &meta, &Where::default(), &vec![]).is_err());

        // A filter's parameters are numbered first; the page bounds follow them.
        let filter = Where {
            sql: " WHERE \"id\" = $1::bigint".into(),
            params: vec!["7".into()],
        };
        let sql = page_sql(&table, &meta, &filter, &vec![("id".into(), false)]).unwrap();
        assert!(
            sql.ends_with(
                "FROM \"public\".\"Files\" WHERE \"id\" = $1::bigint ORDER BY \"id\" LIMIT $2 OFFSET $3"
            ),
            "{sql}"
        );
    }

    #[test]
    fn classifies_types() {
        assert_eq!(value_kind("N", "numeric"), ValueKind::Number);
        assert_eq!(value_kind("U", "bytea"), ValueKind::Binary);
        assert_eq!(value_kind("U", "jsonb"), ValueKind::Json);
        assert_eq!(value_kind("D", "timestamptz"), ValueKind::Temporal);
        assert_eq!(value_kind("A", "_int4"), ValueKind::Other);
    }
}
