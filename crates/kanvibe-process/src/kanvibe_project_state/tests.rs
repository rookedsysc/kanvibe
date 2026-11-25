//! TS `kanvibeProjectState.test.ts`. TS는 hostFileAccess를 mock했지만, 여기서는 임시 디렉터리의 실제 파일과
//! 원격 명령을 이 머신의 sh로 실행하는 실행기로 같은 경로·내용 계약을 확인한다.

use std::sync::{Arc, Mutex};

use kanvibe_domain::task::{SessionType, TaskPriority, TaskStatus};
use serde_json::{Value, json};

use super::*;
use crate::exec::testing::{LoopbackShellRunner, ScriptedCommandRunner};
use crate::exec::{ExecGitError, ShellCommandRunner};

/// 원격 명령을 로컬 sh로 실행하면서 (명령, 호스트)를 기록한다
struct RecordingLoopbackRunner {
    inner: LoopbackShellRunner,
    calls: Mutex<Vec<(String, Option<String>)>>,
}

impl ShellCommandRunner for RecordingLoopbackRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        let call = (command.to_string(), ssh_host.map(str::to_string));
        self.calls.lock().unwrap().push(call);
        self.inner.exec_git(command, ssh_host, options)
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    runner: Arc<RecordingLoopbackRunner>,
    files: HostFileAccess,
}

impl Fixture {
    fn new() -> Self {
        let runner = Arc::new(RecordingLoopbackRunner {
            inner: LoopbackShellRunner::new("/nonexistent-home"),
            calls: Mutex::new(Vec::new()),
        });
        Self {
            directory: tempfile::tempdir().unwrap(),
            files: HostFileAccess::new(runner.clone(), "/nonexistent-home"),
            runner,
        }
    }

    fn repo(&self) -> String {
        self.directory.path().to_string_lossy().into_owned()
    }

    fn state_path(&self, file_name: &str) -> String {
        format!("{}/.kanvibe/{file_name}", self.repo())
    }

    fn write_state(&self, file_name: &str, content: &str) {
        std::fs::create_dir_all(format!("{}/.kanvibe", self.repo())).unwrap();
        std::fs::write(self.state_path(file_name), content).unwrap();
    }

    fn read_state_json(&self, file_name: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.state_path(file_name)).unwrap()).unwrap()
    }

    /// 묶음 읽기 명령의 manifest(경로마다 base64, 다시 전체를 base64)를 풀어 요청한 경로 목록을 돌려준다
    fn remote_manifest_paths(&self) -> Vec<String> {
        use base64::Engine;
        let calls = self.runner.calls.lock().unwrap();
        let (command, host) = calls.first().unwrap();
        assert_eq!(host.as_deref(), Some("ssh-host"));
        let encoded_manifest = command.split('\'').nth(3).unwrap();
        let engine = base64::engine::general_purpose::STANDARD;
        let manifest = String::from_utf8(engine.decode(encoded_manifest).unwrap()).unwrap();
        manifest
            .lines()
            .map(|encoded_path| String::from_utf8(engine.decode(encoded_path).unwrap()).unwrap())
            .collect()
    }

    fn has_remote_call_for(&self, file_name: &str, host: &str) -> bool {
        let state_path = self.state_path(file_name);
        let calls = self.runner.calls.lock().unwrap();
        calls.iter().any(|(command, call_host)| {
            command.contains(&state_path) && call_host.as_deref() == Some(host)
        })
    }
}

fn target(url: &str, task_id: &str) -> KanvibeHookTarget {
    KanvibeHookTarget {
        url: url.into(),
        task_id: task_id.into(),
        ..KanvibeHookTarget::default()
    }
}

fn with_token(mut target: KanvibeHookTarget, token: &str) -> KanvibeHookTarget {
    target.token = Some(token.into());
    target
}

fn without_updated_at(mut value: Value) -> Value {
    let updated_at = value.as_object_mut().unwrap().remove("updatedAt");
    assert!(updated_at.is_some_and(|updated_at| updated_at.as_str().is_some_and(is_iso_timestamp)));
    value
}

fn is_iso_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.len() == 24 && bytes[10] == b'T' && bytes[19] == b'.' && value.ends_with('Z')
}

fn parse_json(content: &str) -> Value {
    serde_json::from_str(content).unwrap()
}

