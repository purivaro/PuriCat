//! Backups: the commands of the backup dialog, the LaunchAgent that starts scheduled
//! backups while the app is closed, and the runner it starts
//! ([ADR 0021](../../docs/adr/0021-backups-with-dump-tools.md)).
//!
//! The runner is this same executable, started as `puricat --run-backups --data-dir <dir>`:
//! one program to sign and to find, and the one the Keychain already knows.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use puricat_core::backup::{self, BackupRequest, Choice, Format};
use puricat_core::backup_schedule::{self, Schedule, Subject};
use puricat_core::model::Engine;
use puricat_core::secrets::password_key;
use puricat_core::store::{now_ms, BackupJob, BackupRun, Store};
use puricat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use crate::commands::{e2e_pick, stored_password};
use crate::state::AppState;

// ---- the LaunchAgent -------------------------------------------------------------------

/// The agent's name in launchd. A development build has one of its own, so that trying
/// things out never replaces the agent of the installed app.
pub const AGENT_LABEL: &str = if cfg!(debug_assertions) {
    "com.purivaro.puricat.backups.dev"
} else {
    "com.purivaro.puricat.backups"
};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    /// Whether this build can install the agent at all.
    pub supported: bool,
    /// At least one job has a schedule.
    pub needed: bool,
    /// launchd has the agent loaded and it starts this copy of the app.
    pub installed: bool,
    /// Why scheduled backups will not run, when they will not.
    pub problem: Option<String>,
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The agent's property list: start the runner at every time some job is scheduled for.
pub fn agent_plist(label: &str, exe: &Path, data_dir: &Path, jobs: &[BackupJob]) -> String {
    let mut times: Vec<(Option<u8>, u8, u8)> =
        jobs.iter().flat_map(|j| j.schedule.calendar()).collect();
    times.sort();
    times.dedup();
    let entries: String = times
        .iter()
        .map(|(weekday, hour, minute)| {
            let weekday = weekday
                .map(|d| format!("<key>Weekday</key><integer>{d}</integer>"))
                .unwrap_or_default();
            format!("    <dict>{weekday}<key>Hour</key><integer>{hour}</integer><key>Minute</key><integer>{minute}</integer></dict>\n")
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--run-backups</string>
    <string>--data-dir</string>
    <string>{data}</string>
  </array>
  <key>StartCalendarInterval</key>
  <array>
{entries}  </array>
  <key>ProcessType</key><string>Background</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = xml(label),
        exe = xml(&exe.to_string_lossy()),
        data = xml(&data_dir.to_string_lossy()),
        log = xml(&data_dir.join("backup-runner.log").to_string_lossy()),
    )
}

#[cfg(target_os = "macos")]
mod launchd {
    use super::*;
    use std::process::Command;

    fn plist_path() -> Option<PathBuf> {
        Some(
            PathBuf::from(std::env::var_os("HOME")?)
                .join(format!("Library/LaunchAgents/{AGENT_LABEL}.plist")),
        )
    }

    fn domain() -> Option<String> {
        use std::os::unix::fs::MetadataExt;
        let home = std::env::var_os("HOME")?;
        Some(format!("gui/{}", std::fs::metadata(home).ok()?.uid()))
    }

    fn launchctl(args: &[&str]) -> bool {
        Command::new("/bin/launchctl")
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn loaded() -> bool {
        domain().is_some_and(|d| launchctl(&["print", &format!("{d}/{AGENT_LABEL}")]))
    }

    /// A development build installs the agent only when asked to (`PURICAT_BACKUP_AGENT=1`):
    /// the end-to-end test saves schedules and must leave nothing in the user's LaunchAgents.
    fn allowed() -> bool {
        !cfg!(debug_assertions) || std::env::var("PURICAT_BACKUP_AGENT").as_deref() == Ok("1")
    }

    pub fn status(jobs: &[BackupJob], data_dir: &Path) -> AgentStatus {
        let needed = jobs.iter().any(|j| j.schedule != Schedule::Off);
        if !allowed() {
            return AgentStatus {
                supported: false,
                needed,
                installed: false,
                problem: needed.then(|| "A development build does not install the background runner: scheduled backups do not run.".into()),
            };
        }
        let wanted = std::env::current_exe()
            .ok()
            .map(|exe| agent_plist(AGENT_LABEL, &exe, data_dir, jobs));
        let on_disk = plist_path().and_then(|p| std::fs::read_to_string(p).ok());
        let installed = needed && loaded() && on_disk.is_some() && on_disk == wanted;
        AgentStatus {
            supported: true,
            needed,
            installed,
            problem: (needed && !installed)
                .then(|| "The background runner is not installed in macOS, so scheduled backups do not run. Save the schedule again to install it.".into()),
        }
    }

    /// Make launchd's state match the jobs: the agent with their times, or no agent.
    pub fn sync(jobs: &[BackupJob], data_dir: &Path) -> Result<AgentStatus> {
        if !allowed() {
            return Ok(status(jobs, data_dir));
        }
        let (Some(path), Some(domain)) = (plist_path(), domain()) else {
            return Err(Error::Unsupported(
                "The home folder could not be found".into(),
            ));
        };
        let target = format!("{domain}/{AGENT_LABEL}");
        if !jobs.iter().any(|j| j.schedule != Schedule::Off) {
            launchctl(&["bootout", &target]);
            let _ = std::fs::remove_file(&path);
            return Ok(status(jobs, data_dir));
        }
        let exe = std::env::current_exe()?;
        let plist = agent_plist(AGENT_LABEL, &exe, data_dir, jobs);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(plist.as_str()) || !loaded() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, plist)?;
            // Loaded with the old times, if at all: out first, then in again.
            launchctl(&["bootout", &target]);
            if !launchctl(&["bootstrap", &domain, &path.to_string_lossy()]) {
                return Err(Error::Unsupported(
                    "macOS did not accept the background runner (launchctl bootstrap failed). The schedule is saved, but it will not run.".into(),
                ));
            }
        }
        Ok(status(jobs, data_dir))
    }
}

