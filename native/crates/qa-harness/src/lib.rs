use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(all(debug_assertions, unix))]
use kanvibe_app::qa_control::KANVIBE_QA_WINDOW_ID_ENV;
use kanvibe_app::qa_control::{QaControlCommand, QaControlResponse};
use kanvibe_app::{
    KANVIBE_DB_PATH_ENV, KANVIBE_LOCALE_ENV, KANVIBE_REPO_ROOT_ENV, ShortcutInput,
    ShortcutPlatform, WindowOpenAction, WindowRecord, build_settings_shell,
    build_task_detail_shell, decide_task_navigation, match_task_detail_dock_shortcut,
    pane_layout_route,
    qa_control::{KANVIBE_QA_FFMPEG_ENV, KANVIBE_QA_SOCKET_ENV, protocol_capabilities},
    resolve_window_open_action, should_keep_current_route_for_notification_activation,
    task_detail_dock_items, task_detail_href,
};
use kanvibe_core::{
    BoardSnapshot, CreateTaskInput, DONE_PAGE_SIZE, DoneCleanupPlan, KanvibeDb, PaneCommand,
    PaneLayoutType, SavePaneLayoutInput, SessionType, TaskPriority, TaskStatus, TaskUpdatePatch,
    ThemePreference,
};
use kanvibe_git::{
    changed_files, create_worktree_with_session, current_branch, file_content,
    original_file_content, save_file_content,
};
use kanvibe_hooks::{
    AI_PROVIDERS, AiSession, AiSessionProvider, BoardEvent, HookProviderStatus, NotificationCenter,
    aggregate_ai_sessions, extract_plugin_hook_server_url, extract_shell_hook_server_url,
    hook_status_visible, local_hook_server_url, validate_hook_server_configuration,
};
use kanvibe_i18n::{BoardLabels, Locale, load_board_labels};
use kanvibe_pty::{
    SessionDependencyRegistry, ShellPlatform, build_session_dependency_check_command,
    build_session_dependency_install_command, build_tmux_pane_layout_commands,
    create_local_shell_environment, generate_zellij_layout_kdl,
};
use serde_json::{Value, json};

pub const SCENARIO_DIR_FROM_REPO_ROOT: &str = "qa/scenarios";
pub const BASELINE_MANIFEST_FROM_REPO_ROOT: &str = "qa/baseline/MANIFEST.md";
pub const NATIVE_REPLAY_DB_DIR_FROM_REPO_ROOT: &str = "qa/parity/native-db";
pub const KANVIBE_QA_ARTIFACT_ROOT_ENV: &str = "KANVIBE_QA_ARTIFACT_ROOT";
pub const NATIVE_VIDEO_DIR_FROM_REPO_ROOT: &str = "qa/parity/native-videos";
pub const NATIVE_APP_BUNDLE_TARGET_BYTES: u64 = 30 * 1024 * 1024;
const QA_EVENTUAL_ASSERTION_TIMEOUT: Duration = Duration::from_secs(10);
const QA_EVENTUAL_ASSERTION_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub const SCENARIO_IDS: &[&str] = &[
    "S01-board-load-and-columns",
    "S02-create-task-modal",
    "S03-task-detail-terminal-dock",
    "S04-task-detail-pr-and-ai-history",
    "S05-diff-route-file-tree",
    "S06-git-worktree-branch-flow",
    "S07-board-drag-drop-status",
    "S08-context-menu-status-and-delete",
    "S09-vim-keyboard-shortcuts-search",
    "S10-settings-pane-layout",
    "S11-notifications-hooks-background-sync",
    "S12-project-filter-and-done-pagination",
    "S13-task-detail-existing-window-focus",
    "S14-remote-session-dependencies",
];

pub fn scenario_count() -> usize {
    SCENARIO_IDS.len()
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct BaselineManifestRow {
    scenario_id: String,
    source: String,
    screen: String,
    video: String,
    notes: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct QaControlClient {
    socket_path: PathBuf,
}

impl QaControlClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn request(
        &self,
        command: QaControlCommand,
    ) -> Result<QaControlResponse, Box<dyn Error + Send + Sync>> {
        request_qa_control(&self.socket_path, &command)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HarnessSeedSnapshot {
    pub board: BoardSnapshot,
    pub labels: BoardLabels,
}

pub fn load_seed_snapshot(
    repo_root: impl AsRef<Path>,
    locale: Locale,
) -> Result<HarnessSeedSnapshot, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let database = KanvibeDb::open_read_only(repo_root.join("qa/seed/kanvibe-seed.sqlite"))?;
    let board = database.board_snapshot(DONE_PAGE_SIZE)?;
    let labels = load_board_labels(repo_root, locale)?;

    Ok(HarnessSeedSnapshot { board, labels })
}

pub fn read_only_board_report(
    repo_root: impl AsRef<Path>,
    locale: Locale,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let snapshot = load_seed_snapshot(repo_root, locale)?;
    let columns = snapshot
        .board
        .columns
        .iter()
        .map(|column| {
            let label = snapshot
                .labels
                .columns
                .iter()
                .find(|candidate| candidate.status == column.status)
                .map(|candidate| candidate.label.as_str())
                .unwrap_or(column.status.as_str());

            json!({
                "status": column.status.as_str(),
                "label": label,
                "taskCount": column.tasks.len(),
                "firstTaskTitle": column.tasks.first().map(|task| task.title.as_str()),
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "artifact": "slice-1-read-only-board",
        "locale": locale.code(),
        "scenarioSubset": ["S01-board-load-and-columns"],
        "projectCount": snapshot.board.projects.len(),
        "doneTotal": snapshot.board.done_total,
        "doneLimit": snapshot.board.done_limit,
        "newTask": snapshot.labels.new_task,
        "allProjects": snapshot.labels.all_projects,
        "columns": columns,
    }))
}

pub fn board_interaction_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let db_path = writable_seed_copy(repo_root, "slice-2-board-interactions")?;
    let database = KanvibeDb::open_read_write(&db_path)?;

    let created = database.create_task(CreateTaskInput {
        id: Some("qa-harness-created-task".to_owned()),
        title: Some("Harness created board task".to_owned()),
        description: Some("Created by qa-harness board interaction report".to_owned()),
        project_id: Some("qa-project-kanvibe".to_owned()),
        priority: Some(TaskPriority::Medium),
        ..CreateTaskInput::default()
    })?;
    let after_create = database.board_snapshot(DONE_PAGE_SIZE)?;

    let edited = database
        .update_task(
            &created.id,
            TaskUpdatePatch {
                title: Some("Harness edited board task".to_owned()),
                description: Some(None),
                priority: Some(Some(TaskPriority::High)),
            },
        )?
        .ok_or("created task disappeared during edit")?;

    let moved_to_progress = database
        .update_task_status(&created.id, TaskStatus::Progress)?
        .ok_or("created task disappeared during status move")?;

    database.reorder_tasks(&[
        "qa-task-todo-remote".to_owned(),
        "qa-task-todo-local".to_owned(),
        "qa-task-todo-unassigned".to_owned(),
    ])?;
    let reordered_todo = database.board_snapshot(DONE_PAGE_SIZE)?;

    let done_cleanup_plan = database.move_task_to_column(
        &created.id,
        TaskStatus::Done,
        &[
            created.id.clone(),
            "qa-task-done-migrated".to_owned(),
            "qa-task-done-cleanup".to_owned(),
            "qa-task-done-remote".to_owned(),
        ],
    )?;
    let (done_page, done_total_after_move) = database.more_done_tasks(0, 2)?;

    let deleted = database.delete_task(&created.id)?;
    let after_delete = database.board_snapshot(DONE_PAGE_SIZE)?;

    database.update_project_color("qa-project-api", "#123456")?;
    let remote_api_color = database
        .projects()?
        .into_iter()
        .find(|project| project.id == "qa-project-api")
        .and_then(|project| project.color)
        .unwrap_or_default();

    let search_match_count = after_delete
        .columns
        .iter()
        .flat_map(|column| &column.tasks)
        .filter(|task| {
            task.title.to_lowercase().contains("terminal")
                || task
                    .branch_name
                    .as_deref()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains("terminal")
        })
        .count();
    let kanvibe_project_task_count = after_delete
        .columns
        .iter()
        .flat_map(|column| &column.tasks)
        .filter(|task| task.project_id.as_deref() == Some("qa-project-kanvibe"))
        .count();

    Ok(json!({
        "artifact": "slice-2-board-interactions",
        "scenarioSubset": [
            "S02-create-task-modal",
            "S07-board-drag-drop-status",
            "S08-context-menu-status-and-delete",
            "S09-vim-keyboard-shortcuts-search",
            "S12-project-filter-and-done-pagination"
        ],
        "createdTask": {
            "id": created.id,
            "status": created.status.as_str(),
            "displayOrder": created.display_order,
        },
        "afterCreate": {
            "todoCount": after_create.task_count(TaskStatus::Todo),
        },
        "editedTask": {
            "title": edited.title,
            "description": edited.description,
            "priority": edited.priority.map(TaskPriority::as_str),
        },
        "movedToProgress": {
            "status": moved_to_progress.task.status.as_str(),
        },
        "doneTransitionCleanupPlan": {
            "hasPlan": done_cleanup_plan.is_some(),
            "hasResourcesToClean": done_cleanup_plan
                .as_ref()
                .is_some_and(DoneCleanupPlan::has_resources_to_clean),
            "rollbackStatus": done_cleanup_plan
                .as_ref()
                .map(|plan| plan.rollback.status.as_str()),
        },
        "todoOrderAfterReorder": reordered_todo
            .column(TaskStatus::Todo)
            .map(|column| column.tasks.iter().map(|task| task.id.as_str()).collect::<Vec<_>>())
            .unwrap_or_default(),
        "donePageAfterMove": {
            "doneTotal": done_total_after_move,
            "firstPageIds": done_page.iter().map(|task| task.id.as_str()).collect::<Vec<_>>(),
        },
        "deletedCreatedTask": deleted,
        "afterDelete": {
            "doneTotal": after_delete.done_total,
            "todoCount": after_delete.task_count(TaskStatus::Todo),
        },
        "search": {
            "query": "terminal",
            "matchCount": search_match_count,
        },
        "projectFilter": {
            "projectId": "qa-project-kanvibe",
            "visibleTaskCount": kanvibe_project_task_count,
        },
        "projectColor": {
            "projectId": "qa-project-api",
            "color": remote_api_color,
        }
    }))
}

pub fn task_detail_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let database = KanvibeDb::open_read_only(repo_root.join("qa/seed/kanvibe-seed.sqlite"))?;
    let terminal_shell = build_task_detail_shell(
        &database,
        "qa-task-progress-terminal",
        Some("ko"),
        ShortcutPlatform::Mac,
    )?
    .ok_or("missing qa-task-progress-terminal")?;
    let pr_shell = build_task_detail_shell(
        &database,
        "qa-task-review-diff",
        Some("ko"),
        ShortcutPlatform::Mac,
    )?
    .ok_or("missing qa-task-review-diff")?;
    let remote_shell = build_task_detail_shell(
        &database,
        "qa-task-todo-remote",
        Some("ko"),
        ShortcutPlatform::Mac,
    )?
    .ok_or("missing qa-task-todo-remote")?;
    let existing_href = task_detail_href("qa-task-progress-pr", Some("ko"));
    let navigation_decision = decide_task_navigation(
        "qa-task-progress-pr",
        Some("ko"),
        false,
        std::slice::from_ref(&existing_href),
    );
    let shell_env = create_local_shell_environment(
        [
            ("PORT".to_owned(), "3000".to_owned()),
            ("HOST".to_owned(), "127.0.0.1".to_owned()),
            ("NODE_ENV".to_owned(), "production".to_owned()),
            (
                "KANVIBE_QA_SOCKET".to_owned(),
                "/tmp/kanvibe.sock".to_owned(),
            ),
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("CUSTOM_VISIBLE".to_owned(), "kept".to_owned()),
        ],
        "/Users/kanvibe",
        ShellPlatform::Mac,
    );
    let forbidden_env_keys = ["PORT", "HOST", "NODE_ENV", "KANVIBE_QA_SOCKET"];

    Ok(json!({
        "artifact": "slice-3-task-detail-pty-dock",
        "scenarioSubset": [
            "S03-task-detail-terminal-dock",
            "S04-task-detail-pr-and-ai-history",
            "S13-task-detail-existing-window-focus",
            "S14-remote-session-dependencies"
        ],
        "terminalTask": {
            "taskId": terminal_shell.task_id.as_str(),
            "href": terminal_shell.href.as_str(),
            "sessionType": terminal_shell.session_type.as_deref(),
            "sessionName": terminal_shell.session_name.as_deref(),
            "dockItems": terminal_shell.dock_items.iter().map(|item| item.id).collect::<Vec<_>>(),
            "shortcutLabels": terminal_shell.dock_items.iter().map(|item| item.shortcut_label.as_str()).collect::<Vec<_>>(),
            "cmd3MatchesTerminal": match_task_detail_dock_shortcut(
                ShortcutInput { key: '3', meta: true, ctrl: false, alt: false, shift: false },
                ShortcutPlatform::Mac,
            ) == Some(3),
        },
        "pullRequestTask": {
            "taskId": pr_shell.task_id.as_str(),
            "dockItems": pr_shell.dock_items.iter().map(|item| item.id).collect::<Vec<_>>(),
            "shortcutLabels": pr_shell.dock_items.iter().map(|item| item.shortcut_label.as_str()).collect::<Vec<_>>(),
            "pullRequestHref": pr_shell.dock_items.iter().find(|item| item.id == "pullRequest").and_then(|item| item.href.as_deref()),
            "aiProviderFilters": pr_shell.ai_provider_filters.as_slice(),
        },
        "withoutPullRequestDock": task_detail_dock_items(None, ShortcutPlatform::Mac)
            .iter()
            .map(|item| json!({ "id": item.id, "shortcut": item.shortcut_label.as_str() }))
            .collect::<Vec<_>>(),
        "existingWindowFocus": {
            "href": existing_href,
            "decision": format!("{navigation_decision:?}"),
        },
        "remoteSession": {
            "taskId": remote_shell.task_id.as_str(),
            "sshHost": remote_shell.ssh_host.as_deref(),
            "sessionType": remote_shell.session_type.as_deref(),
            "dependencyPanelSessionType": "zellij",
        },
        "ptyEnvironment": {
            "forbiddenAbsent": forbidden_env_keys.iter().all(|key| !shell_env.contains_key(*key)),
            "customVisible": shell_env.get("CUSTOM_VISIBLE").map(String::as_str),
            "home": shell_env.get("HOME").map(String::as_str),
            "pathContainsHomebrew": shell_env.get("PATH").is_some_and(|path| path.contains("/opt/homebrew/bin")),
        },
        "sidebarSettings": {
            "defaultCollapsed": terminal_shell.sidebar_default_collapsed,
            "hintDismissed": terminal_shell.sidebar_hint_dismissed,
        }
    }))
}

pub fn git_diff_report(repo_root: impl AsRef<Path>) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let repo = create_temp_git_repo("slice-4-git-diff")?;
    write_file(repo.join("src/app.ts"), "export const value = 'base';\n")?;
    git_raw(&repo, &["add", "."])?;
    git_raw(&repo, &["commit", "-m", "initial"])?;

    let session =
        create_worktree_with_session(&repo, "qa/branch-from-task", "main", SessionType::Tmux)?;
    write_file(
        session.worktree_path.join("src/app.ts"),
        "export const value = 'branch';\n",
    )?;
    git_raw(&session.worktree_path, &["add", "src/app.ts"])?;
    git_raw(&session.worktree_path, &["commit", "-m", "modify app"])?;
    write_file(
        session.worktree_path.join("src/untracked.ts"),
        "export const created = true;\n",
    )?;

    let diff_files = changed_files(&session.worktree_path, "main", "qa/branch-from-task")?;
    let original = original_file_content(&session.worktree_path, "main", "src/app.ts")?;
    let current = file_content(&session.worktree_path, "src/app.ts")?;
    save_file_content(
        &session.worktree_path,
        "src/app.ts",
        "export const value = 'saved';\n",
    )?;
    let saved = file_content(&session.worktree_path, "src/app.ts")?;
    let db_path = writable_seed_copy(repo_root, "slice-4-branch-from-task")?;
    let database = KanvibeDb::open_read_write(&db_path)?;
    let branched = database
        .branch_from_task(
            "qa-task-todo-local",
            "qa-project-kanvibe",
            "main",
            "qa/branch-from-task",
            SessionType::Tmux,
            &session.session_name,
            &session.worktree_path.to_string_lossy(),
        )?
        .ok_or("missing qa-task-todo-local")?;

    Ok(json!({
        "artifact": "slice-4-git-diff-worktree",
        "scenarioSubset": [
            "S05-diff-route-file-tree",
            "S06-git-worktree-branch-flow"
        ],
        "diffRoute": {
            "route": "/ko/task/qa-task-review-diff/diff",
            "fileCount": diff_files.len(),
            "files": diff_files.iter().map(|file| json!({
                "path": file.path,
                "status": file.status.as_str(),
                "additions": file.additions,
                "deletions": file.deletions,
            })).collect::<Vec<_>>(),
            "originalContainsBase": original.contains("base"),
            "currentContainsBranch": current.contains("branch"),
            "savedContainsSaved": saved.contains("saved"),
        },
        "branchFromTask": {
            "taskId": branched.id,
            "status": branched.status.as_str(),
            "branchName": branched.branch_name,
            "baseBranch": branched.base_branch,
            "sessionType": branched.session_type.map(|session_type| session_type.as_str()),
            "sessionName": branched.session_name,
            "worktreePath": branched.worktree_path,
            "worktreeCurrentBranch": current_branch(&session.worktree_path)?,
        }
    }))
}

