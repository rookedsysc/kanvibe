//! 생성한 task.sh를 실제 bash로 실행하고, TS 테스트가 쓰던 task CLI 라우터(`electron/taskCliRoutes.js`)와
//! 같은 응답을 주는 가짜 client로 요청을 기록한다.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;
use crate::hooks::script_test_support::{
    RecordingServer, ShellRun, git, narrow_test_environment, reserve_closed_url, run_program,
};

#[derive(Debug, Clone, PartialEq)]
struct RecordedCall {
    function_name: &'static str,
    authorization: Option<String>,
    body: Value,
}

/// 서비스가 던진 `TaskCliRequestError`(상태 코드, 메시지)
type TaskCliRequestError = (u16, &'static str);

fn route_function_name(path: &str) -> Option<(&'static str, u16)> {
    match path {
        "/api/cli/tasks" => Some(("createTaskFromCli", 201)),
        "/api/cli/tasks/delete" => Some(("deleteTaskFromCli", 200)),
        "/api/cli/tasks/register" => Some(("registerTaskFromPeer", 200)),
        "/api/cli/tasks/list" => Some(("listTasksFromCli", 200)),
        "/api/cli/tasks/session" => Some(("prepareTaskSessionFromCli", 200)),
        _ => None,
    }
}

/// 실제 hook 서버와 같은 task CLI 라우팅·응답 모양을 쓰고, 서비스만 호출 기록으로 바꾼 client
struct FakeClient {
    server: RecordingServer,
}

impl FakeClient {
    fn start() -> Self {
        Self::start_with(|call| Ok(json!({ "handledBy": call.function_name })))
    }

    fn start_with(
        respond: impl Fn(&RecordedCall) -> Result<Value, TaskCliRequestError> + Send + Sync + 'static,
    ) -> Self {
        let server = RecordingServer::tcp_with(move |request| {
            let Some((function_name, success_status)) = route_function_name(&request.path) else {
                return (404, "{}".to_string());
            };
            let call = RecordedCall {
                function_name,
                authorization: request.authorization.clone(),
                body: serde_json::from_str(&request.body).unwrap_or(Value::Null),
            };
            match respond(&call) {
                Ok(data) => (
                    success_status,
                    format!("{{\"success\":true,\"data\":{data}}}"),
                ),
                Err((status, message)) => (
                    status,
                    format!("{{\"success\":false,\"error\":{}}}", Value::from(message)),
                ),
            }
        });
        Self { server }
    }

    fn url(&self) -> &str {
        &self.server.url
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.server
            .received()
            .into_iter()
            .filter_map(|request| {
                let (function_name, _) = route_function_name(&request.path)?;
                Some(RecordedCall {
                    function_name,
                    authorization: request.authorization,
                    body: serde_json::from_str(&request.body).ok()?,
                })
            })
            .collect()
    }
}

/// 공백이 섞인 경로에 현재 브랜치 `feat/current` 커밋 하나가 있는 저장소와 그 안의 task.sh
struct TaskCliWorkspace {
    workspace: tempfile::TempDir,
    repo_dir: PathBuf,
}

impl TaskCliWorkspace {
    fn new() -> Self {
        let workspace = tempfile::Builder::new()
            .prefix("kanvibe cli ")
            .tempdir()
            .unwrap();
        let repo_dir = workspace.path().join("repo with space");
        std::fs::create_dir_all(repo_dir.join("nested dir")).unwrap();
        let fixture = Self {
            workspace,
            repo_dir,
        };
        fixture.git(&["init", "-q", "-b", "feat/current"]);
        fixture.commit("init", &["--allow-empty"]);
        let task_cli_file = build_task_cli_file(fixture.repo(), None);
        std::fs::create_dir_all(Path::new(&task_cli_file.file_path).parent().unwrap()).unwrap();
        std::fs::write(&task_cli_file.file_path, &task_cli_file.content).unwrap();
        let mode = std::fs::Permissions::from_mode(task_cli_file.mode.unwrap());
        std::fs::set_permissions(&task_cli_file.file_path, mode).unwrap();
        fixture
    }

    fn repo(&self) -> &str {
        self.repo_dir.to_str().unwrap()
    }

    fn git(&self, arguments: &[&str]) {
        let mut full_arguments = vec!["-C", self.repo()];
        full_arguments.extend_from_slice(arguments);
        git(&full_arguments, self.workspace.path());
    }

    fn commit(&self, message: &str, extra_arguments: &[&str]) {
        let mut arguments = vec![
            "-c",
            "user.name=kanvibe",
            "-c",
            "user.email=kanvibe@example.com",
            "commit",
            "-q",
        ];
        arguments.extend_from_slice(extra_arguments);
        arguments.extend_from_slice(&["-m", message]);
        self.git(&arguments);
    }

    fn write_targets(&self, targets: Value) {
        let content =
            serde_json::to_string_pretty(&json!({ "schemaVersion": 1, "targets": targets }))
                .unwrap();
        std::fs::write(self.repo_dir.join(".kanvibe/targets.json"), content).unwrap();
    }

    fn run_cli(&self, arguments: &[&str]) -> ShellRun {
        self.run_cli_in(arguments, &self.repo_dir, None)
    }

    /// cwd 기준 상대 경로로 task.sh를 실행한다. path_override가 있으면 PATH를 바꾼다
    fn run_cli_in(
        &self,
        arguments: &[&str],
        cwd: &Path,
        path_override: Option<String>,
    ) -> ShellRun {
        let script_path = self.repo_dir.join(".kanvibe/task.sh");
        let relative_script = if cwd == self.repo_dir {
            ".kanvibe/task.sh".to_string()
        } else {
            "../.kanvibe/task.sh".to_string()
        };
        assert!(script_path.exists());
        let mut environment = narrow_test_environment(self.workspace.path());
        if let Some(path) = path_override {
            environment.insert("PATH".into(), path);
        }
        let mut full_arguments = vec![relative_script.as_str()];
        full_arguments.extend_from_slice(arguments);
        run_program("bash", &full_arguments, cwd, &environment, b"")
    }

    /// 꺼진 PC처럼 패킷을 버리는 주소로 가는 요청만 curl의 연결 시간 초과처럼 끝내는 curl을 PATH 앞에 둔다
    fn timing_out_curl_path(&self, timing_out_url: &str, connect_seconds: &str) -> String {
        let real_curl = run_program(
            "bash",
            &["-c", "command -v curl"],
            self.workspace.path(),
            &narrow_test_environment(self.workspace.path()),
            b"",
        );
        let fake_bin_directory = self.workspace.path().join("fake bin");
        std::fs::create_dir_all(&fake_bin_directory).unwrap();
        let fake_curl_path = fake_bin_directory.join("curl");
        let script = [
            "#!/usr/bin/env bash".to_string(),
            "for argument in \"$@\"; do".to_string(),
            format!("  if [[ \"$argument\" == \"{timing_out_url}\"* ]]; then"),
            "    cat >/dev/null".to_string(),
            format!("    printf '\\n%s\\n000' \"{connect_seconds}\""),
            "    exit 28".to_string(),
            "  fi".to_string(),
            "done".to_string(),
            format!("exec \"{}\" \"$@\"", real_curl.stdout.trim()),
            String::new(),
        ]
        .join("\n");
        std::fs::write(&fake_curl_path, script).unwrap();
        std::fs::set_permissions(&fake_curl_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        format!(
            "{}:{}",
            fake_bin_directory.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }
}

fn target(url: &str, task_id: &str, token: &str) -> Value {
    json!({ "url": url, "taskId": task_id, "token": token })
}

#[test]
fn create_from_a_subdirectory_uses_the_worktree_branch_and_only_the_first_client() {
    let fixture = TaskCliWorkspace::new();
    let first_client = FakeClient::start();
    let second_client = FakeClient::start();
    fixture.write_targets(json!([
        target(
            &format!("{}/", first_client.url()),
            "task-first",
            "token-first"
        ),
        target(second_client.url(), "task-second", "token-second"),
    ]));
    let title = "로그인 \"수정\" $HOME `id` \\ 끝";

    let run = fixture.run_cli_in(
        &[
            "create",
            "--branch",
            "feat/new",
            "--title",
            title,
            "--session-type",
            "tmux",
        ],
        &fixture.repo_dir.join("nested dir"),
        None,
    );

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&run.stdout).unwrap(),
        json!({ "success": true, "data": { "handledBy": "createTaskFromCli" } })
    );
    assert_eq!(
        first_client.calls(),
        [RecordedCall {
            function_name: "createTaskFromCli",
            authorization: Some("Bearer token-first".into()),
            body: json!({
                "contextTaskId": "task-first",
                "branchName": "feat/new",
                "title": title,
                "sessionType": "tmux",
                "baseBranch": "feat/current",
            }),
        }]
    );
    assert!(second_client.calls().is_empty());
}

#[test]
fn control_characters_in_the_title_are_json_escaped() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start();
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));
    let title = "줄\n바꿈\t탭\u{0001}제어\u{001f}끝 \"따옴표\" \\";

    let run = fixture.run_cli(&["create", "--branch", "feat/new", "--title", title]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(client.calls()[0].body["title"], title);
}