#[cfg(not(target_os = "macos"))]
mod launchd {
    use super::*;

    pub fn status(jobs: &[BackupJob], _data_dir: &Path) -> AgentStatus {
        let needed = jobs.iter().any(|j| j.schedule != Schedule::Off);
        AgentStatus {
            supported: false,
            needed,
            installed: false,
            problem: needed.then(|| "Scheduled backups are available on macOS only.".into()),
        }
    }

    pub fn sync(jobs: &[BackupJob], data_dir: &Path) -> Result<AgentStatus> {
        Ok(status(jobs, data_dir))
    }
}

/// Bring the agent in line with the jobs, off the async executor (it runs `launchctl`).
pub async fn sync_agent(state: &AppState) -> Result<AgentStatus> {
    let jobs = state.store.list_backup_jobs().await?;
    let data_dir = state.data_dir.clone();
    tokio::task::spawn_blocking(move || launchd::sync(&jobs, &data_dir))
        .await
        .map_err(|e| Error::Unsupported(e.to_string()))?
}

// ---- commands --------------------------------------------------------------------------

/// Everything the backup dialog of one database shows when it opens.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPlan {
    server_version: String,
    /// The dump tool that would be used, or why none can be.
    choice: Choice,
    /// The settings saved for this database, if any.
    job: Option<BackupJob>,
    /// The folder offered when none was chosen yet.
    default_destination: String,
    runs: Vec<BackupRun>,
    agent: AgentStatus,
    /// The next time the schedule runs, as local time `YYYY-MM-DDTHH:MM`, when it will.
    next_run: Option<String>,
}

fn default_destination() -> String {
    std::env::var_os("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join("Documents")
                .join("PuriCat Backups")
        })
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn next_run(job: Option<&BackupJob>, agent: &AgentStatus) -> Option<String> {
    // Never a time for a schedule that nothing will run.
    if !agent.installed {
        return None;
    }
    let next = job?.schedule.next(chrono::Local::now().naive_local())?;
    Some(next.format("%Y-%m-%dT%H:%M").to_string())
}