pub fn notification_hooks_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let database = KanvibeDb::open_read_only(repo_root.join("qa/seed/kanvibe-seed.sqlite"))?;
    let background_sync_enabled = database
        .get_app_setting("background_sync_enabled")?
        .unwrap_or_else(|| "true".to_owned());
    let background_sync_interval_ms = database
        .get_app_setting("background_sync_interval_ms")?
        .unwrap_or_else(|| "600000".to_owned());
    let notification_enabled = database
        .get_app_setting("notification_enabled")?
        .unwrap_or_else(|| "true".to_owned());
    let notification_statuses = database
        .get_app_setting("notification_statuses")?
        .unwrap_or_else(|| "[]".to_owned());
    let mut notification_center = NotificationCenter::default();

    notification_center.push_event(BoardEvent::TaskStatusChanged {
        task_id: "qa-task-review-ai-history".to_owned(),
        task_title: "Review AI session history".to_owned(),
        new_status: "review".to_owned(),
    });
    notification_center.push_event(BoardEvent::TaskHookInstallFailed {
        task_id: "qa-task-review-ai-history".to_owned(),
        task_title: "Review AI session history".to_owned(),
        error: "fixture hook check".to_owned(),
    });
    notification_center.push_event(BoardEvent::BackgroundSyncReviewNeeded {
        merged_pull_request_count: 1,
        registered_worktree_count: 1,
        failure_count: 0,
    });

    let expected_hook_url = local_hook_server_url(9736);
    let shell_hook_url =
        extract_shell_hook_server_url(&format!("KANVIBE_URL=\"{expected_hook_url}\"\n"));
    let plugin_hook_url =
        extract_plugin_hook_server_url(&format!("const KANVIBE_URL = \"{expected_hook_url}\";"));
    let hook_validation = validate_hook_server_configuration(
        &[shell_hook_url.clone(), plugin_hook_url.clone()],
        Some(&expected_hook_url),
        true,
        false,
        true,
    );
    let hook_statuses = AI_PROVIDERS
        .iter()
        .map(|provider| HookProviderStatus {
            provider: *provider,
            installed: true,
            has_expected_hook_server_url: hook_validation.has_expected_hook_server_url,
            has_reachable_hook_server: hook_validation.has_reachable_hook_server,
        })
        .collect::<Vec<_>>();
    let ai_sessions = aggregate_ai_sessions(vec![
        AiSession {
            id: "claude-review-history".to_owned(),
            provider: AiSessionProvider::Claude,
            started_at: None,
            updated_at: Some("2026-07-08T00:20:00Z".to_owned()),
            matched_path: "/tmp/kanvibe-qa/repos/kanvibe".to_owned(),
            title: Some("Review AI session history".to_owned()),
            first_user_prompt: None,
            message_count: 4,
            source_ref: "claude.jsonl".to_owned(),
        },
        AiSession {
            id: "codex-native-hooks".to_owned(),
            provider: AiSessionProvider::Codex,
            started_at: None,
            updated_at: Some("2026-07-08T00:30:00Z".to_owned()),
            matched_path: "/tmp/kanvibe-qa/repos/kanvibe".to_owned(),
            title: Some("Native hook contract".to_owned()),
            first_user_prompt: None,
            message_count: 3,
            source_ref: "codex.jsonl".to_owned(),
        },
        AiSession {
            id: "gemini-copy-check".to_owned(),
            provider: AiSessionProvider::Gemini,
            started_at: None,
            updated_at: Some("2026-07-08T00:10:00Z".to_owned()),
            matched_path: "/tmp/kanvibe-qa/repos/kanvibe".to_owned(),
            title: Some("Copy check".to_owned()),
            first_user_prompt: None,
            message_count: 2,
            source_ref: "gemini.json".to_owned(),
        },
        AiSession {
            id: "opencode-remote-flow".to_owned(),
            provider: AiSessionProvider::OpenCode,
            started_at: None,
            updated_at: Some("2026-07-08T00:05:00Z".to_owned()),
            matched_path: "/tmp/kanvibe-qa/repos/kanvibe".to_owned(),
            title: Some("Remote flow".to_owned()),
            first_user_prompt: None,
            message_count: 1,
            source_ref: "opencode-remote-flow".to_owned(),
        },
    ]);

    Ok(json!({
        "artifact": "slice-5-notifications-hooks-ai",
        "scenarioSubset": ["S11-notifications-hooks-background-sync"],
        "notificationCenter": {
            "visible": true,
            "unreadCount": notification_center.unread_count(),
            "titles": notification_center.list().iter().map(|notification| notification.title.as_str()).collect::<Vec<_>>(),
        },
        "hookStatus": {
            "taskId": "qa-task-review-ai-history",
            "visible": hook_status_visible(&hook_statuses),
            "expectedHookServerUrl": expected_hook_url,
            "configuredShellUrl": shell_hook_url,
            "configuredPluginUrl": plugin_hook_url,
            "hasExpectedHookServerUrl": hook_validation.has_expected_hook_server_url,
            "hasReachableHookServer": hook_validation.has_reachable_hook_server,
            "providers": hook_statuses.iter().map(|status| json!({
                "provider": status.provider.as_str(),
                "installed": status.installed,
                "hasExpectedHookServerUrl": status.has_expected_hook_server_url,
                "hasReachableHookServer": status.has_reachable_hook_server,
            })).collect::<Vec<_>>(),
        },
        "aiSessions": {
            "providers": ai_sessions.sources.iter().map(|source| source.provider.as_str()).collect::<Vec<_>>(),
            "sessionCount": ai_sessions.sessions.len(),
            "firstSessionId": ai_sessions.sessions.first().map(|session| session.id.as_str()),
            "sourceSessionCounts": ai_sessions.sources.iter().map(|source| json!({
                "provider": source.provider.as_str(),
                "count": source.session_count,
            })).collect::<Vec<_>>(),
        },
        "appSettings": {
            "background_sync_enabled": background_sync_enabled,
            "background_sync_interval_ms": background_sync_interval_ms,
            "notification_enabled": notification_enabled,
            "notification_statuses": notification_statuses,
            "release_update_dismissed_versions": database.get_app_setting("release_update_dismissed_versions")?,
        }
    }))
}

pub fn settings_layout_remote_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let db_path = writable_seed_copy(repo_root, "slice-6-settings-layout-remote")?;
    let database = KanvibeDb::open_read_write(&db_path)?;

    let before_settings = build_settings_shell(&database, Some("ko"))?;
    let before_project_layout = database
        .get_project_pane_layout("qa-project-kanvibe")?
        .ok_or("missing qa-project-kanvibe pane layout")?;
    let global_layout = database
        .get_global_pane_layout()?
        .ok_or("missing global pane layout")?;

    database.set_vim_mode_enabled(false)?;
    database.set_theme_preference(ThemePreference::Dark)?;
    database.set_default_session_type(SessionType::Zellij)?;
    database.set_task_search_shortcut("Mod+Shift+K")?;
    database.set_background_sync_enabled(true)?;
    database.set_background_sync_interval_ms(300_000)?;
    database.dismiss_release_update_version("v1.2.3")?;

    let saved_project_layout = database.save_pane_layout(SavePaneLayoutInput {
        layout_type: PaneLayoutType::Vertical2,
        panes: vec![
            PaneCommand {
                position: 0,
                command: "pnpm desktop:dev".to_owned(),
            },
            PaneCommand {
                position: 1,
                command: "git status --short".to_owned(),
            },
        ],
        project_id: Some("qa-project-kanvibe".to_owned()),
        is_global: false,
    })?;
    let effective_project_layout = database
        .get_effective_pane_layout(Some("qa-project-kanvibe"))?
        .ok_or("missing effective project layout")?;
    let effective_fallback_layout = database
        .get_effective_pane_layout(Some("missing-project"))?
        .ok_or("missing effective fallback layout")?;
    let all_layouts = database.get_all_pane_layouts()?;
    let after_settings = build_settings_shell(&database, Some("ko"))?;
    let tmux_commands = build_tmux_pane_layout_commands(
        "kanvibe-feature-settings",
        effective_project_layout.layout_type,
        &effective_project_layout.panes,
        "/tmp/kanvibe-qa/repos/kanvibe__worktrees/feature-settings",
    );
    let zellij_kdl = generate_zellij_layout_kdl(
        effective_project_layout.layout_type,
        &effective_project_layout.panes,
        "/tmp/kanvibe-qa/repos/kanvibe__worktrees/feature-settings",
    );

    let mut dependency_registry = SessionDependencyRegistry::default();
    let dependency_before_install =
        dependency_registry.status(SessionType::Zellij, Some("qa-remote"));
    dependency_registry.remember_available(SessionType::Zellij, Some("qa-remote"));
    let dependency_after_install =
        dependency_registry.status(SessionType::Zellij, Some("qa-remote"));
    dependency_registry.remember_install_failure(
        SessionType::Tmux,
        Some("qa-remote-blocked"),
        "fixture install failure",
    );
    let blocked_remote_reason = dependency_registry
        .blocked_remote_host_reason("qa-remote-blocked")
        .map(ToOwned::to_owned);
    let zellij_install_command = build_session_dependency_install_command(SessionType::Zellij);

    let pane_layout_route = pane_layout_route(Some("ko"));
    let window_action = resolve_window_open_action(
        "http://localhost:3000/#/ko/pane-layout",
        Some("http://localhost:3000"),
        &[
            WindowRecord {
                id: "board-window".to_owned(),
                url: "http://localhost:3000/#/ko".to_owned(),
            },
            WindowRecord {
                id: "pane-layout-window".to_owned(),
                url: "http://localhost:3000/#/ko/pane-layout".to_owned(),
            },
        ],
        None,
    );
    let (window_action_type, existing_window_id) = match window_action {
        WindowOpenAction::External => ("external", None),
        WindowOpenAction::OpenInternal { .. } => ("open-internal", None),
        WindowOpenAction::FocusExisting {
            existing_window_id, ..
        } => ("focus-existing", Some(existing_window_id)),
    };

    Ok(json!({
        "artifact": "slice-6-settings-layout-remote",
        "scenarioSubset": [
            "S10-settings-pane-layout",
            "S14-remote-session-dependencies"
        ],
        "settings": {
            "route": after_settings.route,
            "themePreferenceBefore": before_settings.theme_preference.as_str(),
            "themePreferenceAfter": after_settings.theme_preference.as_str(),
            "defaultSessionType": after_settings.default_session_type.as_str(),
            "taskSearchShortcut": after_settings.task_search_shortcut,
            "vimModeEnabled": after_settings.vim_mode_enabled,
            "backgroundSyncEnabled": after_settings.background_sync.is_enabled,
            "backgroundSyncIntervalMs": after_settings.background_sync.interval_ms,
            "releaseUpdateDismissedVersions": after_settings.release_update_dismissed_versions,
        },
        "paneLayout": {
            "route": pane_layout_route,
            "projectId": "qa-project-kanvibe",
            "beforeLayoutType": before_project_layout.layout_type.as_str(),
            "savedLayoutType": saved_project_layout.layout_type.as_str(),
            "effectiveLayoutType": effective_project_layout.layout_type.as_str(),
            "fallbackLayoutType": effective_fallback_layout.layout_type.as_str(),
            "globalLayoutType": global_layout.layout_type.as_str(),
            "allLayoutCount": all_layouts.len(),
            "paneCommands": effective_project_layout.panes.iter().map(|pane| json!({
                "position": pane.position,
                "command": pane.command.as_str(),
            })).collect::<Vec<_>>(),
            "tmuxCommandCount": tmux_commands.len(),
            "firstTmuxCommand": tmux_commands.first().map(String::as_str),
            "zellijLayoutContainsVertical": zellij_kdl.contains("split_direction=\"vertical\""),
            "zellijLayoutFilename": kanvibe_pty::ZELLIJ_LAYOUT_FILENAME,
        },
        "remoteSessionDependency": {
            "taskId": "qa-task-todo-remote",
            "sshHost": "qa-remote",
            "sessionType": dependency_after_install.session_type.as_str(),
            "toolName": dependency_after_install.tool_name,
            "visible": dependency_before_install.is_remote && !dependency_before_install.available,
            "availableBeforeInstall": dependency_before_install.available,
            "availableAfterInstall": dependency_after_install.available,
            "checkCommand": build_session_dependency_check_command(SessionType::Zellij),
            "installCommandContainsCargo": zellij_install_command.contains("cargo install --locked zellij"),
            "blockedRemoteReason": blocked_remote_reason,
        },
        "windowPolicy": {
            "action": window_action_type,
            "existingWindowId": existing_window_id,
            "keepsCurrentRouteForBackgroundReview": should_keep_current_route_for_notification_activation(Some("background-sync-review")),
        }
    }))
}

pub fn scenario_control_protocol_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let scenarios = read_scenario_files(repo_root)?;
    let mut step_mappings = Vec::new();
    let mut assertion_mappings = Vec::new();
    let mut unsupported_steps = Vec::new();
    let mut unsupported_assertions = Vec::new();

    for scenario in &scenarios {
        let scenario_id = scenario["id"]
            .as_str()
            .ok_or("scenario missing id")?
            .to_owned();

        for step in scenario["steps"]
            .as_array()
            .ok_or("scenario missing steps")?
        {
            let action = step["action"].as_str().ok_or("step missing action")?;
            if let Some(command) = control_command_for_step(action) {
                step_mappings.push(json!({
                    "scenarioId": scenario_id,
                    "action": action,
                    "controlCommand": command,
                }));
            } else {
                unsupported_steps.push(json!({
                    "scenarioId": scenario_id,
                    "action": action,
                }));
            }
        }

        for assertion in scenario["assertions"]
            .as_array()
            .ok_or("scenario missing assertions")?
        {
            let assertion_type = assertion["type"].as_str().ok_or("assertion missing type")?;
            if let Some(command) = control_command_for_assertion(assertion_type) {
                assertion_mappings.push(json!({
                    "scenarioId": scenario_id,
                    "assertionType": assertion_type,
                    "controlCommand": command,
                }));
            } else {
                unsupported_assertions.push(json!({
                    "scenarioId": scenario_id,
                    "assertionType": assertion_type,
                }));
            }
        }
    }
    let coverage_pass = unsupported_steps.is_empty() && unsupported_assertions.is_empty();

    Ok(json!({
        "artifact": "native-qa-control-protocol",
        "scenarioSubset": SCENARIO_IDS,
        "socketEnv": KANVIBE_QA_SOCKET_ENV,
        "debugOnly": true,
        "protocolCapabilities": protocol_capabilities(),
        "scenarioFileCount": scenarios.len(),
        "stepMappingCount": step_mappings.len(),
        "assertionMappingCount": assertion_mappings.len(),
        "unsupportedSteps": unsupported_steps,
        "unsupportedAssertions": unsupported_assertions,
        "coveragePass": coverage_pass,
        "stepMappings": step_mappings,
        "assertionMappings": assertion_mappings,
    }))
}

pub fn qa_control_replay_plan_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let scenarios = read_scenario_files(repo_root)?;
    let mut scenario_plans = Vec::new();
    let mut unsupported_steps = Vec::new();
    let mut unsupported_assertions = Vec::new();
    let mut launch_action_count = 0usize;
    let mut socket_command_count = 0usize;
    let mut video_command_count = 0usize;

    for scenario in &scenarios {
        let scenario_id = scenario["id"]
            .as_str()
            .ok_or("scenario missing id")?
            .to_owned();
        let video_path = scenario_native_video_relative_path(&scenario_id, scenario);
        let mut replay_items = Vec::new();

        for step in scenario["steps"]
            .as_array()
            .ok_or("scenario missing steps")?
        {
            match replay_items_for_step(&scenario_id, step) {
                Some(items) => {
                    launch_action_count += items
                        .iter()
                        .filter(|item| item["kind"] == "launchApp")
                        .count();
                    socket_command_count += items
                        .iter()
                        .filter(|item| item["kind"] == "controlCommand")
                        .count();
                    for item in items {
                        let starts_app = item["kind"] == "launchApp";
                        replay_items.push(item);
                        if starts_app {
                            replay_items.push(video_capture_replay_item(
                                &scenario_id,
                                "start",
                                &video_path,
                            ));
                            socket_command_count += 1;
                            video_command_count += 1;
                        }
                    }
                }
                None => unsupported_steps.push(json!({
                    "scenarioId": scenario_id,
                    "action": step["action"],
                })),
            }
        }

        for assertion in scenario["assertions"]
            .as_array()
            .ok_or("scenario missing assertions")?
        {
            match replay_items_for_assertion(&scenario_id, assertion) {
                Some(items) => {
                    socket_command_count += items
                        .iter()
                        .filter(|item| item["kind"] == "controlCommand")
                        .count();
                    replay_items.extend(items);
                }
                None => unsupported_assertions.push(json!({
                    "scenarioId": scenario_id,
                    "assertionType": assertion["type"],
                })),
            }
        }
        replay_items.push(video_capture_replay_item(&scenario_id, "stop", &video_path));
        socket_command_count += 1;
        video_command_count += 1;

        scenario_plans.push(json!({
            "scenarioId": scenario_id,
            "title": scenario["title"],
            "nativeVideoPath": video_path,
            "replayItems": replay_items,
        }));
    }
    let coverage_pass = unsupported_steps.is_empty() && unsupported_assertions.is_empty();

    Ok(json!({
        "artifact": "native-qa-control-replay-plan",
        "scenarioSubset": SCENARIO_IDS,
        "socketEnv": KANVIBE_QA_SOCKET_ENV,
        "transport": "unix-line-json",
        "client": {
            "type": "QaControlClient",
            "requestShape": "one-json-command-line-one-json-response-line",
            "unixSupportedInThisBuild": cfg!(unix),
        },
        "scenarioFileCount": scenarios.len(),
        "launchActionCount": launch_action_count,
        "socketCommandCount": socket_command_count,
        "videoCommandCount": video_command_count,
        "videoArtifactCount": video_command_count / 2,
        "videoCapture": {
            "outputDir": native_videos_dir(),
            "macosStrategy": "QA control startVideoCapture/stopVideoCapture records screencapture frames and encodes mp4 with ffmpeg",
            "linux": "blocked",
            "ffmpegEnv": KANVIBE_QA_FFMPEG_ENV,
        },
        "unsupportedSteps": unsupported_steps,
        "unsupportedAssertions": unsupported_assertions,
        "coveragePass": coverage_pass,
        "scenarioPlans": scenario_plans,
    }))
}

pub fn qa_control_replay_execution_report(
    repo_root: impl AsRef<Path>,
    socket_path: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let socket_path = socket_path.as_ref();
    let client = QaControlClient::new(socket_path.to_path_buf());
    execute_qa_control_replay(
        repo_root.as_ref(),
        &client,
        "externalSocket",
        Some(socket_path),
    )
}

