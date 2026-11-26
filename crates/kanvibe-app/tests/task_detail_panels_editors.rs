//! 작업 정보 패널의 편집기(TS `PriorityEditor`·`PrioritySelector`·`ProjectColorEditor`·`TaskDetailTitleCard` 설명 편집·
//! `TaskDetailInfoCard`)와 상태 패널의 삭제 버튼(TS `DeleteTaskButton`)을 실제 앱으로 확인한다.
//! 저장 결과는 DB에 남은 값과 화면 글자로, 실패·지연은 QA 고장 주입으로 정한다.

mod task_detail_panels_support;

use std::time::Duration;

use gpui_kit::TestAppContext;
use gpui_kit::gpui;
use kanvibe_app::qa::faults::FaultMode;
use kanvibe_db::repositories::tasks::find_task;
use kanvibe_domain::task::{KanbanTask, TaskPriority};
use task_detail_panels_support::*;

const UPDATE_TASK: &str = "kanban.updateTask";
const UPDATE_COLOR: &str = "kanban.updateProjectColor";
const DELETE_TASK: &str = "kanban.deleteTask";

/// 작업 정보 패널이 열린 작업 상세
fn open_overview(
    cx: &mut TestAppContext,
    edit: impl FnOnce(&mut kanvibe_db::repositories::tasks::NewKanbanTask),
) -> (Detail, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let task = task(&database, "보드 필터 추가", edit);
    let detail = open_detail(cx, &task.id);
    (detail, task)
}

fn saved_task(detail: &mut Detail, task_id: &str) -> KanbanTask {
    let database = detail.database();
    find_task(&database.connection(), task_id)
        .expect("read task")
        .expect("task exists")
}

// ───────────── PrioritySelector / PriorityEditor ─────────────

#[gpui::test]
fn priority_selector_offers_four_options(cx: &mut TestAppContext) {
    let (mut detail, _) = open_overview(cx, |_| {});

    for label in ["없음", "!", "!!", "!!!"] {
        assert!(detail.has_role_name("button", label), "{label}");
    }
}

fn assert_priority_click_saves(
    cx: &mut TestAppContext,
    label: &str,
    expected: Option<TaskPriority>,
) {
    let (mut detail, task) = open_overview(cx, |task| {
        task.priority = match expected {
            None => Some(TaskPriority::Low),
            Some(_) => None,
        }
    });

    detail.click_role("button", label);

    assert_eq!(saved_task(&mut detail, &task.id).priority, expected);
}

#[gpui::test]
fn single_mark_saves_low(cx: &mut TestAppContext) {
    assert_priority_click_saves(cx, "!", Some(TaskPriority::Low));
}

#[gpui::test]
fn double_mark_saves_medium(cx: &mut TestAppContext) {
    assert_priority_click_saves(cx, "!!", Some(TaskPriority::Medium));
}

#[gpui::test]
fn triple_mark_saves_high(cx: &mut TestAppContext) {
    assert_priority_click_saves(cx, "!!!", Some(TaskPriority::High));
}

#[gpui::test]
fn none_option_clears_the_priority(cx: &mut TestAppContext) {
    assert_priority_click_saves(cx, "없음", None);
}

#[gpui::test]
fn saved_priority_refreshes_the_task_detail(cx: &mut TestAppContext) {
    let (mut detail, _) = open_overview(cx, |_| {});
    let probe = detail.refresh_probe();

    detail.click_role("button", "!");

    assert_eq!(detail.refresh_count(&probe), 1);
}

#[gpui::test]
fn pending_priority_save_blocks_other_options(cx: &mut TestAppContext) {
    let (mut detail, task) = open_overview(cx, |_| {});
    detail.delay(UPDATE_TASK, 1_000);

    detail.click_role("button", "!!");
    assert!(detail.has_text("저장 중…"));
    detail.click_role("button", "!!!");
    detail.advance_clock(Duration::from_millis(1_000));

    assert!(!detail.has_text("저장 중…"));
    assert_eq!(
        saved_task(&mut detail, &task.id).priority,
        Some(TaskPriority::Medium)
    );
}

#[gpui::test]
fn failed_priority_save_reports_the_reason(cx: &mut TestAppContext) {
    let (mut detail, _) = open_overview(cx, |_| {});
    let probe = detail.refresh_probe();
    detail.fail(UPDATE_TASK, "db locked");

    detail.click_role("button", "!!");

    assert!(detail.has_text_containing("저장하지 못했습니다:"));
    assert!(detail.has_text_containing("db locked"));
    assert_eq!(detail.refresh_count(&probe), 0);
}

// ───────────── ProjectColorEditor ─────────────

