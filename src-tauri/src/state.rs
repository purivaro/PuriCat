use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use puricat_core::db::Session;
use puricat_core::secrets::{OsKeychain, SecretStore};
use puricat_core::store::Store;
use puricat_core::{Error, Result};
use tokio::sync::RwLock;

pub struct AppState {
    pub store: Store,
    pub secrets: Arc<dyn SecretStore>,
    /// Open sessions by connection id.
    sessions: RwLock<HashMap<String, Arc<Session>>>,
}

impl AppState {
    pub async fn open(data_dir: &Path) -> Result<Self> {
        Ok(AppState {
            store: Store::open(&data_dir.join("puricat.db")).await?,
            secrets: secret_store(),
            sessions: RwLock::new(HashMap::new()),
        })
    }

    pub async fn session(&self, connection_id: &str) -> Result<Arc<Session>> {
        self.sessions
            .read()
            .await
            .get(connection_id)
            .cloned()
            .ok_or_else(|| Error::Invalid("This connection is not open".into()))
    }

    /// Register a session, closing any previous one for the same connection.
    pub async fn insert_session(&self, connection_id: &str, session: Session) {
        let previous = self
            .sessions
            .write()
            .await
            .insert(connection_id.to_string(), Arc::new(session));
        if let Some(old) = previous {
            old.close().await;
        }
    }

    /// Close and forget a session. Returns whether one was open.
    pub async fn remove_session(&self, connection_id: &str) -> bool {
        let removed = self.sessions.write().await.remove(connection_id);
        match removed {
            Some(old) => {
                old.close().await;
                true
            }
            None => false,
        }
    }

    /// Blocking keychain calls run off the async executor.
    pub async fn with_secrets<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&dyn SecretStore) -> Result<T> + Send + 'static,
    {
        let secrets = self.secrets.clone();
        tokio::task::spawn_blocking(move || f(secrets.as_ref()))
            .await
            .map_err(|e| Error::Secret(e.to_string()))?
    }
}

fn secret_store() -> Arc<dyn SecretStore> {
    // Development escape hatch: keep secrets in memory so unsigned debug builds do not
    // trigger a Keychain prompt on every rebuild. Compiled out of release builds.
    #[cfg(debug_assertions)]
    if std::env::var("PURICAT_SECRET_STORE").as_deref() == Ok("memory") {
        return Arc::new(puricat_core::secrets::MemoryStore::default());
    }
    Arc::new(OsKeychain)
}
