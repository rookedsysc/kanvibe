//! 모바일 경계([`MobileBridge`])를 실제 데스크톱 조립으로 지나 만든 터미널에 서버 런타임 값이 새지 않는지 본다.
//! 런타임 crate의 시험 준비물(격리 HOME·tmux·git 저장소)을 그대로 쓰고, tmux가 없으면 건너뛴다.

#[path = "../../kanvibe-runtime/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kanvibe_server::bridge::MobileBridge;
use kanvibe_server::desktop::{DesktopServerConfig, DesktopServices};
use serde_json::{Value, json};

use support::{
    REPOSITORY_NAME, RuntimeFixture, assert_runtime_values_absent, environment_keys, has_tmux,
    register_sample_project,
};

fn open_services(fixture: &RuntimeFixture) -> DesktopServices {
    let runtime = Arc::new(fixture.open_runtime());
    DesktopServices::new(
        runtime,
        DesktopServerConfig {
            home_directory: PathBuf::from(fixture.home()),
            mobile_tls_port: 0,
            push_relay: None,
            remote_grants: None,
            diagram_renderer: None,
        },
    )
}

fn assert_pane_environment_is_clean(fixture: &RuntimeFixture, pane_target: &str, source: &str) {
    let pane_pid = fixture.tmux(&["display-message", "-p", "-t", pane_target, "#{pane_pid}"]);
    let pane_pid = String::from_utf8_lossy(&pane_pid.stdout).trim().to_string();
    assert!(
        pane_pid.parse::<u32>().is_ok(),
        "{source} pane pid: {pane_pid:?}"
    );
    let bytes =
        std::fs::read(Path::new("/proc").join(&pane_pid).join("environ")).unwrap_or_default();
    assert_runtime_values_absent(&environment_keys(&String::from_utf8_lossy(&bytes)), source);
}

#[test]
fn terminals_created_through_mobile_routes_get_no_server_runtime_values() {
    if !has_tmux() {
        eprintln!("tmux가 없어 건너뛴다");
        return;
    }
    let fixture = RuntimeFixture::new();
    fixture.create_git_repository();
    let services = open_services(&fixture);
    let project = register_sample_project(&services.runtime, &fixture);

    let draft = json!({ "projectId": project.id, "branchName": "feat-mobile", "baseBranch": "main", "sessionType": "tmux" });
    let task = services
        .create_mobile_task(&draft)
        .expect("모바일 태스크 생성");
    let task_id = task["id"].as_str().unwrap().to_string();
    let session = format!("={REPOSITORY_NAME}-feat-mobile:");
    let surfaces = services
        .get_task_surfaces(&task_id, None)
        .expect("pane 조회가 세션을 붙인다");
    assert_eq!(surfaces["sessionType"], Value::String("tmux".into()));
    assert_pane_environment_is_clean(&fixture, &session, "모바일이 만든 태스크 pane");

    let created = services
        .create_mobile_terminal_tab(&task_id)
        .expect("모바일 탭 생성");
    let pane_id = created["paneId"].as_str().unwrap().to_string();
    assert_pane_environment_is_clean(&fixture, &pane_id, "모바일이 만든 탭 pane");
}