fn open_project_task(cx: &mut TestAppContext) -> (Detail, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let project = project_with_color(&database, "kanvibe", Some("#93C5FD"));
    let task = task(&database, "보드 필터 추가", |task| {
        task.project_id = Some(project.id.clone());
    });
    let detail = open_detail(cx, &task.id);
    (detail, task)
}

#[gpui::test]
fn failed_color_save_restores_the_saved_color(cx: &mut TestAppContext) {
    let (mut detail, _) = open_project_task(cx);
    let probe = detail.refresh_probe();
    detail.fail(UPDATE_COLOR, "db locked");

    detail.click_role("button", "Pick color");
    detail.click_role("button", "Color #F9A8D4");

    assert!(detail.has_text_containing("db locked"));
    assert!(detail.has_color_button("#93c5fd"));
    assert_eq!(detail.refresh_count(&probe), 0);
}

#[gpui::test]
fn color_picker_cannot_reopen_while_saving(cx: &mut TestAppContext) {
    let (mut detail, _) = open_project_task(cx);
    let probe = detail.refresh_probe();
    detail.delay(UPDATE_COLOR, 1_000);

    detail.click_role("button", "Pick color");
    detail.click_role("button", "Color #F9A8D4");
    detail.click_role("button", "Pick color");
    assert!(!detail.has_role_name("button", "OK"));

    detail.advance_clock(Duration::from_millis(1_000));
    assert_eq!(detail.refresh_count(&probe), 1);
}

// ───────────── TaskDetailInfoCard ─────────────

#[gpui::test]
fn project_shortcut_is_titled_with_the_base_branch(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let project = project(&database, "kanvibe");
    task(&database, "main 작업", |task| {
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("main".into());
    });
    let child = task(&database, "기능 작업", |task| {
        task.project_id = Some(project.id.clone());
        task.base_branch = Some("main".into());
    });
    // base 브랜치가 있는 작업은 자동 연결이 실제 PTY를 띄우므로, 연결 요청을 멈춰 테스트 스케줄러 밖 스레드를 만들지 않는다
    hang_before_open(cx, "kanban.connectTerminalSession");
    let mut detail = open_detail(cx, &child.id);

    assert!(detail.has_role_name("link", "main"));
}

#[gpui::test]
fn project_shortcut_falls_back_to_the_project_name(cx: &mut TestAppContext) {
    let (mut detail, _) = open_project_task(cx);

    assert!(detail.has_role_name("link", "kanvibe"));
}

#[gpui::test]
fn task_without_project_has_no_project_shortcut(cx: &mut TestAppContext) {
    let (mut detail, _) = open_overview(cx, |_| {});

    assert!(!detail.has_text("프로젝트"));
    assert!(!detail.has_role_name("link", "보드 필터 추가"));
}

#[gpui::test]
fn task_information_does_not_repeat_the_pr_link(cx: &mut TestAppContext) {
    let (mut detail, _) = open_overview(cx, |task| {
        task.pr_url = Some("https://github.com/example/repo/pull/1".into());
    });

    assert!(detail.has_text("작업 정보"));
    assert!(!detail.has_text("PR 링크"));
    assert!(!detail.has_text("PR"));
}

// ───────────── TaskDetailTitleCard 설명 편집 ─────────────

fn open_with_description(
    cx: &mut TestAppContext,
    description: Option<&str>,
) -> (Detail, KanbanTask) {
    open_overview(cx, |task| {
        task.description = description.map(str::to_string)
    })
}

#[gpui::test]
fn missing_description_shows_the_placeholder(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, None);

    assert!(detail.has_text("설명 추가"));
}

#[gpui::test]
fn description_text_is_shown(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, Some("기존 설명"));

    assert!(detail.has_text("기존 설명"));
}

#[gpui::test]
fn clicking_the_description_edits_it(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, Some("기존 설명"));

    detail.click_text("기존 설명");

    assert!(detail.has_textbox("기존 설명"));
}

#[gpui::test]
fn clicking_the_placeholder_starts_with_an_empty_draft(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, None);

    detail.click_text("설명 추가");

    assert!(detail.has_textbox(""));
}

#[gpui::test]
fn edit_mode_offers_cancel_and_save(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, Some("기존 설명"));

    detail.click_text("기존 설명");

    assert!(detail.has_role_name("button", "취소"));
    assert!(detail.has_role_name("button", "저장"));
}

#[gpui::test]
fn cancel_closes_the_editor_and_drops_the_draft(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, Some("기존 설명"));
    detail.click_text("기존 설명");
    detail.type_text(" 고침");

    detail.click_role("button", "취소");

    assert!(!detail.has_any_textbox());
    assert!(detail.has_text("기존 설명"));
}

#[gpui::test]
fn escape_closes_the_editor(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, Some("기존 설명"));
    detail.click_text("기존 설명");

    detail.keys("escape");

    assert!(!detail.has_any_textbox());
    assert!(detail.has_text("기존 설명"));
}

