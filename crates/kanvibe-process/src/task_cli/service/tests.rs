use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};
use serde_json::json;

use super::*;

const AUTHORIZATION: Option<&str> = Some("Bearer valid-token");
const CONTEXT_TASK_ID: &str = "context-task";

fn peer_results() -> Vec<PeerTargetResult> {
    vec![PeerTargetResult {
        url: "http://10.0.0.7:9736".into(),
        is_success: true,
        http_status: "200".into(),
        message: "ok".into(),
    }]
}

fn project() -> Project {
    Project {
        id: "project-1".into(),
        name: "repo".into(),
        repo_path: "/remote/repo".into(),
        default_branch: "main".into(),
        ssh_host: Some("devbox".into()),
        is_worktree: false,
        color: None,
        icon_data_url: None,
        created_at: String::new(),
    }
}

fn task(
    id: &str,
    project_id: &str,
    branch_name: Option<&str>,
    session_type: Option<SessionType>,
) -> KanbanTask {
    KanbanTask {
        id: id.into(),
        title: id.into(),
        description: None,
        status: TaskStatus::Todo,
        branch_name: branch_name.map(str::to_string),
        worktree_path: None,
        session_type,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: Some(project_id.into()),
        base_branch: None,
        pr_url: None,
        priority: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn context_task() -> KanbanTask {
    let mut context = task(
        CONTEXT_TASK_ID,
        "project-1",
        Some("feat/current"),
        Some(SessionType::Tmux),
    );
    context.worktree_path = Some("/remote/repo__worktrees/feat-current".into());
    context
}

/// TS 테스트의 `vi.mock` 묶음. id나 브랜치, 프로젝트가 모두 맞는 task만 돌려줘 실제 저장소 조회처럼 프로젝트 경계를 지킨다
struct FakeBackend {
    tasks: Mutex<Vec<KanbanTask>>,
    project: Mutex<Option<Project>>,
    token_error: Mutex<Option<String>>,
    branch_task_ids: Mutex<VecDeque<Option<String>>>,
    fallback_branch_task_id: Mutex<Option<String>>,
    create_error: Mutex<Option<String>>,
    deletion: Mutex<Result<TaskDeletion, TaskDeletionError>>,
    has_sync_changes: Mutex<bool>,
    prepare_result: Mutex<Result<CliTaskSession, String>>,
    propagation_gate: Mutex<Option<mpsc::Receiver<()>>>,
    events: Mutex<Vec<String>>,
    creations: Mutex<Vec<CliTaskCreation>>,
    deletions: Mutex<Vec<(String, TaskDeletionOptions)>>,
    prepared_tasks: Mutex<Vec<KanbanTask>>,
}

impl Default for FakeBackend {
    fn default() -> Self {
        Self {
            tasks: Mutex::new(vec![context_task()]),
            project: Mutex::new(Some(project())),
            token_error: Mutex::new(None),
            branch_task_ids: Mutex::new(VecDeque::new()),
            fallback_branch_task_id: Mutex::new(None),
            create_error: Mutex::new(None),
            deletion: Mutex::new(Ok(TaskDeletion {
                is_deleted: true,
                peer_results: peer_results(),
            })),
            has_sync_changes: Mutex::new(false),
            prepare_result: Mutex::new(Ok(CliTaskSession {
                is_prepared: true,
                ..CliTaskSession::default()
            })),
            propagation_gate: Mutex::new(None),
            events: Mutex::new(Vec::new()),
            creations: Mutex::new(Vec::new()),
            deletions: Mutex::new(Vec::new()),
            prepared_tasks: Mutex::new(Vec::new()),
        }
    }
}

impl FakeBackend {
    fn with_tasks(tasks: Vec<KanbanTask>) -> Self {
        let backend = Self::default();
        *backend.tasks.lock().unwrap() = tasks;
        backend
    }

    fn record(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }

    fn events_starting_with(&self, prefix: &str) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with(prefix))
            .cloned()
            .collect()
    }
}

