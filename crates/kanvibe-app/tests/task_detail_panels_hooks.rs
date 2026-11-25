//! 상태 패널의 hooks 카드(TS `HooksStatusCard`)를 실제 앱으로 열어 확인한다.
//! hook 상태 조회와 설치 응답은 QA 고장 주입의 값 대체·실패로 정한다(TS 테스트의 `actions/project` 모의와 같은 자리).

mod task_detail_panels_support;

use gpui_kit::TestAppContext;
use gpui_kit::gpui;
use kanvibe_app::app_events::{RefreshScope, trigger_refresh};
use kanvibe_app::qa::faults::FaultMode;
use serde_json::{Value, json};
use task_detail_panels_support::*;

const STATUSES: &str = "project.getTaskHooksStatuses";
const INSTALL_CLAUDE: &str = "project.installTaskHooks";

fn missing_statuses() -> Value {
    json!({ "claude": null, "gemini": null, "codex": null, "openCode": null })
}

/// 프로젝트 작업을 열고 상태 패널(`Mod+2`)을 연다. `setup`은 패널을 열기 전에 고장을 건다
fn open_status_panel(
    cx: &mut TestAppContext,
    is_remote: bool,
    setup: impl FnOnce(&mut Detail),
) -> Detail {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let project = project(&database, "kanvibe");
    let task = task(&database, "hooks 확인", |task| {
        task.project_id = Some(project.id.clone());
        task.ssh_host = is_remote.then(|| "devbox".to_string());
    });
    let mut detail = open_detail(cx, &task.id);
    setup(&mut detail);
    detail.keys("ctrl-2");
    detail
}

// parity: TS-components-HooksStatusCard-c8efdf
#[gpui::test]
fn hook_tools_are_listed_inline(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, false, |detail| {
        detail.substitute(STATUSES, missing_statuses())
    });

    assert!(detail.has_text("HOOKS 상태"));
    for tool in ["Claude", "Gemini", "Codex", "OpenCode"] {
        assert!(detail.has_text(tool), "{tool}");
    }
    assert!(detail.has_text("0/4"));
}

// parity: TS-components-HooksStatusCard-6e362e
#[gpui::test]
fn installed_hooks_offer_reinstall(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, false, |detail| {
        let statuses = json!({ "claude": { "installed": true }, "gemini": null, "codex": null, "openCode": null });
        detail.substitute(STATUSES, statuses)
    });

    assert!(detail.has_role_name("button", "재설치 Claude"));
    assert!(detail.has_role_name("button", "Hooks 설치 Gemini"));
    assert!(detail.has_text("1/4"));
}

// parity: TS-components-HooksStatusCard-8a2b46
#[gpui::test]
fn remote_status_is_not_described_as_unsupported(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, true, |detail| {
        detail.fail(STATUSES, "ssh: Could not resolve hostname devbox")
    });

    assert!(detail.has_text("HOOKS 상태"));
    assert!(!detail.has_text_containing("지원하지 않"));
    assert!(detail.has_text("미설치"));
}

// parity: TS-components-HooksStatusCard-f7a60a
#[gpui::test]
fn failed_open_code_verification_differs_from_missing_hooks(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, false, |detail| {
        let statuses = json!({
            "claude": null, "gemini": null, "codex": null,
            "openCode": { "installed": false, "pluginRegistrationError": "timeout" }
        });
        detail.substitute(STATUSES, statuses)
    });

    assert!(detail.has_text("검증 실패"));
}

// parity: TS-components-HooksStatusCard-fd11d1
#[gpui::test]
fn installing_claude_updates_its_row_and_rereads_all_statuses_once(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, false, |detail| {
        detail.substitute(STATUSES, missing_statuses())
    });
    detail.substitute(
        INSTALL_CLAUDE,
        json!({ "success": true, "status": { "installed": true } }),
    );
    detail.clear_fault(STATUSES);
    let installed = json!({ "installed": true });
    detail.substitute(
        STATUSES,
        json!({ "claude": installed, "gemini": installed, "codex": installed, "openCode": installed }),
    );

    detail.click_role("button", "Hooks 설치 Claude");

    assert!(detail.has_text("Hooks 설치 완료"));
    assert!(detail.has_role_name("button", "재설치 Claude"));
    assert!(detail.has_text("4/4"));
}

// parity: TS-components-HooksStatusCard-96ff36
#[gpui::test]
fn reading_statuses_says_loading_until_done(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, true, |detail| detail.hang(STATUSES));
    assert!(detail.has_text("로딩 중..."));

    let mut done = open_status_panel(cx, true, |detail| {
        detail.substitute(STATUSES, missing_statuses())
    });
    assert!(!done.has_text("로딩 중..."));
}

// parity: TS-components-HooksStatusCard-b27f4f
#[gpui::test]
fn failed_install_without_result_offers_retry_of_the_same_tool(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, false, |detail| {
        detail.substitute(STATUSES, missing_statuses())
    });
    detail.fail(INSTALL_CLAUDE, "ipc closed");

    detail.click_role("button", "Hooks 설치 Claude");
    assert!(detail.has_text_containing("Hooks 설치 실패"));
    assert!(detail.has_role_name("button", "다시 시도"));

    detail.clear_fault(INSTALL_CLAUDE);
    detail.substitute(
        INSTALL_CLAUDE,
        json!({ "success": true, "status": { "installed": true } }),
    );
    detail.click_role("button", "다시 시도");

    assert!(!detail.has_text_containing("Hooks 설치 실패"));
    assert!(detail.has_text("Hooks 설치 완료"));
}

