//! The IPC surface. Every command is a thin wrapper over `puricat-core`.
//!
//! Passwords cross this boundary in one direction only (UI -> backend). Nothing here
//! returns a stored secret; the UI learns only whether one exists.

use std::collections::HashMap;

use puricat_core::agent::{self, AgentEvent, ChatRequest, ModelChoice, ToolSetup};
use puricat_core::ai_detect::{self, CliStatus, Provider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use puricat_core::db::console::{RunOptions, RunOutcome, Status};
use puricat_core::db::export::{self, Format};
use puricat_core::db::script;
use puricat_core::db::smart::{Analysis, PasteRequest, UniqueKey};
use puricat_core::db::write::{ApplyResult, ChangeSet, Statement};
use puricat_core::db::{
    DbObject, Namespace, PageRequest, RowPage, ServerInfo, Session, TableMeta, TableRef,
};
use puricat_core::model::ConnectionProfile;
use puricat_core::secrets::password_key;
use puricat_core::store::{AuditEntry, HistoryEntry};
use puricat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use crate::state::{AppState, ConsoleSlot, Proposal};

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
        bulk_review_rows: 0,
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
    // What was run on a connection goes with it. First, so that a failure here leaves the
    // connection whole rather than without its password.
    state.store.clear_history(&id).await?;
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

/// Columns of a table, for the SQL editor's completion.
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

/// The exact statements a save would run, for the review step.
#[tauri::command]
pub async fn plan_changes(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
    changes: ChangeSet,
) -> Result<Vec<Statement>> {
    state
        .session(&id)
        .await?
        .plan_changes(&table, &changes)
        .await
}

/// Save staged edits in one transaction.
#[tauri::command]
pub async fn apply_changes(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
    changes: ChangeSet,
) -> Result<ApplyResult> {
    // Refuse early with a clear message; the server would reject the write anyway.
    if state.store.get_connection(&id).await?.read_only {
        return Err(Error::Invalid(
            "This connection is read-only. Turn off the read-only session in its settings to edit data.".into(),
        ));
    }
    state
        .session(&id)
        .await?
        .apply_changes(&table, &changes)
        .await
}

/// Keys a pasted row can be matched on (primary key first, then unique indexes).
#[tauri::command]
pub async fn unique_keys(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
) -> Result<Vec<UniqueKey>> {
    state.session(&id).await?.unique_keys(&table).await
}

/// Smart Paste preview: what running it would do, row by row. Reads only.
#[tauri::command]
pub async fn analyze_paste(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
    request: PasteRequest,
) -> Result<Analysis> {
    state
        .session(&id)
        .await?
        .analyze_paste(&table, &request)
        .await
}

/// Smart Paste run. `expected` is the fingerprint of the previewed outcome; if a fresh
/// analysis disagrees, nothing runs. Progress is streamed; `cancel_job` rolls it back.
#[tauri::command]
pub async fn apply_paste(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
    request: PasteRequest,
    expected: String,
    job: String,
    on_progress: Channel<u32>,
) -> Result<ApplyResult> {
    if state.store.get_connection(&id).await?.read_only {
        return Err(Error::Invalid(
            "This connection is read-only. Turn off the read-only session in its settings to edit data.".into(),
        ));
    }
    let session = state.session(&id).await?;
    let cancelled = Arc::new(AtomicBool::new(false));
    state
        .jobs
        .lock()
        .unwrap()
        .insert(job.clone(), cancelled.clone());
    let keep_going = move |done: usize| {
        if done % 20 == 0 {
            let _ = on_progress.send(done as u32);
        }
        !cancelled.load(Ordering::Relaxed)
    };
    let result = session
        .apply_paste(&table, &request, &expected, &keep_going)
        .await;
    state.jobs.lock().unwrap().remove(&job);
    result
}

/// Ask a running job to stop. It rolls back at the next statement boundary.
#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job: String) {
    if let Some(flag) = state.jobs.lock().unwrap().get(&job) {
        flag.store(true, Ordering::Relaxed);
    }
}

