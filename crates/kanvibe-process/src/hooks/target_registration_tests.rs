//! TS `hookTargetRegistration.test.ts`를 옮긴 테스트.

use std::sync::Arc;

use serde_json::{Value, json};

use super::installer_test_support::{FakeHookClientRuntime, install_context};
use super::target_registration::{
    HookTargetRegistrationStatus, register_kanvibe_hook_target, verify_hook_target_registration,
};
use crate::exec::testing::LoopbackShellRunner;

fn targets_content() -> String {
    json!({
        "schemaVersion": 1,
        "targets": [
            { "url": "http://127.0.0.1:9736", "taskId": "task-local", "token": "token-local", "transport": "local" },
            { "url": "http://10.0.0.5:9736", "taskId": "task-remote", "token": "token-remote", "transport": "local" },
        ],
    })
    .to_string()
}

/// task CLI 토큰이 생기기 전에 설치된 targets.json
fn tokenless_targets_content() -> String {
    json!({
        "schemaVersion": 1,
        "targets": [
            { "url": "http://127.0.0.1:9736", "taskId": "task-local" },
            { "url": "http://10.0.0.5:9736", "taskId": "task-remote" },
        ],
    })
    .to_string()
}

fn status(is_registered: bool, url: Option<&str>) -> HookTargetRegistrationStatus {
    HookTargetRegistrationStatus {
        has_registered_hook_target: is_registered,
        registered_hook_target_url: url.map(str::to_string),
    }
}

fn read_targets(repo_path: &std::path::Path) -> Value {
    let content = std::fs::read_to_string(repo_path.join(".kanvibe/targets.json")).unwrap();
    serde_json::from_str(&content).unwrap()
}

#[test]
fn registering_a_client_records_the_task_cli_token() {
    let repo = tempfile::tempdir().unwrap();
    let repo_path = repo.path().to_str().unwrap();
    let runtime = Arc::new(FakeHookClientRuntime::new());
    let runner = Arc::new(LoopbackShellRunner::new(repo_path));
    let context = install_context(runner, runtime.clone(), repo_path);

    register_kanvibe_hook_target(
        &context,
        repo_path,
        "task-1",
        "http://127.0.0.1:9736",
        Some("remote-host"),
    )
    .unwrap();

    assert_eq!(*runtime.token_task_ids.lock().unwrap(), ["task-1"]);
    assert_eq!(*runtime.tunnel_hosts.lock().unwrap(), ["remote-host"]);
    let target = &read_targets(repo.path())["targets"][0];
    assert_eq!(
        *target,
        json!({ "url": "http://127.0.0.1:9736", "taskId": "task-1", "token": "signed-token", "clientId": "client-a", "transport": "ssh" })
    );
}

#[test]
fn remote_registration_fails_instead_of_writing_a_legacy_target_when_the_daemon_connection_errors()
{
    let repo = tempfile::tempdir().unwrap();
    let repo_path = repo.path().to_str().unwrap();
    let mut runtime = FakeHookClientRuntime::new();
    runtime.remote_host_namespace = Err("Host preparation cancelled".into());
    let runtime = Arc::new(runtime);
    let runner = Arc::new(LoopbackShellRunner::new(repo_path));
    let context = install_context(runner, runtime.clone(), repo_path);

    let result = register_kanvibe_hook_target(
        &context,
        repo_path,
        "task-1",
        "http://127.0.0.1:9736",
        Some("remote-host"),
    );

    assert_eq!(
        result.unwrap_err().to_string(),
        "Host preparation cancelled"
    );
    assert!(!repo.path().join(".kanvibe/targets.json").exists());
    assert!(runtime.tunnel_hosts.lock().unwrap().is_empty());
}

