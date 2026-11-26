//! 선택한 pane 크기 조회와 읽기 전용 명령의 재시도 허용 범위.

use std::time::Duration;

use serde_json::{Value, json};

use super::fixtures::{fixture, respond_in_sequence, result};
use crate::herdr::{HerdrPaneSize, build_herdr_command};

fn selected() -> Value {
    json!({ "pane_id": "w1:p1", "tab_id": "w1:t1", "scroll": { "max_offset_from_bottom": 120, "viewport_rows": 22 } })
}

fn layout() -> Value {
    json!({
        "area": { "x": 0, "y": 0, "width": 160, "height": 50 }, "focused_pane_id": "w1:p1", "zoomed": false,
        "panes": [{ "pane_id": "w1:p1", "rect": { "x": 0, "y": 0, "width": 78, "height": 24 } }],
    })
}

#[test]
fn reads_only_the_selected_pane_and_its_layout_for_each_zoom_state() {
    for (zoomed, focused, cols) in [
        (false, "w1:p1", 78),
        (true, "w1:p1", 160),
        (true, "w1:p2", 78),
    ] {
        let herdr = fixture();
        let mut zoom_layout = layout();
        zoom_layout["zoomed"] = json!(zoomed);
        zoom_layout["focused_pane_id"] = json!(focused);
        let pane = result(json!({ "pane": selected() }));
        let layout_output = result(json!({ "layout": zoom_layout }));
        respond_in_sequence(&herdr.runner, vec![Ok(&pane), Ok(&layout_output)]);

        let size = herdr
            .service
            .read_herdr_task_pane_size("feature", "w1:p1", Some("remote"))
            .unwrap();

        assert_eq!(size, Some(HerdrPaneSize { cols, rows: 22 }));
        let calls = herdr.runner.calls();
        let expected = [
            build_herdr_command("feature", &["pane", "get", "w1:p1"]).unwrap(),
            build_herdr_command("feature", &["pane", "layout", "--pane", "w1:p1"]).unwrap(),
        ];
        assert_eq!(
            calls
                .iter()
                .map(|call| call.command.clone())
                .collect::<Vec<_>>(),
            expected
        );
        for call in &calls {
            assert_eq!(call.ssh_host.as_deref(), Some("remote"));
            assert_eq!(call.options.timeout, Some(Duration::from_secs(5)));
            assert!(call.options.is_retry_safe);
            assert!(!call.options.is_failure_expected);
        }
    }
}

#[test]
fn unavailable_selected_pane_skips_the_layout_read() {
    let mut restore_failed = selected();
    restore_failed["restore_error"] = json!("Saved directory is unavailable");
    let mut other_pane = selected();
    other_pane["pane_id"] = json!("w1:p2");
    for pane in [Value::Null, restore_failed, other_pane] {
        let herdr = fixture();
        let output = result(json!({ "pane": pane }));
        respond_in_sequence(&herdr.runner, vec![Ok(&output)]);

        assert_eq!(
            herdr
                .service
                .read_herdr_task_pane_size("feature", "w1:p1", None)
                .unwrap(),
            None
        );
        assert_eq!(herdr.runner.calls().len(), 1);
    }
}

#[test]
fn selected_pane_missing_from_its_layout_is_not_granted() {
    let herdr = fixture();
    let mut empty_layout = layout();
    empty_layout["panes"] = json!([]);
    let pane = result(json!({ "pane": selected() }));
    let layout_output = result(json!({ "layout": empty_layout }));
    respond_in_sequence(&herdr.runner, vec![Ok(&pane), Ok(&layout_output)]);

    assert_eq!(
        herdr
            .service
            .read_herdr_task_pane_size("feature", "w1:p1", None)
            .unwrap(),
        None
    );
    assert_eq!(herdr.runner.calls().len(), 2);
}

#[test]
fn connection_failures_propagate_without_extra_probes() {
    for message in [
        "Connection reset by peer",
        "Permission denied (publickey)",
        "Host key verification failed",
    ] {
        let herdr = fixture();
        respond_in_sequence(&herdr.runner, vec![Err(message)]);

        let error = herdr
            .service
            .read_herdr_task_pane_size("feature", "w1:p1", Some("remote"))
            .unwrap_err();

        assert!(error.to_string().contains(message));
        assert_eq!(herdr.runner.calls().len(), 1);
    }
}

#[test]
fn keeps_the_kanvibe_space_guard() {
    let herdr = fixture();
    herdr.runner.respond(|command, _| {
        let output = if command.contains("'session' 'list'") {
            json!({ "sessions": [{ "name": "kanvibe", "running": true, "session_dir": "/shared", "socket_path": "/shared/herdr.sock" }] }).to_string()
        } else if command.starts_with("test -f ") {
            json!({ "task-a": "w1" }).to_string()
        } else if command.contains("'pane' 'get'") {
            result(json!({ "pane": selected() }))
        } else if command.contains("'pane' 'layout'") {
            result(json!({ "layout": layout() }))
        } else {
            return Some(Err(kanvibe_process::exec::ExecGitError::new("Unexpected broad query")));
        };
        Some(Ok(output))
    });

    let size = herdr
        .service
        .read_herdr_task_pane_size("kanvibe:task-a", "w1:p1", Some("remote"))
        .unwrap();

    assert_eq!(size, Some(HerdrPaneSize { cols: 78, rows: 22 }));
    let pane_probes = || -> Vec<String> {
        herdr
            .runner
            .commands()
            .into_iter()
            .filter(|command| command.contains("'pane'"))
            .collect()
    };
    assert_eq!(
        pane_probes(),
        vec![
            build_herdr_command("kanvibe", &["pane", "get", "w1:p1"]).unwrap(),
            build_herdr_command("kanvibe", &["pane", "layout", "--pane", "w1:p1"]).unwrap(),
        ]
    );
    let error = herdr
        .service
        .read_herdr_task_pane_size("kanvibe:task-a", "w2:p2", Some("remote"))
        .unwrap_err();
    assert!(error.to_string().contains("다른 Space"));
    assert_eq!(pane_probes().len(), 2);
}

#[test]
fn keeps_mutations_outside_the_read_only_retry_whitelist() {
    let mutations: [&[&str]; 3] = [
        &["pane", "split", "w1:p1"],
        &["pane", "send-text", "w1:p1", "hello"],
        &["workspace", "create"],
    ];
    for arguments in mutations {
        let herdr = fixture();
        let output = result(json!({}));
        respond_in_sequence(&herdr.runner, vec![Ok(&output)]);

        herdr
            .service
            .run_herdr_command("feature", arguments, Some("remote"))
            .unwrap();

        let calls = herdr.runner.calls();
        assert_eq!(
            calls[0].command,
            build_herdr_command("feature", arguments).unwrap()
        );
        assert_eq!(calls[0].ssh_host.as_deref(), Some("remote"));
        assert_eq!(calls[0].options.timeout, Some(Duration::from_secs(5)));
        assert!(!calls[0].options.is_retry_safe);
        assert!(!calls[0].options.is_failure_expected);
    }
}
