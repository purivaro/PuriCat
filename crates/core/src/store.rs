//! Local settings database (SQLite): connection profiles and key/value settings.

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;

use crate::model::ConnectionProfile;
use crate::{Error, Result};

/// Ordered schema migrations. Append only; never edit a shipped entry.
const MIGRATIONS: &[&str] = &[
    // 1 — connection profiles (JSON document per row) and key/value settings.
    "CREATE TABLE connections (
        id          TEXT PRIMARY KEY NOT NULL,
        profile     TEXT NOT NULL,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        last_used_at INTEGER
     );
     CREATE TABLE settings (
        key   TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
     );",
    // 2 — SQL history. Statements are stored after password literals were masked.
    "CREATE TABLE query_history (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        connection_id TEXT NOT NULL,
        database_name TEXT,
        sql           TEXT NOT NULL,
        status        TEXT NOT NULL,
        summary       TEXT NOT NULL,
        elapsed_ms    INTEGER NOT NULL,
        started_at    INTEGER NOT NULL
     );
     CREATE INDEX query_history_recent ON query_history (connection_id, id DESC);",
    // 3 — what the AI did on a database and what the user decided. It keeps the
    //     connection's name, so an entry still says where it happened after the connection
    //     is deleted.
    "CREATE TABLE agent_audit (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        at              INTEGER NOT NULL,
        connection_id   TEXT NOT NULL,
        connection_name TEXT NOT NULL,
        database_name   TEXT NOT NULL,
        provider        TEXT NOT NULL,
        model           TEXT,
        event           TEXT NOT NULL,
        sql             TEXT NOT NULL,
        detail          TEXT NOT NULL
     );",
    // 4 — scripts the user saved for a connection, kept exactly as written.
    "CREATE TABLE saved_scripts (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        connection_id TEXT NOT NULL,
        name          TEXT NOT NULL,
        sql           TEXT NOT NULL,
        created_at    INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL
     );
     CREATE UNIQUE INDEX saved_scripts_name ON saved_scripts (connection_id, name COLLATE NOCASE);",
    // 5 — backups: what to back up where and when (one job per database), and every run.
    "CREATE TABLE backup_jobs (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        connection_id TEXT NOT NULL,
        database_name TEXT NOT NULL,
        destination   TEXT NOT NULL,
        format        TEXT NOT NULL,
        compress      INTEGER NOT NULL,
        schedule      TEXT NOT NULL,
        keep          INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL,
        UNIQUE (connection_id, database_name)
     );
     CREATE TABLE backup_runs (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        started_at      INTEGER NOT NULL,
        finished_at     INTEGER NOT NULL,
        connection_id   TEXT NOT NULL,
        connection_name TEXT NOT NULL,
        database_name   TEXT NOT NULL,
        scheduled       INTEGER NOT NULL,
        status          TEXT NOT NULL,
        file            TEXT,
        bytes           INTEGER,
        tool            TEXT NOT NULL,
        message         TEXT NOT NULL,
        removed_at      INTEGER
     );
     CREATE INDEX backup_runs_database ON backup_runs (connection_id, database_name, id);",
];

/// Entries kept; older ones are dropped as new ones arrive.
const HISTORY_LIMIT: i64 = 1000;

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub connection_id: String,
    pub database: Option<String>,
    pub sql: String,
    pub status: String,
    pub summary: String,
    pub elapsed_ms: i64,
    pub started_at: i64,
}

/// One line of the AI activity log. `event` is one of `read` (rows were read), `read_failed`
/// (a reading query was refused or failed), `proposed`,
/// `approved`, `rejected`, `executed`, `blocked` (rolled back by the impact guard) and
/// `failed`. SQL is stored with password literals masked.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub at: i64,
    pub connection_id: String,
    pub connection_name: String,
    pub database: String,
    pub provider: String,
    pub model: Option<String>,
    pub event: String,
    pub sql: String,
    pub detail: String,
}

/// Audit entries kept; older ones are dropped as new ones arrive.
const AUDIT_LIMIT: i64 = 5000;