impl TaskCliBackend for FakeBackend {
    fn is_valid_task_cli_token(&self, task_id: &str, token: &str) -> Result<bool, String> {
        if let Some(error) = self.token_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(task_id == CONTEXT_TASK_ID && token == "valid-token")
    }

    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String> {
        self.record(format!("find_task {task_id}"));
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .iter()
            .find(|task| task.id == task_id)
            .cloned())
    }

    fn find_project_task(
        &self,
        project_id: &str,
        selector: &ProjectTaskSelector,
    ) -> Result<Option<KanbanTask>, String> {
        self.record(format!("find_project_task {project_id} {selector:?}"));
        let tasks = self.tasks.lock().unwrap();
        let is_selected = |task: &&KanbanTask| match selector {
            ProjectTaskSelector::Id(id) => &task.id == id,
            ProjectTaskSelector::Branch(branch) => task.branch_name.as_ref() == Some(branch),
        };
        Ok(tasks
            .iter()
            .filter(|task| task.project_id.as_deref() == Some(project_id))
            .find(is_selected)
            .cloned())
    }

    fn find_project(&self, _project_id: &str) -> Result<Option<Project>, String> {
        Ok(self.project.lock().unwrap().clone())
    }

    fn list_project_tasks(&self, project_id: &str) -> Result<Vec<KanbanTask>, String> {
        self.record(format!("list_project_tasks {project_id}"));
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks
            .iter()
            .filter(|task| task.project_id.as_deref() == Some(project_id))
            .cloned()
            .collect())
    }

    fn task_id_by_project_and_branch(
        &self,
        _project_id: &str,
        _branch_name: &str,
    ) -> Result<Option<String>, String> {
        let queued = self.branch_task_ids.lock().unwrap().pop_front();
        Ok(queued.unwrap_or_else(|| self.fallback_branch_task_id.lock().unwrap().clone()))
    }

    fn create_task(&self, creation: &CliTaskCreation) -> Result<KanbanTask, String> {
        self.creations.lock().unwrap().push(creation.clone());
        if let Some(error) = self.create_error.lock().unwrap().clone() {
            return Err(error);
        }
        let mut created = task(
            "created-task",
            &creation.project_id,
            Some(&creation.branch_name),
            Some(creation.session_type),
        );
        created.title = creation.title.clone();
        created.base_branch = Some(creation.base_branch.clone());
        created.worktree_path = Some("/remote/repo__worktrees/feat-new".into());
        created.session_name = Some("repo-feat-new".into());
        created.ssh_host = Some("devbox".into());
        self.tasks.lock().unwrap().push(created.clone());
        Ok(created)
    }

    fn delete_task_across_targets(
        &self,
        task_id: &str,
        options: &TaskDeletionOptions,
    ) -> Result<TaskDeletion, TaskDeletionError> {
        self.deletions
            .lock()
            .unwrap()
            .push((task_id.to_string(), *options));
        self.deletion.lock().unwrap().clone()
    }

    fn sync_registered_project_worktrees(&self, project: &Project) -> Result<bool, String> {
        self.record(format!("sync {}", project.id));
        Ok(*self.has_sync_changes.lock().unwrap())
    }

    fn broadcast_board_update(&self) {
        self.record("broadcast".into());
    }

    fn prepare_task_terminal(&self, task: &KanbanTask) -> Result<CliTaskSession, String> {
        self.prepared_tasks.lock().unwrap().push(task.clone());
        self.record(format!("prepare {}", task.id));
        self.prepare_result.lock().unwrap().clone()
    }

    fn propagate_task_registration_to_peers(
        &self,
        project_repo_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult> {
        let gate = self.propagation_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.recv().unwrap();
        }
        self.record(format!(
            "propagate {project_repo_path} {branch_name} {}",
            ssh_host.unwrap_or("null")
        ));
        peer_results()
    }
}

