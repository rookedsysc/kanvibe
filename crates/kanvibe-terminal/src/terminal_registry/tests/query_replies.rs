//! 터미널 질의 응답. TS에서는 렌더러 xterm.js만 답했으므로 데스크탑 뷰가 붙어 있을 때만 PTY에 쓴다

use std::sync::Arc;

use kanvibe_domain::task::SessionType;

use super::fakes::*;

/// 1차 장치 속성 질의(DA1)
const PRIMARY_DEVICE_ATTRIBUTES_QUERY: &str = "\u{1b}[c";
/// 커서 위치 질의(CPR)
const CURSOR_POSITION_QUERY: &str = "\u{1b}[6n";

fn attach(harness: &Harness, session_type: SessionType, client: &Arc<FakeClient>) {
    let request = local_request("task-1", None, session_type, "proj-main", Some("/work"));
    harness.attach_local(request, client).unwrap().unwrap();
}

fn replies(pty: &FakePty) -> Vec<String> {
    pty.writes()
        .into_iter()
        .filter(|write| write.starts_with('\u{1b}'))
        .collect()
}

#[test]
fn desktop_view_answers_terminal_queries_exactly_once() {
    let harness = Harness::new();
    let desktop = FakeClient::desktop_view();
    attach(&harness, SessionType::Zellij, &desktop);
    let pty = harness.ptys.pty(0);

    pty.emit_output(PRIMARY_DEVICE_ATTRIBUTES_QUERY);
    pty.emit_output(&format!("ab{CURSOR_POSITION_QUERY}"));

    let written = replies(&pty);
    assert_eq!(written.len(), 2, "{written:?}");
    assert!(written[0].starts_with("\u{1b}[?"), "{written:?}");
    assert_eq!(written[1], "\u{1b}[1;3R");
    assert_eq!(desktop.sent().len(), 2, "출력은 그대로 화면에 간다");
}

#[test]
fn mobile_only_clients_get_no_replies_from_the_registry() {
    for session_type in [SessionType::Zellij, SessionType::Terminal] {
        let harness = Harness::new();
        let mobile = FakeClient::new();
        attach(&harness, session_type, &mobile);
        let pty = harness.ptys.pty(0);

        pty.emit_output(PRIMARY_DEVICE_ATTRIBUTES_QUERY);

        assert!(replies(&pty).is_empty(), "{session_type:?}");
    }
}

#[test]
fn replies_stop_once_the_desktop_view_closes() {
    let harness = Harness::new();
    let desktop = FakeClient::desktop_view();
    attach(&harness, SessionType::Terminal, &desktop);
    let pty = harness.ptys.pty(0);
    desktop.set_open(false);

    pty.emit_output(PRIMARY_DEVICE_ATTRIBUTES_QUERY);

    assert!(replies(&pty).is_empty());
}

#[test]
fn desktop_view_reads_the_registry_screen_of_a_multiplexer_session() {
    let harness = Harness::new();
    let desktop = FakeClient::desktop_view();
    attach(&harness, SessionType::Tmux, &desktop);
    let pty = harness.ptys.pty(0);

    pty.emit_output("\u{1b}[1;31mhello\u{1b}[0m");

    let first_cell = harness
        .registry
        .read_terminal_screen("task-1", None, |screen| {
            screen.visible_cells(0).unwrap()[0].clone()
        })
        .expect("데스크탑 뷰가 연 멀티플렉서 세션에도 화면이 있다");
    assert_eq!(first_cell.text, "h");
    assert!(first_cell.is_bold);
    assert_eq!(first_cell.fg, crate::screen::CellColor::Palette(1));
}

#[test]
fn mobile_opened_multiplexer_sessions_keep_no_screen() {
    let harness = Harness::new();
    attach(&harness, SessionType::Tmux, &FakeClient::new());

    assert!(
        harness
            .registry
            .read_terminal_screen("task-1", None, |_| ())
            .is_none()
    );
}
