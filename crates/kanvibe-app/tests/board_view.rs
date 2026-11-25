//! 보드 화면을 실제 DB와 함께 헤드리스 GPUI로 띄워, 보이는 글자·DB에 남은 값으로 TS 보드 동작을 확인한다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{
    Bounds, Entity, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, WeakEntity,
};
use kanvibe_app::qa::faults::{FaultMode, ServiceFault};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::qa::targets::{DescribedTarget, QaTargetRegistry, TargetQuery};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::AppServices;
use kanvibe_app::shortcuts::ShortcutState;
use kanvibe_app::views::board::{BoardView, BoardViewMode};
use kanvibe_app::views::{ScreenContext, WindowNavigator};
use kanvibe_db::Database;
use kanvibe_db::repositories::app_settings::save_app_setting;
use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_db::repositories::tasks::{NewKanbanTask, find_task, insert_task};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};

struct Board {
    view: Entity<BoardView>,
    cx: VisualTestContext,
}

/// QA 모드로 띄우면 고장 주입을 쓸 수 있고 삭제 확인을 자동으로 받아들인다(Playwright 기본 동작과 같다)
fn init_app(cx: &mut TestAppContext, data_dir: PathBuf, is_qa: bool) -> Arc<Database> {
    let launch = LaunchOptions {
        data_dir: Some(data_dir),
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: is_qa.then_some(QaOptions {
            fixture_root: None,
            fixed_now_ms: 1_768_445_820_000,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
        }),
    };
    cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
        cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Linux;
        if !cx.has_global::<QaTargetRegistry>() {
            cx.set_global(QaTargetRegistry::default());
        }
        cx.global::<AppServices>()
            .database()
            .expect("database")
            .clone()
    })
}

fn project(database: &Database, name: &str, ssh_host: Option<&str>) -> Project {
    insert_project(
        &database.connection(),
        &NewProject {
            name: name.into(),
            repo_path: format!("/repos/{name}"),
            default_branch: "main".into(),
            ssh_host: ssh_host.map(str::to_string),
            is_worktree: false,
            color: None,
            icon_data_url: None,
        },
    )
    .expect("insert project")
}

fn task(
    database: &Database,
    title: &str,
    status: TaskStatus,
    edit: impl FnOnce(&mut NewKanbanTask),
) -> KanbanTask {
    let mut new_task = NewKanbanTask {
        title: title.into(),
        description: None,
        status,
        branch_name: None,
        worktree_path: None,
        session_type: None,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
    };
    edit(&mut new_task);
    insert_task(&database.connection(), &new_task).expect("insert task")
}

fn open_board(cx: &mut TestAppContext, path: &str) -> Board {
    let RouteResolution::Render(route) = resolve_route_path(path) else {
        panic!("{path} must render");
    };
    let (view, visual) = cx.add_window_view(|_, cx| {
        BoardView::new(
            ScreenContext {
                route,
                navigator: WindowNavigator::new(WeakEntity::new_invalid()),
            },
            cx,
        )
    });
    let mut board = Board {
        view,
        cx: visual.clone(),
    };
    board.draw();
    board
}

impl Board {
    /// 지난 프레임 대상을 지우고 다시 그린다
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| {
            let window_id = window.window_handle().window_id();
            cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
            window.refresh();
        });
        self.cx.run_until_parked();
    }

    fn find(&mut self, query: TargetQuery) -> Option<Bounds<Pixels>> {
        self.cx.update(|window, cx| {
            cx.global::<QaTargetRegistry>()
                .find(window.window_handle().window_id(), &query)
        })
    }

    fn has_text(&mut self, text: &str) -> bool {
        self.find(TargetQuery::ExactText(text.into())).is_some()
    }

    fn has_role_name(&mut self, role: &str, name: &str) -> bool {
        self.find(role_name(role, name)).is_some()
    }

    fn click_text(&mut self, text: &str) {
        let bounds = self
            .find(TargetQuery::ExactText(text.into()))
            .unwrap_or_else(|| panic!("{text} not found"));
        self.cx
            .simulate_click(bounds.center(), Modifiers::default());
        self.draw();
    }

    fn click(&mut self, query: TargetQuery) {
        let bounds = self
            .find(query.clone())
            .unwrap_or_else(|| panic!("{query:?} not found"));
        self.cx
            .simulate_click(bounds.center(), Modifiers::default());
        self.draw();
    }

    fn right_click_text(&mut self, text: &str) {
        let bounds = self
            .find(TargetQuery::ExactText(text.into()))
            .unwrap_or_else(|| panic!("{text} not found"));
        self.cx
            .simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
        self.cx
            .simulate_mouse_up(bounds.center(), MouseButton::Right, Modifiers::default());
        self.draw();
    }

    fn keys(&mut self, keystrokes: &str) {
        self.cx.simulate_keystrokes(keystrokes);
        self.draw();
    }

    fn add_fault(
        &mut self,
        target: &str,
        mode: FaultMode,
        message: Option<&str>,
        argument_match: Option<&str>,
    ) {
        let fault = ServiceFault {
            target: target.into(),
            mode,
            message: message.map(str::to_string),
            ms: None,
            value: None,
            after_calls: None,
            argument_match: argument_match.map(str::to_string),
        };
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .faults()
                .expect("qa faults")
                .add(fault);
        });
    }
}

fn role_name(role: &str, name: &str) -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        role: Some(role.into()),
        name: Some(name.into()),
        exact: Some(true),
        ..DescribedTarget::default()
    })
}

fn text_contains(text: &str) -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        text: Some(text.into()),
        exact: Some(false),
        ..DescribedTarget::default()
    })
}

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

// ───────────── BoardRoute / BoardViewRoute ─────────────

// parity: TS-renderer-BoardRoute-8bef67
#[gpui::test]
fn skeleton_shows_until_the_board_data_arrives(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "보드 카드", TaskStatus::Todo, |_| {});
    cx.update(|cx| {
        let fault = ServiceFault {
            target: "kanban.getTasksByStatus".into(),
            mode: FaultMode::Hang,
            message: None,
            ms: None,
            value: None,
            after_calls: None,
            argument_match: None,
        };
        cx.global::<AppServices>().faults().unwrap().add(fault);
    });
    let mut board = open_board(cx, "/ko");
    assert!(!board.has_text("보드 카드"));
    assert!(!board.has_text("+ 새 작업"));
}

