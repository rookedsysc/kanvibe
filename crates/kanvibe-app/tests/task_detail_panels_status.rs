//! 상태 패널의 상태 이동 버튼(TS `TaskDetailRoute`의 `statusMove`)을 실제 앱으로 열어 확인한다.
//! 이동 응답의 지연·실패는 QA 고장 주입(`kanban.updateTaskStatus`)으로 정하고, 저장 결과는 DB에서 읽는다.

mod task_detail_panels_support;

use std::time::Duration;

use gpui_kit::TestAppContext;
use gpui_kit::gpui;
use kanvibe_app::qa::faults::FaultMode;
use kanvibe_db::repositories::tasks::find_task;
use kanvibe_domain::task::TaskStatus;
use task_detail_panels_support::*;

const UPDATE_STATUS: &str = "kanban.updateTaskStatus";
const MOVING_TO_REVIEW: &str = "Review(으)로 옮기는 중…";

/// Todo 작업을 열고 상태 패널(`Mod+2`)을 연다. 프로젝트가 없어 자동 연결도 hook 조회도 하지 않는다
fn open_status_panel(cx: &mut TestAppContext) -> (Detail, String) {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let task = task(&database, "상태 옮기기", |task| {
        task.status = TaskStatus::Todo
    });
    let mut detail = open_detail(cx, &task.id);
    detail.keys("ctrl-2");
    (detail, task.id)
}

fn stored_status(detail: &mut Detail, task_id: &str) -> TaskStatus {
    let database = detail.database();
    let task = find_task(&database.connection(), task_id).unwrap();
    task.expect("task").status
}

// parity: TS-renderer-TaskDetailRoute-6039de
#[gpui::test]
fn a_pending_move_names_its_target_and_blocks_other_moves(cx: &mut TestAppContext) {
    let (mut detail, task_id) = open_status_panel(cx);
    // 첫 이동만 1초 걸리고, 두 번째 이동 요청이 나가면 실패 안내로 드러난다
    detail.fault_after(
        UPDATE_STATUS,
        FaultMode::Fail,
        Some("second move"),
        None,
        Some(1),
    );
    detail.delay(UPDATE_STATUS, 1_000);

    detail.click_role("button", "Review");
    assert!(detail.has_role_name("button", MOVING_TO_REVIEW));
    detail.click_role("button", "Progress");
    assert!(detail.has_role_name("button", MOVING_TO_REVIEW));

    detail.advance_clock(Duration::from_millis(1_000));
    assert_eq!(stored_status(&mut detail, &task_id), TaskStatus::Review);
    assert!(detail.has_role_name("button", "Todo"));
    assert!(!detail.has_role_name("button", "Review"));
    assert!(!detail.has_text_containing("second move"));
}

// parity: TS-renderer-TaskDetailRoute-7e3688
#[gpui::test]
fn a_failed_move_keeps_the_status_and_retry_moves_again(cx: &mut TestAppContext) {
    let (mut detail, task_id) = open_status_panel(cx);
    detail.fail(UPDATE_STATUS, "db locked");

    detail.click_role("button", "Review");
    // 사유는 TS IPC 오류와 같은 문구(`format_service_error`)로 보인다
    assert!(detail.has_text_containing("상태를 바꾸지 못했습니다: "));
    assert!(detail.has_text_containing("db locked"));
    assert!(detail.has_role_name("button", "Review"));
    assert_eq!(stored_status(&mut detail, &task_id), TaskStatus::Todo);

    detail.clear_fault(UPDATE_STATUS);
    detail.click_role("button", "다시 시도");

    assert!(!detail.has_role_name("button", "Review"));
    assert!(!detail.has_text_containing("상태를 바꾸지 못했습니다"));
    assert_eq!(stored_status(&mut detail, &task_id), TaskStatus::Review);
}

// parity: TS-renderer-TaskDetailRoute-85b7a9
#[gpui::test]
fn moving_to_done_stays_on_the_detail_screen(cx: &mut TestAppContext) {
    let (mut detail, task_id) = open_status_panel(cx);

    detail.click_role("button", "Done");

    assert_eq!(stored_status(&mut detail, &task_id), TaskStatus::Done);
    assert!(!detail.has_role_name("button", "Done"));
    // 보드로 가지 않고 같은 상태 패널에 남아 다음 이동 버튼을 보인다
    assert!(detail.has_role_name("button", "Todo"));
}