#[cfg(unix)]
pub fn native_app_launch_report(
    repo_root: impl AsRef<Path>,
    app_binary: Option<PathBuf>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let app_binary = resolve_native_app_binary(repo_root, app_binary);
    if !app_binary.is_file() {
        return Ok(json!({
            "artifact": "native-app-launch-contract",
            "status": "BLOCKED",
            "mode": "externalNativeAppProcess",
            "appBinary": app_binary.to_string_lossy(),
            "qaSocketReady": false,
            "blockers": [
                "native app debug binary is missing; run `cargo build -p kanvibe-app` before external launch replay"
            ],
        }));
    }

    let db_path = prepare_native_app_launch_seed_copy(repo_root)?;
    let socket_path = unique_socket_path("qa-native-app-launch")?;
    let mut child = Command::new(&app_binary)
        .current_dir(repo_root)
        .env_clear()
        .env(KANVIBE_REPO_ROOT_ENV, repo_root)
        .env(KANVIBE_DB_PATH_ENV, &db_path)
        .env(KANVIBE_LOCALE_ENV, "ko")
        .env(KANVIBE_QA_SOCKET_ENV, &socket_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let client = QaControlClient::new(socket_path.clone());
    let mut last_socket_error = None;

    for _ in 0..200 {
        match client.request(QaControlCommand::Ping) {
            Ok(QaControlResponse::Pong) => {
                let root_response = client.request(QaControlCommand::QueryElement {
                    id: "app.root".to_owned(),
                });
                let _ = child.kill();
                let output = child.wait_with_output()?;
                let _ = fs::remove_file(&socket_path);
                let root_response_json = match root_response {
                    Ok(response) => serde_json::to_value(response)?,
                    Err(error) => json!({ "type": "error", "message": error.to_string() }),
                };

                return Ok(json!({
                    "artifact": "native-app-launch-contract",
                    "status": if root_response_json["exists"].as_bool() == Some(true) { "PASS" } else { "FAIL" },
                    "mode": "externalNativeAppProcess",
                    "appBinary": app_binary.to_string_lossy(),
                    "dbCopyPath": db_path.to_string_lossy(),
                    "socketPath": socket_path.to_string_lossy(),
                    "qaSocketReady": true,
                    "processKilledAfterSocketReady": true,
                    "rootQuery": root_response_json,
                    "childEnvKeys": [
                        KANVIBE_REPO_ROOT_ENV,
                        KANVIBE_DB_PATH_ENV,
                        KANVIBE_LOCALE_ENV,
                        KANVIBE_QA_SOCKET_ENV
                    ],
                    "stdout": output_text(&output.stdout),
                    "stderr": output_text(&output.stderr),
                }));
            }
            Ok(other) => {
                last_socket_error = Some(format!("unexpected ping response: {other:?}"));
            }
            Err(error) => {
                last_socket_error = Some(error.to_string());
            }
        }

        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            let _ = fs::remove_file(&socket_path);

            return Ok(json!({
                "artifact": "native-app-launch-contract",
                "status": "BLOCKED",
                "mode": "externalNativeAppProcess",
                "appBinary": app_binary.to_string_lossy(),
                "dbCopyPath": db_path.to_string_lossy(),
                "socketPath": socket_path.to_string_lossy(),
                "qaSocketReady": false,
                "processExitedBeforeSocket": true,
                "exitStatus": output.status.code(),
                "blockers": [
                    "native app process exited before opening the QA socket; on this host the app may be a non-macOS headless stub"
                ],
                "lastSocketError": last_socket_error,
                "childEnvKeys": [
                    KANVIBE_REPO_ROOT_ENV,
                    KANVIBE_DB_PATH_ENV,
                    KANVIBE_LOCALE_ENV,
                    KANVIBE_QA_SOCKET_ENV
                ],
                "stdout": output_text(&output.stdout),
                "stderr": output_text(&output.stderr),
            }));
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    let _ = child.kill();
    let output = child.wait_with_output()?;
    let _ = fs::remove_file(&socket_path);

    Ok(json!({
        "artifact": "native-app-launch-contract",
        "status": "BLOCKED",
        "mode": "externalNativeAppProcess",
        "appBinary": app_binary.to_string_lossy(),
        "dbCopyPath": db_path.to_string_lossy(),
        "socketPath": socket_path.to_string_lossy(),
        "qaSocketReady": false,
        "processKilledAfterTimeout": true,
        "blockers": [
            "native app process did not open the QA socket before the launch timeout"
        ],
        "lastSocketError": last_socket_error,
        "childEnvKeys": [
            KANVIBE_REPO_ROOT_ENV,
            KANVIBE_DB_PATH_ENV,
            KANVIBE_LOCALE_ENV,
            KANVIBE_QA_SOCKET_ENV
        ],
        "stdout": output_text(&output.stdout),
        "stderr": output_text(&output.stderr),
    }))
}

#[cfg(not(unix))]
pub fn native_app_launch_report(
    _repo_root: impl AsRef<Path>,
    _app_binary: Option<PathBuf>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    Ok(json!({
        "artifact": "native-app-launch-contract",
        "status": "BLOCKED",
        "mode": "externalNativeAppProcessUnavailable",
        "qaSocketReady": false,
        "blockers": [
            "native app launch replay requires the Unix QA socket transport"
        ],
    }))
}

#[cfg(all(debug_assertions, unix))]
pub fn native_app_replay_report(
    repo_root: impl AsRef<Path>,
    app_binary: Option<PathBuf>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let app_binary = resolve_native_app_binary(repo_root, app_binary);
    let plan = qa_control_replay_plan_report(repo_root)?;
    if !app_binary.is_file() {
        return Ok(json!({
            "artifact": "native-app-replay-contract",
            "status": "BLOCKED",
            "mode": "externalNativeAppProcessPerScenario",
            "appBinary": app_binary.to_string_lossy(),
            "qaSocketReadyPass": false,
            "blockers": [
                "native app debug binary is missing; run `cargo build -p kanvibe-app` before external app replay"
            ],
            "sourceReplayPlanArtifact": plan["artifact"],
            "scenarioFileCount": plan["scenarioFileCount"],
        }));
    }

    let scenario_plans = plan["scenarioPlans"]
        .as_array()
        .ok_or("replay plan missing scenarioPlans")?;
    let mut totals = ReplayExecutionCounters::default();
    let mut scenario_results = Vec::new();
    let mut app_launch_attempt_count = 0usize;
    let mut qa_socket_ready_count = 0usize;
    let mut process_exited_before_socket_count = 0usize;
    let mut process_timeout_count = 0usize;

    for scenario_plan in scenario_plans {
        let scenario_id = scenario_plan["scenarioId"]
            .as_str()
            .ok_or("scenario plan missing scenarioId")?;
        let db_path = prepare_scenario_seed_copy(repo_root, scenario_id)?;
        let socket_path = unique_socket_path(&format!("qa-native-app-replay-{scenario_id}"))?;
        let mut child_env_keys = vec![
            KANVIBE_REPO_ROOT_ENV,
            KANVIBE_DB_PATH_ENV,
            KANVIBE_LOCALE_ENV,
            KANVIBE_QA_SOCKET_ENV,
        ];
        let mut command = Command::new(&app_binary);
        command
            .current_dir(repo_root)
            .env_clear()
            .env(KANVIBE_REPO_ROOT_ENV, repo_root)
            .env(KANVIBE_DB_PATH_ENV, &db_path)
            .env(KANVIBE_LOCALE_ENV, "ko")
            .env(KANVIBE_QA_SOCKET_ENV, &socket_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(ffmpeg_path) = std::env::var_os(KANVIBE_QA_FFMPEG_ENV) {
            command.env(KANVIBE_QA_FFMPEG_ENV, ffmpeg_path);
            child_env_keys.push(KANVIBE_QA_FFMPEG_ENV);
        }
        if let Some(window_id) = std::env::var_os(KANVIBE_QA_WINDOW_ID_ENV) {
            command.env(KANVIBE_QA_WINDOW_ID_ENV, window_id);
            child_env_keys.push(KANVIBE_QA_WINDOW_ID_ENV);
        }
        let mut child = command.spawn()?;
        app_launch_attempt_count += 1;
        let client = QaControlClient::new(socket_path.clone());
        let mut last_socket_error = None;
        let mut socket_ready = false;
        let mut process_exited_before_socket = false;

        for _ in 0..200 {
            match client.request(QaControlCommand::Ping) {
                Ok(QaControlResponse::Pong) => {
                    socket_ready = true;
                    break;
                }
                Ok(other) => {
                    last_socket_error = Some(format!("unexpected ping response: {other:?}"));
                }
                Err(error) => {
                    last_socket_error = Some(error.to_string());
                }
            }

            if child.try_wait()?.is_some() {
                process_exited_before_socket = true;
                break;
            }

            std::thread::sleep(Duration::from_millis(10));
        }

        if socket_ready {
            qa_socket_ready_count += 1;
            let mut scenario_result =
                execute_qa_control_scenario_plan(scenario_plan, &client, &mut totals)?;
            let _ = child.kill();
            let output = child.wait_with_output()?;
            let _ = fs::remove_file(&socket_path);
            if let Some(result) = scenario_result.as_object_mut() {
                result.insert(
                    "appProcess".to_owned(),
                    json!({
                        "appBinary": app_binary.to_string_lossy(),
                        "dbCopyPath": db_path.to_string_lossy(),
                        "socketPath": socket_path.to_string_lossy(),
                        "qaSocketReady": true,
                        "processKilledAfterReplay": true,
                        "childEnvKeys": child_env_keys,
                        "stdout": output_text(&output.stdout),
                        "stderr": output_text(&output.stderr),
                    }),
                );
            }
            scenario_results.push(scenario_result);
        } else if process_exited_before_socket {
            let output = child.wait_with_output()?;
            let _ = fs::remove_file(&socket_path);
            process_exited_before_socket_count += 1;
            scenario_results.push(json!({
                "scenarioId": scenario_id,
                "title": scenario_plan["title"],
                "status": "BLOCKED",
                "launchActionCount": 1,
                "executedCommandCount": 0,
                "blockerCount": 1,
                "blockers": [
                    "native app process exited before opening the QA socket; on this host the app may be a non-macOS headless stub"
                ],
                "appProcess": {
                    "appBinary": app_binary.to_string_lossy(),
                    "dbCopyPath": db_path.to_string_lossy(),
                    "socketPath": socket_path.to_string_lossy(),
                    "qaSocketReady": false,
                    "processExitedBeforeSocket": true,
                    "exitStatus": output.status.code(),
                    "lastSocketError": last_socket_error,
                    "childEnvKeys": child_env_keys,
                    "stdout": output_text(&output.stdout),
                    "stderr": output_text(&output.stderr),
                }
            }));
        } else if child.try_wait()?.is_none() {
            let _ = child.kill();
            let output = child.wait_with_output()?;
            let _ = fs::remove_file(&socket_path);
            process_timeout_count += 1;
            scenario_results.push(json!({
                "scenarioId": scenario_id,
                "title": scenario_plan["title"],
                "status": "BLOCKED",
                "launchActionCount": 1,
                "executedCommandCount": 0,
                "blockerCount": 1,
                "blockers": [
                    "native app process did not open the QA socket before the replay launch timeout"
                ],
                "appProcess": {
                    "appBinary": app_binary.to_string_lossy(),
                    "dbCopyPath": db_path.to_string_lossy(),
                    "socketPath": socket_path.to_string_lossy(),
                    "qaSocketReady": false,
                    "processKilledAfterTimeout": true,
                    "lastSocketError": last_socket_error,
                    "childEnvKeys": child_env_keys,
                    "stdout": output_text(&output.stdout),
                    "stderr": output_text(&output.stderr),
                }
            }));
        }
    }

    let mut blockers = replay_execution_blockers(&totals)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if process_exited_before_socket_count > 0 {
        blockers.push(
            "one or more native app processes exited before opening the QA socket".to_owned(),
        );
    }
    if process_timeout_count > 0 {
        blockers.push(
            "one or more native app processes timed out before opening the QA socket".to_owned(),
        );
    }
    let qa_socket_ready_pass = qa_socket_ready_count == scenario_plans.len();
    let status = if !totals.transport_pass() {
        "FAIL"
    } else if !blockers.is_empty() || !qa_socket_ready_pass {
        "BLOCKED"
    } else {
        "PASS"
    };

    Ok(json!({
        "artifact": "native-app-replay-contract",
        "scenarioSubset": SCENARIO_IDS,
        "status": status,
        "mode": "externalNativeAppProcessPerScenario",
        "appBinary": app_binary.to_string_lossy(),
        "sourceReplayPlanArtifact": plan["artifact"],
        "scenarioFileCount": plan["scenarioFileCount"],
        "appLaunchAttemptCount": app_launch_attempt_count,
        "qaSocketReadyCount": qa_socket_ready_count,
        "qaSocketReadyPass": qa_socket_ready_pass,
        "processExitedBeforeSocketCount": process_exited_before_socket_count,
        "processTimeoutCount": process_timeout_count,
        "transportPass": totals.transport_pass(),
        "launchActionCount": totals.launch_action_count,
        "executedCommandCount": totals.executed_command_count,
        "transportErrorCount": totals.transport_error_count,
        "structuredErrorCount": totals.structured_error_count,
        "missingElementCount": totals.missing_element_count,
        "pendingDispatchCount": totals.pending_dispatch_count,
        "screenshotBlockedCount": totals.screenshot_blocked_count,
        "videoBlockedCount": totals.video_blocked_count,
        "blockerCount": totals.blocker_count() + process_exited_before_socket_count + process_timeout_count,
        "blockers": blockers,
        "scenarioResults": scenario_results,
    }))
}

#[cfg(not(all(debug_assertions, unix)))]
pub fn native_app_replay_report(
    repo_root: impl AsRef<Path>,
    _app_binary: Option<PathBuf>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let plan = qa_control_replay_plan_report(repo_root)?;

    Ok(json!({
        "artifact": "native-app-replay-contract",
        "status": "BLOCKED",
        "mode": "externalNativeAppProcessPerScenarioUnavailable",
        "qaSocketReadyPass": false,
        "sourceReplayPlanArtifact": plan["artifact"],
        "scenarioFileCount": plan["scenarioFileCount"],
        "blockers": [
            "native app replay requires a debug Unix build so the app-side QA socket is available"
        ],
    }))
}

#[cfg(all(debug_assertions, unix))]
pub fn qa_control_replay_smoke_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    execute_qa_control_replay_with_scenario_clients(repo_root)
}

#[cfg(not(all(debug_assertions, unix)))]
pub fn qa_control_replay_smoke_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let plan = qa_control_replay_plan_report(repo_root)?;

    Ok(json!({
        "artifact": "native-qa-control-replay-execution",
        "scenarioSubset": SCENARIO_IDS,
        "status": "BLOCKED",
        "mode": "inProcessDebugSocketUnavailable",
        "transportPass": false,
        "sourceReplayPlanArtifact": plan["artifact"],
        "scenarioFileCount": plan["scenarioFileCount"],
        "launchActionCount": plan["launchActionCount"],
        "executedCommandCount": 0,
        "blockers": [
            "QA replay smoke execution requires a debug Unix build so the app-side QA socket is available"
        ],
    }))
}

pub fn full_parity_report(
    repo_root: impl AsRef<Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let replay_execution_report = qa_control_replay_smoke_report(repo_root)?;
    let visual_report = native_visual_parity_report(repo_root, None)?;
    let performance_report = native_performance_report(repo_root, None, None, None)?;
    let slice_reports = vec![
        read_only_board_report(repo_root, Locale::En)?,
        read_only_board_report(repo_root, Locale::Ko)?,
        board_interaction_report(repo_root)?,
        task_detail_report(repo_root)?,
        git_diff_report(repo_root)?,
        notification_hooks_report(repo_root)?,
        settings_layout_remote_report(repo_root)?,
        scenario_control_protocol_report(repo_root)?,
        qa_control_replay_plan_report(repo_root)?,
        replay_execution_report.clone(),
        visual_report.clone(),
        performance_report.clone(),
    ];
    let covered_scenarios = slice_reports
        .iter()
        .filter_map(|report| report.get("scenarioSubset"))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let missing_scenarios = SCENARIO_IDS
        .iter()
        .copied()
        .filter(|scenario_id| !covered_scenarios.contains(scenario_id))
        .collect::<Vec<_>>();
    let slice_statuses = slice_reports
        .iter()
        .map(|report| {
            json!({
                "artifact": report["artifact"].as_str().unwrap_or("unknown"),
                "status": report["status"].as_str().unwrap_or("PASS"),
            })
        })
        .collect::<Vec<_>>();
    let full_status = full_parity_status(missing_scenarios.len(), &slice_reports);
    let replay_status = replay_execution_report["status"]
        .as_str()
        .unwrap_or("UNKNOWN");
    let replay_transport_pass = replay_execution_report["transportPass"]
        .as_bool()
        .unwrap_or(false);
    let functional_status = if !missing_scenarios.is_empty() {
        "FAIL"
    } else if replay_status == "PASS" && replay_transport_pass {
        "PASS"
    } else if replay_status == "FAIL" {
        "FAIL"
    } else {
        "BLOCKED"
    };
    let macos_runtime_gates = [
        "native GPUI screenshot/video parity for S01-S14 requires macOS",
        "native .app/DMG packaging requires Darwin, GPUI native-ui, and codesign tooling",
        "native GPUI cold start, idle memory, and terminal scroll FPS require macOS",
        "two consecutive PASS reports cannot be issued until macOS visual/package gates run",
    ];

    Ok(json!({
        "artifact": "phase-5-full-parity",
        "status": full_status,
        "headlessContractsPass": missing_scenarios.is_empty(),
        "scenarioCount": SCENARIO_IDS.len(),
        "coveredScenarioIds": covered_scenarios.iter().copied().collect::<Vec<_>>(),
        "missingScenarioIds": missing_scenarios,
        "sliceArtifacts": slice_reports.iter().map(|report| report["artifact"].as_str().unwrap_or("unknown")).collect::<Vec<_>>(),
        "sliceStatuses": slice_statuses,
        "functionalEvidence": {
            "status": functional_status,
            "scenarioCoverage": {
                "covered": covered_scenarios.len(),
                "total": SCENARIO_IDS.len(),
                "missingScenarioCount": missing_scenarios.len(),
            },
            "qaControlReplay": {
                "artifact": replay_execution_report["artifact"],
                "status": replay_execution_report["status"],
                "mode": replay_execution_report["mode"],
                "transportPass": replay_execution_report["transportPass"],
                "executedCommandCount": replay_execution_report["executedCommandCount"],
                "scenarioFileCount": replay_execution_report["scenarioFileCount"],
                "blockerCount": replay_execution_report["blockerCount"],
                "blockers": replay_execution_report["blockers"],
            },
        },
        "visualEvidence": {
            "status": visual_report["status"],
            "baselineManifest": visual_report["baselineManifest"],
            "nativeExpectedCaptures": visual_report["nativeExpectedCaptures"],
            "comparison": visual_report["comparison"],
            "blockers": visual_report["blockers"],
        },
        "performanceEvidence": {
            "status": performance_report["status"],
            "targetSummary": performance_report["targetSummary"],
            "electronBaseline": performance_report["electronBaseline"],
            "native": performance_report["native"],
            "blockers": performance_report["blockers"],
        },
        "macosRuntimeGates": macos_runtime_gates,
        "linuxEvidence": {
            "workspaceTests": "cargo test --workspace --quiet",
            "workspaceBuild": "cargo build --workspace --quiet",
            "releaseBuild": "cargo build --release --quiet",
            "bundleScriptLinuxExit": 78,
        },
    }))
}

fn full_parity_status(missing_scenario_count: usize, slice_reports: &[Value]) -> &'static str {
    if missing_scenario_count > 0
        || slice_reports
            .iter()
            .any(|report| report["status"].as_str() == Some("FAIL"))
    {
        return "FAIL";
    }

    if slice_reports
        .iter()
        .any(|report| report["status"].as_str() == Some("BLOCKED"))
    {
        return "BLOCKED";
    }

    "PASS"
}