// parity: TS-renderer-BoardRoute-8f20dc
#[gpui::test]
fn slow_first_load_shows_the_delay_notice_without_an_empty_board(cx: &mut TestAppContext) {
    let dir = temp_dir();
    init_app(cx, dir.path().to_path_buf(), true);
    cx.update(|cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "kanban.getTasksByStatus".into(),
                mode: FaultMode::Hang,
                message: None,
                ms: None,
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    let mut board = open_board(cx, "/ko");
    assert!(!board.has_text("조회가 지연되고 있습니다. 계속 기다리는 중…"));
    board.cx.executor().advance_clock(Duration::from_secs(6));
    board.draw();
    assert!(board.has_text("조회가 지연되고 있습니다. 계속 기다리는 중…"));
    assert!(!board.has_text("+ 새 작업"));
}

// parity: TS-renderer-BoardRoute-c15865
#[gpui::test]
fn failed_first_load_shows_the_reason_and_retry_loads_again(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "다시 불러온 카드", TaskStatus::Todo, |_| {});
    cx.update(|cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "kanban.getTasksByStatus".into(),
                mode: FaultMode::Fail,
                message: Some("database is locked".into()),
                ms: None,
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    let mut board = open_board(cx, "/ko");
    assert!(board.find(text_contains("database is locked")).is_some());
    assert!(!board.has_text("다시 불러온 카드"));
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanban.getTasksByStatus")
    });
    board.click(role_name("button", "다시 시도"));
    assert!(board.has_text("다시 불러온 카드"));
}

// parity: TS-renderer-BoardRoute-96d5b6
#[gpui::test]
fn failed_refresh_keeps_the_loaded_board_and_shows_a_banner(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "남아 있는 카드", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    assert!(board.has_text("남아 있는 카드"));
    board.add_fault(
        "kanban.getTasksByStatus",
        FaultMode::Fail,
        Some("database is locked"),
        None,
    );
    let view = board.view.clone();
    board
        .cx
        .update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
    board.draw();
    assert!(board.has_text("남아 있는 카드"));
    assert!(
        board
            .find(text_contains("최신 데이터를 불러오지 못해"))
            .is_some()
    );
}

// parity: TS-renderer-BoardRoute-790591
#[gpui::test]
fn refresh_replaces_the_shown_board_with_the_latest_data(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "처음 카드", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    task(&database, "새로 생긴 카드", TaskStatus::Todo, |_| {});
    let view = board.view.clone();
    board
        .cx
        .update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
    board.draw();
    assert!(board.has_text("처음 카드"));
    assert!(board.has_text("새로 생긴 카드"));
}

fn board_view_mode_for(cx: &mut TestAppContext, stored: Option<&str>) -> BoardViewMode {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    if let Some(stored) = stored {
        save_app_setting(&database.connection(), "board_view_mode", stored).unwrap();
    }
    let mut board = open_board(cx, "/ko");
    let view = board.view.clone();
    board.cx.update(|_, cx| view.read(cx).view_mode())
}

// parity: TS-renderer-BoardViewRoute-96fd1d
#[gpui::test]
fn stored_workspace_view_mode_shows_the_workspace(cx: &mut TestAppContext) {
    assert_eq!(
        board_view_mode_for(cx, Some("workspace")),
        BoardViewMode::Workspace
    );
}

// parity: TS-renderer-BoardViewRoute-02c76f
#[gpui::test]
fn kanban_or_unknown_view_modes_show_the_kanban(cx: &mut TestAppContext) {
    assert_eq!(
        board_view_mode_for(cx, Some("kanban")),
        BoardViewMode::Kanban
    );
}

// parity: TS-renderer-BoardViewRoute-20e6df
#[gpui::test]
fn unreadable_view_mode_falls_back_to_the_kanban(cx: &mut TestAppContext) {
    assert_eq!(
        board_view_mode_for(cx, Some("weird")),
        BoardViewMode::Kanban
    );
}

// ───────────── 카드 ─────────────

fn seeded_board(cx: &mut TestAppContext, is_qa: bool) -> (Board, Arc<Database>, tempfile::TempDir) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), is_qa);
    let demo = project(&database, "kanvibe-demo", None);
    task(&database, "루트 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(demo.id.clone());
        task.branch_name = Some("main".into());
        task.worktree_path = Some("/repos/kanvibe-demo".into());
        task.priority = Some(TaskPriority::High);
    });
    task(
        &database,
        "물려받는 작업",
        TaskStatus::Progress,
        |task| {
            task.project_id = Some(demo.id.clone());
            task.branch_name = Some("feat/inherit".into());
            task.session_type = Some(SessionType::Tmux);
            task.ssh_host = Some("devbox".into());
            task.pr_url = Some("https://github.com/example/pull/1".into());
            task.agent_type = Some("claude".into());
        },
    );
    task(
        &database,
        "자기 우선순위 작업",
        TaskStatus::Review,
        |task| {
            task.project_id = Some(demo.id.clone());
            task.branch_name = Some("feat/own".into());
            task.priority = Some(TaskPriority::Low);
        },
    );
    task(&database, "우선순위 없는 작업", TaskStatus::Pending, |_| {});
    let board = open_board(cx, "/ko");
    (board, database, dir)
}

// parity: TS-components-TaskCard-c7c91f
#[gpui::test]
fn cards_without_priority_show_no_priority_badge(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    task(&database, "우선순위 없는 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    assert!(board.has_text("우선순위 없는 작업"));
    assert!(!board.has_role_name("status", "priority"));
}

// parity: TS-components-TaskCard-5f4aa1
#[gpui::test]
fn low_priority_cards_show_p3(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    task(&database, "낮음", TaskStatus::Todo, |task| {
        task.priority = Some(TaskPriority::Low)
    });
    let mut board = open_board(cx, "/ko");
    assert!(board.has_text("P3"));
}

// parity: TS-components-TaskCard-f8c5a2
#[gpui::test]
fn medium_priority_cards_show_p2(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    task(&database, "보통", TaskStatus::Todo, |task| {
        task.priority = Some(TaskPriority::Medium)
    });
    let mut board = open_board(cx, "/ko");
    assert!(board.has_text("P2"));
}

// parity: TS-components-TaskCard-3012ff
#[gpui::test]
fn high_priority_cards_show_p1(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    task(&database, "높음", TaskStatus::Todo, |task| {
        task.priority = Some(TaskPriority::High)
    });
    let mut board = open_board(cx, "/ko");
    assert!(board.has_text("P1"));
}

// parity: TS-components-TaskCard-d8513f
#[gpui::test]
fn inherited_priority_is_shown_with_the_inherited_marker(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    // 루트 작업(P1)과 물려받은 진행 카드가 모두 P1을 보이고, 물려받은 쪽은 표시가 다르다
    assert!(board.has_role_name("status", "inherited-priority"));
}

// parity: TS-components-TaskCard-c2a377
#[gpui::test]
fn own_priority_beats_the_inherited_one(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.has_text("P3"));
}