// parity: TS-lib-kanvibeProjectState-e4ac82
#[test]
fn builds_extensible_status_json_without_task_or_url_mappings() {
    let content = build_kanvibe_task_state_content(TaskStatus::Review);

    assert_eq!(
        without_updated_at(parse_json(&content)),
        json!({ "schemaVersion": 1, "status": "review" })
    );
    assert!(content.ends_with("}\n"));
    for leaked in ["task-1", "taskId", "url"] {
        assert!(!content.contains(leaked));
    }
}

// parity: TS-lib-kanvibeProjectState-73dbb5
#[test]
fn project_color_lives_in_project_json_not_status_json() {
    let task_state = parse_json(&build_kanvibe_task_state_content(TaskStatus::Progress));
    let project_state = parse_json(&build_kanvibe_project_state_content(
        Some("#65D08A".into()),
        None,
    ));

    assert_eq!(
        without_updated_at(task_state),
        json!({ "schemaVersion": 1, "status": "progress" })
    );
    assert_eq!(
        without_updated_at(project_state),
        json!({ "schemaVersion": 1, "projectColor": "#65D08A" })
    );
}

// parity: TS-lib-kanvibeProjectState-02f514
#[test]
fn project_priority_is_stored_in_project_json_with_the_color() {
    let content =
        build_kanvibe_project_state_content(Some("#65D08A".into()), Some(Some(TaskPriority::High)));

    assert_eq!(
        without_updated_at(parse_json(&content)),
        json!({ "schemaVersion": 1, "projectColor": "#65D08A", "priority": "high" })
    );
    let parsed = parse_kanvibe_project_state(&content).unwrap();
    assert_eq!(parsed.project_color.as_deref(), Some("#65D08A"));
    assert_eq!(parsed.priority, Some(Some(TaskPriority::High)));
    assert!(parsed.updated_at.is_some());
}

