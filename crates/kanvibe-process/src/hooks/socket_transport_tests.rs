//! hook 스크립트와 task CLI가 원격 터널 소켓·loopback HTTP 가운데 어디로 토큰을 보내는지 실제 bash로 확인한다(TS `hookSocketTransport.test.ts`).
//! 실제 `/tmp/kv-<uid>`에는 실행 중인 앱의 소켓이 있을 수 있어, 생성한 셸의 소켓 디렉터리를 테스트 디렉터리로 바꿔 실행한다.

use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::script_test_support::{RecordingServer, ShellRun, run_bash};
use super::shell_script::{ShellHookStatus, build_shell_kanvibe_status_updater};
use super::socket_path::{
    build_hook_socket_file_name, build_remote_hook_socket_prepare_command,
    build_shell_hook_socket_resolver, build_shell_hook_transport_resolver,
    build_shell_owned_hook_socket_assignment,
};
use crate::task_cli::script::build_task_cli_command;

const CLIENT_ID: &str = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";
const SOCKET_DIR_SHELL: &str = "/tmp/kv-$(id -u)";

/// Unix 소켓 경로는 약 104자로 제한되어 실제 소켓처럼 /tmp 아래 짧은 경로를 쓴다
struct SocketWorkspace {
    workspace: tempfile::TempDir,
    socket_directory: PathBuf,
}

impl SocketWorkspace {
    fn new() -> Self {
        let workspace = tempfile::Builder::new()
            .prefix("kvs-")
            .tempdir_in("/tmp")
            .unwrap();
        let socket_directory = workspace.path().join("sock");
        std::fs::create_dir(&socket_directory).unwrap();
        set_mode(&socket_directory, 0o700);
        Self {
            workspace,
            socket_directory,
        }
    }

    fn path(&self) -> &Path {
        self.workspace.path()
    }

    fn socket_path(&self) -> PathBuf {
        self.socket_directory
            .join(build_hook_socket_file_name(CLIENT_ID).unwrap())
    }

    fn with_test_socket_directory(&self, script: &str) -> String {
        script.replace(SOCKET_DIR_SHELL, self.socket_directory.to_str().unwrap())
    }

    fn run(&self, script: &str) -> ShellRun {
        run_bash(script, self.path())
    }

    fn resolve_socket(&self, arguments: &str) -> ShellRun {
        let resolver = self.with_test_socket_directory(&build_shell_hook_socket_resolver());
        self.run(&format!("{resolver}\nkanvibe_hook_socket {arguments}"))
    }

    fn write_targets(&self, targets: Value) {
        std::fs::create_dir_all(self.path().join(".kanvibe")).unwrap();
        let content = json!({ "targets": targets }).to_string();
        std::fs::write(self.path().join(".kanvibe/targets.json"), content).unwrap();
    }