pub fn native_visual_parity_report(
    repo_root: impl AsRef<Path>,
    artifact_root: Option<PathBuf>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let artifact_root = artifact_root.map(|path| resolve_repo_path(repo_root, path));
    let scenarios = read_scenario_files(repo_root)?;
    let manifest_path = repo_root.join(BASELINE_MANIFEST_FROM_REPO_ROOT);
    let manifest_exists = manifest_path.is_file();
    let manifest_rows = if manifest_exists {
        parse_baseline_manifest_rows(&fs::read_to_string(&manifest_path)?)
    } else {
        Vec::new()
    };
    let manifest_by_scenario = manifest_rows
        .iter()
        .map(|row| (row.scenario_id.clone(), row.clone()))
        .collect::<BTreeMap<_, _>>();
    let manifest_scenario_ids = manifest_by_scenario
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let missing_manifest_scenario_ids = SCENARIO_IDS
        .iter()
        .copied()
        .filter(|scenario_id| !manifest_scenario_ids.contains(*scenario_id))
        .collect::<Vec<_>>();
    let extra_manifest_scenario_ids = manifest_scenario_ids
        .iter()
        .copied()
        .filter(|scenario_id| !SCENARIO_IDS.contains(scenario_id))
        .collect::<Vec<_>>();

    let mut baseline_missing_screens = Vec::new();
    let mut baseline_missing_videos = Vec::new();
    for row in &manifest_rows {
        if !repo_root.join("qa/baseline").join(&row.screen).is_file() {
            baseline_missing_screens.push(json!({
                "scenarioId": row.scenario_id,
                "path": format!("qa/baseline/{}", row.screen),
            }));
        }
        if !repo_root.join("qa/baseline").join(&row.video).is_file() {
            baseline_missing_videos.push(json!({
                "scenarioId": row.scenario_id,
                "path": format!("qa/baseline/{}", row.video),
            }));
        }
    }

    let mut scenario_mappings = Vec::new();
    let mut native_missing_screens = Vec::new();
    let mut native_missing_videos = Vec::new();
    let mut scenario_manifest_screen_mismatches = Vec::new();
    let mut scenario_manifest_video_mismatches = Vec::new();
    let mut scenario_declared_screen_count = 0usize;
    let mut scenario_declared_video_count = 0usize;
    let mut native_expected_screen_count = 0usize;
    let mut native_expected_video_count = 0usize;

    for scenario in &scenarios {
        let scenario_id = scenario["id"].as_str().ok_or("scenario missing id")?;
        let declared_screens = json_string_array(&scenario["artifacts"]["screens"]);
        let declared_videos = json_string_array(&scenario["artifacts"]["videos"]);
        scenario_declared_screen_count += declared_screens.len();
        scenario_declared_video_count += declared_videos.len();

        let native_screens = scenario["steps"]
            .as_array()
            .ok_or("scenario missing steps")?
            .iter()
            .filter(|step| step["action"] == "captureScreen")
            .map(|step| {
                scenario_native_screen_relative_path(scenario_id, step, artifact_root.as_deref())
            })
            .collect::<Vec<_>>();
        let native_video = scenario_native_video_relative_path_for(
            scenario_id,
            scenario,
            artifact_root.as_deref(),
        );
        native_expected_screen_count += native_screens.len();
        native_expected_video_count += 1;

        let native_screen_reports = native_screens
            .iter()
            .map(|path| {
                let exists = resolve_repo_path(repo_root, PathBuf::from(path)).is_file();
                if !exists {
                    native_missing_screens.push(json!({
                        "scenarioId": scenario_id,
                        "path": path,
                    }));
                }
                json!({
                    "path": path,
                    "exists": exists,
                })
            })
            .collect::<Vec<_>>();
        let native_video_exists =
            resolve_repo_path(repo_root, PathBuf::from(&native_video)).is_file();
        if !native_video_exists {
            native_missing_videos.push(json!({
                "scenarioId": scenario_id,
                "path": native_video,
            }));
        }

        let manifest = manifest_by_scenario.get(scenario_id);
        let manifest_screen = manifest.map(|row| row.screen.clone());
        let manifest_video = manifest.map(|row| row.video.clone());
        if let Some(screen) = manifest_screen.as_ref()
            && !declared_screens.is_empty()
            && !declared_screens.contains(screen)
        {
            scenario_manifest_screen_mismatches.push(json!({
                "scenarioId": scenario_id,
                "manifestScreen": screen,
                "scenarioDeclaredScreens": declared_screens,
            }));
        }
        if let Some(video) = manifest_video.as_ref()
            && !declared_videos.is_empty()
            && !declared_videos.contains(video)
        {
            scenario_manifest_video_mismatches.push(json!({
                "scenarioId": scenario_id,
                "manifestVideo": video,
                "scenarioDeclaredVideos": declared_videos,
            }));
        }

        scenario_mappings.push(json!({
            "scenarioId": scenario_id,
            "title": scenario["title"],
            "baselineManifest": manifest.map(|row| json!({
                "source": &row.source,
                "screen": format!("qa/baseline/{}", row.screen),
                "screenExists": repo_root.join("qa/baseline").join(&row.screen).is_file(),
                "video": format!("qa/baseline/{}", row.video),
                "videoExists": repo_root.join("qa/baseline").join(&row.video).is_file(),
                "notes": &row.notes,
            })),
            "scenarioDeclaredArtifacts": {
                "screens": declared_screens
                    .iter()
                    .map(|path| json!({
                        "path": format!("qa/baseline/{path}"),
                        "exists": repo_root.join("qa/baseline").join(path).is_file(),
                    }))
                    .collect::<Vec<_>>(),
                "videos": declared_videos
                    .iter()
                    .map(|path| json!({
                        "path": format!("qa/baseline/{path}"),
                        "exists": repo_root.join("qa/baseline").join(path).is_file(),
                    }))
                    .collect::<Vec<_>>(),
            },
            "nativeExpected": {
                "screens": native_screen_reports,
                "videos": [{
                    "path": native_video,
                    "exists": native_video_exists,
                }],
            },
        }));
    }

    let baseline_manifest_pass = manifest_exists
        && missing_manifest_scenario_ids.is_empty()
        && extra_manifest_scenario_ids.is_empty()
        && baseline_missing_screens.is_empty()
        && baseline_missing_videos.is_empty();
    let native_capture_ready =
        native_missing_screens.is_empty() && native_missing_videos.is_empty();
    let comparison_performed = false;
    let status = if baseline_manifest_pass {
        "BLOCKED"
    } else {
        "FAIL"
    };
    let mut blockers = Vec::new();
    if !manifest_exists {
        blockers.push(format!(
            "baseline manifest is missing: {BASELINE_MANIFEST_FROM_REPO_ROOT}"
        ));
    }
    if !missing_manifest_scenario_ids.is_empty() {
        blockers.push(format!(
            "baseline manifest is missing {} scenario rows",
            missing_manifest_scenario_ids.len()
        ));
    }
    if !extra_manifest_scenario_ids.is_empty() {
        blockers.push(format!(
            "baseline manifest has {} unexpected scenario rows",
            extra_manifest_scenario_ids.len()
        ));
    }
    if !baseline_missing_screens.is_empty() || !baseline_missing_videos.is_empty() {
        blockers.push("one or more Electron baseline screen/video files are missing".to_owned());
    }
    if !native_capture_ready {
        blockers.push(format!(
            "native visual review is blocked until {} screenshots and {} videos are captured",
            native_missing_screens.len(),
            native_missing_videos.len()
        ));
    }
    if native_capture_ready && !comparison_performed {
        blockers.push(
            "native captures exist, but structural visual comparison has not been recorded"
                .to_owned(),
        );
    }

    Ok(json!({
        "artifact": "native-visual-parity-evidence",
        "status": status,
        "scenarioSubset": SCENARIO_IDS,
        "baselineManifest": {
            "path": BASELINE_MANIFEST_FROM_REPO_ROOT,
            "exists": manifest_exists,
            "rowCount": manifest_rows.len(),
            "expectedScenarioCount": SCENARIO_IDS.len(),
            "missingScenarioIds": missing_manifest_scenario_ids,
            "extraScenarioIds": extra_manifest_scenario_ids,
            "missingScreenCount": baseline_missing_screens.len(),
            "missingVideoCount": baseline_missing_videos.len(),
            "missingScreens": baseline_missing_screens,
            "missingVideos": baseline_missing_videos,
            "pass": baseline_manifest_pass,
        },
        "scenarioDeclarations": {
            "scenarioFileCount": scenarios.len(),
            "declaredScreenCount": scenario_declared_screen_count,
            "declaredVideoCount": scenario_declared_video_count,
            "manifestScreenMismatchCount": scenario_manifest_screen_mismatches.len(),
            "manifestVideoMismatchCount": scenario_manifest_video_mismatches.len(),
            "manifestScreenMismatches": scenario_manifest_screen_mismatches,
            "manifestVideoMismatches": scenario_manifest_video_mismatches,
        },
        "nativeExpectedCaptures": {
            "artifactRoot": artifact_root.as_ref().map(|path| path.to_string_lossy().to_string()),
            "screenDir": native_screens_dir_for_artifact_root(artifact_root.as_deref()),
            "videoDir": native_videos_dir_for_artifact_root(artifact_root.as_deref()),
            "expectedScreenCount": native_expected_screen_count,
            "expectedVideoCount": native_expected_video_count,
            "missingScreenCount": native_missing_screens.len(),
            "missingVideoCount": native_missing_videos.len(),
            "missingScreens": native_missing_screens,
            "missingVideos": native_missing_videos,
            "captureReady": native_capture_ready,
        },
        "comparison": {
            "method": "manual-structural-side-by-side",
            "performed": comparison_performed,
            "ready": baseline_manifest_pass && native_capture_ready,
            "criteria": [
                "layout structure",
                "component presence",
                "text parity",
                "brand/status color token parity",
                "spacing hierarchy",
            ],
            "note": "This artifact inventories baseline/native captures and refuses visual PASS until the native files exist and a structural review is recorded.",
        },
        "scenarioMappings": scenario_mappings,
        "blockers": blockers,
    }))
}

pub fn native_performance_report(
    repo_root: impl AsRef<Path>,
    app_bundle: Option<PathBuf>,
    dmg_path: Option<PathBuf>,
    release_binary: Option<PathBuf>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let repo_root = repo_root.as_ref();
    let app_bundle = resolve_repo_path(
        repo_root,
        app_bundle.unwrap_or_else(|| PathBuf::from("native/dist/KanVibe.app")),
    );
    let dmg_path = resolve_repo_path(
        repo_root,
        dmg_path.unwrap_or_else(|| PathBuf::from("native/dist/KanVibe-0.1.0.dmg")),
    );
    let release_binary = resolve_repo_path(
        repo_root,
        release_binary.unwrap_or_else(|| PathBuf::from("native/target/release/kanvibe-app")),
    );

    let app_bundle_report = size_target_report(
        "appBundle",
        &app_bundle,
        Some(NATIVE_APP_BUNDLE_TARGET_BYTES),
    )?;
    let dmg_report = size_target_report("dmg", &dmg_path, None)?;
    let release_binary_report = size_target_report("releaseBinary", &release_binary, None)?;
    let macos_measurements = json!({
        "coldStartToWindow": {
            "status": "BLOCKED",
            "targetMs": 500,
            "reason": "requires launching the macOS GPUI app and observing the first native window"
        },
        "idleMemory": {
            "status": "BLOCKED",
            "target": "Electron idle RSS / 4",
            "reason": "requires macOS process-tree sampling after the native board is ready"
        },
        "terminalScrollFps": {
            "status": "BLOCKED",
            "targetFps": 60,
            "reason": "requires macOS GPUI terminal scenario replay and frame-rate sampling"
        }
    });
    let mut blockers = Vec::new();

    append_size_blockers(&mut blockers, &app_bundle_report);
    append_size_blockers(&mut blockers, &dmg_report);
    append_size_blockers(&mut blockers, &release_binary_report);
    blockers.push("native GPUI cold start requires macOS runtime measurement".to_owned());
    blockers.push("native GPUI idle memory requires macOS runtime measurement".to_owned());
    blockers.push("native terminal scroll FPS requires macOS runtime measurement".to_owned());

    let status = if app_bundle_report["status"] == "FAIL"
        || dmg_report["status"] == "FAIL"
        || release_binary_report["status"] == "FAIL"
    {
        "FAIL"
    } else {
        "BLOCKED"
    };

    Ok(json!({
        "artifact": "native-performance-comparison",
        "status": status,
        "targetSummary": {
            "appBundleMaxBytes": NATIVE_APP_BUNDLE_TARGET_BYTES,
            "appBundleMaxMb": bytes_to_mib(NATIVE_APP_BUNDLE_TARGET_BYTES),
            "coldStartMaxMs": 500,
            "idleMemory": "Electron idle RSS / 4",
            "terminalScrollFps": "60fps+"
        },
        "electronBaseline": electron_perf_baseline_summary(repo_root),
        "native": {
            "appBundle": app_bundle_report,
            "dmg": dmg_report,
            "releaseBinary": release_binary_report,
            "macosMeasurements": macos_measurements,
        },
        "blockers": blockers,
    }))
}

fn read_scenario_files(repo_root: &Path) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
    let mut scenario_paths = fs::read_dir(repo_root.join(SCENARIO_DIR_FROM_REPO_ROOT))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    scenario_paths.retain(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('S') && name.ends_with(".json"))
    });
    scenario_paths.sort();

    scenario_paths
        .into_iter()
        .map(|path| {
            let content = fs::read_to_string(path)?;
            Ok(serde_json::from_str::<Value>(&content)?)
        })
        .collect::<Result<Vec<_>, Box<dyn Error + Send + Sync>>>()
}

fn parse_baseline_manifest_rows(content: &str) -> Vec<BaselineManifestRow> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with('|') || line.contains("---") || !line.contains('`') {
                return None;
            }

            let cells = line
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>();
            if cells.len() < 5 {
                return None;
            }

            let scenario_label = markdown_cell_value(cells[0]);
            if !scenario_label.starts_with('S') {
                return None;
            }
            let source = markdown_cell_value(cells[1]);
            let scenario_id = Path::new(&source)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| stem.starts_with(&scenario_label))
                .map(str::to_owned)
                .unwrap_or(scenario_label);

            Some(BaselineManifestRow {
                scenario_id,
                source,
                screen: markdown_cell_value(cells[2]),
                video: markdown_cell_value(cells[3]),
                notes: markdown_cell_value(cells[4]),
            })
        })
        .collect()
}

fn markdown_cell_value(cell: &str) -> String {
    let cell = cell.trim();
    if let Some(start) = cell.find('`')
        && let Some(end) = cell[start + 1..].find('`')
    {
        return cell[start + 1..start + 1 + end].to_owned();
    }

    cell.to_owned()
}

fn json_string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn resolve_repo_path(repo_root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    }
}

fn size_target_report(
    kind: &str,
    path: &Path,
    max_bytes: Option<u64>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    if !path.exists() {
        return Ok(json!({
            "kind": kind,
            "status": "BLOCKED",
            "path": path.to_string_lossy(),
            "exists": false,
            "reason": "artifact has not been generated on this host",
        }));
    }

    let size_bytes = path_size_bytes(path)?;
    let status = match max_bytes {
        Some(max_bytes) if size_bytes > max_bytes => "FAIL",
        _ => "PASS",
    };

    Ok(json!({
        "kind": kind,
        "status": status,
        "path": path.to_string_lossy(),
        "exists": true,
        "sizeBytes": size_bytes,
        "sizeMiB": bytes_to_mib(size_bytes),
        "maxBytes": max_bytes,
        "maxMiB": max_bytes.map(bytes_to_mib),
    }))
}

fn append_size_blockers(blockers: &mut Vec<String>, report: &Value) {
    match report["status"].as_str() {
        Some("BLOCKED") => blockers.push(format!(
            "{} is not available: {}",
            report["kind"].as_str().unwrap_or("artifact"),
            report["reason"].as_str().unwrap_or("missing artifact")
        )),
        Some("FAIL") => blockers.push(format!(
            "{} exceeds target size: {} bytes > {} bytes",
            report["kind"].as_str().unwrap_or("artifact"),
            report["sizeBytes"].as_u64().unwrap_or_default(),
            report["maxBytes"].as_u64().unwrap_or_default()
        )),
        _ => {}
    }
}

fn path_size_bytes(path: &Path) -> Result<u64, Box<dyn Error + Send + Sync>> {
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }

    let mut size = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        size += path_size_bytes(&entry.path())?;
    }

    Ok(size)
}

fn bytes_to_mib(bytes: u64) -> f64 {
    ((bytes as f64 / 1024.0 / 1024.0) * 100.0).round() / 100.0
}

fn electron_perf_baseline_summary(repo_root: &Path) -> Value {
    let path = repo_root.join("qa/PERF_BASELINE.md");
    let Ok(content) = fs::read_to_string(&path) else {
        return json!({
            "path": path.to_string_lossy(),
            "exists": false,
        });
    };

    json!({
        "path": path.to_string_lossy(),
        "exists": true,
        "linuxPackagedDirectoryBytes": markdown_row_first_number(&content, "dist/linux-unpacked"),
        "linuxFirstPageObservedMs": markdown_row_first_number(&content, "First page observed"),
        "linuxBoardReadyMs": markdown_row_first_number(&content, "Board ready"),
        "linuxBoardReadyProcessCount": markdown_row_number_at(&content, "After board ready", 2),
        "linuxBoardReadyRssKb": markdown_row_number_at(&content, "After board ready", 3),
        "linuxTaskOperationsProcessCount": markdown_row_number_at(&content, "After task operations", 2),
        "linuxTaskOperationsRssKb": markdown_row_number_at(&content, "After task operations", 3),
    })
}

fn markdown_row_first_number(content: &str, label: &str) -> Option<f64> {
    markdown_row_number_at(content, label, 2)
}

fn markdown_row_number_at(content: &str, label: &str, cell_index: usize) -> Option<f64> {
    content
        .lines()
        .find(|line| line.starts_with('|') && line.contains(label))
        .and_then(|line| line.split('|').nth(cell_index))
        .and_then(first_number)
}

fn first_number(value: &str) -> Option<f64> {
    let number = value
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(|character| {
            character.is_ascii_digit() || *character == ',' || *character == '.'
        })
        .filter(|character| *character != ',')
        .collect::<String>();

    if number.is_empty() {
        None
    } else {
        number.parse().ok()
    }
}

#[derive(Default)]
struct ReplayExecutionCounters {
    launch_action_count: usize,
    executed_command_count: usize,
    transport_error_count: usize,
    structured_error_count: usize,
    missing_element_count: usize,
    pending_dispatch_count: usize,
    screenshot_blocked_count: usize,
    video_blocked_count: usize,
    accepted_synthetic_input_count: usize,
    db_snapshot_count: usize,
    pong_count: usize,
}

impl ReplayExecutionCounters {
    fn blocker_count(&self) -> usize {
        self.missing_element_count
            + self.pending_dispatch_count
            + self.screenshot_blocked_count
            + self.video_blocked_count
    }

    fn transport_pass(&self) -> bool {
        self.transport_error_count == 0 && self.structured_error_count == 0
    }

    fn status(&self) -> &'static str {
        if !self.transport_pass() {
            "FAIL"
        } else if self.blocker_count() > 0 {
            "BLOCKED"
        } else {
            "PASS"
        }
    }
}

#[derive(Default)]
struct ScenarioExecutionCounters {
    launch_action_count: usize,
    executed_command_count: usize,
    transport_error_count: usize,
    structured_error_count: usize,
    missing_element_count: usize,
    pending_dispatch_count: usize,
    screenshot_blocked_count: usize,
    video_blocked_count: usize,
}

impl ScenarioExecutionCounters {
    fn blocker_count(&self) -> usize {
        self.missing_element_count
            + self.pending_dispatch_count
            + self.screenshot_blocked_count
            + self.video_blocked_count
    }

    fn status(&self) -> &'static str {
        if self.transport_error_count > 0 || self.structured_error_count > 0 {
            "FAIL"
        } else if self.blocker_count() > 0 {
            "BLOCKED"
        } else {
            "PASS"
        }
    }
}

struct ResponseAssessment {
    status: &'static str,
    blocker: Option<String>,
}

fn execute_qa_control_replay(
    repo_root: &Path,
    client: &QaControlClient,
    mode: &str,
    socket_path: Option<&Path>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let plan = qa_control_replay_plan_report(repo_root)?;
    let scenario_plans = plan["scenarioPlans"]
        .as_array()
        .ok_or("replay plan missing scenarioPlans")?;
    let mut totals = ReplayExecutionCounters::default();
    let mut scenario_results = Vec::new();

    for scenario_plan in scenario_plans {
        scenario_results.push(execute_qa_control_scenario_plan(
            scenario_plan,
            client,
            &mut totals,
        )?);
    }

    Ok(replay_execution_report_json(
        &plan,
        &totals,
        scenario_results,
        mode,
        socket_path,
        None,
    ))
}