// parity: TS-lib-kanvibeProjectState-98486e
#[test]
fn only_rrggbb_colors_pass() {
    assert_eq!(parse_project_color("#65D08A").as_deref(), Some("#65D08A"));
    assert_eq!(
        parse_project_color("  #65d08a  ").as_deref(),
        Some("#65d08a")
    );
    assert_eq!(parse_project_color("65D08A"), None);
    assert_eq!(parse_project_color("#65D08"), None);
    assert_eq!(parse_project_color("#65D08A\" ; rm -rf /"), None);
    assert_eq!(parse_kanvibe_project_state(r#"{"projectColor":42}"#), None);
}

// parity: TS-lib-kanvibeProjectState-61e159
#[test]
fn normalizes_persisted_task_statuses_and_rejects_unusable_state_files() {
    assert_eq!(parse_task_status("PROGRESS"), Some(TaskStatus::Progress));
    assert_eq!(parse_task_status("pending"), Some(TaskStatus::Pending));
    assert_eq!(parse_task_status("blocked"), None);
    assert_eq!(parse_kanvibe_task_state(r#"{"status":42}"#), None);

    assert_eq!(parse_kanvibe_task_state(""), None);
    assert_eq!(parse_kanvibe_task_state("not-a-status"), None);
    let state = parse_kanvibe_task_state(
        r#"{"schemaVersion":1,"status":"review","updatedAt":"2026-06-03T00:00:00.000Z"}"#,
    );
    assert_eq!(
        state,
        Some(KanvibeTaskState {
            schema_version: 1,
            status: TaskStatus::Review,
            updated_at: Some("2026-06-03T00:00:00.000Z".into()),
        })
    );
    assert_eq!(
        parse_kanvibe_task_state("DONE\n").map(|state| (state.status, state.updated_at)),
        Some((TaskStatus::Done, None))
    );
    assert_eq!(
        parse_kanvibe_task_state("  review  \n").map(|state| state.status),
        Some(TaskStatus::Review)
    );
}

// parity: TS-lib-kanvibeProjectState-54d23f
#[test]
fn uses_host_aware_status_json_paths_when_reading_and_writing() {
    let fixture = Fixture::new();
    fixture.write_state("status.json", r#"{"schemaVersion":1,"status":"pending"}"#);

    let state = read_kanvibe_task_state(&fixture.files, &fixture.repo(), Some("ssh-host")).unwrap();
    write_kanvibe_task_status(
        &fixture.files,
        &fixture.repo(),
        TaskStatus::Done,
        Some("ssh-host"),
    )
    .unwrap();

    assert_eq!(state.map(|state| state.status), Some(TaskStatus::Pending));
    assert!(fixture.has_remote_call_for("status.json", "ssh-host"));
    let written = std::fs::read_to_string(fixture.state_path("status.json")).unwrap();
    assert!(written.contains(r#""status": "done""#));
    assert_eq!(
        without_updated_at(parse_json(&written)),
        json!({ "schemaVersion": 1, "status": "done" })
    );
    for leaked in ["task-2", "taskId", "url"] {
        assert!(!written.contains(leaked));
    }
    assert_eq!(
        get_kanvibe_task_state_path("/local/repo"),
        "/local/repo/.kanvibe/status.json"
    );
}

// parity: TS-lib-kanvibeProjectState-e78359
#[test]
fn status_and_color_writes_use_different_files() {
    let fixture = Fixture::new();

    write_kanvibe_task_status(&fixture.files, &fixture.repo(), TaskStatus::Review, None).unwrap();
    write_kanvibe_project_color(&fixture.files, &fixture.repo(), "#0064FF", None).unwrap();

    assert!(
        !fixture
            .read_state_json("status.json")
            .to_string()
            .contains("projectColor")
    );
    assert!(
        !fixture
            .read_state_json("project.json")
            .to_string()
            .contains("status")
    );
}

// parity: TS-lib-kanvibeProjectState-228ea6
#[test]
fn status_write_overwrites_status_json_without_reading() {
    let runner = Arc::new(ScriptedCommandRunner::returning(""));
    let files = HostFileAccess::new(runner.clone(), "/nonexistent-home");

    write_kanvibe_task_status(&files, "/remote/repo", TaskStatus::Review, Some("ssh-host"))
        .unwrap();

    let commands = runner.commands();
    assert_eq!(commands.len(), 1);
    assert!(commands[0].starts_with("mkdir -p '/remote/repo/.kanvibe' && printf '%s' "));
    assert!(commands[0].ends_with("> '/remote/repo/.kanvibe/status.json'"));
}

// parity: TS-lib-kanvibeProjectState-9bf616
#[test]
fn color_is_read_from_the_host_specific_project_json() {
    let fixture = Fixture::new();
    fixture.write_state(
        "project.json",
        r##"{"schemaVersion":1,"projectColor":"#0064FF"}"##,
    );

    let color =
        read_kanvibe_project_color(&fixture.files, &fixture.repo(), Some("ssh-host")).unwrap();

    assert_eq!(color.as_deref(), Some("#0064FF"));
    assert!(fixture.has_remote_call_for("project.json", "ssh-host"));
    assert_eq!(
        get_kanvibe_project_state_path("/local/repo"),
        "/local/repo/.kanvibe/project.json"
    );
}

// parity: TS-lib-kanvibeProjectState-7d8d37
#[test]
fn color_and_priority_writes_keep_each_other() {
    let fixture = Fixture::new();
    fixture.write_state(
        "project.json",
        r##"{"projectColor":"#65D08A","priority":"low"}"##,
    );

    write_kanvibe_project_color(&fixture.files, &fixture.repo(), "#0064FF", None).unwrap();
    let after_color = fixture.read_state_json("project.json");
    write_kanvibe_project_priority(
        &fixture.files,
        &fixture.repo(),
        Some(TaskPriority::High),
        None,
    )
    .unwrap();
    let after_priority = fixture.read_state_json("project.json");

    assert_eq!(
        (
            after_color["projectColor"].as_str(),
            after_color["priority"].as_str()
        ),
        (Some("#0064FF"), Some("low"))
    );
    assert_eq!(
        (
            after_priority["projectColor"].as_str(),
            after_priority["priority"].as_str()
        ),
        (Some("#0064FF"), Some("high"))
    );
}

// parity: TS-lib-kanvibeProjectState-f6299d
#[test]
fn malformed_colors_are_not_written() {
    let fixture = Fixture::new();

    write_kanvibe_project_color(&fixture.files, &fixture.repo(), "not-a-color", None).unwrap();

    assert!(!std::path::Path::new(&fixture.state_path("project.json")).exists());
}

// parity: TS-lib-kanvibeProjectState-2070d5
#[test]
fn seed_color_is_written_only_when_the_file_is_absent() {
    let fixture = Fixture::new();
    let seeded = Fixture::new();
    fixture.write_state("project.json", "authoritative\n");

    write_kanvibe_project_color_if_absent(&fixture.files, &fixture.repo(), "#0064FF", None)
        .unwrap();
    write_kanvibe_project_color_if_absent(&seeded.files, &seeded.repo(), "#0064FF", None).unwrap();

    assert_eq!(
        std::fs::read_to_string(fixture.state_path("project.json")).unwrap(),
        "authoritative\n"
    );
    assert_eq!(
        seeded.read_state_json("project.json")["projectColor"],
        "#0064FF"
    );
}

// parity: TS-lib-kanvibeProjectState-1657cc
#[test]
fn malformed_seed_colors_are_not_written() {
    let fixture = Fixture::new();

    write_kanvibe_project_color_if_absent(&fixture.files, &fixture.repo(), "not-a-color", None)
        .unwrap();

    assert!(!std::path::Path::new(&fixture.state_path("project.json")).exists());
}

// parity: TS-lib-kanvibeProjectState-5af8ce
#[test]
fn builds_and_parses_targets_json_as_client_url_to_task_mappings() {
    let content = build_kanvibe_targets_content(&[
        target("http://127.0.0.1:19736/", "task-local"),
        target("http://127.0.0.1:19736", "task-dev"),
        target("http://10.0.0.5:19736", "task-remote"),
    ]);

    let parsed = parse_json(&content);
    assert_eq!(
        without_updated_at(parsed.clone()),
        json!({
            "schemaVersion": 1,
            "targets": [
                { "url": "http://127.0.0.1:19736", "taskId": "task-local" },
                { "url": "http://10.0.0.5:19736", "taskId": "task-remote" },
            ],
        })
    );
    let reparsed = serde_json::to_value(parse_kanvibe_targets(&content).targets).unwrap();
    assert_eq!(reparsed, parsed["targets"]);
    assert_eq!(parse_kanvibe_targets("not-json").targets, []);
}

// parity: TS-lib-kanvibeProjectState-168ef2
#[test]
fn keeps_transport_and_drops_unknown_transports_instead_of_downgrading() {
    let targets = vec![
        KanvibeHookTarget {
            transport: Some(HookTargetTransport::Local),
            ..target("http://localhost:9736", "local-task")
        },
        KanvibeHookTarget {
            client_id: Some("client-a".into()),
            transport: Some(HookTargetTransport::Ssh),
            ..target("http://10.0.0.5:9736", "remote-task")
        },
    ];

    let round_trip = parse_kanvibe_targets(&build_kanvibe_targets_content(&targets)).targets;
    let invalid = parse_kanvibe_targets(
        r#"{"targets":[{"url":"http://localhost:9736","taskId":"local-task","transport":"invalid"}]}"#,
    );

    assert_eq!(round_trip, targets);
    assert_eq!(invalid.targets, []);
}

// parity: TS-lib-kanvibeProjectState-8a8359
#[test]
fn other_clients_watching_the_same_task_stay_registered() {
    let fixture = Fixture::new();
    fixture.write_state(
        "targets.json",
        r#"{"schemaVersion":1,"targets":[{"url":"http://10.0.0.5:19736","taskId":"shared-task"}]}"#,
    );

    upsert_kanvibe_hook_target(
        &fixture.files,
        &fixture.repo(),
        &target("http://127.0.0.1:19736/", "shared-task"),
        Some("ssh-host"),
    )
    .unwrap();

    assert!(fixture.has_remote_call_for("targets.json", "ssh-host"));
    assert_eq!(
        without_updated_at(fixture.read_state_json("targets.json")),
        json!({
            "schemaVersion": 1,
            "targets": [
                { "url": "http://10.0.0.5:19736", "taskId": "shared-task" },
                { "url": "http://127.0.0.1:19736", "taskId": "shared-task" },
            ],
        })
    );
}

// parity: TS-lib-kanvibeProjectState-c8c08d
#[test]
fn refreshes_my_token_keeps_peer_tokens_and_writes_targets_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.write_state("targets.json", r#"{"schemaVersion":1,"targets":[{"url":"http://10.0.0.5:19736","taskId":"peer-task","token":"peer-token"},{"url":"http://127.0.0.1:19736","taskId":"my-task","token":"old-token"}]}"#);

    let mine = with_token(target("http://127.0.0.1:19736", "my-task"), "new-token");
    upsert_kanvibe_hook_target(&fixture.files, &fixture.repo(), &mine, None).unwrap();

    assert_eq!(
        fixture.read_state_json("targets.json")["targets"],
        json!([
            { "url": "http://10.0.0.5:19736", "taskId": "peer-task", "token": "peer-token" },
            { "url": "http://127.0.0.1:19736", "taskId": "my-task", "token": "new-token" },
        ])
    );
    let mode = std::fs::metadata(fixture.state_path("targets.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

// parity: TS-lib-kanvibeProjectState-9f8c09
#[test]
fn reinstalling_the_same_client_for_another_task_replaces_only_the_task_id() {
    let fixture = Fixture::new();
    fixture.write_state("targets.json", r#"{"schemaVersion":1,"targets":[{"url":"http://127.0.0.1:19736","taskId":"old-task"},{"url":"http://10.0.0.5:19736","taskId":"other-client-task"}]}"#);

    upsert_kanvibe_hook_target(
        &fixture.files,
        &fixture.repo(),
        &target("http://127.0.0.1:19736", "new-task"),
        None,
    )
    .unwrap();

    assert_eq!(
        fixture.read_state_json("targets.json")["targets"],
        json!([
            { "url": "http://127.0.0.1:19736", "taskId": "new-task" },
            { "url": "http://10.0.0.5:19736", "taskId": "other-client-task" },
        ])
    );
    assert_eq!(
        get_kanvibe_targets_path("/local/repo"),
        "/local/repo/.kanvibe/targets.json"
    );
}

// parity: TS-lib-kanvibeProjectState-6d34fc
#[test]
fn reinstall_after_ip_change_replaces_the_stale_entry_and_its_old_address() {
    let stale_targets = [
        json!({ "url": "http://192.168.0.10:19736", "taskId": "my-task", "token": "old-token", "clientId": "client-a" }),
        json!({ "url": "http://192.168.0.10:19736", "taskId": "my-task", "token": "old-token" }),
    ];
    for stale_target in stale_targets {
        let fixture = Fixture::new();
        let peer = json!({ "url": "http://10.0.0.5:19736", "taskId": "peer-task", "token": "peer-token", "clientId": "client-b" });
        let existing = json!({ "schemaVersion": 1, "targets": [stale_target, peer.clone()] });
        fixture.write_state("targets.json", &existing.to_string());
        let mine = KanvibeHookTarget {
            client_id: Some("client-a".into()),
            ..with_token(target("http://192.168.0.23:19736", "my-task"), "new-token")
        };

        upsert_kanvibe_hook_target(&fixture.files, &fixture.repo(), &mine, Some("ssh-host"))
            .unwrap();

        let expected_mine = json!({ "url": "http://192.168.0.23:19736", "taskId": "my-task", "token": "new-token", "clientId": "client-a" });
        assert_eq!(
            fixture.read_state_json("targets.json")["targets"],
            json!([expected_mine, peer])
        );
    }
}

// parity: TS-lib-kanvibeProjectState-1e84ef
#[test]
fn a_target_counts_as_registered_only_when_url_and_task_both_match() {
    let content =
        r#"{"schemaVersion":1,"targets":[{"url":"http://127.0.0.1:19736","taskId":"task-1"}]}"#;

    assert!(has_kanvibe_hook_target(
        content,
        &target("http://127.0.0.1:19736/", "task-1")
    ));
    assert!(!has_kanvibe_hook_target(
        content,
        &target("http://127.0.0.1:19736", "task-2")
    ));
    assert!(!has_kanvibe_hook_target(
        content,
        &target("http://10.0.0.5:19736", "task-1")
    ));
    assert!(!has_kanvibe_hook_target(
        "",
        &target("http://127.0.0.1:19736", "task-1")
    ));
}

// parity: TS-lib-kanvibeProjectState-dccfb4
#[test]
fn task_description_is_written_to_task_json_not_status_json() {
    let fixture = Fixture::new();
    let metadata = KanvibeTaskMetadataUpdate {
        description: Some(Some("결제 실패 로그 원인 추적".into())),
        priority: None,
    };

    write_kanvibe_task_metadata(&fixture.files, &fixture.repo(), &metadata, None).unwrap();

    assert_eq!(
        without_updated_at(fixture.read_state_json("task.json")),
        json!({ "schemaVersion": 1, "description": "결제 실패 로그 원인 추적" })
    );
    assert!(!std::path::Path::new(&fixture.state_path("status.json")).exists());
    assert_eq!(
        get_kanvibe_task_description_path("/local/repo"),
        "/local/repo/.kanvibe/task.json"
    );
    assert_eq!(
        get_kanvibe_task_description_path("/remote/repo"),
        "/remote/repo/.kanvibe/task.json"
    );
}

// parity: TS-lib-kanvibeProjectState-eb55b3
#[test]
fn task_priority_is_written_to_task_json_with_the_description() {
    let content = build_kanvibe_task_description_content(
        Some(Some("결제 실패 로그 원인 추적".into())),
        Some(Some(TaskPriority::Low)),
        None,
    );

    let expected =
        json!({ "schemaVersion": 1, "description": "결제 실패 로그 원인 추적", "priority": "low" });
    assert_eq!(without_updated_at(parse_json(&content)), expected);
    let parsed = parse_kanvibe_task_description(&content).unwrap();
    assert_eq!(
        parsed.description,
        Some(Some("결제 실패 로그 원인 추적".into()))
    );
    assert_eq!(parsed.priority, Some(Some(TaskPriority::Low)));
    assert!(parsed.updated_at.is_some());
}

// parity: TS-lib-kanvibeProjectState-d0b64f
#[test]
fn clearing_the_description_writes_null_so_other_devices_clear_it() {
    let content = build_kanvibe_task_description_content(Some(None), None, None);

    assert_eq!(
        without_updated_at(parse_json(&content)),
        json!({ "schemaVersion": 1, "description": null })
    );
    assert_eq!(
        parse_kanvibe_task_description(&content)
            .unwrap()
            .description,
        Some(None)
    );
}

// parity: TS-lib-kanvibeProjectState-4903b6
#[test]
fn session_write_keeps_the_existing_description_and_priority() {
    let fixture = Fixture::new();
    let existing = build_kanvibe_task_description_content(
        Some(Some("결제 로그 추적".into())),
        Some(Some(TaskPriority::High)),
        None,
    );
    fixture.write_state("task.json", &existing);
    let session = KanvibeTaskSession {
        session_type: SessionType::Zellij,
        session_name: "repo-feat".into(),
    };

    write_kanvibe_task_session(
        &fixture.files,
        &fixture.repo(),
        &session,
        Some("build-host"),
    )
    .unwrap();

    assert!(fixture.has_remote_call_for("task.json", "build-host"));
    assert_eq!(
        without_updated_at(fixture.read_state_json("task.json")),
        json!({
            "schemaVersion": 1,
            "description": "결제 로그 추적",
            "priority": "high",
            "session": { "sessionType": "zellij", "sessionName": "repo-feat" },
        })
    );
}

// parity: TS-lib-kanvibeProjectState-7c13e6
#[test]
fn description_write_keeps_the_existing_session() {
    let fixture = Fixture::new();
    let session = KanvibeTaskSession {
        session_type: SessionType::Tmux,
        session_name: "repo-feat".into(),
    };
    fixture.write_state(
        "task.json",
        &build_kanvibe_task_description_content(None, None, Some(session)),
    );
    let metadata = KanvibeTaskMetadataUpdate {
        description: Some(Some("새 설명".into())),
        priority: None,
    };

    write_kanvibe_task_metadata(&fixture.files, &fixture.repo(), &metadata, None).unwrap();

    assert_eq!(
        without_updated_at(fixture.read_state_json("task.json")),
        json!({
            "schemaVersion": 1,
            "description": "새 설명",
            "session": { "sessionType": "tmux", "sessionName": "repo-feat" },
        })
    );
}

// parity: TS-lib-kanvibeProjectState-028247
#[test]
fn reads_session_only_task_json_and_ignores_unknown_types_or_empty_names() {
    let session_only = parse_kanvibe_task_description(
        r#"{"session":{"sessionType":"zellij","sessionName":"repo-feat"}}"#,
    );
    let unknown_type = parse_kanvibe_task_description(
        r#"{"session":{"sessionType":"screen","sessionName":"repo-feat"}}"#,
    );
    let empty_name = parse_kanvibe_task_description(
        r#"{"description":"설명","session":{"sessionType":"tmux","sessionName":""}}"#,
    );

    let expected = KanvibeTaskSession {
        session_type: SessionType::Zellij,
        session_name: "repo-feat".into(),
    };
    assert_eq!(session_only.unwrap().session, Some(expected));
    assert_eq!(unknown_type, None);
    assert_eq!(empty_name.unwrap().session, None);
}

// parity: TS-lib-kanvibeProjectState-633990
#[test]
fn sync_state_includes_the_session_from_task_json() {
    let fixture = Fixture::new();
    fixture.write_state(
        "task.json",
        r#"{"session":{"sessionType":"herdr","sessionName":"repo-feat"}}"#,
    );

    let sync_state =
        read_kanvibe_task_sync_state(&fixture.files, &fixture.repo(), None, None).unwrap();

    let expected = KanvibeTaskSession {
        session_type: SessionType::Herdr,
        session_name: "repo-feat".into(),
    };
    assert_eq!(sync_state.session, Some(expected));
    assert_eq!(sync_state.description, None);
}

// parity: TS-lib-kanvibeProjectState-48b1e6
#[test]
fn missing_or_malformed_description_files_mean_no_information() {
    assert_eq!(parse_kanvibe_task_description(""), None);
    assert_eq!(parse_kanvibe_task_description("not json"), None);
    assert_eq!(
        parse_kanvibe_task_description(r#"{"schemaVersion":1}"#),
        None
    );
    assert_eq!(
        parse_kanvibe_task_description(r#"{"description":123}"#),
        None
    );
}

// parity: TS-lib-kanvibeProjectState-dc4ac5
#[test]
fn reads_status_and_description_in_one_remote_round_trip() {
    let fixture = Fixture::new();
    fixture.write_state("status.json", r#"{"status":"review"}"#);
    fixture.write_state("task.json", r#"{"description":"결제 실패 로그 원인 추적"}"#);

    let sync_state =
        read_kanvibe_task_sync_state(&fixture.files, &fixture.repo(), Some("ssh-host"), None)
            .unwrap();

    assert_eq!(sync_state.status, Some(TaskStatus::Review));
    let description = sync_state
        .description
        .and_then(|metadata| metadata.description);
    assert_eq!(description, Some(Some("결제 실패 로그 원인 추적".into())));
    assert_eq!(fixture.runner.calls.lock().unwrap().len(), 1);
    let expected_paths: Vec<String> = ["status.json", "task.json", "project.json"]
        .iter()
        .map(|file_name| fixture.state_path(file_name))
        .collect();
    assert_eq!(fixture.remote_manifest_paths(), expected_paths);
}

// parity: TS-lib-kanvibeProjectState-ef04c7
#[test]
fn task_priority_wins_and_project_priority_is_returned_alongside() {
    let cases = [
        (r#"{"priority":"low"}"#, Some(Some(TaskPriority::Low))),
        (r#"{"description":null}"#, None),
        (r#"{"priority":null}"#, Some(None)),
    ];
    for (task_content, expected_priority) in cases {
        let task_repo = Fixture::new();
        let project_repo = Fixture::new();
        task_repo.write_state("task.json", task_content);
        project_repo.write_state("project.json", r#"{"priority":"high"}"#);

        let sync_state = read_kanvibe_task_sync_state(
            &task_repo.files,
            &task_repo.repo(),
            None,
            Some(&project_repo.repo()),
        )
        .unwrap();

        assert_eq!(sync_state.priority, expected_priority, "{task_content}");
        assert_eq!(sync_state.project_priority, Some(Some(TaskPriority::High)));
    }
}

// parity: TS-lib-kanvibeProjectState-53ef58
#[test]
fn missing_status_or_description_files_are_each_reported_as_absent() {
    let fixture = Fixture::new();
    fixture.write_state("status.json", r#"{"status":"review"}"#);

    let sync_state =
        read_kanvibe_task_sync_state(&fixture.files, &fixture.repo(), None, None).unwrap();

    assert_eq!(
        sync_state,
        KanvibeTaskSyncState {
            status: Some(TaskStatus::Review),
            ..KanvibeTaskSyncState::default()
        }
    );
}

#[test]
fn joins_paths_like_node_path_join() {
    assert_eq!(
        join_posix_path(&["/repo/", ".kanvibe", "task.json"]),
        "/repo/.kanvibe/task.json"
    );
    assert_eq!(
        join_posix_path(&["/repo/sub/..", ".kanvibe"]),
        "/repo/.kanvibe"
    );
    assert_eq!(join_posix_path(&["", ".kanvibe", "x"]), ".kanvibe/x");
    assert_eq!(join_posix_path(&["../a", "..", "..", "b"]), "../../b");
}

#[test]
fn iso_timestamps_match_node_to_iso_string() {
    assert!(is_iso_timestamp(&content::iso_timestamp_now()));
}
