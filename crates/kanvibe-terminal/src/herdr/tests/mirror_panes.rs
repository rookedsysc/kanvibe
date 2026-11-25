//! 모바일 pane 미러 목록. 탭 순서, 확대 pane 너비, 최소 목록의 좌표 검증.

use std::sync::LazyLock;

use kanvibe_process::exec::ExecGitError;
use regex::Regex;
use serde_json::{Value, json};

use super::fixtures::{command_for, fixture, result};
use crate::herdr::HerdrMirrorPaneOptions;
use crate::pane_mirror::MirrorPane;
use crate::test_support::ScriptedShellRunner;

static PROCESS_INFO_PANE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("'process-info' '--pane' '([^']+)'").unwrap());

fn scroll(rows: i64) -> Value {
    json!({ "max_offset_from_bottom": 0, "offset_from_bottom": 0, "viewport_rows": rows })
}

fn rect(x: i64, width: i64) -> Value {
    json!({ "x": x, "y": 0, "width": width, "height": 40 })
}

fn panes() -> Value {
    json!([
        { "pane_id": "w1:p2", "tab_id": "w1:t2", "scroll": scroll(38) },
        { "pane_id": "w1:p1", "tab_id": "w1:t1", "scroll": scroll(38) },
        { "pane_id": "w1:p3", "tab_id": "w1:t1", "scroll": scroll(38) },
    ])
}

fn tabs() -> Value {
    json!([
        { "tab_id": "w1:t1", "number": 1, "label": "work", "focused": true },
        { "tab_id": "w1:t2", "number": 2, "label": "logs", "focused": false },
    ])
}

fn layout(zoomed: bool) -> Value {
    json!({ "layout": {
        "area": { "x": 0, "y": 0, "width": 120, "height": 40 },
        "focused_pane_id": "w1:p1",
        "zoomed": zoomed,
        "panes": [
            { "pane_id": "w1:p1", "rect": rect(0, 60) },
            { "pane_id": "w1:p3", "rect": rect(60, 60) },
            { "pane_id": "w1:p2", "rect": rect(0, 120) },
        ],
    } })
}

fn foreground(names: &[&str]) -> String {
    let processes: Vec<Value> = names.iter().map(|name| json!({ "name": name })).collect();
    result(json!({ "process_info": { "foreground_processes": processes } }))
}

/// 탭 두 개와 pane 세 개를 가진 세션. pane별 실행 명령은 `process_info`가 정한다
fn stub_session(
    runner: &ScriptedShellRunner,
    zoomed: bool,
    process_info: impl Fn(&str) -> Result<String, ExecGitError> + Send + Sync + 'static,
) {
    runner.respond(move |command, _| {
        if command == command_for(&["pane", "list"]) {
            return Some(Ok(result(json!({ "panes": panes() }))));
        }
        if command == command_for(&["tab", "list"]) {
            return Some(Ok(result(json!({ "tabs": tabs() }))));
        }
        if command.contains("'pane' 'layout'") {
            return Some(Ok(result(layout(zoomed))));
        }
        let pane_id = PROCESS_INFO_PANE
            .captures(command)
            .map_or(String::new(), |captures| captures[1].to_string());
        Some(process_info(&pane_id))
    });
}

fn mirror_pane(
    id: &str,
    tab_id: &str,
    tab_name: &str,
    command: &str,
    left: i64,
    width: i64,
) -> MirrorPane {
    MirrorPane {
        id: id.into(),
        tab_id: tab_id.into(),
        tab_name: tab_name.into(),
        command: command.into(),
        left,
        top: 0,
        width,
        height: 38,
    }
}

fn minimal(tab_id: Option<&str>, selected_pane_id: Option<&str>) -> HerdrMirrorPaneOptions {
    HerdrMirrorPaneOptions {
        is_minimal: true,
        tab_id: tab_id.map(str::to_string),
        selected_pane_id: selected_pane_id.map(str::to_string),
    }
}

