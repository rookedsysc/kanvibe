use std::sync::Mutex;

use kanvibe_db::repositories::projects::NewProject;
use kanvibe_db::repositories::tasks::NewKanbanTask;
use kanvibe_domain::task::SessionType;
use kanvibe_process::exec::ShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::kanvibe_state_store::HostKanvibeStateStore;
use serde_json::json;

use super::*;
use crate::board_notifier::BoardEvent;
use crate::test_support::{
    RecordedBoardEvents, ScriptedCommandRunner, TemporaryDatabase, new_project, new_task,
};

#[derive(Default)]
struct RecordingBindings {
    recorded: Mutex<Vec<(PaneChatBindingTarget, Value)>>,
}

impl PaneChatSessionBindingRecorder for RecordingBindings {
    fn record_pane_chat_session_binding(
        &self,
        target: &PaneChatBindingTarget,
        agent_session: &Value,
    ) {
        self.recorded
            .lock()
            .unwrap()
            .push((target.clone(), agent_session.clone()));
    }
}

struct Fixture {
    database: TemporaryDatabase,
    runner: Arc<ScriptedCommandRunner>,
    events: RecordedBoardEvents,
    bindings: Arc<RecordingBindings>,
    service: HookStatusService,
}

/// 실제 `.kanvibe` 상태 저장 경로를 쓰되 git 명령은 기록만 한다. 서비스 수명 동안 살아야 하는 참조는 테스트에서만 새게 둔다
fn fixture() -> Fixture {
    let database = TemporaryDatabase::open();
    let runner = Arc::new(ScriptedCommandRunner::returning(".git"));
    let home = database.directory.path().join("home");
    let files: &'static HostFileAccess = Box::leak(Box::new(HostFileAccess::new(
        Arc::clone(&runner) as Arc<dyn ShellCommandRunner>,
        home.to_string_lossy(),
    )));
    let store: &'static HostKanvibeStateStore<'static> =
        Box::leak(Box::new(HostKanvibeStateStore::new(files)));
    let records: &'static Database = Box::leak(Box::new(Arc::clone(&database.database)));
    let task_state = Arc::new(TaskStateService::new(store, records));
    let board = BoardNotifier::new();
    let events = RecordedBoardEvents::subscribe(&board);
    let bindings = Arc::new(RecordingBindings::default());
    let service = HookStatusService::new(
        Arc::clone(&database.database),
        board,
        task_state,
        Arc::clone(&bindings) as Arc<dyn PaneChatSessionBindingRecorder>,
    );
    Fixture {
        database,
        runner,
        events,
        bindings,
        service,
    }
}

impl Fixture {
    fn authorization(&self, task_id: &str) -> String {
        let token = TaskCliTokenService::new(&self.database.database)
            .build_task_cli_token(task_id)
            .unwrap();
        format!("Bearer {token}")
    }

    fn update(
        &self,
        task_id: &str,
        status: &str,
        agent_session: Option<Value>,
        hosts: Option<&[&str]>,
    ) -> Result<HookStatusUpdated, HookStatusRejection> {
        let input = HookStatusInput {
            task_id: task_id.into(),
            status: status.into(),
            agent_session,
        };
        let source_hosts: Option<HashSet<String>> =
            hosts.map(|hosts| hosts.iter().map(|host| host.to_string()).collect());
        self.service.update_hook_task_status(
            &input,
            Some(&self.authorization(task_id)),
            source_hosts.as_ref(),
        )
    }

    fn stored_status(&self, task_id: &str) -> TaskStatus {
        find_task(&self.database.database.connection(), task_id)
            .unwrap()
            .unwrap()
            .status
    }

    fn status_changes(&self) -> Vec<TaskStatusChangedPayload> {
        self.events
            .events()
            .into_iter()
            .filter_map(|event| match event {
                BoardEvent::TaskStatusChanged(changed) => Some(changed),
                _ => None,
            })
            .collect()
    }
}

