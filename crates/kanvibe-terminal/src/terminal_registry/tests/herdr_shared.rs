//! TS `terminalSessionLifecycle.test.ts` "공유 Herdr Space 클라이언트"

use kanvibe_domain::task::SessionType;
use serde_json::json;

use super::fakes::*;
use crate::terminal_registry::{HerdrSurfaceBounds, HerdrTabBarPosition};

fn attach_shared_space(harness: &Harness, task_id: &str, working_directory: &str) {
    let session_name = format!("kanvibe:{task_id}");
    let request = local_request(
        task_id,
        None,
        SessionType::Herdr,
        &session_name,
        Some(working_directory),
    );
    harness.attach_local(request, &FakeClient::new()).unwrap();
}

/// task-a의 공유 Space에 로컬 또는 원격(daemon 없이 ssh 포워딩)으로 붙는다
fn attach_task_a_shared_space(harness: &Harness, is_remote: bool) {
    let client = FakeClient::new();
    let session_name = "kanvibe:task-a";
    if !is_remote {
        let request = local_request(
            "task-a",
            None,
            SessionType::Herdr,
            session_name,
            Some("/work"),
        );
        harness.attach_local(request, &client).unwrap();
        return;
    }
    let mut request = remote_request(
        "task-a",
        None,
        "host",
        SessionType::Herdr,
        session_name,
        Some("/work"),
    );
    request.ssh_config = ssh_config("host", "host", 22, "user", "/key");
    harness.attach_remote(request, &client).unwrap();
}

#[test]
fn shared_space_connects_through_the_client_relay_and_closes_it_with_the_pty() {
    for is_remote in [false, true] {
        // Given
        let harness = Harness::new();
        let relay = FakeRelay::new("/tmp/proxy/client");
        harness.relays.queue(&relay);

        // When
        attach_task_a_shared_space(&harness, is_remote);

        // Then
        let (upstream_socket_path, workspace_id) = harness.relays.created()[0].clone();
        assert_eq!(workspace_id, "w1");
        let args = harness.ptys.request(0).args;
        let launched = args.join(" ");
        assert!(launched.contains("HERDR_CLIENT_SOCKET_PATH="), "{launched}");
        assert!(!launched.contains("--session 'kanvibe'"));
        let forwarding_directory = std::path::Path::new(&upstream_socket_path)
            .parent()
            .unwrap()
            .to_path_buf();
        if is_remote {
            for forwarding_arg in ["-L", "-R", "ExitOnForwardFailure=yes"] {
                assert!(
                    args.iter().any(|arg| arg == forwarding_arg),
                    "{forwarding_arg}"
                );
            }
            assert!(upstream_socket_path.ends_with("/upstream"));
            assert!(forwarding_directory.exists());
        } else {
            assert_eq!(
                upstream_socket_path,
                "/config/herdr/sessions/kanvibe/herdr.sock"
            );
        }
        harness.registry.kill_all_terminal_sessions();
        assert_eq!(relay.close_count(), 1);
        // 원격 ssh 포워딩용 임시 디렉터리는 PTY와 함께 지운다
        assert!(!is_remote || !forwarding_directory.exists());
    }
}

#[test]
fn window_closed_while_preparing_the_relay_cleans_up_without_a_pty() {
    // Given
    let harness = Harness::new();
    let relay = FakeRelay::new("/tmp/proxy/client");
    harness.relays.queue(&relay);
    let relay_gate = harness.relays.create_gate.install();
    let client = FakeClient::new();
    let request = local_request(
        "task-a",
        None,
        SessionType::Herdr,
        "kanvibe:task-a",
        Some("/work"),
    );
    let attaching = harness.attach_in_background(AttachTarget::Local(request), &client);
    wait_until("중계를 준비하는 중이다", || {
        relay_gate.entered_count() == 1
    });

    // When
    client.set_open(false);
    relay_gate.open();
    let attachment = attaching.join().unwrap();

    // Then
    assert_eq!(attachment, Ok(None));
    assert_eq!(harness.ptys.count(), 0);
    assert_eq!(relay.close_count(), 1);
}