#[test]
fn from_task_sends_the_task_id_instead_of_the_current_branch() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start();
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));
    fixture.git(&["checkout", "-q", "--detach"]);

    let run = fixture.run_cli(&[
        "create",
        "--branch",
        "feat/followup",
        "--from-task",
        "base-task",
    ]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        client.calls()[0].body,
        json!({ "contextTaskId": "task-1", "branchName": "feat/followup", "fromTaskId": "base-task" })
    );
}

#[test]
fn detached_head_without_a_base_task_is_a_usage_error() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start();
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));
    fixture.git(&["checkout", "-q", "--detach"]);

    let run = fixture.run_cli(&["create", "--branch", "feat/new"]);

    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("detached HEAD"));
    assert!(client.calls().is_empty());
}

#[test]
fn invalid_create_arguments_are_usage_errors_without_requests() {
    let cases: [(&[&str], &str); 3] = [
        (&["create"], "--branch는 필수입니다"),
        (&["create", "--branch"], "--branch 에 값이 필요합니다"),
        (
            &["create", "--branch", "feat/new", "--unknown"],
            "알 수 없는 옵션입니다: --unknown",
        ),
    ];
    for (arguments, message) in cases {
        let fixture = TaskCliWorkspace::new();
        let client = FakeClient::start();
        fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

        let run = fixture.run_cli(arguments);

        assert_eq!(run.code, 2, "{arguments:?}");
        assert!(run.stderr.contains(message), "{}", run.stderr);
        assert!(client.calls().is_empty());
    }
}