fn edit_description_and(
    cx: &mut TestAppContext,
    submit: impl FnOnce(&mut Detail),
) -> (Detail, KanbanTask) {
    let (mut detail, task) = open_with_description(cx, None);
    detail.click_text("설명 추가");
    detail.type_text("새로운 설명");
    submit(&mut detail);
    (detail, task)
}

#[gpui::test]
fn save_button_stores_the_changed_description(cx: &mut TestAppContext) {
    let (mut detail, task) = open_with_description(cx, None);
    let probe = detail.refresh_probe();
    detail.click_text("설명 추가");
    detail.type_text("새로운 설명");

    detail.click_role("button", "저장");

    let saved = saved_task(&mut detail, &task.id);
    assert_eq!(saved.description.as_deref(), Some("새로운 설명"));
    assert_eq!(detail.refresh_count(&probe), 1);
}

#[gpui::test]
fn mod_enter_saves_the_description(cx: &mut TestAppContext) {
    let (mut detail, task) = edit_description_and(cx, |detail| detail.keys("ctrl-enter"));

    let saved = saved_task(&mut detail, &task.id);
    assert_eq!(saved.description.as_deref(), Some("새로운 설명"));
}

#[gpui::test]
fn enter_saves_the_description_without_a_newline(cx: &mut TestAppContext) {
    let (mut detail, task) = edit_description_and(cx, |detail| detail.keys("enter"));

    let saved = saved_task(&mut detail, &task.id);
    assert_eq!(saved.description.as_deref(), Some("새로운 설명"));
    assert!(!detail.has_any_textbox());
}

#[gpui::test]
fn pending_description_save_is_sent_once(cx: &mut TestAppContext) {
    let (mut detail, task) = open_with_description(cx, Some("기존 설명"));
    // 두 번째 저장 요청이 나가면 실패를 받아 화면에 남는다
    detail.fault_after(
        UPDATE_TASK,
        FaultMode::Fail,
        Some("두 번째 저장"),
        None,
        Some(1),
    );
    detail.delay(UPDATE_TASK, 1_000);
    detail.click_text("기존 설명");
    detail.type_text(" 고침");

    detail.keys("enter");
    detail.keys("enter");
    assert!(detail.has_role_name("button", "저장 중…"));
    detail.advance_clock(Duration::from_millis(1_000));

    assert!(!detail.has_text_containing("두 번째 저장"));
    let saved = saved_task(&mut detail, &task.id);
    assert_eq!(saved.description.as_deref(), Some("기존 설명 고침"));
}

#[gpui::test]
fn failed_description_save_keeps_the_editor_open(cx: &mut TestAppContext) {
    let (mut detail, _) = open_with_description(cx, Some("기존 설명"));
    let probe = detail.refresh_probe();
    detail.fail(UPDATE_TASK, "db locked");
    detail.click_text("기존 설명");
    detail.type_text(" 고침");

    detail.click_role("button", "저장");

    assert!(detail.has_text_containing("저장하지 못했습니다:"));
    assert!(detail.has_any_textbox());
    assert_eq!(detail.refresh_count(&probe), 0);
}

// ───────────── DeleteTaskButton ─────────────

fn open_status(cx: &mut TestAppContext) -> (Detail, KanbanTask) {
    let (mut detail, task) = open_overview(cx, |_| {});
    detail.keys("ctrl-2");
    (detail, task)
}

#[gpui::test]
fn pending_delete_blocks_the_button_and_says_deleting(cx: &mut TestAppContext) {
    let (mut detail, task) = open_status(cx);
    detail.fault_after(
        DELETE_TASK,
        FaultMode::Fail,
        Some("두 번째 삭제"),
        None,
        Some(1),
    );
    detail.delay(DELETE_TASK, 1_000);

    detail.click_role("button", "삭제");
    assert!(detail.has_role_name("button", "삭제 중…"));
    detail.click_role("button", "삭제 중…");
    detail.advance_clock(Duration::from_millis(1_000));

    assert!(!detail.has_text_containing("두 번째 삭제"));
    let database = detail.database();
    assert!(
        find_task(&database.connection(), &task.id)
            .unwrap()
            .is_none()
    );
}

#[gpui::test]
fn failed_delete_reports_the_reason_and_reenables_the_button(cx: &mut TestAppContext) {
    let (mut detail, _) = open_status(cx);
    detail.fail(DELETE_TASK, "worktree busy");

    detail.click_role("button", "삭제");

    assert!(detail.has_text_containing("삭제하지 못했습니다:"));
    assert!(detail.has_text_containing("worktree busy"));
    detail.clear_fault(DELETE_TASK);
    detail.hang(DELETE_TASK);
    detail.click_role("button", "삭제");
    assert!(detail.has_role_name("button", "삭제 중…"));
}
