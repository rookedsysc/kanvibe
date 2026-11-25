//! TS `projectService task description sync`, `projectService read and recovery isolation`

use std::sync::{Arc, Mutex};

use kanvibe_db::repositories::projects::NewProject;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};
use kanvibe_process::kanvibe_project_state::{
    KanvibeTaskDescription, KanvibeTaskSession, KanvibeTaskSyncState,
};

use super::fakes::{Gate, Harness, wait_until, worktree};
use crate::board_notifier::BoardEvent;
use crate::test_support::new_project;

const API: &str = "/workspace/api";
const LOGIN_PATH: &str = "/workspace/api-worktrees/feature-login";

struct DescriptionFixture {
    harness: Harness,
    root: KanbanTask,
    login: KanbanTask,
}

fn description_fixture(login_session: Option<KanvibeTaskSession>) -> DescriptionFixture {
    let harness = Harness::new();
    let project = harness.project("api", API, None);
    let root = harness.root_task(&project, Some(API));
    let login = harness.task(|task| {
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("feature/login".into());
        task.base_branch = Some("main".into());
        task.worktree_path = Some(LOGIN_PATH.into());
        task.description = Some("옛 설명".into());
        task.session_type = login_session.as_ref().map(|session| session.session_type);
        task.session_name = login_session.map(|session| session.session_name);
    });
    harness.repositories.returning_worktrees(vec![
        worktree(API, "main"),
        worktree(LOGIN_PATH, "feature/login"),
    ]);
    DescriptionFixture {
        harness,
        root,
        login,
    }
}

fn task_file(
    description: Option<&str>,
    priority: Option<TaskPriority>,
    session: Option<KanvibeTaskSession>,
) -> KanvibeTaskSyncState {
    KanvibeTaskSyncState {
        description: description.map(|description| KanvibeTaskDescription {
            schema_version: 1,
            description: Some(Some(description.into())),
            priority: None,
            session: session.clone(),
            updated_at: None,
        }),
        priority: priority.map(Some),
        session,
        ..KanvibeTaskSyncState::default()
    }
}

fn share(harness: &Harness, path: &str, state: KanvibeTaskSyncState) {
    harness
        .state_files
        .sync_states
        .lock()
        .unwrap()
        .insert(path.into(), state);
}

fn zellij_session() -> KanvibeTaskSession {
    KanvibeTaskSession {
        session_type: SessionType::Zellij,
        session_name: "api-feature-login".into(),
    }
}

// parity: TS-main-projectService-892ff2
#[test]
fn description_left_by_another_device_is_imported() {
    let fixture = description_fixture(None);
    share(
        &fixture.harness,
        LOGIN_PATH,
        task_file(Some("결제 실패 로그 원인 추적"), None, None),
    );

    let result = fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(result.has_changed);
    let login = fixture.harness.find_task(&fixture.login.id).unwrap();
    assert_eq!(
        login.description.as_deref(),
        Some("결제 실패 로그 원인 추적")
    );
}

// parity: TS-main-projectService-970e4a
#[test]
fn different_shared_session_is_followed_after_saving_and_the_old_terminal_is_detached() {
    let fixture = description_fixture(None);
    share(
        &fixture.harness,
        LOGIN_PATH,
        task_file(Some("옛 설명"), None, Some(zellij_session())),
    );
    let saved_at_broadcast: Arc<Mutex<Option<KanbanTask>>> = Arc::default();
    let (database, sink, login_id) = (
        Arc::clone(&fixture.harness.db.database),
        Arc::clone(&saved_at_broadcast),
        fixture.login.id.clone(),
    );
    let _subscription = fixture.harness.board.subscribe(move |event| {
        if matches!(event, BoardEvent::TaskSessionChanged(_)) {
            let task =
                kanvibe_db::repositories::tasks::find_task(&database.connection(), &login_id)
                    .unwrap();
            *sink.lock().unwrap() = task;
        }
    });

    let result = fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(result.has_changed);
    let login = fixture.harness.find_task(&fixture.login.id).unwrap();
    assert_eq!(login.session_type, Some(SessionType::Zellij));
    assert_eq!(login.session_name.as_deref(), Some("api-feature-login"));
    let detached = fixture.harness.sessions.detached.lock().unwrap().clone();
    assert_eq!(
        detached,
        [(
            fixture.login.id.clone(),
            "follow-shared-task-session".to_string()
        )]
    );
    let broadcast_state = saved_at_broadcast.lock().unwrap().clone().unwrap();
    assert_eq!(broadcast_state.session_type, Some(SessionType::Zellij));
}

// parity: TS-main-projectService-6a8ee0
#[test]
fn same_shared_session_is_neither_saved_nor_detached() {
    let fixture = description_fixture(Some(zellij_session()));
    share(
        &fixture.harness,
        LOGIN_PATH,
        KanvibeTaskSyncState {
            session: Some(zellij_session()),
            ..KanvibeTaskSyncState::default()
        },
    );

    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let installs = fixture.harness.hooks.install_calls.lock().unwrap().clone();
    assert!(
        installs
            .iter()
            .all(|(_, task_id, ..)| *task_id != fixture.login.id)
    );
    assert!(fixture.harness.sessions.detached.lock().unwrap().is_empty());
    let events = fixture.harness.events.events();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, BoardEvent::TaskSessionChanged(_)))
    );
}