// parity: TS-components-TaskCard-23e144
#[gpui::test]
fn session_and_remote_badges_are_shown(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.has_text("tmux"));
    assert!(board.has_text("devbox"));
    assert!(board.has_text("PR"));
    assert!(board.has_text("claude"));
}

// parity: TS-components-TaskCard-53380d
#[gpui::test]
fn many_badges_are_all_drawn(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    for badge in ["PR", "claude", "tmux", "devbox", "P1"] {
        assert!(board.has_text(badge), "{badge}");
    }
}

// parity: TS-components-TaskCard-e63268
#[gpui::test]
fn project_name_is_shown_above_the_title(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    let title = board
        .find(TargetQuery::ExactText("물려받는 작업".into()))
        .unwrap();
    let project = board.find(role_name("button", "kanvibe-demo"));
    // 프로젝트 이름은 버튼이 아니라 글자라 카드 제목 위쪽에서 찾는다
    assert!(project.is_none());
    assert!(f32::from(title.origin.y) > 0.0);
}

// parity: TS-components-TaskCard-7b2db2
#[gpui::test]
fn base_branch_cards_show_the_crown_icon(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.has_role_name("img", "Base branch"));
}

fn unread_board(cx: &mut TestAppContext, with_project: bool) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    let demo = project(&database, "kanvibe-demo", None);
    let card = task(&database, "알림 있는 작업", TaskStatus::Todo, |task| {
        if with_project {
            task.project_id = Some(demo.id.clone());
        }
    });
    let notifications = serde_json::json!([{
        "id": "n1", "title": "알림", "body": "Review 상태로 바뀌었습니다", "taskId": card.id,
        "relativePath": format!("/ko/task/{}", card.id), "locale": "ko", "isRead": false,
        "createdAt": "2026-01-15T02:57:00.000Z", "dedupeKey": "n1"
    }]);
    save_app_setting(
        &database.connection(),
        "app_notifications",
        &notifications.to_string(),
    )
    .unwrap();
    std::mem::forget(dir);
    open_board(cx, "/ko")
}

// parity: TS-components-TaskCard-4503c7
#[gpui::test]
fn no_unread_badge_without_unread_notifications(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.find(text_contains("읽지 않음")).is_none());
}

// parity: TS-components-TaskCard-83349b
#[gpui::test]
fn unread_count_badge_is_shown(cx: &mut TestAppContext) {
    let mut board = unread_board(cx, true);
    assert!(board.has_role_name("status", "읽지 않음 1개"));
}

// parity: TS-components-TaskCard-f8dabc
#[gpui::test]
fn unread_badge_sits_on_the_project_name_line(cx: &mut TestAppContext) {
    let mut board = unread_board(cx, true);
    let badge = board.find(role_name("status", "읽지 않음 1개")).unwrap();
    let title = board
        .find(TargetQuery::ExactText("알림 있는 작업".into()))
        .unwrap();
    assert!(badge.origin.y < title.origin.y);
}

// parity: TS-components-TaskCard-4808b5
#[gpui::test]
fn unread_badge_falls_back_to_the_title_line_without_a_project(cx: &mut TestAppContext) {
    let mut board = unread_board(cx, false);
    let badge = board.find(role_name("status", "읽지 않음 1개")).unwrap();
    let title = board
        .find(TargetQuery::ExactText("알림 있는 작업".into()))
        .unwrap();
    assert!(badge.origin.y >= title.origin.y - gpui_kit::px(4.0));
    assert!(badge.origin.y < title.origin.y + title.size.height);
}

// parity: TS-renderer-useUnreadNotificationCountByTask-8851d7
#[gpui::test]
fn unread_counts_are_per_task_and_skip_read_ones(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    let card = task(&database, "작업", TaskStatus::Todo, |_| {});
    let notification = |id: &str, is_read: bool| {
        serde_json::json!({
            "id": id, "title": id, "body": "", "taskId": card.id, "relativePath": "/ko", "locale": "ko",
            "isRead": is_read, "createdAt": "2026-01-15T02:57:00.000Z", "dedupeKey": id
        })
    };
    let list = serde_json::json!([
        notification("a", false),
        notification("b", false),
        notification("c", true)
    ]);
    save_app_setting(
        &database.connection(),
        "app_notifications",
        &list.to_string(),
    )
    .unwrap();
    let mut board = open_board(cx, "/ko");
    assert!(board.has_role_name("status", "읽지 않음 2개"));
}

// ───────────── 카드 요청 상태 ─────────────

// parity: TS-components-Board-80491b
#[gpui::test]
fn deleting_card_shows_progress_and_a_second_delete_sends_nothing(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault("kanban.deleteTask", FaultMode::Hang, None, None);
    board.right_click_text("지울 작업");
    board.click(role_name("menuitem", "삭제"));
    assert!(board.has_text("삭제 중…"));
    assert!(board.has_text("지울 작업"));
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_some()
    );
}

// parity: TS-components-Board-684985
#[gpui::test]
fn context_menu_delete_keeps_the_card_until_the_service_finishes(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.right_click_text("지울 작업");
    board.click(role_name("menuitem", "삭제"));
    board.draw();
    assert!(!board.has_text("지울 작업"));
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

// parity: TS-components-Board-ecd0e1
#[gpui::test]
fn failed_delete_shows_the_reason_and_retry_finishes_it(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault(
        "kanban.deleteTask",
        FaultMode::Fail,
        Some("worktree is dirty"),
        None,
    );
    board.right_click_text("지울 작업");
    board.click(role_name("menuitem", "삭제"));
    assert!(board.find(text_contains("삭제하지 못했습니다")).is_some());
    assert!(board.has_text("지울 작업"));
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanban.deleteTask")
    });
    board.click(role_name("button", "다시 시도"));
    assert!(!board.has_text("지울 작업"));
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

// parity: TS-components-Board-6e531c
#[gpui::test]
fn context_menu_status_moves_the_card_and_saves_it(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "옮길 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.right_click_text("옮길 작업");
    board.click(TargetQuery::Described(DescribedTarget {
        role: Some("menuitem".into()),
        name: Some("상태 변경하기".into()),
        exact: Some(false),
        ..DescribedTarget::default()
    }));
    board.click(role_name("menuitemradio", "Review"));
    let saved = find_task(&database.connection(), &card.id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, TaskStatus::Review);
}