    /// 소켓을 쓸 수 없는 상태를 만든다. missing은 아무것도 두지 않는다
    fn prepare_unavailable_socket(&self, kind: &str) -> Option<RecordingServer> {
        let socket_path = self.socket_path();
        match kind {
            "regular" => std::fs::write(&socket_path, "unexpected file").unwrap(),
            "symlink" => {
                let target_path = self.socket_directory.join("other.sock");
                let server = RecordingServer::unix(&target_path);
                symlink(&target_path, &socket_path).unwrap();
                return Some(server);
            }
            "permissions" => {
                let server = RecordingServer::unix(&socket_path);
                set_mode(&self.socket_directory, 0o755);
                return Some(server);
            }
            "stale" => drop(UnixListener::bind(&socket_path).unwrap()),
            _ => {}
        }
        None
    }
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

const UNAVAILABLE_SOCKET_KINDS: [&str; 5] =
    ["missing", "stale", "regular", "symlink", "permissions"];

// parity: TS-lib-hookSocketTransport-3a13d1
#[test]
fn shell_and_desktop_agree_on_the_socket_name() {
    let fixture = SocketWorkspace::new();
    let _server = RecordingServer::unix(&fixture.socket_path());

    let result = fixture.resolve_socket(&format!("'{CLIENT_ID}'"));

    assert_eq!(result.stdout, fixture.socket_path().to_str().unwrap());
}

/// namespace 소켓 하나로 hook 상태 통보와 task CLI 삭제가 client ID와 함께 도착하는지 본다
fn assert_namespaced_endpoint_is_reached(namespace: &str) {
    let fixture = SocketWorkspace::new();
    let directory = fixture.socket_directory.join(namespace);
    std::fs::create_dir(&directory).unwrap();
    set_mode(&directory, 0o700);
    let server = RecordingServer::unix(&directory.join("hooks-v1.sock"));
    fixture.write_targets(json!([{ "url": "http://stale.invalid:9736", "taskId": "task-1", "token": "token-1", "clientId": CLIENT_ID, "transport": "ssh", "remoteHostNamespace": namespace }]));
    let assignment = fixture.with_test_socket_directory(
        &build_shell_owned_hook_socket_assignment("found", CLIENT_ID, Some(namespace)).unwrap(),
    );
    let found = fixture.run(&format!("{assignment}; printf '%s' \"$found\""));
    assert_eq!(
        found.stdout,
        directory.join("hooks-v1.sock").to_str().unwrap()
    );
    let updater = fixture.with_test_socket_directory(&build_shell_kanvibe_status_updater(
        ShellHookStatus::Review,
        None,
    ));
    let hook = format!(
        "TASK_ID=task-1\nKANVIBE_URL=http://stale.invalid:9736\nset --\ngit init -q\n{updater}"
    );
    assert_eq!(fixture.run(&hook).code, 0);
    let cli = build_task_cli_command(fixture.path().to_str().unwrap(), &["delete", "--yes"], None);
    assert_eq!(
        fixture.run(&fixture.with_test_socket_directory(&cli)).code,
        0
    );

    let received: Vec<(String, Option<String>)> = server
        .received()
        .into_iter()
        .map(|request| (request.path, request.client_id))
        .collect();
    assert_eq!(
        received,
        [
            ("/api/hooks/status".to_string(), Some(CLIENT_ID.to_string())),
            (
                "/api/cli/tasks/delete".to_string(),
                Some(CLIENT_ID.to_string())
            ),
        ]
    );
}

// parity: TS-lib-hookSocketTransport-c27a2a
#[test]
fn namespaced_hook_and_cli_reach_the_stable_endpoint_with_client_identity() {
    for namespace in ["dev", "prod", "qa-fixture"] {
        assert_namespaced_endpoint_is_reached(namespace);
    }
}

// parity: TS-lib-hookSocketTransport-600c98
#[test]
fn invalid_namespaces_are_rejected_even_if_a_legacy_socket_is_live() {
    for namespace in ["qa-", "../prod", "qa-TOO", "qa-1234567890123456789012345"] {
        let fixture = SocketWorkspace::new();
        let _server = RecordingServer::unix(&fixture.socket_path());

        let result = fixture.resolve_socket(&format!("'{CLIENT_ID}' '{namespace}'"));

        assert_eq!(result.stdout, "", "{namespace}");
        assert!(
            build_shell_owned_hook_socket_assignment("found", CLIENT_ID, Some(namespace)).is_err()
        );
    }
}

// parity: TS-lib-hookSocketTransport-69d251
#[test]
fn local_http_boundary_accepts_only_loopback_urls_with_local_transport() {
    let cases = [
        ("http://localhost:9736", "local", 0),
        ("http://127.0.0.1:9736", "", 0),
        ("http://[::1]:9736", "local", 0),
        ("http://localhost.evil:9736", "local", 1),
        ("http://user@127.0.0.1:9736", "local", 1),
        ("http://localhost:9736@evil", "local", 1),
        ("http://localhost:9736bad", "local", 1),
        ("http://127.0.0.1:9736", "ssh", 1),
        ("http://127.0.0.1:9736", "unknown", 1),
    ];
    let fixture = SocketWorkspace::new();
    for (url, transport, expected_code) in cases {
        let result = fixture.run(&format!(
            "{}\nkanvibe_hook_uses_local_http '{url}' '{transport}'",
            build_shell_hook_transport_resolver()
        ));

        assert_eq!(result.code, expected_code, "{url} {transport}");
    }
}

// parity: TS-lib-hookSocketTransport-82d9b9
#[test]
fn client_ids_with_path_or_shell_characters_never_resolve_a_socket() {
    let fixture = SocketWorkspace::new();
    let _server = RecordingServer::unix(&fixture.socket_path());
    for client_id in ["../../etc/x", "a;rm", "", "_"] {
        let result = fixture.resolve_socket(&format!("'{client_id}'"));

        assert_eq!(result.stdout, "", "{client_id}");
        assert!(build_hook_socket_file_name(client_id).is_err());
    }
}

// parity: TS-lib-hookSocketTransport-0fe91c
#[test]
fn sockets_in_shared_directories_or_regular_files_get_no_token() {
    let shared_directory = SocketWorkspace::new();
    let _server = RecordingServer::unix(&shared_directory.socket_path());
    set_mode(&shared_directory.socket_directory, 0o755);
    let regular_file = SocketWorkspace::new();
    std::fs::write(regular_file.socket_path(), "").unwrap();

    for fixture in [shared_directory, regular_file] {
        let result = fixture.resolve_socket(&format!("'{CLIENT_ID}'"));

        assert_eq!(result.stdout, "");
    }
}

// parity: TS-lib-hookSocketTransport-3d3c3a
#[test]
fn symlinked_socket_directory_is_not_used() {
    let fixture = SocketWorkspace::new();
    let linked_directory = fixture.path().join("linked");
    symlink(&fixture.socket_directory, &linked_directory).unwrap();
    let _server = RecordingServer::unix(&fixture.socket_path());
    let resolver = build_shell_hook_socket_resolver()
        .replace(SOCKET_DIR_SHELL, linked_directory.to_str().unwrap());

    let result = fixture.run(&format!("{resolver}\nkanvibe_hook_socket '{CLIENT_ID}'"));

    assert_eq!(result.stdout, "");
}

// parity: TS-lib-hookSocketTransport-73f8d4
#[test]
fn prepare_command_removes_the_old_socket_and_makes_a_0700_directory() {
    let fixture = SocketWorkspace::new();
    let prepared_directory = fixture.path().join("prepared");
    std::fs::create_dir(&prepared_directory).unwrap();
    set_mode(&prepared_directory, 0o755);
    let socket_file_name = build_hook_socket_file_name(CLIENT_ID).unwrap();
    let _server = RecordingServer::unix(&prepared_directory.join(&socket_file_name));
    let prepared = prepared_directory.to_str().unwrap();
    let script = build_remote_hook_socket_prepare_command(CLIENT_ID)
        .unwrap()
        .replace(SOCKET_DIR_SHELL, prepared);

    let result = fixture.run(&format!(
        "{script}; ls -ld '{prepared}' | cut -c1-10; ls -A '{prepared}' | wc -l"
    ));

    assert!(
        result
            .stdout
            .starts_with(prepared_directory.join(&socket_file_name).to_str().unwrap())
    );
    assert!(result.stdout.contains("drwx------"));
    assert!(result.stdout.trim().ends_with('0'));
}

// parity: TS-lib-hookSocketTransport-565be9
#[test]
fn prepare_command_does_not_delete_unexpected_files() {
    for kind in ["regular", "symlink"] {
        let fixture = SocketWorkspace::new();
        let file_path = fixture.socket_path();
        let protected_path = fixture.path().join("protected");
        std::fs::write(&protected_path, "keep").unwrap();
        if kind == "regular" {
            std::fs::write(&file_path, "keep").unwrap();
        } else {
            symlink(&protected_path, &file_path).unwrap();
        }
        let command = build_remote_hook_socket_prepare_command(CLIENT_ID).unwrap();

        let result = fixture.run(&fixture.with_test_socket_directory(&command));

        assert_ne!(result.code, 0, "{kind}");
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "keep");
    }
}