#[test]
fn only_an_unreachable_client_moves_on_to_the_next_client() {
    let fixture = TaskCliWorkspace::new();
    let closed_url = reserve_closed_url();
    let reachable_client = FakeClient::start();
    fixture.write_targets(json!([
        target(&closed_url, "task-offline", "token-offline"),
        target(reachable_client.url(), "task-online", "token-online"),
    ]));

    let run = fixture.run_cli(&["create", "--branch", "feat/new"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        run.stderr
            .contains("연결할 수 없어 다음 client를 시도합니다")
    );
    let context_task_ids: Vec<Value> = reachable_client
        .calls()
        .into_iter()
        .map(|call| call.body["contextTaskId"].clone())
        .collect();
    assert_eq!(context_task_ids, [json!("task-online")]);
}

#[test]
fn silent_client_is_a_connect_timeout_and_the_next_client_is_tried() {
    let fixture = TaskCliWorkspace::new();
    let silent_url = "http://localhost:9736";
    let reachable_client = FakeClient::start();
    fixture.write_targets(json!([
        target(silent_url, "task-sleeping", "token-sleeping"),
        target(reachable_client.url(), "task-online", "token-online"),
    ]));
    let path = fixture.timing_out_curl_path(silent_url, "0.000000");

    let run = fixture.run_cli_in(
        &["create", "--branch", "feat/new"],
        &fixture.repo_dir,
        Some(path),
    );

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stderr.contains(&format!(
        "{silent_url} 에 연결할 수 없어 다음 client를 시도합니다"
    )));
    assert_eq!(
        reachable_client.calls()[0].body["contextTaskId"],
        "task-online"
    );
    assert_eq!(reachable_client.calls().len(), 1);
}

#[test]
fn timeout_after_connecting_is_not_retried_on_another_client() {
    let fixture = TaskCliWorkspace::new();
    let slow_url = "http://localhost:9736";
    let other_client = FakeClient::start();
    fixture.write_targets(json!([
        target(slow_url, "task-slow", "token-slow"),
        target(other_client.url(), "task-other", "token-other"),
    ]));
    let path = fixture.timing_out_curl_path(slow_url, "0.000349");

    let run = fixture.run_cli_in(
        &["create", "--branch", "feat/new"],
        &fixture.repo_dir,
        Some(path),
    );

    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("curl 28"));
    assert!(other_client.calls().is_empty());
}