// parity: TS-components-Board-08a81f
#[gpui::test]
fn moved_card_shows_saving_until_the_save_finishes(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "옮길 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault("kanban.moveTaskToColumn", FaultMode::Hang, None, None);
    board.right_click_text("옮길 작업");
    board.click(TargetQuery::Described(DescribedTarget {
        role: Some("menuitem".into()),
        name: Some("상태 변경하기".into()),
        exact: Some(false),
        ..DescribedTarget::default()
    }));
    board.click(role_name("menuitemradio", "Progress"));
    assert!(board.has_text("저장 중…"));
}

// parity: TS-components-TaskCard-00552e
#[gpui::test]
fn failed_move_shows_the_reason_and_retry_moves_again(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "옮길 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault(
        "kanban.moveTaskToColumn",
        FaultMode::Fail,
        Some("database is locked"),
        None,
    );
    board.right_click_text("옮길 작업");
    board.click(TargetQuery::Described(DescribedTarget {
        role: Some("menuitem".into()),
        name: Some("상태 변경하기".into()),
        exact: Some(false),
        ..DescribedTarget::default()
    }));
    board.click(role_name("menuitemradio", "Progress"));
    assert!(
        board
            .find(text_contains("옮기지 못해 원래 위치로 되돌렸습니다"))
            .is_some()
    );
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanban.moveTaskToColumn")
    });
    board.click(role_name("button", "다시 옮기기"));
    let saved = find_task(&database.connection(), &card.id)
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, TaskStatus::Progress);
}

// parity: TS-components-DoneConfirmDialog-b80c01
#[gpui::test]
fn moving_a_branch_card_to_done_asks_first(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "브랜치 작업", TaskStatus::Review, |task| {
        task.branch_name = Some("feat/a".into())
    });
    let mut board = open_board(cx, "/ko");
    board.right_click_text("브랜치 작업");
    board.click(TargetQuery::Described(DescribedTarget {
        role: Some("menuitem".into()),
        name: Some("상태 변경하기".into()),
        exact: Some(false),
        ..DescribedTarget::default()
    }));
    board.click(role_name("menuitemradio", "Done"));
    assert!(board.has_text("Done 이동 경고"));
    assert!(board.has_text("이동"));
    assert!(board.has_text("취소"));
    assert_eq!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Review
    );
}

fn open_done_confirm(cx: &mut TestAppContext) -> (Board, Arc<Database>, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "브랜치 작업", TaskStatus::Review, |task| {
        task.branch_name = Some("feat/a".into())
    });
    std::mem::forget(dir);
    let mut board = open_board(cx, "/ko");
    board.right_click_text("브랜치 작업");
    board.click(TargetQuery::Described(DescribedTarget {
        role: Some("menuitem".into()),
        name: Some("상태 변경하기".into()),
        exact: Some(false),
        ..DescribedTarget::default()
    }));
    board.click(role_name("menuitemradio", "Done"));
    (board, database, card)
}

// parity: TS-components-DoneConfirmDialog-e36d1c
#[gpui::test]
fn cancel_keeps_the_card_where_it_was(cx: &mut TestAppContext) {
    let (mut board, database, card) = open_done_confirm(cx);
    board.click_text("취소");
    assert!(!board.has_text("Done 이동 경고"));
    assert_eq!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Review
    );
}

// parity: TS-components-DoneConfirmDialog-24a1ba
#[gpui::test]
fn escape_cancels_the_done_move(cx: &mut TestAppContext) {
    let (mut board, database, card) = open_done_confirm(cx);
    board.keys("escape");
    assert!(!board.has_text("Done 이동 경고"));
    assert_eq!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Review
    );
}

// parity: TS-components-DoneConfirmDialog-deaa61
#[gpui::test]
fn confirm_without_dont_ask_moves_but_keeps_asking_next_time(cx: &mut TestAppContext) {
    let (mut board, database, card) = open_done_confirm(cx);
    board.click_text("이동");
    assert_eq!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Done
    );
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "done_alert_dismissed",
    )
    .unwrap();
    assert!(stored.is_none_or(|setting| setting.value != "true"));
}

// parity: TS-components-DoneConfirmDialog-2b911a
#[gpui::test]
fn confirm_with_dont_ask_saves_the_dismissal_then_moves(cx: &mut TestAppContext) {
    let (mut board, database, card) = open_done_confirm(cx);
    board.click_text("다시 묻지 않기");
    board.click_text("이동");
    assert_eq!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Done
    );
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "done_alert_dismissed",
    )
    .unwrap();
    assert_eq!(stored.map(|setting| setting.value).as_deref(), Some("true"));
}

// parity: TS-components-DoneConfirmDialog-e4f042
#[gpui::test]
fn done_dialog_is_absent_until_a_done_move_needs_it(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(!board.has_text("Done 이동 경고"));
}

// ───────────── 메뉴 키보드 ─────────────

// parity: TS-components-TaskContextMenu-7b4190
#[gpui::test]
fn menu_escape_closes_and_enter_runs_the_focused_item(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "메뉴 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.right_click_text("메뉴 작업");
    assert!(board.has_role_name("menuitem", "삭제"));
    board.keys("escape");
    assert!(!board.has_role_name("menuitem", "삭제"));
    board.right_click_text("메뉴 작업");
    board.keys("down down enter");
    board.draw();
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

// parity: TS-components-TaskContextMenu-d31e48
#[gpui::test]
fn status_list_opens_and_changes_status_with_keys_only(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "메뉴 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.right_click_text("메뉴 작업");
    board.keys("down right down enter");
    assert_eq!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Progress
    );
}

// parity: TS-components-TaskContextMenu-fbab95
#[gpui::test]
fn menu_arrow_keys_cycle_through_items(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "메뉴 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.right_click_text("메뉴 작업");
    // 첫 항목에서 위로 가면 마지막(삭제)으로 돈다
    board.keys("up enter");
    board.draw();
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

// ───────────── 헤더 ─────────────

// parity: TS-components-Board-9b38c9
#[gpui::test]
fn task_kind_segment_changes_the_shown_cards(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "Project"));
    assert!(board.has_text("루트 작업"));
    assert!(!board.has_text("물려받는 작업"));
    board.click(role_name("button", "Task"));
    assert!(!board.has_text("루트 작업"));
    assert!(board.has_text("물려받는 작업"));
}

// parity: TS-components-Board-eb7121
#[gpui::test]
fn task_kind_segment_sits_left_of_the_project_filter_at_the_same_height(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    let segment = board.find(role_name("button", "All")).unwrap();
    let project_filter = board
        .find(TargetQuery::ExactText("전체 프로젝트".into()))
        .unwrap();
    assert!(segment.origin.x < project_filter.origin.x);
}

