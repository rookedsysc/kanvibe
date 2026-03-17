use serde::Serialize;

use super::{projects, settings, terminal_sessions, worktree};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendModuleStatus {
    pub name: &'static str,
    pub stage: &'static str,
    pub source: &'static str,
    pub target: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus {
    pub runtime: &'static str,
    pub mode: &'static str,
    pub migration_stage: &'static str,
    pub node_runtime_required: bool,
    pub modules: Vec<BackendModuleStatus>,
}

fn build_module_statuses() -> Vec<BackendModuleStatus> {
    vec![
        BackendModuleStatus {
            name: "tasks",
            stage: "removed-from-active-runtime",
            source: "src/app/actions/kanban.ts",
            target: "integrate into rust desktop shell as needed",
        },
        BackendModuleStatus {
            name: projects::MODULE_NAME,
            stage: "implemented",
            source: "src/app/actions/project.ts",
            target: "src-tauri/src/backend/projects.rs",
        },
        BackendModuleStatus {
            name: terminal_sessions::MODULE_NAME,
            stage: "planned",
            source: "src/lib/terminal.ts",
            target: "src-tauri/src/backend/terminal_sessions.rs",
        },
        BackendModuleStatus {
            name: worktree::MODULE_NAME,
            stage: "implemented",
            source: "src/lib/worktree.ts",
            target: "src-tauri/src/backend/worktree.rs",
        },
        BackendModuleStatus {
            name: settings::MODULE_NAME,
            stage: "implemented",
            source: "src/app/actions/appSettings.ts",
            target: "src-tauri/src/backend/settings.rs",
        },
        BackendModuleStatus {
            name: "hooks",
            stage: "removed-from-active-runtime",
            source: "src/lib/openCodeHooksSetup.ts",
            target: "integrate into rust desktop shell as needed",
        },
        BackendModuleStatus {
            name: "aiSessions",
            stage: "removed-from-active-runtime",
            source: "src/lib/aiSessions/aggregateAiSessions.ts",
            target: "integrate into rust desktop shell as needed",
        },
        BackendModuleStatus {
            name: "diffs",
            stage: "removed-from-active-runtime",
            source: "src/app/actions/diff.ts",
            target: "integrate into rust desktop shell as needed",
        },
    ]
}

#[tauri::command]
pub fn get_backend_status() -> BackendStatus {
    BackendStatus {
        runtime: "tauri",
        mode: "migration",
        migration_stage: "rust-backend-shell",
        node_runtime_required: false,
        modules: build_module_statuses(),
    }
}

#[tauri::command]
pub fn list_backend_modules() -> Vec<BackendModuleStatus> {
    build_module_statuses()
}
