use std::collections::BTreeMap;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{Value, json};

use super::*;
use crate::hooks::script_test_support::{git, narrow_test_environment, run_program};

/// 표준 시스템 디렉터리에서만 실제 실행 파일을 찾는다. PATH를 물려받지 못하는 설치 프로그램 환경을 그대로 재현한다
const SYSTEM_BIN_DIRS: [&str; 8] = [
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
    "/opt/homebrew/bin",
    "/home/linuxbrew/.linuxbrew/bin",
];

fn resolve_real_binary(name: &str) -> Option<PathBuf> {
    SYSTEM_BIN_DIRS
        .iter()
        .map(|directory| Path::new(directory).join(name))
        .find(|candidate| {
            std::fs::metadata(candidate).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
}

fn write_executable(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn review_updater() -> String {
    build_shell_kanvibe_status_updater(ShellHookStatus::Review, None)
}

// parity: TS-lib-shellHookScript-8eda33
#[test]
fn shell_resolver_pins_the_task_id_inside_the_hook_file() {
    let resolver = build_shell_task_id_resolver("fallback-task");

    assert_eq!(resolver, "TASK_ID=\"fallback-task\"");
    assert!(!resolver.contains(".kanvibe/task-id"));
}

// parity: TS-lib-shellHookScript-34676e
#[test]
fn pinned_task_id_is_read_back_and_missing_one_is_none() {
    let script = ["#!/bin/bash", "TASK_ID=\"task-123\"", "echo \"$TASK_ID\""].join("\n");

    assert_eq!(extract_shell_task_id(&script).as_deref(), Some("task-123"));
    assert_eq!(
        extract_shell_task_id("#!/bin/bash\necho \"$TASK_ID\""),
        None
    );
}

// parity: TS-lib-shellHookScript-38c44f
#[test]
fn task_id_resolver_escapes_double_quoted_shell_characters_reversibly() {
    let task_id = "task-\"quoted\"-\\-`cmd`-$HOME";

    let resolver = build_shell_task_id_resolver(task_id);

    assert_eq!(
        resolver,
        "TASK_ID=\"task-\\\"quoted\\\"-\\\\-\\`cmd\\`-\\$HOME\""
    );
    assert_eq!(extract_shell_task_id(&resolver).as_deref(), Some(task_id));
}

// parity: TS-lib-shellHookScript-918360
#[test]
fn status_updater_saves_status_json_and_fans_out_to_every_target() {
    let updater = review_updater();

    for fragment in [
        "KANVIBE_STATUS=\"review\"",
        "status.json",
        "targets.json",
        "KANVIBE_TARGETS_FILE",
        "--git-common-dir",
        "/info/exclude",
        ".kanvibe/",
        "grep -qxF",
        "\\\"schemaVersion\\\":1",
        "\\\"status\\\":\\\"${KANVIBE_STATUS}\\\"",
        "\\\"updatedAt\\\":\\\"${KANVIBE_UPDATED_AT}\\\"",
        "${KANVIBE_TASK_STATE_FILE}",
        "KANVIBE_TARGET_URL",
        "KANVIBE_TARGET_TASK_ID",
        "while IFS=",
    ] {
        assert!(updater.contains(fragment), "{fragment}");
    }
    assert!(!updater.contains("hooks-targets.json"));
    assert!(!updater.contains("task-state.json"));
}

// parity: TS-lib-shellHookScript-409fd4
#[test]
fn target_fanout_check_rejects_hooks_without_targets_fanout() {
    let updater = review_updater();
    let targets_start = updater.find("\nKANVIBE_TARGETS_FILE=").unwrap();
    let updater_without_targets = &updater[..targets_start];

    assert!(has_shell_kanvibe_target_fanout(&updater));
    assert!(!has_shell_kanvibe_target_fanout(updater_without_targets));
    assert!(!has_shell_kanvibe_target_fanout(&updater.replacen(
        HOOK_TRANSPORT_VERSION_MARKER,
        "",
        1
    )));
}

// parity: TS-lib-shellHookScript-b5dc79
#[test]
fn parallel_fanout_check_rejects_sequential_curl_loops() {
    let updater = review_updater();
    let sequential_updater = updater.replacen("> /dev/null 2>&1 &", "> /dev/null 2>&1 || true", 1);

    assert!(has_shell_kanvibe_parallel_target_fanout(&updater));
    assert!(!has_shell_kanvibe_parallel_target_fanout(
        &sequential_updater
    ));
}

// parity: TS-lib-shellHookScript-0be4ea
#[test]
fn notify_timeout_check_rejects_unbounded_curl() {
    let updater = review_updater();
    let unbounded_updater = updater.replacen("--max-time 3 ", "", 1);

    assert!(has_shell_kanvibe_bounded_notify_timeout(&updater));
    assert!(!has_shell_kanvibe_bounded_notify_timeout(
        &unbounded_updater
    ));
}

// parity: TS-lib-shellHookScript-2bc65c
#[test]
fn status_updater_does_not_touch_the_project_color() {
    let updater = review_updater();

    assert!(!updater.contains("projectColor"));
    assert!(!updater.contains("project.json"));
}

/// node 없이 hook이 쓰는 POSIX 도구만 sandbox PATH로 노출한다
fn create_tool_sandbox(sandbox_bin_directory: &Path, tools: &[&str]) {
    std::fs::create_dir_all(sandbox_bin_directory).unwrap();
    for tool in tools {
        let real = resolve_real_binary(tool)
            .unwrap_or_else(|| panic!("테스트에 필요한 {tool} 바이너리를 찾을 수 없습니다"));
        symlink(real, sandbox_bin_directory.join(tool)).unwrap();
    }
    if let Some(git_binary) = resolve_real_binary("git") {
        let _ = symlink(git_binary, sandbox_bin_directory.join("git"));
    }
}

fn write_targets(repo_path: &Path, targets: Value) {
    std::fs::create_dir_all(repo_path.join(".kanvibe")).unwrap();
    let content = json!({ "schemaVersion": 1, "targets": targets }).to_string();
    std::fs::write(repo_path.join(".kanvibe/targets.json"), content).unwrap();
}

/// 테스트 hook 파일을 쓰고 sandbox PATH만 가진 환경으로 실행한다
fn run_sandboxed_hook(repo_path: &Path, sandbox_bin_directory: &Path, curl_log_path: &Path) {
    let hook_directory = repo_path.join(".claude/hooks");
    std::fs::create_dir_all(&hook_directory).unwrap();
    let script_path = hook_directory.join("kanvibe-test-hook.sh");
    let script = [
        "#!/bin/bash".to_string(),
        "KANVIBE_URL=\"http://fallback:9736\"".to_string(),
        build_shell_task_id_resolver("fallback-task"),
        review_updater(),
    ]
    .join("\n");
    write_executable(&script_path, &script);
    let environment = BTreeMap::from([
        (
            "PATH".to_string(),
            sandbox_bin_directory.display().to_string(),
        ),
        ("HOME".to_string(), repo_path.display().to_string()),
        (
            "KANVIBE_CURL_LOG".to_string(),
            curl_log_path.display().to_string(),
        ),
    ]);
    let bash = resolve_real_binary("bash").unwrap();
    let run = run_program(
        bash.to_str().unwrap(),
        &[script_path.to_str().unwrap()],
        repo_path,
        &environment,
        b"",
    );
    assert_eq!(run.code, 0, "{}", run.stderr);
}

fn captured_value(line: &str, key: &str) -> Option<String> {
    let pattern = regex::Regex::new(&format!("\"{key}\":\\s*\"([^\"]+)\"")).unwrap();
    pattern
        .captures(line)
        .map(|captures| captures[1].to_string())
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// 가짜 curl 기록 한 줄에서 (taskId, 통보 주소, status)를 뽑아 taskId 순으로 정렬한다
fn status_deliveries(curl_log: &str) -> Vec<(String, String, String)> {
    let mut deliveries: Vec<(String, String, String)> = curl_log
        .trim()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let endpoint = line
                .split_whitespace()
                .find(|token| token.ends_with("/api/hooks/status"))
                .unwrap_or_default();
            (
                captured_value(line, "taskId").unwrap_or_default(),
                endpoint.to_string(),
                captured_value(line, "status").unwrap_or_default(),
            )
        })
        .collect();
    deliveries.sort();
    deliveries
}

/// 상태·색상 파일을 미리 둔 저장소와 node 없는 도구 sandbox를 준비한다
fn prepare_fanout_repo(repo_path: &Path, sandbox_bin_directory: &Path) {
    write_targets(
        repo_path,
        json!([
            { "url": "http://127.0.0.1:9736/", "taskId": "local-task" },
            { "url": "http://localhost:9737", "taskId": "dev-task", "transport": "local" },
        ]),
    );
    let status_path = repo_path.join(".kanvibe/status.json");
    std::fs::write(status_path, r#"{"schemaVersion":1,"status":"todo"}"#).unwrap();
    let project_path = repo_path.join(".kanvibe/project.json");
    std::fs::write(
        project_path,
        r##"{"schemaVersion":1,"projectColor":"#65D08A"}"##,
    )
    .unwrap();
    let tools = [
        "bash", "grep", "awk", "sed", "date", "mkdir", "touch", "dirname",
    ];
    create_tool_sandbox(sandbox_bin_directory, &tools);
    write_executable(
        &sandbox_bin_directory.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$KANVIBE_CURL_LOG\"",
    );
}

// parity: TS-lib-shellHookScript-118cfe
#[test]
fn hook_fans_out_status_to_every_client_without_node_or_user_env() {
    let workspace = tempfile::tempdir().unwrap();
    let repo_path = workspace.path();
    let sandbox_bin_directory = repo_path.join("sandbox-bin");
    prepare_fanout_repo(repo_path, &sandbox_bin_directory);
    let curl_log_path = repo_path.join("curl.log");

    run_sandboxed_hook(repo_path, &sandbox_bin_directory, &curl_log_path);

    let status = read_json(&repo_path.join(".kanvibe/status.json"));
    assert_eq!(status["status"], "review");
    assert!(status.get("projectColor").is_none());
    let project = read_json(&repo_path.join(".kanvibe/project.json"));
    assert_eq!(project["projectColor"], "#65D08A");
    let deliveries = status_deliveries(&std::fs::read_to_string(&curl_log_path).unwrap());
    let expected = [
        (
            "dev-task",
            "http://localhost:9737/api/hooks/status",
            "review",
        ),
        (
            "local-task",
            "http://127.0.0.1:9736/api/hooks/status",
            "review",
        ),
    ]
    .map(|(task_id, endpoint, status)| {
        (
            task_id.to_string(),
            endpoint.to_string(),
            status.to_string(),
        )
    });
    assert_eq!(deliveries, expected);
}

/// 가짜 curl 기록 `헤더 | 인자` 줄을 (taskId, 헤더, 인자)로 나눠 taskId 순으로 정렬한다
fn header_deliveries(curl_log: &str) -> Vec<(String, String, String)> {
    let mut deliveries: Vec<(String, String, String)> = curl_log
        .trim()
        .lines()
        .map(|line| {
            let (headers, argv) = line.split_once(" | ").unwrap();
            let task_id = captured_value(argv, "taskId").unwrap_or_default();
            (task_id, headers.to_string(), argv.to_string())
        })
        .collect();
    deliveries.sort();
    deliveries
}

// parity: TS-lib-shellHookScript-7c2e6e
#[test]
fn each_client_token_goes_through_stdin_and_never_the_command_line() {
    let workspace = tempfile::tempdir().unwrap();
    let repo_path = workspace.path();
    let sandbox_bin_directory = repo_path.join("sandbox-bin");
    write_targets(
        repo_path,
        json!([
            { "url": "http://127.0.0.1:9736", "taskId": "local-task", "token": "localtoken1" },
            { "url": "http://localhost:9737", "taskId": "dev-task", "token": "devtoken2", "clientId": "client-2", "transport": "local" },
        ]),
    );
    let tools = [
        "bash", "grep", "awk", "sed", "date", "mkdir", "touch", "dirname", "cat",
    ];
    create_tool_sandbox(&sandbox_bin_directory, &tools);
    write_executable(
        &sandbox_bin_directory.join("curl"),
        "#!/bin/sh\nheaders=\"$(cat)\"\nprintf '%s | %s\\n' \"$headers\" \"$*\" >> \"$KANVIBE_CURL_LOG\"",
    );
    let curl_log_path = repo_path.join("curl.log");

    run_sandboxed_hook(repo_path, &sandbox_bin_directory, &curl_log_path);

    let deliveries = header_deliveries(&std::fs::read_to_string(&curl_log_path).unwrap());
    let headers: Vec<(&str, &str)> = deliveries
        .iter()
        .map(|(task_id, headers, _)| (headers.as_str(), task_id.as_str()))
        .collect();
    assert_eq!(
        headers,
        [
            ("Authorization: Bearer devtoken2", "dev-task"),
            ("Authorization: Bearer localtoken1", "local-task"),
        ]
    );
    assert!(
        deliveries
            .iter()
            .all(|(_, _, argv)| !argv.contains("devtoken2") && !argv.contains("localtoken1"))
    );
}

// parity: TS-lib-shellHookScript-8848b5
#[test]
fn status_json_persistence_check_rejects_legacy_status_md_hooks() {
    let updater = review_updater();
    let legacy_updater = updater.replace("status.json", "status.md");

    assert!(has_shell_kanvibe_status_json_persistence(&updater));
    assert!(!has_shell_kanvibe_status_json_persistence(&legacy_updater));
}

// parity: TS-lib-shellHookScript-dd3258
#[test]
fn status_json_persistence_check_rejects_hooks_without_common_exclude_update() {
    let updater = review_updater();
    let pattern = regex::Regex::new(r"\nKANVIBE_GIT_COMMON_DIR=[\s\S]*?fi\n\nmkdir -p").unwrap();
    let updater_without_common_exclude = pattern.replace(&updater, "\nmkdir -p");

    assert_ne!(updater_without_common_exclude, updater);
    assert!(!has_shell_kanvibe_status_json_persistence(
        &updater_without_common_exclude
    ));
}

fn task_id_payload_script(task_id: &str) -> String {
    [
        format!("TASK_ID=\"{task_id}\""),
        "curl -d \"{\\\"taskId\\\": \\\"${TASK_ID}\\\"}\"".to_string(),
    ]
    .join("\n")
}

// parity: TS-lib-shellHookScript-955434
#[test]
fn binding_check_accepts_hooks_pinning_the_same_task_id() {
    let scripts = [
        task_id_payload_script("task-123"),
        task_id_payload_script("task-123"),
    ];
    let contents: Vec<&str> = scripts.iter().map(String::as_str).collect();

    let status = get_shell_task_id_binding_status(&contents);

    assert_eq!(
        status,
        ShellTaskIdBindingStatus {
            has_task_id_binding: true,
            bound_task_id: Some("task-123".into()),
        }
    );
}

// parity: TS-lib-shellHookScript-34d8c7
#[test]
fn binding_check_fails_for_different_or_missing_task_ids() {
    let scripts = [
        task_id_payload_script("task-123"),
        task_id_payload_script("task-999"),
    ];
    let contents: Vec<&str> = scripts.iter().map(String::as_str).collect();
    let unbound = ShellTaskIdBindingStatus {
        has_task_id_binding: false,
        bound_task_id: None,
    };

    assert_eq!(get_shell_task_id_binding_status(&contents), unbound);
    assert_eq!(get_shell_task_id_binding_status(&[]), unbound);
}

// parity: TS-lib-shellHookScript-203eec
#[test]
fn only_http_target_urls_reach_curl_and_options_or_files_are_skipped() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_path = workspace.path();
    let fake_bin_directory = workspace_path.join("bin");
    let curl_log_path = workspace_path.join("curl.log");
    std::fs::create_dir(&fake_bin_directory).unwrap();
    write_executable(
        &fake_bin_directory.join("curl"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
            curl_log_path.display()
        ),
    );
    git(&["init", "-q"], workspace_path);
    write_targets(
        workspace_path,
        json!([
            { "url": "-K/tmp/evil", "taskId": "task-option" },
            { "url": "file:///etc/hosts", "taskId": "task-file" },
            { "url": "http://localhost:9736", "taskId": "task-http", "transport": "local" },
        ]),
    );
    let mut environment = narrow_test_environment(workspace_path);
    let path = format!("{}:{}", fake_bin_directory.display(), environment["PATH"]);
    environment.insert("PATH".into(), path);

    let run = run_program(
        "bash",
        &["-c", &review_updater()],
        workspace_path,
        &environment,
        b"",
    );

    assert_eq!(run.code, 0, "{}", run.stderr);
    let curl_log = std::fs::read_to_string(&curl_log_path).unwrap();
    assert_eq!(curl_log.trim().lines().count(), 1);
    assert!(curl_log.contains("--url http://localhost:9736/api/hooks/status"));
    assert!(curl_log.contains("task-http"));
}

/// 가짜 curl이 받은 -d 본문을 파일로 남기도록 한 hook을 stdin 본문과 함께 실행하고, 보낸 JSON과 hook PID를 돌려준다
fn run_metadata_hook(provider: ShellHookAgentProvider, payload: &Value) -> (Value, u32) {
    let directory = tempfile::tempdir().unwrap();
    git(&["init", "-q"], directory.path());
    let capture = directory.path().join("payload.json");
    let script = [
        "TASK_ID=\"task-hook\"".to_string(),
        "KANVIBE_URL=\"http://127.0.0.1:1\"".to_string(),
        "curl() { while [ \"$#\" -gt 0 ]; do if [ \"$1\" = \"-d\" ]; then shift; printf \"%s\" \"$1\" > \"$KANVIBE_QA_CAPTURE\"; return; fi; shift; done; }".to_string(),
        build_shell_kanvibe_status_updater(ShellHookStatus::Progress, Some(provider)),
    ]
    .join("\n");
    let mut environment = narrow_test_environment(directory.path());
    environment.insert("KANVIBE_QA_CAPTURE".into(), capture.display().to_string());
    let mut child = std::process::Command::new("bash")
        .args(["-c", &script])
        .current_dir(directory.path())
        .env_clear()
        .envs(&environment)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let process_id = child.id();
    let mut input = child.stdin.take().unwrap();
    let body = payload.to_string();
    let writer =
        std::thread::spawn(move || drop(std::io::Write::write_all(&mut input, body.as_bytes())));
    assert!(child.wait().unwrap().success());
    writer.join().unwrap();
    let posted = serde_json::from_str(&std::fs::read_to_string(capture).unwrap()).unwrap();
    (posted, process_id)
}

// parity: TS-lib-shellHookChatMetadata-a18a07
#[test]
fn hooks_forward_stdin_and_sender_pid_and_drain_oversized_input() {
    let cases = [
        (ShellHookAgentProvider::Claude, false),
        (ShellHookAgentProvider::Codex, false),
        (ShellHookAgentProvider::Gemini, false),
        (ShellHookAgentProvider::Claude, true),
    ];
    for (provider, is_oversized) in cases {
        let prompt = if is_oversized {
            "큰 본문".repeat(100_000)
        } else {
            "줄1\n줄2".to_string()
        };
        let payload = json!({ "session_id": "session-hook", "cwd": "/work", "transcript_path": "/safe/한글.jsonl", "prompt": prompt });

        let (posted, process_id) = run_metadata_hook(provider, &payload);

        assert_eq!(posted["taskId"], "task-hook");
        assert_eq!(posted["status"], "progress");
        assert_eq!(posted["agentSession"]["provider"], provider.as_str());
        assert_eq!(posted["agentSession"]["hookPid"], process_id);
        let forwarded = base64::engine::general_purpose::STANDARD
            .decode(posted["agentSession"]["payloadBase64"].as_str().unwrap())
            .unwrap();
        if is_oversized {
            assert!(forwarded.len() <= 64 * 1_024);
        } else {
            assert_eq!(
                serde_json::from_slice::<Value>(&forwarded).unwrap(),
                payload
            );
        }
    }
}