fn layout_commands(runner: &ScriptedShellRunner) -> Vec<String> {
    runner
        .commands()
        .into_iter()
        .filter(|command| command.contains("'pane' 'layout'"))
        .collect()
}

fn has_process_info(runner: &ScriptedShellRunner) -> bool {
    runner
        .commands()
        .iter()
        .any(|command| command.contains("'process-info'"))
}

// parity: TS-lib-herdr-f9b2c6
#[test]
fn merges_geometry_and_commands_in_tab_order() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |pane_id| {
        Ok(foreground(&[if pane_id == "w1:p3" {
            "vim"
        } else {
            "zsh"
        }]))
    });

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes(
            "feature",
            Some("remote"),
            &HerdrMirrorPaneOptions::default(),
        )
        .unwrap();

    assert_eq!(
        mirror_panes,
        vec![
            mirror_pane("w1:p1", "w1:t1", "work", "zsh", 0, 60),
            mirror_pane("w1:p3", "w1:t1", "work", "vim", 60, 60),
            mirror_pane("w1:p2", "w1:t2", "logs", "zsh", 0, 120),
        ]
    );
    assert!(
        herdr
            .runner
            .calls()
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote"))
    );
}

// parity: TS-lib-herdr-5de66a
#[test]
fn minimal_list_skips_commands_and_unselected_tab_layouts() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |_| Ok(foreground(&[])));

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes("feature", Some("remote"), &minimal(None, None))
        .unwrap();

    assert_eq!(
        mirror_panes[0],
        mirror_pane("w1:p1", "w1:t1", "work", "", 0, 60)
    );
    let ids: Vec<&str> = mirror_panes.iter().map(|pane| pane.id.as_str()).collect();
    assert_eq!(ids, vec!["w1:p1", "w1:p3", "w1:p2"]);
    assert!(!has_process_info(&herdr.runner));
    assert!(
        !herdr
            .runner
            .commands()
            .contains(&command_for(&["pane", "layout", "--pane", "w1:p2"]))
    );
}

// parity: TS-lib-herdr-771725
#[test]
fn selected_pane_check_reads_only_its_tab_geometry_without_commands() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |_| Ok(foreground(&["vim"])));

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes("feature", Some("remote"), &minimal(None, Some("w1:p2")))
        .unwrap();

    let selected = mirror_panes.iter().find(|pane| pane.id == "w1:p2").unwrap();
    assert_eq!(
        (selected.width, selected.height, selected.command.as_str()),
        (120, 38, "")
    );
    assert_eq!(
        layout_commands(&herdr.runner),
        vec![command_for(&["pane", "layout", "--pane", "w1:p2"])]
    );
    assert!(
        herdr
            .runner
            .calls()
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote"))
    );
    assert!(!has_process_info(&herdr.runner));
}

// parity: TS-lib-herdr-b02a7e
#[test]
fn minimal_list_reads_every_pane_geometry_of_the_requested_tab() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |_| Ok(foreground(&[])));
    let mut reversed_tabs = tabs().as_array().unwrap().clone();
    reversed_tabs.reverse();
    herdr.runner.respond(move |command, _| {
        (command == command_for(&["tab", "list"]))
            .then(|| Ok(result(json!({ "tabs": reversed_tabs }))))
    });

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes("feature", Some("remote"), &minimal(Some("w1:t1"), None))
        .unwrap();

    let requested: Vec<&MirrorPane> = mirror_panes
        .iter()
        .filter(|pane| pane.tab_id == "w1:t1")
        .collect();
    assert_eq!(
        requested,
        vec![
            &mirror_pane("w1:p1", "w1:t1", "work", "", 0, 60),
            &mirror_pane("w1:p3", "w1:t1", "work", "", 60, 60)
        ]
    );
    assert_eq!(
        layout_commands(&herdr.runner),
        vec![command_for(&["pane", "layout", "--pane", "w1:p1"])]
    );
}