#[cfg(all(debug_assertions, unix))]
fn execute_qa_control_replay_with_scenario_clients(
    repo_root: &Path,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let plan = qa_control_replay_plan_report(repo_root)?;
    let scenario_plans = plan["scenarioPlans"]
        .as_array()
        .ok_or("replay plan missing scenarioPlans")?;
    let mut totals = ReplayExecutionCounters::default();
    let mut scenario_results = Vec::new();
    let mut scenario_socket_count = 0usize;

    for scenario_plan in scenario_plans {
        let scenario_id = scenario_plan["scenarioId"]
            .as_str()
            .ok_or("scenario plan missing scenarioId")?;
        let (client, socket_path) = spawn_in_process_qa_client(repo_root, scenario_id)?;
        scenario_socket_count += 1;
        let scenario_result = execute_qa_control_scenario_plan(scenario_plan, &client, &mut totals);
        let _ = fs::remove_file(&socket_path);
        scenario_results.push(scenario_result?);
    }

    Ok(replay_execution_report_json(
        &plan,
        &totals,
        scenario_results,
        "inProcessDebugSocketPerScenario",
        None,
        Some(scenario_socket_count),
    ))
}

fn execute_qa_control_scenario_plan(
    scenario_plan: &Value,
    client: &QaControlClient,
    totals: &mut ReplayExecutionCounters,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let scenario_id = scenario_plan["scenarioId"]
        .as_str()
        .ok_or("scenario plan missing scenarioId")?;
    let mut scenario_counters = ScenarioExecutionCounters::default();
    let mut command_results = Vec::new();

    for item in scenario_plan["replayItems"]
        .as_array()
        .ok_or("scenario plan missing replayItems")?
    {
        match item["kind"].as_str().unwrap_or("unknown") {
            "launchApp" => {
                totals.launch_action_count += 1;
                scenario_counters.launch_action_count += 1;
                command_results.push(json!({
                    "kind": "launchApp",
                    "status": "prepared",
                    "env": item["env"],
                    "theme": item["theme"],
                    "viewport": item["viewport"],
                }));
            }
            "controlCommand" => {
                let command = serde_json::from_value::<QaControlCommand>(item["command"].clone())?;
                totals.executed_command_count += 1;
                scenario_counters.executed_command_count += 1;

                match request_qa_item(client, &command, item) {
                    Ok((response, assessment)) => {
                        apply_response_counters(
                            &response,
                            &assessment,
                            totals,
                            &mut scenario_counters,
                        );
                        command_results.push(json!({
                            "kind": "controlCommand",
                            "sourceKind": item["sourceKind"],
                            "sourceType": item["sourceType"],
                            "commandType": item["commandType"],
                            "command": item["command"],
                            "transport": "ok",
                            "behaviorStatus": assessment.status,
                            "blocker": assessment.blocker,
                            "response": serde_json::to_value(response)?,
                        }));
                    }
                    Err(error) => {
                        totals.transport_error_count += 1;
                        scenario_counters.transport_error_count += 1;
                        command_results.push(json!({
                            "kind": "controlCommand",
                            "sourceKind": item["sourceKind"],
                            "sourceType": item["sourceType"],
                            "commandType": item["commandType"],
                            "command": item["command"],
                            "transport": "error",
                            "behaviorStatus": "fail",
                            "error": error.to_string(),
                        }));
                    }
                }
            }
            other => {
                totals.transport_error_count += 1;
                scenario_counters.transport_error_count += 1;
                command_results.push(json!({
                    "kind": other,
                    "transport": "error",
                    "behaviorStatus": "fail",
                    "error": "unknown replay item kind",
                }));
            }
        }
    }

    Ok(json!({
        "scenarioId": scenario_id,
        "title": scenario_plan["title"],
        "status": scenario_counters.status(),
        "launchActionCount": scenario_counters.launch_action_count,
        "executedCommandCount": scenario_counters.executed_command_count,
        "transportErrorCount": scenario_counters.transport_error_count,
        "structuredErrorCount": scenario_counters.structured_error_count,
        "missingElementCount": scenario_counters.missing_element_count,
        "pendingDispatchCount": scenario_counters.pending_dispatch_count,
        "screenshotBlockedCount": scenario_counters.screenshot_blocked_count,
        "videoBlockedCount": scenario_counters.video_blocked_count,
        "blockerCount": scenario_counters.blocker_count(),
        "commandResults": command_results,
    }))
}

fn request_qa_item(
    client: &QaControlClient,
    command: &QaControlCommand,
    item: &Value,
) -> Result<(QaControlResponse, ResponseAssessment), Box<dyn Error + Send + Sync>> {
    let eventually = item["source"]["eventually"].as_bool() == Some(true);
    let deadline = Instant::now() + QA_EVENTUAL_ASSERTION_TIMEOUT;
    loop {
        let response = client.request(command.clone())?;
        let assessment = assess_control_response_for_item(command, &response, item);
        if !eventually || assessment.status == "pass" || Instant::now() >= deadline {
            return Ok((response, assessment));
        }
        std::thread::sleep(QA_EVENTUAL_ASSERTION_POLL_INTERVAL);
    }
}

fn replay_execution_report_json(
    plan: &Value,
    totals: &ReplayExecutionCounters,
    scenario_results: Vec<Value>,
    mode: &str,
    socket_path: Option<&Path>,
    scenario_socket_count: Option<usize>,
) -> Value {
    json!({
        "artifact": "native-qa-control-replay-execution",
        "scenarioSubset": SCENARIO_IDS,
        "status": totals.status(),
        "mode": mode,
        "socketPath": socket_path.map(|path| path.to_string_lossy().to_string()),
        "scenarioSocketCount": scenario_socket_count,
        "transportPass": totals.transport_pass(),
        "sourceReplayPlanArtifact": plan["artifact"],
        "scenarioFileCount": plan["scenarioFileCount"],
        "launchActionCount": totals.launch_action_count,
        "executedCommandCount": totals.executed_command_count,
        "transportErrorCount": totals.transport_error_count,
        "structuredErrorCount": totals.structured_error_count,
        "missingElementCount": totals.missing_element_count,
        "pendingDispatchCount": totals.pending_dispatch_count,
        "screenshotBlockedCount": totals.screenshot_blocked_count,
        "videoBlockedCount": totals.video_blocked_count,
        "acceptedSyntheticInputCount": totals.accepted_synthetic_input_count,
        "dbSnapshotCount": totals.db_snapshot_count,
        "pongCount": totals.pong_count,
        "blockerCount": totals.blocker_count(),
        "blockers": replay_execution_blockers(totals),
        "scenarioResults": scenario_results,
    })
}

#[cfg(all(debug_assertions, unix))]
fn spawn_in_process_qa_client(
    repo_root: &Path,
    scenario_id: &str,
) -> Result<(QaControlClient, PathBuf), Box<dyn Error + Send + Sync>> {
    let socket_path = unique_socket_path(&format!("qa-replay-smoke-{scenario_id}"))?;
    let seed = prepare_scenario_seed_copy(repo_root, scenario_id)?;
    let bootstrap = kanvibe_app::load_read_only_board(repo_root, seed, Locale::Ko)?;
    let spec = kanvibe_app::build_native_ui_render_spec(&bootstrap);
    let _socket_thread =
        kanvibe_app::qa_control::spawn_debug_qa_socket_at_path(socket_path.clone(), spec)?
            .ok_or("debug QA socket is disabled in this build")?;
    let client = QaControlClient::new(socket_path.clone());
    wait_for_qa_socket(&client)?;

    Ok((client, socket_path))
}

fn assess_control_response(
    _command: &QaControlCommand,
    response: &QaControlResponse,
) -> ResponseAssessment {
    match response {
        QaControlResponse::Pong => ResponseAssessment {
            status: "pass",
            blocker: None,
        },
        QaControlResponse::Element { exists: true, .. } => ResponseAssessment {
            status: "pass",
            blocker: None,
        },
        QaControlResponse::Element {
            id, exists: false, ..
        } => ResponseAssessment {
            status: "blocked",
            blocker: Some(format!(
                "element `{id}` is not available in the current native QA render state"
            )),
        },
        QaControlResponse::SyntheticInput {
            accepted: true,
            dispatch_status,
        } if dispatch_status == "protocol-ready-gpui-dispatch-pending" => ResponseAssessment {
            status: "blocked",
            blocker: Some(dispatch_status.clone()),
        },
        QaControlResponse::SyntheticInput {
            accepted: true,
            dispatch_status: _,
        } => ResponseAssessment {
            status: "pass",
            blocker: None,
        },
        QaControlResponse::SyntheticInput {
            accepted: false,
            dispatch_status,
        } => ResponseAssessment {
            status: "fail",
            blocker: Some(dispatch_status.clone()),
        },
        QaControlResponse::Screenshot { captured: true, .. } => ResponseAssessment {
            status: "pass",
            blocker: None,
        },
        QaControlResponse::Screenshot {
            captured: false,
            reason,
            ..
        } => ResponseAssessment {
            status: "blocked",
            blocker: Some(reason.clone()),
        },
        QaControlResponse::VideoCapture {
            active: true,
            captured: false,
            ..
        }
        | QaControlResponse::VideoCapture { captured: true, .. } => ResponseAssessment {
            status: "pass",
            blocker: None,
        },
        QaControlResponse::VideoCapture {
            active: false,
            captured: false,
            reason,
            ..
        } => ResponseAssessment {
            status: "blocked",
            blocker: Some(reason.clone()),
        },
        QaControlResponse::DbSnapshot { .. } => ResponseAssessment {
            status: "pass",
            blocker: None,
        },
        QaControlResponse::Error { message } => ResponseAssessment {
            status: "fail",
            blocker: Some(message.clone()),
        },
    }
}

fn assess_control_response_for_item(
    command: &QaControlCommand,
    response: &QaControlResponse,
    item: &Value,
) -> ResponseAssessment {
    if item["sourceKind"].as_str() == Some("assertion") {
        if matches!(command, QaControlCommand::DbSnapshot)
            && let QaControlResponse::DbSnapshot { .. } = response
        {
            return assess_db_assertion_response(&item["source"], response);
        }

        if matches!(
            command,
            QaControlCommand::QueryElement { .. } | QaControlCommand::QueryText { .. }
        ) && let QaControlResponse::Element { .. } = response
        {
            return assess_query_assertion_response(&item["source"], response);
        }
    }

    assess_control_response(command, response)
}

fn assess_query_assertion_response(
    assertion: &Value,
    response: &QaControlResponse,
) -> ResponseAssessment {
    let QaControlResponse::Element { id, exists, text } = response else {
        return ResponseAssessment {
            status: "fail",
            blocker: Some("expected element response".to_owned()),
        };
    };
    let assertion_type = assertion["type"].as_str().unwrap_or("unknown");

    if assertion_type == "taskNotVisible" {
        return if *exists {
            ResponseAssessment {
                status: "fail",
                blocker: Some(format!("taskNotVisible expected `{id}` to be absent")),
            }
        } else {
            ResponseAssessment {
                status: "pass",
                blocker: None,
            }
        };
    }

    if !*exists {
        return ResponseAssessment {
            status: "blocked",
            blocker: Some(format!(
                "assertion `{assertion_type}` expected `{id}` to be available"
            )),
        };
    }

    let failure = match assertion_type {
        "aiProviderFilters" => expected_array_values(assertion, "expected")
            .into_iter()
            .find(|expected| !text_contains(text, expected))
            .map(|expected| {
                format!(
                    "aiProviderFilters missing `{expected}` in `{}`",
                    text_value(text)
                )
            }),
        "dockItems" => expected_array_values(assertion, "expected")
            .into_iter()
            .find(|expected| !text_contains(text, expected))
            .map(|expected| format!("dockItems missing `{expected}` in `{}`", text_value(text))),
        "hookProvidersReady" => expected_array_values(assertion, "expected")
            .into_iter()
            .find(|expected| !text_contains(text, expected))
            .map(|expected| {
                format!(
                    "hookProvidersReady missing `{expected}` in `{}`",
                    text_value(text)
                )
            }),
        "route" => assertion["pattern"].as_str().and_then(|expected| {
            (!text_contains(text, expected)).then(|| {
                format!(
                    "route expected text containing `{expected}`, got `{}`",
                    text_value(text)
                )
            })
        }),
        "sessionDependencyState" => assertion["expected"].as_str().and_then(|expected| {
            (text.as_deref() != Some(expected)).then(|| {
                format!(
                    "sessionDependencyState expected `{expected}`, got `{}`",
                    text_value(text)
                )
            })
        }),
        "shortcutLabels" => assertion["expectedPrefix"].as_str().and_then(|expected| {
            (!text_contains(text, expected)).then(|| {
                format!(
                    "shortcutLabels expected text containing `{expected}`, got `{}`",
                    text_value(text)
                )
            })
        }),
        "taskField" => {
            if let Some(expected) = assertion["expected"].as_str() {
                (text.as_deref() != Some(expected)).then(|| {
                    format!(
                        "taskField expected `{expected}`, got `{}`",
                        text_value(text)
                    )
                })
            } else {
                assertion["expectedContains"].as_str().and_then(|expected| {
                    (!text_contains(text, expected)).then(|| {
                        format!(
                            "taskField expected text containing `{expected}`, got `{}`",
                            text_value(text)
                        )
                    })
                })
            }
        }
        "taskTitleVisible" => assertion["title"].as_str().and_then(|expected| {
            (!text_contains(text, expected)).then(|| {
                format!(
                    "{assertion_type} expected text containing `{expected}`, got `{}`",
                    text_value(text)
                )
            })
        }),
        "taskVisible" => assess_task_visible_text(assertion, text),
        _ => None,
    };

    if let Some(failure) = failure {
        ResponseAssessment {
            status: "fail",
            blocker: Some(failure),
        }
    } else {
        ResponseAssessment {
            status: "pass",
            blocker: None,
        }
    }
}

fn assess_task_visible_text(assertion: &Value, text: &Option<String>) -> Option<String> {
    if let Some(expected) = assertion["title"].as_str() {
        return (!text_contains(text, expected)).then(|| {
            format!(
                "taskVisible expected title `{expected}`, got `{}`",
                text_value(text)
            )
        });
    }

    assertion["branchName"].as_str().and_then(|expected| {
        (!text_contains(text, expected)).then(|| {
            format!(
                "taskVisible expected branch `{expected}`, got `{}`",
                text_value(text)
            )
        })
    })
}

fn expected_array_values<'a>(assertion: &'a Value, key: &str) -> Vec<&'a str> {
    assertion[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn text_contains(text: &Option<String>, expected: &str) -> bool {
    text.as_deref()
        .is_some_and(|actual| actual.contains(expected))
}

fn text_value(text: &Option<String>) -> &str {
    text.as_deref().unwrap_or("<missing>")
}

fn assess_db_assertion_response(
    assertion: &Value,
    response: &QaControlResponse,
) -> ResponseAssessment {
    let QaControlResponse::DbSnapshot {
        project_count,
        tasks,
        settings,
        pane_layouts,
        no_generic_env_leak,
        worktree_created_titles,
        ..
    } = response
    else {
        return ResponseAssessment {
            status: "fail",
            blocker: Some("expected dbSnapshot response".to_owned()),
        };
    };
    let assertion_type = assertion["type"].as_str().unwrap_or("unknown");
    let failure = match assertion_type {
        "dbCount" => {
            let table = assertion["table"].as_str().unwrap_or_default();
            let expected = assertion["expected"].as_u64().unwrap_or_default() as usize;
            let actual = match table {
                "projects" => *project_count,
                "kanban_tasks" => tasks.len(),
                other => {
                    return ResponseAssessment {
                        status: "fail",
                        blocker: Some(format!("unsupported dbCount table `{other}`")),
                    };
                }
            };

            (actual != expected)
                .then(|| format!("dbCount `{table}` expected {expected}, got {actual}"))
        }
        "taskCountByStatus" => {
            let expected = assertion["expected"].as_object();
            expected.and_then(|expected| {
                expected.iter().find_map(|(status, expected_value)| {
                    let expected_count = expected_value.as_u64().unwrap_or_default() as usize;
                    let actual_count = tasks.iter().filter(|task| task.status == *status).count();
                    (actual_count != expected_count).then(|| {
                        format!(
                            "taskCountByStatus `{status}` expected {expected_count}, got {actual_count}"
                        )
                    })
                })
            })
        }
        "dbRow" => assess_db_row(assertion, tasks, settings, pane_layouts),
        "dbMissing" => {
            let table = assertion["table"].as_str().unwrap_or_default();
            let found = match table {
                "kanban_tasks" => tasks
                    .iter()
                    .any(|task| row_matches_task(task, &assertion["where"])),
                other => {
                    return ResponseAssessment {
                        status: "fail",
                        blocker: Some(format!("unsupported dbMissing table `{other}`")),
                    };
                }
            };

            found.then(|| format!("dbMissing `{table}` unexpectedly found a row"))
        }
        "noWorktreeCreated" => {
            let title = assertion["title"].as_str().unwrap_or_default();
            worktree_created_titles
                .iter()
                .any(|candidate| candidate == title)
                .then(|| format!("unexpected worktree created for `{title}`"))
        }
        "noGenericEnvLeak" => (!*no_generic_env_leak)
            .then(|| "generic runtime environment keys leaked into native shell state".to_owned()),
        other => Some(format!("unsupported db assertion `{other}`")),
    };

    if let Some(failure) = failure {
        ResponseAssessment {
            status: "fail",
            blocker: Some(failure),
        }
    } else {
        ResponseAssessment {
            status: "pass",
            blocker: None,
        }
    }
}

fn assess_db_row(
    assertion: &Value,
    tasks: &[kanvibe_app::qa_control::QaTaskSnapshot],
    settings: &std::collections::BTreeMap<String, String>,
    pane_layouts: &[kanvibe_app::qa_control::QaPaneLayoutSnapshot],
) -> Option<String> {
    match assertion["table"].as_str().unwrap_or_default() {
        "kanban_tasks" => {
            let Some(task) = tasks
                .iter()
                .find(|task| row_matches_task(task, &assertion["where"]))
            else {
                return Some("dbRow `kanban_tasks` did not find a matching row".to_owned());
            };
            assertion["expected"].as_object().and_then(|expected| {
                expected.iter().find_map(|(field, expected_value)| {
                    let actual = task_field_value(task, field);
                    let expected = expected_value.as_str().unwrap_or_default();
                    (actual.as_deref() != Some(expected)).then(|| {
                        format!(
                            "dbRow `kanban_tasks.{field}` expected `{expected}`, got `{}`",
                            actual.unwrap_or_else(|| "<null>".to_owned())
                        )
                    })
                })
            })
        }
        "app_settings" => {
            let key = assertion["where"]["key"].as_str().unwrap_or_default();
            let actual = settings.get(key).map(String::as_str);
            let expected = assertion["expected"]["value"].as_str().unwrap_or_default();

            (actual != Some(expected)).then(|| {
                format!(
                    "dbRow `app_settings.{key}` expected `{expected}`, got `{}`",
                    actual.unwrap_or("<missing>")
                )
            })
        }
        "pane_layout_configs" => {
            let project_id = assertion["where"]["project_id"]
                .as_str()
                .unwrap_or_default();
            let actual = pane_layouts
                .iter()
                .find(|layout| layout.project_id.as_deref() == Some(project_id))
                .map(|layout| layout.layout_type.as_str());
            let expected = assertion["expected"]["layout_type"]
                .as_str()
                .unwrap_or_default();

            (actual != Some(expected)).then(|| {
                format!(
                    "dbRow `pane_layout_configs.{project_id}` expected `{expected}`, got `{}`",
                    actual.unwrap_or("<missing>")
                )
            })
        }
        other => Some(format!("unsupported dbRow table `{other}`")),
    }
}

fn row_matches_task(task: &kanvibe_app::qa_control::QaTaskSnapshot, where_clause: &Value) -> bool {
    let Some(where_clause) = where_clause.as_object() else {
        return false;
    };

    where_clause.iter().all(|(field, value)| {
        value
            .as_str()
            .is_some_and(|expected| task_field_value(task, field).as_deref() == Some(expected))
    })
}

fn task_field_value(task: &kanvibe_app::qa_control::QaTaskSnapshot, field: &str) -> Option<String> {
    match field {
        "id" => Some(task.id.clone()),
        "title" => Some(task.title.clone()),
        "status" => Some(task.status.clone()),
        "project_id" => task.project_id.clone(),
        "branch_name" => task.branch_name.clone(),
        "base_branch" => task.base_branch.clone(),
        "session_type" => task.session_type.clone(),
        "ssh_host" => task.ssh_host.clone(),
        "pr_url" => task.pr_url.clone(),
        "priority" => task.priority.clone(),
        "project_color" => task.project_color.clone(),
        _ => None,
    }
}

fn apply_response_counters(
    response: &QaControlResponse,
    assessment: &ResponseAssessment,
    totals: &mut ReplayExecutionCounters,
    scenario: &mut ScenarioExecutionCounters,
) {
    match response {
        QaControlResponse::Pong => totals.pong_count += 1,
        QaControlResponse::Element { exists: false, .. } if assessment.status != "pass" => {
            totals.missing_element_count += 1;
            scenario.missing_element_count += 1;
        }
        QaControlResponse::SyntheticInput {
            accepted: true,
            dispatch_status,
        } => {
            totals.accepted_synthetic_input_count += 1;
            if dispatch_status == "protocol-ready-gpui-dispatch-pending" {
                totals.pending_dispatch_count += 1;
                scenario.pending_dispatch_count += 1;
            }
        }
        QaControlResponse::Screenshot {
            captured: false, ..
        } => {
            totals.screenshot_blocked_count += 1;
            scenario.screenshot_blocked_count += 1;
        }
        QaControlResponse::VideoCapture {
            active: false,
            captured: false,
            ..
        } => {
            totals.video_blocked_count += 1;
            scenario.video_blocked_count += 1;
        }
        QaControlResponse::DbSnapshot { .. } => totals.db_snapshot_count += 1,
        QaControlResponse::Error { .. } => {
            totals.structured_error_count += 1;
            scenario.structured_error_count += 1;
        }
        QaControlResponse::Element { .. }
        | QaControlResponse::Screenshot { .. }
        | QaControlResponse::VideoCapture { .. } => {}
        QaControlResponse::SyntheticInput {
            accepted: false, ..
        } => {
            totals.structured_error_count += 1;
            scenario.structured_error_count += 1;
        }
    }

    if assessment.status == "fail"
        && !matches!(
            response,
            QaControlResponse::Error { .. }
                | QaControlResponse::SyntheticInput {
                    accepted: false,
                    ..
                }
        )
    {
        totals.structured_error_count += 1;
        scenario.structured_error_count += 1;
    }
}

fn replay_execution_blockers(counters: &ReplayExecutionCounters) -> Vec<&'static str> {
    let mut blockers = Vec::new();

    if counters.missing_element_count > 0 {
        blockers.push("some scenario elements are not exposed by the current native QA state");
    }
    if counters.pending_dispatch_count > 0 {
        blockers.push("synthetic GPUI event dispatch is still protocol-ready but not implemented");
    }
    if counters.screenshot_blocked_count > 0 {
        blockers.push("native screenshot capture requires macOS screencapture and a QA window id");
    }
    if counters.video_blocked_count > 0 {
        blockers
            .push("native video capture requires macOS screencapture, ffmpeg, and a QA window id");
    }
    if counters.transport_error_count > 0 {
        blockers.push("one or more QA control commands failed at the socket transport layer");
    }
    if counters.structured_error_count > 0 {
        blockers.push("one or more QA control commands returned structured errors");
    }

    blockers
}