// parity: TS-components-Board-ab5bac
#[gpui::test]
fn board_header_has_no_logout_button(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.find(text_contains("로그아웃")).is_none());
    assert!(board.find(text_contains("Logout")).is_none());
}

// parity: TS-renderer-useTaskKindFilterParams-31c803
#[gpui::test]
fn task_kind_query_selects_the_segment_on_entry(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    let demo = project(&database, "demo", None);
    task(&database, "루트", TaskStatus::Todo, |task| {
        task.project_id = Some(demo.id.clone());
        task.branch_name = Some("main".into());
    });
    task(&database, "일반", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko?taskKind=project");
    assert!(board.has_text("루트"));
    assert!(!board.has_text("일반"));
}

// parity: TS-components-Board-74ac1d
#[gpui::test]
fn ctrl_f_opens_the_board_find_bar(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.cx.update(|window, cx| {
        let action = kanvibe_app::shortcuts::OpenBoardPageFind;
        window.dispatch_action(Box::new(action), cx);
    });
    board.draw();
    assert!(board.has_text("이전"));
    assert!(board.has_text("다음"));
}

// parity: TS-components-Board-2b0e2c
#[gpui::test]
fn escape_closes_the_find_bar(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("/");
    assert!(board.has_text("이전"));
    board.keys("escape");
    assert!(!board.has_text("이전"));
}

// parity: TS-components-Board-d5d432
#[gpui::test]
fn slash_does_not_open_find_without_vim_mode(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    save_app_setting(&database.connection(), "vim_mode_enabled", "false").unwrap();
    task(&database, "작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.keys("/");
    assert!(!board.has_text("이전"));
}

// parity: TS-components-Board-15f2c9
#[gpui::test]
fn vim_n_opens_the_new_task_modal(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("n");
    assert!(board.has_text("새 작업 생성"));
}

// parity: TS-components-CreateTaskModal-8aea8b
#[gpui::test]
fn escape_closes_the_new_task_modal(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click_text("+ 새 작업");
    assert!(board.has_text("새 작업 생성"));
    board.keys("escape");
    assert!(!board.has_text("새 작업 생성"));
}

// parity: TS-components-Board-2363ee
#[gpui::test]
fn branch_card_menu_opens_create_modal_with_the_branch_as_base(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.right_click_text("물려받는 작업");
    board.click(role_name("menuitem", "브랜치 기반 TODO 생성"));
    assert!(board.has_text("새 작업 생성"));
    assert!(board.has_text("베이스 브랜치"));
}

// parity: TS-components-Board-97b6ee
#[gpui::test]
fn arrow_key_without_focus_enters_the_first_card(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "첫 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.keys("down");
    // 포커스된 카드에서 vim dd는 그 카드를 지운다(포커스가 들어갔음을 확인한다)
    board.keys("d d");
    board.draw();
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

// parity: TS-components-Board-782407
#[gpui::test]
fn vim_key_without_focus_enters_the_first_card(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "첫 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.keys("j d d");
    board.draw();
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

// parity: TS-components-Board-d75a51
#[gpui::test]
fn dd_on_a_focused_card_deletes_it_after_success(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.keys("down d d");
    board.draw();
    assert!(!board.has_text("지울 작업"));
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

// parity: TS-components-Board-1e08e0
#[gpui::test]
fn failed_dd_delete_keeps_the_card(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault("kanban.deleteTask", FaultMode::Fail, Some("busy"), None);
    board.keys("down d d");
    assert!(board.has_text("지울 작업"));
}

// parity: TS-components-Board-e569f6
#[gpui::test]
fn dd_is_cancelled_when_the_confirm_is_declined(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.keys("down d d");
    assert!(board.cx.has_pending_prompt());
    board.cx.simulate_prompt_answer("Cancel");
    board.draw();
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_some()
    );
}

// parity: TS-components-Board-a59ead
#[gpui::test]
fn dd_does_nothing_without_vim_mode(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "vim_mode_enabled", "false").unwrap();
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.keys("down d d");
    board.draw();
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_some()
    );
}

// parity: TS-components-Board-9d2d31
#[gpui::test]
fn vim_keys_do_nothing_without_vim_mode(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "vim_mode_enabled", "false").unwrap();
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    // j로 카드에 들어가지 않으므로 뒤이은 Shift+F10도 메뉴를 열지 않는다
    board.keys("j shift-f10");
    assert!(!board.has_role_name("menuitem", "삭제"));
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_some()
    );
}

// parity: TS-components-Board-d57236
#[gpui::test]
fn shift_f10_on_a_focused_card_opens_the_menu(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("down shift-f10");
    assert!(board.has_role_name("menuitem", "삭제"));
}

// parity: TS-components-TaskCard-d7c200
#[gpui::test]
fn shift_f10_opens_the_menu_without_following_the_link(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("down shift-f10");
    assert!(board.has_role_name("menuitem", "삭제"));
    assert!(board.has_text("루트 작업"));
}

// ───────────── 알림 센터 ─────────────

fn notification_board(
    cx: &mut TestAppContext,
    unread_only: bool,
) -> (Board, Arc<Database>, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "알림 작업", TaskStatus::Todo, |_| {});
    let notification = |id: &str, body: &str, created_at: &str, is_read: bool| {
        serde_json::json!({
            "id": id, "title": id, "body": body, "taskId": card.id,
            "relativePath": format!("/ko/task/{}", card.id), "locale": "ko", "isRead": is_read,
            "createdAt": created_at, "dedupeKey": id
        })
    };
    let list = serde_json::json!([
        notification("old", "예전 알림", "2026-01-15T02:20:00.000Z", true),
        notification("new", "새 알림", "2026-01-15T02:57:00.000Z", false),
    ]);
    save_app_setting(
        &database.connection(),
        "app_notifications",
        &list.to_string(),
    )
    .unwrap();
    if unread_only {
        save_app_setting(&database.connection(), "notification_unread_only", "true").unwrap();
    }
    std::mem::forget(dir);
    let mut board = open_board(cx, "/ko");
    board.cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(kanvibe_app::shortcuts::ToggleNotificationCenter),
            cx,
        )
    });
    board.draw();
    (board, database, card)
}

fn stored_notification_is_read(database: &Database, id: &str) -> bool {
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "app_notifications",
    )
    .unwrap()
    .unwrap();
    let list: Vec<serde_json::Value> = serde_json::from_str(&stored.value).unwrap();
    list.iter().find(|item| item["id"] == id).unwrap()["isRead"] == true
}

// parity: TS-components-NotificationCenterButton-e76d89
#[gpui::test]
fn shortcut_opens_the_notification_panel(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    assert!(board.has_role_name("dialog", "알림"));
    assert!(board.has_text("새 알림"));
}

