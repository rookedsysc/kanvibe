//! TS `projectService local hook installation` 뒷부분: 스캔·background sync의 worktree 등록, 공유 상태 반영, 직렬화

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kanvibe_domain::task::{SessionType, TaskStatus};
use kanvibe_process::worktree::{format_project_branch_session_name, sanitize_herdr_session_name};

use super::fakes::{Gate, Harness, wait_until, worktree};

const API: &str = "/workspace/api";

fn install_calls(harness: &Harness) -> Vec<(String, String, Option<String>)> {
    let calls = harness.hooks.install_calls.lock().unwrap();
    calls
        .iter()
        .map(|(path, task_id, _, host)| (path.clone(), task_id.clone(), host.clone()))
        .collect()
}

fn scheduled_installs(harness: &Harness) -> Vec<(String, String, Option<String>)> {
    let calls = harness.hooks.scheduled_installs.lock().unwrap();
    calls
        .iter()
        .map(|(path, task_id, _, host)| (path.clone(), task_id.clone(), host.clone()))
        .collect()
}

#[test]
fn scanned_local_todo_worktree_task_gets_hooks() {
    let harness = Harness::new();
    let worktree_path = "/workspace/api-worktrees/feature-login";
    *harness.repositories.scanned_repos.lock().unwrap() = vec![API.into()];
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-login")]);

    let result = harness
        .service
        .scan_and_register_projects("/workspace", None)
        .unwrap();

    assert!(result.worktree_tasks.contains(&"feature-login".to_string()));
    let task = harness.branch_task("feature-login").unwrap();
    assert!(install_calls(&harness).contains(&(worktree_path.to_string(), task.id, None)));
}

#[test]
fn project_scan_does_not_register_git_submodules_separately() {
    let harness = Harness::new();
    *harness.repositories.scanned_repos.lock().unwrap() =
        vec!["/workspace/app".into(), "/workspace/app/packages/ui".into()];
    harness.repositories.on_command(|command| {
        let is_ui = command.contains("'/workspace/app/packages/ui'");
        if command.contains("rev-parse --show-superproject-working-tree") {
            return Ok(if is_ui {
                "/workspace/app".into()
            } else {
                String::new()
            });
        }
        if command.contains("rev-parse --path-format=absolute --git-common-dir") {
            return Ok(match is_ui {
                true => "/workspace/app/.git/modules/packages/ui".into(),
                false => "/workspace/app/.git".into(),
            });
        }
        Ok(String::new())
    });

    let result = harness
        .service
        .scan_and_register_projects("/workspace", None)
        .unwrap();

    assert_eq!(result.registered, ["app"]);
    let projects = harness.projects();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].repo_path, "/workspace/app");
    let branch_calls = harness
        .repositories
        .default_branch_calls
        .lock()
        .unwrap()
        .clone();
    assert!(branch_calls.contains(&("/workspace/app".to_string(), None)));
    assert!(!branch_calls.contains(&("/workspace/app/packages/ui".to_string(), None)));
}

#[test]
fn project_scan_bounds_concurrent_git_checks_and_keeps_the_first_order_of_shared_repos() {
    let harness = Harness::new();
    let mut scanned: Vec<String> = (0..20)
        .map(|index| format!("/workspace/repo-{index:02}"))
        .collect();
    scanned.push("/workspace/repo-00__worktrees/feature".into());
    *harness.repositories.scanned_repos.lock().unwrap() = scanned;
    let active_checks = Arc::new(AtomicUsize::new(0));
    let max_active_checks = Arc::new(AtomicUsize::new(0));
    let (active, max_active) = (Arc::clone(&active_checks), Arc::clone(&max_active_checks));
    harness.repositories.on_command(move |command| {
        if !command.contains("rev-parse") {
            return Ok(String::new());
        }
        let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
        max_active.fetch_max(now_active, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(5));
        active.fetch_sub(1, Ordering::SeqCst);
        if !command.contains("--git-common-dir") {
            return Ok(String::new());
        }
        let repo_name = command
            .split("'/workspace/")
            .nth(1)
            .and_then(|rest| rest.split(['\'', '_']).next())
            .unwrap_or_default();
        Ok(format!("/workspace/{repo_name}/.git"))
    });

    let result = harness
        .service
        .scan_and_register_projects("/workspace", None)
        .unwrap();

    assert!(max_active_checks.load(Ordering::SeqCst) <= 8);
    let expected: Vec<String> = (0..20).map(|index| format!("repo-{index:02}")).collect();
    assert_eq!(result.registered, expected);
}

