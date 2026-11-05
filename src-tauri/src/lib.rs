mod ai;
mod core;
mod database;
mod dependencies;
mod git;
mod integrations;
mod notifications;
#[cfg(debug_assertions)]
mod qa_bridge;
mod runtime;
mod server;
mod terminal;

use serde_json::Value;
use tauri::Manager;

#[tauri::command]
async fn desktop_invoke(
    app: tauri::AppHandle,
    namespace: String,
    method: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || match namespace.as_str() {
        "terminalTabs" => terminal::tabs_dispatch(&app, &method, args),
        "sessionDependency" | "githubCliDependency" => {
            runtime::dependency_dispatch(&namespace, &method, args)
        }
        "aiAccounts" | "aiUsage" => ai::dispatch(&app, &namespace, &method, args),
        "mobileBridge" | "releaseUpdates" | "backgroundTaskSync" => {
            integrations::dispatch(&app, &namespace, &method, args)
        }
        _ => core::dispatch(&app, &namespace, &method, args),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn desktop_native(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    method: String,
    args: Vec<Value>,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || runtime::native(&app, &window, &method, args))
        .await
        .map_err(|e| e.to_string())?
}

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(terminal::TerminalState::default())
        .manage(runtime::NativeState::default());
    #[cfg(target_os = "macos")]
    let builder = builder.menu(runtime::desktop_menu);
    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        desktop_invoke,
        desktop_native,
        qa_bridge::qa_result,
        qa_bridge::qa_snapshot,
        qa_bridge::qa_resize
    ]);
    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![desktop_invoke, desktop_native]);
    builder
        .setup(|app| {
            let path = runtime::database_path(app.handle())?;
            app.manage(database::Database::open(&path)?);
            runtime::initialize(app.handle())?;
            #[cfg(debug_assertions)]
            qa_bridge::start(app.handle().clone())?;
            server::start(app.handle().clone())?;
            integrations::start_background(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(target_os = "macos")]
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                window
                    .app_handle()
                    .state::<runtime::NativeState>()
                    .last_window_closing
                    .store(
                        window.app_handle().webview_windows().len() == 1,
                        std::sync::atomic::Ordering::SeqCst,
                    );
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                terminal::close_client(window.app_handle(), window.label());
                runtime::close_window(window.app_handle(), window.label());
            }
        })
        .build(tauri::generate_context!())
        .expect("KanVibe runtime initialization failed")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                if app
                    .state::<runtime::NativeState>()
                    .take_last_window_close(code)
                {
                    api.prevent_exit();
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                if app.webview_windows().is_empty() {
                    if let Some(config) = app.config().app.windows.first() {
                        if let Err(error) = tauri::WebviewWindowBuilder::from_config(app, config)
                            .and_then(|builder| builder.build())
                        {
                            eprintln!("Failed to reopen KanVibe: {error}");
                        }
                    }
                } else if let Some(window) = app.webview_windows().values().next() {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            tauri::RunEvent::Exit => terminal::shutdown(app),
            _ => {}
        });
}