fn body(value: serde_json::Value) -> RequestBody {
    value.as_object().expect("JSON 객체").clone()
}

fn create_body(extra: serde_json::Value) -> RequestBody {
    let mut request = body(
        json!({ "contextTaskId": CONTEXT_TASK_ID, "branchName": "feat/new", "baseBranch": "main" }),
    );
    request.extend(body(extra));
    request
}

fn assert_cli_error<T: std::fmt::Debug>(
    result: Result<T, TaskCliError>,
    status_code: u16,
    message: Option<&str>,
) {
    let error = result.expect_err("요청이 거절돼야 한다");
    assert_eq!(error.status_code(), Some(status_code), "{error}");
    if let Some(message) = message {
        assert!(error.to_string().contains(message), "{error}");
    }
}

fn hosts(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

// parity: TS-main-taskCliService-1a96f8
#[test]
fn missing_or_foreign_tokens_are_rejected_with_401_before_any_lookup() {
    let backend = FakeBackend::default();
    let service = TaskCliService::new(&backend);

    assert_cli_error(
        service.create_task_from_cli(None, &create_body(json!({})), None),
        401,
        None,
    );
    assert_cli_error(
        service.create_task_from_cli(Some("Bearer other"), &create_body(json!({})), None),
        401,
        None,
    );

    assert!(backend.events_starting_with("find_task").is_empty());
    assert!(backend.creations.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-f85864
#[test]
fn internal_token_check_errors_become_a_redacted_500() {
    let backend = FakeBackend::default();
    *backend.token_error.lock().unwrap() = Some("SQLITE_BUSY: /Users/secret/kanvibe.db".into());

    let result = TaskCliService::new(&backend).delete_task_from_cli(
        Some("Bearer anything"),
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "confirmed": true })),
        None,
    );

    let error = result.unwrap_err();
    assert_eq!(error.status_code(), Some(500));
    assert!(!error.to_string().contains("secret"));
    assert!(backend.events_starting_with("find_task").is_empty());
}

// parity: TS-main-taskCliService-af0d8d
#[test]
fn requests_through_another_hosts_tunnel_are_rejected_even_with_a_valid_token() {
    let backend = FakeBackend::default();
    let service = TaskCliService::new(&backend);
    let other_host = hosts(&["other-host"]);
    let delete_body = body(json!({ "contextTaskId": CONTEXT_TASK_ID, "confirmed": true }));
    let register_body = body(json!({ "contextTaskId": CONTEXT_TASK_ID, "branchName": "feat/new" }));

    assert_cli_error(
        service.delete_task_from_cli(AUTHORIZATION, &delete_body, Some(&other_host)),
        401,
        None,
    );
    assert_cli_error(
        service.create_task_from_cli(AUTHORIZATION, &create_body(json!({})), Some(&other_host)),
        401,
        None,
    );
    assert_cli_error(
        service.register_task_from_peer(AUTHORIZATION, &register_body, Some(&other_host)),
        401,
        None,
    );
    assert!(backend.deletions.lock().unwrap().is_empty());
    assert!(backend.creations.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-45ccce
#[test]
fn task_without_a_host_accepts_the_project_hosts_tunnel() {
    let backend = FakeBackend::default();
    *backend.deletion.lock().unwrap() = Ok(TaskDeletion {
        is_deleted: true,
        peer_results: Vec::new(),
    });

    let deleted = TaskCliService::new(&backend)
        .delete_task_from_cli(
            AUTHORIZATION,
            &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "confirmed": true })),
            Some(&hosts(&["devbox"])),
        )
        .unwrap();

    assert_eq!(deleted.id, CONTEXT_TASK_ID);
    assert!(deleted.is_deleted);
}

