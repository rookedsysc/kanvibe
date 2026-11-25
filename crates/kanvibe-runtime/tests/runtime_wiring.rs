//! 실제 어댑터로 만든 런타임 그래프를 끝까지 지나는 통합 테스트.
//! tmux가 필요한 테스트는 tmux가 없으면 건너뛰고, tmux는 임시 `TMUX_TMPDIR`의 격리 서버만 쓴다.

mod support;

use std::path::Path;

use kanvibe_domain::task::{SessionType, TaskStatus};
use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::task_cli::service::{RequestBody, TaskCliService};
use kanvibe_services::hook_status::HookStatusInput;
use serde_json::json;

use support::{
    REPOSITORY_NAME, RuntimeFixture, app_setting, assert_runtime_values_absent, create_branch_task,
    environment_keys, expected_task_token, has_tmux, read_own_target, register_sample_project,
};

fn migration_rows(runtime: &kanvibe_runtime::DesktopRuntime) -> Vec<(i64, i64, String)> {
    let connection = runtime.database().connection();
    let mut statement = connection
        .prepare("SELECT id, timestamp, name FROM migrations ORDER BY id")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn request_body(value: serde_json::Value) -> RequestBody {
    value.as_object().unwrap().clone()
}

#[test]
fn runtime_opens_a_fresh_database_and_reopens_it_without_rerunning_migrations() {
    let fixture = RuntimeFixture::new();
    let first_runtime = fixture.open_runtime();
    let applied_migrations = migration_rows(&first_runtime);
    first_runtime
        .app_settings()
        .set_app_setting("runtime_reopen_marker", "kept")
        .unwrap();
    assert!(!applied_migrations.is_empty());
    assert!(fixture.path("data/kanvibe.db").exists());
    drop(first_runtime);

    let reopened_runtime = fixture.open_runtime();

    assert_eq!(migration_rows(&reopened_runtime), applied_migrations);
    assert_eq!(
        app_setting(&reopened_runtime, "runtime_reopen_marker").as_deref(),
        Some("kept")
    );
}

#[test]
fn tmux_task_gets_worktree_session_and_signed_hook_target() {
    if !has_tmux() {
        eprintln!("tmux가 없어 건너뛴다");
        return;
    }
    let fixture = RuntimeFixture::new();
    let repository = fixture.create_git_repository();
    let runtime = fixture.open_runtime();
    let project = register_sample_project(&runtime, &fixture);

    let task = create_branch_task(&runtime, &project, "feat/runtime", SessionType::Tmux);

    let expected_worktree = format!(
        "{}/{REPOSITORY_NAME}__worktrees/feat-runtime",
        fixture.workspace()
    );
    assert_eq!(project.repo_path, repository.to_string_lossy());
    assert_eq!(
        task.worktree_path.as_deref(),
        Some(expected_worktree.as_str())
    );
    assert!(Path::new(&expected_worktree).join("README.md").exists());
    let session_name = format!("{REPOSITORY_NAME}-feat-runtime");
    assert_eq!(task.session_name.as_deref(), Some(session_name.as_str()));

    let own_target = read_own_target(&expected_worktree, &task.id);
    assert_eq!(own_target["token"], expected_task_token(&runtime, &task.id));
    assert_eq!(
        own_target["clientId"].as_str(),
        app_setting(&runtime, "hook_client_id").as_deref()
    );
    assert_eq!(
        own_target["url"],
        format!("http://localhost:{}", fixture.hook_server_port)
    );

    let prepared = runtime
        .terminal_bridge()
        .prepare_task_terminal(&task)
        .unwrap();
    assert!(prepared.is_prepared, "{prepared:?}");
    let has_session = fixture.tmux(&["has-session", "-t", &format!("={session_name}")]);
    assert!(has_session.status.success(), "{has_session:?}");
}

// parity: TS-main-runtimeEnvironment-e208fe, TS-main-runtimeEnvironment-c26eaf
#[test]
fn the_launch_hook_port_reaches_installed_hooks_without_the_port_variable() {
    let fixture = RuntimeFixture::new();
    fixture.create_git_repository();
    let process_port = std::env::var_os("PORT");
    // 시작 환경 스냅샷에는 다른 값의 PORT(48123)가 있다. hook 포트는 실행 설정으로만 받는다
    let runtime = fixture.open_runtime();
    let project = register_sample_project(&runtime, &fixture);

    let task = create_branch_task(&runtime, &project, "feat/port", SessionType::Terminal);

    let own_target = read_own_target(task.worktree_path.as_deref().unwrap(), &task.id);
    assert_ne!(fixture.hook_server_port, 48123);
    assert_eq!(
        own_target["url"],
        format!("http://localhost:{}", fixture.hook_server_port)
    );
    assert_eq!(
        runtime.hook_server_address().port(),
        fixture.hook_server_port
    );
    assert_eq!(std::env::var_os("PORT"), process_port);
}

#[test]
fn hook_status_update_is_accepted_only_with_the_task_token() {
    let fixture = RuntimeFixture::new();
    fixture.create_git_repository();
    let runtime = fixture.open_runtime();
    let project = register_sample_project(&runtime, &fixture);
    let task = create_branch_task(&runtime, &project, "feat/hook", SessionType::Terminal);
    let worktree_path = task.worktree_path.clone().unwrap();
    let token = read_own_target(&worktree_path, &task.id)["token"]
        .as_str()
        .unwrap()
        .to_string();
    let input = HookStatusInput {
        task_id: task.id.clone(),
        status: "PROGRESS".into(),
        agent_session: None,
    };

    let rejected = runtime
        .hook_status()
        .update_hook_task_status(&input, Some("Bearer wrong-token"), None)
        .unwrap_err();
    let updated = runtime
        .hook_status()
        .update_hook_task_status(&input, Some(&format!("Bearer {token}")), None)
        .unwrap();

    assert_eq!(rejected.status_code, 401);
    assert_eq!(updated.status, TaskStatus::Progress);
    assert_eq!(updated.project_name, REPOSITORY_NAME);
    let stored = runtime.kanban().get_task_by_id(&task.id).unwrap().unwrap();
    assert_eq!(stored.task.status, TaskStatus::Progress);
    let status_file =
        std::fs::read_to_string(Path::new(&worktree_path).join(".kanvibe/status.json")).unwrap();
    assert!(status_file.contains("\"progress\""), "{status_file}");
}

#[test]
fn task_cli_lists_and_creates_tasks_through_the_desktop_backend() {
    let fixture = RuntimeFixture::new();
    fixture.create_git_repository();
    let runtime = fixture.open_runtime();
    let project = register_sample_project(&runtime, &fixture);
    let context_task = create_branch_task(&runtime, &project, "feat/cli", SessionType::Terminal);
    let authorization = format!("Bearer {}", expected_task_token(&runtime, &context_task.id));
    let service = TaskCliService::new(runtime.task_cli_backend().as_ref());
    let list_body = request_body(json!({ "contextTaskId": context_task.id }));

    let unauthorized = service
        .list_tasks_from_cli(Some("Bearer wrong-token"), &list_body, None)
        .unwrap_err();
    let created = service
        .create_task_from_cli(
            Some(&authorization),
            &request_body(json!({
                "contextTaskId": context_task.id,
                "branchName": "feat/cli-child",
                "baseBranch": "main",
            })),
            None,
        )
        .unwrap();
    let listed = service
        .list_tasks_from_cli(Some(&authorization), &list_body, None)
        .unwrap();

    assert_eq!(unauthorized.status_code(), Some(401));
    assert_eq!(created.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(created.session_type, Some(SessionType::Terminal));
    assert_eq!(
        created.worktree_path,
        Some(format!(
            "{}/{REPOSITORY_NAME}__worktrees/feat-cli-child",
            fixture.workspace()
        ))
    );
    let listed_branches: Vec<_> = listed
        .iter()
        .filter_map(|task| task.branch_name.as_deref())
        .collect();
    assert!(listed_branches.contains(&"feat/cli"), "{listed_branches:?}");
    assert!(
        listed_branches.contains(&"feat/cli-child"),
        "{listed_branches:?}"
    );
}

// parity: TS-main-runtimeEnvironment-08531a
#[test]
fn server_runtime_values_never_reach_child_processes() {
    let fixture = RuntimeFixture::new();
    let runtime = fixture.open_runtime();

    let child_listing = runtime
        .shell_runner()
        .exec_git("env", None, &ExecGitOptions::default())
        .unwrap();
    let pty_keys: Vec<String> = runtime
        .terminal_environment()
        .shell_environment
        .keys()
        .cloned()
        .collect();

    assert_runtime_values_absent(&environment_keys(&child_listing), "로컬 명령 환경");
    assert_runtime_values_absent(&pty_keys, "PTY 환경");
    assert!(child_listing.contains(&format!("HOME={}", fixture.home())));
}

#[test]
fn server_runtime_values_never_reach_the_tmux_session() {
    if !has_tmux() {
        eprintln!("tmux가 없어 건너뛴다");
        return;
    }
    let fixture = RuntimeFixture::new();
    fixture.create_git_repository();
    let runtime = fixture.open_runtime();
    let project = register_sample_project(&runtime, &fixture);
    let task = create_branch_task(&runtime, &project, "feat/env", SessionType::Tmux);
    runtime
        .terminal_bridge()
        .prepare_task_terminal(&task)
        .unwrap();
    let pane_target = format!("={REPOSITORY_NAME}-feat-env:");

    let global_environment = fixture.tmux(&["show-environment", "-g"]);
    let pane_pid = fixture.tmux(&["display-message", "-p", "-t", &pane_target, "#{pane_pid}"]);

    let global_listing = String::from_utf8_lossy(&global_environment.stdout).into_owned();
    assert_runtime_values_absent(&environment_keys(&global_listing), "tmux 전역 환경");
    let pane_pid = String::from_utf8_lossy(&pane_pid.stdout).trim().to_string();
    assert!(pane_pid.parse::<u32>().is_ok(), "pane pid: {pane_pid:?}");
    let process_environment = Path::new("/proc").join(&pane_pid).join("environ");
    if let Ok(bytes) = std::fs::read(&process_environment) {
        let pane_listing = String::from_utf8_lossy(&bytes).into_owned();
        assert_runtime_values_absent(&environment_keys(&pane_listing), "tmux pane 프로세스");
    }
}