#[test]
fn answered_failure_is_not_retried_and_reports_the_error_body() {
    let fixture = TaskCliWorkspace::new();
    let failing_client =
        FakeClient::start_with(|_| Err((409, "이미 같은 브랜치의 task가 있습니다: feat/new")));
    let other_client = FakeClient::start();
    fixture.write_targets(json!([
        target(failing_client.url(), "task-1", "token-1"),
        target(other_client.url(), "task-2", "token-2"),
    ]));

    let run = fixture.run_cli(&["create", "--branch", "feat/new"]);

    assert_eq!(run.code, 1);
    assert_eq!(run.stdout, "");
    assert!(run.stderr.contains("HTTP 409"));
    assert!(run.stderr.contains("이미 같은 브랜치의 task가 있습니다"));
    assert!(other_client.calls().is_empty());
}

#[test]
fn target_url_selects_one_client_and_unknown_urls_are_usage_errors() {
    let fixture = TaskCliWorkspace::new();
    let first_client = FakeClient::start();
    let second_client = FakeClient::start();
    fixture.write_targets(json!([
        target(first_client.url(), "task-1", "token-1"),
        target(second_client.url(), "task-2", "token-2"),
    ]));
    let selected_url = format!("{}/", second_client.url());

    let selected_run = fixture.run_cli(&[
        "create",
        "--branch",
        "feat/new",
        "--target-url",
        &selected_url,
    ]);
    let unknown_run = fixture.run_cli(&[
        "create",
        "--branch",
        "feat/new",
        "--target-url",
        "http://127.0.0.1:1",
    ]);

    assert_eq!(selected_run.code, 0, "{}", selected_run.stderr);
    assert!(first_client.calls().is_empty());
    let authorizations: Vec<Option<String>> = second_client
        .calls()
        .into_iter()
        .map(|call| call.authorization)
        .collect();
    assert_eq!(authorizations, [Some("Bearer token-2".to_string())]);
    assert_eq!(unknown_run.code, 2);
    assert!(unknown_run.stderr.contains("등록된 client가 아닙니다"));
}

#[test]
fn description_and_priority_are_sent_in_the_body() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start();
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

    let run = fixture.run_cli(&[
        "create",
        "--branch",
        "feat/new",
        "--description",
        "로그인\n오류 수정",
        "--priority",
        "high",
    ]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let body = &client.calls()[0].body;
    assert_eq!(body["description"], "로그인\n오류 수정");
    assert_eq!(body["priority"], "high");
}

fn peers_response(is_second_success: bool, data: Value) -> Value {
    let mut data = data;
    data["peers"] = json!([
        { "url": "http://10.0.0.7:9736", "isSuccess": !is_second_success, "httpStatus": "000", "message": "연결할 수 없습니다" },
        { "url": "http://10.0.0.8:9736", "isSuccess": is_second_success, "httpStatus": "200", "message": "ok" },
    ]);
    data
}

#[test]
fn create_succeeds_but_warns_about_unpropagated_peers() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start_with(|_| {
        Ok(peers_response(
            true,
            json!({ "id": "created-task", "session": { "isPrepared": true } }),
        ))
    });
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

    let run = fixture.run_cli(&["create", "--branch", "feat/new"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&run.stdout).unwrap()["data"]["id"],
        "created-task"
    );
    assert!(
        run.stderr
            .contains("1곳에는 task 등록을 전파하지 못했습니다")
    );
}

#[test]
fn not_found_on_the_first_live_client_suggests_another_target_url() {
    let fixture = TaskCliWorkspace::new();
    let answering_client =
        FakeClient::start_with(|_| Err((404, "feat/other 브랜치의 task를 찾을 수 없습니다.")));
    let owner_client = FakeClient::start();
    fixture.write_targets(json!([
        target(answering_client.url(), "task-1", "token-1"),
        target(owner_client.url(), "task-2", "token-2"),
    ]));

    let run = fixture.run_cli(&["delete", "--yes", "--branch", "feat/other"]);

    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("--target-url로 지정해 다시 실행하세요"));
    assert!(run.stderr.contains(owner_client.url()));
    assert!(owner_client.calls().is_empty());
}