async fn agent_status(state: &AppState) -> Result<AgentStatus> {
    let jobs = state.store.list_backup_jobs().await?;
    let data_dir = state.data_dir.clone();
    tokio::task::spawn_blocking(move || launchd::status(&jobs, &data_dir))
        .await
        .map_err(|e| Error::Unsupported(e.to_string()))
}

#[tauri::command]
pub async fn backup_plan(
    state: State<'_, AppState>,
    id: String,
    database: String,
) -> Result<BackupPlan> {
    let profile = state.store.get_connection(&id).await?;
    let server_version = state.session(&id).await?.server_info().await?.version;
    let tools = backup::find_tools(profile.engine).await;
    let job = state.store.backup_job_for(&id, &database).await?;
    let agent = agent_status(&state).await?;
    Ok(BackupPlan {
        choice: backup::choose(profile.engine, &server_version, &tools),
        server_version,
        next_run: next_run(job.as_ref(), &agent),
        job,
        default_destination: default_destination(),
        runs: state
            .store
            .list_backup_runs(Some((&id, &database)), 20)
            .await?,
        agent,
    })
}

/// Let the user choose the folder backups go to. `None` when the dialog was cancelled.
#[tauri::command]
pub async fn pick_backup_folder(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>> {
    let path = match e2e_pick(&state) {
        Some(answer) => answer,
        None => {
            use tauri_plugin_dialog::DialogExt;
            let (chosen, answer) = tokio::sync::oneshot::channel();
            window
                .dialog()
                .file()
                .set_parent(&window)
                .set_can_create_directories(true)
                .pick_folder(move |folder| {
                    let _ = chosen.send(folder);
                });
            match answer.await {
                Ok(Some(folder)) => Some(
                    folder
                        .into_path()
                        .map_err(|e| Error::Invalid(format!("This folder cannot be used: {e}")))?,
                ),
                _ => None,
            }
        }
    };
    Ok(path.map(|p| p.to_string_lossy().into_owned()))
}

/// What the dialog sends: where, in which form, when, and how many to keep.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
    destination: String,
    format: Format,
    compress: bool,
    schedule: Schedule,
    keep: u32,
}