/// 원격 쓰기 명령은 내용을 base64로 실어 보낸다. `printf '%s' '<base64>'` 부분을 풀어 확인한다
fn decode_remote_write(command: &str) -> String {
    use base64::Engine;
    let encoded = command
        .split("printf '%s' '")
        .nth(1)
        .and_then(|rest| rest.split('\'').next())
        .expect("base64 내용");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

fn task_in(project: &Project, title: &str, status: TaskStatus) -> NewKanbanTask {
    NewKanbanTask {
        project_id: Some(project.id.clone()),
        ..new_task(title, status)
    }
}

#[test]
fn rejects_missing_foreign_or_malformed_tokens_without_touching_the_task() {
    let fixture = fixture();
    let task = fixture
        .database
        .insert_task(new_task("task", TaskStatus::Progress));
    let foreign = format!(
        "Bearer {}",
        fixture
            .authorization("other-task")
            .trim_start_matches("Bearer ")
    );
    let bare_token = fixture
        .authorization(&task.id)
        .trim_start_matches("Bearer ")
        .to_string();
    let input = HookStatusInput {
        task_id: task.id.clone(),
        status: "review".into(),
        agent_session: None,
    };

    for header in [None, Some(foreign.as_str()), Some(bare_token.as_str())] {
        let rejection = fixture
            .service
            .update_hook_task_status(&input, header, None)
            .unwrap_err();
        assert_eq!(rejection.status_code, 401);
    }

    assert_eq!(fixture.stored_status(&task.id), TaskStatus::Progress);
    assert!(fixture.events.events().is_empty());
}

#[test]
fn tunnel_requests_cannot_change_tasks_of_other_hosts() {
    let fixture = fixture();
    let task = fixture.database.insert_task(NewKanbanTask {
        ssh_host: Some("host-b".into()),
        ..new_task("task", TaskStatus::Progress)
    });

    let rejection = fixture
        .update(&task.id, "review", None, Some(&["host-a"]))
        .unwrap_err();

    assert_eq!(rejection.status_code, 404);
    assert_eq!(fixture.stored_status(&task.id), TaskStatus::Progress);
    assert!(fixture.status_changes().is_empty());
}

#[test]
fn changes_the_task_status_by_id() {
    let fixture = fixture();
    let project = fixture
        .database
        .insert_project(new_project("kanvibe", "/workspace/kanvibe"));
    let task = fixture.database.insert_task(NewKanbanTask {
        description: Some("debug electron hook".into()),
        branch_name: Some("fix-electron-notification".into()),
        ..task_in(&project, "Fix notification", TaskStatus::Progress)
    });

    let updated = fixture.update(&task.id, "review", None, None).unwrap();

    assert_eq!(
        fixture.status_changes(),
        [TaskStatusChangedPayload {
            project_name: "kanvibe".into(),
            branch_name: "fix-electron-notification".into(),
            task_title: "Fix notification".into(),
            description: Some("debug electron hook".into()),
            new_status: "review".into(),
            task_id: task.id.clone(),
        }]
    );
    assert_eq!(
        updated,
        HookStatusUpdated {
            id: task.id.clone(),
            status: TaskStatus::Review,
            branch_name: Some("fix-electron-notification".into()),
            project_name: "kanvibe".into(),
        }
    );
    assert_eq!(fixture.stored_status(&task.id), TaskStatus::Review);
}

#[test]
fn done_from_a_hook_keeps_session_and_worktree() {
    let fixture = fixture();
    let project = fixture.database.insert_project(NewProject {
        ssh_host: Some("remote-host".into()),
        ..new_project("kanvibe", "/workspace/api")
    });
    let task = fixture.database.insert_task(NewKanbanTask {
        branch_name: Some("feature-hook-done".into()),
        session_type: Some(SessionType::Tmux),
        session_name: Some("api-feature-hook-done".into()),
        worktree_path: Some("/workspace/api__worktrees/feature-hook-done".into()),
        ssh_host: Some("remote-host".into()),
        ..task_in(&project, "Preserve hook resources", TaskStatus::Review)
    });

    let updated = fixture.update(&task.id, "done", None, None).unwrap();

    let exclude_check = fixture
        .runner
        .calls()
        .into_iter()
        .find(|call| call.command.contains("rev-parse"))
        .expect("git exclude 확인");
    assert_eq!(
        exclude_check.command,
        "git -C '/workspace/api__worktrees/feature-hook-done' rev-parse --path-format=absolute --git-common-dir"
    );
    assert_eq!(exclude_check.ssh_host.as_deref(), Some("remote-host"));
    assert!(exclude_check.options.should_check_host_key_strictly);
    let stored = find_task(&fixture.database.database.connection(), &task.id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, TaskStatus::Done);
    assert_eq!(stored.session_type, Some(SessionType::Tmux));
    assert_eq!(
        stored.session_name.as_deref(),
        Some("api-feature-hook-done")
    );
    assert_eq!(stored.worktree_path, task.worktree_path);
    assert_eq!(stored.ssh_host.as_deref(), Some("remote-host"));
    assert_eq!(updated.project_name, "kanvibe");
}

#[test]
fn writes_the_status_into_the_worktree_state_file() {
    let fixture = fixture();
    let project = fixture
        .database
        .insert_project(new_project("kanvibe", "/workspace/api"));
    let worktree = fixture
        .database
        .directory
        .path()
        .join("api__worktrees/feature-sync");
    std::fs::create_dir_all(&worktree).unwrap();
    let task = fixture.database.insert_task(NewKanbanTask {
        branch_name: Some("feature-sync".into()),
        worktree_path: Some(worktree.to_string_lossy().into_owned()),
        ..task_in(&project, "Fix sync", TaskStatus::Progress)
    });

    fixture.update(&task.id, "review", None, None).unwrap();

    let state = std::fs::read_to_string(worktree.join(".kanvibe/status.json")).unwrap();
    assert!(state.contains("\"status\": \"review\""), "{state}");
}

#[test]
fn a_failed_state_file_write_still_answers_and_broadcasts() {
    let fixture = fixture();
    let project = fixture
        .database
        .insert_project(new_project("kanvibe", "/workspace/api"));
    let not_a_directory = fixture.database.directory.path().join("feature-sync");
    std::fs::write(&not_a_directory, "file").unwrap();
    let task = fixture.database.insert_task(NewKanbanTask {
        branch_name: Some("feature-sync".into()),
        worktree_path: Some(not_a_directory.to_string_lossy().into_owned()),
        ..task_in(&project, "Fix sync", TaskStatus::Progress)
    });

    let updated = fixture.update(&task.id, "review", None, None).unwrap();

    assert_eq!(
        updated,
        HookStatusUpdated {
            id: task.id.clone(),
            status: TaskStatus::Review,
            branch_name: Some("feature-sync".into()),
            project_name: "kanvibe".into(),
        }
    );
    assert_eq!(fixture.events.board_update_count(), 1);
    assert_eq!(fixture.status_changes()[0].new_status, "review");
}

#[test]
fn rejects_a_missing_task_id_with_400() {
    let fixture = fixture();
    let input = HookStatusInput {
        task_id: String::new(),
        status: "review".into(),
        agent_session: None,
    };

    let rejection = fixture
        .service
        .update_hook_task_status(&input, Some("Bearer token"), None)
        .unwrap_err();

    assert_eq!(
        rejection,
        HookStatusRejection::new("taskId, status는 필수입니다.", 400)
    );
}

#[test]
fn tasks_without_a_worktree_follow_the_shared_location_rule() {
    for branch_name in ["feature", "main"] {
        let fixture = fixture();
        let project = fixture.database.insert_project(NewProject {
            ssh_host: Some("remote".into()),
            ..new_project("repo", "/fixture/root")
        });
        let task = fixture.database.insert_task(NewKanbanTask {
            branch_name: Some(branch_name.into()),
            ..task_in(&project, "task", TaskStatus::Progress)
        });

        assert!(fixture.update(&task.id, "done", None, None).is_ok());

        let remote_writes: Vec<_> = fixture
            .runner
            .calls()
            .into_iter()
            .filter(|call| call.command.contains("/fixture/root/.kanvibe/status.json"))
            .collect();
        if branch_name == "main" {
            assert_eq!(remote_writes[0].ssh_host.as_deref(), Some("remote"));
            let written = decode_remote_write(&remote_writes[0].command);
            assert!(written.contains("\"status\": \"done\""), "{written}");
        } else {
            assert!(fixture.runner.calls().is_empty(), "{branch_name}");
        }
    }
}

#[test]
fn local_hook_metadata_binds_only_the_stored_task_scope() {
    let fixture = fixture();
    let project = fixture
        .database
        .insert_project(new_project("repo", "/fixture/root"));
    let task = fixture.database.insert_task(NewKanbanTask {
        worktree_path: Some("/fixture/feature".into()),
        ..task_in(&project, "task", TaskStatus::Todo)
    });
    let metadata = json!({ "provider": "claude", "hookPid": 123, "payloadBase64": "e30=" });

    assert!(
        fixture
            .update(&task.id, "progress", Some(metadata.clone()), None)
            .is_ok()
    );

    assert_eq!(
        *fixture.bindings.recorded.lock().unwrap(),
        [(
            PaneChatBindingTarget {
                task_id: task.id.clone(),
                worktree_path: Some("/fixture/feature".into()),
                repo_path: Some("/fixture/root".into()),
                ssh_host: None,
            },
            metadata
        )]
    );
}

#[test]
fn remote_metadata_binds_only_through_that_hosts_tunnel() {
    let fixture = fixture();
    let project = fixture.database.insert_project(NewProject {
        ssh_host: Some("remote".into()),
        ..new_project("repo", "/fixture/root")
    });
    let task = fixture.database.insert_task(NewKanbanTask {
        worktree_path: Some("/fixture/feature".into()),
        ssh_host: Some("remote".into()),
        ..task_in(&project, "task", TaskStatus::Progress)
    });
    let metadata = json!({ "provider": "codex", "hookPid": 123, "payloadBase64": "e30=" });

    fixture
        .update(&task.id, "progress", Some(metadata.clone()), None)
        .unwrap();
    assert!(fixture.bindings.recorded.lock().unwrap().is_empty());
    fixture
        .update(&task.id, "progress", Some(metadata), Some(&["remote"]))
        .unwrap();

    let recorded = fixture.bindings.recorded.lock().unwrap();
    assert_eq!(recorded[0].0.ssh_host.as_deref(), Some("remote"));
}

#[test]
fn a_missing_task_is_reported_to_the_board() {
    let fixture = fixture();

    let rejection = fixture
        .update("task-404", "Review", None, None)
        .unwrap_err();

    assert_eq!(rejection.status_code, 404);
    assert_eq!(
        fixture.events.events(),
        [BoardEvent::HookStatusTargetMissing(
            HookStatusTargetMissingPayload {
                task_id: "task-404".into(),
                requested_status: "review".into(),
                reason: HookStatusTargetMissingReason::TaskNotFound,
            }
        )]
    );
}
