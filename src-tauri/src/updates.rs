//! Updating the app itself ([ADR 0020](../../docs/adr/0020-in-app-updates.md)).
//!
//! The only network request PuriCat makes that is not to a database the user connected:
//! one `GET` of the release manifest on GitHub, and, when the user asks for it, one of the
//! package. Nothing is installed that does not carry a signature made with the release key
//! for the very version the manifest announced. The web view is given no updater
//! permission: it can only call the three commands below.

use std::time::Duration;

use puricat_core::{Error, Result};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::state::AppState;

/// How long the look at the manifest may take: it runs unasked at start-up.
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// The package is some tens of megabytes; a slow line must still finish.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Progress is reported this often, not per network chunk.
const PROGRESS_STEP: u64 = 256 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    current_version: String,
    /// As the manifest wrote it (RFC 3339).
    date: Option<String>,
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    downloaded: u64,
    total: Option<u64>,
}

fn updater(app: &AppHandle) -> Result<Updater> {
    let builder = app.updater_builder().timeout(CHECK_TIMEOUT);
    // A development build never asks GitHub: a test names a stand-in server, and without
    // one there is no check at all.
    #[cfg(debug_assertions)]
    let builder = match std::env::var("PURICAT_UPDATE_ENDPOINT") {
        Ok(url) => {
            let url = url
                .parse()
                .map_err(|e| Error::Invalid(format!("PURICAT_UPDATE_ENDPOINT: {e}")))?;
            builder.endpoints(vec![url]).map_err(check_failed)?
        }
        Err(_) => {
            return Err(Error::Unsupported(
                "A development build does not check for updates.".into(),
            ))
        }
    };
    builder.build().map_err(check_failed)
}

fn check_failed(e: tauri_plugin_updater::Error) -> Error {
    Error::Io(std::io::Error::other(format!(
        "Could not check for updates: {e}"
    )))
}

fn install_failed(e: tauri_plugin_updater::Error) -> Error {
    use tauri_plugin_updater::Error as E;
    let text = match &e {
        E::Minisign(_) | E::MissingSignedVersion | E::SignedVersionMismatch { .. } => format!(
            "The download did not pass the signature check, so nothing was installed ({e})."
        ),
        E::Reqwest(_) | E::Network(_) => format!("The download failed: {e}"),
        E::PreviousAppNotRestored(_) => format!("The update failed: {e}"),
        _ => format!("The update could not be installed and PuriCat was left as it was: {e}"),
    };
    Error::Io(std::io::Error::other(text))
}

/// Whether this process runs from a macOS app bundle. The updater replaces the directory
/// it works out from the executable's path: for anything but a bundle that would be the
/// build directory.
fn installed_app() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    in_bundle(&exe)
}

fn in_bundle(exe: &std::path::Path) -> bool {
    let Some(macos) = exe.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    macos.file_name().is_some_and(|n| n == "MacOS")
        && contents.file_name().is_some_and(|n| n == "Contents")
        && contents
            .parent()
            .and_then(|app| app.extension())
            .is_some_and(|ext| ext == "app")
}

/// Ask the release manifest whether a newer version exists. What was found is kept here:
/// installing takes no argument from the web view.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<UpdateInfo>> {
    let found = updater(&app)?.check().await.map_err(check_failed)?;
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        date: u
            .raw_json
            .get("pub_date")
            .and_then(|d| d.as_str())
            .map(str::to_string),
        notes: u.body.clone().filter(|n| !n.trim().is_empty()),
    });
    *state.update.lock().unwrap() = found;
    Ok(info)
}

/// Download the update found by the last check, verify its signature and put it in the
/// place of this app. The running process is not touched: the new version starts with the
/// next launch.
#[tauri::command]
pub async fn install_update(
    state: State<'_, AppState>,
    on_progress: Channel<UpdateProgress>,
) -> Result<()> {
    let found: Option<Update> = state.update.lock().unwrap().clone();
    let mut update =
        found.ok_or_else(|| Error::Invalid("No update was found to install.".into()))?;
    if !installed_app() {
        return Err(Error::Unsupported(
            "This copy of PuriCat does not run from an app bundle, so it cannot replace itself."
                .into(),
        ));
    }
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    let mut downloaded = 0u64;
    let mut reported = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                if downloaded - reported >= PROGRESS_STEP || Some(downloaded) == total {
                    reported = downloaded;
                    let _ = on_progress.send(UpdateProgress { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(install_failed)?;
    // Installed: a second request finds nothing to do.
    *state.update.lock().unwrap() = None;
    Ok(())
}

/// Start the app again, which is now the installed version.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.request_restart();
}

/// The id of "Check for Updates…" in the application menu.
#[cfg(target_os = "macos")]
pub const MENU_CHECK: &str = "check-updates";
/// What the window is told when that item is chosen.
#[cfg(target_os = "macos")]
pub const MENU_EVENT: &str = "menu:check-updates";

/// The default macOS menu with "Check for Updates…" under "About PuriCat".
#[cfg(target_os = "macos")]
pub fn menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind};

    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(application)) = menu.items()?.first() {
        let item = MenuItem::with_id(app, MENU_CHECK, "Check for Updates…", true, None::<&str>)?;
        application.insert(&item, 1)?;
    }
    Ok(menu)
}

#[cfg(test)]
mod tests {
    use super::in_bundle;
    use std::path::Path;

    #[test]
    fn only_an_app_bundle_may_replace_itself() {
        assert!(in_bundle(Path::new(
            "/Applications/PuriCat.app/Contents/MacOS/puricat"
        )));
        assert!(in_bundle(Path::new(
            "/Users/someone/Desktop/My Apps/PuriCat.app/Contents/MacOS/puricat"
        )));
        // `pnpm tauri dev` and `cargo run`: the updater would take the build directory.
        assert!(!in_bundle(Path::new("/src/PuriCat/target/debug/puricat")));
        assert!(!in_bundle(Path::new("/opt/MacOS/puricat")));
        assert!(!in_bundle(Path::new("/x/PuriCat.app/Contents/puricat")));
        assert!(!in_bundle(Path::new("puricat")));
    }
}