/// The folder backups go to: an absolute path, created if it is not there yet.
fn prepared_destination(text: &str) -> Result<PathBuf> {
    let path = PathBuf::from(text.trim());
    if !path.is_absolute() {
        return Err(Error::Invalid("Choose the folder the backups go to".into()));
    }
    std::fs::create_dir_all(&path).map_err(|e| {
        Error::Invalid(format!("The folder {} cannot be used: {e}", path.display()))
    })?;
    Ok(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedJob {
    job: BackupJob,
    agent: AgentStatus,
    next_run: Option<String>,
}

async fn save_job(
    state: &AppState,
    id: &str,
    database: &str,
    settings: BackupSettings,
) -> Result<BackupJob> {
    let profile = state.store.get_connection(id).await?;
    if !matches!(profile.engine, Engine::Mysql | Engine::Postgres) {
        return Err(Error::Unsupported(
            "Backups are available for MySQL and PostgreSQL connections.".into(),
        ));
    }
    if settings.format == Format::Custom && profile.engine != Engine::Postgres {
        return Err(Error::Invalid(
            "The custom archive format is PostgreSQL's".into(),
        ));
    }
    let destination = prepared_destination(&settings.destination)?;
    state
        .store
        .save_backup_job(&BackupJob {
            id: 0,
            connection_id: id.to_string(),
            database: database.to_string(),
            destination: destination.to_string_lossy().into_owned(),
            format: settings.format,
            compress: settings.compress,
            schedule: settings.schedule.validated()?,
            keep: settings.keep.min(1000),
            updated_at: 0,
        })
        .await
}

/// Save a database's backup settings and bring the background runner in line with every
/// schedule there is. Installing or removing the runner is what saving a schedule asks for:
/// the dialog says so before this is called.
#[tauri::command]
pub async fn save_backup_job(
    state: State<'_, AppState>,
    id: String,
    database: String,
    settings: BackupSettings,
) -> Result<SavedJob> {
    let job = save_job(&state, &id, &database, settings).await?;
    let agent = sync_agent(&state).await?;
    Ok(SavedJob {
        next_run: next_run(Some(&job), &agent),
        job,
        agent,
    })
}

/// Where a backup made now goes, and in which form.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupTarget {
    destination: String,
    format: Format,
    compress: bool,
}

/// Back the database up now, with these settings, which are remembered for the next time
/// (the schedule and the number to keep stay as they were). `job` names the run for
/// `cancel_job`. The run is in the history whatever its outcome.
#[tauri::command]
pub async fn run_backup(
    state: State<'_, AppState>,
    id: String,
    database: String,
    target: BackupTarget,
    job: String,
    on_progress: Channel<u64>,
) -> Result<BackupRun> {
    let BackupTarget {
        destination,
        format,
        compress,
    } = target;
    let profile = state.store.get_connection(&id).await?;
    let server_version = state.session(&id).await?.server_info().await?.version;
    let saved = state.store.backup_job_for(&id, &database).await?;
    let (schedule, keep) = saved
        .map(|j| (j.schedule, j.keep))
        .unwrap_or((Schedule::Off, 7));
    let settings = BackupSettings {
        destination,
        format,
        compress,
        schedule,
        keep,
    };
    let remembered = save_job(&state, &id, &database, settings).await?;
    let password = stored_password(&state, &profile).await?;

    let stop = Arc::new(AtomicBool::new(false));
    state.jobs.lock().unwrap().insert(job.clone(), stop.clone());
    let mut reported = 0u64;
    let run = backup_schedule::perform(
        &state.store,
        Subject {
            profile: &profile,
            password: password.as_deref(),
            server_version: &server_version,
            scheduled: false,
            keep: 0,
            started_at: now_ms(),
        },
        &BackupRequest {
            database,
            destination: remembered.destination.into(),
            format,
            compress,
        },
        &stop,
        |bytes| {
            // A message per megabyte is plenty for a progress line.
            if bytes - reported >= 1024 * 1024 || bytes < reported {
                reported = bytes;
                let _ = on_progress.send(bytes);
            }
        },
    )
    .await;
    state.jobs.lock().unwrap().remove(&job);
    run
}

/// The history: of one database, or of everything.
#[tauri::command]
pub async fn list_backup_runs(
    state: State<'_, AppState>,
    id: Option<String>,
    database: Option<String>,
    limit: u32,
) -> Result<Vec<BackupRun>> {
    let of = id.as_deref().zip(database.as_deref());
    state.store.list_backup_runs(of, limit).await
}

/// Show a backup in Finder. Only a file the history recorded: the web view names a run,
/// never a path.
#[tauri::command]
pub async fn reveal_backup(state: State<'_, AppState>, run: i64) -> Result<()> {
    let run = state.store.get_backup_run(run).await?;
    let file = run
        .file
        .filter(|f| run.removed_at.is_none() && Path::new(f).is_file())
        .ok_or_else(|| Error::NotFound("The backup file".into()))?;
    #[cfg(target_os = "macos")]
    {
        let shown = tokio::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg("--")
            .arg(&file)
            .status()
            .await?;
        if !shown.success() {
            return Err(Error::Unsupported(format!("Finder could not show {file}")));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(Error::Unsupported(format!("The file is {file}")))
    }
}

// ---- the runner ------------------------------------------------------------------------

/// Tell the user that a scheduled backup failed, through the system's notifications. The
/// texts are arguments of the script, never part of it.
fn notify(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
                title,
                body,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (title, body);
    }
}

/// `puricat --run-backups --data-dir <dir>`: run the jobs that are due, then end. No window.
/// The exit code is 0 unless the runner itself could not work; a failed backup is in the
/// history, in the log and in a notification.
pub fn run_backups(data_dir: &Path) -> std::result::Result<(), String> {
    // One runner at a time: launchd may start another while a long backup is running.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir.join("backup-runner.lock"))
        .map_err(|e| format!("the lock file: {e}"))?;
    if lock.try_lock().is_err() {
        eprintln!(
            "{}: another backup runner is at work; nothing to do",
            stamp()
        );
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let store = Store::open(&data_dir.join("puricat.db"))
            .await
            .map_err(|e| e.to_string())?;
        let secrets = crate::state::secret_store();
        let password_of = |profile: &puricat_core::model::ConnectionProfile| {
            if profile.engine == Engine::Sqlite {
                return Ok(None);
            }
            secrets.get(&password_key(&profile.id))
        };
        let runs = backup_schedule::run_due(&store, password_of, now_ms())
            .await
            .map_err(|e| e.to_string())?;
        for run in &runs {
            match run.status.as_str() {
                "ok" => eprintln!(
                    "{}: {} › {}: {} ({} bytes, {})",
                    stamp(),
                    run.connection_name,
                    run.database,
                    run.file.as_deref().unwrap_or(""),
                    run.bytes.unwrap_or(0),
                    run.tool
                ),
                _ => {
                    eprintln!(
                        "{}: {} › {}: FAILED: {}",
                        stamp(),
                        run.connection_name,
                        run.database,
                        run.message
                    );
                    notify(
                        "PuriCat: a scheduled backup failed",
                        &format!(
                            "{} › {}: {}",
                            run.connection_name, run.database, run.message
                        ),
                    );
                }
            }
        }
        if runs.is_empty() {
            eprintln!("{}: no backup is due", stamp());
        }
        Ok(())
    })
}

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(schedule: Schedule) -> BackupJob {
        BackupJob {
            id: 1,
            connection_id: "c".into(),
            database: "d".into(),
            destination: "/b".into(),
            format: Format::Sql,
            compress: true,
            schedule,
            keep: 7,
            updated_at: 0,
        }
    }

    #[test]
    fn the_agent_starts_the_runner_at_every_scheduled_time_once() {
        let jobs = [
            job(Schedule::Daily { hour: 2, minute: 0 }),
            job(Schedule::Off),
            job(Schedule::Weekly {
                days: vec![1, 7],
                hour: 23,
                minute: 30,
            }),
            // The same time as the first job: one entry.
            job(Schedule::Daily { hour: 2, minute: 0 }),
        ];
        let plist = agent_plist(
            "com.example.backups",
            Path::new("/Applications/Puri & Cat <1>.app/Contents/MacOS/puricat"),
            Path::new("/Users/someone/Library/Application Support/com.purivaro.puricat"),
            &jobs,
        );
        assert!(plist.contains("<key>Label</key><string>com.example.backups</string>"));
        // The path is text, whatever is in it.
        assert!(plist.contains(
            "<string>/Applications/Puri &amp; Cat &lt;1&gt;.app/Contents/MacOS/puricat</string>"
        ));
        assert!(plist.contains("<string>--run-backups</string>\n    <string>--data-dir</string>\n    <string>/Users/someone/Library/Application Support/com.purivaro.puricat</string>"));
        let entries: Vec<&str> = plist
            .lines()
            .filter(|l| l.contains("<key>Hour</key>"))
            .map(str::trim)
            .collect();
        assert_eq!(
            entries,
            [
                "<dict><key>Hour</key><integer>2</integer><key>Minute</key><integer>0</integer></dict>",
                "<dict><key>Weekday</key><integer>0</integer><key>Hour</key><integer>23</integer><key>Minute</key><integer>30</integer></dict>",
                "<dict><key>Weekday</key><integer>1</integer><key>Hour</key><integer>23</integer><key>Minute</key><integer>30</integer></dict>",
            ]
        );
        assert!(plist.contains("backup-runner.log</string>"));
    }

    #[test]
    fn the_folder_must_be_a_full_path() {
        assert!(prepared_destination("").is_err());
        assert!(prepared_destination("Backups").is_err());
        assert!(prepared_destination("~/Backups").is_err());
        let dir = std::env::temp_dir()
            .join(format!("puricat-test-{}", std::process::id()))
            .join("new folder");
        assert_eq!(
            prepared_destination(&format!("  {}  ", dir.display())).unwrap(),
            dir
        );
        assert!(dir.is_dir());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
