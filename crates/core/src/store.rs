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
