#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            backend::contracts::get_backend_status,
            backend::contracts::list_backend_modules,
            backend::board_snapshot::get_board_snapshot,
            backend::projects::scan_git_repositories,
            backend::projects::list_projects,
            backend::projects::register_projects_from_scan,
            backend::projects::delete_project,
            backend::projects::get_available_ssh_hosts,
            backend::projects::list_git_branches,
            backend::settings::get_app_settings_snapshot,
            backend::settings::update_app_settings,
            backend::terminal_sessions::list_terminal_sessions,
            backend::terminal_sessions::create_terminal_session,
            backend::worktree::get_runtime_tool_status,
            backend::worktree::list_worktrees,
            backend::worktree::create_worktree_session,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_, _| {});
}