// parity: TS-lib-hookSocketTransport-db977f
#[test]
fn one_line_check_ignores_sockets_in_non_0700_directories() {
    let fixture = SocketWorkspace::new();
    let assignment = fixture.with_test_socket_directory(
        &build_shell_owned_hook_socket_assignment("found", CLIENT_ID, None).unwrap(),
    );
    let _server = RecordingServer::unix(&fixture.socket_path());
    set_mode(&fixture.socket_directory, 0o755);

    let result = fixture.run(&format!("{assignment}; printf '%s' \"$found\""));

    assert_eq!(result.stdout, "");
}

// parity: TS-lib-hookSocketTransport-a6171c
#[test]
fn one_line_check_only_keeps_owned_sockets() {
    let fixture = SocketWorkspace::new();
    let assignment = fixture.with_test_socket_directory(
        &build_shell_owned_hook_socket_assignment("found", CLIENT_ID, None).unwrap(),
    );
    let print_found = format!("{assignment}; printf '%s' \"$found\"");

    let missing = fixture.run(&print_found);
    let _server = RecordingServer::unix(&fixture.socket_path());
    let present = fixture.run(&print_found);

    assert_eq!(missing.stdout, "");
    assert_eq!(present.stdout, fixture.socket_path().to_str().unwrap());
}

