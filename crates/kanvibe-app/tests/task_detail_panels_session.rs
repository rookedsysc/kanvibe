//! 작업 정보 패널의 세션 타입 편집(TS `SessionTypeEditor`)을 실제 앱으로 확인한다.
//! 세션 도구 확인·설치, 기존 세션 생존 확인, 세션 타입 변경 응답은 QA 고장 주입으로 정하고,
//! 확인 대화상자는 묻게 바꿔 TS 테스트의 `window.confirm` 모의처럼 답한다.

mod task_detail_panels_support;

use gpui_kit::TestAppContext;
use gpui_kit::gpui;
use kanvibe_domain::task::{KanbanTask, SessionType};
use serde_json::{Value, json};
use task_detail_panels_support::*;

const DEPENDENCY: &str = "sessionDependency.getSessionDependencyStatus";
const INSTALL: &str = "sessionDependency.installSessionDependency";
const IS_ALIVE: &str = "kanban.isTaskSessionAlive";
const CHANGE: &str = "kanban.changeTaskSessionType";

fn dependency(is_available: bool) -> Value {
    json!({ "available": is_available, "toolName": "zellij", "isRemote": false, "sshHost": null, "blockedReason": null })
}

/// 세션이 붙은 작업을 열어 둔다. 확인 대화상자는 묻게 한다
fn open_session_task(cx: &mut TestAppContext) -> (Detail, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let task = task(&database, "세션 바꾸기", |task| {
        without_terminal_process(task);
    });
    let mut detail = open_detail(cx, &task.id);
    detail.ask_confirmations();
    detail.substitute(DEPENDENCY, dependency(true));
    (detail, task)
}

fn changed_task(task: &KanbanTask, session_type: SessionType) -> Value {
    let mut changed = task.clone();
    changed.session_type = Some(session_type);
    serde_json::to_value(changed).expect("task json")
}

fn choose(detail: &mut Detail, session_type: &str) {
    detail.click_role("combobox", "terminal");
    detail.click_role("option", session_type);
}

// parity: TS-components-SessionTypeEditor-5f7b1f
#[gpui::test]
fn pending_change_shows_progress_and_blocks_another_choice(cx: &mut TestAppContext) {
    let (mut detail, _) = open_session_task(cx);
    detail.substitute(IS_ALIVE, json!(false));
    detail.hang(CHANGE);

    choose(&mut detail, "zellij");
    assert!(detail.has_text("세션 유형을 변경하고 있습니다…"));

    detail.click_role("combobox", "terminal");
    assert!(!detail.has_role_name("option", "herdr"));
}

// parity: TS-components-SessionTypeEditor-77fc75
#[gpui::test]
fn change_errors_are_shown_and_not_retried(cx: &mut TestAppContext) {
    let (mut detail, _) = open_session_task(cx);
    detail.substitute(IS_ALIVE, json!(false));
    detail.fail(CHANGE, "SSH transport failure");

    choose(&mut detail, "zellij");

    assert!(detail.has_text_containing("SSH transport failure"));
    assert!(!detail.has_text("세션 유형을 변경하고 있습니다…"));
}

// parity: TS-components-SessionTypeEditor-eba788
#[gpui::test]
fn live_session_is_closed_after_confirmation(cx: &mut TestAppContext) {
    let (mut detail, task) = open_session_task(cx);
    let probe = detail.refresh_probe();
    detail.substitute(IS_ALIVE, json!(true));
    detail.substitute_matching(
        CHANGE,
        "\"zellij\",true",
        changed_task(&task, SessionType::Zellij),
    );
    detail.fail(CHANGE, "기존 세션을 남기는 변경");

    choose(&mut detail, "zellij");
    assert!(detail.has_pending_prompt());
    detail.answer_prompt("OK");

    assert!(!detail.has_text_containing("기존 세션을 남기는 변경"));
    assert_eq!(detail.refresh_count(&probe), 1);
}

// parity: TS-components-SessionTypeEditor-e07f89
#[gpui::test]
fn declining_keeps_the_existing_session(cx: &mut TestAppContext) {
    let (mut detail, task) = open_session_task(cx);
    detail.substitute(IS_ALIVE, json!(true));
    detail.substitute_matching(
        CHANGE,
        "\"zellij\",false",
        changed_task(&task, SessionType::Zellij),
    );
    detail.fail(CHANGE, "기존 세션을 닫는 변경");

    choose(&mut detail, "zellij");
    detail.answer_prompt("Cancel");

    assert!(!detail.has_text_containing("기존 세션을 닫는 변경"));
}

// parity: TS-components-SessionTypeEditor-a7ac59
#[gpui::test]
fn dead_session_changes_without_asking(cx: &mut TestAppContext) {
    let (mut detail, task) = open_session_task(cx);
    detail.substitute(IS_ALIVE, json!(false));
    detail.substitute_matching(
        CHANGE,
        "\"herdr\",false",
        changed_task(&task, SessionType::Herdr),
    );
    detail.fail(CHANGE, "다른 인자로 바꿈");

    choose(&mut detail, "herdr");

    assert!(!detail.has_pending_prompt());
    assert!(!detail.has_text_containing("다른 인자로 바꿈"));
}

// parity: TS-components-SessionTypeEditor-e23f88
#[gpui::test]
fn declining_the_tool_install_changes_nothing(cx: &mut TestAppContext) {
    let (mut detail, _) = open_session_task(cx);
    detail.clear_fault(DEPENDENCY);
    detail.substitute(DEPENDENCY, dependency(false));
    detail.fail(INSTALL, "설치를 요청함");
    detail.fail(IS_ALIVE, "세션 생존을 물음");
    detail.fail(CHANGE, "세션을 바꿈");

    choose(&mut detail, "zellij");
    assert!(detail.has_pending_prompt());
    detail.answer_prompt("Cancel");

    assert!(!detail.has_text_containing("설치를 요청함"));
    assert!(!detail.has_text_containing("세션 생존을 물음"));
    assert!(!detail.has_text_containing("세션을 바꿈"));
}

// parity: TS-components-SessionTypeEditor-08a44e
#[gpui::test]
fn missing_changed_task_reports_failure(cx: &mut TestAppContext) {
    let (mut detail, _) = open_session_task(cx);
    let probe = detail.refresh_probe();
    detail.substitute(IS_ALIVE, json!(false));
    detail.substitute(CHANGE, Value::Null);

    choose(&mut detail, "zellij");

    assert!(detail.has_text("세션 타입을 바꾸지 못했습니다. 서버 로그를 확인하세요."));
    assert_eq!(detail.refresh_count(&probe), 0);
}