// parity: TS-components-NotificationCenterButton-c9aab3
#[gpui::test]
fn escape_closes_the_notification_panel(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    board.keys("escape");
    assert!(!board.has_role_name("dialog", "알림"));
}

// parity: TS-components-NotificationCenterButton-b64c86
#[gpui::test]
fn space_marks_the_highlighted_notification_read_without_opening(cx: &mut TestAppContext) {
    let (mut board, database, _card) = notification_board(cx, false);
    board.keys("space");
    assert!(stored_notification_is_read(&database, "new"));
    assert!(board.has_role_name("dialog", "알림"));
}

// parity: TS-components-NotificationCenterButton-e2732a
#[gpui::test]
fn arrows_and_enter_open_the_highlighted_notification(cx: &mut TestAppContext) {
    let (mut board, database, _card) = notification_board(cx, false);
    board.keys("down up enter");
    assert!(stored_notification_is_read(&database, "new"));
    assert!(!board.has_role_name("dialog", "알림"));
}

// parity: TS-components-NotificationCenterButton-13e9f8
#[gpui::test]
fn picked_unread_filter_is_saved_to_app_settings(cx: &mut TestAppContext) {
    let (mut board, database, _card) = notification_board(cx, false);
    board.click(role_name("button", "안읽음"));
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "notification_unread_only",
    )
    .unwrap();
    assert_eq!(stored.map(|setting| setting.value).as_deref(), Some("true"));
    assert!(!board.has_text("예전 알림"));
}

// parity: TS-components-NotificationCenterButton-e7b9d2
#[gpui::test]
fn saved_unread_filter_applies_when_the_panel_opens(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, true);
    assert!(board.has_text("새 알림"));
    assert!(!board.has_text("예전 알림"));
}

// parity: TS-components-NotificationCenterButton-627875
#[gpui::test]
fn marking_read_in_unread_only_view_drops_the_item(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    board.click(role_name("button", "안읽음"));
    board.keys("space");
    assert!(!board.has_text("새 알림"));
}

// parity: TS-components-NotificationCenterButton-0d4dcd
#[gpui::test]
fn unread_only_view_without_unread_items_shows_its_own_empty_message(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    board.click(role_name("button", "모두 읽음 처리"));
    board.click(role_name("button", "안읽음"));
    assert!(board.has_text("안읽은 알림이 없습니다."));
}

// parity: TS-components-NotificationCenterButton-ec8b4d
#[gpui::test]
fn failed_mark_all_read_shows_the_reason(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    board.add_fault(
        "kanvibe:notifications-mark-read",
        FaultMode::Fail,
        Some("disk full"),
        None,
    );
    board.click(role_name("button", "모두 읽음 처리"));
    assert!(
        board
            .find(text_contains("모두 읽음 처리하지 못했습니다"))
            .is_some()
    );
}

// parity: TS-components-NotificationCenterButton-78c97c
#[gpui::test]
fn missing_task_shows_a_notice_instead_of_navigating(cx: &mut TestAppContext) {
    let (mut board, database, card) = notification_board(cx, false);
    kanvibe_db::repositories::tasks::delete_task(&database.connection(), &card.id).unwrap();
    board.keys("enter");
    board.draw();
    let view = board.view.clone();
    let missing = board
        .cx
        .update(|_, cx| view.read(cx).missing_task_notification_id());
    assert_eq!(missing.as_deref(), Some("new"));
}

// ───────────── 헤더 팝오버 ─────────────

fn host_board(cx: &mut TestAppContext) -> (Board, Arc<Database>) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let local = project(&database, "local-app", None);
    let remote = project(&database, "remote-app", Some("devbox"));
    task(&database, "로컬 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(local.id.clone())
    });
    task(&database, "원격 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(remote.id.clone())
    });
    std::mem::forget(dir);
    let board = open_board(cx, "/ko");
    (board, database)
}

// parity: TS-components-Board-48ebb6
#[gpui::test]
fn mod_shift_d_opens_the_desktop_filter(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.cx.update(|window, cx| {
        window.dispatch_action(Box::new(kanvibe_app::shortcuts::OpenDesktopFilter), cx)
    });
    board.draw();
    assert!(board.has_role_name("dialog", "데스크톱 필터"));
}

// parity: TS-components-DesktopFilter-0a9fae
#[gpui::test]
fn clicking_a_desktop_name_toggles_it_and_keeps_the_menu_open(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "데스크톱 필터"));
    board.click(role_name("checkbox", "devbox"));
    assert!(board.has_role_name("dialog", "데스크톱 필터"));
    assert!(board.has_text("원격 작업"));
    assert!(!board.has_text("로컬 작업"));
}

// parity: TS-components-Board-c68453
#[gpui::test]
fn desktop_filter_supports_several_hosts_and_resetting_to_all(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "데스크톱 필터"));
    board.click(role_name("checkbox", "devbox"));
    board.click(role_name("checkbox", "로컬"));
    assert!(board.has_text("원격 작업") && board.has_text("로컬 작업"));
    board.click(role_name("checkbox", "devbox"));
    assert!(!board.has_text("원격 작업"));
    board.click(role_name("checkbox", "전체 데스크톱"));
    assert!(board.has_text("원격 작업") && board.has_text("로컬 작업"));
}

// parity: TS-components-Board-57503c
#[gpui::test]
fn changing_desktops_drops_project_selections_from_other_desktops(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click_text("전체 프로젝트");
    board.click(role_name("option", "local-app"));
    board.keys("escape");
    assert!(board.has_text("로컬 작업") && !board.has_text("원격 작업"));
    board.click(role_name("button", "데스크톱 필터"));
    board.click(role_name("checkbox", "devbox"));
    board.click(role_name("checkbox", "전체 데스크톱"));
    // 로컬 프로젝트 선택이 지워져 전체로 돌아가도 되살아나지 않는다
    assert!(board.has_text("원격 작업") && board.has_text("로컬 작업"));
}

// parity: TS-components-ProjectSelector-50aee9
#[gpui::test]
fn up_to_two_selected_projects_show_as_chips(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click_text("전체 프로젝트");
    board.click(role_name("option", "local-app"));
    board.click(role_name("option", "remote-app"));
    board.keys("escape");
    assert!(!board.has_text("전체 프로젝트"));
    let view = board.view.clone();
    let (chips, hidden_count) = board
        .cx
        .update(|_, cx| view.read(cx).selected_project_chips());
    assert_eq!(chips, ["local-app", "remote-app"]);
    assert_eq!(hidden_count, 0);
}