#[test]
fn not_found_hint_is_skipped_for_selected_or_single_clients() {
    let fixture = TaskCliWorkspace::new();
    let only_client = FakeClient::start_with(|_| Err((404, "task를 찾을 수 없습니다")));
    fixture.write_targets(json!([target(only_client.url(), "task-1", "token-1")]));
    let single_run = fixture.run_cli(&["session", "--branch", "feat/other"]);

    let other_client = FakeClient::start();
    fixture.write_targets(json!([
        target(only_client.url(), "task-1", "token-1"),
        target(other_client.url(), "task-2", "token-2"),
    ]));
    let selected_run = fixture.run_cli(&[
        "session",
        "--branch",
        "feat/other",
        "--target-url",
        only_client.url(),
    ]);

    assert_eq!([single_run.code, selected_run.code], [1, 1]);
    assert!(!single_run.stderr.contains("--target-url로 지정해"));
    assert!(!selected_run.stderr.contains("--target-url로 지정해"));
}

#[test]
fn registrations_without_cli_tokens_ask_to_reinstall_hooks() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start();
    fixture.write_targets(json!([{ "url": client.url(), "taskId": "task-1" }]));

    let run = fixture.run_cli(&["create", "--branch", "feat/new"]);

    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("hooks를 다시 설치하세요"));
    assert!(client.calls().is_empty());
}

#[test]
fn list_requests_the_list_route_with_the_current_task_token() {
    let fixture = TaskCliWorkspace::new();
    let client =
        FakeClient::start_with(|_| Ok(json!([{ "id": "task-1", "branchName": "feat/current" }])));
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

    let run = fixture.run_cli(&["list"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&run.stdout).unwrap(),
        json!({ "success": true, "data": [{ "id": "task-1", "branchName": "feat/current" }] })
    );
    assert_eq!(
        client.calls(),
        [RecordedCall {
            function_name: "listTasksFromCli",
            authorization: Some("Bearer token-1".into()),
            body: json!({ "contextTaskId": "task-1" }),
        }]
    );
}

#[test]
fn session_targets_the_given_branch_or_the_current_task() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start();
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

    let current_run = fixture.run_cli(&["session"]);
    let branch_run = fixture.run_cli(&["session", "--branch", "feat/sibling"]);

    assert_eq!([current_run.code, branch_run.code], [0, 0]);
    let calls: Vec<(&str, Value)> = client
        .calls()
        .into_iter()
        .map(|call| (call.function_name, call.body))
        .collect();
    assert_eq!(
        calls,
        [
            (
                "prepareTaskSessionFromCli",
                json!({ "contextTaskId": "task-1" })
            ),
            (
                "prepareTaskSessionFromCli",
                json!({ "contextTaskId": "task-1", "branchName": "feat/sibling" })
            ),
        ]
    );
}

#[test]
fn delete_without_yes_sends_nothing() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start();
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

    let run = fixture.run_cli(&["delete"]);

    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("--yes"));
    assert!(client.calls().is_empty());
}

#[test]
fn delete_sends_confirmation_force_and_branch_to_one_client() {
    let fixture = TaskCliWorkspace::new();
    let first_client = FakeClient::start();
    let second_client = FakeClient::start();
    fixture.write_targets(json!([
        target(first_client.url(), "task-1", "token-1"),
        target(second_client.url(), "task-2", "token-2"),
    ]));

    let default_run = fixture.run_cli(&["delete", "--yes"]);
    let forced_run = fixture.run_cli(&["delete", "--yes", "--force", "--branch", "feat/sibling"]);

    assert_eq!([default_run.code, forced_run.code], [0, 0]);
    let bodies: Vec<Value> = first_client
        .calls()
        .into_iter()
        .map(|call| call.body)
        .collect();
    assert_eq!(
        bodies,
        [
            json!({ "contextTaskId": "task-1", "confirmed": true, "force": false }),
            json!({ "contextTaskId": "task-1", "confirmed": true, "force": true, "branchName": "feat/sibling" }),
        ]
    );
    assert!(second_client.calls().is_empty());
}

#[test]
fn delete_succeeds_but_warns_about_unpropagated_peers() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start_with(|_| {
        Ok(peers_response(
            false,
            json!({ "id": "task-1", "deleted": true }),
        ))
    });
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

    let run = fixture.run_cli(&["delete", "--yes"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&run.stdout).unwrap()["data"]["deleted"],
        true
    );
    assert!(run.stderr.contains("1곳에는 삭제를 전파하지 못했습니다"));
}