// ---- SQL console -----------------------------------------------------------------------

/// Split a script into statements and say what each one does, for the pre-run confirmation.
#[tauri::command]
pub async fn analyze_sql(
    state: State<'_, AppState>,
    id: String,
    sql: String,
) -> Result<Vec<script::Statement>> {
    Ok(script::split(state.session(&id).await?.engine(), &sql))
}

async fn console_slot(
    state: &AppState,
    connection_id: &str,
    tab: &str,
    database: Option<String>,
) -> Result<Arc<ConsoleSlot>> {
    let mut consoles = state.consoles.lock().await;
    if let Some(slot) = consoles.get(tab) {
        if slot.connection_id == connection_id && slot.database == database {
            return Ok(slot.clone());
        }
    }
    let console = state
        .session(connection_id)
        .await?
        .open_console(database.as_deref())
        .await?;
    let slot = Arc::new(ConsoleSlot {
        connection_id: connection_id.to_string(),
        database,
        canceller: console.canceller(),
        console: tokio::sync::Mutex::new(console),
        stop: AtomicBool::new(false),
    });
    consoles.insert(tab.to_string(), slot.clone());
    Ok(slot)
}

/// Run a script on the tab's own connection and record it in the local history.
#[tauri::command]
pub async fn run_sql(
    state: State<'_, AppState>,
    id: String,
    tab: String,
    database: Option<String>,
    sql: String,
    options: RunOptions,
) -> Result<RunOutcome> {
    run_on_console(&state, &id, &tab, database, &sql, &options).await
}

/// How a run ended, in a word: the status of the last statement that was reached.
fn run_status(outcome: &RunOutcome) -> &'static str {
    match outcome.statements.last().map(|s| s.status) {
        Some(Status::Error) => "error",
        Some(Status::Blocked) => "blocked",
        Some(Status::Cancelled) => "cancelled",
        _ => "ok",
    }
}

fn run_summary(outcome: &RunOutcome) -> String {
    let rows: usize = outcome
        .statements
        .iter()
        .flat_map(|s| &s.results)
        .map(|r| r.rows.len())
        .sum();
    let affected: u64 = outcome
        .statements
        .iter()
        .filter_map(|s| s.rows_affected)
        .sum();
    format!(
        "{} statements, {rows} rows, {affected} affected",
        outcome.statements.len()
    )
}

/// Run a script on the console registered under `tab` (opened on first use) and record it
/// in the local history.
async fn run_on_console(
    state: &AppState,
    id: &str,
    tab: &str,
    database: Option<String>,
    sql: &str,
    options: &RunOptions,
) -> Result<RunOutcome> {
    let slot = console_slot(state, id, tab, database.clone()).await?;
    let mut console = slot.console.lock().await;
    slot.stop.store(false, Ordering::Relaxed);
    let outcome = console.run(sql, options, &slot.stop).await?;
    drop(console);
    if outcome.connection_lost {
        // Forget the dead connection; the tab's next run opens a new one.
        let mut consoles = state.consoles.lock().await;
        if consoles
            .get(tab)
            .is_some_and(|current| Arc::ptr_eq(current, &slot))
        {
            consoles.remove(tab);
        }
    }

    // History is a convenience: failing to write it must not fail the query.
    let _ = state
        .store
        .add_history(
            id,
            database.as_deref(),
            &script::redact(sql),
            run_status(&outcome),
            &run_summary(&outcome),
            outcome.elapsed_ms,
        )
        .await;
    Ok(outcome)
}

/// Interrupt the statement a tab is running. The rest of its script is not run.
#[tauri::command]
pub async fn cancel_sql(state: State<'_, AppState>, tab: String) -> Result<()> {
    let slot = state.consoles.lock().await.get(&tab).cloned();
    if let Some(slot) = slot {
        slot.stop.store(true, Ordering::Relaxed);
        slot.canceller.cancel().await?;
    }
    Ok(())
}