/// task CLI delete 명령을 만든다. client_id·transport가 None이면 그 필드를 targets.json에서 뺀다
fn build_cli_command(
    fixture: &SocketWorkspace,
    target_url: &str,
    client_id: Option<&str>,
    transport: Option<&str>,
) -> String {
    let mut target = json!({ "url": target_url, "taskId": "task-1", "token": "token-1" });
    if let Some(client_id) = client_id {
        target["clientId"] = json!(client_id);
    }
    if let Some(transport) = transport {
        target["transport"] = json!(transport);
    }
    fixture.write_targets(json!([target]));
    let command =
        build_task_cli_command(fixture.path().to_str().unwrap(), &["delete", "--yes"], None);
    fixture.with_test_socket_directory(&command)
}

// parity: TS-lib-hookSocketTransport-7e1c51
#[test]
fn cli_uses_the_socket_so_the_token_never_hits_the_plain_port() {
    let fixture = SocketWorkspace::new();
    let via_http = RecordingServer::tcp();
    let via_socket = RecordingServer::unix(&fixture.socket_path());

    let result = fixture.run(&build_cli_command(
        &fixture,
        &via_http.url,
        Some(CLIENT_ID),
        Some("ssh"),
    ));

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(via_socket.received().len(), 1);
    assert!(via_http.received().is_empty());
}

// parity: TS-lib-hookSocketTransport-5e85c6
#[test]
fn cli_fails_without_http_fallback_when_the_remote_socket_is_unusable() {
    for kind in UNAVAILABLE_SOCKET_KINDS {
        let fixture = SocketWorkspace::new();
        let via_http = RecordingServer::tcp();
        let _socket_server = fixture.prepare_unavailable_socket(kind);

        let result = fixture.run(&build_cli_command(
            &fixture,
            &via_http.url,
            Some(CLIENT_ID),
            Some("ssh"),
        ));

        assert_ne!(result.code, 0, "{kind}");
        assert!(via_http.received().is_empty(), "{kind}");
    }
}

// parity: TS-lib-hookSocketTransport-c3d94c
#[test]
fn stale_target_address_is_ignored_in_favor_of_the_client_socket() {
    let fixture = SocketWorkspace::new();
    let via_stale_address = RecordingServer::tcp();
    let via_socket = RecordingServer::unix(&fixture.socket_path());

    let command = build_cli_command(
        &fixture,
        &via_stale_address.url,
        Some(CLIENT_ID),
        Some("ssh"),
    );
    let result = fixture.run(&command);

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(via_socket.received().len(), 1);
    assert!(via_stale_address.received().is_empty());
}

// parity: TS-lib-hookSocketTransport-9c372a
#[test]
fn remote_registration_without_client_id_sends_nowhere() {
    let fixture = SocketWorkspace::new();
    let via_http = RecordingServer::tcp();
    let via_socket = RecordingServer::unix(&fixture.socket_path());

    let result = fixture.run(&build_cli_command(
        &fixture,
        &via_http.url,
        None,
        Some("ssh"),
    ));

    assert_ne!(result.code, 0);
    assert!(via_socket.received().is_empty());
    assert!(via_http.received().is_empty());
}