/// What to back up, where to and when: one per database of a connection. It is also where
/// the choices of the last backup made by hand are remembered.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupJob {
    pub id: i64,
    pub connection_id: String,
    pub database: String,
    /// The folder the files go to.
    pub destination: String,
    pub format: crate::backup::Format,
    pub compress: bool,
    pub schedule: crate::backup_schedule::Schedule,
    /// How many of the scheduled backups to keep; 0 keeps all.
    pub keep: u32,
    pub updated_at: i64,
}

/// One backup that was run: by hand or by the schedule, kept or failed.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupRun {
    pub id: i64,
    pub started_at: i64,
    pub finished_at: i64,
    pub connection_id: String,
    pub connection_name: String,
    pub database: String,
    pub scheduled: bool,
    /// `ok`, `failed` or `stopped`.
    pub status: String,
    /// The file that was kept, for an `ok` run.
    pub file: Option<String>,
    pub bytes: Option<i64>,
    /// The dump tool and its version.
    pub tool: String,
    /// Why it failed; empty for an `ok` run.
    pub message: String,
    /// When the retention rule deleted the file.
    pub removed_at: Option<i64>,
}

/// Runs kept in the history; older ones are dropped as new ones arrive. Their files are not.
const BACKUP_RUN_LIMIT: i64 = 2000;

/// A script saved for one connection. Unlike the history it is the user's own text, stored
/// exactly as written: nothing is masked.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedScript {
    pub id: i64,
    pub connection_id: String,
    pub name: String,
    pub sql: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Longest name of a saved script, in characters.
pub const SCRIPT_NAME_MAX: usize = 120;
/// Largest saved script, in bytes of UTF-8.
pub const SCRIPT_MAX_BYTES: usize = 1024 * 1024;

const SCRIPT_COLUMNS: &str = "id, connection_id, name, sql, created_at, updated_at";

fn script_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("Give the script a name.".into()));
    }
    if name.chars().count() > SCRIPT_NAME_MAX {
        return Err(Error::Invalid(format!(
            "A script name can be {SCRIPT_NAME_MAX} characters at most."
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "A script name must be a single line of text.".into(),
        ));
    }
    Ok(name)
}

fn script_text(sql: &str) -> Result<()> {
    if sql.len() > SCRIPT_MAX_BYTES {
        return Err(Error::Invalid(
            "This script is larger than 1 MB, the most a saved script can hold.".into(),
        ));
    }
    Ok(())
}

/// A name that is already taken is a conflict the user can resolve; anything else is the
/// store failing.
fn script_error(name: &str) -> impl FnOnce(sqlx::Error) -> Error + '_ {
    move |e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => Error::Conflict(format!(
            "This connection already has a script named “{name}”."
        )),
        _ => Error::store(e),
    }
}

