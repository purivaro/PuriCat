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

    /// The client sessions on the server ([`super::monitor`]).
    pub async fn processes(&self) -> Result<Vec<super::monitor::Process>> {
        use super::monitor::{postgres_list_sql, process};
        let rows = sqlx::query(&postgres_list_sql())
            .fetch_all(&self.pool(&self.home).await?)
            .await
            .map_err(Error::database)?;
        let text = |r: &sqlx::postgres::PgRow, i: usize| -> Option<String> {
            r.try_get::<Option<String>, _>(i).ok().flatten()
        };
        Ok(rows
            .iter()
            .filter_map(|r| {
                process(
                    crate::model::Engine::Postgres,
                    text(r, 0),
                    text(r, 1),
                    text(r, 2),
                    text(r, 3),
                    text(r, 4),
                    text(r, 5),
                    text(r, 6),
                    text(r, 7),
                    text(r, 8),
                    r.try_get::<Option<bool>, _>(9).ok().flatten() == Some(true),
                )
            })
            .collect())
    }

    /// `pg_cancel_backend` / `pg_terminate_backend`: `false` means no such session, or not
    /// one this account may signal.
    pub async fn signal(&self, sql: &str) -> Result<()> {
        let row = sqlx::query(sql)
            .fetch_one(&self.pool(&self.home).await?)
            .await
            .map_err(Error::database)?;
        if row.try_get::<Option<bool>, _>(0).map_err(Error::database)? == Some(true) {
            Ok(())
        } else {
            Err(Error::Database(
                "The server did not signal that session: it has ended, or this account may not signal it.".into(),
            ))
        }
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

    /// Close this session's own connections to `database`: the server refuses to drop or
    /// rename a database that anyone, PuriCat included, is still connected to.
    pub async fn release(&self, database: &str) {
        let pool = self.pools.lock().await.remove(database);
        if let Some(pool) = pool {
            pool.close().await;
        }
    }

    /// The database this session was opened on.
    pub fn home(&self) -> &str {
        &self.home
    }

    /// Run one statement that is not a query and has no parameters (DDL from a form), on
    /// the session's own database and outside any transaction.
    pub async fn execute_statement(&self, sql: &str) -> Result<()> {
        self.execute_on(&self.home, sql).await
    }

    /// The same, on `database`: a table is changed from inside the database that holds it.
    pub async fn execute_on(&self, database: &str, sql: &str) -> Result<()> {
        sqlx::raw_sql(sql)
            .execute(&self.pool(database).await?)
            .await
            .map_err(Error::database)?;
        Ok(())
    }

    /// One statement on `database`, for [`super::Session::execute_one`]: sent through the
    /// extended protocol, where the server refuses a text that holds more than one statement
    /// ("cannot insert multiple commands into a prepared statement").
    pub async fn execute_one_on(&self, database: &str, sql: &str) -> Result<()> {
        sqlx::query(sql)
            .persistent(false)
            .execute(&self.pool(database).await?)
            .await
            .map_err(Error::database)?;
        Ok(())
    }

    /// The query of a plain view, as `pg_get_viewdef` prints it.
    pub async fn view_select(&self, table: &TableRef) -> Result<String> {
        let schema = super::require_schema(table.schema.as_deref())?;
        self.query_text(
            &table.database,
            "SELECT pg_get_viewdef(c.oid, true)
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind = 'v'",
            &[Some(schema.to_string()), Some(table.name.clone())],
        )
        .await?
        .into_iter()
        .next()
        .and_then(|row| row.into_iter().next().flatten())
        .map(|query| query.trim().trim_end_matches(';').trim_end().to_string())
        .ok_or_else(|| Error::NotFound(format!("View {}", table.name)))
    }

    /// The statements that would create a table or a view as it is now. PostgreSQL has no
    /// statement that prints them: a view's is its stored query under a `CREATE VIEW` line,
    /// and a table's is put together from the catalogue with the server's own `pg_get_…`
    /// functions. Not included: owner, grants, row-level-security policies, storage
    /// parameters, inheritance and the options of identity sequences.
    pub async fn table_definition(&self, table: &TableRef, kind: ObjectKind) -> Result<String> {
        let schema = super::require_schema(table.schema.as_deref())?;
        let name = format!(
            "{}.{}",
            ident::postgres(schema)?,
            ident::postgres(&table.name)?
        );
        let kinds = if kind == ObjectKind::Table {
            "'r', 'p'"
        } else {
            "'v', 'm'"
        };
        let found = self
            .query_text(
                &table.database,
                &format!(
                    "SELECT c.oid::text, c.relkind::text, c.relpersistence::text,
                            CASE WHEN c.relkind = 'p' THEN pg_get_partkeydef(c.oid) END,
                            quote_literal(obj_description(c.oid, 'pg_class'))
                     FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
                     WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ({kinds})"
                ),
                &[Some(schema.to_string()), Some(table.name.clone())],
            )
            .await?;
        let mut found = found
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound(format!("Table {}", table.name)))?
            .into_iter();
        let mut next = || found.next().flatten();
        let oid = [next()];
        let (relkind, persistence, partition_key, comment) = (
            next().unwrap_or_default(),
            next().unwrap_or_default(),
            next(),
            next(),
        );
        // One column of text per row, read by the object's oid.
        let lines = |sql: &'static str| async {
            let rows = self.query_text(&table.database, sql, &oid).await?;
            Ok::<_, Error>(
                rows.into_iter()
                    .filter_map(|r| r.into_iter().next().flatten())
                    .collect::<Vec<_>>(),
            )
        };
        let ended = |s: String| format!("{};", s.trim_end().trim_end_matches(';'));

        let mut parts: Vec<String> = Vec::new();
        let word = match relkind.as_str() {
            "v" => "VIEW",
            "m" => "MATERIALIZED VIEW",
            _ => "TABLE",
        };
        if kind == ObjectKind::Table {
            let columns = self
                .query_text(
                    &table.database,
                    "SELECT quote_ident(a.attname), format_type(a.atttypid, a.atttypmod),
                            CASE WHEN a.attcollation <> t.typcollation
                                 THEN quote_ident(cn.nspname) || '.' || quote_ident(co.collname) END,
                            a.attidentity::text, a.attgenerated::text,
                            pg_get_expr(d.adbin, d.adrelid), a.attnotnull::text
                     FROM pg_attribute a
                     JOIN pg_type t ON t.oid = a.atttypid
                     LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
                     LEFT JOIN pg_collation co ON co.oid = a.attcollation
                     LEFT JOIN pg_namespace cn ON cn.oid = co.collnamespace
                     WHERE a.attrelid = $1::oid AND a.attnum > 0 AND NOT a.attisdropped
                     ORDER BY a.attnum",
                    &oid,
                )
                .await?;
            let mut body: Vec<String> = Vec::with_capacity(columns.len());
            for c in columns {
                let cell = |i: usize| c.get(i).cloned().flatten().unwrap_or_default();
                let mut line = format!("    {} {}", cell(0), cell(1));
                if !cell(2).is_empty() {
                    line.push_str(&format!(" COLLATE {}", cell(2)));
                }
                match (cell(3).as_str(), cell(4).as_str()) {
                    ("a", _) => line.push_str(" GENERATED ALWAYS AS IDENTITY"),
                    ("d", _) => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
                    (_, "s") => {
                        line.push_str(&format!(" GENERATED ALWAYS AS ({}) STORED", cell(5)))
                    }
                    _ if !cell(5).is_empty() => line.push_str(&format!(" DEFAULT {}", cell(5))),
                    _ => {}
                }
                if cell(6) == "true" {
                    line.push_str(" NOT NULL");
                }
                body.push(line);
            }
            // Primary key, unique, check and exclusion constraints, then foreign keys.
            body.extend(
                lines(
                    "SELECT '    CONSTRAINT ' || quote_ident(conname) || ' ' || pg_get_constraintdef(oid, true)
                     FROM pg_constraint
                     WHERE conrelid = $1::oid AND contype IN ('p', 'u', 'c', 'x', 'f')
                     ORDER BY array_position(ARRAY['p', 'u', 'c', 'x', 'f'], contype::text), conname",
                )
                .await?,
            );
            let unlogged = if persistence == "u" { "UNLOGGED " } else { "" };
            let mut create = format!("CREATE {unlogged}TABLE {name} (\n{}\n)", body.join(",\n"));
            if let Some(key) = partition_key {
                create.push_str(&format!("\nPARTITION BY {key}"));
            }
            parts.push(ended(create));
        } else {
            let query = lines("SELECT pg_get_viewdef($1::oid, true)").await?;
            parts.push(ended(format!(
                "CREATE {word} {name} AS\n{}",
                query.concat()
            )));
        }

        // Indexes that are not the ones a constraint above creates by itself.
        let indexes = lines(
            "SELECT pg_get_indexdef(i.indexrelid)
             FROM pg_index i JOIN pg_class ic ON ic.oid = i.indexrelid
             WHERE i.indrelid = $1::oid
               AND NOT EXISTS (SELECT 1 FROM pg_constraint k WHERE k.conindid = i.indexrelid)
             ORDER BY ic.relname",
        )
        .await?;
        let triggers = lines(
            "SELECT pg_get_triggerdef(oid, true) FROM pg_trigger
             WHERE tgrelid = $1::oid AND NOT tgisinternal ORDER BY tgname",
        )
        .await?;
        let mut comments: Vec<String> = comment
            .into_iter()
            .map(|text| format!("COMMENT ON {word} {name} IS {text};"))
            .collect();
        comments.extend(
            self.query_text(
                &table.database,
                "SELECT quote_ident(a.attname), quote_literal(col_description(a.attrelid, a.attnum))
                 FROM pg_attribute a
                 WHERE a.attrelid = $1::oid AND a.attnum > 0 AND NOT a.attisdropped
                   AND col_description(a.attrelid, a.attnum) IS NOT NULL
                 ORDER BY a.attnum",
                &oid,
            )
            .await?
            .into_iter()
            .filter_map(|r| match (r.first().cloned().flatten(), r.get(1).cloned().flatten()) {
                (Some(column), Some(text)) => Some(format!("COMMENT ON COLUMN {name}.{column} IS {text};")),
                _ => None,
            }),
        );
        for group in [indexes, triggers] {
            if !group.is_empty() {
                parts.push(group.into_iter().map(ended).collect::<Vec<_>>().join("\n"));
            }
        }
        if !comments.is_empty() {
            parts.push(comments.join("\n"));
        }
        Ok(parts.join("\n\n"))
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
        // One unit more than the preview shows, so a cut value can be told from a short one.
        exprs.push(cell_expr(c, MAX_CELL_CHARS + 1, MAX_BINARY_BYTES + 1)?);
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

/// One column as the text the grid shows: at most `chars` characters, or `bytes` bytes in hex.
pub(super) fn cell_expr(c: &ColumnMeta, chars: usize, bytes: usize) -> Result<String> {
    let col = ident::postgres(&c.name)?;
    Ok(if c.kind == ValueKind::Binary {
        format!("encode(substring({col} from 1 for {bytes}), 'hex')")
    } else {
        format!("left({col}::text, {chars})")
    })
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
