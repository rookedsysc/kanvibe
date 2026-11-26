//! TS `hookInstallBundle.test.ts`를 옮긴 테스트.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::HookProviderFile;
use super::install_bundle::{
    HOOK_INSTALL_SCRIPT_PATH, HOOK_INSTALL_SUCCESS_MARKER, HOOK_INSTALL_TOKEN_HEADER,
    HookInstallScripts, HookInstallTicket, build_hook_install_bootstrap_command,
    build_hook_install_script,
};
use super::script_test_support::{RecordingServer, narrow_test_environment, run_program};

const CLIENT_ID: &str = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";
const HOOK_SERVER_URL: &str = "http://192.168.0.10:9736";

fn hook_files() -> Vec<HookProviderFile> {
    vec![
        HookProviderFile {
            file_path: "/home/user/project/.claude/hooks/kanvibe-stop-hook.sh".into(),
            content: "#!/bin/bash\necho '작업 완료'\n".into(),
            mode: Some(0o755),
        },
        HookProviderFile {
            file_path: "/home/user/project/.claude/settings.json".into(),
            content: "{\n  \"hooks\": {}\n}\n".into(),
            mode: None,
        },
    ]
}

fn sample_ticket() -> HookInstallTicket {
    HookInstallTicket {
        token: "token-value".into(),
        script_sha256: "a".repeat(64),
    }
}

fn bootstrap(hook_server_url: &str, ticket: &HookInstallTicket) -> String {
    build_hook_install_bootstrap_command(hook_server_url, ticket, CLIENT_ID, None).unwrap()
}

#[test]
fn writes_every_provider_file_and_marks_completion() {
    let script = build_hook_install_script(&hook_files());

    assert!(script.contains("set -e"));
    assert!(script.contains("mkdir -p '/home/user/project/.claude/hooks'"));
    assert!(script.contains("> '/home/user/project/.claude/hooks/kanvibe-stop-hook.sh'"));
    assert!(script.contains("chmod 755 '/home/user/project/.claude/hooks/kanvibe-stop-hook.sh'"));
    assert!(script.contains("> '/home/user/project/.claude/settings.json'"));
    assert!(
        script
            .trim_end()
            .ends_with(&format!("printf '%s\\n' '{HOOK_INSTALL_SUCCESS_MARKER}'"))
    );
}

#[test]
fn carries_file_content_as_base64_so_quotes_and_newlines_survive() {
    let files = hook_files();
    let script = build_hook_install_script(&files);

    let encoded_stop_hook = BASE64.encode(files[0].content.as_bytes());
    assert!(script.contains(&format!("printf '%s' '{encoded_stop_hook}'")));
    assert!(!script.contains("echo '작업 완료'"));
}

#[test]
fn does_not_set_a_mode_for_files_that_do_not_need_one() {
    let script = build_hook_install_script(&hook_files()[1..]);

    assert!(!script.contains("chmod"));
}

#[test]
fn produces_the_same_script_for_the_same_files() {
    assert_eq!(
        build_hook_install_script(&hook_files()),
        build_hook_install_script(&hook_files())
    );
}

#[test]
fn returns_the_registered_script_for_its_token() {
    let scripts = HookInstallScripts::default();
    let ticket = scripts.issue(&hook_files());

    assert_eq!(
        scripts.read(Some(&ticket.token)),
        Some(build_hook_install_script(&hook_files()))
    );
}

#[test]
fn stays_readable_so_a_retried_install_can_reuse_the_token() {
    let scripts = HookInstallScripts::default();
    let ticket = scripts.issue(&hook_files());

    scripts.read(Some(&ticket.token));

    assert!(scripts.read(Some(&ticket.token)).is_some());
}

#[test]
fn stops_serving_the_script_once_the_install_revokes_its_token() {
    let scripts = HookInstallScripts::default();
    let ticket = scripts.issue(&hook_files());

    scripts.revoke(&ticket.token);

    assert_eq!(scripts.read(Some(&ticket.token)), None);
}

#[test]
fn rejects_an_unknown_token() {
    let scripts = HookInstallScripts::default();
    scripts.issue(&hook_files());

    assert_eq!(scripts.read(Some(&"0".repeat(48))), None);
}

#[test]
fn rejects_a_missing_token() {
    let scripts = HookInstallScripts::default();
    scripts.issue(&hook_files());

    assert_eq!(scripts.read(None), None);
    assert_eq!(scripts.read(Some("")), None);
}

#[test]
fn issues_an_unguessable_token_per_install() {
    let scripts = HookInstallScripts::default();
    let first_ticket = scripts.issue(&hook_files());
    let second_ticket = scripts.issue(&hook_files());

    assert_ne!(first_ticket.token, second_ticket.token);
    assert_eq!(first_ticket.token.len(), 48);
    assert!(
        first_ticket
            .token
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    );
}