#[test]
fn fully_propagated_delete_prints_no_warning() {
    let fixture = TaskCliWorkspace::new();
    let client = FakeClient::start_with(|_| {
        Ok(json!({
            "id": "task-1",
            "deleted": true,
            "peers": [{ "url": "http://10.0.0.7:9736", "isSuccess": true, "httpStatus": "200", "message": "ok" }],
        }))
    });
    fixture.write_targets(json!([target(client.url(), "task-1", "token-1")]));

    let run = fixture.run_cli(&["delete", "--yes"]);

    assert_eq!(run.code, 0);
    assert_eq!(run.stderr, "");
}

#[test]
fn propagate_delete_reaches_every_other_client_with_its_own_task_and_token() {
    let fixture = TaskCliWorkspace::new();
    let own_client = FakeClient::start();
    let peer_client = FakeClient::start();
    let closed_url = reserve_closed_url();
    fixture.write_targets(json!([
        target(own_client.url(), "task-own", "token-own"),
        target(peer_client.url(), "task-peer", "token-peer"),
        target(&closed_url, "task-offline", "token-offline"),
    ]));

    let run = fixture.run_cli(&["propagate-delete", "--exclude-url", own_client.url()]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(own_client.calls().is_empty());
    assert_eq!(
        peer_client.calls(),
        [RecordedCall {
            function_name: "deleteTaskFromCli",
            authorization: Some("Bearer token-peer".into()),
            body: json!({ "contextTaskId": "task-peer", "confirmed": true, "force": true, "propagateToPeers": false }),
        }]
    );
    let lines: Vec<&str> = run.stdout.trim().lines().collect();
    assert_eq!(
        lines,
        [
            format!(
                "__KANVIBE_PEER_RESULT__\t{}\t200\t{{\"success\":true,\"data\":{{\"handledBy\":\"deleteTaskFromCli\"}}}}",
                peer_client.url()
            ),
            format!("__KANVIBE_PEER_RESULT__\t{closed_url}\t000\t연결할 수 없습니다"),
        ]
    );
}

#[test]
fn propagate_register_only_registers_the_new_branch_on_other_clients() {
    let fixture = TaskCliWorkspace::new();
    let own_client = FakeClient::start();
    let peer_client = FakeClient::start();
    fixture.write_targets(json!([
        target(own_client.url(), "root-own", "token-own"),
        target(peer_client.url(), "root-peer", "token-peer"),
    ]));

    let run = fixture.run_cli(&[
        "propagate-register",
        "--exclude-url",
        own_client.url(),
        "--branch",
        "feat/new",
    ]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        peer_client.calls(),
        [RecordedCall {
            function_name: "registerTaskFromPeer",
            authorization: Some("Bearer token-peer".into()),
            body: json!({ "contextTaskId": "root-peer", "branchName": "feat/new" }),
        }]
    );
}

#[test]
fn tokens_are_never_passed_to_curl_as_arguments() {
    let content = build_task_cli_file("/repo", None).content;

    assert!(content.contains("printf 'Authorization: Bearer %s\\n' \"${token}\" | curl"));
    assert!(content.contains("-H @-"));
    let inline_authorization = regex::Regex::new(r#"-H\s+["']Authorization"#).unwrap();
    assert!(!inline_authorization.is_match(&content));
}

const REPLACED_SCRIPT_MARKER: &str = "replaced-script-ran";

/// 앱이 설치한 task.sh가 ignored 상태일 때 그 파일을 추적하는 브랜치를 checkout해 git이 덮어쓰게 한다
fn checkout_branch_that_replaces_task_cli(fixture: &TaskCliWorkspace) {
    let task_cli_path = fixture.repo_dir.join(".kanvibe/task.sh");
    let trusted_script = std::fs::read_to_string(&task_cli_path).unwrap();
    std::fs::write(fixture.repo_dir.join(".git/info/exclude"), ".kanvibe/\n").unwrap();
    fixture.git(&["checkout", "-q", "-b", "untrusted"]);
    let marker_path = fixture.workspace.path().join(REPLACED_SCRIPT_MARKER);
    std::fs::write(
        &task_cli_path,
        format!("#!/bin/bash\ntouch '{}'\n", marker_path.display()),
    )
    .unwrap();
    fixture.git(&["add", "-f", ".kanvibe/task.sh"]);
    fixture.commit("tracked task.sh", &[]);
    fixture.git(&["checkout", "-q", "feat/current"]);
    std::fs::write(&task_cli_path, trusted_script).unwrap();
    fixture.git(&["checkout", "-q", "untrusted"]);
    assert!(
        std::fs::read_to_string(&task_cli_path)
            .unwrap()
            .contains(REPLACED_SCRIPT_MARKER)
    );
}

fn run_sh(fixture: &TaskCliWorkspace, flag: &str, command: &str) -> ShellRun {
    run_program(
        "sh",
        &[flag, command],
        fixture.workspace.path(),
        &narrow_test_environment(fixture.workspace.path()),
        b"",
    )
}

#[test]
fn client_runs_its_own_script_instead_of_an_overwritten_task_sh() {
    let fixture = TaskCliWorkspace::new();
    let peer = FakeClient::start();
    fixture.write_targets(json!([target(peer.url(), "task-peer", "peer-token")]));
    checkout_branch_that_replaces_task_cli(&fixture);
    let command = build_task_cli_command(
        fixture.repo(),
        &["propagate-delete", "--exclude-url", "http://own:9736"],
        None,
    );

    let run = run_sh(&fixture, "-c", &command);

    assert!(
        !fixture
            .workspace
            .path()
            .join(REPLACED_SCRIPT_MARKER)
            .exists()
    );
    let function_names: Vec<&str> = peer
        .calls()
        .into_iter()
        .map(|call| call.function_name)
        .collect();
    assert_eq!(function_names, ["deleteTaskFromCli"]);
    assert!(
        run.stdout
            .contains(&format!("__KANVIBE_PEER_RESULT__\t{}\t200", peer.url()))
    );
}

#[test]
fn script_and_arguments_survive_two_nested_shells() {
    let fixture = TaskCliWorkspace::new();
    let peer = FakeClient::start();
    fixture.write_targets(json!([target(peer.url(), "task-peer", "peer-token")]));
    let command = build_task_cli_command(
        fixture.repo(),
        &["propagate-register", "--branch", "feat/it's new"],
        None,
    );
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));

    let run = run_sh(
        &fixture,
        "-lc",
        &format!("sh -lc {}", quote(&format!("sh -lc {}", quote(&command)))),
    );

    assert_eq!(run.code, 0, "{}", run.stderr);
    let call = &peer.calls()[0];
    assert_eq!(call.function_name, "registerTaskFromPeer");
    assert_eq!(call.body["contextTaskId"], "task-peer");
    assert_eq!(call.body["branchName"], "feat/it's new");
}