#[test]
fn tab_focus_goes_only_to_the_owning_task_client() {
    // Given
    let harness = Harness::new();
    let relay_a = FakeRelay::new("/tmp/a/client");
    let relay_b = FakeRelay::new("/tmp/b/client");
    harness.relays.queue(&relay_a);
    harness.relays.queue(&relay_b);
    attach_shared_space(&harness, "task-a", "/work/a");
    attach_shared_space(&harness, "task-b", "/work/b");

    // When
    harness
        .registry
        .focus_herdr_task_tab("task-a", "w1:t2")
        .unwrap();

    // Then
    assert_eq!(
        relay_a.requests(),
        vec![("tab.focus".to_string(), json!({ "tab_id": "w1:t2" }))]
    );
    assert!(relay_b.requests().is_empty());
    let error = harness
        .registry
        .focus_herdr_task_tab("task-a", "w2:t1")
        .unwrap_err();
    assert!(error.contains("다른 Space"));
    harness.registry.kill_all_terminal_sessions();
}

#[test]
fn pane_focus_goes_only_to_the_owning_task_client_and_rejects_others() {
    // Given
    let harness = Harness::new();
    let relay_a = FakeRelay::new("/tmp/a/client");
    let relay_b = FakeRelay::new("/tmp/b/client");
    harness.relays.queue(&relay_a);
    harness.relays.queue(&relay_b);
    assert!(
        harness
            .registry
            .focus_herdr_task_pane("task-a", "w1:p2")
            .unwrap_err()
            .contains("연결")
    );
    attach_shared_space(&harness, "task-a", "/work/a");
    attach_shared_space(&harness, "task-b", "/work/b");

    // When
    harness
        .registry
        .focus_herdr_task_pane("task-a", "w1:p2")
        .unwrap();

    // Then
    assert_eq!(
        relay_a.requests(),
        vec![("pane.focus".to_string(), json!({ "pane_id": "w1:p2" }))]
    );
    assert!(relay_b.requests().is_empty());
    assert!(
        harness
            .registry
            .focus_herdr_task_pane("task-a", "w2:p1")
            .unwrap_err()
            .contains("다른 Space")
    );
    assert_eq!(relay_a.requests().len(), 1);
    harness.registry.kill_all_terminal_sessions();
}

/// 160x48 PTY로 task-a 공유 Space에 붙고, 클라이언트 surface 크기를 알려 줄 중계를 돌려준다
fn attach_with_surface(harness: &Harness, surface_size: (u16, u16)) -> std::sync::Arc<FakeRelay> {
    let relay = FakeRelay::new("/tmp/a/client");
    *relay.surface_size.lock().unwrap() = Some(surface_size);
    harness.relays.queue(&relay);
    let mut request = local_request(
        "task-a",
        None,
        SessionType::Herdr,
        "kanvibe:task-a",
        Some("/work/a"),
    );
    request.columns = Some(160);
    request.rows = Some(48);
    harness.attach_local(request, &FakeClient::new()).unwrap();
    relay
}

fn surface_bounds(left: u16, top: u16, width: u16) -> Option<HerdrSurfaceBounds> {
    Some(HerdrSurfaceBounds {
        left,
        top,
        width,
        height: 47,
    })
}

#[test]
fn herdr_pane_bounds_follow_the_client_surface_and_tab_bar_position() {
    for (position, other_position, top) in [
        (HerdrTabBarPosition::Top, HerdrTabBarPosition::Bottom, 1),
        (HerdrTabBarPosition::Bottom, HerdrTabBarPosition::Top, 0),
    ] {
        // Given
        let harness = Harness::new();
        *harness.herdr.tab_bar_position.lock().unwrap() = Some(position);
        let relay = attach_with_surface(&harness, (134, 47));

        // When / Then
        assert_eq!(
            harness.registry.herdr_task_surface_bounds("task-a"),
            surface_bounds(26, top, 134)
        );
        *relay.surface_size.lock().unwrap() = Some((156, 47));
        assert_eq!(
            harness.registry.herdr_task_surface_bounds("task-a"),
            surface_bounds(4, top, 156)
        );
        *harness.herdr.tab_bar_position.lock().unwrap() = Some(other_position);
        assert_eq!(harness.registry.herdr_task_surface_bounds("task-a"), None);
        harness.registry.kill_all_terminal_sessions();
    }
}