#[cfg(all(debug_assertions, unix))]
fn wait_for_qa_socket(client: &QaControlClient) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut last_error = None;

    for _ in 0..50 {
        match client.request(QaControlCommand::Ping) {
            Ok(QaControlResponse::Pong) => return Ok(()),
            Ok(other) => {
                return Err(
                    format!("QA socket ping returned unexpected response: {other:?}").into(),
                );
            }
            Err(error) => {
                last_error = Some(error.to_string());
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }

    Err(format!(
        "QA socket did not become ready: {}",
        last_error.unwrap_or_else(|| "no response".to_owned())
    )
    .into())
}

#[cfg(unix)]
fn unique_socket_path(name: &str) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let short_name = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(24)
        .collect::<String>();
    Ok(Path::new("/tmp").join(format!(
        "kanvibe-{short_name}-{}-{}.sock",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    )))
}

fn scenario_native_video_relative_path(scenario_id: &str, scenario: &Value) -> String {
    scenario_native_video_relative_path_for(scenario_id, scenario, None)
}

fn scenario_native_video_relative_path_for(
    scenario_id: &str,
    scenario: &Value,
    artifact_root: Option<&Path>,
) -> String {
    let file_name = scenario["artifacts"]["videos"]
        .as_array()
        .and_then(|videos| videos.first())
        .and_then(Value::as_str)
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{scenario_id}.mp4"));

    format!(
        "{}/{file_name}",
        native_videos_dir_for_artifact_root(artifact_root)
    )
}

fn scenario_native_screen_relative_path(
    scenario_id: &str,
    step: &Value,
    artifact_root: Option<&Path>,
) -> String {
    format!(
        "{}/{scenario_id}-{}.png",
        native_screens_dir_for_artifact_root(artifact_root),
        step["name"].as_str().unwrap_or("screen")
    )
}

fn native_videos_dir() -> String {
    native_videos_dir_for_artifact_root(None)
}

fn native_screens_dir_for_artifact_root(artifact_root: Option<&Path>) -> String {
    native_artifact_dir_for_artifact_root(artifact_root, "native-screens", "screens")
}

fn native_videos_dir_for_artifact_root(artifact_root: Option<&Path>) -> String {
    native_artifact_dir_for_artifact_root(artifact_root, "native-videos", "videos")
}

fn native_artifact_dir_for_artifact_root(
    artifact_root: Option<&Path>,
    default_leaf: &str,
    run_leaf: &str,
) -> String {
    if let Some(root) = artifact_root {
        let root = root.to_string_lossy();
        if !root.trim().is_empty() {
            return format!("{}/{}", root.trim_end_matches('/'), run_leaf);
        }
    }

    native_artifact_dir_from_env(default_leaf, run_leaf)
}

fn native_artifact_dir_from_env(default_leaf: &str, run_leaf: &str) -> String {
    match std::env::var(KANVIBE_QA_ARTIFACT_ROOT_ENV) {
        Ok(root) if !root.trim().is_empty() => {
            format!("{}/{}", root.trim_end_matches('/'), run_leaf)
        }
        _ => format!("qa/parity/{default_leaf}"),
    }
}

fn video_capture_replay_item(scenario_id: &str, phase: &str, path: &str) -> Value {
    let command = match phase {
        "start" => QaControlCommand::StartVideoCapture {
            path: path.to_owned(),
        },
        "stop" => QaControlCommand::StopVideoCapture {
            path: path.to_owned(),
        },
        _ => unreachable!("video capture phase is controlled by the harness"),
    };

    json!({
        "kind": "controlCommand",
        "scenarioId": scenario_id,
        "sourceKind": "artifact",
        "sourceType": format!("videoCapture.{phase}"),
        "commandType": qa_command_type(&command),
        "command": serde_json::to_value(command).expect("QA control command should serialize"),
        "source": {
            "type": "video",
            "phase": phase,
            "nativePath": path,
        },
    })
}

fn replay_items_for_step(scenario_id: &str, step: &Value) -> Option<Vec<Value>> {
    let action = step["action"].as_str()?;

    match action {
        "launchApp" => Some(vec![json!({
            "kind": "launchApp",
            "scenarioId": scenario_id,
            "action": action,
            "sourceSeed": step["seed"].as_str().unwrap_or("qa/seed/kanvibe-seed.sqlite"),
            "dbCopyPath": scenario_seed_copy_relative_path(scenario_id),
            "env": {
                "KANVIBE_QA_SOCKET": "<per-run-debug-socket-path>",
                "KANVIBE_DB_PATH": scenario_seed_copy_relative_path(scenario_id),
                "KANVIBE_LOCALE": step["locale"].as_str().unwrap_or("ko"),
            },
            "theme": step["theme"].as_str().unwrap_or("dark"),
            "viewport": step["viewport"].clone(),
        })]),
        _ => qa_command_for_step(scenario_id, step).map(|command| {
            vec![control_replay_item(
                scenario_id,
                "step",
                action,
                step,
                command,
            )]
        }),
    }
}

fn replay_items_for_assertion(scenario_id: &str, assertion: &Value) -> Option<Vec<Value>> {
    let assertion_type = assertion["type"].as_str()?;

    match assertion_type {
        "columns" => {
            let statuses = assertion["statuses"].as_array()?;
            Some(
                statuses
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|status| {
                        control_replay_item(
                            scenario_id,
                            "assertion",
                            assertion_type,
                            assertion,
                            QaControlCommand::QueryElement {
                                id: format!("column.{status}"),
                            },
                        )
                    })
                    .collect(),
            )
        }
        _ => qa_command_for_assertion(assertion).map(|command| {
            vec![control_replay_item(
                scenario_id,
                "assertion",
                assertion_type,
                assertion,
                command,
            )]
        }),
    }
}

fn control_replay_item(
    scenario_id: &str,
    source_kind: &str,
    source_type: &str,
    source: &Value,
    command: QaControlCommand,
) -> Value {
    json!({
        "kind": "controlCommand",
        "scenarioId": scenario_id,
        "sourceKind": source_kind,
        "sourceType": source_type,
        "commandType": qa_command_type(&command),
        "command": serde_json::to_value(command).expect("QA control command should serialize"),
        "source": source,
    })
}

fn qa_command_for_step(scenario_id: &str, step: &Value) -> Option<QaControlCommand> {
    let action = step["action"].as_str()?;

    match action {
        "waitForBoardReady" => Some(QaControlCommand::QueryElement {
            id: "app.root".to_owned(),
        }),
        "captureScreen" => Some(QaControlCommand::DumpScreenshot {
            path: scenario_native_screen_relative_path(scenario_id, step, None),
        }),
        "pressKeys" => Some(QaControlCommand::SyntheticKey {
            key: step["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("+"),
            modifiers: Vec::new(),
        }),
        "pressShortcut" => Some(QaControlCommand::SyntheticKey {
            key: step["command"].as_str().unwrap_or("shortcut").to_owned(),
            modifiers: vec!["cmd".to_owned()],
        }),
        "typeText" => Some(QaControlCommand::SyntheticKey {
            key: step["text"].as_str().unwrap_or_default().to_owned(),
            modifiers: Vec::new(),
        }),
        "recordWindowCount" => Some(QaControlCommand::QueryText {
            id: format!(
                "window.count.{}",
                step["name"].as_str().unwrap_or("baseline")
            ),
        }),
        "chooseContextAction"
        | "confirmDialog"
        | "dragTaskToColumn"
        | "fillBranchTaskForm"
        | "fillTaskForm"
        | "focusTask"
        | "openCreateTaskModal"
        | "openDiffRoute"
        | "openNotificationCenter"
        | "openPaneLayoutRoute"
        | "openProjectFilter"
        | "openSessionDependencyPanel"
        | "checkSessionDependency"
        | "checkTaskHooks"
        | "collapseTaskSidebar"
        | "dismissSidebarHint"
        | "installTaskHooks"
        | "recheckTaskHooks"
        | "installSessionDependency"
        | "retrySessionDependency"
        | "openSettings"
        | "openTaskContextMenu"
        | "openTaskDetail"
        | "selectDockItem"
        | "selectFirstChangedFile"
        | "selectPaneLayout"
        | "selectProjects"
        | "setAppSetting"
        | "setProjectColor"
        | "submitBranchTask"
        | "submitCreateTask" => Some(QaControlCommand::SyntheticClick {
            id: semantic_click_target(action, step),
            button: "left".to_owned(),
            payload: Some(step.clone()),
        }),
        _ => None,
    }
}

fn qa_command_for_assertion(assertion: &Value) -> Option<QaControlCommand> {
    let assertion_type = assertion["type"].as_str()?;

    match assertion_type {
        "dbCount" | "dbMissing" | "dbRow" | "noGenericEnvLeak" | "noWorktreeCreated"
        | "taskCountByStatus" => Some(QaControlCommand::DbSnapshot),
        "diffPaneVisible"
        | "diffSidebarVisible"
        | "dockItems"
        | "hookStatusVisible"
        | "notificationCenterVisible"
        | "onlyProjectTasksVisible"
        | "searchResultVisible"
        | "sessionDependencyVisible"
        | "shortcutLabels"
        | "taskNotVisible"
        | "taskTitleVisible"
        | "taskVisible"
        | "windowCountUnchanged" => Some(QaControlCommand::QueryElement {
            id: assertion_element_id(assertion_type, assertion),
        }),
        "aiProviderFilters"
        | "hookProvidersReady"
        | "route"
        | "sessionDependencyState"
        | "taskField" => Some(QaControlCommand::QueryText {
            id: assertion_text_id(assertion_type, assertion),
        }),
        "externalToolBlockerAllowed" => Some(QaControlCommand::QueryText {
            id: "protocol.blocker.externalTool".to_owned(),
        }),
        _ => None,
    }
}

fn semantic_click_target(action: &str, step: &Value) -> String {
    match action {
        "chooseContextAction" => format!(
            "context.action.{}",
            step["actionName"].as_str().unwrap_or("unknown")
        ),
        "confirmDialog" => "dialog.confirm".to_owned(),
        "dragTaskToColumn" => format!(
            "column.{}.dropTarget",
            step["toStatus"].as_str().unwrap_or("unknown")
        ),
        "fillBranchTaskForm" => "branchTask.form".to_owned(),
        "fillTaskForm" => "createTask.form".to_owned(),
        "focusTask" | "openTaskContextMenu" | "openTaskDetail" => {
            format!("task.{}", step["taskId"].as_str().unwrap_or("unknown"))
        }
        "openCreateTaskModal" => "board.primaryAction".to_owned(),
        "openDiffRoute" => format!("task.{}.diff", step["taskId"].as_str().unwrap_or("unknown")),
        "openNotificationCenter" => "notification.centerButton".to_owned(),
        "openPaneLayoutRoute" => "settings.paneLayout".to_owned(),
        "openProjectFilter" => "board.projectFilter".to_owned(),
        "openSessionDependencyPanel" => "sessionDependency.panelTrigger".to_owned(),
        "checkSessionDependency" => "sessionDependency.check".to_owned(),
        "checkTaskHooks" => "taskHooks.check".to_owned(),
        "collapseTaskSidebar" => "taskSidebar.collapse".to_owned(),
        "dismissSidebarHint" => "taskSidebar.dismissHint".to_owned(),
        "installTaskHooks" => "taskHooks.install".to_owned(),
        "recheckTaskHooks" => "taskHooks.recheck".to_owned(),
        "installSessionDependency" => "sessionDependency.install".to_owned(),
        "retrySessionDependency" => "sessionDependency.retry".to_owned(),
        "openSettings" => "app.settingsButton".to_owned(),
        "selectDockItem" => format!("dock.{}", step["item"].as_str().unwrap_or("unknown")),
        "selectFirstChangedFile" => "diff.fileList.firstChangedFile".to_owned(),
        "selectPaneLayout" => format!(
            "paneLayout.option.{}",
            step["layoutType"].as_str().unwrap_or("unknown")
        ),
        "selectProjects" => "projectFilter.selection".to_owned(),
        "setAppSetting" => format!("settings.{}", step["key"].as_str().unwrap_or("unknown")),
        "setProjectColor" => format!(
            "projectColor.{}",
            step["projectId"].as_str().unwrap_or("unknown")
        ),
        "submitBranchTask" => "branchTask.submit".to_owned(),
        "submitCreateTask" => "createTask.submit".to_owned(),
        _ => format!("action.{action}"),
    }
}

fn assertion_element_id(assertion_type: &str, assertion: &Value) -> String {
    match assertion_type {
        "diffPaneVisible" => "diff.pane".to_owned(),
        "diffSidebarVisible" => "diff.sidebar".to_owned(),
        "dockItems" | "shortcutLabels" => "dock.root".to_owned(),
        "hookStatusVisible" => format!(
            "hooks.status.{}",
            assertion["taskId"].as_str().unwrap_or("current")
        ),
        "notificationCenterVisible" => "notification.center".to_owned(),
        "onlyProjectTasksVisible" => format!(
            "projectFilter.{}",
            assertion["projectId"].as_str().unwrap_or("selected")
        ),
        "searchResultVisible" | "taskNotVisible" | "taskVisible" => assertion["taskId"]
            .as_str()
            .map(|task_id| format!("task.{task_id}"))
            .unwrap_or_else(|| {
                assertion["title"]
                    .as_str()
                    .map(|title| format!("taskTitle.{}", slug_fragment(title)))
                    .unwrap_or_else(|| format!("{assertion_type}.target"))
            }),
        "sessionDependencyVisible" => format!(
            "sessionDependency.{}",
            assertion["sessionType"].as_str().unwrap_or("current")
        ),
        "taskTitleVisible" => format!(
            "taskTitle.{}",
            slug_fragment(assertion["title"].as_str().unwrap_or("current"))
        ),
        "windowCountUnchanged" => format!(
            "window.count.{}",
            assertion["baseline"].as_str().unwrap_or("baseline")
        ),
        _ => format!("{assertion_type}.target"),
    }
}

fn assertion_text_id(assertion_type: &str, assertion: &Value) -> String {
    match assertion_type {
        "aiProviderFilters" => "ai.providerFilters".to_owned(),
        "hookProvidersReady" => format!(
            "hooks.providers.{}",
            assertion["taskId"].as_str().unwrap_or("current")
        ),
        "sessionDependencyState" => format!(
            "sessionDependency.{}.state",
            assertion["sessionType"].as_str().unwrap_or("current")
        ),
        "route" => format!(
            "route.{}",
            slug_fragment(assertion["pattern"].as_str().unwrap_or("current"))
        ),
        "taskField" => format!(
            "task.{}.field.{}",
            assertion["taskId"].as_str().unwrap_or("current"),
            assertion["field"].as_str().unwrap_or("unknown")
        ),
        _ => format!("{assertion_type}.text"),
    }
}

fn slug_fragment(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        "current".to_owned()
    } else {
        slug
    }
}

fn qa_command_type(command: &QaControlCommand) -> &'static str {
    match command {
        QaControlCommand::Ping => "ping",
        QaControlCommand::QueryElement { .. } => "queryElement",
        QaControlCommand::QueryText { .. } => "queryText",
        QaControlCommand::SyntheticClick { .. } => "syntheticClick",
        QaControlCommand::SyntheticKey { .. } => "syntheticKey",
        QaControlCommand::SyntheticMouse { .. } => "syntheticMouse",
        QaControlCommand::DumpScreenshot { .. } => "dumpScreenshot",
        QaControlCommand::StartVideoCapture { .. } => "startVideoCapture",
        QaControlCommand::StopVideoCapture { .. } => "stopVideoCapture",
        QaControlCommand::DbSnapshot => "dbSnapshot",
    }
}

