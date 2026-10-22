//! Desktop composition root. Command adapters live in `commands/`; daemon
//! communication and service lifecycle live in `daemon/`.

mod app_update;
mod clipboard_files;
mod commands;
mod config;
mod daemon;
#[cfg(target_os = "linux")]
mod microphone;
mod window;

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            microphone::configure(app)?;
            window::fit_main_window(app)?;
            Ok(())
        })
        .manage(daemon::DaemonManager::new())
        .manage(daemon::DaemonClient::new())
        .invoke_handler(tauri::generate_handler![
            commands::application::app_check_update,
            commands::application::app_install_update,
            commands::application::prepare_application_uninstall,
            commands::application::exit_application,
            commands::application::read_pasted_files,
            commands::daemon::daemon_bootstrap,
            commands::daemon::daemon_install,
            commands::daemon::daemon_check_update,
            commands::daemon::daemon_update,
            commands::daemon::daemon_health,
            commands::daemon::daemon_version,
            commands::daemon::get_daemon_config,
            commands::daemon::set_daemon_config,
            commands::daemon::vacuum_database,
            commands::agents::list_agents,
            commands::agents::install_agent,
            commands::agents::authenticate_agent,
            commands::agents::get_amarcode_acp_config,
            commands::agents::set_amarcode_acp_config,
            commands::chats::create_chat,
            commands::chats::list_chats,
            commands::chats::get_chat,
            commands::chats::get_message_parts,
            commands::chats::list_acp_events_for_chat,
            commands::chats::list_agent_runs_for_chat,
            commands::chats::get_attachment,
            commands::chats::update_chat,
            commands::chats::delete_chat,
            commands::sessions::discover_commands,
            commands::sessions::transcribe_audio,
            commands::sessions::prompt,
            commands::sessions::set_session_config_option,
            commands::sessions::cancel,
            commands::sessions::respond_permission,
            commands::sessions::respond_input,
            commands::workspace::list_workspace_files,
            commands::workspace::get_workspace_info,
            commands::workspace::list_workspace_changes,
            commands::workspace::stage_workspace_file,
            commands::workspace::unstage_workspace_file,
            commands::workspace::stage_all_workspace_files,
            commands::workspace::commit_workspace_changes,
            commands::workspace::get_workspace_file_diff,
            commands::workspace::read_workspace_file,
            commands::workspace::save_workspace_file,
            commands::events::subscribe_events,
        ])
        .build(tauri::generate_context!())
        .expect("error while building amarcode");

    // The native service manager exclusively owns the daemon lifecycle. The
    // desktop application has no daemon child process to stop on exit.
    app.run(|_, _| {});
}
