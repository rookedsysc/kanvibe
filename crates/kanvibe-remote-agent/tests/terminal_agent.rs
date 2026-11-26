//! 원격 터미널 agent를 실제 TLS·WebSocket과 stdin/stdout 봉투로 확인한다(TS `remoteTerminalTransport.test.ts`,
//! `remoteTerminalService.test.ts`의 agent 쪽 동작). Herdr controller는 같은 레코드를 주고받는 python3 가짜로 대신한다.

mod terminal_agent_support;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;
use terminal_agent_support::*;

fn base64(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(text)
}

#[test]
fn direct_and_relay_input_reach_one_shared_controller() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    let direct_grant = fixture.grant("direct-one", "direct-one");
    let relay_grant = fixture.grant("relay-one", "relay-one");
    assert_eq!(
        fixture
            .owner
            .request("grant", json!({ "grant": direct_grant }))["ok"],
        true
    );
    assert_eq!(
        fixture
            .owner
            .request("grant", json!({ "grant": relay_grant }))["ok"],
        true
    );

    let direct = fixture.connect(&direct_grant).expect("직접 연결");
    wait_until("controller 첫 화면", || {
        direct.received().contains("controller-ready")
    });
    direct.send_text("DIRECT_INPUT");
    wait_until("직접 입력의 반향", || {
        direct.received().contains("DIRECT_INPUT")
    });
    let attach = json!({ "relayId": "relay-transport", "grantId": "relay-one", "deviceId": "relay-one", "taskId": "task-b", "paneId": "w1:p1" });
    assert_eq!(fixture.owner.request("attach", attach)["ok"], true);
    fixture.owner.send(json!({ "type": "data", "relayId": "relay-transport", "binary": false, "data": base64("RELAY_INPUT") }));
    wait_until("relay 입력의 반향", || {
        fixture
            .owner
            .relay_output("relay-transport")
            .contains("RELAY_INPUT")
    });

    assert_eq!(
        fixture.counter_lines(),
        vec![
            "start".to_string(),
            format!("input:{}", base64("DIRECT_INPUT")),
            format!("input:{}", base64("RELAY_INPUT"))
        ]
    );
}

#[test]
fn history_refresh_from_a_direct_device_keeps_the_stream_and_later_input() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    let grant = fixture.grant("direct-refresh", "direct-refresh");
    assert_eq!(
        fixture.owner.request("grant", json!({ "grant": grant }))["ok"],
        true
    );

    let direct = fixture.connect(&grant).expect("직접 연결");
    wait_until("controller 첫 화면", || {
        direct.received().contains("controller-ready")
    });
    direct.send_binary(json!({ "type": "refresh-history" }).to_string().as_bytes());
    direct.send_text("AFTER_REFRESH");
    wait_until("갱신 뒤 입력", || {
        direct.closure().is_some() || direct.received().contains("AFTER_REFRESH")
    });

    assert_eq!(direct.closure(), None);
    assert_eq!(
        fixture.counter_lines(),
        vec![
            "start".to_string(),
            format!("input:{}", base64("AFTER_REFRESH"))
        ]
    );
}

#[test]
fn direct_history_uses_the_alternate_screen_and_wheel_goes_to_terminal_scroll() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    let grant = fixture.grant("direct-scroll", "direct-scroll");
    assert_eq!(
        fixture.owner.request("grant", json!({ "grant": grant }))["ok"],
        true
    );

    let direct = fixture.connect(&grant).expect("직접 연결");
    wait_until("controller 첫 화면", || {
        direct.received().contains("controller-ready")
    });
    let first = direct
        .messages()
        .into_iter()
        .find(|message| message.contains("initial-history"))
        .expect("이력");
    assert!(first.contains("\x1b[?1049h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l"));
    direct.send_binary(
        json!({ "type": "scroll", "direction": "up", "column": 4, "row": 2 })
            .to_string()
            .as_bytes(),
    );
    direct.send_text("AFTER_SCROLL");
    wait_until("스크롤 뒤 입력", || {
        direct.received().contains("AFTER_SCROLL")
    });

    let scroll =
        json!({ "type": "terminal.scroll", "direction": "up", "lines": 1, "column": 4, "row": 2 });
    assert_eq!(
        fixture.counter_lines(),
        vec![
            "start".to_string(),
            format!("scroll:{scroll}"),
            format!("input:{}", base64("AFTER_SCROLL"))
        ]
    );
}

#[test]
fn wrong_capability_and_foreign_pane_relay_are_rejected_and_device_revoke_closes_direct() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    let grant = fixture.grant("device-one", "device-one");
    fixture.owner.request("grant", json!({ "grant": grant }));

    assert_eq!(
        fixture.connect_with_token(&grant, "wrong-capability").err(),
        Some(401)
    );
    let foreign = json!({ "relayId": "wrong-scope", "grantId": "device-one", "deviceId": "device-one", "taskId": "task-b", "paneId": "w2:p1" });
    assert_eq!(fixture.owner.request("attach", foreign)["ok"], false);
    let direct = fixture.connect(&grant).expect("직접 연결");
    assert_eq!(
        fixture
            .owner
            .request("revokeDevice", json!({ "deviceId": "device-one" }))["ok"],
        true
    );

    wait_until("기기 해제로 닫힘", || direct.closure().is_some());
    assert_eq!(
        direct.closure(),
        Some((4001, "device-unpaired".to_string()))
    );
}