fn remote_worktree_scan_harness() -> (Harness, &'static str) {
    let harness = Harness::new();
    let worktree_path = "/remote/repo__worktrees/feature-login";
    let project = harness.project("repo", "/remote/repo", Some("remote-host"));
    harness.root_task(&project, None);
    *harness.repositories.scanned_repos.lock().unwrap() = vec![worktree_path.into()];
    harness.repositories.on_command(|command| {
        Ok(
            match command.contains("rev-parse --path-format=absolute --git-common-dir") {
                true => "/remote/repo/.git".into(),
                false => String::new(),
            },
        )
    });
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-login")]);
    (harness, worktree_path)
}

#[test]
fn scanning_only_a_worktree_path_keeps_the_remote_host() {
    for has_session in [false, true] {
        let (harness, worktree_path) = remote_worktree_scan_harness();
        harness
            .sessions
            .on_alive(move |session_type, _| has_session && session_type == SessionType::Tmux);

        let result = harness
            .service
            .scan_and_register_projects("/remote/workspace", Some("remote-host"))
            .unwrap();

        assert!(result.worktree_tasks.contains(&"feature-login".to_string()));
        let task = harness.branch_task("feature-login").unwrap();
        assert_eq!(task.ssh_host.as_deref(), Some("remote-host"));
        assert_eq!(task.session_type, has_session.then_some(SessionType::Tmux));
        let lists = harness.repositories.worktree_lists.lock().unwrap().clone();
        assert!(lists.contains(&("/remote/repo".to_string(), Some("remote-host".to_string()))));
        assert_eq!(
            scheduled_installs(&harness),
            [(
                worktree_path.to_string(),
                task.id,
                Some("remote-host".to_string())
            )]
        );
        assert!(install_calls(&harness).is_empty());
    }
}

#[test]
fn alive_herdr_session_is_used_when_tmux_is_absent() {
    let (harness, _) = remote_worktree_scan_harness();
    harness.sessions.on_alive(|session_type, name| {
        session_type == SessionType::Herdr && !name.starts_with("kanvibe:")
    });

    let result = harness
        .service
        .scan_and_register_projects("/remote/workspace", Some("remote-host"))
        .unwrap();

    assert!(result.worktree_tasks.contains(&"feature-login".to_string()));
    let task = harness.branch_task("feature-login").unwrap();
    let branch_session = format_project_branch_session_name("/remote/repo", "feature-login");
    assert_eq!(task.ssh_host.as_deref(), Some("remote-host"));
    assert_eq!(task.session_type, Some(SessionType::Herdr));
    assert_eq!(
        task.session_name,
        Some(sanitize_herdr_session_name(&branch_session))
    );
}

#[test]
fn remote_scan_registers_worktrees_without_reading_root_hook_status() {
    let harness = Harness::new();
    *harness.repositories.scanned_repos.lock().unwrap() = vec!["/remote/repo".into()];
    *harness.hooks.status_error.lock().unwrap() =
        Some("remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22".into());
    harness.repositories.returning_worktrees(vec![worktree(
        "/remote/repo__worktrees/feature-login",
        "feature-login",
    )]);

    let result = harness
        .service
        .scan_and_register_projects("/remote/workspace", Some("remote-host"))
        .unwrap();

    assert!(result.worktree_tasks.contains(&"feature-login".to_string()));
    assert!(harness.hooks.status_calls.lock().unwrap().is_empty());
}

#[test]
fn scan_follow_up_only_touches_repositories_found_by_this_scan() {
    let harness = Harness::new();
    harness.project("web", "/workspace/web", None);
    *harness.repositories.scanned_repos.lock().unwrap() = vec![API.into()];

    harness
        .service
        .scan_and_register_projects("/workspace", None)
        .unwrap();

    let listed = harness.repositories.listed_repo_paths();
    assert!(listed.contains(&API.to_string()));
    assert!(!listed.contains(&"/workspace/web".to_string()));
}