/// Drop a tab's connection (closing the tab). A running statement is interrupted and an open
/// transaction is rolled back by the server.
#[tauri::command]
pub async fn close_console(state: State<'_, AppState>, tab: String) -> Result<()> {
    let slot = state.consoles.lock().await.remove(&tab);
    if let Some(slot) = slot {
        slot.abandon().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn list_history(
    state: State<'_, AppState>,
    id: String,
    limit: u32,
) -> Result<Vec<HistoryEntry>> {
    state.store.list_history(&id, limit).await
}

#[tauri::command]
pub async fn clear_history(state: State<'_, AppState>, id: String) -> Result<()> {
    state.store.clear_history(&id).await
}

/// Write one result of the tab's last run to a file the user chose. Returns rows written.
#[tauri::command]
pub async fn export_result(
    state: State<'_, AppState>,
    tab: String,
    result: usize,
    format: Format,
    path: String,
) -> Result<usize> {
    let slot = state
        .consoles
        .lock()
        .await
        .get(&tab)
        .cloned()
        .ok_or_else(|| Error::NotFound("This query tab's results (run the query again)".into()))?;
    let console = slot.console.lock().await;
    let set = console.result(result)?;
    tokio::task::block_in_place(|| export::write(std::path::Path::new(&path), format, set))
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

// ---- agent chat -------------------------------------------------------------------------

/// Models offered in the chat's model selector for one provider.
#[tauri::command]
pub async fn agent_models(provider: Provider) -> Vec<ModelChoice> {
    agent::models(provider).await
}

/// The schema context for a chat: exactly the text that will be sent, so the UI can show
/// it first. Names and types only — never rows, hosts, user names or passwords.
#[tauri::command]
pub async fn agent_context(
    state: State<'_, AppState>,
    id: String,
    database: String,
    schema: Option<String>,
    sql: Option<String>,
) -> Result<String> {
    let session = state.session(&id).await?;
    let info = session.server_info().await?;
    let objects = session.list_objects(&database, schema.as_deref()).await?;
    let mut tables = Vec::with_capacity(objects.len());
    for (i, object) in objects.into_iter().enumerate() {
        let meta = if i < agent::MAX_CONTEXT_TABLES {
            let table = TableRef {
                database: database.clone(),
                schema: schema.clone(),
                name: object.name.clone(),
            };
            session.describe_table(&table).await.ok()
        } else {
            None
        };
        tables.push((object, meta));
    }
    // Password literals in the active query are masked, as they are in the history.
    let sql = sql.as_deref().map(script::redact);
    Ok(agent::render_context(
        info.engine,
        &info.version,
        &database,
        schema.as_deref(),
        &tables,
        sql.as_deref(),
    ))
}

/// Append to the AI activity log. The log is a record, not a gate: failing to write it does
/// not fail what was done.
#[allow(clippy::too_many_arguments)]
async fn audit(
    state: &AppState,
    connection_id: &str,
    database: &str,
    provider: &str,
    model: Option<&str>,
    event: &str,
    sql: &str,
    detail: &str,
) {
    let connection_name = state
        .store
        .get_connection(connection_id)
        .await
        .map(|profile| profile.name)
        .unwrap_or_default();
    let _ = state
        .store
        .add_audit(&AuditEntry {
            id: 0,
            at: 0,
            connection_id: connection_id.to_string(),
            connection_name,
            database: database.to_string(),
            provider: provider.to_string(),
            model: model.map(str::to_string),
            event: event.to_string(),
            // Password literals are masked, as in the query history.
            sql: script::redact(sql),
            detail: detail.chars().take(300).collect(),
        })
        .await;
}

/// One chat turn through the user's CLI. The answer streams over `on_event`, together with
/// what the tools did; the return value is the complete answer. `cancel_job` stops the CLI.
///
/// With `request.tools` the CLI gets PuriCat's tools for the named database of an open
/// connection: catalogue lookups, a read-only query when `readRows` is set, and proposals.
/// A proposal is stored here and run only by `agent_proposal_run`.
#[tauri::command]
pub async fn agent_send(
    state: State<'_, AppState>,
    job: String,
    request: ChatRequest,
    on_event: Channel<AgentEvent>,
) -> Result<String> {
    let tools = match &request.tools {
        Some(scope) => Some(ToolSetup {
            session: state.session(&scope.connection_id).await?,
            database: scope.database.clone(),
            schema: scope.schema.clone(),
            read_rows: scope.read_rows,
            // The CLI starts this same executable as its MCP server; it only relays.
            bridge: vec![
                std::env::current_exe()?.to_string_lossy().into_owned(),
                "--mcp-bridge".into(),
            ],
        }),
        None => None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    state.jobs.lock().unwrap().insert(job.clone(), stop.clone());

    let provider = request.provider.binary();
    let mut model = request.model.clone();
    let mut calls: HashMap<String, (String, String)> = HashMap::new();
    // (event, sql, detail) for the activity log, written once the turn is over.
    let mut log: Vec<(&str, String, String)> = Vec::new();
    let result = agent::chat(&request, &state.agent_scratch, tools, &stop, |event| {
        match &event {
            AgentEvent::Started { model: Some(used) } => model = Some(used.clone()),
            AgentEvent::ToolStarted { id, name, input } => {
                calls.insert(id.clone(), (name.clone(), input.clone()));
            }
            AgentEvent::ToolFinished { id, ok, output } => {
                // Reading rows is what gets logged; catalogue lookups are not.
                if let Some((name, input)) = calls.remove(id) {
                    if name == "run_select" {
                        // A query that was refused or failed read nothing: it is logged as such.
                        let event = if *ok { "read" } else { "read_failed" };
                        let first = output.lines().next().unwrap_or("");
                        log.push((event, input, first.to_string()));
                    }
                }
            }
            AgentEvent::Proposal {
                id, sql, reason, ..
            } => {
                if let Some(scope) = &request.tools {
                    state.proposals.lock().unwrap().insert(
                        id.clone(),
                        Proposal {
                            connection_id: scope.connection_id.clone(),
                            database: scope.database.clone(),
                            sql: sql.clone(),
                            provider: provider.to_string(),
                            model: model.clone(),
                            resume: None,
                            running: false,
                        },
                    );
                    log.push(("proposed", sql.clone(), reason.clone().unwrap_or_default()));
                }
            }
            _ => {}
        }
        let _ = on_event.send(event);
    })
    .await;
    state.jobs.lock().unwrap().remove(&job);
    if let Some(scope) = &request.tools {
        for (event, sql, detail) in log {
            audit(
                &state,
                &scope.connection_id,
                &scope.database,
                provider,
                model.as_deref(),
                event,
                &sql,
                &detail,
            )
            .await;
        }
    }
    result
}

/// The user approved a proposal: PuriCat runs it, on a connection of its own, with the
/// console's impact guard. `lift_limit` runs what the guard held back, without the limit.
#[tauri::command]
pub async fn agent_proposal_run(
    state: State<'_, AppState>,
    id: String,
    lift_limit: bool,
) -> Result<RunOutcome> {
    let proposal = {
        let mut proposals = state.proposals.lock().unwrap();
        let proposal = proposals.get_mut(&id).ok_or_else(|| {
            Error::NotFound(
                "That proposal (it was already decided, or the app was restarted)".into(),
            )
        })?;
        if proposal.running {
            return Err(Error::Conflict("This proposal is already running".into()));
        }
        proposal.running = true;
        proposal.clone()
    };
    let result = run_proposal(&state, &id, &proposal, lift_limit).await;
    let mut proposals = state.proposals.lock().unwrap();
    match &result {
        // Held back by the guard: it stays, so the user can run the rest without the limit.
        Ok(outcome) if outcome.resume.is_some() => {
            if let Some(kept) = proposals.get_mut(&id) {
                kept.running = false;
                kept.resume = outcome.resume.clone();
            }
        }
        // It ran (wholly or up to an error): it cannot be run again.
        Ok(_) => {
            proposals.remove(&id);
        }
        // Nothing ran (connection closed, read-only): it can be approved again.
        Err(_) => {
            if let Some(kept) = proposals.get_mut(&id) {
                kept.running = false;
            }
        }
    }
    result
}

async fn run_proposal(
    state: &AppState,
    id: &str,
    proposal: &Proposal,
    lift_limit: bool,
) -> Result<RunOutcome> {
    let profile = state.store.get_connection(&proposal.connection_id).await?;
    if profile.read_only {
        return Err(Error::Invalid(
            "This connection is read-only. Turn off the read-only session in its settings to run this.".into(),
        ));
    }
    let sql = if lift_limit {
        proposal
            .resume
            .clone()
            .ok_or_else(|| Error::Invalid("Nothing of this proposal was held back".into()))?
    } else {
        proposal.sql.clone()
    };
    let options = RunOptions {
        max_rows: 200,
        change_limit: (!lift_limit).then_some(profile.bulk_review_rows as u64),
    };
    let log = |event: &'static str, detail: String| {
        let sql = sql.clone();
        async move {
            audit(
                state,
                &proposal.connection_id,
                &proposal.database,
                &proposal.provider,
                proposal.model.as_deref(),
                event,
                &sql,
                &detail,
            )
            .await
        }
    };
    let approval = if lift_limit {
        "Approved by the user, without the row limit"
    } else {
        "Approved by the user"
    };
    log("approved", approval.into()).await;

    // A console of its own, registered like a query tab's so `cancel_sql` can stop it.
    let tab = format!("proposal:{id}");
    let result = run_on_console(
        state,
        &proposal.connection_id,
        &tab,
        Some(proposal.database.clone()),
        &sql,
        &options,
    )
    .await;
    // Closing it rolls back a transaction the script may have left open.
    if let Some(slot) = state.consoles.lock().await.remove(&tab) {
        slot.abandon().await;
    }
    match &result {
        Ok(outcome) => {
            let event = match run_status(outcome) {
                "ok" => "executed",
                "blocked" => "blocked",
                _ => "failed",
            };
            let mut detail = run_summary(outcome);
            if let Some(message) = outcome.statements.last().and_then(|s| s.message.as_ref()) {
                detail.push_str(&format!(" — {message}"));
            }
            if outcome.in_transaction {
                detail.push_str(" — a transaction was left open and rolled back");
            }
            log(event, detail).await;
        }
        Err(e) => log("failed", e.to_string()).await,
    }
    result
}

/// The user turned a proposal down (or cleared the conversation it belonged to).
#[tauri::command]
pub async fn agent_proposal_reject(state: State<'_, AppState>, id: String) -> Result<()> {
    let proposal = {
        let mut proposals = state.proposals.lock().unwrap();
        if proposals.get(&id).is_some_and(|p| p.running) {
            return Err(Error::Conflict("This proposal is running".into()));
        }
        proposals.remove(&id)
    };
    if let Some(proposal) = proposal {
        audit(
            &state,
            &proposal.connection_id,
            &proposal.database,
            &proposal.provider,
            proposal.model.as_deref(),
            "rejected",
            &proposal.sql,
            "",
        )
        .await;
    }
    Ok(())
}

/// The AI activity log, most recent first.
#[tauri::command]
pub async fn agent_audit(state: State<'_, AppState>, limit: u32) -> Result<Vec<AuditEntry>> {
    state.store.list_audit(limit).await
}

#[tauri::command]
pub async fn clear_agent_audit(state: State<'_, AppState>) -> Result<()> {
    state.store.clear_audit().await
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
