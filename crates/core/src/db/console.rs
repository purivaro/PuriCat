//! The SQL console: runs what the user typed on a connection of its own.
//!
//! Each query tab owns one dedicated connection, so session state (`USE`, `SET`, temporary
//! tables, an open transaction) behaves as it would in a terminal client. Statements are
//! sent with the simple/text protocol, where the server renders every value as text — the
//! same exactness the grid has, for any result shape.
//!
//! Safeguards here are about what a statement *did*, not what it looks like:
//! - row-changing statements run inside a guard transaction and are rolled back when they
//!   touch more rows than the caller allows (unless the user is managing a transaction);
//! - a result is capped at `max_rows`; a plain query is cancelled on the server past that,
//!   while anything that writes runs to its end (cancelling it would undo it).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::{Column, Either, Executor, Row, TypeInfo, ValueRef};

use super::script::{self, Kind};
use super::{ValueKind, MAX_CELL_CHARS};
use crate::model::Engine;
use crate::{Error, Result};

pub const MAX_RESULT_ROWS: u32 = 100_000;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunOptions {
    /// Rows kept per result set; the query is cancelled beyond this.
    pub max_rows: u32,
    /// A row-changing statement that affects more rows than this is rolled back.
    /// `None` lifts the limit (the user confirmed a large change).
    #[serde(default)]
    pub change_limit: Option<u64>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Error,
    /// Rolled back by the impact guard: it changed more rows than allowed.
    Blocked,
    Cancelled,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultColumn {
    pub name: String,
    /// Type as the driver names it (`INT`, `VARCHAR`, `TIMESTAMPTZ`, ...).
    pub type_name: String,
    pub kind: ValueKind,
}

/// A full result set as fetched, kept by the console so it can be exported losslessly.
#[derive(Clone, Debug, Default)]
pub struct ResultSet {
    pub columns: Vec<ResultColumn>,
    pub rows: Vec<Vec<Option<String>>>,
}

/// What the UI gets for a result set: the same rows, with long cells cut for display.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResultPreview {
    /// Index into the console's stored results, for export.
    pub id: usize,
    pub columns: Vec<ResultColumn>,
    pub rows: Vec<Vec<Option<String>>>,
    /// `[row, column]` of cells cut to the preview length (the export has them in full).
    pub truncated: Vec<[u32; 2]>,
    /// The result was cut at `max_rows`; more rows exist on the server.
    pub capped: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StatementOutcome {
    pub sql: String,
    pub kind: Kind,
    pub status: Status,
    pub results: Vec<ResultPreview>,
    /// Rows changed, for statements that do not return rows.
    pub rows_affected: Option<u64>,
    pub message: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
    pub statements: Vec<StatementOutcome>,
    /// Statements of the script that were not reached (after an error, block or cancel).
    pub skipped: usize,
    /// When the impact guard blocked a statement: that statement and the rest of the
    /// script, so the user can run exactly the part that has not happened yet.
    pub resume: Option<String>,
    /// The session is inside a transaction the user opened (or MySQL autocommit is off).
    pub in_transaction: bool,
    /// The tab's connection died during this run; the caller should open a new console.
    pub connection_lost: bool,
    pub elapsed_ms: u64,
}

/// Detached from the session's pool, so open query tabs never starve the explorer and grid.
enum Conn {
    MySql(sqlx::MySqlConnection),
    Postgres(sqlx::PgConnection),
}

/// Stops whatever the console's connection is running, from another connection.
#[derive(Clone)]
pub enum Canceller {
    MySql {
        pool: sqlx::MySqlPool,
        connection_id: u64,
    },
    Postgres {
        pool: sqlx::PgPool,
        pid: i32,
    },
}

impl Canceller {
    pub async fn cancel(&self) -> Result<()> {
        match self {
            // The id is a number reported by the server; KILL takes no bind parameters.
            Canceller::MySql {
                pool,
                connection_id,
            } => {
                pool.execute(format!("KILL QUERY {connection_id}").as_str())
                    .await
                    .map_err(Error::database)?;
            }
            Canceller::Postgres { pool, pid } => {
                sqlx::query("SELECT pg_cancel_backend($1)")
                    .bind(pid)
                    .execute(pool)
                    .await
                    .map_err(Error::database)?;
            }
        }
        Ok(())
    }
}

pub struct Console {
    conn: Conn,
    canceller: Canceller,
    /// The user ran BEGIN and has not ended it: the guard must not add its own transaction.
    in_transaction: bool,
    /// MySQL's `@@autocommit`, when known. With autocommit off every statement already
    /// runs inside a transaction that the user ends, so the guard must stay out of it.
    autocommit: Option<bool>,
    /// The connection failed below the SQL level; nothing more can run on it.
    lost: bool,
    /// Results of the last run, in full.
    stored: Vec<ResultSet>,
}

struct Executed {
    sets: Vec<(ResultSet, bool)>,
    affected: Option<u64>,
    error: Option<String>,
    /// Stop was asked for while the statement ran. An interrupted statement does not
    /// always fail (MySQL's SLEEP just returns), so this is the only dependable sign.
    cancelled: bool,
    /// The error came from the connection, not from the server's answer.
    lost: bool,
}

fn value_kind(type_name: &str) -> ValueKind {
    let t = type_name.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| t.contains(n));
    if has(&["interval"]) {
        ValueKind::Temporal // before "int", which it contains
    } else if has(&["point"]) {
        ValueKind::Text // likewise
    } else if has(&["bool"]) {
        ValueKind::Bool
    } else if has(&["json"]) {
        ValueKind::Json
    } else if has(&["blob", "binary", "bytea", "geometry"]) {
        ValueKind::Binary
    } else if has(&[
        "int", "numeric", "decimal", "float", "double", "real", "money", "serial", "oid", "year",
        "bit",
    ]) {
        ValueKind::Number
    } else if has(&["date", "time", "interval"]) {
        ValueKind::Temporal
    } else {
        ValueKind::Text
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("0x");
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn is_cancellation(e: &sqlx::Error) -> bool {
    // PostgreSQL: query_canceled. MySQL: ER_QUERY_INTERRUPTED (SQLSTATE 70100).
    e.as_database_error()
        .and_then(|d| d.code().map(|c| c == "57014" || c == "70100"))
        .unwrap_or(false)
}

/// Did this error end the connection, as opposed to one statement?
fn connection_ended(e: &sqlx::Error) -> bool {
    let sqlx::Error::Database(db) = e else {
        return true; // I/O, protocol, TLS: nothing more can be said on this connection
    };
    if let Some(mysql) = db.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>() {
        // Server shutdown, connection killed, disconnected for inactivity.
        return matches!(mysql.number(), 1053 | 1927 | 4031);
    }
    // PostgreSQL: connection exceptions, and an operator ending the session.
    db.code()
        .is_some_and(|c| c.starts_with("08") || matches!(c.as_ref(), "57P01" | "57P02" | "57P03"))
}

fn message_of(e: &sqlx::Error) -> String {
    match e {
        sqlx::Error::Database(db) => db.message().to_string(),
        other => other.to_string(),
    }
}

/// Collect one statement's rows and counts from the driver's stream, generically.
macro_rules! drain {
    ($conn:expr, $sql:expr, $max:expr, $cancel_at_cap:expr, $stop:expr, $canceller:expr, $decode:expr) => {{
        let mut out = Executed {
            sets: Vec::new(),
            affected: None,
            error: None,
            cancelled: false,
            lost: false,
        };
        let mut open: Option<(ResultSet, bool)> = None;
        let mut asked_to_stop = false;
        let mut stream = sqlx::raw_sql($sql).fetch_many(&mut *$conn);
        while let Some(item) = stream.next().await {
            if $stop.load(Ordering::Relaxed) && !asked_to_stop {
                asked_to_stop = true;
                out.cancelled = true;
                let _ = $canceller.cancel().await;
            }
            match item {
                Ok(Either::Left(done)) => {
                    out.affected = Some(out.affected.unwrap_or(0) + done.rows_affected());
                    out.sets.extend(open.take());
                }
                Ok(Either::Right(row)) => {
                    let (set, capped) = open.get_or_insert_with(|| {
                        let columns = row
                            .columns()
                            .iter()
                            .map(|c| {
                                let type_name = c.type_info().name().to_string();
                                ResultColumn {
                                    name: c.name().to_string(),
                                    kind: value_kind(&type_name),
                                    type_name,
                                }
                            })
                            .collect();
                        (
                            ResultSet {
                                columns,
                                rows: Vec::new(),
                            },
                            false,
                        )
                    });
                    if set.rows.len() < $max as usize {
                        let mut cells = Vec::with_capacity(set.columns.len());
                        for i in 0..set.columns.len() {
                            cells.push($decode(&row, i, &set.columns[i]));
                        }
                        set.rows.push(cells);
                    } else if !*capped {
                        // Enough rows. A plain query is stopped on the server; otherwise the
                        // rest is read and dropped so the statement completes.
                        *capped = true;
                        if $cancel_at_cap && !asked_to_stop {
                            asked_to_stop = true;
                            let _ = $canceller.cancel().await;
                        }
                    }
                }
                // Interrupted by Stop, or simply cut at the cap.
                Err(e) if asked_to_stop && is_cancellation(&e) => break,
                Err(e) => {
                    out.lost = connection_ended(&e);
                    out.error = Some(message_of(&e));
                    break;
                }
            }
        }
        out.sets.extend(open.take());
        out
    }};
}

fn decode_mysql(row: &sqlx::mysql::MySqlRow, i: usize, column: &ResultColumn) -> Option<String> {
    // Text protocol: the server already rendered the value; binary types arrive as bytes.
    let bytes: Option<Vec<u8>> = row.try_get_unchecked(i).ok()?;
    let bytes = bytes?;
    Some(if column.type_name.eq_ignore_ascii_case("BIT") {
        bytes
            .iter()
            .fold(0u64, |n, b| (n << 8) | *b as u64)
            .to_string()
    } else if column.kind == ValueKind::Binary {
        hex(&bytes)
    } else {
        match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    })
}

fn decode_postgres(row: &sqlx::postgres::PgRow, i: usize, column: &ResultColumn) -> Option<String> {
    let raw = row.try_get_raw(i).ok()?;
    if raw.is_null() {
        return None;
    }
    let text = raw.as_str().ok()?;
    // bytea is rendered as \x...; show it like MySQL binaries.
    Some(match text.strip_prefix("\\x") {
        Some(rest) if column.kind == ValueKind::Binary => format!("0x{rest}"),
        _ => text.to_string(),
    })
}

impl Console {
    pub(crate) async fn mysql(pool: &sqlx::MySqlPool, database: Option<&str>) -> Result<Self> {
        // Acquired through the pool so the session set-up (strict mode, read-only) applies.
        let mut conn = pool.acquire().await.map_err(Error::database)?.detach();
        if let Some(db) = database {
            let quoted = super::ident::mysql(db)?;
            conn.execute(format!("USE {quoted}").as_str())
                .await
                .map_err(Error::database)?;
        }
        let row = sqlx::query("SELECT CAST(CONNECTION_ID() AS CHAR)")
            .fetch_one(&mut conn)
            .await
            .map_err(Error::database)?;
        let id: Vec<u8> = row.try_get_unchecked(0).map_err(Error::database)?;
        let connection_id = String::from_utf8_lossy(&id).parse().unwrap_or(0);
        Ok(Console {
            conn: Conn::MySql(conn),
            canceller: Canceller::MySql {
                pool: pool.clone(),
                connection_id,
            },
            in_transaction: false,
            autocommit: None,
            lost: false,
            stored: Vec::new(),
        })
    }

    pub(crate) async fn postgres(pool: &sqlx::PgPool) -> Result<Self> {
        let mut conn = pool.acquire().await.map_err(Error::database)?.detach();
        let row = sqlx::query("SELECT pg_backend_pid()")
            .fetch_one(&mut conn)
            .await
            .map_err(Error::database)?;
        let pid: i32 = row.try_get(0).map_err(Error::database)?;
        Ok(Console {
            conn: Conn::Postgres(conn),
            canceller: Canceller::Postgres {
                pool: pool.clone(),
                pid,
            },
            in_transaction: false,
            autocommit: Some(true),
            lost: false,
            stored: Vec::new(),
        })
    }

    pub fn engine(&self) -> Engine {
        match self.conn {
            Conn::MySql(_) => Engine::Mysql,
            Conn::Postgres(_) => Engine::Postgres,
        }
    }

    pub fn canceller(&self) -> Canceller {
        self.canceller.clone()
    }

    /// A full result of the last run, for export.
    pub fn result(&self, id: usize) -> Result<&ResultSet> {
        self.stored
            .get(id)
            .ok_or_else(|| Error::NotFound("That result (run the query again)".into()))
    }

    async fn execute(
        &mut self,
        sql: &str,
        max_rows: u32,
        cancel_at_cap: bool,
        stop: &AtomicBool,
    ) -> Executed {
        let canceller = self.canceller.clone();
        let out = match &mut self.conn {
            Conn::MySql(conn) => {
                drain!(
                    conn,
                    sql,
                    max_rows,
                    cancel_at_cap,
                    stop,
                    canceller,
                    decode_mysql
                )
            }
            Conn::Postgres(conn) => {
                drain!(
                    conn,
                    sql,
                    max_rows,
                    cancel_at_cap,
                    stop,
                    canceller,
                    decode_postgres
                )
            }
        };
        self.lost |= out.lost;
        out
    }

    async fn control(&mut self, sql: &str) -> std::result::Result<(), String> {
        let stop = AtomicBool::new(false);
        match self.execute(sql, 0, false, &stop).await.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Whether the session commits each statement on its own. Always true on PostgreSQL;
    /// on MySQL it is asked from the server and remembered until something may change it.
    async fn autocommit(&mut self) -> bool {
        if let Some(known) = self.autocommit {
            return known;
        }
        let Conn::MySql(conn) = &mut self.conn else {
            return true;
        };
        let answer = sqlx::query("SELECT CAST(@@autocommit AS CHAR)")
            .fetch_one(&mut *conn)
            .await
            .and_then(|row| row.try_get_unchecked::<Vec<u8>, _>(0));
        match answer {
            Ok(value) => {
                let on = value != b"0";
                self.autocommit = Some(on);
                on
            }
            // Could not ask: guard as usual; the guard's own BEGIN reports a dead connection.
            Err(_) => true,
        }
    }

    /// Column names of a query that returned no rows (the stream only carries them on rows).
    async fn describe(&mut self, sql: &str) -> Vec<ResultColumn> {
        macro_rules! columns {
            ($conn:expr) => {
                match (&mut *$conn).describe(sql).await {
                    Ok(d) => d
                        .columns()
                        .iter()
                        .map(|c| {
                            let type_name = c.type_info().name().to_string();
                            ResultColumn {
                                name: c.name().to_string(),
                                kind: value_kind(&type_name),
                                type_name,
                            }
                        })
                        .collect(),
                    Err(_) => Vec::new(),
                }
            };
        }
        match &mut self.conn {
            Conn::MySql(conn) => columns!(conn),
            Conn::Postgres(conn) => columns!(conn),
        }
    }

    /// Run a script statement by statement, stopping at the first one that fails, is
    /// blocked by the impact guard, or is cancelled through `stop`.
    pub async fn run(
        &mut self,
        sql: &str,
        options: &RunOptions,
        stop: &AtomicBool,
    ) -> Result<RunOutcome> {
        if options.max_rows == 0 || options.max_rows > MAX_RESULT_ROWS {
            return Err(Error::Invalid(format!(
                "Rows per result must be between 1 and {MAX_RESULT_ROWS}"
            )));
        }
        if self.lost {
            return Err(Error::Database(
                "The connection was lost. Run again to reconnect.".into(),
            ));
        }
        let engine = self.engine();
        let statements = script::split(engine, sql);
        if statements.is_empty() {
            return Err(Error::Invalid("There is no statement to run".into()));
        }
        let started = Instant::now();
        self.stored.clear();
        let mut outcomes = Vec::with_capacity(statements.len());
        let total = statements.len();
        let mut resume = None;

        for statement in statements {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let began = Instant::now();
            // The guard measures impact, so it needs a transaction of its own to undo; it
            // must never touch a transaction that belongs to the user.
            let theirs =
                self.in_transaction || (statement.kind == Kind::Dml && !self.autocommit().await);
            let guarded = statement.kind == Kind::Dml && !theirs;
            // Cancelling is how a result is cut short. Only a plain query may be cut: a
            // statement that writes would be undone, and inside a PostgreSQL transaction
            // the cancellation would abort everything the user has done in it.
            let cancel_at_cap = statement.kind == Kind::Read
                && !(engine == Engine::Postgres && self.in_transaction);
            let mut outcome = StatementOutcome {
                sql: statement.text.clone(),
                kind: statement.kind,
                status: Status::Ok,
                results: Vec::new(),
                rows_affected: None,
                message: None,
                elapsed_ms: 0,
            };

            if guarded {
                let begin = if engine == Engine::Mysql {
                    "START TRANSACTION"
                } else {
                    "BEGIN"
                };
                if let Err(e) = self.control(begin).await {
                    outcome.status = Status::Error;
                    outcome.message = Some(e);
                    outcomes.push(outcome);
                    break;
                }
            }

            let mut executed = self
                .execute(&statement.text, options.max_rows, cancel_at_cap, stop)
                .await;
            // SET, CALL and the like may have switched autocommit: ask again when needed.
            if statement.kind == Kind::Other && engine == Engine::Mysql {
                self.autocommit = None;
            }
            let affected = executed.affected.unwrap_or(0);
            let over_limit = guarded && options.change_limit.is_some_and(|limit| affected > limit);

            if let Some(error) = executed.error.take() {
                outcome.status = Status::Error;
                outcome.message = Some(if executed.lost {
                    format!("{error} — the connection was lost. Run again to reconnect.")
                } else {
                    error
                });
            } else if executed.cancelled {
                outcome.status = Status::Cancelled;
                outcome.message = Some("Cancelled".into());
            } else if over_limit {
                outcome.status = Status::Blocked;
                outcome.rows_affected = Some(affected);
            }

            if guarded && !self.lost {
                let finish = if outcome.status == Status::Ok {
                    "COMMIT"
                } else {
                    "ROLLBACK"
                };
                if let Err(e) = self.control(finish).await {
                    outcome.status = Status::Error;
                    outcome.message = Some(format!("{finish} failed: {e}"));
                }
            }

            if outcome.status == Status::Ok {
                if statement.kind == Kind::Transaction {
                    if let Some(open) = script::transaction_effect(engine, &statement.text) {
                        self.in_transaction = open;
                    }
                }
                // MySQL commits implicitly around DDL, except for temporary tables.
                if statement.kind == Kind::Ddl
                    && engine == Engine::Mysql
                    && !script::mentions(engine, &statement.text, "TEMPORARY")
                {
                    self.in_transaction = false;
                }
                if executed.sets.is_empty() && statement.kind == Kind::Read {
                    let columns = self.describe(&statement.text).await;
                    if !columns.is_empty() {
                        executed.sets.push((
                            ResultSet {
                                columns,
                                rows: Vec::new(),
                            },
                            false,
                        ));
                    }
                }
                if executed.sets.is_empty() || statement.kind != Kind::Read {
                    outcome.rows_affected = executed.affected;
                }
                for (set, capped) in executed.sets {
                    outcome
                        .results
                        .push(preview(self.stored.len(), &set, capped));
                    self.stored.push(set);
                }
            } else if self.in_transaction
                && !self.lost
                && engine == Engine::Postgres
                && outcome.status != Status::Blocked
            {
                // PostgreSQL aborts the user's transaction on any error; say so.
                let note = "The open transaction is now aborted: run ROLLBACK.";
                outcome.message = Some(match outcome.message.take() {
                    Some(m) => format!("{m} — {note}"),
                    None => note.to_string(),
                });
            }

            outcome.elapsed_ms = began.elapsed().as_millis() as u64;
            if outcome.status == Status::Blocked {
                resume = Some(sql[statement.start..].to_string());
            }
            let failed = outcome.status != Status::Ok;
            outcomes.push(outcome);
            if failed {
                break;
            }
        }

        if self.lost {
            // The server rolled back whatever was open when the connection went away.
            self.in_transaction = false;
        }
        let in_transaction = !self.lost && (self.in_transaction || !self.autocommit().await);
        Ok(RunOutcome {
            skipped: total - outcomes.len(),
            resume,
            statements: outcomes,
            in_transaction,
            connection_lost: self.lost,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

fn preview(id: usize, set: &ResultSet, capped: bool) -> ResultPreview {
    let mut truncated = Vec::new();
    let rows = set
        .rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            row.iter()
                .enumerate()
                .map(|(c, cell)| {
                    let text = cell.as_ref()?;
                    Some(match text.char_indices().nth(MAX_CELL_CHARS) {
                        Some((cut, _)) => {
                            truncated.push([r as u32, c as u32]);
                            text[..cut].to_string()
                        }
                        None => text.clone(),
                    })
                })
                .collect()
        })
        .collect();
    ResultPreview {
        id,
        columns: set.columns.clone(),
        rows,
        truncated,
        capped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_driver_type_names() {
        for (name, kind) in [
            ("BIGINT UNSIGNED", ValueKind::Number),
            ("NUMERIC", ValueKind::Number),
            ("FLOAT8", ValueKind::Number),
            ("VARCHAR", ValueKind::Text),
            ("TEXT", ValueKind::Text),
            ("TIMESTAMPTZ", ValueKind::Temporal),
            ("DATE", ValueKind::Temporal),
            ("JSONB", ValueKind::Json),
            ("BYTEA", ValueKind::Binary),
            ("VARBINARY", ValueKind::Binary),
            ("BOOL", ValueKind::Bool),
            ("INTERVAL", ValueKind::Temporal),
            ("POINT", ValueKind::Text),
            ("INT4", ValueKind::Number),
        ] {
            assert_eq!(value_kind(name), kind, "{name}");
        }
    }

    #[test]
    fn previews_cut_long_cells_but_keep_the_stored_value() {
        let long = "ก".repeat(MAX_CELL_CHARS + 5);
        let set = ResultSet {
            columns: vec![ResultColumn {
                name: "a".into(),
                type_name: "TEXT".into(),
                kind: ValueKind::Text,
            }],
            rows: vec![
                vec![Some(long.clone())],
                vec![None],
                vec![Some("short".into())],
            ],
        };
        let p = preview(3, &set, true);
        assert_eq!(p.id, 3);
        assert!(p.capped);
        assert_eq!(p.truncated, vec![[0, 0]]);
        assert_eq!(
            p.rows[0][0].as_ref().unwrap().chars().count(),
            MAX_CELL_CHARS
        );
        assert_eq!(p.rows[1][0], None);
        assert_eq!(set.rows[0][0].as_deref(), Some(long.as_str()));
        assert_eq!(hex(&[0, 255, 16]), "0x00ff10");
    }
}