#[test]
fn orphan_task_with_only_the_same_branch_name_is_not_reused() {
    let harness = Harness::new();
    let worktree_path = "/workspace/api-worktrees/feature-login";
    let orphan = harness.task(|task| {
        task.branch_name = Some("feature-login".into());
        task.worktree_path = Some("/other/repo__worktrees/feature-login".into());
    });
    *harness.repositories.scanned_repos.lock().unwrap() = vec![API.into()];
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-login")]);

    harness
        .service
        .scan_and_register_projects("/workspace", None)
        .unwrap();

    let project_id = harness.projects()[0].id.clone();
    let created = harness
        .tasks()
        .into_iter()
        .find(|task| {
            task.project_id.as_deref() == Some(project_id.as_str())
                && task.branch_name.as_deref() == Some("feature-login")
        })
        .unwrap();
    assert_ne!(created.id, orphan.id);
    assert_eq!(created.worktree_path.as_deref(), Some(worktree_path));
    assert_eq!(harness.find_task(&orphan.id).unwrap().project_id, None);
    assert!(install_calls(&harness).contains(&(worktree_path.to_string(), created.id, None)));
}

#[test]
fn background_sync_does_not_repair_existing_root_hooks() {
    let harness = Harness::new();
    let project = harness.project("api", API, None);
    harness.root_task(&project, Some(API));
    harness
        .repositories
        .returning_worktrees(vec![worktree(API, "main")]);

    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(!result.has_changed);
    assert!(harness.hooks.status_calls.lock().unwrap().is_empty());
    assert!(harness.hooks.ensure_calls.lock().unwrap().is_empty());
    assert!(install_calls(&harness).is_empty());
}

#[test]
fn non_git_project_path_is_skipped_without_writing_state_files() {
    let harness = Harness::new();
    harness.project(
        "removed-workspace",
        "/remote/removed-workspace",
        Some("remote-mac"),
    );
    harness.repositories.on_command(|command| {
        Ok(match command.contains("rev-parse --is-inside-work-tree") {
            true => "__KANVIBE_NOT_GIT_REPOSITORY__".into(),
            false => String::new(),
        })
    });

    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(result.errors.is_empty());
    assert!(!result.has_changed);
    assert!(harness.repositories.listed_repo_paths().is_empty());
    assert!(harness.tasks().is_empty());
    assert!(harness.state_files.persisted.lock().unwrap().is_empty());
}

#[test]
fn ssh_failure_in_the_git_probe_does_not_skip_the_sync() {
    let harness = Harness::new();
    let project = harness.project("api", "/remote/api", Some("remote-mac"));
    harness.root_task(&project, Some("/remote/api"));
    harness.repositories.on_command(|command| {
        if command.contains("rev-parse --is-inside-work-tree") {
            return Err(
                "remote-mac 원격 명령 실패: Connection reset by 100.99.109.53 port 22".into(),
            );
        }
        Ok(String::new())
    });

    harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let lists = harness.repositories.worktree_lists.lock().unwrap().clone();
    assert!(lists.contains(&("/remote/api".to_string(), Some("remote-mac".to_string()))));
}

fn shared_color_harness() -> (Harness, kanvibe_domain::project::Project) {
    let harness = Harness::new();
    let mut project = harness.project("api", API, None);
    project.color = Some("#65D08A".into());
    kanvibe_db::repositories::projects::update_project(&harness.db.database.connection(), &project)
        .unwrap();
    harness.root_task(&project, Some(API));
    harness
        .repositories
        .returning_worktrees(vec![worktree(API, "main")]);
    harness
        .state_files
        .shared_colors
        .lock()
        .unwrap()
        .insert(API.into(), "#0064FF".into());
    (harness, project)
}

#[test]
fn user_color_edit_during_shared_color_sync_is_not_overwritten() {
    let (harness, project) = shared_color_harness();
    // sync가 읽어 둔 색상과 달리 그 사이 사용자가 DB 색상을 바꾼 상태
    let mut edited = project.clone();
    edited.color = Some("#111111".into());
    kanvibe_db::repositories::projects::update_project(&harness.db.database.connection(), &edited)
        .unwrap();

    let result = harness
        .service
        .sync_registered_project_worktrees(Some(std::slice::from_ref(&project)))
        .unwrap();

    assert!(!result.has_changed);
    assert_eq!(harness.projects()[0].color.as_deref(), Some("#111111"));
}