// parity: TS-main-projectService-e552e9
#[test]
fn missing_task_file_keeps_the_db_description() {
    let fixture = description_fixture(None);

    let result = fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(!result.has_changed);
    let login = fixture.harness.find_task(&fixture.login.id).unwrap();
    assert_eq!(login.description.as_deref(), Some("옛 설명"));
}

fn project_priority(priority: TaskPriority) -> KanvibeTaskSyncState {
    KanvibeTaskSyncState {
        project_priority: Some(Some(priority)),
        ..KanvibeTaskSyncState::default()
    }
}

// parity: TS-main-projectService-131fc5
#[test]
fn task_priority_wins_over_project_priority() {
    let fixture = description_fixture(None);
    let mut login_state = task_file(None, Some(TaskPriority::Low), None);
    login_state.project_priority = Some(Some(TaskPriority::High));
    share(&fixture.harness, LOGIN_PATH, login_state);

    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let login = fixture.harness.find_task(&fixture.login.id).unwrap();
    assert_eq!(login.priority, Some(TaskPriority::Low));
}

// parity: TS-main-projectService-5a8cce
#[test]
fn project_priority_is_imported_when_no_task_priority_exists() {
    let fixture = description_fixture(None);
    share(&fixture.harness, API, project_priority(TaskPriority::High));
    share(
        &fixture.harness,
        LOGIN_PATH,
        project_priority(TaskPriority::High),
    );

    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert_eq!(
        fixture
            .harness
            .find_task(&fixture.root.id)
            .unwrap()
            .priority,
        Some(TaskPriority::High)
    );
    assert_eq!(
        fixture
            .harness
            .find_task(&fixture.login.id)
            .unwrap()
            .priority,
        None
    );
}

const LATENCY_REPO: &str = "/latency/api";
const LATENCY_HOST: &str = "latency-host";

/// 이미 복구된 원격 프로젝트: 기본 task, 공유 상태 파일, 같은 공유 색상이 모두 있다
fn recovered_project(harness: &Harness, repo_path: &str, ssh_host: &str) -> Project {
    let project = harness.db.insert_project(NewProject {
        ssh_host: Some(ssh_host.into()),
        icon_data_url: Some("data:icon".into()),
        color: Some("blue".into()),
        ..new_project("api", repo_path)
    });
    harness
        .state_files
        .statuses
        .lock()
        .unwrap()
        .insert(repo_path.into(), TaskStatus::Todo);
    harness
        .state_files
        .shared_colors
        .lock()
        .unwrap()
        .insert(repo_path.into(), "blue".into());
    project
}

fn latency_fixture() -> (Harness, Project, KanbanTask) {
    let harness = Harness::new();
    let project = recovered_project(&harness, LATENCY_REPO, LATENCY_HOST);
    let root = harness.task(|task| {
        task.title = "main".into();
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("main".into());
        task.base_branch = Some("main".into());
        task.worktree_path = Some(LATENCY_REPO.into());
        task.ssh_host = Some(LATENCY_HOST.into());
        task.session_type = Some(SessionType::Herdr);
        task.session_name = Some("kanvibe:api".into());
    });
    harness
        .repositories
        .on_worktree_list(|repo_path, _| vec![worktree(repo_path, "main")]);
    (harness, project, root)
}

/// 첫 worktree 조회 한 번만 관문에 묶는다(TS `mockImplementationOnce`)
fn block_first_worktree_list(harness: &Harness, empty_after: bool) -> Arc<Gate> {
    let gate = Arc::new(Gate::default());
    let first_gate = Arc::clone(&gate);
    let has_blocked = Arc::new(Mutex::new(false));
    harness.repositories.on_worktree_list(move |repo_path, _| {
        let is_first = !std::mem::replace(&mut *has_blocked.lock().unwrap(), true);
        if is_first {
            first_gate.wait();
            return vec![worktree(repo_path, "main")];
        }
        if empty_after {
            Vec::new()
        } else {
            vec![worktree(repo_path, "main")]
        }
    });
    gate
}