// parity: TS-lib-herdr-4d7381
#[test]
fn minimal_list_refuses_to_guess_when_needed_geometry_is_missing() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |_| Ok(foreground(&[])));
    herdr.runner.respond(|command, _| {
        command.contains("'pane' 'layout'").then(|| {
            Ok(result(
                json!({ "layout": { "area": { "width": 120, "height": 40 }, "panes": [] } }),
            ))
        })
    });

    let error = herdr
        .service
        .list_herdr_mirror_panes("feature", Some("remote"), &minimal(None, None))
        .unwrap_err();

    assert!(error.to_string().contains("배치를 확인할 수 없습니다"));
}

// parity: TS-lib-herdr-22970b
#[test]
fn minimal_list_rejects_an_invalid_zoomed_pane_width() {
    let herdr = fixture();
    stub_session(&herdr.runner, true, |_| Ok(foreground(&[])));
    herdr.runner.respond(|command, _| {
        command.contains("'pane' 'layout'").then(|| {
            let mut zoomed = layout(true);
            zoomed["layout"]["area"]["width"] = json!(0);
            Ok(result(zoomed))
        })
    });

    let error = herdr
        .service
        .list_herdr_mirror_panes("feature", Some("remote"), &minimal(None, None))
        .unwrap_err();

    assert!(error.to_string().contains("배치를 확인할 수 없습니다"));
}

// parity: TS-lib-herdr-f6c962
#[test]
fn unknown_selected_pane_is_rejected_without_reading_other_tabs() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |_| Ok(foreground(&[])));

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes("feature", Some("remote"), &minimal(None, Some("w9:p9")))
        .unwrap();

    assert_eq!(mirror_panes, vec![]);
    assert!(layout_commands(&herdr.runner).is_empty());
    assert!(!has_process_info(&herdr.runner));
}

// parity: TS-lib-herdr-3fe427
#[test]
fn zoomed_pane_uses_the_whole_tab_width_and_others_stay() {
    let herdr = fixture();
    stub_session(&herdr.runner, true, |_| Ok(foreground(&[])));

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes("feature", None, &HerdrMirrorPaneOptions::default())
        .unwrap();

    let width_of = |id: &str| {
        mirror_panes
            .iter()
            .find(|pane| pane.id == id)
            .unwrap()
            .width
    };
    assert_eq!(width_of("w1:p1"), 120);
    assert_eq!(width_of("w1:p3"), 60);
}

// parity: TS-lib-herdr-78537d
#[test]
fn pane_list_survives_unreadable_commands() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |_| {
        Err(ExecGitError::new("process lookup failed"))
    });

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes("feature", None, &HerdrMirrorPaneOptions::default())
        .unwrap();

    let commands: Vec<&str> = mirror_panes
        .iter()
        .map(|pane| pane.command.as_str())
        .collect();
    assert_eq!(commands, vec!["", "", ""]);
}

// parity: TS-lib-herdr-fc1715
#[test]
fn panes_that_failed_to_restore_are_left_out() {
    let herdr = fixture();
    stub_session(&herdr.runner, false, |_| Ok(foreground(&[])));
    herdr.runner.respond(|command, _| {
        (command == command_for(&["pane", "list"])).then(|| {
            let mut listed = vec![json!({ "pane_id": "w1:p1", "tab_id": "w1:t1", "restore_error": "Saved directory is unavailable." })];
            listed.extend(panes().as_array().unwrap().iter().filter(|pane| pane["pane_id"] != "w1:p1").cloned());
            Ok(result(json!({ "panes": listed })))
        })
    });

    let mirror_panes = herdr
        .service
        .list_herdr_mirror_panes("feature", None, &HerdrMirrorPaneOptions::default())
        .unwrap();

    let ids: Vec<&str> = mirror_panes.iter().map(|pane| pane.id.as_str()).collect();
    assert_eq!(ids, vec!["w1:p3", "w1:p2"]);
}
