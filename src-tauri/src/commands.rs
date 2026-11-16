//! The IPC surface. Every command is a thin wrapper over `puricat-core`.
//!
//! Passwords cross this boundary in one direction only (UI -> backend). Nothing here
//! returns a stored secret; the UI learns only whether one exists.

use std::collections::HashMap;

use puricat_core::ai_detect::{self, CliStatus};
use puricat_core::db::{
    DbObject, Namespace, PageRequest, RowPage, ServerInfo, Session, TableMeta, TableRef,
};
use puricat_core::model::ConnectionProfile;
use puricat_core::secrets::password_key;
use puricat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    #[serde(flatten)]
    profile: ConnectionProfile,
    has_password: bool,
}

/// What to do with the stored password when saving or testing a profile.
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum PasswordChange {
    /// Leave the stored password as it is.
    Keep,
    Set {
        value: String,
    },
    Clear,
}

async fn view(state: &AppState, profile: ConnectionProfile) -> Result<ConnectionView> {
    let key = password_key(&profile.id);
    let has_password = state
        .with_secrets(move |s| Ok(s.get(&key)?.is_some()))
        .await?;
    Ok(ConnectionView {
        profile,
        has_password,
    })
}

async fn stored_password(state: &AppState, connection_id: &str) -> Result<Option<String>> {
    if connection_id.is_empty() {
        return Ok(None);
    }
    let key = password_key(connection_id);
    state.with_secrets(move |s| s.get(&key)).await
}

#[tauri::command]
pub async fn list_connections(state: State<'_, AppState>) -> Result<Vec<ConnectionView>> {
    let mut out = Vec::new();
    for profile in state.store.list_connections().await? {
        out.push(view(&state, profile).await?);
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    connection: ConnectionView,
    /// An open session was closed because the new settings change how to connect.
    session_closed: bool,
}

/// Fields that do not affect an already open session.
fn same_endpoint(a: &ConnectionProfile, b: &ConnectionProfile) -> bool {
    let cosmetic = |p: &ConnectionProfile| ConnectionProfile {
        name: String::new(),
        environment: Default::default(),
        favorite: false,
        tags: Vec::new(),
        created_at: 0,
        updated_at: 0,
        last_used_at: None,
        ..p.clone()
    };
    cosmetic(a) == cosmetic(b)
}

#[tauri::command]
pub async fn save_connection(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    password: PasswordChange,
) -> Result<SaveResult> {
    let previous = match profile.id.as_str() {
        "" => None,
        id => Some(state.store.get_connection(id).await?),
    };
    let password_changed = !matches!(password, PasswordChange::Keep);
    let saved = state.store.save_connection(profile).await?;
    let key = password_key(&saved.id);
    state
        .with_secrets(move |s| match password {
            PasswordChange::Keep => Ok(()),
            PasswordChange::Set { value } => s.set(&key, &value),
            PasswordChange::Clear => s.delete(&key),
        })
        .await?;
    // An open session keeps using the settings it was opened with: close it when they changed.
    let session_closed = match &previous {
        Some(old) if password_changed || !same_endpoint(old, &saved) => {
            state.remove_session(&saved.id).await
        }
        _ => false,
    };
    Ok(SaveResult {
        connection: view(&state, saved).await?,
        session_closed,
    })
}

#[tauri::command]
pub async fn duplicate_connection(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<ConnectionView> {
    let mut copy = state.store.get_connection(&id).await?;
    let password = stored_password(&state, &id).await?;
    copy.id = String::new();
    copy.name = name;
    copy.favorite = false;
    let saved = state.store.save_connection(copy).await?;
    if let Some(pw) = password {
        let key = password_key(&saved.id);
        state.with_secrets(move |s| s.set(&key, &pw)).await?;
    }
    view(&state, saved).await
}

#[tauri::command]
pub async fn delete_connection(state: State<'_, AppState>, id: String) -> Result<()> {
    state.remove_session(&id).await;
    let key = password_key(&id);
    state.with_secrets(move |s| s.delete(&key)).await?;
    state.store.delete_connection(&id).await
}

/// Connect with the values currently in the form, without saving anything.
#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
    password: PasswordChange,
) -> Result<ServerInfo> {
    let profile = profile.normalized()?;
    let password = match password {
        PasswordChange::Set { value } => Some(value),
        PasswordChange::Clear => None,
        PasswordChange::Keep => stored_password(&state, &profile.id).await?,
    };
    let started = std::time::Instant::now();
    let session = Session::connect(&profile, password.as_deref()).await?;
    let info = session.server_info().await;
    session.close().await;
    let mut info = info?;
    info.connect_ms = started.elapsed().as_millis() as u64;
    Ok(info)
}

#[tauri::command]
pub async fn open_connection(state: State<'_, AppState>, id: String) -> Result<ServerInfo> {
    let profile = state.store.get_connection(&id).await?;
    let password = stored_password(&state, &id).await?;
    let started = std::time::Instant::now();
    let session = Session::connect(&profile, password.as_deref()).await?;
    let mut info = match session.server_info().await {
        Ok(info) => info,
        Err(e) => {
            session.close().await;
            return Err(e);
        }
    };
    info.connect_ms = started.elapsed().as_millis() as u64;
    state.insert_session(&id, session).await;
    state.store.mark_connection_used(&id).await?;
    Ok(info)
}

#[tauri::command]
pub async fn close_connection(state: State<'_, AppState>, id: String) -> Result<()> {
    state.remove_session(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn list_databases(state: State<'_, AppState>, id: String) -> Result<Vec<Namespace>> {
    state.session(&id).await?.list_databases().await
}

#[tauri::command]
pub async fn list_schemas(
    state: State<'_, AppState>,
    id: String,
    database: String,
) -> Result<Vec<Namespace>> {
    state.session(&id).await?.list_schemas(&database).await
}

#[tauri::command]
pub async fn list_objects(
    state: State<'_, AppState>,
    id: String,
    database: String,
    schema: Option<String>,
) -> Result<Vec<DbObject>> {
    state
        .session(&id)
        .await?
        .list_objects(&database, schema.as_deref())
        .await
}

#[tauri::command]
pub async fn describe_table(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
) -> Result<TableMeta> {
    state.session(&id).await?.describe_table(&table).await
}

#[tauri::command]
pub async fn fetch_rows(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
    page: PageRequest,
) -> Result<RowPage> {
    state.session(&id).await?.fetch_rows(&table, &page).await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<HashMap<String, String>> {
    state.store.settings().await
}

#[tauri::command]
pub async fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<()> {
    if key.is_empty() || key.len() > 128 {
        return Err(Error::Invalid("Invalid setting key".into()));
    }
    state.store.set_setting(&key, &value).await
}

#[tauri::command]
pub async fn detect_ai_clis() -> Vec<CliStatus> {
    ai_detect::detect_all().await
}

/// Lets an injected end-to-end script hand its findings to the test runner.
/// Only does anything in debug builds started with `PURICAT_E2E_REPORT` set.
#[tauri::command]
pub fn e2e_report(text: String) -> Result<()> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("PURICAT_E2E_REPORT") {
        return Ok(std::fs::write(path, text)?);
    }
    let _ = text;
    Err(Error::Unsupported("Not available in this build".into()))
}