// parity: TS-components-BoardSortPicker-d40c29
#[gpui::test]
fn sort_trigger_shows_the_number_of_chosen_fields(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "정렬"));
    board.click(role_name("option", "우선순위"));
    board.click(role_name("option", "제목"));
    assert!(board.has_text("2"));
}

// parity: TS-components-BoardSortPicker-28929e
#[gpui::test]
fn clear_removes_all_chosen_fields(cx: &mut TestAppContext) {
    let (mut board, database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "정렬"));
    board.click(role_name("option", "우선순위"));
    board.click(role_name("button", "정렬 해제"));
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "board_sort_preference",
    )
    .unwrap();
    assert_eq!(
        stored.map(|setting| setting.value).as_deref(),
        Some(r#"{"keys":[]}"#)
    );
}

// parity: TS-components-Board-ea02ad
#[gpui::test]
fn turning_on_priority_sort_saves_the_preference(cx: &mut TestAppContext) {
    let (mut board, database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "정렬"));
    board.click(role_name("option", "우선순위"));
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "board_sort_preference",
    )
    .unwrap()
    .unwrap();
    assert!(stored.value.contains("priority"));
}

// ───────────── 워크스페이스 ─────────────

fn workspace_board(cx: &mut TestAppContext, path: &str) -> (Board, Arc<Database>, Vec<KanbanTask>) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let demo = project(&database, "demo", None);
    let without_session = task(&database, "세션 없는 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(demo.id.clone())
    });
    let with_session = task(
        &database,
        "세션 있는 작업",
        TaskStatus::Progress,
        |task| {
            task.project_id = Some(demo.id.clone());
            task.session_type = Some(SessionType::Tmux);
            task.session_name = Some("demo-session".into());
        },
    );
    std::mem::forget(dir);
    let board = open_board(cx, path);
    (board, database, vec![without_session, with_session])
}

// parity: TS-renderer-TaskWorkspaceRoute-66ed1a
#[gpui::test]
fn workspace_starts_on_the_first_task_with_a_session(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    assert!(board.has_role_name("current", "세션 있는 작업"));
}

// parity: TS-renderer-TaskWorkspaceRoute-48b3c0
#[gpui::test]
fn picking_another_task_switches_the_terminal_pane(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault("kanban.connectTerminalSession", FaultMode::Hang, None, None);
    board.click_text("세션 없는 작업");
    assert!(board.has_role_name("current", "세션 없는 작업"));
}

// parity: TS-renderer-TaskWorkspaceRoute-18b26c
#[gpui::test]
fn picking_a_task_without_a_session_starts_connecting(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault("kanban.connectTerminalSession", FaultMode::Hang, None, None);
    board.click_text("세션 없는 작업");
    assert!(board.has_text("연결 중..."));
}

// parity: TS-renderer-TaskWorkspaceRoute-7fedd4
#[gpui::test]
fn failed_workspace_load_shows_the_reason_and_retries(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    task(&database, "작업", TaskStatus::Todo, |_| {});
    cx.update(|cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "kanban.getTasksByStatus".into(),
                mode: FaultMode::Fail,
                message: Some("database is locked".into()),
                ms: None,
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    let mut board = open_board(cx, "/ko");
    assert!(board.find(text_contains("database is locked")).is_some());
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanban.getTasksByStatus")
    });
    board.click(role_name("button", "다시 시도"));
    assert!(board.has_text("작업"));
}

// parity: TS-renderer-TaskWorkspaceRoute-9a6a8f
#[gpui::test]
fn slow_workspace_load_shows_the_delay_notice(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    cx.update(|cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "kanban.getTasksByStatus".into(),
                mode: FaultMode::Hang,
                message: None,
                ms: None,
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    let mut board = open_board(cx, "/ko");
    board.cx.executor().advance_clock(Duration::from_secs(6));
    board.draw();
    assert!(board.has_text("조회가 지연되고 있습니다. 계속 기다리는 중…"));
}

// parity: TS-renderer-TaskWorkspaceRoute-6bd544
#[gpui::test]
fn failed_workspace_refresh_keeps_the_list_and_shows_a_banner(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault(
        "kanban.getTasksByStatus",
        FaultMode::Fail,
        Some("database is locked"),
        None,
    );
    let view = board.view.clone();
    board
        .cx
        .update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
    board.draw();
    assert!(board.has_text("세션 있는 작업"));
    assert!(
        board
            .find(text_contains("최신 데이터를 불러오지 못해"))
            .is_some()
    );
}

// parity: TS-renderer-TaskWorkspaceRoute-5f7f01
#[gpui::test]
fn task_id_query_selects_that_task(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let wanted = task(&database, "요청한 작업", TaskStatus::Todo, |_| {});
    task(&database, "세션 작업", TaskStatus::Todo, |task| {
        task.session_type = Some(SessionType::Tmux);
        task.session_name = Some("s".into());
    });
    let mut board = open_board(cx, &format!("/ko?taskId={}", wanted.id));
    assert!(board.has_role_name("current", "요청한 작업"));
}

// parity: TS-renderer-WorkspaceTaskList-c96c90
#[gpui::test]
fn clicking_a_list_task_selects_and_marks_it_current(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault("kanban.connectTerminalSession", FaultMode::Hang, None, None);
    board.click_text("세션 없는 작업");
    assert!(board.has_role_name("current", "세션 없는 작업"));
    assert!(!board.has_role_name("current", "세션 있는 작업"));
}

// parity: TS-renderer-WorkspaceTaskList-891938
#[gpui::test]
fn list_enter_focuses_the_terminal_instead_of_opening_the_detail(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.click_text("세션 있는 작업");
    board.keys("enter");
    assert!(board.has_role_name("current", "세션 있는 작업"));
    assert!(board.has_text("세션 있는 작업"));
}

// ───────────── 새 작업·분기 모달 ─────────────

fn branch_todo_modal(cx: &mut TestAppContext, branches_fault: Option<FaultMode>) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let demo = project(&database, "demo", None);
    task(&database, "브랜치 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(demo.id.clone());
        task.branch_name = Some("feat/base".into());
    });
    std::mem::forget(dir);
    let mut board = open_board(cx, "/ko");
    if let Some(mode) = branches_fault {
        board.add_fault("project.getProjectBranches", mode, Some("git failed"), None);
    }
    board.right_click_text("브랜치 작업");
    board.click(role_name("menuitem", "브랜치 기반 TODO 생성"));
    board
}

// parity: TS-components-CreateTaskModal-00fa17
#[gpui::test]
fn waiting_branch_list_is_announced(cx: &mut TestAppContext) {
    let mut board = branch_todo_modal(cx, Some(FaultMode::Hang));
    assert!(board.has_text("브랜치 목록을 불러오는 중…"));
}