// parity: TS-main-taskCliService-4aa3f6
#[test]
fn tasks_without_a_project_are_rejected_with_409() {
    let backend = FakeBackend::default();
    *backend.project.lock().unwrap() = None;

    let result = TaskCliService::new(&backend).delete_task_from_cli(
        AUTHORIZATION,
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "confirmed": true })),
        None,
    );

    assert_cli_error(result, 409, None);
    assert!(backend.deletions.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-2b5938
#[test]
fn default_creation_inherits_the_context_session_type_and_propagates_registration() {
    let backend = FakeBackend::default();

    let created = TaskCliService::new(&backend)
        .create_task_from_cli(
            AUTHORIZATION,
            &create_body(json!({ "baseBranch": "feat/current", "title": "로그인 \"수정\"" })),
            None,
        )
        .unwrap();

    assert_eq!(
        backend.creations.lock().unwrap()[0],
        CliTaskCreation {
            title: "로그인 \"수정\"".into(),
            description: None,
            priority: None,
            branch_name: "feat/new".into(),
            base_branch: "feat/current".into(),
            session_type: SessionType::Tmux,
            project_id: "project-1".into(),
        }
    );
    assert_eq!(
        backend.events_starting_with("propagate"),
        ["propagate /remote/repo feat/new devbox"]
    );
    assert_eq!(created.id, "created-task");
    assert_eq!(created.base_branch.as_deref(), Some("feat/current"));
    assert_eq!(created.peers, peer_results());
}

// parity: TS-main-taskCliService-7aee76
#[test]
fn from_task_uses_that_tasks_branch_and_session_type() {
    let backend = FakeBackend::with_tasks(vec![
        context_task(),
        task(
            "base-task",
            "project-1",
            Some("feat/base"),
            Some(SessionType::Zellij),
        ),
    ]);
    let mut request = create_body(json!({ "fromTaskId": "base-task" }));
    request.remove("baseBranch");

    TaskCliService::new(&backend)
        .create_task_from_cli(AUTHORIZATION, &request, None)
        .unwrap();

    let creation = backend.creations.lock().unwrap()[0].clone();
    assert_eq!(creation.base_branch, "feat/base");
    assert_eq!(creation.session_type, SessionType::Zellij);
}

// parity: TS-main-taskCliService-d3a592
#[test]
fn explicit_session_type_wins_and_terminal_is_the_last_fallback() {
    let mut sessionless_context = context_task();
    sessionless_context.session_type = None;
    let backend = FakeBackend::with_tasks(vec![sessionless_context]);
    let service = TaskCliService::new(&backend);

    service
        .create_task_from_cli(
            AUTHORIZATION,
            &create_body(json!({ "branchName": "feat/a" })),
            None,
        )
        .unwrap();
    service
        .create_task_from_cli(
            AUTHORIZATION,
            &create_body(json!({ "branchName": "feat/b", "sessionType": "herdr" })),
            None,
        )
        .unwrap();

    let session_types: Vec<SessionType> = backend
        .creations
        .lock()
        .unwrap()
        .iter()
        .map(|creation| creation.session_type)
        .collect();
    assert_eq!(session_types, [SessionType::Terminal, SessionType::Herdr]);
}

// parity: TS-main-taskCliService-72c5e5
#[test]
fn tasks_from_other_projects_or_without_branches_are_not_used_as_a_base() {
    let backend = FakeBackend::with_tasks(vec![
        context_task(),
        task("other-project-task", "project-2", Some("feat/other"), None),
        task("branchless-task", "project-1", None, None),
    ]);
    let service = TaskCliService::new(&backend);

    let foreign = service.create_task_from_cli(
        AUTHORIZATION,
        &create_body(json!({ "fromTaskId": "other-project-task" })),
        None,
    );
    let branchless = service.create_task_from_cli(
        AUTHORIZATION,
        &create_body(json!({ "fromTaskId": "branchless-task" })),
        None,
    );

    assert_cli_error(foreign, 404, None);
    assert_cli_error(branchless, 409, None);
    assert!(backend.creations.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-9bedc0
#[test]
fn invalid_input_is_rejected_with_400_and_creates_nothing() {
    let cases = [
        (json!({ "baseBranch": "main" }), "branchName는 필수"),
        (
            json!({ "branchName": "feat/$(rm -rf ~)", "baseBranch": "main" }),
            "branchName에 쓸 수 없는 문자",
        ),
        (json!({ "branchName": "feat/new" }), "baseBranch는 필수"),
        (
            json!({ "branchName": "feat/new", "baseBranch": "main", "sessionType": "screen" }),
            "sessionType 값이 올바르지 않습니다",
        ),
    ];
    for (fields, message) in cases {
        let backend = FakeBackend::default();
        let mut request = body(json!({ "contextTaskId": CONTEXT_TASK_ID }));
        request.extend(body(fields));

        let result =
            TaskCliService::new(&backend).create_task_from_cli(AUTHORIZATION, &request, None);

        assert_cli_error(result, 400, Some(message));
        assert!(backend.creations.lock().unwrap().is_empty());
    }
}

// parity: TS-main-taskCliService-e6dfe3
#[test]
fn an_existing_branch_task_in_the_project_is_rejected_with_409() {
    let backend = FakeBackend::default();
    *backend.fallback_branch_task_id.lock().unwrap() = Some("existing-task".into());

    let result = TaskCliService::new(&backend).create_task_from_cli(
        AUTHORIZATION,
        &create_body(json!({})),
        None,
    );

    assert_cli_error(result, 409, None);
    assert!(backend.creations.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-c112ae
#[test]
fn failed_worktree_creation_is_not_propagated_to_peers() {
    let backend = FakeBackend::default();
    *backend.create_error.lock().unwrap() =
        Some("fatal: a branch named 'feat/new' already exists".into());

    let error = TaskCliService::new(&backend)
        .create_task_from_cli(AUTHORIZATION, &create_body(json!({})), None)
        .unwrap_err();

    assert!(error.to_string().contains("already exists"));
    assert!(backend.events_starting_with("propagate").is_empty());
}

// parity: TS-main-taskCliService-622f69
#[test]
fn created_task_session_is_prepared_once_and_returned() {
    let backend = FakeBackend::default();

    let created = TaskCliService::new(&backend)
        .create_task_from_cli(AUTHORIZATION, &create_body(json!({})), None)
        .unwrap();

    let prepared_tasks = backend.prepared_tasks.lock().unwrap().clone();
    assert_eq!(prepared_tasks.len(), 1);
    assert_eq!(prepared_tasks[0].id, "created-task");
    assert_eq!(
        prepared_tasks[0].session_name.as_deref(),
        Some("repo-feat-new")
    );
    assert_eq!(created.session_name.as_deref(), Some("repo-feat-new"));
    assert_eq!(created.ssh_host.as_deref(), Some("devbox"));
    assert!(created.session.is_prepared);
}

// parity: TS-main-taskCliService-2d91e4
#[test]
fn session_preparation_starts_without_waiting_for_peer_propagation() {
    let backend = FakeBackend::default();
    let (release, gate) = mpsc::channel();
    *backend.propagation_gate.lock().unwrap() = Some(gate);

    let created = std::thread::scope(|scope| {
        let creation = scope.spawn(|| {
            TaskCliService::new(&backend).create_task_from_cli(
                AUTHORIZATION,
                &create_body(json!({})),
                None,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while backend.events_starting_with("prepare").is_empty() {
            assert!(
                Instant::now() < deadline,
                "전파를 기다리느라 세션 준비가 시작되지 않았다"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        release.send(()).unwrap();
        creation.join().unwrap().unwrap()
    });

    assert!(created.session.is_prepared);
    assert_eq!(created.peers, peer_results());
}

// parity: TS-main-taskCliService-426a91
#[test]
fn failed_session_preparation_keeps_the_task_and_reports_session_error() {
    let backend = FakeBackend::default();
    *backend.prepare_result.lock().unwrap() = Err("tmux 세션 생성에 실패했습니다.".into());

    let created = TaskCliService::new(&backend)
        .create_task_from_cli(AUTHORIZATION, &create_body(json!({})), None)
        .unwrap();

    assert_eq!(created.id, "created-task");
    assert!(!created.session.is_prepared);
    assert_eq!(
        created.session.error.as_deref(),
        Some("tmux 세션 생성에 실패했습니다.")
    );
    assert_eq!(backend.events_starting_with("propagate").len(), 1);
}

// parity: TS-main-taskCliService-e05fc8
#[test]
fn description_and_priority_are_saved_and_unknown_priorities_are_rejected() {
    let backend = FakeBackend::default();
    let service = TaskCliService::new(&backend);

    service
        .create_task_from_cli(
            AUTHORIZATION,
            &create_body(json!({ "description": "로그인 오류 수정", "priority": "high" })),
            None,
        )
        .unwrap();
    let creation = backend.creations.lock().unwrap()[0].clone();
    assert_eq!(creation.description.as_deref(), Some("로그인 오류 수정"));
    assert_eq!(creation.priority, Some(TaskPriority::High));

    backend.creations.lock().unwrap().clear();
    let result = service.create_task_from_cli(
        AUTHORIZATION,
        &create_body(json!({ "branchName": "feat/x", "priority": "urgent" })),
        None,
    );
    assert_cli_error(result, 400, Some("priority 값이 올바르지 않습니다"));
    assert!(backend.creations.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-eb9c61
#[test]
fn listing_returns_only_project_tasks_with_session_details() {
    let mut current = context_task();
    current.title = "현재".into();
    current.status = TaskStatus::Progress;
    current.session_name = Some("repo-feat-current".into());
    current.ssh_host = Some("devbox".into());
    let mut foreign = task("foreign-task", "project-2", Some("feat/foreign"), None);
    foreign.title = "남의 것".into();
    let backend = FakeBackend::with_tasks(vec![current, foreign]);

    let listed = TaskCliService::new(&backend)
        .list_tasks_from_cli(
            AUTHORIZATION,
            &body(json!({ "contextTaskId": CONTEXT_TASK_ID })),
            None,
        )
        .unwrap();

    assert_eq!(
        listed,
        [ListedCliTask {
            id: "context-task".into(),
            title: "현재".into(),
            branch_name: Some("feat/current".into()),
            status: "progress".into(),
            priority: None,
            worktree_path: Some("/remote/repo__worktrees/feat-current".into()),
            session_type: Some(SessionType::Tmux),
            session_name: Some("repo-feat-current".into()),
            ssh_host: Some("devbox".into()),
        }]
    );
}

// parity: TS-main-taskCliService-7a2b7b
#[test]
fn another_tasks_token_cannot_read_the_list() {
    let backend = FakeBackend::default();

    let result = TaskCliService::new(&backend).list_tasks_from_cli(
        Some("Bearer other-token"),
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID })),
        None,
    );

    assert_cli_error(result, 401, None);
    assert!(
        backend
            .events_starting_with("list_project_tasks")
            .is_empty()
    );
}

// parity: TS-main-taskCliService-4152b1
#[test]
fn session_command_without_a_branch_prepares_the_context_task() {
    let mut current = context_task();
    current.session_name = Some("repo-feat-current".into());
    current.ssh_host = Some("devbox".into());
    let backend = FakeBackend::with_tasks(vec![current]);

    let prepared = TaskCliService::new(&backend)
        .prepare_task_session_from_cli(
            AUTHORIZATION,
            &body(json!({ "contextTaskId": CONTEXT_TASK_ID })),
            None,
        )
        .unwrap();

    assert_eq!(
        prepared,
        PreparedCliTaskSession {
            id: "context-task".into(),
            branch_name: Some("feat/current".into()),
            session_type: Some(SessionType::Tmux),
            session_name: Some("repo-feat-current".into()),
            ssh_host: Some("devbox".into()),
            session: CliTaskSession {
                is_prepared: true,
                ..CliTaskSession::default()
            },
        }
    );
}

// parity: TS-main-taskCliService-8c27dc
#[test]
fn session_command_prepares_sibling_branches_but_not_other_projects() {
    let mut sibling = task(
        "sibling-task",
        "project-1",
        Some("feat/sibling"),
        Some(SessionType::Zellij),
    );
    sibling.session_name = Some("repo-feat-sibling".into());
    let mut foreign = task(
        "foreign-task",
        "project-2",
        Some("feat/foreign"),
        Some(SessionType::Tmux),
    );
    foreign.session_name = Some("other".into());
    let backend = FakeBackend::with_tasks(vec![context_task(), sibling, foreign]);
    let service = TaskCliService::new(&backend);

    service
        .prepare_task_session_from_cli(
            AUTHORIZATION,
            &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "branchName": "feat/sibling" })),
            None,
        )
        .unwrap();
    assert_eq!(backend.prepared_tasks.lock().unwrap()[0].id, "sibling-task");

    backend.prepared_tasks.lock().unwrap().clear();
    let result = service.prepare_task_session_from_cli(
        AUTHORIZATION,
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "branchName": "feat/foreign" })),
        None,
    );
    assert_cli_error(result, 404, None);
    assert!(backend.prepared_tasks.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-d2d427
#[test]
fn deletion_requires_confirmation() {
    let backend = FakeBackend::default();

    let result = TaskCliService::new(&backend).delete_task_from_cli(
        AUTHORIZATION,
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID })),
        None,
    );

    assert_cli_error(result, 400, None);
    assert!(backend.deletions.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-b4d214
#[test]
fn deletion_without_a_branch_removes_the_context_task_rejecting_dirty_worktrees_and_propagating() {
    let backend = FakeBackend::default();

    let deleted = TaskCliService::new(&backend)
        .delete_task_from_cli(
            AUTHORIZATION,
            &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "confirmed": true })),
            None,
        )
        .unwrap();

    assert_eq!(
        *backend.deletions.lock().unwrap(),
        [(
            "context-task".to_string(),
            TaskDeletionOptions {
                should_reject_dirty_worktree: true,
                should_propagate_to_peers: true
            }
        )]
    );
    assert_eq!(
        deleted,
        DeletedCliTask {
            id: "context-task".into(),
            branch_name: Some("feat/current".into()),
            is_deleted: true,
            peers: peer_results(),
        }
    );
}

// parity: TS-main-taskCliService-53ba70
#[test]
fn sibling_deletion_by_branch_honours_force_and_peer_requests_do_not_propagate_again() {
    let backend = FakeBackend::with_tasks(vec![
        context_task(),
        task("sibling-task", "project-1", Some("feat/sibling"), None),
    ]);

    TaskCliService::new(&backend)
        .delete_task_from_cli(
            AUTHORIZATION,
            &body(json!({
                "contextTaskId": CONTEXT_TASK_ID,
                "branchName": "feat/sibling",
                "confirmed": true,
                "force": true,
                "propagateToPeers": false,
            })),
            None,
        )
        .unwrap();

    assert_eq!(
        *backend.deletions.lock().unwrap(),
        [(
            "sibling-task".to_string(),
            TaskDeletionOptions {
                should_reject_dirty_worktree: false,
                should_propagate_to_peers: false
            }
        )]
    );
}

// parity: TS-main-taskCliService-912481
#[test]
fn branches_of_other_projects_are_not_found() {
    let backend = FakeBackend::with_tasks(vec![
        context_task(),
        task("foreign-task", "project-2", Some("feat/foreign"), None),
    ]);

    let result = TaskCliService::new(&backend).delete_task_from_cli(
        AUTHORIZATION,
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "branchName": "feat/foreign", "confirmed": true })),
        None,
    );

    assert_cli_error(result, 404, None);
    assert!(backend.deletions.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-343d01
#[test]
fn the_default_branch_root_task_cannot_be_deleted_from_the_cli() {
    let backend = FakeBackend::with_tasks(vec![
        context_task(),
        task("root-task", "project-1", Some("main"), None),
    ]);

    let result = TaskCliService::new(&backend).delete_task_from_cli(
        AUTHORIZATION,
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "branchName": "main", "confirmed": true, "force": true })),
        None,
    );

    assert_cli_error(result, 403, Some("기본 브랜치"));
    assert!(backend.deletions.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-e77632
#[test]
fn dirty_worktrees_report_409_with_the_force_hint() {
    let backend = FakeBackend::default();
    *backend.deletion.lock().unwrap() = Err(TaskDeletionError::DirtyWorktree(
        "커밋하지 않은 변경이 있어 삭제하지 않았습니다: /remote/repo__worktrees/feat-current"
            .into(),
    ));

    let result = TaskCliService::new(&backend).delete_task_from_cli(
        AUTHORIZATION,
        &body(json!({ "contextTaskId": CONTEXT_TASK_ID, "confirmed": true })),
        None,
    );

    assert_cli_error(result, 409, Some("--force"));
}

fn register_body(branch_name: &str) -> RequestBody {
    body(json!({ "contextTaskId": CONTEXT_TASK_ID, "branchName": branch_name }))
}

// parity: TS-main-taskCliService-720dec
#[test]
fn already_registered_branches_return_the_task_without_rescanning() {
    let backend = FakeBackend::default();
    *backend.fallback_branch_task_id.lock().unwrap() = Some("known-task".into());

    let registered = TaskCliService::new(&backend)
        .register_task_from_peer(AUTHORIZATION, &register_body("feat/new"), None)
        .unwrap();

    assert_eq!(
        registered,
        RegisteredPeerTask {
            id: "known-task".into(),
            branch_name: "feat/new".into()
        }
    );
    assert!(backend.events_starting_with("sync").is_empty());
    assert!(backend.creations.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-caa516
#[test]
fn new_branches_are_registered_through_the_project_worktree_sync() {
    let backend = FakeBackend::default();
    *backend.branch_task_ids.lock().unwrap() =
        VecDeque::from([None, Some("registered-task".to_string())]);
    *backend.has_sync_changes.lock().unwrap() = true;

    let registered = TaskCliService::new(&backend)
        .register_task_from_peer(AUTHORIZATION, &register_body("feat/new"), None)
        .unwrap();

    assert_eq!(
        registered,
        RegisteredPeerTask {
            id: "registered-task".into(),
            branch_name: "feat/new".into()
        }
    );
    assert_eq!(backend.events_starting_with("sync"), ["sync project-1"]);
    assert_eq!(backend.events_starting_with("broadcast").len(), 1);
    assert!(backend.creations.lock().unwrap().is_empty());
}

// parity: TS-main-taskCliService-75431a
#[test]
fn missing_worktree_after_sync_reports_404() {
    let backend = FakeBackend::default();

    let result = TaskCliService::new(&backend).register_task_from_peer(
        AUTHORIZATION,
        &register_body("feat/missing"),
        None,
    );

    assert_cli_error(result, 404, None);
}

#[test]
fn bearer_tokens_follow_the_case_insensitive_single_token_rule() {
    assert_eq!(parse_bearer_token(Some("Bearer abc")), Some("abc"));
    assert_eq!(parse_bearer_token(Some("bearer \t abc")), Some("abc"));
    assert_eq!(parse_bearer_token(Some("Bearerabc")), None);
    assert_eq!(parse_bearer_token(Some("Bearer a b")), None);
    assert_eq!(parse_bearer_token(Some("Bearer ")), None);
    assert_eq!(parse_bearer_token(None), None);
}