fn row_to_script(r: &sqlx::sqlite::SqliteRow) -> Result<SavedScript> {
    Ok(SavedScript {
        id: r.try_get(0).map_err(Error::store)?,
        connection_id: r.try_get(1).map_err(Error::store)?,
        name: r.try_get(2).map_err(Error::store)?,
        sql: r.try_get(3).map_err(Error::store)?,
        created_at: r.try_get(4).map_err(Error::store)?,
        updated_at: r.try_get(5).map_err(Error::store)?,
    })
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .map_err(Error::store)?;
        restrict_permissions(path);
        Self::init(pool).await
    }

    pub async fn open_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:").map_err(Error::store)?;
        // A single connection, otherwise every pooled connection gets its own empty database.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .map_err(Error::store)?;
        Self::init(pool).await
    }

    async fn init(pool: SqlitePool) -> Result<Self> {
        let store = Store { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<()> {
        let current: i64 = sqlx::query("PRAGMA user_version")
            .fetch_one(&self.pool)
            .await
            .map_err(Error::store)?
            .try_get(0)
            .map_err(Error::store)?;
        if current as usize > MIGRATIONS.len() {
            return Err(Error::Store(format!(
                "settings database is from a newer PuriCat (schema {current}, this build knows {})",
                MIGRATIONS.len()
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            let mut tx = self.pool.begin().await.map_err(Error::store)?;
            sqlx::raw_sql(sql)
                .execute(&mut *tx)
                .await
                .map_err(Error::store)?;
            sqlx::raw_sql(&format!("PRAGMA user_version = {}", i + 1))
                .execute(&mut *tx)
                .await
                .map_err(Error::store)?;
            tx.commit().await.map_err(Error::store)?;
        }
        Ok(())
    }

    // ---- connections -------------------------------------------------------------------

    pub async fn list_connections(&self) -> Result<Vec<ConnectionProfile>> {
        let rows = sqlx::query(
            "SELECT profile, created_at, updated_at, last_used_at FROM connections
             ORDER BY json_extract(profile, '$.name') COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::store)?;
        rows.iter().map(row_to_profile).collect()
    }

    pub async fn get_connection(&self, id: &str) -> Result<ConnectionProfile> {
        let row = sqlx::query(
            "SELECT profile, created_at, updated_at, last_used_at FROM connections WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::store)?
        .ok_or_else(|| Error::NotFound("Connection".into()))?;
        row_to_profile(&row)
    }

    /// Insert (empty id) or update a profile. Returns the stored profile.
    pub async fn save_connection(&self, profile: ConnectionProfile) -> Result<ConnectionProfile> {
        let mut p = profile.normalized()?;
        let now = now_ms();
        if p.id.is_empty() {
            p.id = uuid::Uuid::new_v4().to_string();
            p.created_at = now;
            p.last_used_at = None;
        } else {
            let existing = self.get_connection(&p.id).await?;
            p.created_at = existing.created_at;
            p.last_used_at = existing.last_used_at;
        }
        p.updated_at = now;
        let json = serde_json::to_string(&p).map_err(|e| Error::Store(e.to_string()))?;
        sqlx::query(
            "INSERT INTO connections (id, profile, created_at, updated_at, last_used_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET profile = excluded.profile, updated_at = excluded.updated_at",
        )
        .bind(&p.id)
        .bind(json)
        .bind(p.created_at)
        .bind(p.updated_at)
        .bind(p.last_used_at)
        .execute(&self.pool)
        .await
        .map_err(Error::store)?;
        Ok(p)
    }

    pub async fn delete_connection(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM connections WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    pub async fn mark_connection_used(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE connections SET last_used_at = ? WHERE id = ?")
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    // ---- query history ------------------------------------------------------------------

    pub async fn add_history(
        &self,
        connection_id: &str,
        database: Option<&str>,
        sql: &str,
        status: &str,
        summary: &str,
        elapsed_ms: u64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO query_history (connection_id, database_name, sql, status, summary, elapsed_ms, started_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(connection_id)
        .bind(database)
        .bind(sql)
        .bind(status)
        .bind(summary)
        .bind(elapsed_ms as i64)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(Error::store)?;
        sqlx::query(
            "DELETE FROM query_history WHERE id <= (SELECT MAX(id) FROM query_history) - ?",
        )
        .bind(HISTORY_LIMIT)
        .execute(&self.pool)
        .await
        .map_err(Error::store)?;
        Ok(())
    }

    /// Most recent first, for one connection.
    pub async fn list_history(&self, connection_id: &str, limit: u32) -> Result<Vec<HistoryEntry>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, database_name, sql, status, summary, elapsed_ms, started_at
             FROM query_history WHERE connection_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(connection_id)
        .bind(limit.min(500))
        .fetch_all(&self.pool)
        .await
        .map_err(Error::store)?;
        rows.iter()
            .map(|r| {
                Ok(HistoryEntry {
                    id: r.try_get(0).map_err(Error::store)?,
                    connection_id: r.try_get(1).map_err(Error::store)?,
                    database: r.try_get(2).map_err(Error::store)?,
                    sql: r.try_get(3).map_err(Error::store)?,
                    status: r.try_get(4).map_err(Error::store)?,
                    summary: r.try_get(5).map_err(Error::store)?,
                    elapsed_ms: r.try_get(6).map_err(Error::store)?,
                    started_at: r.try_get(7).map_err(Error::store)?,
                })
            })
            .collect()
    }

    pub async fn clear_history(&self, connection_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM query_history WHERE connection_id = ?")
            .bind(connection_id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    // ---- saved scripts -------------------------------------------------------------------

    /// The scripts of one connection, by name.
    pub async fn list_scripts(&self, connection_id: &str) -> Result<Vec<SavedScript>> {
        let rows = sqlx::query(&format!(
            "SELECT {SCRIPT_COLUMNS} FROM saved_scripts WHERE connection_id = ?
             ORDER BY name COLLATE NOCASE, id"
        ))
        .bind(connection_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Error::store)?;
        rows.iter().map(row_to_script).collect()
    }

    pub async fn get_script(&self, id: i64) -> Result<SavedScript> {
        let row = sqlx::query(&format!(
            "SELECT {SCRIPT_COLUMNS} FROM saved_scripts WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::store)?
        .ok_or_else(|| Error::NotFound("Saved script".into()))?;
        row_to_script(&row)
    }

    /// Save a new script. A name is used once per connection, whatever its letter case.
    pub async fn create_script(
        &self,
        connection_id: &str,
        name: &str,
        sql: &str,
    ) -> Result<SavedScript> {
        let name = script_name(name)?;
        script_text(sql)?;
        let now = now_ms();
        let done = sqlx::query(
            "INSERT INTO saved_scripts (connection_id, name, sql, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(connection_id)
        .bind(name)
        .bind(sql)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(script_error(name))?;
        self.get_script(done.last_insert_rowid()).await
    }

    /// Replace the text of a saved script.
    pub async fn update_script(&self, id: i64, sql: &str) -> Result<SavedScript> {
        script_text(sql)?;
        sqlx::query("UPDATE saved_scripts SET sql = ?, updated_at = ? WHERE id = ?")
            .bind(sql)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        // Not found when the script was deleted meanwhile: nothing was written.
        self.get_script(id).await
    }

    pub async fn rename_script(&self, id: i64, name: &str) -> Result<SavedScript> {
        let name = script_name(name)?;
        sqlx::query("UPDATE saved_scripts SET name = ?, updated_at = ? WHERE id = ?")
            .bind(name)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(script_error(name))?;
        self.get_script(id).await
    }

    pub async fn delete_script(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM saved_scripts WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    /// Every script of a connection, when the connection itself is deleted.
    pub async fn clear_scripts(&self, connection_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM saved_scripts WHERE connection_id = ?")
            .bind(connection_id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    // ---- AI activity log ------------------------------------------------------------------

    /// Append an entry. `id` and `at` are assigned here.
    pub async fn add_audit(&self, entry: &AuditEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_audit
               (at, connection_id, connection_name, database_name, provider, model, event, sql, detail)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(now_ms())
        .bind(&entry.connection_id)
        .bind(&entry.connection_name)
        .bind(&entry.database)
        .bind(&entry.provider)
        .bind(&entry.model)
        .bind(&entry.event)
        .bind(&entry.sql)
        .bind(&entry.detail)
        .execute(&self.pool)
        .await
        .map_err(Error::store)?;
        sqlx::query("DELETE FROM agent_audit WHERE id <= (SELECT MAX(id) FROM agent_audit) - ?")
            .bind(AUDIT_LIMIT)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    /// Most recent first.
    pub async fn list_audit(&self, limit: u32) -> Result<Vec<AuditEntry>> {
        let rows = sqlx::query(
            "SELECT id, at, connection_id, connection_name, database_name, provider, model, event, sql, detail
             FROM agent_audit ORDER BY id DESC LIMIT ?",
        )
        .bind(limit.min(1000))
        .fetch_all(&self.pool)
        .await
        .map_err(Error::store)?;
        rows.iter()
            .map(|r| {
                Ok(AuditEntry {
                    id: r.try_get(0).map_err(Error::store)?,
                    at: r.try_get(1).map_err(Error::store)?,
                    connection_id: r.try_get(2).map_err(Error::store)?,
                    connection_name: r.try_get(3).map_err(Error::store)?,
                    database: r.try_get(4).map_err(Error::store)?,
                    provider: r.try_get(5).map_err(Error::store)?,
                    model: r.try_get(6).map_err(Error::store)?,
                    event: r.try_get(7).map_err(Error::store)?,
                    sql: r.try_get(8).map_err(Error::store)?,
                    detail: r.try_get(9).map_err(Error::store)?,
                })
            })
            .collect()
    }

    pub async fn clear_audit(&self) -> Result<()> {
        sqlx::query("DELETE FROM agent_audit")
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    // ---- backups -----------------------------------------------------------------------

    fn backup_job(r: &sqlx::sqlite::SqliteRow) -> Result<BackupJob> {
        let format: String = r.try_get(4).map_err(Error::store)?;
        let schedule: String = r.try_get(6).map_err(Error::store)?;
        Ok(BackupJob {
            id: r.try_get(0).map_err(Error::store)?,
            connection_id: r.try_get(1).map_err(Error::store)?,
            database: r.try_get(2).map_err(Error::store)?,
            destination: r.try_get(3).map_err(Error::store)?,
            format: serde_json::from_value(serde_json::Value::String(format))
                .map_err(|e| Error::Store(format!("backup job: {e}")))?,
            compress: r.try_get::<i64, _>(5).map_err(Error::store)? != 0,
            schedule: serde_json::from_str(&schedule)
                .map_err(|e| Error::Store(format!("backup job: {e}")))?,
            keep: r.try_get::<i64, _>(7).map_err(Error::store)?.max(0) as u32,
            updated_at: r.try_get(8).map_err(Error::store)?,
        })
    }

    const BACKUP_JOB_COLUMNS: &'static str =
        "id, connection_id, database_name, destination, format, compress, schedule, keep, updated_at";

    /// Every job, in a stable order.
    pub async fn list_backup_jobs(&self) -> Result<Vec<BackupJob>> {
        let rows = sqlx::query(&format!(
            "SELECT {} FROM backup_jobs ORDER BY id",
            Self::BACKUP_JOB_COLUMNS
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(Error::store)?;
        rows.iter().map(Self::backup_job).collect()
    }

    pub async fn backup_job_for(
        &self,
        connection_id: &str,
        database: &str,
    ) -> Result<Option<BackupJob>> {
        let row = sqlx::query(&format!(
            "SELECT {} FROM backup_jobs WHERE connection_id = ? AND database_name = ?",
            Self::BACKUP_JOB_COLUMNS
        ))
        .bind(connection_id)
        .bind(database)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::store)?;
        row.as_ref().map(Self::backup_job).transpose()
    }

    /// Create the job of a database or replace its settings. `id` and `updated_at` are
    /// assigned here.
    pub async fn save_backup_job(&self, job: &BackupJob) -> Result<BackupJob> {
        let format = serde_json::to_value(job.format)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let schedule =
            serde_json::to_string(&job.schedule).map_err(|e| Error::Store(e.to_string()))?;
        sqlx::query(
            "INSERT INTO backup_jobs (connection_id, database_name, destination, format, compress, schedule, keep, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (connection_id, database_name) DO UPDATE SET
               destination = excluded.destination, format = excluded.format, compress = excluded.compress,
               schedule = excluded.schedule, keep = excluded.keep, updated_at = excluded.updated_at",
        )
        .bind(&job.connection_id)
        .bind(&job.database)
        .bind(&job.destination)
        .bind(format)
        .bind(job.compress as i64)
        .bind(schedule)
        .bind(job.keep as i64)
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(Error::store)?;
        self.backup_job_for(&job.connection_id, &job.database)
            .await?
            .ok_or_else(|| Error::NotFound("backup job".into()))
    }

    /// A connection was deleted: its jobs go with it. The runs stay in the history (they say
    /// where files are), under the name the connection had.
    pub async fn clear_backup_jobs(&self, connection_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM backup_jobs WHERE connection_id = ?")
            .bind(connection_id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    /// Record a run. `id` is assigned here.
    pub async fn add_backup_run(&self, run: &BackupRun) -> Result<i64> {
        let id = sqlx::query(
            "INSERT INTO backup_runs
               (started_at, finished_at, connection_id, connection_name, database_name, scheduled, status, file, bytes, tool, message)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(run.started_at)
        .bind(run.finished_at)
        .bind(&run.connection_id)
        .bind(&run.connection_name)
        .bind(&run.database)
        .bind(run.scheduled as i64)
        .bind(&run.status)
        .bind(&run.file)
        .bind(run.bytes)
        .bind(&run.tool)
        .bind(&run.message)
        .execute(&self.pool)
        .await
        .map_err(Error::store)?
        .last_insert_rowid();
        sqlx::query("DELETE FROM backup_runs WHERE id <= (SELECT MAX(id) FROM backup_runs) - ?")
            .bind(BACKUP_RUN_LIMIT)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(id)
    }

    fn backup_run(r: &sqlx::sqlite::SqliteRow) -> Result<BackupRun> {
        Ok(BackupRun {
            id: r.try_get(0).map_err(Error::store)?,
            started_at: r.try_get(1).map_err(Error::store)?,
            finished_at: r.try_get(2).map_err(Error::store)?,
            connection_id: r.try_get(3).map_err(Error::store)?,
            connection_name: r.try_get(4).map_err(Error::store)?,
            database: r.try_get(5).map_err(Error::store)?,
            scheduled: r.try_get::<i64, _>(6).map_err(Error::store)? != 0,
            status: r.try_get(7).map_err(Error::store)?,
            file: r.try_get(8).map_err(Error::store)?,
            bytes: r.try_get(9).map_err(Error::store)?,
            tool: r.try_get(10).map_err(Error::store)?,
            message: r.try_get(11).map_err(Error::store)?,
            removed_at: r.try_get(12).map_err(Error::store)?,
        })
    }

    const BACKUP_RUN_COLUMNS: &'static str = "id, started_at, finished_at, connection_id, connection_name, database_name, scheduled, status, file, bytes, tool, message, removed_at";

    /// Most recent first. With a database: its runs only; otherwise every run.
    pub async fn list_backup_runs(
        &self,
        of: Option<(&str, &str)>,
        limit: u32,
    ) -> Result<Vec<BackupRun>> {
        let filter = if of.is_some() {
            "WHERE connection_id = ? AND database_name = ?"
        } else {
            ""
        };
        let sql = format!(
            "SELECT {} FROM backup_runs {filter} ORDER BY id DESC LIMIT ?",
            Self::BACKUP_RUN_COLUMNS
        );
        let mut query = sqlx::query(&sql);
        if let Some((connection_id, database)) = of {
            query = query.bind(connection_id).bind(database);
        }
        let rows = query
            .bind(limit.min(1000))
            .fetch_all(&self.pool)
            .await
            .map_err(Error::store)?;
        rows.iter().map(Self::backup_run).collect()
    }

    pub async fn get_backup_run(&self, id: i64) -> Result<BackupRun> {
        let row = sqlx::query(&format!(
            "SELECT {} FROM backup_runs WHERE id = ?",
            Self::BACKUP_RUN_COLUMNS
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::store)?;
        row.as_ref()
            .map(Self::backup_run)
            .transpose()?
            .ok_or_else(|| Error::NotFound("backup".into()))
    }

    /// The retention rule deleted this run's file.
    pub async fn mark_backup_removed(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE backup_runs SET removed_at = ? WHERE id = ?")
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    // ---- settings ----------------------------------------------------------------------

    pub async fn settings(&self) -> Result<HashMap<String, String>> {
        let rows = sqlx::query("SELECT key, value FROM settings")
            .fetch_all(&self.pool)
            .await
            .map_err(Error::store)?;
        let mut out = HashMap::with_capacity(rows.len());
        for r in rows {
            out.insert(
                r.try_get::<String, _>(0).map_err(Error::store)?,
                r.try_get::<String, _>(1).map_err(Error::store)?,
            );
        }
        Ok(out)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(Error::store)?;
        Ok(())
    }
}

fn row_to_profile(row: &sqlx::sqlite::SqliteRow) -> Result<ConnectionProfile> {
    let json: String = row.try_get(0).map_err(Error::store)?;
    let mut p: ConnectionProfile =
        serde_json::from_str(&json).map_err(|e| Error::Store(e.to_string()))?;
    // The columns are authoritative for timestamps.
    p.created_at = row.try_get(1).map_err(Error::store)?;
    p.updated_at = row.try_get(2).map_err(Error::store)?;
    p.last_used_at = row.try_get(3).map_err(Error::store)?;
    Ok(p)
}

/// The store holds host names and user names: keep it private to the OS user.
fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Engine, Environment};

    fn profile(name: &str) -> ConnectionProfile {
        serde_json::from_value(serde_json::json!({
            "name": name, "engine": "mysql", "host": "127.0.0.1", "port": 3306, "username": "root"
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn connection_crud() {
        let store = Store::open_in_memory().await.unwrap();
        assert!(store.list_connections().await.unwrap().is_empty());

        let a = store.save_connection(profile("beta")).await.unwrap();
        let b = store.save_connection(profile("Alpha")).await.unwrap();
        assert_eq!(a.id.len(), 36);
        assert_ne!(a.id, b.id);
        assert!(a.created_at > 0);

        // sorted by name, case-insensitively
        let names: Vec<_> = store
            .list_connections()
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["Alpha", "beta"]);

        // update keeps id + created_at, changes fields
        let mut edit = a.clone();
        edit.environment = Environment::Production;
        edit.engine = Engine::Postgres;
        edit.created_at = 1; // ignored: callers cannot rewrite history
        let saved = store.save_connection(edit).await.unwrap();
        assert_eq!(saved.id, a.id);
        assert_eq!(saved.created_at, a.created_at);
        let loaded = store.get_connection(&a.id).await.unwrap();
        assert_eq!(loaded.environment, Environment::Production);
        assert_eq!(loaded.engine, Engine::Postgres);

        store.mark_connection_used(&a.id).await.unwrap();
        assert!(store
            .get_connection(&a.id)
            .await
            .unwrap()
            .last_used_at
            .is_some());

        store.delete_connection(&a.id).await.unwrap();
        assert!(matches!(
            store.get_connection(&a.id).await,
            Err(Error::NotFound(_))
        ));
        assert_eq!(store.list_connections().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn updating_a_missing_connection_fails() {
        let store = Store::open_in_memory().await.unwrap();
        let mut p = profile("ghost");
        p.id = "does-not-exist".into();
        assert!(matches!(
            store.save_connection(p).await,
            Err(Error::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn audit_log_keeps_what_happened_newest_first() {
        let store = Store::open_in_memory().await.unwrap();
        let entry = |event: &str, sql: &str| AuditEntry {
            id: 0,
            at: 0,
            connection_id: "c1".into(),
            connection_name: "Shop (prod)".into(),
            database: "shop".into(),
            provider: "claude".into(),
            model: Some("haiku".into()),
            event: event.into(),
            sql: sql.into(),
            detail: "3 rows".into(),
        };
        store.add_audit(&entry("read", "SELECT 1")).await.unwrap();
        store
            .add_audit(&entry("proposed", "UPDATE t SET a = 1"))
            .await
            .unwrap();
        let log = store.list_audit(10).await.unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].event, "proposed");
        assert_eq!(log[1].sql, "SELECT 1");
        assert!(log[0].id > log[1].id && log[0].at > 0);
        assert_eq!(log[0].connection_name, "Shop (prod)");
        assert_eq!(log[0].model.as_deref(), Some("haiku"));
        // Deleting the connection's history does not touch the audit log.
        store.clear_history("c1").await.unwrap();
        assert_eq!(store.list_audit(1).await.unwrap().len(), 1);
        store.clear_audit().await.unwrap();
        assert!(store.list_audit(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn history_is_per_connection_newest_first_and_bounded() {
        let store = Store::open_in_memory().await.unwrap();
        for i in 0..(HISTORY_LIMIT + 5) {
            let connection = if i % 2 == 0 { "a" } else { "b" };
            store
                .add_history(
                    connection,
                    Some("db"),
                    &format!("SELECT {i}"),
                    "ok",
                    "1 row",
                    3,
                )
                .await
                .unwrap();
        }
        let a = store.list_history("a", 3).await.unwrap();
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].sql, format!("SELECT {}", HISTORY_LIMIT + 4));
        assert!(a[0].id > a[1].id && a[0].started_at > 0);
        assert_eq!(a[0].database.as_deref(), Some("db"));
        // Nothing older than the limit survives, across all connections.
        let all = store.list_history("a", 500).await.unwrap().len()
            + store.list_history("b", 500).await.unwrap().len();
        assert_eq!(all, 1000);
        store.clear_history("a").await.unwrap();
        assert!(store.list_history("a", 10).await.unwrap().is_empty());
        assert!(!store.list_history("b", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn saved_scripts_belong_to_a_connection_and_keep_their_text() {
        let store = Store::open_in_memory().await.unwrap();
        let text = "-- รายงานประจำวัน\nSELECT 'it''s' AS a;\nSET PASSWORD = 'secret'";
        let report = store.create_script("a", "  Report  ", text).await.unwrap();
        assert_eq!(report.name, "Report");
        // Stored as written: nothing is masked, unlike the history.
        assert_eq!(report.sql, text);
        assert!(report.id > 0 && report.created_at > 0);
        store.create_script("a", "audit", "SELECT 2").await.unwrap();
        store
            .create_script("b", "Report", "SELECT 3")
            .await
            .unwrap();

        // By name, without regard to case; another connection's scripts stay out.
        let names: Vec<_> = store
            .list_scripts("a")
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["audit", "Report"]);

        // One name per connection, whatever its case.
        assert!(matches!(
            store.create_script("a", "REPORT", "SELECT 4").await,
            Err(Error::Conflict(_))
        ));
        let audit = store.list_scripts("a").await.unwrap().remove(0);
        assert!(matches!(
            store.rename_script(audit.id, "report").await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(store.get_script(audit.id).await.unwrap().name, "audit");
        // A script may change the case of its own name.
        assert_eq!(
            store.rename_script(report.id, "REPORT").await.unwrap().name,
            "REPORT"
        );

        let saved = store.update_script(report.id, "SELECT 5").await.unwrap();
        assert_eq!(
            (saved.sql.as_str(), saved.name.as_str()),
            ("SELECT 5", "REPORT")
        );
        assert_eq!(saved.created_at, report.created_at);

        store.delete_script(report.id).await.unwrap();
        assert!(matches!(
            store.update_script(report.id, "SELECT 6").await,
            Err(Error::NotFound(_))
        ));
        store.clear_scripts("a").await.unwrap();
        assert!(store.list_scripts("a").await.unwrap().is_empty());
        assert_eq!(store.list_scripts("b").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn saved_scripts_refuse_bad_names_and_oversized_text() {
        let store = Store::open_in_memory().await.unwrap();
        for name in ["", "   ", "two\nlines", &"x".repeat(SCRIPT_NAME_MAX + 1)] {
            assert!(
                matches!(
                    store.create_script("a", name, "SELECT 1").await,
                    Err(Error::Invalid(_))
                ),
                "{name:?}"
            );
        }
        // The limit counts characters, not bytes.
        let thai = "ก".repeat(SCRIPT_NAME_MAX);
        let kept = store.create_script("a", &thai, "").await.unwrap();
        assert_eq!(kept.name, thai);
        let big = " ".repeat(SCRIPT_MAX_BYTES + 1);
        assert!(matches!(
            store.create_script("a", "big", &big).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            store.update_script(kept.id, &big).await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(store.list_scripts("a").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn settings_round_trip() {
        let store = Store::open_in_memory().await.unwrap();
        store.set_setting("theme", "dark").await.unwrap();
        store.set_setting("theme", "light").await.unwrap();
        store.set_setting("language", "th").await.unwrap();
        let all = store.settings().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all["theme"], "light");
    }

    #[tokio::test]
    async fn migrations_are_idempotent_on_reopen() {
        let dir = std::env::temp_dir().join(format!("puricat-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("puricat.db");
        {
            let s = Store::open(&path).await.unwrap();
            s.save_connection(profile("keep me")).await.unwrap();
        }
        let s = Store::open(&path).await.unwrap();
        assert_eq!(s.list_connections().await.unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