#[test]
fn issues_a_checksum_that_changes_with_the_install_payload() {
    let scripts = HookInstallScripts::default();
    let ticket = scripts.issue(&hook_files());
    let other_ticket = scripts.issue(&hook_files()[1..]);

    assert_eq!(ticket.script_sha256.len(), 64);
    assert!(
        ticket
            .script_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
    assert_ne!(ticket.script_sha256, other_ticket.script_sha256);
}

#[test]
fn issues_the_checksum_the_remote_computes_for_the_restored_script() {
    let scripts = HookInstallScripts::default();
    let ticket = scripts.issue(&hook_files());
    let script = scripts.read(Some(&ticket.token)).unwrap();
    let workspace = tempfile::tempdir().unwrap();

    // `$(...)`가 끝 개행을 떼고 원격이 `printf '%s\n'`으로 되살리는 과정을 그대로 재현한다
    let run = run_program(
        "sh",
        &[
            "-c",
            "__downloaded=$(printf '%s' \"$1\"); printf '%s\\n' \"$__downloaded\" | { sha256sum 2>/dev/null || shasum -a 256; } | cut -d' ' -f1",
            "sh",
            &script,
        ],
        workspace.path(),
        &narrow_test_environment(workspace.path()),
        b"",
    );

    assert_eq!(run.stdout.trim(), ticket.script_sha256);
}

#[test]
fn missing_install_socket_never_sends_the_token_to_a_tcp_address() {
    let workspace = tempfile::tempdir().unwrap();
    let scripts = HookInstallScripts::default();
    let ticket = scripts.issue(&[HookProviderFile {
        file_path: workspace.path().join("installed.sh").display().to_string(),
        content: "echo installed\n".into(),
        mode: None,
    }]);
    let server = RecordingServer::tcp();
    let command = bootstrap(&server.url, &ticket)
        .replace("/tmp/kv-$(id -u)", &workspace.path().display().to_string());

    let run = run_program(
        "sh",
        &["-c", &command],
        workspace.path(),
        &narrow_test_environment(workspace.path()),
        b"",
    );

    assert!(server.received().is_empty());
    assert_ne!(run.code, 0);
}

#[test]
fn keeps_the_ssh_command_short_instead_of_carrying_the_payload() {
    let scripts = HookInstallScripts::default();
    let command = bootstrap(HOOK_SERVER_URL, &scripts.issue(&hook_files()));

    assert!(command.len() < 1500);
    assert!(!command.contains("base64 -d"));
    assert!(command.contains(&format!("{HOOK_SERVER_URL}{HOOK_INSTALL_SCRIPT_PATH}'")));
}

#[test]
fn sends_the_install_token_as_a_header_not_in_the_url() {
    let scripts = HookInstallScripts::default();
    let ticket = scripts.issue(&hook_files());

    let command = bootstrap(HOOK_SERVER_URL, &ticket);

    assert!(!command.contains("?token="));
    assert!(command.contains(&format!(
        "-H '{HOOK_INSTALL_TOKEN_HEADER}: {}'",
        ticket.token
    )));
}

#[test]
fn keeps_the_ssh_command_the_same_size_for_any_payload() {
    let scripts = HookInstallScripts::default();
    let bulky_files: Vec<HookProviderFile> = (0..40)
        .map(|index| HookProviderFile {
            file_path: format!("/home/user/project/.claude/hooks/hook-{index}.sh"),
            content: format!("#!/bin/bash\n{}", "echo hook payload\n".repeat(200)),
            mode: Some(0o755),
        })
        .collect();

    let small_command = bootstrap(HOOK_SERVER_URL, &scripts.issue(&hook_files()));
    let bulky_ticket = scripts.issue(&bulky_files);
    let bulky_command = bootstrap(HOOK_SERVER_URL, &bulky_ticket);

    assert!(scripts.read(Some(&bulky_ticket.token)).unwrap().len() > 100_000);
    assert_eq!(bulky_command.len(), small_command.len());
}

#[test]
fn downloads_the_install_script_through_the_unix_socket() {
    let command = bootstrap(HOOK_SERVER_URL, &sample_ticket());

    assert!(command.contains(
        "curl --unix-socket \"$__kanvibe_socket\" --connect-timeout 3 --max-time 10 -fsSL"
    ));
}

#[test]
fn bounds_the_download_so_fallback_is_quick() {
    let command = bootstrap(HOOK_SERVER_URL, &sample_ticket());

    assert!(command.contains("--connect-timeout 3"));
    assert!(command.contains("--max-time 10"));
}

#[test]
fn downloads_fully_before_executing() {
    let command = bootstrap(HOOK_SERVER_URL, &sample_ticket());

    assert!(command.contains(" && __kanvibe_install_script=$("));
    assert!(command.contains("printf '%s\\n' \"$__kanvibe_install_script\" | sh"));
}

#[test]
fn executes_the_downloaded_script_only_after_its_checksum_matches() {
    let ticket = sample_ticket();
    let command = bootstrap(HOOK_SERVER_URL, &ticket);

    let checksum_guard = format!(
        "[ \"$__kanvibe_install_checksum\" = '{}' ]",
        ticket.script_sha256
    );
    let checksum_guard_index = command.find(&checksum_guard).expect("체크섬 비교");
    let execution_index = command
        .find("printf '%s\\n' \"$__kanvibe_install_script\" | sh")
        .expect("실행 단계");
    assert!(execution_index > checksum_guard_index);
    assert!(command.contains("sha256sum 2>/dev/null || shasum -a 256"));
}

#[test]
fn does_not_double_the_slash_for_a_trailing_slash_url() {
    let command = bootstrap("http://192.168.0.10:9736/", &sample_ticket());

    assert!(command.contains("http://192.168.0.10:9736/api/install/hooks.sh'"));
    assert!(!command.contains("9736//api"));
}