fn control_command_for_step(action: &str) -> Option<&'static str> {
    match action {
        "launchApp" => Some("launchAppWithQaSocket"),
        "waitForBoardReady" => Some("queryElement"),
        "captureScreen" => Some("dumpScreenshot"),
        "pressKeys" | "pressShortcut" | "typeText" => Some("syntheticKey"),
        "recordWindowCount" => Some("queryText"),
        "chooseContextAction"
        | "confirmDialog"
        | "dragTaskToColumn"
        | "fillBranchTaskForm"
        | "fillTaskForm"
        | "focusTask"
        | "openCreateTaskModal"
        | "openDiffRoute"
        | "openNotificationCenter"
        | "openPaneLayoutRoute"
        | "openProjectFilter"
        | "openSessionDependencyPanel"
        | "checkSessionDependency"
        | "checkTaskHooks"
        | "collapseTaskSidebar"
        | "dismissSidebarHint"
        | "installTaskHooks"
        | "recheckTaskHooks"
        | "installSessionDependency"
        | "retrySessionDependency"
        | "openSettings"
        | "openTaskContextMenu"
        | "openTaskDetail"
        | "selectDockItem"
        | "selectFirstChangedFile"
        | "selectPaneLayout"
        | "selectProjects"
        | "setAppSetting"
        | "setProjectColor"
        | "submitBranchTask"
        | "submitCreateTask" => Some("syntheticClick"),
        _ => None,
    }
}

fn control_command_for_assertion(assertion_type: &str) -> Option<&'static str> {
    match assertion_type {
        "dbCount" | "dbMissing" | "dbRow" | "noGenericEnvLeak" | "noWorktreeCreated"
        | "taskCountByStatus" => Some("dbSnapshot"),
        "columns"
        | "diffPaneVisible"
        | "diffSidebarVisible"
        | "dockItems"
        | "hookStatusVisible"
        | "notificationCenterVisible"
        | "onlyProjectTasksVisible"
        | "searchResultVisible"
        | "sessionDependencyVisible"
        | "shortcutLabels"
        | "taskNotVisible"
        | "taskTitleVisible"
        | "taskVisible"
        | "windowCountUnchanged" => Some("queryElement"),
        "aiProviderFilters"
        | "hookProvidersReady"
        | "route"
        | "sessionDependencyState"
        | "taskField" => Some("queryText"),
        "externalToolBlockerAllowed" => Some("protocolRecordedBlocker"),
        _ => None,
    }
}

pub fn full_parity_markdown(report: &Value) -> String {
    let status = report["status"].as_str().unwrap_or("UNKNOWN");
    let scenario_count = report["scenarioCount"].as_u64().unwrap_or_default();
    let headless_contracts_pass = report["headlessContractsPass"].as_bool().unwrap_or(false);
    let covered = report["coveredScenarioIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let missing = report["missingScenarioIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let slice_artifacts = report["sliceArtifacts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let slice_statuses = report["sliceStatuses"]
        .as_array()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let macos_runtime_gates = report["macosRuntimeGates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    let mut markdown = String::new();
    markdown.push_str("# Native Phase 5 QA Report\n\n");
    markdown.push_str(&format!("- Status: `{status}`\n"));
    markdown.push_str(&format!(
        "- Headless/native contract coverage: `{}`\n",
        if headless_contracts_pass {
            "PASS"
        } else {
            "FAIL"
        }
    ));
    markdown.push_str(&format!(
        "- Scenario coverage: `{}/{scenario_count}`\n\n",
        covered.len()
    ));

    let functional = &report["functionalEvidence"];
    let replay = &functional["qaControlReplay"];
    markdown.push_str("## Functional Evidence\n\n");
    markdown.push_str(&format!(
        "- Status: `{}`\n",
        functional["status"].as_str().unwrap_or("UNKNOWN")
    ));
    markdown.push_str(&format!(
        "- QA control replay: `{}` transport, `{}` commands executed, mode `{}`\n",
        if replay["transportPass"].as_bool().unwrap_or(false) {
            "PASS"
        } else {
            "FAIL"
        },
        replay["executedCommandCount"].as_u64().unwrap_or_default(),
        replay["mode"].as_str().unwrap_or("unknown")
    ));
    if let Some(blockers) = replay["blockers"]
        .as_array()
        .filter(|blockers| !blockers.is_empty())
    {
        markdown.push_str("\n### Functional Blockers\n\n");
        for blocker in blockers.iter().filter_map(Value::as_str) {
            markdown.push_str(&format!("- {blocker}\n"));
        }
    }

    let visual = &report["visualEvidence"];
    let baseline = &visual["baselineManifest"];
    let native_captures = &visual["nativeExpectedCaptures"];
    let comparison = &visual["comparison"];
    let expected_screens = native_captures["expectedScreenCount"]
        .as_u64()
        .unwrap_or_default();
    let missing_screens = native_captures["missingScreenCount"]
        .as_u64()
        .unwrap_or_default();
    let expected_videos = native_captures["expectedVideoCount"]
        .as_u64()
        .unwrap_or_default();
    let missing_videos = native_captures["missingVideoCount"]
        .as_u64()
        .unwrap_or_default();
    markdown.push_str("\n## Visual Evidence\n\n");
    markdown.push_str(&format!(
        "- Status: `{}`\n",
        visual["status"].as_str().unwrap_or("UNKNOWN")
    ));
    markdown.push_str(&format!(
        "- Baseline manifest: `{}` with `{}` rows\n",
        if baseline["pass"].as_bool().unwrap_or(false) {
            "PASS"
        } else {
            "FAIL"
        },
        baseline["rowCount"].as_u64().unwrap_or_default()
    ));
    markdown.push_str(&format!(
        "- Native screenshots: `{}/{}` captured\n",
        expected_screens.saturating_sub(missing_screens),
        expected_screens
    ));
    markdown.push_str(&format!(
        "- Native videos: `{}/{}` captured\n",
        expected_videos.saturating_sub(missing_videos),
        expected_videos
    ));
    markdown.push_str(&format!(
        "- Structural comparison: `{}`\n",
        if comparison["performed"].as_bool().unwrap_or(false) {
            "RECORDED"
        } else {
            "NOT RECORDED"
        }
    ));
    if let Some(blockers) = visual["blockers"]
        .as_array()
        .filter(|blockers| !blockers.is_empty())
    {
        markdown.push_str("\n### Visual Blockers\n\n");
        for blocker in blockers.iter().filter_map(Value::as_str) {
            markdown.push_str(&format!("- {blocker}\n"));
        }
    }

    let performance = &report["performanceEvidence"];
    let native_perf = &performance["native"];
    markdown.push_str("\n## Performance Evidence\n\n");
    markdown.push_str(&format!(
        "- Status: `{}`\n",
        performance["status"].as_str().unwrap_or("UNKNOWN")
    ));
    markdown.push_str(&format!(
        "- Release binary: `{}`",
        native_perf["releaseBinary"]["status"]
            .as_str()
            .unwrap_or("UNKNOWN")
    ));
    if let Some(size_bytes) = native_perf["releaseBinary"]["sizeBytes"].as_u64() {
        markdown.push_str(&format!(" (`{size_bytes}` bytes)"));
    }
    markdown.push('\n');
    markdown.push_str(&format!(
        "- App bundle: `{}`\n",
        native_perf["appBundle"]["status"]
            .as_str()
            .unwrap_or("UNKNOWN")
    ));
    markdown.push_str(&format!(
        "- DMG: `{}`\n",
        native_perf["dmg"]["status"].as_str().unwrap_or("UNKNOWN")
    ));
    markdown.push_str(&format!(
        "- macOS cold start: `{}`\n",
        native_perf["macosMeasurements"]["coldStartToWindow"]["status"]
            .as_str()
            .unwrap_or("UNKNOWN")
    ));
    markdown.push_str(&format!(
        "- macOS idle memory: `{}`\n",
        native_perf["macosMeasurements"]["idleMemory"]["status"]
            .as_str()
            .unwrap_or("UNKNOWN")
    ));
    markdown.push_str(&format!(
        "- Terminal scroll FPS: `{}`\n",
        native_perf["macosMeasurements"]["terminalScrollFps"]["status"]
            .as_str()
            .unwrap_or("UNKNOWN")
    ));
    if let Some(blockers) = performance["blockers"]
        .as_array()
        .filter(|blockers| !blockers.is_empty())
    {
        markdown.push_str("\n### Performance Blockers\n\n");
        for blocker in blockers.iter().filter_map(Value::as_str) {
            markdown.push_str(&format!("- {blocker}\n"));
        }
    }

    markdown.push_str("\n## Slice Artifacts\n\n");
    if slice_statuses.is_empty() {
        for artifact in slice_artifacts {
            markdown.push_str(&format!("- `{artifact}`\n"));
        }
    } else {
        for artifact in slice_statuses {
            markdown.push_str(&format!(
                "- `{}` - `{}`\n",
                artifact["artifact"].as_str().unwrap_or("unknown"),
                artifact["status"].as_str().unwrap_or("UNKNOWN")
            ));
        }
    }

    markdown.push_str("\n## Covered Scenarios\n\n");
    for scenario_id in covered {
        markdown.push_str(&format!("- `{scenario_id}`\n"));
    }

    if !missing.is_empty() {
        markdown.push_str("\n## Missing Scenarios\n\n");
        for scenario_id in missing {
            markdown.push_str(&format!("- `{scenario_id}`\n"));
        }
    }

    markdown.push_str("\n## Blocking Gates\n\n");
    for gate in macos_runtime_gates {
        markdown.push_str(&format!("- {gate}\n"));
    }

    markdown.push_str("\n## Linux Verification Commands\n\n");
    markdown.push_str("- `cargo test --workspace --quiet`\n");
    markdown.push_str("- `cargo build --workspace --quiet`\n");
    markdown.push_str("- `cargo build --release --quiet`\n");
    markdown.push_str("- `native/scripts/package-macos-app.sh` -> exit 78 on Linux by design\n");
    markdown
}

#[cfg(unix)]
fn request_qa_control(
    socket_path: &Path,
    command: &QaControlCommand,
) -> Result<QaControlResponse, Box<dyn Error + Send + Sync>> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)?;
    let request_json = serde_json::to_string(command)?;
    stream.write_all(request_json.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    if reader.read_line(&mut response)? == 0 {
        return Err("QA control socket closed without a response".into());
    }

    Ok(serde_json::from_str(response.trim_end())?)
}

#[cfg(not(unix))]
fn request_qa_control(
    _socket_path: &Path,
    _command: &QaControlCommand,
) -> Result<QaControlResponse, Box<dyn Error + Send + Sync>> {
    Err("QA control client requires a Unix socket transport".into())
}

fn create_temp_git_repo(name: &str) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kanvibe-harness-{name}-{}-{unique}",
        std::process::id()
    ));
    let repo = root.join(name);

    fs::create_dir_all(repo.join("src"))?;
    git_raw_in_dir(&repo, &["init", "-b", "main"])?;
    git_raw(&repo, &["config", "user.email", "qa@kanvibe.test"])?;
    git_raw(&repo, &["config", "user.name", "KanVibe QA"])?;

    Ok(repo)
}

fn write_file(path: PathBuf, content: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn git_raw(repo: &Path, args: &[&str]) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut git_args = vec!["-C", repo.to_str().ok_or("non-utf8 repo path")?];
    git_args.extend_from_slice(args);
    git_raw_in_dir(repo, &git_args)
}

fn git_raw_in_dir(cwd: &Path, args: &[&str]) -> Result<(), Box<dyn Error + Send + Sync>> {
    let output = Command::new("git").current_dir(cwd).args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn writable_seed_copy(
    repo_root: &Path,
    artifact_name: &str,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "kanvibe-{artifact_name}-{}-{unique}.sqlite",
        std::process::id()
    ));

    fs::copy(repo_root.join("qa/seed/kanvibe-seed.sqlite"), &path)?;
    Ok(path)
}

fn resolve_native_app_binary(repo_root: &Path, app_binary: Option<PathBuf>) -> PathBuf {
    let candidate = app_binary.unwrap_or_else(|| repo_root.join("native/target/debug/kanvibe-app"));
    let resolved = if candidate.is_absolute() || candidate.is_file() {
        candidate
    } else {
        repo_root.join(candidate)
    };

    resolved.canonicalize().unwrap_or(resolved)
}

fn prepare_native_app_launch_seed_copy(
    repo_root: &Path,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let destination = repo_root.join(format!(
        "{NATIVE_REPLAY_DB_DIR_FROM_REPO_ROOT}/native-app-launch.sqlite"
    ));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(repo_root.join("qa/seed/kanvibe-seed.sqlite"), &destination)?;
    Ok(destination)
}

#[cfg(all(debug_assertions, unix))]
fn prepare_scenario_seed_copy(
    repo_root: &Path,
    scenario_id: &str,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    let destination_dir = repo_root.join(NATIVE_REPLAY_DB_DIR_FROM_REPO_ROOT);
    fs::create_dir_all(&destination_dir)?;
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let destination = destination_dir.join(format!(
        "{scenario_id}-{}-{unique}.sqlite",
        std::process::id()
    ));

    fs::copy(repo_root.join("qa/seed/kanvibe-seed.sqlite"), &destination)?;
    Ok(destination)
}

fn scenario_seed_copy_relative_path(scenario_id: &str) -> String {
    format!("{NATIVE_REPLAY_DB_DIR_FROM_REPO_ROOT}/{scenario_id}.sqlite")
}