// parity: TS-components-HooksStatusCard-1e4e9b
#[gpui::test]
fn reopened_panel_shows_the_statuses_already_read(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, false, |detail| {
        let statuses = json!({ "claude": null, "gemini": null, "codex": null, "openCode": { "installed": true } });
        detail.substitute(STATUSES, statuses)
    });
    detail.keys("ctrl-2");
    detail.clear_fault(STATUSES);
    detail.hang(STATUSES);

    detail.keys("ctrl-2");

    // 다시 읽는 동안에도 읽는 중 표시 없이 이미 읽은 OpenCode 상태를 보인다
    assert!(detail.has_role_name("button", "재설치 OpenCode"));
    assert!(!detail.has_text("로딩 중..."));
}

const CONNECT: &str = "kanban.connectTerminalSession";
const PR_LOOKUP: &str = "kanban.fetchAndSavePrUrl";

/// 세션 없는 프로젝트 작업을 연다(상태 패널은 닫힌 채). 자동 연결과 PR 자동 조회는 멈춰 바깥 프로세스를 만들지 않는다
fn open_closed_project_task(cx: &mut TestAppContext) -> Detail {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let project = project(&database, "kanvibe");
    let task = task(&database, "hooks 확인", |task| {
        task.project_id = Some(project.id.clone());
    });
    hang_before_open(cx, CONNECT);
    hang_before_open(cx, PR_LOOKUP);
    open_detail(cx, &task.id)
}

/// 보드 갱신이 이어진 것처럼 작업 상세에 새로 고침 신호를 보낸다
fn board_refresh(detail: &mut Detail) {
    detail
        .cx
        .update(|_, cx| trigger_refresh(RefreshScope::All, cx));
    detail.draw();
}

fn statuses_with(installed: &[&str]) -> Value {
    let status = |tool: &str| match installed.contains(&tool) {
        true => json!({ "installed": true }),
        false => Value::Null,
    };
    json!({
        "claude": status("claude"), "gemini": status("gemini"),
        "codex": status("codex"), "openCode": status("openCode"),
    })
}

// parity: TS-renderer-TaskDetailRoute-34029c
#[gpui::test]
fn slow_ai_sessions_do_not_hold_back_hook_statuses(cx: &mut TestAppContext) {
    let mut detail = open_closed_project_task(cx);
    detail.hang("project.getTaskAiSessions");
    detail.keys("ctrl-3");
    detail.substitute(STATUSES, statuses_with(&["codex"]));

    detail.keys("ctrl-2");

    assert!(detail.has_role_name("button", "재설치 Codex"));
    assert!(!detail.has_text("로딩 중..."));
}

// parity: TS-renderer-TaskDetailRoute-478add
#[gpui::test]
fn hook_statuses_are_read_only_while_the_status_panel_is_open(cx: &mut TestAppContext) {
    let mut detail = open_closed_project_task(cx);
    // 첫 조회만 Claude 설치로 답한다. 닫힌 창이 보드 갱신마다 조회했다면 패널을 열 때 실패가 보인다
    detail.fault_after(
        STATUSES,
        FaultMode::Fail,
        Some("too many reads"),
        None,
        Some(1),
    );
    detail.substitute(STATUSES, statuses_with(&["claude"]));
    for _ in 0..3 {
        board_refresh(&mut detail);
    }
    detail.keys("ctrl-2");
    assert!(detail.has_role_name("button", "재설치 Claude"));

    // 패널이 열려 있으면 보드 갱신이 백그라운드 설치 결과를 다시 읽는다(두 번째 조회)
    detail.clear_fault(STATUSES);
    detail.substitute(STATUSES, statuses_with(&["claude", "codex"]));
    board_refresh(&mut detail);
    assert!(detail.has_text("2/4"));

    // 패널을 닫은 뒤의 갱신은 읽지 않으므로, 다시 열 때가 세 번째 조회다
    detail.keys("ctrl-2");
    detail.clear_fault(STATUSES);
    let every_tool = statuses_with(&["claude", "gemini", "codex", "openCode"]);
    detail.fault_after(STATUSES, FaultMode::Result, None, Some(every_tool), Some(3));
    detail.substitute(STATUSES, statuses_with(&["claude"]));
    board_refresh(&mut detail);
    detail.keys("ctrl-2");
    assert!(detail.has_text("1/4"));
}

// parity: TS-renderer-TaskDetailRoute-93fbc9
#[gpui::test]
fn the_status_card_says_loading_while_statuses_are_read(cx: &mut TestAppContext) {
    let mut detail = open_closed_project_task(cx);
    // 첫 조회는 끝나지 않고, 그 뒤 조회는 Gemini 설치로 답한다
    let gemini = statuses_with(&["gemini"]);
    detail.fault_after(STATUSES, FaultMode::Result, None, Some(gemini), Some(1));
    detail.hang(STATUSES);

    detail.keys("ctrl-2");
    assert!(detail.has_text("로딩 중..."));

    // 열린 패널의 다시 읽기가 끝나면 읽는 중 표시가 사라진다
    board_refresh(&mut detail);
    assert!(!detail.has_text("로딩 중..."));
    assert!(detail.has_role_name("button", "재설치 Gemini"));
}

// parity: TS-renderer-project-c87884
#[gpui::test]
fn installing_task_hooks_does_not_refresh_the_whole_app(cx: &mut TestAppContext) {
    let mut detail = open_status_panel(cx, false, |detail| {
        detail.substitute(STATUSES, missing_statuses())
    });
    detail.substitute(
        INSTALL_CLAUDE,
        json!({ "success": true, "status": { "installed": true } }),
    );
    let probe = detail.refresh_probe();

    detail.click_role("button", "Hooks 설치 Claude");

    assert!(detail.has_text("Hooks 설치 완료"));
    assert_eq!(detail.refresh_count(&probe), 0);
}