#[test]
fn unchanged_shared_color_is_applied_to_the_db_and_board() {
    let (harness, _) = shared_color_harness();

    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(result.has_changed);
    assert_eq!(harness.projects()[0].color.as_deref(), Some("#0064FF"));
}

#[test]
fn remote_background_sync_does_not_schedule_root_hook_repair() {
    let harness = Harness::new();
    let project = harness.project("api", "/remote/api", Some("remote-host"));
    harness.root_task(&project, Some("/remote/api"));
    harness
        .repositories
        .returning_worktrees(vec![worktree("/remote/api", "main")]);

    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(!result.has_changed);
    assert!(harness.hooks.status_calls.lock().unwrap().is_empty());
    assert!(harness.hooks.ensure_calls.lock().unwrap().is_empty());
    assert!(scheduled_installs(&harness).is_empty());
    assert!(install_calls(&harness).is_empty());
}

#[test]
fn background_sync_registers_a_new_worktree_as_a_todo_task() {
    let harness = Harness::new();
    let worktree_path = "/workspace/api__worktrees/feature-sync";
    let project = harness.project("api", API, None);
    harness.root_task(&project, None);
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-sync")]);

    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let task = harness.branch_task("feature-sync").unwrap();
    assert_eq!(result.worktree_tasks, ["feature-sync"]);
    let registered = serde_json::to_value(&result.registered_worktrees).unwrap();
    assert_eq!(
        registered,
        serde_json::json!([{
            "taskId": task.id,
            "projectName": "api",
            "branchName": "feature-sync",
            "worktreePath": worktree_path,
            "sshHost": null,
        }])
    );
    assert_eq!(task.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(task.status, TaskStatus::Todo);
    assert_eq!(
        install_calls(&harness),
        [(worktree_path.to_string(), task.id, None)]
    );
}

#[test]
fn new_worktree_with_an_alive_session_but_no_shared_state_stays_todo() {
    let harness = Harness::new();
    let project = harness.project("api", API, None);
    harness.root_task(&project, Some(API));
    harness.sessions.on_alive(|_, _| true);
    harness.repositories.returning_worktrees(vec![worktree(
        "/workspace/api__worktrees/feature-active",
        "feature-active",
    )]);

    harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let task = harness.branch_task("feature-active").unwrap();
    assert_eq!(task.status, TaskStatus::Todo);
    assert_eq!(
        task.session_name,
        Some(format_project_branch_session_name(API, "feature-active"))
    );
}

/// 기본 브랜치 root task와 기준 브랜치가 정해진 자식 worktree task
fn child_worktree_fixture(
    base_branch: Option<&str>,
    persisted_status: Option<TaskStatus>,
) -> (Harness, kanvibe_domain::task::KanbanTask) {
    let harness = Harness::new();
    let worktree_path = "/workspace/api__worktrees/feature-child";
    let project = harness.project("api", API, None);
    let mut root = harness.root_task(&project, Some(API));
    root.session_type = Some(SessionType::Tmux);
    root.session_name = Some("api-main".into());
    kanvibe_db::repositories::tasks::update_task(&harness.db.database.connection(), &root).unwrap();
    let child = harness.task(|task| {
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("feature-child".into());
        task.base_branch = base_branch.map(str::to_string);
        task.worktree_path = Some(worktree_path.into());
    });
    harness.repositories.returning_worktrees(vec![
        worktree(API, "main"),
        worktree(worktree_path, "feature-child"),
    ]);
    if let Some(status) = persisted_status {
        let mut statuses = harness.state_files.statuses.lock().unwrap();
        statuses.insert(worktree_path.into(), status);
    }
    (harness, child)
}

#[test]
fn worktree_sync_keeps_an_existing_base_branch_and_only_fills_missing_values() {
    let cases = [
        (Some("feature/parent"), None, "feature/parent"),
        (
            Some("feature/parent"),
            Some(TaskStatus::Review),
            "feature/parent",
        ),
        (None, None, "main"),
        (Some(""), None, "main"),
    ];
    for (base_branch, persisted_status, expected_base) in cases {
        let (harness, child) = child_worktree_fixture(base_branch, persisted_status);

        let result = harness
            .service
            .sync_registered_project_worktrees(None)
            .unwrap();

        assert!(result.errors.is_empty());
        let synced = harness.find_task(&child.id).unwrap();
        assert_eq!(synced.base_branch.as_deref(), Some(expected_base));
        assert_eq!(synced.status, persisted_status.unwrap_or(TaskStatus::Todo));
        let should_save = persisted_status.is_some() || base_branch.is_none_or(str::is_empty);
        let has_saved_child = install_calls(&harness)
            .iter()
            .any(|(_, task_id, _)| *task_id == child.id);
        assert_eq!(has_saved_child, should_save);
    }
}

#[test]
fn new_worktree_is_registered_with_the_shared_task_status() {
    let harness = Harness::new();
    let worktree_path = "/workspace/api__worktrees/feature-review";
    let project = harness.project("api", API, None);
    harness.root_task(&project, Some(API));
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-review")]);
    harness
        .state_files
        .statuses
        .lock()
        .unwrap()
        .insert(worktree_path.into(), TaskStatus::Review);

    harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let reads = harness.state_files.sync_state_reads.lock().unwrap().clone();
    assert!(reads.contains(&(Some(worktree_path.to_string()), None, API.to_string())));
    assert_eq!(
        harness.branch_task("feature-review").unwrap().status,
        TaskStatus::Review
    );
}

#[test]
fn existing_worktree_task_follows_the_shared_task_status() {
    let harness = Harness::new();
    let worktree_path = "/workspace/api__worktrees/feature-pending";
    let project = harness.project("api", API, None);
    harness.root_task(&project, Some(API));
    let existing = harness.task(|task| {
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("feature-pending".into());
        task.base_branch = Some("main".into());
        task.worktree_path = Some(worktree_path.into());
    });
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-pending")]);
    harness
        .state_files
        .statuses
        .lock()
        .unwrap()
        .insert(worktree_path.into(), TaskStatus::Pending);

    harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert_eq!(
        harness.find_task(&existing.id).unwrap().status,
        TaskStatus::Pending
    );
}

#[test]
fn registration_keeps_the_default_worktree_status_after_attaching_tmux() {
    let harness = Harness::new();
    harness.use_default_session_type(SessionType::Tmux);
    harness
        .repositories
        .returning_worktrees(vec![worktree(API, "main")]);
    harness
        .state_files
        .statuses
        .lock()
        .unwrap()
        .insert(API.into(), TaskStatus::Review);
    *harness.sessions.created_session.lock().unwrap() = Ok("api-main".into());

    let result = harness.service.register_project("api", API, None).unwrap();

    assert!(result.is_success);
    let creations = harness.sessions.creations.lock().unwrap().clone();
    assert_eq!(
        creations,
        [(
            API.to_string(),
            "main".to_string(),
            SessionType::Tmux,
            None,
            Some(API.to_string())
        )]
    );
    let root = harness.branch_task("main").unwrap();
    assert_eq!(root.session_name.as_deref(), Some("api-main"));
    assert_eq!(root.status, TaskStatus::Review);
}

#[test]
fn orphan_worktree_task_is_linked_with_the_shared_status() {
    let harness = Harness::new();
    let worktree_path = "/workspace/api__worktrees/feature-orphan";
    let project = harness.project("api", API, None);
    harness.root_task(&project, Some(API));
    let orphan = harness.task(|task| {
        task.branch_name = Some("feature-orphan".into());
        task.worktree_path = Some(worktree_path.into());
    });
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-orphan")]);
    harness
        .state_files
        .statuses
        .lock()
        .unwrap()
        .insert(worktree_path.into(), TaskStatus::Done);

    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(
        result
            .worktree_tasks
            .contains(&"feature-orphan".to_string())
    );
    let linked = harness.find_task(&orphan.id).unwrap();
    assert_eq!(linked.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(linked.base_branch.as_deref(), Some("main"));
    assert_eq!(linked.status, TaskStatus::Done);
}

#[test]
fn attaching_the_main_branch_session_keeps_the_shared_status() {
    let harness = Harness::new();
    let project = harness.project("api", API, None);
    let root = harness.root_task(&project, None);
    harness.sessions.on_alive(|_, _| true);
    harness
        .state_files
        .statuses
        .lock()
        .unwrap()
        .insert(API.into(), TaskStatus::Progress);

    harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let attached = harness.find_task(&root.id).unwrap();
    assert_eq!(attached.session_type, Some(SessionType::Tmux));
    assert_eq!(
        attached.session_name,
        Some(format_project_branch_session_name(API, "main"))
    );
    assert_eq!(attached.status, TaskStatus::Progress);
}

#[test]
fn remote_new_worktree_hooks_are_scheduled_in_the_background() {
    let harness = Harness::new();
    let worktree_path = "/remote/api__worktrees/feature-sync";
    let project = harness.project("api", "/remote/api", Some("remote-host"));
    harness.root_task(&project, Some("/remote/api"));
    harness
        .repositories
        .returning_worktrees(vec![worktree(worktree_path, "feature-sync")]);

    let result = harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(result.worktree_tasks.contains(&"feature-sync".to_string()));
    let task = harness.branch_task("feature-sync").unwrap();
    assert_eq!(
        scheduled_installs(&harness),
        [(
            worktree_path.to_string(),
            task.id,
            Some("remote-host".to_string())
        )]
    );
    assert!(install_calls(&harness).is_empty());
}

#[test]
fn background_sync_lists_worktrees_one_project_at_a_time() {
    let harness = Harness::new();
    let first = harness.project("api-a", "/workspace/api-a", None);
    let second = harness.project("api-b", "/workspace/api-b", None);
    let gate = Arc::new(Gate::default());
    let first_calls: Arc<Mutex<Vec<String>>> = Arc::default();
    let (list_gate, seen) = (Arc::clone(&gate), Arc::clone(&first_calls));
    harness.repositories.on_worktree_list(move |repo_path, _| {
        let is_first_call = {
            let mut seen = seen.lock().unwrap();
            let is_first = !seen.contains(&repo_path.to_string());
            if is_first {
                seen.push(repo_path.to_string());
            }
            is_first
        };
        if is_first_call {
            list_gate.wait();
        }
        Vec::new()
    });

    let projects = [first, second];
    std::thread::scope(|scope| {
        let syncing = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(&projects))
        });
        wait_until(|| !first_calls.lock().unwrap().is_empty());
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(*first_calls.lock().unwrap(), ["/workspace/api-a"]);
        gate.open();
        syncing.join().unwrap().unwrap();
    });
    assert_eq!(
        *first_calls.lock().unwrap(),
        ["/workspace/api-a", "/workspace/api-b"]
    );
}