// parity: TS-main-projectService-a29cc8
#[test]
fn repeated_snapshot_reads_dispatch_no_remote_work() {
    let (harness, project, _) = latency_fixture();

    let expected = [project];
    assert_eq!(harness.service.get_all_projects().unwrap(), expected);
    assert_eq!(harness.service.get_all_projects().unwrap(), expected);
    harness.service.join_background_jobs();

    assert!(harness.repositories.checked_paths().is_empty());
    assert!(harness.repositories.listed_repo_paths().is_empty());
    assert!(harness.repositories.command_texts().is_empty());
    assert!(harness.state_files.persisted.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-d91b32
#[test]
fn unchanged_root_state_avoids_status_and_exclude_writes() {
    let (harness, project, root) = latency_fixture();

    let result = harness
        .service
        .sync_registered_project_worktrees(Some(&[project]))
        .unwrap();

    assert!(result.errors.is_empty());
    assert_eq!(harness.tasks(), [root]);
    assert!(harness.state_files.persisted.lock().unwrap().is_empty());
    let commands = harness.repositories.command_texts();
    assert!(
        commands
            .iter()
            .all(|command| !command.contains("--git-common-dir"))
    );
}

// parity: TS-main-projectService-63eb9a
#[test]
fn missing_shared_status_is_persisted_during_recovery() {
    let (harness, project, root) = latency_fixture();
    harness.state_files.statuses.lock().unwrap().clear();

    let result = harness
        .service
        .sync_registered_project_worktrees(Some(&[project]))
        .unwrap();

    assert!(result.errors.is_empty());
    let persisted = harness.state_files.persisted.lock().unwrap().clone();
    assert_eq!(
        persisted,
        [(
            LATENCY_REPO.to_string(),
            root.id,
            TaskStatus::Todo,
            Some(LATENCY_HOST.to_string())
        )]
    );
}

// parity: TS-main-projectService-e22f9c
#[test]
fn concurrent_recovery_of_the_same_project_shares_its_scan() {
    let (harness, project, _) = latency_fixture();
    let gate = block_first_worktree_list(&harness, false);
    let projects = [project];

    let (left, right, scans_while_blocked) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(&projects))
        });
        wait_until(|| !harness.repositories.listed_repo_paths().is_empty());
        let second = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(&projects))
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let scans_while_blocked = harness.repositories.listed_repo_paths().len();
        gate.open();
        (
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
            scans_while_blocked,
        )
    });

    assert_eq!(scans_while_blocked, 1);
    assert_eq!(left, right);
    assert!(left.errors.is_empty());
}

// parity: TS-main-projectService-50ce55
#[test]
fn recovery_serializes_one_host_while_another_host_progresses() {
    let (harness, project, _) = latency_fixture();
    let other = recovered_project(&harness, "/latency/other", LATENCY_HOST);
    let independent = recovered_project(&harness, "/latency/independent", "another-host");
    let gate = block_first_worktree_list(&harness, true);

    let scans_while_blocked = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(std::slice::from_ref(&project)))
        });
        wait_until(|| !harness.repositories.listed_repo_paths().is_empty());
        let second = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(std::slice::from_ref(&other)))
        });
        let third = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(std::slice::from_ref(&independent)))
        });
        wait_until(|| {
            harness
                .repositories
                .listed_repo_paths()
                .contains(&"/latency/independent".to_string())
        });
        let scans = harness.repositories.listed_repo_paths();
        gate.open();
        for handle in [first, second, third] {
            handle.join().unwrap().unwrap();
        }
        scans
    });

    assert!(scans_while_blocked.contains(&LATENCY_REPO.to_string()));
    assert!(scans_while_blocked.contains(&"/latency/independent".to_string()));
    assert!(!scans_while_blocked.contains(&"/latency/other".to_string()));
}

// parity: TS-main-projectService-2b9bc9
#[test]
fn project_deleted_while_queued_on_its_host_is_not_repaired() {
    let (harness, project, _) = latency_fixture();
    let deleting = recovered_project(&harness, "/latency/deleted", LATENCY_HOST);
    let gate = block_first_worktree_list(&harness, true);

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(std::slice::from_ref(&project)))
        });
        wait_until(|| !harness.repositories.listed_repo_paths().is_empty());
        let queued = scope.spawn(|| {
            harness
                .service
                .sync_registered_project_worktrees(Some(std::slice::from_ref(&deleting)))
        });
        wait_until(|| {
            harness
                .repositories
                .checked_paths()
                .contains(&"/latency/deleted".to_string())
        });
        assert!(harness.service.delete_project(&deleting.id).unwrap());
        gate.open();
        first.join().unwrap().unwrap();
        queued.join().unwrap().unwrap();
    });

    assert!(
        !harness
            .repositories
            .listed_repo_paths()
            .contains(&"/latency/deleted".to_string())
    );
    assert!(
        harness
            .tasks()
            .iter()
            .all(|task| task.project_id.as_deref() != Some(deleting.id.as_str()))
    );
}

// parity: TS-main-projectService-7bc5ab
#[test]
fn failed_recovery_is_retried_and_restores_the_missing_root() {
    let harness = Harness::new();
    let project = recovered_project(&harness, LATENCY_REPO, LATENCY_HOST);
    harness
        .repositories
        .on_worktree_list(|repo_path, _| vec![worktree(repo_path, "main")]);
    *harness.state_files.next_read_error.lock().unwrap() = Some("database offline".into());
    let projects = [project.clone()];

    let failed = harness
        .service
        .sync_registered_project_worktrees(Some(&projects))
        .unwrap();
    assert!(failed.errors.join(" ").contains("database offline"));
    assert!(harness.tasks().is_empty());

    let retried = harness
        .service
        .sync_registered_project_worktrees(Some(&projects))
        .unwrap();
    assert!(retried.errors.is_empty());
    let root = harness.branch_task("main").unwrap();
    assert_eq!(root.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(root.status, TaskStatus::Todo);
    assert!(retried.has_changed);
}
