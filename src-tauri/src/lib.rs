mod backups;
mod commands;
mod state;
mod updates;

pub use backups::run_backups;

use tauri::Manager;

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        // No updater permission is in the capability file: the web view reaches the plugin
        // only through the commands of `updates`.
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            // The app may have moved since a schedule was saved: point the background runner
            // at this copy again. Nothing happens when no job has a schedule.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = backups::sync_agent(&handle.state::<state::AppState>()).await;
            });
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
            commands::describe_table,
            commands::fetch_rows,
            commands::count_rows,
            commands::fetch_cell,
            commands::plan_changes,
            commands::apply_changes,
            commands::database_options,
            commands::plan_create_database,
            commands::create_database,
            commands::database_details,
            commands::plan_alter_database,
            commands::alter_database,
            commands::plan_drop_database,
            commands::drop_database,
            commands::table_definition,
            commands::plan_table_change,
            commands::apply_table_change,
            commands::view_select,
            commands::plan_view,
            commands::save_view,
            commands::preview_view_select,
            commands::list_accounts,
            commands::account_details,
            commands::plan_user,
            commands::create_user,
            commands::plan_password,
            commands::change_password,
            commands::plan_drop_user,
            commands::drop_user,
            commands::plan_flush,
            commands::flush_server,
            commands::column_types,
            commands::plan_create_table,
            commands::create_table,
            commands::table_indexes,
            commands::plan_alter_table,
            commands::alter_table,
            commands::column_definitions,
            commands::foreign_keys,
            commands::list_processes,
            commands::plan_end_process,
            commands::end_process,
            commands::unique_keys,
            commands::analyze_paste,
            commands::apply_paste,
            commands::cancel_job,
            commands::analyze_sql,
            commands::run_sql,
            commands::cancel_sql,
            commands::close_console,
            commands::list_history,
            commands::clear_history,
            commands::list_scripts,
            commands::get_script,
            commands::create_script,
            commands::update_script,
            commands::rename_script,
            commands::delete_script,
            commands::export_result,
            commands::pick_table_file,
            commands::pick_database_file,
            commands::pick_sql_file,
            commands::read_table_file,
            commands::get_settings,
            commands::set_setting,
            commands::detect_ai_clis,
            commands::agent_models,
            commands::agent_context,
            commands::agent_send,
            commands::agent_proposal_run,
            commands::agent_proposal_reject,
            commands::agent_audit,
            commands::clear_agent_audit,
            backups::backup_plan,
            backups::pick_backup_folder,
            backups::save_backup_job,
            backups::run_backup,
            backups::list_backup_runs,
            backups::reveal_backup,
            backups::restore_plan,
            backups::restore_backup,
            updates::check_update,
            updates::install_update,
            updates::restart_app,
            commands::e2e_report,
        ]);

    #[cfg(target_os = "macos")]
    let builder = {
        use tauri::Emitter;
        builder.menu(updates::menu).on_menu_event(|app, event| {
            if event.id() == updates::MENU_CHECK {
                let _ = app.emit(updates::MENU_EVENT, ());
            }
        })
    };

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
                // A covered WKWebView pauses its timers and animations, which stalls the test.
                let _ = webview.window().set_always_on_top(true);
                let _ = webview.eval(&script);
            }
        })
    }
}