#[test]
fn icon_backfill_runs_in_parallel_before_the_sequential_worktree_scan() {
    let harness = Harness::new();
    let first = harness.project_without_icon("api-a", "/workspace/api-a");
    let second = harness.project_without_icon("api-b", "/workspace/api-b");
    let kept = harness.project("api-c", "/workspace/api-c", None);
    let gate = Arc::new(Gate::default());
    let icon_gate = Arc::clone(&gate);
    harness.icons.on_lookup(move |repo_path| {
        icon_gate.wait();
        Some(format!("data:image/png;base64,{repo_path}"))
    });

    let projects = [first, second, kept];
    let result = std::thread::scope(|scope| {
        let syncing = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(&projects))
        });
        wait_until(|| harness.icons.lookups.lock().unwrap().len() == 2);
        let mut lookups = harness.icons.lookups.lock().unwrap().clone();
        lookups.sort();
        assert_eq!(lookups, ["/workspace/api-a", "/workspace/api-b"]);
        assert!(harness.repositories.listed_repo_paths().is_empty());
        gate.open();
        syncing.join().unwrap().unwrap()
    });

    assert!(result.has_changed);
    let icons: Vec<_> = harness
        .projects()
        .into_iter()
        .map(|project| (project.repo_path, project.icon_data_url))
        .collect();
    assert!(icons.contains(&(
        "/workspace/api-a".into(),
        Some("data:image/png;base64,/workspace/api-a".into())
    )));
    assert!(icons.contains(&(
        "/workspace/api-b".into(),
        Some("data:image/png;base64,/workspace/api-b".into())
    )));
    assert!(icons.contains(&("/workspace/api-c".into(), Some("data:existing".into()))));
}