#[test]
fn abandoned_direct_handshake_keeps_the_capability_for_relay_fallback() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    let grant = fixture.grant("fallback", "device-a");
    fixture.owner.request("grant", json!({ "grant": grant }));
    let direct = fixture.connect(&grant).expect("직접 연결");
    wait_until("첫 메시지", || !direct.messages().is_empty());
    direct.close();

    let attach = json!({ "relayId": "relay-fallback", "grantId": "fallback", "deviceId": "device-a", "taskId": "task-b", "paneId": "w1:p1" });
    assert_eq!(fixture.owner.request("attach", attach)["ok"], true);
    fixture.owner.send(json!({ "type": "data", "relayId": "relay-fallback", "binary": false, "data": base64("fallback") }));

    wait_until("relay 입력의 반향", || {
        fixture
            .owner
            .relay_output("relay-fallback")
            .contains("fallback")
    });
}

#[test]
fn ordinary_unauthenticated_https_requests_get_a_prompt_not_found() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    let started_at = Instant::now();

    let status = https_get_status(fixture.port(), "/");

    assert_eq!(status, Some(404));
    assert!(started_at.elapsed() < Duration::from_millis(500));
}

#[test]
fn admission_uses_the_agent_clock_even_when_the_desktop_clock_is_a_day_behind() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    let mut grant = fixture.grant("late-clock", "device-a");
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    grant["expiresAt"] = json!(now_ms - 24 * 60 * 60_000 + 60_000);
    assert_eq!(
        fixture.owner.request("grant", json!({ "grant": grant }))["ok"],
        true
    );

    let direct = fixture.connect(&grant).expect("직접 연결");

    wait_until("첫 메시지", || !direct.messages().is_empty());
    assert!(direct.messages()[0].contains("history"));
}

#[test]
fn a_later_grant_reads_the_restored_native_baseline_before_claiming() {
    let Some(fixture) = Fixture::start() else {
        return;
    };
    fixture.write_geometry(120, 40);
    let first = fixture.grant("lease-a", "device-a");
    let second = fixture.grant("lease-b", "device-b");
    fixture.owner.request("grant", json!({ "grant": first }));
    let attach_a = json!({ "relayId": "relay-a", "grantId": "lease-a", "deviceId": "device-a", "taskId": "task-b", "paneId": "w1:p1" });
    assert_eq!(fixture.owner.request("attach", attach_a)["ok"], true);
    assert_eq!(fixture.geometry(), (80, 24));
    fixture.owner.request("grant", json!({ "grant": second }));

    fixture
        .owner
        .send(json!({ "type": "detach", "relayId": "relay-a" }));
    wait_until("A가 떠난 뒤 원래 크기", || {
        fixture.geometry() == (120, 40)
    });
    let attach_b = json!({ "relayId": "relay-b", "grantId": "lease-b", "deviceId": "device-b", "taskId": "task-b", "paneId": "w1:p1" });
    assert_eq!(fixture.owner.request("attach", attach_b)["ok"], true);
    assert_eq!(fixture.geometry(), (80, 24));
    fixture
        .owner
        .send(json!({ "type": "detach", "relayId": "relay-b" }));

    wait_until("B가 떠난 뒤 원래 크기", || {
        fixture.geometry() == (120, 40)
    });
    let records = fixture.controller_records();
    assert_eq!(
        records.last().map(String::as_str),
        Some(r#"{"type":"terminal.release"}"#)
    );
}

#[test]
fn controller_and_query_children_do_not_inherit_server_runtime_variables() {
    let Some(fixture) = BinaryFixture::start(&[
        ("PORT", "4100"),
        ("HOST", "0.0.0.0"),
        ("NODE_ENV", "production"),
        ("KANVIBE_SECRET", "x"),
    ]) else {
        return;
    };
    let mut grant = fixture.files.grant("env-check", "device-a");
    grant["geometryCommand"] = json!(format!(
        "env > {}/query.env; {}",
        fixture.files.directory(),
        grant["geometryCommand"].as_str().unwrap()
    ));
    fixture.owner.request("grant", json!({ "grant": grant }));

    let attach = json!({ "relayId": "relay-env", "grantId": "env-check", "deviceId": "device-a", "taskId": "task-b", "paneId": "w1:p1" });
    assert_eq!(fixture.owner.request("attach", attach)["ok"], true);

    for file_name in ["query.env", "counter.env"] {
        let environment = fixture.files.read(file_name);
        assert!(!environment.is_empty(), "{file_name} 비어 있음");
        for key in ["PORT=", "HOST=", "NODE_ENV=", "KANVIBE_"] {
            assert!(
                !environment.lines().any(|line| line.starts_with(key)),
                "{file_name}에 {key}가 샜다"
            );
        }
    }
}
