//! 생성한 OpenCode 플러그인을 실제 Node로 실행해 원격 터널 소켓·loopback HTTP 가운데 어디로 상태를 보내는지 확인한다
//! (TS `hookSocketTransport.test.ts`의 "OpenCode 생성 플러그인" 묶음).
//! 플러그인은 OpenCode(Bun) 안에서 도는 TypeScript라 JS 런타임 없이는 동작을 검증할 수 없다. 이 머신의 `node`(타입 제거 지원)로
//! 실행하고, Node `fetch`에는 Bun의 `unix` 옵션이 없어 TS 테스트처럼 HTTP 어댑터 경계만 실제 socketPath 요청으로 잇는다.

use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::open_code::generate_plugin_script;
use super::open_code_registry::file_url_from_path;
use super::script_test_support::{RecordingServer, narrow_test_environment, run_program};
use super::socket_path::build_hook_socket_file_name;

const CLIENT_ID: &str = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";

/// TS 테스트의 `fetchOverHttp`와 `plugin.event(session.idle)` 호출을 그대로 옮긴 Node 실행기
const NODE_DRIVER: &str = r#"import http from "node:http";
const [pluginUrl] = process.argv.slice(2);
globalThis.fetch = (url, init) => new Promise((resolve, reject) => {
  const request = http.request(url, { method: init.method, headers: init.headers, socketPath: init.unix }, (response) => {
    response.resume();
    response.once("end", () => resolve({ ok: response.statusCode === 200, status: response.statusCode }));
  });
  request.once("error", reject);
  request.end(init.body);
});
const { KanvibePlugin } = await import(pluginUrl);
const plugin = await KanvibePlugin({});
await plugin.event({ event: { type: "session.idle", properties: { sessionID: "session-1", parentID: "" } } });
"#;

/// Unix 소켓 경로 길이 제한 때문에 /tmp 아래 짧은 작업 디렉터리를 쓴다
struct PluginWorkspace {
    workspace: tempfile::TempDir,
    socket_directory: PathBuf,
}

impl PluginWorkspace {
    fn new() -> Self {
        let workspace = tempfile::Builder::new()
            .prefix("kvo-")
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

    fn write_targets(&self, target_url: &str, transport: Option<&str>, token: Option<&str>) {
        let mut target = Map::new();
        target.insert("url".into(), json!(target_url));
        target.insert("taskId".into(), json!("task-1"));
        target.insert("clientId".into(), json!(CLIENT_ID));
        transport
            .into_iter()
            .for_each(|value| drop(target.insert("transport".into(), json!(value))));
        token
            .into_iter()
            .for_each(|value| drop(target.insert("token".into(), json!(value))));
        std::fs::create_dir_all(self.path().join(".kanvibe")).unwrap();
        let content = json!({ "targets": [Value::Object(target)] }).to_string();
        std::fs::write(self.path().join(".kanvibe/targets.json"), content).unwrap();
    }

    /// 생성한 플러그인에 session.idle 이벤트를 한 번 보낸다
    fn notify(&self, target_url: &str, transport: Option<&str>, token: Option<&str>) {
        self.write_targets(target_url, transport, token);
        let plugin_directory = self.path().join(".opencode/plugins");
        std::fs::create_dir_all(&plugin_directory).unwrap();
        let plugin_path = plugin_directory.join("kanvibe-plugin.ts");
        let source = generate_plugin_script(target_url, "task-1")
            .replace(
                "import.meta.url",
                &json!(file_url_from_path(plugin_path.to_str().unwrap())).to_string(),
            )
            .replacen(
                "\"/tmp/kv-\" + uid",
                &json!(self.socket_directory.to_str().unwrap()).to_string(),
                1,
            );
        let module_path = plugin_directory.join("kanvibe-plugin.mts");
        std::fs::write(&module_path, source).unwrap();
        let driver_path = self.path().join("driver.mjs");
        std::fs::write(&driver_path, NODE_DRIVER).unwrap();
        let module_url = file_url_from_path(module_path.to_str().unwrap());
        let run = run_program(
            "node",
            &[driver_path.to_str().unwrap(), &module_url],
            self.path(),
            &narrow_test_environment(self.path()),
            b"",
        );
        assert_eq!(run.code, 0, "node 플러그인 실행 실패: {}", run.stderr);
    }
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn received_paths(server: &RecordingServer) -> Vec<String> {
    server
        .received()
        .into_iter()
        .map(|request| request.path)
        .collect()
}

// parity: TS-lib-hookSocketTransport-db3950
#[test]
fn ssh_registrations_send_status_only_through_the_real_unix_socket() {
    let fixture = PluginWorkspace::new();
    let via_http = RecordingServer::tcp();
    let via_socket = RecordingServer::unix(&fixture.socket_path());

    fixture.notify(&via_http.url, Some("ssh"), None);

    assert_eq!(received_paths(&via_socket), ["/api/hooks/status"]);
    assert!(via_http.received().is_empty());
}

// parity: TS-lib-hookSocketTransport-144568
#[test]
fn unusable_ssh_sockets_never_fall_back_to_a_responding_tcp_server() {
    for kind in ["missing", "stale", "regular", "symlink", "permissions"] {
        let fixture = PluginWorkspace::new();
        let server = RecordingServer::tcp();
        let _socket_server = fixture.prepare_unavailable_socket(kind);

        fixture.notify(&server.url, Some("ssh"), None);

        assert!(server.received().is_empty(), "{kind}");
    }
}

// parity: TS-lib-hookSocketTransport-21db97
#[test]
fn sends_the_task_token_from_targets_json_as_the_authorization_header() {
    let fixture = PluginWorkspace::new();
    let server = RecordingServer::tcp();

    fixture.notify(&server.url, Some("local"), Some("tasktoken1"));

    let requests: Vec<(String, Option<String>)> = server
        .received()
        .into_iter()
        .map(|request| (request.path, request.authorization))
        .collect();
    assert_eq!(
        requests,
        [(
            "/api/hooks/status".to_string(),
            Some("Bearer tasktoken1".to_string())
        )]
    );
}

// parity: TS-lib-hookSocketTransport-bc5a3a
#[test]
fn local_and_legacy_loopback_registrations_use_http() {
    for transport in [Some("local"), None] {
        let fixture = PluginWorkspace::new();
        let server = RecordingServer::tcp();

        fixture.notify(&server.url, transport, None);

        assert_eq!(
            received_paths(&server),
            ["/api/hooks/status"],
            "{transport:?}"
        );
    }
}
