mod commands;
mod state;

use tauri::Manager;

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            // Tests point debug builds at a throwaway directory instead of the real settings.
            #[cfg(debug_assertions)]
            let dir = std::env::var_os("PURICAT_DATA_DIR")
                .map(Into::into)
                .unwrap_or(dir);
            std::fs::create_dir_all(&dir)?;
            let state = tauri::async_runtime::block_on(state::AppState::open(&dir))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_connections,
            commands::save_connection,
            commands::duplicate_connection,
            commands::delete_connection,
            commands::test_connection,
            commands::open_connection,
            commands::close_connection,
            commands::list_databases,
            commands::list_schemas,
            commands::list_objects,
            commands::fetch_rows,
            commands::plan_changes,
            commands::apply_changes,
            commands::unique_keys,
            commands::analyze_paste,
            commands::apply_paste,
            commands::cancel_job,
            commands::get_settings,
            commands::set_setting,
            commands::detect_ai_clis,
            commands::e2e_report,
        ]);

    #[cfg(debug_assertions)]
    let builder = e2e::install(builder);

    builder
        .run(tauri::generate_context!())
        .expect("error while running PuriCat");
}

/// End-to-end test hook, debug builds only. WKWebView has no WebDriver, so the E2E suite
/// drives the real window by injecting the script named by `PURICAT_E2E_SCRIPT` once the
/// page has loaded. Release builds contain none of this.
#[cfg(debug_assertions)]
mod e2e {
    use tauri::webview::PageLoadEvent;

    pub fn install(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
        let Some(script) = std::env::var_os("PURICAT_E2E_SCRIPT")
            .and_then(|path| std::fs::read_to_string(path).ok())
        else {
            return builder;
        };
        builder.on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = webview.eval(&script);
            }
        })
    }
}
