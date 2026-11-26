//! 작업 정보 패널의 변경 파일 요약(TS `TaskDiffSummary`)과 같은 프로젝트 작업 모달(TS `ProjectBranchTasksModal`)을
//! 실제 앱으로 확인한다. 변경 파일 목록은 QA 고장 주입의 값 대체로 정한다.

mod task_detail_panels_support;

use gpui_kit::TestAppContext;
use gpui_kit::gpui;
use kanvibe_domain::task::TaskStatus;
use serde_json::json;
use task_detail_panels_support::*;

const DIFF_FILES: &str = "diff.getGitDiffFiles";
const BOARD_TASKS: &str = "kanban.getTasksByStatus";

/// 브랜치가 있는 작업을 열고, 변경 파일 조회를 값 대체한 뒤 작업 정보 패널을 다시 열어 바로 읽게 한다
fn open_branch_task(cx: &mut TestAppContext, files: serde_json::Value) -> Detail {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let task = task(&database, "보드 필터 추가", |task| {
        task.branch_name = Some("feat/board-filters".into());
    });
    let mut detail = open_detail(cx, &task.id);
    detail.substitute(DIFF_FILES, files);
    detail.keys("ctrl-1");
    detail.keys("ctrl-1");
    detail
}

fn changed_files() -> serde_json::Value {
    json!([
        { "path": "src/components/TaskCard.tsx", "status": "modified", "additions": 60, "deletions": 3 },
        { "path": "src/hooks/useTaskDiffStats.ts", "status": "added", "additions": 40, "deletions": 0 }
    ])
}

#[gpui::test]
fn diff_summary_shows_totals_with_the_list_folded(cx: &mut TestAppContext) {
    let mut detail = open_branch_task(cx, changed_files());

    assert!(detail.has_text("2"));
    assert!(detail.has_text("+100"));
    assert!(detail.has_text("-3"));
    assert!(!detail.has_text("src/components/TaskCard.tsx"));
}

#[gpui::test]
fn clicking_the_totals_unfolds_the_file_list(cx: &mut TestAppContext) {
    let mut detail = open_branch_task(cx, changed_files());

    detail.click_role("button", "변경 파일 목록 펼치기");

    assert!(detail.has_text("src/components/TaskCard.tsx"));
    assert!(detail.has_text("src/hooks/useTaskDiffStats.ts"));
    assert!(detail.has_role_name("button", "변경 파일 목록 접기"));
}

#[gpui::test]
fn no_changes_leave_only_the_diff_link(cx: &mut TestAppContext) {
    let mut detail = open_branch_task(cx, json!([]));

    assert!(detail.has_role_name("link", "Diff 보기"));
    assert!(!detail.has_role_name("button", "변경 파일 목록 펼치기"));
}

#[gpui::test]
fn failed_project_task_list_offers_retry(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.keep());
    let project = project(&database, "kanvibe");
    let opened = task(&database, "보드 필터 추가", |task| {
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("feat/board-filters".into());
        without_terminal_process(task);
    });
    task(&database, "Login page", |task| {
        task.project_id = Some(project.id.clone());
        task.status = TaskStatus::Review;
        task.branch_name = Some("feat/login".into());
        without_terminal_process(task);
    });
    let mut detail = open_detail(cx, &opened.id);
    detail.fail(BOARD_TASKS, "db closed");

    detail.click_role("button", "View other tasks in this project");
    assert!(detail.has_role_name("button", "다시 시도"));
    assert!(!detail.has_text("작업 없음"));

    detail.clear_fault(BOARD_TASKS);
    detail.click_role("button", "다시 시도");

    assert!(detail.has_text("Login page"));
    assert!(detail.has_text("총 2개"));
    assert!(!detail.has_role_name("button", "다시 시도"));
}