#[test]
fn option_or_non_http_target_urls_are_never_called() {
    let fixture = TaskCliWorkspace::new();
    let curl_config_directory = fixture.workspace.path().join("cfg");
    let written_path = fixture.workspace.path().join("WRITTEN");
    std::fs::create_dir_all(curl_config_directory.join("api/cli/tasks")).unwrap();
    std::fs::write(
        curl_config_directory.join("api/cli/tasks/delete"),
        format!(
            "url = \"file:///etc/hosts\"\noutput = \"{}\"\n",
            written_path.display()
        ),
    )
    .unwrap();
    fixture.write_targets(json!([
        target(
            &format!("-K{}", curl_config_directory.display()),
            "task-option",
            "token"
        ),
        target("file:///etc/hosts", "task-file", "token"),
    ]));
    let command = build_task_cli_command(
        fixture.repo(),
        &["propagate-delete", "--exclude-url", "http://own:9736"],
        None,
    );

    let run = run_sh(&fixture, "-c", &command);

    assert!(!written_path.exists());
    assert!(!run.stdout.contains(TASK_CLI_PEER_RESULT_PREFIX));
}

#[test]
fn posix_join_normalizes_like_node() {
    assert_eq!(
        join_posix_path(&["/srv/repo/", ".kanvibe", "task.sh"]),
        "/srv/repo/.kanvibe/task.sh"
    );
    assert_eq!(join_posix_path(&["/a/b/../c", "./d"]), "/a/c/d");
    assert_eq!(join_posix_path(&["repo", "..", "..", "x"]), "../x");
    assert_eq!(join_posix_path(&["/", "..", "x/"]), "/x/");
    assert_eq!(join_posix_path(&[""]), ".");
}