fn output_text(bytes: &[u8]) -> String {
    const MAX_OUTPUT_CHARS: usize = 4_000;

    let text = String::from_utf8_lossy(bytes).trim().to_owned();
    if text.chars().count() <= MAX_OUTPUT_CHARS {
        text
    } else {
        let tail = text
            .chars()
            .rev()
            .take(MAX_OUTPUT_CHARS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("<truncated>\n{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn harness_tracks_required_phase_five_scenarios() {
        assert_eq!(scenario_count(), 14);
        assert_eq!(SCENARIO_IDS[0], "S01-board-load-and-columns");
        assert_eq!(SCENARIO_IDS[13], "S14-remote-session-dependencies");
    }

    #[test]
    fn harness_loads_read_only_seed_snapshot() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let snapshot = load_seed_snapshot(repo_root, Locale::Ko).expect("seed snapshot");

        assert_eq!(snapshot.board.projects.len(), 3);
        assert_eq!(snapshot.board.done_total, 3);
        assert_eq!(snapshot.labels.new_task, "+ 새 작업");
    }

    #[test]
    fn read_only_board_report_is_deterministic_for_s01() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = read_only_board_report(repo_root, Locale::En).expect("read-only report");

        assert_eq!(report["artifact"], "slice-1-read-only-board");
        assert_eq!(report["columns"][0]["status"], "todo");
        assert_eq!(
            report["columns"][0]["firstTaskTitle"],
            "Draft native board shell"
        );
    }

    #[test]
    fn board_interaction_report_exercises_slice_two_behaviors() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = board_interaction_report(repo_root).expect("board interaction report");

        assert_eq!(report["artifact"], "slice-2-board-interactions");
        assert_eq!(report["afterCreate"]["todoCount"], 4);
        assert_eq!(report["editedTask"]["priority"], "high");
        assert_eq!(report["movedToProgress"]["status"], "progress");
        assert_eq!(report["donePageAfterMove"]["doneTotal"], 4);
        assert_eq!(report["deletedCreatedTask"], true);
        assert_eq!(report["projectColor"]["color"], "#123456");
    }

    #[test]
    fn task_detail_report_exercises_slice_three_behaviors() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = task_detail_report(repo_root).expect("task detail report");

        assert_eq!(report["artifact"], "slice-3-task-detail-pty-dock");
        assert_eq!(report["terminalTask"]["sessionType"], "tmux");
        assert_eq!(report["terminalTask"]["dockItems"][2], "terminal");
        assert_eq!(report["pullRequestTask"]["dockItems"][3], "pullRequest");
        assert_eq!(
            report["pullRequestTask"]["aiProviderFilters"][3],
            "opencode"
        );
        assert_eq!(report["existingWindowFocus"]["decision"], "FocusExisting");
        assert_eq!(report["remoteSession"]["sshHost"], "qa-remote");
        assert_eq!(report["remoteSession"]["sessionType"], "zellij");
        assert_eq!(report["ptyEnvironment"]["forbiddenAbsent"], true);
    }

    #[test]
    fn git_diff_report_exercises_slice_four_behaviors() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = git_diff_report(repo_root).expect("git diff report");

        assert_eq!(report["artifact"], "slice-4-git-diff-worktree");
        assert_eq!(report["diffRoute"]["fileCount"], 2);
        assert_eq!(report["diffRoute"]["originalContainsBase"], true);
        assert_eq!(report["diffRoute"]["currentContainsBranch"], true);
        assert_eq!(report["diffRoute"]["savedContainsSaved"], true);
        assert_eq!(report["branchFromTask"]["status"], "progress");
        assert_eq!(report["branchFromTask"]["baseBranch"], "main");
        assert_eq!(report["branchFromTask"]["sessionType"], "tmux");
        assert_eq!(
            report["branchFromTask"]["worktreeCurrentBranch"],
            "qa/branch-from-task"
        );
    }

    #[test]
    fn notification_hooks_report_exercises_slice_five_behaviors() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = notification_hooks_report(repo_root).expect("notification hooks report");

        assert_eq!(report["artifact"], "slice-5-notifications-hooks-ai");
        assert_eq!(report["notificationCenter"]["visible"], true);
        assert_eq!(report["notificationCenter"]["unreadCount"], 3);
        assert_eq!(report["hookStatus"]["visible"], true);
        assert_eq!(report["hookStatus"]["hasExpectedHookServerUrl"], true);
        assert_eq!(report["aiSessions"]["providers"][0], "claude");
        assert_eq!(report["aiSessions"]["sessionCount"], 4);
        assert_eq!(report["aiSessions"]["firstSessionId"], "codex-native-hooks");
        assert_eq!(report["appSettings"]["background_sync_enabled"], "false");
    }

    #[test]
    fn settings_layout_remote_report_exercises_slice_six_behaviors() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report =
            settings_layout_remote_report(repo_root).expect("settings layout remote report");

        assert_eq!(report["artifact"], "slice-6-settings-layout-remote");
        assert_eq!(report["settings"]["route"], "/ko/settings");
        assert_eq!(report["settings"]["themePreferenceAfter"], "dark");
        assert_eq!(report["settings"]["defaultSessionType"], "zellij");
        assert_eq!(report["settings"]["taskSearchShortcut"], "Mod+Shift+K");
        assert_eq!(report["settings"]["vimModeEnabled"], false);
        assert_eq!(report["paneLayout"]["beforeLayoutType"], "horizontal_2");
        assert_eq!(report["paneLayout"]["savedLayoutType"], "vertical_2");
        assert_eq!(report["paneLayout"]["effectiveLayoutType"], "vertical_2");
        assert_eq!(report["paneLayout"]["fallbackLayoutType"], "quad");
        assert_eq!(report["paneLayout"]["zellijLayoutContainsVertical"], true);
        assert_eq!(report["remoteSessionDependency"]["visible"], true);
        assert_eq!(
            report["remoteSessionDependency"]["availableBeforeInstall"],
            false
        );
        assert_eq!(
            report["remoteSessionDependency"]["availableAfterInstall"],
            true
        );
        assert_eq!(
            report["remoteSessionDependency"]["installCommandContainsCargo"],
            true
        );
        assert_eq!(report["windowPolicy"]["action"], "focus-existing");
        assert_eq!(
            report["windowPolicy"]["keepsCurrentRouteForBackgroundReview"],
            true
        );
    }

    #[test]
    fn full_parity_report_covers_all_scenarios_and_records_macos_gate() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = full_parity_report(repo_root).expect("full parity report");
        let markdown = full_parity_markdown(&report);

        assert_eq!(report["artifact"], "phase-5-full-parity");
        assert_eq!(report["status"], "BLOCKED");
        assert_eq!(report["headlessContractsPass"], true);
        assert_eq!(report["scenarioCount"], 14);
        assert_eq!(report["missingScenarioIds"].as_array().unwrap().len(), 0);
        assert_eq!(report["coveredScenarioIds"].as_array().unwrap().len(), 14);
        assert_eq!(report["functionalEvidence"]["status"], "BLOCKED");
        assert_eq!(report["visualEvidence"]["status"], "BLOCKED");
        assert_eq!(report["performanceEvidence"]["status"], "BLOCKED");
        assert!(
            report["sliceStatuses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|artifact| artifact["status"] == "BLOCKED")
        );
        assert!(
            report["sliceArtifacts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|artifact| artifact == "native-visual-parity-evidence")
        );
        assert!(markdown.contains("Status: `BLOCKED`"));
        assert!(markdown.contains("## Functional Evidence"));
        assert!(markdown.contains("## Visual Evidence"));
        assert!(markdown.contains("## Performance Evidence"));
        assert!(markdown.contains("`native-visual-parity-evidence` - `BLOCKED`"));
        assert!(markdown.contains("macOS"));
    }

    #[test]
    fn full_parity_status_prioritizes_failures_then_blockers() {
        assert_eq!(
            full_parity_status(
                0,
                &[
                    json!({"artifact": "legacy-pass"}),
                    json!({"artifact": "blocked", "status": "BLOCKED"})
                ]
            ),
            "BLOCKED"
        );
        assert_eq!(
            full_parity_status(
                0,
                &[
                    json!({"artifact": "blocked", "status": "BLOCKED"}),
                    json!({"artifact": "failed", "status": "FAIL"})
                ]
            ),
            "FAIL"
        );
        assert_eq!(
            full_parity_status(1, &[json!({"artifact": "legacy-pass"})]),
            "FAIL"
        );
        assert_eq!(
            full_parity_status(0, &[json!({"artifact": "legacy-pass"})]),
            "PASS"
        );
    }

    #[test]
    fn native_visual_parity_report_maps_manifest_and_missing_native_captures() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-visual-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_root).expect("create temp visual root");

        let report = native_visual_parity_report(&repo_root, Some(temp_root.clone()))
            .expect("native visual parity report");

        assert_eq!(report["artifact"], "native-visual-parity-evidence");
        assert_eq!(report["status"], "BLOCKED");
        assert_eq!(report["baselineManifest"]["rowCount"], 14);
        assert_eq!(report["baselineManifest"]["pass"], true);
        assert_eq!(report["nativeExpectedCaptures"]["expectedScreenCount"], 14);
        assert_eq!(report["nativeExpectedCaptures"]["expectedVideoCount"], 14);
        assert_eq!(report["nativeExpectedCaptures"]["missingScreenCount"], 14);
        assert_eq!(report["nativeExpectedCaptures"]["missingVideoCount"], 14);
        assert_eq!(report["comparison"]["performed"], false);
        assert_eq!(report["scenarioMappings"].as_array().unwrap().len(), 14);

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn native_performance_report_records_size_targets_and_runtime_blockers() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let temp_root = std::env::temp_dir().join(format!(
            "kanvibe-native-perf-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let app_bundle = temp_root.join("KanVibe.app");
        let release_binary = temp_root.join("kanvibe-app");
        fs::create_dir_all(app_bundle.join("Contents/MacOS")).expect("create fake bundle");
        fs::write(app_bundle.join("Contents/MacOS/KanVibe"), b"fake-app").expect("write fake app");
        fs::write(&release_binary, b"fake-release").expect("write fake release");

        let report = native_performance_report(
            &repo_root,
            Some(app_bundle.clone()),
            None,
            Some(release_binary.clone()),
        )
        .expect("native performance report");

        assert_eq!(report["artifact"], "native-performance-comparison");
        assert_eq!(report["status"], "BLOCKED");
        assert_eq!(report["native"]["appBundle"]["status"], "PASS");
        assert_eq!(report["native"]["releaseBinary"]["status"], "PASS");
        assert_eq!(
            report["native"]["macosMeasurements"]["coldStartToWindow"]["status"],
            "BLOCKED"
        );
        assert_eq!(report["electronBaseline"]["exists"], true);

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn scenario_control_protocol_report_maps_all_declared_steps_and_assertions() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = scenario_control_protocol_report(repo_root).expect("qa control report");

        assert_eq!(report["artifact"], "native-qa-control-protocol");
        assert_eq!(report["socketEnv"], "KANVIBE_QA_SOCKET");
        assert_eq!(report["debugOnly"], true);
        assert_eq!(report["scenarioFileCount"], 14);
        assert_eq!(report["coveragePass"], true);
        assert_eq!(report["unsupportedSteps"].as_array().unwrap().len(), 0);
        assert_eq!(report["unsupportedAssertions"].as_array().unwrap().len(), 0);
        assert!(
            report["stepMappingCount"]
                .as_u64()
                .is_some_and(|count| count > 40)
        );
        assert!(
            report["assertionMappingCount"]
                .as_u64()
                .is_some_and(|count| count > 20)
        );
    }

    #[test]
    fn qa_control_replay_plan_builds_executable_socket_sequence() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = qa_control_replay_plan_report(repo_root).expect("qa replay plan");

        assert_eq!(report["artifact"], "native-qa-control-replay-plan");
        assert_eq!(report["scenarioFileCount"], 14);
        assert_eq!(report["launchActionCount"], 14);
        assert_eq!(report["videoCommandCount"], 28);
        assert_eq!(report["videoArtifactCount"], 14);
        assert_eq!(report["coveragePass"], true);
        assert_eq!(report["unsupportedSteps"].as_array().unwrap().len(), 0);
        assert_eq!(report["unsupportedAssertions"].as_array().unwrap().len(), 0);
        assert!(
            report["socketCommandCount"]
                .as_u64()
                .is_some_and(|count| count > 100)
        );

        let first_plan = report["scenarioPlans"][0]["replayItems"]
            .as_array()
            .expect("first replay item array");
        assert_eq!(first_plan[0]["kind"], "launchApp");
        assert_eq!(first_plan[0]["sourceSeed"], "qa/seed/kanvibe-seed.sqlite");
        assert_eq!(
            first_plan[0]["env"]["KANVIBE_DB_PATH"],
            "qa/parity/native-db/S01-board-load-and-columns.sqlite"
        );
        assert_eq!(
            first_plan[1]["command"],
            json!({
                "type": "startVideoCapture",
                "path": "qa/parity/native-videos/S01-board-load-and-columns.mp4"
            })
        );
        assert_eq!(
            first_plan[2]["command"],
            json!({"type": "queryElement", "id": "app.root"})
        );
        assert_eq!(first_plan[3]["command"]["type"], "dumpScreenshot");
        assert_eq!(
            first_plan.last().unwrap()["command"],
            json!({
                "type": "stopVideoCapture",
                "path": "qa/parity/native-videos/S01-board-load-and-columns.mp4"
            })
        );

        let create_plan = report["scenarioPlans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plan| plan["scenarioId"] == "S02-create-task-modal")
            .expect("S02 replay plan");
        let fill_command = create_plan["replayItems"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["sourceType"] == "fillTaskForm")
            .expect("fill task command");
        assert_eq!(
            fill_command["command"]["payload"]["title"],
            "QA created task"
        );
        assert_eq!(fill_command["command"]["payload"]["priority"], "medium");

        let color_plan = report["scenarioPlans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plan| plan["scenarioId"] == "S08-context-menu-status-and-delete")
            .expect("S08 replay plan");
        let color_command = color_plan["replayItems"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["sourceType"] == "setProjectColor")
            .expect("project color command");
        assert_eq!(
            color_command["command"]["id"],
            "projectColor.qa-project-kanvibe"
        );
        assert_eq!(color_command["command"]["payload"]["color"], "#5EEAD4");
        let color_assertion = color_plan["replayItems"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| {
                item["sourceType"] == "taskField" && item["source"]["field"] == "project_color"
            })
            .expect("eventual project color assertion");
        assert_eq!(color_assertion["source"]["eventually"], true);
    }

    #[test]
    fn macos_release_scripts_fail_closed_around_signing_and_notarization() {
        let package = include_str!("../../../scripts/package-macos-app.sh");
        let verify = include_str!("../../../scripts/verify-macos-release.sh");
        let workflow = include_str!("../../../../.github/workflows/native-release-candidate.yml");

        for contract in [
            "--release",
            "--sign-identity",
            "--notary-profile",
            "--options runtime",
            "notarytool submit",
            "stapler staple",
            "spctl --assess",
            "notary-app.json",
            "notary-dmg.json",
            "shasum -a 256",
            "DMG_CHECKSUM_PATH",
            "HELPERS_DIR",
            "KanVibeUpdater",
            "KanVibeBuildCommit",
            "requires a clean source tree",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "lipo -create",
        ] {
            assert!(
                package.contains(contract),
                "missing package contract: {contract}"
            );
        }
        for contract in [
            "Developer ID Application:",
            "TeamIdentifier=",
            "Runtime Version=",
            "stapler validate",
            "spctl --assess",
            "hdiutil attach -readonly",
            "verify_bundle \"$MOUNTED_APP\"",
            "ACTUAL_DIGEST=",
            "KanVibeBuildCommit",
            "TeamIdentifier=$APP_TEAM_ID",
            "Contents/Helpers/KanVibeUpdater",
            "lipo -archs",
        ] {
            assert!(
                verify.contains(contract),
                "missing verifier contract: {contract}"
            );
        }
        for contract in [
            "workflow_dispatch:",
            "permissions:\n  contents: read",
            "aarch64-apple-darwin,x86_64-apple-darwin",
            "MACOS_CERTIFICATE_P12_BASE64",
            "notarytool store-credentials",
            "package-macos-app.sh --release",
            "verify-macos-release.sh",
            "actions/upload-artifact@v4",
            "if: always()",
            "security delete-keychain",
        ] {
            assert!(
                workflow.contains(contract),
                "missing native release-candidate workflow contract: {contract}"
            );
        }
    }

    #[test]
    fn root_native_commands_are_node_free_and_electron_is_explicitly_legacy() {
        let launcher = include_str!("../../../../kanvibe-native");
        for contract in [
            "dev)",
            "build)",
            "test)",
            "check)",
            "package)",
            "phase5)",
            "verify-phase5)",
            "native/Cargo.toml",
            "native/crates/kanvibe-terminal/Cargo.toml",
            "package-macos-app.sh",
            "phase5-macos-run.sh",
            "verify-phase5-run.sh",
        ] {
            assert!(
                launcher.contains(contract),
                "missing root native command contract: {contract}"
            );
        }
        for forbidden in ["node ", "pnpm ", "electron"] {
            assert!(
                !launcher.to_ascii_lowercase().contains(forbidden),
                "root native command unexpectedly depends on legacy runtime: {forbidden}"
            );
        }

        let package: Value =
            serde_json::from_str(include_str!("../../../../package.json")).expect("package.json");
        let scripts = package["scripts"].as_object().expect("package scripts");
        for name in [
            "legacy:electron:dev",
            "legacy:electron:start",
            "legacy:electron:build",
            "legacy:electron:package",
            "legacy:electron:package:dir",
            "legacy:electron:deploy",
            "legacy:electron:qa",
        ] {
            assert!(
                scripts.get(name).and_then(Value::as_str).is_some(),
                "missing explicit Electron baseline command: {name}"
            );
        }

        let workflow = include_str!("../../../../.github/workflows/ci.yml");
        for contract in [
            "name: Legacy Electron baseline",
            "run: ./kanvibe-native check",
            "run: ./kanvibe-native test",
            "KANVIBE_BUILD_COMMIT: ${{ github.sha }}",
            "Contents/Helpers/KanVibeUpdater",
            "Print :KanVibeBuildCommit",
        ] {
            assert!(
                workflow.contains(contract),
                "missing native/legacy CI boundary: {contract}"
            );
        }
    }

    #[test]
    fn native_updater_source_keeps_signature_and_rollback_gates_fail_closed() {
        let updater = include_str!("../../kanvibe-app/src/native_updater.rs");
        for contract in [
            "SHA-256 did not match GitHub metadata",
            "\"/usr/bin/codesign\"",
            "\"stapler\", \"validate\"",
            "\"/usr/sbin/spctl\"",
            "\"attach\", \"-readonly\", \"-nobrowse\"",
            "TeamIdentifier=",
            "CFBundleIdentifier",
            "CFBundleShortVersionString",
            "NativeUpdateState::AwaitingHealth",
            "NativeUpdateState::RolledBack",
            "finish_or_rollback_update(&mut journal, journal_path, false)",
        ] {
            assert!(
                updater.contains(contract),
                "missing native updater safety contract: {contract}"
            );
        }
    }

    #[test]
    fn electron_removal_ledger_covers_every_registered_service_export_and_direct_ipc() {
        let ledger = include_str!("../../../../qa/ELECTRON_REMOVAL_LEDGER.md");
        let services = [
            include_str!("../../../../src/desktop/main/services/appSettingsService.ts"),
            include_str!("../../../../src/desktop/main/services/diffService.ts"),
            include_str!("../../../../src/desktop/main/services/githubCliDependencyService.ts"),
            include_str!("../../../../src/desktop/main/services/hookService.ts"),
            include_str!("../../../../src/desktop/main/services/kanbanService.ts"),
            include_str!("../../../../src/desktop/main/services/paneLayoutService.ts"),
            include_str!("../../../../src/desktop/main/services/projectService.ts"),
            include_str!("../../../../src/desktop/main/services/releaseUpdateService.ts"),
            include_str!("../../../../src/desktop/main/services/sessionDependencyService.ts"),
        ];
        for source in services {
            for line in source.lines().map(str::trim) {
                let declaration = line
                    .strip_prefix("export async function ")
                    .or_else(|| line.strip_prefix("export function "));
                let Some(declaration) = declaration else {
                    continue;
                };
                let name = declaration
                    .split(['(', '<'])
                    .next()
                    .expect("exported function name");
                assert!(
                    ledger.contains(&format!("`{name}`")),
                    "missing Electron service method in removal ledger: {name}"
                );
            }
        }

        let electron_main = include_str!("../../../../electron/main.js");
        for line in electron_main.lines() {
            let Some(channel) = line
                .split_once("ipcMain.handle(\"")
                .and_then(|(_, suffix)| suffix.split_once('"').map(|(channel, _)| channel))
            else {
                continue;
            };
            assert!(
                ledger.contains(&format!("`{channel}`")),
                "missing direct Electron IPC in removal ledger: {channel}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn native_app_launch_report_records_missing_binary_blocker() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = native_app_launch_report(
            &repo_root,
            Some(repo_root.join("native/target/debug/missing-kanvibe-app")),
        )
        .expect("native app launch report");

        assert_eq!(report["artifact"], "native-app-launch-contract");
        assert_eq!(report["status"], "BLOCKED");
        assert_eq!(report["qaSocketReady"], false);
        assert!(
            report["blockers"][0]
                .as_str()
                .is_some_and(|blocker| blocker.contains("debug binary is missing"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn native_app_replay_report_records_missing_binary_blocker() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = native_app_replay_report(
            &repo_root,
            Some(repo_root.join("native/target/debug/missing-kanvibe-app")),
        )
        .expect("native app replay report");

        assert_eq!(report["artifact"], "native-app-replay-contract");
        assert_eq!(report["status"], "BLOCKED");
        assert_eq!(report["qaSocketReadyPass"], false);
        assert_eq!(
            report["sourceReplayPlanArtifact"],
            "native-qa-control-replay-plan"
        );
        assert!(
            report["blockers"][0]
                .as_str()
                .is_some_and(|blocker| blocker.contains("debug binary is missing"))
        );
    }

    #[cfg(all(debug_assertions, unix))]
    #[test]
    fn qa_control_replay_smoke_executes_all_commands_over_socket() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let report = qa_control_replay_smoke_report(repo_root).expect("qa replay smoke report");

        assert_eq!(report["artifact"], "native-qa-control-replay-execution");
        assert_eq!(report["mode"], "inProcessDebugSocketPerScenario");
        assert_eq!(report["status"], "BLOCKED");
        assert_eq!(report["transportPass"], true);
        assert_eq!(report["scenarioFileCount"], 14);
        assert_eq!(report["scenarioSocketCount"], 14);
        assert_eq!(report["launchActionCount"], 14);
        assert_eq!(report["executedCommandCount"], 158);
        assert_eq!(report["transportErrorCount"], 0);
        assert_eq!(report["structuredErrorCount"], 0);
        assert_eq!(report["screenshotBlockedCount"], 14);
        assert_eq!(report["videoBlockedCount"], 28);
        assert_eq!(report["pendingDispatchCount"], 0);
        assert_eq!(report["missingElementCount"], 0);
        assert_eq!(report["scenarioResults"].as_array().unwrap().len(), 14);
        let db_results = report["scenarioResults"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|scenario| scenario["commandResults"].as_array().unwrap())
            .filter(|command| command["commandType"] == "dbSnapshot")
            .collect::<Vec<_>>();
        assert_eq!(db_results.len(), 14);
        assert!(
            db_results
                .iter()
                .all(|command| command["behaviorStatus"] == "pass")
        );
        let assertion_results = report["scenarioResults"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|scenario| scenario["commandResults"].as_array().unwrap())
            .filter(|command| command["sourceKind"] == "assertion")
            .collect::<Vec<_>>();
        assert!(assertion_results.len() > db_results.len());
        assert!(
            assertion_results
                .iter()
                .all(|command| command["behaviorStatus"] == "pass")
        );
    }

    #[cfg(all(debug_assertions, unix))]
    #[test]
    fn qa_control_client_round_trips_against_debug_socket() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let seed = repo_root.join("qa/seed/kanvibe-seed.sqlite");
        let bootstrap = kanvibe_app::load_read_only_board(&repo_root, seed, Locale::En)
            .expect("read-only bootstrap");
        let spec = kanvibe_app::build_native_ui_render_spec(&bootstrap);
        let socket_path = std::env::temp_dir().join(format!(
            "kanvibe-qa-control-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        let _socket_thread =
            kanvibe_app::qa_control::spawn_debug_qa_socket_at_path(socket_path.clone(), spec)
                .expect("spawn debug QA socket")
                .expect("debug QA socket enabled");
        let client = QaControlClient::new(socket_path.clone());

        let mut ping = None;
        for _ in 0..50 {
            match client.request(QaControlCommand::Ping) {
                Ok(response) => {
                    ping = Some(response);
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }

        assert_eq!(ping, Some(QaControlResponse::Pong));
        assert_eq!(
            client
                .request(QaControlCommand::QueryElement {
                    id: "task.qa-task-todo-local".to_owned(),
                })
                .expect("query task element"),
            QaControlResponse::Element {
                id: "task.qa-task-todo-local".to_owned(),
                exists: true,
                text: Some("Draft native board shell".to_owned()),
            }
        );

        let _ = fs::remove_file(socket_path);
    }
}