#[test]
fn remote_registration_without_transport_or_client_id_needs_reinstall() {
    let remote_url = Some("http://10.0.0.5:9736");
    let remote_host = Some("remote-host");
    assert!(
        !verify_hook_target_registration(
            &targets_content(),
            Some("task-remote"),
            remote_url,
            remote_host
        )
        .has_registered_hook_target
    );
    assert!(
        !verify_hook_target_registration(
            &targets_content(),
            Some("task-remote"),
            None,
            remote_host
        )
        .has_registered_hook_target
    );

    let with_client_id = json!({
        "schemaVersion": 1,
        "targets": [{ "url": "http://10.0.0.5:9736", "taskId": "task-remote", "token": "token-remote", "clientId": "client-a", "transport": "ssh" }],
    })
    .to_string();
    assert!(
        verify_hook_target_registration(
            &with_client_id,
            Some("task-remote"),
            remote_url,
            remote_host
        )
        .has_registered_hook_target
    );
    let legacy_transport = with_client_id.replace(",\"transport\":\"ssh\"", "");
    assert!(
        !verify_hook_target_registration(
            &legacy_transport,
            Some("task-remote"),
            remote_url,
            remote_host
        )
        .has_registered_hook_target
    );
    let local_transport = with_client_id.replace("\"ssh\"", "\"local\"");
    assert!(
        !verify_hook_target_registration(
            &local_transport,
            Some("task-remote"),
            remote_url,
            remote_host
        )
        .has_registered_hook_target
    );
}

#[test]
fn local_registration_records_the_transport_too() {
    let repo = tempfile::tempdir().unwrap();
    let repo_path = repo.path().to_str().unwrap();
    let runtime = Arc::new(FakeHookClientRuntime::new());
    let runner = Arc::new(LoopbackShellRunner::new(repo_path));
    let context = install_context(runner, runtime.clone(), repo_path);

    register_kanvibe_hook_target(&context, repo_path, "task-1", "http://localhost:9736", None)
        .unwrap();

    assert_eq!(
        read_targets(repo.path())["targets"][0]["transport"],
        "local"
    );
    assert!(runtime.tunnel_hosts.lock().unwrap().is_empty());
}

#[test]
fn tokenless_legacy_registration_counts_as_not_installed() {
    assert_eq!(
        verify_hook_target_registration(
            &tokenless_targets_content(),
            Some("task-local"),
            Some("http://127.0.0.1:9736"),
            None
        ),
        status(false, Some("http://127.0.0.1:9736"))
    );
    assert_eq!(
        verify_hook_target_registration(
            &tokenless_targets_content(),
            Some("task-remote"),
            None,
            None
        ),
        status(false, Some("http://10.0.0.5:9736"))
    );
}

#[test]
fn both_client_url_and_task_id_must_be_registered() {
    assert_eq!(
        verify_hook_target_registration(
            &targets_content(),
            Some("task-local"),
            Some("http://127.0.0.1:9736"),
            None
        ),
        status(true, Some("http://127.0.0.1:9736"))
    );
}

#[test]
fn another_client_registering_the_task_does_not_install_this_client() {
    assert_eq!(
        verify_hook_target_registration(
            &targets_content(),
            Some("task-remote"),
            Some("http://127.0.0.1:9736"),
            None
        ),
        status(false, Some("http://10.0.0.5:9736"))
    );
}

#[test]
fn unregistered_task_is_not_installed() {
    assert_eq!(
        verify_hook_target_registration(
            &targets_content(),
            Some("task-unknown"),
            Some("http://127.0.0.1:9736"),
            None
        ),
        status(false, None)
    );
}

#[test]
fn unknown_hook_server_url_falls_back_to_task_registration() {
    assert_eq!(
        verify_hook_target_registration(&targets_content(), Some("task-remote"), None, None),
        status(true, Some("http://10.0.0.5:9736"))
    );
    assert_eq!(
        verify_hook_target_registration(&targets_content(), Some("task-unknown"), None, None),
        status(false, None)
    );
}

#[test]
fn no_task_to_check_skips_the_registration_check() {
    assert_eq!(
        verify_hook_target_registration("", None, Some("http://127.0.0.1:9736"), None),
        status(true, None)
    );
}