// parity: TS-lib-hookSocketTransport-e1beaf
#[test]
fn local_or_legacy_loopback_registrations_keep_http() {
    for transport in [Some("local"), None] {
        let fixture = SocketWorkspace::new();
        let server = RecordingServer::tcp();

        let result = fixture.run(&build_cli_command(&fixture, &server.url, None, transport));

        assert_eq!(result.code, 0, "{transport:?}: {}", result.stderr);
        assert_eq!(server.received().len(), 1);
    }
}

// parity: TS-lib-hookSocketTransport-3445bb
#[test]
fn explicitly_invalid_transport_never_uses_tcp_even_on_loopback() {
    for transport in [json!(""), json!("unknown"), Value::Null, json!(false)] {
        let fixture = SocketWorkspace::new();
        let server = RecordingServer::tcp();
        let command = build_cli_command(&fixture, &server.url, None, None);
        fixture.write_targets(json!([{ "url": server.url, "taskId": "task-1", "token": "token-1", "transport": transport }]));

        let result = fixture.run(&command);

        assert_ne!(result.code, 0, "{transport}");
        assert!(server.received().is_empty(), "{transport}");
    }
}

// parity: TS-lib-hookSocketTransport-bcdb6a
#[test]
fn reordered_json_fields_keep_the_ssh_transport_for_the_cli() {
    let fixture = SocketWorkspace::new();
    let server = RecordingServer::tcp();
    let via_socket = RecordingServer::unix(&fixture.socket_path());
    let command = build_cli_command(&fixture, &server.url, None, None);
    fixture.write_targets(json!([{ "transport": "ssh", "clientId": CLIENT_ID, "taskId": "task-1", "token": "token-1", "url": server.url }]));

    let result = fixture.run(&command);

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(via_socket.received().len(), 1);
    assert!(server.received().is_empty());
}

// parity: TS-lib-hookSocketTransport-1a2d96
#[test]
fn userinfo_urls_never_use_tcp_without_a_socket() {
    for transport in [Some("local"), Some("unknown"), None] {
        let fixture = SocketWorkspace::new();
        let server = RecordingServer::tcp();
        let url = server.url.replace("http://", "http://user@");

        let result = fixture.run(&build_cli_command(&fixture, &url, None, transport));

        assert_ne!(result.code, 0, "{transport:?}");
        assert!(server.received().is_empty(), "{transport:?}");
    }
}

fn context_task_ids(server: &RecordingServer) -> Vec<Value> {
    server
        .received()
        .into_iter()
        .map(|request| {
            serde_json::from_str::<Value>(&request.body).unwrap()["contextTaskId"].clone()
        })
        .collect()
}

// parity: TS-lib-hookSocketTransport-fa20a2
#[test]
fn peer_propagation_keeps_each_client_transport() {
    let fixture = SocketWorkspace::new();
    let remote_tcp = RecordingServer::tcp();
    let local_tcp = RecordingServer::tcp();
    let socket = RecordingServer::unix(&fixture.socket_path());
    fixture.write_targets(json!([
        { "url": remote_tcp.url, "taskId": "remote-task", "token": "remote-token", "clientId": CLIENT_ID, "transport": "ssh" },
        { "url": local_tcp.url, "taskId": "local-task", "token": "local-token", "transport": "local" },
    ]));
    let command = build_task_cli_command(
        fixture.path().to_str().unwrap(),
        &["propagate-delete", "--exclude-url", "http://localhost:1"],
        None,
    );

    let result = fixture.run(&fixture.with_test_socket_directory(&command));

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(remote_tcp.received().is_empty());
    assert_eq!(context_task_ids(&socket), [json!("remote-task")]);
    assert_eq!(context_task_ids(&local_tcp), [json!("local-task")]);
}

