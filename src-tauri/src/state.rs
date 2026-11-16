use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use puricat_core::db::console::{Canceller, Console};
use puricat_core::db::Session;
use puricat_core::secrets::{OsKeychain, SecretStore};
use puricat_core::store::Store;
use puricat_core::{Error, Result};
use tokio::sync::RwLock;

/// A query tab's console. The canceller and stop flag live outside the console's lock so a
/// running statement can be interrupted.
pub struct ConsoleSlot {
    pub connection_id: String,
    pub database: Option<String>,
    pub console: tokio::sync::Mutex<Console>,
    pub canceller: Canceller,
    pub stop: std::sync::atomic::AtomicBool,
}

impl ConsoleSlot {
    /// The tab is going away: interrupt what it is running, if anything, so the statement
    /// does not carry on unseen on the server. Dropping the slot then closes its connection.
    pub async fn abandon(&self) {
        if self.console.try_lock().is_err() {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = self.canceller.cancel().await;
        }
    }
}

pub struct AppState {
    pub store: Store,
    pub secrets: Arc<dyn SecretStore>,
    /// Open sessions by connection id.
    sessions: RwLock<HashMap<String, Arc<Session>>>,
    /// One SQL console per query tab, by tab id.
    pub consoles: tokio::sync::Mutex<HashMap<String, Arc<ConsoleSlot>>>,
    /// Cancel flags of running long jobs, by job id.
    pub jobs: std::sync::Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
    /// An empty directory the AI CLIs are started in, so they have no files to look at.
    pub agent_scratch: std::path::PathBuf,
}

impl AppState {
    pub async fn open(data_dir: &Path) -> Result<Self> {
        Ok(AppState {
            store: Store::open(&data_dir.join("puricat.db")).await?,
            secrets: secret_store(),
            sessions: RwLock::new(HashMap::new()),
            consoles: Default::default(),
            jobs: Default::default(),
            agent_scratch: data_dir.join("agent-scratch"),
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
        // Query tabs of this connection lose their consoles (and any open transaction).
        let dropped: Vec<_> = {
            let mut consoles = self.consoles.lock().await;
            let tabs: Vec<String> = consoles
                .iter()
                .filter(|(_, slot)| slot.connection_id == connection_id)
                .map(|(tab, _)| tab.clone())
                .collect();
            tabs.iter().filter_map(|tab| consoles.remove(tab)).collect()
        };
        // Cancelling goes through the session's pool, so it happens before the pool closes.
        for slot in dropped {
            slot.abandon().await;
        }
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