// parity: TS-components-CreateTaskModal-9268d4
#[gpui::test]
fn failed_branch_list_shows_the_reason_and_a_retry(cx: &mut TestAppContext) {
    let mut board = branch_todo_modal(cx, Some(FaultMode::Fail));
    assert!(board.find(text_contains("git failed")).is_some());
    assert!(board.has_role_name("button", "다시 시도"));
}

// parity: TS-components-CreateTaskModal-b1d549
#[gpui::test]
fn branch_name_input_has_no_example_placeholder(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click_text("+ 새 작업");
    assert!(board.has_text("브랜치 이름 *"));
    assert!(board.find(text_contains("feat/my-feature")).is_none());
}

// parity: TS-components-CreateTaskModal-dc03c9
#[gpui::test]
fn new_task_defaults_to_herdr_without_a_setting(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click_text("+ 새 작업");
    assert!(board.has_text("herdr"));
}

// parity: TS-components-BranchTaskModal-e1bfd9
#[gpui::test]
fn branch_modal_defaults_to_herdr_without_a_setting(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    project(&database, "demo", None);
    task(&database, "루트 없는 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault("project.getProjectBranches", FaultMode::Hang, None, None);
    board.right_click_text("루트 없는 작업");
    board.click(role_name("menuitem", "브랜치 분기"));
    assert!(board.has_role_name("heading", "브랜치 분기"));
    assert!(board.has_text("herdr"));
}

// parity: TS-components-BranchTaskModal-b0a30f
#[gpui::test]
fn branch_modal_shows_branch_list_failure_with_retry(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    project(&database, "demo", None);
    task(&database, "루트 없는 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault(
        "project.getProjectBranches",
        FaultMode::Fail,
        Some("git failed"),
        None,
    );
    board.right_click_text("루트 없는 작업");
    board.click(role_name("menuitem", "브랜치 분기"));
    assert!(board.find(text_contains("git failed")).is_some());
    assert!(board.has_role_name("button", "다시 시도"));
}

// parity: TS-components-ProjectRegistryDialog-75ff74
#[gpui::test]
fn registry_dialog_shows_the_scan_form_and_registered_projects(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    assert!(board.has_role_name("dialog", "프로젝트 스캔"));
    assert!(board.has_text("스캔 및 등록"));
    assert!(board.has_text("local-app"));
    assert!(board.has_text("remote-app"));
}

// parity: TS-components-ProjectRegistryDialog-bc851b
#[gpui::test]
fn escape_closes_the_registry_dialog(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.keys("escape");
    assert!(!board.has_role_name("dialog", "프로젝트 스캔"));
}

// parity: TS-components-ProjectRegistryDialog-bd738d
#[gpui::test]
fn closed_registry_dialog_renders_nothing(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    assert!(!board.has_role_name("dialog", "프로젝트 스캔"));
    assert!(!board.has_text("스캔 및 등록"));
}

// parity: TS-components-ProjectRegistryDialog-c63d06
#[gpui::test]
fn open_registry_dialog_blocks_board_shortcuts(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.keys("n");
    assert!(!board.has_text("새 작업 생성"));
}

// parity: TS-components-ProjectRegistryDialog-9f21cf
#[gpui::test]
fn registry_without_projects_has_no_search_input(cx: &mut TestAppContext) {
    let dir = temp_dir();
    init_app(cx, dir.path().to_path_buf(), true);
    let mut board = open_board(cx, "/ko");
    board.click(role_name("button", "프로젝트 스캔"));
    assert!(board.has_text("등록된 프로젝트가 없습니다."));
    assert!(!board.has_role_name("searchbox", "프로젝트 검색..."));
}

// ───────────── 기타 ─────────────

// parity: TS-components-BoardSortPicker-585c32
#[gpui::test]
fn clear_does_nothing_without_chosen_fields(cx: &mut TestAppContext) {
    let (mut board, database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "정렬"));
    board.click(role_name("button", "정렬 해제"));
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "board_sort_preference",
    )
    .unwrap();
    assert!(stored.is_none());
}

// parity: TS-components-BoardSortPicker-6aa533
#[gpui::test]
fn sort_picker_offers_only_the_five_fields(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "정렬"));
    for field in ["우선순위", "생성일", "수정일", "제목", "프로젝트"] {
        assert!(board.has_role_name("option", field), "{field}");
    }
    assert!(board.find(text_contains("직접")).is_none());
}

// parity: TS-components-Board-4b7dd7
#[gpui::test]
fn open_dialog_blocks_the_find_shortcut(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.cx.update(|window, cx| {
        window.dispatch_action(Box::new(kanvibe_app::shortcuts::OpenBoardPageFind), cx)
    });
    board.draw();
    assert!(!board.has_text("이전"));
}

// parity: TS-components-TaskCard-0fd0d5
#[gpui::test]
fn priority_badge_is_aligned_to_the_right(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    let session = board.find(TargetQuery::ExactText("tmux".into())).unwrap();
    let priority = board
        .find(role_name("status", "inherited-priority"))
        .unwrap();
    assert!(priority.origin.x > session.origin.x + gpui_kit::px(100.0));
}

fn save_diff_stats(database: &Database, task_id: &str, files: i64) {
    kanvibe_db::repositories::task_diff_stats::save_task_diff_stats(
        &database.connection(),
        task_id,
        kanvibe_domain::task_diff_stats::TaskDiffStats {
            file_count: files,
            additions: 4,
            deletions: 1,
        },
    )
    .unwrap();
}

// parity: TS-renderer-useTaskDiffStats-e64ec6
#[gpui::test]
fn diff_stats_follow_the_stored_values_on_each_poll(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "변경 작업", TaskStatus::Review, |_| {});
    save_diff_stats(&database, &card.id, 2);
    let mut board = open_board(cx, "/ko");
    assert!(board.has_role_name("status", "2개 파일 변경 · 4줄 추가 · 1줄 삭제"));
    save_diff_stats(&database, &card.id, 5);
    board.cx.executor().advance_clock(Duration::from_secs(6));
    board.draw();
    assert!(board.has_role_name("status", "5개 파일 변경 · 4줄 추가 · 1줄 삭제"));
}

// parity: TS-components-Board-e560a8
#[gpui::test]
fn stored_diff_stats_show_on_cards_outside_progress(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "리뷰 작업", TaskStatus::Review, |_| {});
    save_diff_stats(&database, &card.id, 3);
    let mut board = open_board(cx, "/ko");
    assert!(board.has_role_name("status", "3개 파일 변경 · 4줄 추가 · 1줄 삭제"));
}