/// targets.json을 쓰고 review 상태를 통보하는 hook 스크립트를 만든다
fn build_hook_script(fixture: &SocketWorkspace, target_url: &str, transport: &str) -> String {
    fixture.write_targets(json!([{ "url": target_url, "taskId": "task-1", "clientId": CLIENT_ID, "transport": transport }]));
    let updater = fixture.with_test_socket_directory(&build_shell_kanvibe_status_updater(
        ShellHookStatus::Review,
        None,
    ));
    format!(
        "TASK_ID=task-1\nKANVIBE_URL={target_url}\nset --\ncd '{}'\ngit init -q 2>/dev/null\n{updater}",
        fixture.path().display()
    )
}

// parity: TS-lib-hookSocketTransport-e1f143
#[test]
fn hook_status_goes_through_the_socket_instead_of_http() {
    let fixture = SocketWorkspace::new();
    let via_http = RecordingServer::tcp();
    let via_socket = RecordingServer::unix(&fixture.socket_path());

    fixture.run(&build_hook_script(&fixture, &via_http.url, "ssh"));

    let received = via_socket.received();
    let paths: Vec<&str> = received
        .iter()
        .map(|request| request.path.as_str())
        .collect();
    assert_eq!(paths, ["/api/hooks/status"]);
    assert_eq!(
        serde_json::from_str::<Value>(&received[0].body).unwrap(),
        json!({ "taskId": "task-1", "status": "review" })
    );
    assert!(via_http.received().is_empty());
}

// parity: TS-lib-hookSocketTransport-de11b3
#[test]
fn hook_status_never_falls_back_to_http_for_unusable_sockets() {
    for kind in UNAVAILABLE_SOCKET_KINDS {
        let fixture = SocketWorkspace::new();
        let via_http = RecordingServer::tcp();
        let _socket_server = fixture.prepare_unavailable_socket(kind);

        fixture.run(&build_hook_script(&fixture, &via_http.url, "ssh"));

        assert!(via_http.received().is_empty(), "{kind}");
    }
}

// parity: TS-lib-hookSocketTransport-621597
#[test]
fn hook_status_with_invalid_transport_never_uses_tcp() {
    for transport in ["", "unknown"] {
        let fixture = SocketWorkspace::new();
        let server = RecordingServer::tcp();

        fixture.run(&build_hook_script(&fixture, &server.url, transport));

        assert!(server.received().is_empty(), "{transport}");
    }
}

// parity: TS-lib-hookSocketTransport-516b71
#[test]
fn reordered_json_fields_keep_the_ssh_transport_for_hooks() {
    let fixture = SocketWorkspace::new();
    let server = RecordingServer::tcp();
    let via_socket = RecordingServer::unix(&fixture.socket_path());
    let script = build_hook_script(&fixture, &server.url, "ssh");
    fixture.write_targets(json!([{ "transport": "ssh", "clientId": CLIENT_ID, "taskId": "task-1", "url": server.url }]));

    fixture.run(&script);

    assert_eq!(via_socket.received().len(), 1);
    assert!(server.received().is_empty());
}

// parity: TS-lib-hookSocketTransport-4ba625
#[test]
fn failed_notification_still_leaves_the_latest_status_json() {
    let fixture = SocketWorkspace::new();
    let offline_desktop = RecordingServer::unix_with(&fixture.socket_path(), |_| {
        (503, "Desktop offline; request not replayed".to_string())
    });

    let result = fixture.run(&build_hook_script(&fixture, "http://127.0.0.1:1", "ssh"));

    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(offline_desktop.received().len(), 1);
    let status: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.path().join(".kanvibe/status.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(status["schemaVersion"], 1);
    assert_eq!(status["status"], "review");
}

// parity: TS-lib-hookSocketTransport-21ab99
#[test]
fn local_loopback_registration_sends_status_over_http() {
    let fixture = SocketWorkspace::new();
    let server = RecordingServer::tcp();

    fixture.run(&build_hook_script(&fixture, &server.url, "local"));

    let paths: Vec<String> = server
        .received()
        .into_iter()
        .map(|request| request.path)
        .collect();
    assert_eq!(paths, ["/api/hooks/status"]);
}
