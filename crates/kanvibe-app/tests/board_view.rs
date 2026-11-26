//! 보드 화면을 실제 DB와 함께 헤드리스 GPUI로 띄워, 보이는 글자·DB에 남은 값으로 TS 보드 동작을 확인한다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{Bounds, Entity, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext};
use kanvibe_app::app_window::AppWindow;
use kanvibe_app::qa::faults::{FaultMode, ServiceFault};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::qa::targets::{DescribedTarget, QaTargetRegistry, TargetQuery};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::AppServices;
use kanvibe_app::shortcuts::ShortcutState;
use kanvibe_app::views::board::{BoardView, BoardViewMode};
use kanvibe_app::views::locale_shell::LocaleShell;
use kanvibe_app::views::locale_shell::background_sync::background_sync_store;
use kanvibe_db::Database;
use kanvibe_db::repositories::app_settings::save_app_setting;
use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_db::repositories::tasks::{NewKanbanTask, find_task, insert_task};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};

struct Board {
    view: Entity<BoardView>,
    app_window: Entity<AppWindow>,
    /// 창의 전역 덮개(커맨드 팔레트·빠른 검색·릴리스·토스트). 보드는 창 루트(`AppWindow`) 아래에 뜬다
    shell: Entity<LocaleShell>,
    cx: VisualTestContext,
}

/// QA 모드로 띄우면 고장 주입을 쓸 수 있고 삭제 확인을 자동으로 받아들인다(Playwright 기본 동작과 같다)
/// QA 모드는 기본 네트워크 응답 표를 쓴다. 창을 앞으로 가져오는 시험이 릴리스 확인으로 바깥 요청을 하지 않게 한다
fn init_app(cx: &mut TestAppContext, data_dir: PathBuf, is_qa: bool) -> Arc<Database> {
    init_app_with_network(cx, data_dir, is_qa, is_qa.then_some("default"))
}

/// QA 네트워크 응답 표(`parity/fixtures/network/*.json`)를 쓰는 앱
fn init_app_with_network(
    cx: &mut TestAppContext,
    data_dir: PathBuf,
    is_qa: bool,
    network: Option<&str>,
) -> Arc<Database> {
    let network_responses_path = network.map(|name| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../parity/fixtures/network")
            .join(format!("{name}.json"))
    });
    let launch = LaunchOptions {
        data_dir: Some(data_dir),
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: is_qa.then_some(QaOptions {
            fixture_root: None,
            fixed_now_ms: 1_768_445_820_000,
            network_responses_path,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
            loading_holds: Default::default(),
        }),
    };
    cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
        cx.global_mut::<ShortcutState>().platform = ShortcutPlatform::Linux;
        if !cx.has_global::<QaTargetRegistry>() {
            cx.set_global(QaTargetRegistry::default());
        }
        // 워크스페이스 터미널이 실제 tmux 세션을 붙이지 않게 터미널 열기 기본 응답을 건다
        if let Some(faults) = cx.global::<AppServices>().faults() {
            faults.add(result_fault(
                "kanvibe:terminal-open",
                serde_json::json!({ "ok": true }),
            ));
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
    let RouteResolution::Render(_) = resolve_route_path(path) else {
        panic!("{path} must render");
    };
    let (app_window, visual) = cx.add_window_view(|window, cx| AppWindow::new(path, window, cx));
    let (view, shell) = visual.update(|_, cx| {
        let app_window = app_window.read(cx);
        let view = app_window
            .screen()
            .clone()
            .downcast::<BoardView>()
            .expect("board route renders the board");
        (view, app_window.locale_shell().clone())
    });
    let mut board = Board {
        view,
        app_window: app_window.clone(),
        shell,
        cx: visual.clone(),
    };
    board.draw();
    board
}

impl Board {
    fn shell_read<T>(&mut self, read: impl FnOnce(&LocaleShell, &gpui_kit::App) -> T) -> T {
        let shell = self.shell.clone();
        self.cx.update(|_, cx| read(shell.read(cx), cx))
    }

    /// 다른 화면으로 갔다가 보드로 돌아온다. 보드 화면은 새로 만들어진다
    fn return_to_board_via(&mut self, other_path: &str) {
        let app_window = self.app_window.clone();
        self.cx.update(|_, cx| {
            app_window.update(cx, |app_window, cx| app_window.navigate(other_path, cx))
        });
        self.draw();
        self.cx
            .update(|_, cx| app_window.update(cx, |app_window, cx| app_window.navigate("/ko", cx)));
        self.view = self.cx.update(|_, cx| {
            app_window
                .read(cx)
                .screen()
                .clone()
                .downcast::<BoardView>()
                .expect("board route renders the board")
        });
        self.draw();
    }

    /// 앱처럼 창을 앞으로 가져온다. 주기 조회는 창이 활성일 때만 돈다(TS `usePolledValue`)
    fn activate_window(&mut self) {
        self.cx.update(|window, _| window.activate_window());
        self.draw();
    }

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

    /// 포커스된 입력칸에 글자를 넣는다
    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.draw();
    }

    /// 런타임 보드 이벤트를 내고 전경 스레드가 받을 때까지 돌린다
    fn emit_board_event(&mut self, event: kanvibe_services::board_notifier::BoardEvent) {
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .runtime()
                .expect("runtime")
                .board_notifier()
                .emit(event)
        });
        self.draw();
    }

    /// 떠 있는 작업 결과 알림 글자
    fn toast_messages(&mut self) -> Vec<String> {
        self.shell_read(|shell, _| shell.toast_messages())
    }

    /// 창이 지금 보이는 경로
    fn current_path(&mut self) -> String {
        let app_window = self.app_window.clone();
        self.cx
            .update(|_, cx| app_window.read(cx).current_path().to_string())
    }

    /// 서비스가 실제로 돌지 않고 `value`를 응답으로 돌려주게 한다(`match`가 있으면 그 인자일 때만)
    fn add_result_fault(
        &mut self,
        target: &str,
        value: serde_json::Value,
        argument_match: Option<&str>,
    ) {
        let fault = ServiceFault {
            target: target.into(),
            mode: FaultMode::Result,
            message: None,
            ms: None,
            value: Some(value),
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

    fn advance(&mut self, duration: Duration) {
        self.cx.executor().advance_clock(duration);
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

#[gpui::test]
fn returning_to_the_board_draws_the_last_cards_while_the_data_reloads(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "보드 카드", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    assert!(board.has_text("보드 카드"));
    board.add_fault("kanban.getTasksByStatus", FaultMode::Hang, None, None);

    board.return_to_board_via("/ko/settings");

    assert!(board.has_text("보드 카드"));
    assert!(board.has_text("+ 새 작업"));
}

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

#[gpui::test]
fn stored_workspace_view_mode_shows_the_workspace(cx: &mut TestAppContext) {
    assert_eq!(
        board_view_mode_for(cx, Some("workspace")),
        BoardViewMode::Workspace
    );
}

#[gpui::test]
fn kanban_or_unknown_view_modes_show_the_kanban(cx: &mut TestAppContext) {
    assert_eq!(
        board_view_mode_for(cx, Some("kanban")),
        BoardViewMode::Kanban
    );
}

#[gpui::test]
fn unreadable_view_mode_falls_back_to_the_kanban(cx: &mut TestAppContext) {
    assert_eq!(
        board_view_mode_for(cx, Some("weird")),
        BoardViewMode::Kanban
    );
}

#[gpui::test]
fn the_board_reuses_the_view_mode_it_read_instead_of_reading_it_again(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let mut board = open_board(cx, "/ko");
    save_app_setting(&database.connection(), "board_view_mode", "kanban").unwrap();

    board.return_to_board_via("/ko/settings");

    let view = board.view.clone();
    assert_eq!(
        board.cx.update(|_, cx| view.read(cx).view_mode()),
        BoardViewMode::Workspace
    );
}

#[gpui::test]
fn a_hanging_view_mode_read_does_not_hold_the_board(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "보드 카드", TaskStatus::Todo, |_| {});
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    add_fault_before_open(
        cx,
        ServiceFault {
            target: "appSettings.getAppSetting".into(),
            mode: FaultMode::Hang,
            message: None,
            ms: None,
            value: None,
            after_calls: None,
            argument_match: None,
        },
    );

    let mut board = open_board(cx, "/ko");

    assert!(board.has_text("보드 카드"));
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

#[gpui::test]
fn cards_without_priority_show_no_priority_badge(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), false);
    task(&database, "우선순위 없는 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    assert!(board.has_text("우선순위 없는 작업"));
    assert!(!board.has_role_name("status", "priority"));
}

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

#[gpui::test]
fn inherited_priority_is_shown_with_the_inherited_marker(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    // 루트 작업(P1)과 물려받은 진행 카드가 모두 P1을 보이고, 물려받은 쪽은 표시가 다르다
    assert!(board.has_role_name("status", "inherited-priority"));
}

#[gpui::test]
fn own_priority_beats_the_inherited_one(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.has_text("P3"));
}

#[gpui::test]
fn session_and_remote_badges_are_shown(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.has_text("tmux"));
    assert!(board.has_text("devbox"));
    assert!(board.has_text("PR"));
    assert!(board.has_text("claude"));
}

#[gpui::test]
fn many_badges_are_all_drawn(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    for badge in ["PR", "claude", "tmux", "devbox", "P1"] {
        assert!(board.has_text(badge), "{badge}");
    }
}

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

#[gpui::test]
fn no_unread_badge_without_unread_notifications(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.find(text_contains("읽지 않음")).is_none());
}

#[gpui::test]
fn unread_count_badge_is_shown(cx: &mut TestAppContext) {
    let mut board = unread_board(cx, true);
    assert!(board.has_role_name("status", "읽지 않음 1개"));
}

#[gpui::test]
fn unread_badge_sits_on_the_project_name_line(cx: &mut TestAppContext) {
    let mut board = unread_board(cx, true);
    let badge = board.find(role_name("status", "읽지 않음 1개")).unwrap();
    let title = board
        .find(TargetQuery::ExactText("알림 있는 작업".into()))
        .unwrap();
    assert!(badge.origin.y < title.origin.y);
}

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

#[gpui::test]
fn a_hanging_dismissal_save_does_not_hold_the_done_move(cx: &mut TestAppContext) {
    let (mut board, database, card) = open_done_confirm(cx);
    board.add_fault("appSettings.dismissDoneAlert", FaultMode::Hang, None, None);
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
    assert!(stored.is_none());
}

#[gpui::test]
fn done_dialog_is_absent_until_a_done_move_needs_it(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(!board.has_text("Done 이동 경고"));
}

// ───────────── 메뉴 키보드 ─────────────

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

#[gpui::test]
fn task_kind_segment_sits_left_of_the_project_filter_at_the_same_height(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    let segment = board.find(role_name("button", "All")).unwrap();
    let project_filter = board
        .find(TargetQuery::ExactText("전체 프로젝트".into()))
        .unwrap();
    assert!(segment.origin.x < project_filter.origin.x);
}

#[gpui::test]
fn board_header_has_no_logout_button(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    assert!(board.find(text_contains("로그아웃")).is_none());
    assert!(board.find(text_contains("Logout")).is_none());
}

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

#[gpui::test]
fn escape_closes_the_find_bar(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("/");
    assert!(board.has_text("이전"));
    board.keys("escape");
    assert!(!board.has_text("이전"));
}

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

#[gpui::test]
fn vim_n_opens_the_new_task_modal(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("n");
    assert!(board.has_text("새 작업 생성"));
}

#[gpui::test]
fn escape_closes_the_new_task_modal(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click_text("+ 새 작업");
    assert!(board.has_text("새 작업 생성"));
    board.keys("escape");
    assert!(!board.has_text("새 작업 생성"));
}

#[gpui::test]
fn branch_card_menu_opens_create_modal_with_the_branch_as_base(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.right_click_text("물려받는 작업");
    board.click(role_name("menuitem", "브랜치 기반 TODO 생성"));
    assert!(board.has_text("새 작업 생성"));
    assert!(board.has_text("베이스 브랜치"));
}

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

#[gpui::test]
fn shift_f10_on_a_focused_card_opens_the_menu(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("down shift-f10");
    assert!(board.has_role_name("menuitem", "삭제"));
}

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

#[gpui::test]
fn shortcut_opens_the_notification_panel(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    assert!(board.has_role_name("dialog", "알림"));
    assert!(board.has_text("새 알림"));
}

#[gpui::test]
fn escape_closes_the_notification_panel(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    board.keys("escape");
    assert!(!board.has_role_name("dialog", "알림"));
}

#[gpui::test]
fn space_marks_the_highlighted_notification_read_without_opening(cx: &mut TestAppContext) {
    let (mut board, database, _card) = notification_board(cx, false);
    board.keys("space");
    assert!(stored_notification_is_read(&database, "new"));
    assert!(board.has_role_name("dialog", "알림"));
}

#[gpui::test]
fn arrows_and_enter_open_the_highlighted_notification(cx: &mut TestAppContext) {
    let (mut board, database, _card) = notification_board(cx, false);
    board.keys("down up enter");
    assert!(stored_notification_is_read(&database, "new"));
    assert!(!board.has_role_name("dialog", "알림"));
}

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

#[gpui::test]
fn saved_unread_filter_applies_when_the_panel_opens(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, true);
    assert!(board.has_text("새 알림"));
    assert!(!board.has_text("예전 알림"));
}

#[gpui::test]
fn a_hanging_unread_filter_save_does_not_hold_the_filter(cx: &mut TestAppContext) {
    let (mut board, database, _card) = notification_board(cx, false);
    board.add_fault(
        "appSettings.setNotificationUnreadOnlyEnabled",
        FaultMode::Hang,
        None,
        None,
    );
    board.click(role_name("button", "안읽음"));
    assert!(!board.has_text("예전 알림"));
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "notification_unread_only",
    )
    .unwrap();
    assert!(stored.is_none());
}

#[gpui::test]
fn picking_a_filter_before_the_saved_filter_arrives_keeps_the_pick(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, true);
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().expect("qa faults");
        faults.add(ServiceFault {
            target: "appSettings.getNotificationUnreadOnlyEnabled".into(),
            mode: FaultMode::Delay,
            message: None,
            ms: Some(5_000),
            value: None,
            after_calls: None,
            argument_match: None,
        });
    });
    board.return_to_board_via("/ko/settings");
    board.cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(kanvibe_app::shortcuts::ToggleNotificationCenter),
            cx,
        )
    });
    board.draw();
    assert!(board.has_text("예전 알림"));

    board.click(role_name("button", "전체"));
    board.advance(Duration::from_secs(6));

    assert!(board.has_text("예전 알림"));
}

#[gpui::test]
fn marking_read_in_unread_only_view_drops_the_item(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    board.click(role_name("button", "안읽음"));
    board.keys("space");
    assert!(!board.has_text("새 알림"));
}

#[gpui::test]
fn unread_only_view_without_unread_items_shows_its_own_empty_message(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = notification_board(cx, false);
    board.click(role_name("button", "모두 읽음 처리"));
    board.click(role_name("button", "안읽음"));
    assert!(board.has_text("안읽은 알림이 없습니다."));
}

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

#[gpui::test]
fn missing_task_shows_a_notice_instead_of_navigating(cx: &mut TestAppContext) {
    let (mut board, database, card) = notification_board(cx, false);
    kanvibe_db::repositories::tasks::delete_task(&database.connection(), &card.id).unwrap();
    board.keys("enter");
    board.draw();
    let view = board.view.clone();
    let missing = board.cx.update(|_, cx| {
        view.read(cx)
            .notification_center()
            .read(cx)
            .missing_task_notification_id()
    });
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

#[gpui::test]
fn mod_shift_d_opens_the_desktop_filter(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.cx.update(|window, cx| {
        window.dispatch_action(Box::new(kanvibe_app::shortcuts::OpenDesktopFilter), cx)
    });
    board.draw();
    assert!(board.has_role_name("dialog", "데스크톱 필터"));
}

#[gpui::test]
fn clicking_a_desktop_name_toggles_it_and_keeps_the_menu_open(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "데스크톱 필터"));
    board.click(role_name("checkbox", "devbox"));
    assert!(board.has_role_name("dialog", "데스크톱 필터"));
    assert!(board.has_text("원격 작업"));
    assert!(!board.has_text("로컬 작업"));
}

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

#[gpui::test]
fn the_project_filter_survives_leaving_and_returning_to_the_board(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click_text("전체 프로젝트");
    board.click(role_name("option", "local-app"));
    board.keys("escape");
    assert!(board.current_path().contains("projects="));

    board.return_to_board_via("/ko/settings");

    assert!(board.has_text("로컬 작업") && !board.has_text("원격 작업"));
    assert!(board.current_path().contains("projects="));
}

#[gpui::test]
fn clearing_the_project_filter_is_not_restored_on_return(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click_text("전체 프로젝트");
    board.click(role_name("option", "local-app"));
    board.click(role_name("option", "local-app"));
    board.keys("escape");

    board.return_to_board_via("/ko/settings");

    assert!(board.has_text("로컬 작업") && board.has_text("원격 작업"));
    assert!(!board.current_path().contains("projects="));
}

#[gpui::test]
fn an_unknown_project_in_the_query_is_dropped_from_the_filter(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let local = project(&database, "local-app", None);
    task(&database, "로컬 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(local.id.clone())
    });

    let mut board = open_board(cx, "/ko?projects=missing-project");

    assert!(board.has_text("로컬 작업"));
    assert!(!board.current_path().contains("projects="));
}

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

#[gpui::test]
fn sort_trigger_shows_the_number_of_chosen_fields(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "정렬"));
    board.click(role_name("option", "우선순위"));
    board.click(role_name("option", "제목"));
    assert!(board.has_text("2"));
}

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

#[gpui::test]
fn a_hanging_sort_save_does_not_hold_the_sort_change(cx: &mut TestAppContext) {
    let (mut board, database, _dir) = seeded_board(cx, true);
    board.add_fault(
        "appSettings.setBoardSortPreference",
        FaultMode::Hang,
        None,
        None,
    );
    board.click(role_name("button", "정렬"));
    board.click(role_name("option", "우선순위"));
    board.click(role_name("option", "제목"));
    assert!(board.has_text("2"));
    let stored = kanvibe_db::repositories::app_settings::find_app_setting(
        &database.connection(),
        "board_sort_preference",
    )
    .unwrap();
    assert!(stored.is_none());
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

#[gpui::test]
fn workspace_starts_on_the_first_task_with_a_session(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    assert!(board.has_role_name("current", "세션 있는 작업"));
}

#[gpui::test]
fn workspace_shows_the_shared_terminal_view_for_a_task_with_a_session(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault("kanban.connectTerminalSession", FaultMode::Hang, None, None);
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().expect("qa faults");
        faults.clear("kanvibe:terminal-open");
    });
    board.add_fault("kanvibe:terminal-open", FaultMode::Hang, None, None);

    board.click_text("세션 없는 작업");
    board.click_text("세션 있는 작업");

    assert!(board.has_role_name("status", "터미널에 연결하고 있습니다…"));
}

#[gpui::test]
fn workspace_shows_a_requested_done_task_beyond_the_first_page(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let old_done = task(&database, "오래된 완료 작업", TaskStatus::Done, |_| {});
    database
        .connection()
        .execute(
            "UPDATE kanban_tasks SET updated_at = '2020-01-01 00:00:00' WHERE id = ?1",
            [old_done.id.as_str()],
        )
        .unwrap();
    for index in 0..21 {
        task(
            &database,
            &format!("최근 완료 {index}"),
            TaskStatus::Done,
            |_| {},
        );
    }
    std::mem::forget(dir);

    let mut board = open_board(cx, &format!("/ko?taskId={}", old_done.id));

    assert!(board.has_role_name("current", "오래된 완료 작업"));
}

fn terminal_session_workspace(cx: &mut TestAppContext, list_faults: Vec<ServiceFault>) -> Board {
    open_terminal_session_workspace(cx, Vec::new(), list_faults)
}

/// `open_faults`가 있으면 `init_app`이 건 기본 터미널 연결 응답을 지우고 그것으로 바꾼다
fn open_terminal_session_workspace(
    cx: &mut TestAppContext,
    open_faults: Vec<ServiceFault>,
    list_faults: Vec<ServiceFault>,
) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let demo = project(&database, "demo", None);
    task(
        &database,
        "터미널 세션 작업",
        TaskStatus::Progress,
        |task| {
            task.project_id = Some(demo.id.clone());
            task.session_type = Some(SessionType::Terminal);
            task.session_name = Some("demo-terminal".into());
        },
    );
    std::mem::forget(dir);
    if !open_faults.is_empty() {
        cx.update(|cx| {
            let faults = cx.global::<AppServices>().faults().expect("qa faults");
            faults.clear("kanvibe:terminal-open");
        });
    }
    for fault in open_faults.into_iter().chain(list_faults) {
        add_fault_before_open(cx, fault);
    }
    open_board(cx, "/ko")
}

#[gpui::test]
fn a_terminal_session_without_tabs_offers_a_new_tab_in_the_workspace(cx: &mut TestAppContext) {
    let tabs_after_create = ServiceFault {
        after_calls: Some(1),
        ..result_fault(
            "terminalTabs.listTerminalTabs",
            serde_json::json!({ "ok": true, "tabs": [{ "id": "t1", "nativeIndex": 0, "name": "t1", "isActive": true }] }),
        )
    };
    let no_tabs = result_fault(
        "terminalTabs.listTerminalTabs",
        serde_json::json!({ "ok": true, "tabs": [] }),
    );
    let mut board = terminal_session_workspace(cx, vec![tabs_after_create, no_tabs]);
    assert!(board.has_role_name("button", "새 탭"));

    board.click(role_name("button", "새 탭"));

    assert!(board.find(loader_new_tab_button()).is_none());
    assert!(board.has_role_name("tab", "t1"));
}

/// 탭이 없을 때 터미널 자리에 뜨는 새 탭 버튼. 탭 바의 `+` 버튼과 이름이 같아 보이는 글자로 가른다
fn loader_new_tab_button() -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        role: Some("button".into()),
        name: Some("새 탭".into()),
        text: Some("새 탭".into()),
        exact: Some(true),
        ..DescribedTarget::default()
    })
}

fn tab_list(entries: &[(&str, bool)]) -> serde_json::Value {
    let tabs: Vec<serde_json::Value> = entries
        .iter()
        .enumerate()
        .map(|(index, (id, is_active))| {
            serde_json::json!({"id": id, "nativeIndex": index, "name": id, "isActive": is_active})
        })
        .collect();
    serde_json::json!({"ok": true, "tabs": tabs})
}

/// 탭 목록 응답을 바꾼다. 먼저 건 고장이 이기므로 지우고 다시 건다
fn replace_tab_list(board: &mut Board, entries: &[(&str, bool)]) {
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().expect("qa faults");
        faults.clear("terminalTabs.listTerminalTabs");
    });
    board.add_result_fault("terminalTabs.listTerminalTabs", tab_list(entries), None);
    board.draw();
}

fn two_tab_workspace(cx: &mut TestAppContext) -> Board {
    let list = result_fault(
        "terminalTabs.listTerminalTabs",
        tab_list(&[("t1", false), ("t2", true)]),
    );
    terminal_session_workspace(cx, vec![list])
}

#[gpui::test]
fn a_terminal_session_task_shows_its_tab_bar_in_the_workspace_header(cx: &mut TestAppContext) {
    let mut board = two_tab_workspace(cx);

    assert!(board.has_role_name("tab", "t1"));
    assert!(board.has_role_name("tab", "t2"));
    assert!(board.find(loader_new_tab_button()).is_none());
    assert!(board.has_role_name("button", "새 탭"));
}

#[gpui::test]
fn picking_a_workspace_tab_switches_the_terminal_and_reuses_the_hidden_one(
    cx: &mut TestAppContext,
) {
    // 첫 터미널 연결만 바로 끝나고 두 번째부터 멈춘다. 새 탭으로 바꾸면 연결 중이 보이고,
    // 숨겨 둔 첫 탭 터미널로 돌아가면 새 연결 없이 그 연결을 다시 쓴다
    let open_faults = vec![
        ServiceFault {
            after_calls: Some(1),
            mode: FaultMode::Hang,
            value: None,
            ..result_fault("kanvibe:terminal-open", serde_json::Value::Null)
        },
        result_fault("kanvibe:terminal-open", serde_json::json!({ "ok": true })),
    ];
    let list = result_fault(
        "terminalTabs.listTerminalTabs",
        tab_list(&[("t1", false), ("t2", true)]),
    );
    let mut board = open_terminal_session_workspace(cx, open_faults, vec![list]);
    board.add_result_fault(
        "terminalTabs.selectTerminalTab",
        serde_json::json!({"ok": true}),
        None,
    );
    assert!(!board.has_role_name("status", "터미널에 연결하고 있습니다…"));

    replace_tab_list(&mut board, &[("t1", true), ("t2", false)]);
    board.click(role_name("tab", "t1"));
    assert!(
        board.has_role_name("status", "터미널에 연결하고 있습니다…"),
        "t1 opens its own terminal"
    );

    replace_tab_list(&mut board, &[("t1", false), ("t2", true)]);
    board.click(role_name("tab", "t2"));
    assert!(
        !board.has_role_name("status", "터미널에 연결하고 있습니다…"),
        "t2 reuses its first connection"
    );
}

#[gpui::test]
fn a_failed_workspace_tab_command_shows_the_reason_and_retries_it(cx: &mut TestAppContext) {
    let mut board = two_tab_workspace(cx);
    board.add_fault(
        "terminalTabs.selectTerminalTab",
        FaultMode::Fail,
        Some("select sent"),
        None,
    );

    board.click(role_name("tab", "t1"));
    assert!(board.find(text_contains("select sent")).is_some());

    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().expect("qa faults");
        faults.clear("terminalTabs.selectTerminalTab");
    });
    board.add_result_fault(
        "terminalTabs.selectTerminalTab",
        serde_json::json!({"ok": true}),
        None,
    );
    board.click(role_name("button", "다시 시도"));
    assert!(board.find(text_contains("select sent")).is_none());
}

#[gpui::test]
fn adding_a_workspace_tab_sends_a_create_command(cx: &mut TestAppContext) {
    let mut board = two_tab_workspace(cx);
    board.add_fault(
        "terminalTabs.createTerminalTab",
        FaultMode::Fail,
        Some("create sent"),
        None,
    );

    board.click(role_name("button", "새 탭"));

    assert!(board.find(text_contains("create sent")).is_some());
}

fn double_click_tab(board: &mut Board, name: &str) {
    let position = board
        .find(role_name("tab", name))
        .unwrap_or_else(|| panic!("tab {name} not found"))
        .center();
    for click_count in [1, 2] {
        board.cx.simulate_event(gpui_kit::MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        });
        board.cx.simulate_event(gpui_kit::MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count,
        });
    }
    board.draw();
}

#[gpui::test]
fn renaming_a_workspace_tab_sends_the_new_name_and_escape_cancels(cx: &mut TestAppContext) {
    let mut board = two_tab_workspace(cx);
    board.add_fault(
        "terminalTabs.renameTerminalTab",
        FaultMode::Fail,
        Some("rename sent"),
        Some("\"build\""),
    );

    double_click_tab(&mut board, "t2");
    board.keys("ctrl-a");
    board.type_text("draft");
    board.keys("escape");
    assert!(board.find(text_contains("rename sent")).is_none());
    assert!(board.has_role_name("tab", "t2"));

    double_click_tab(&mut board, "t2");
    board.keys("ctrl-a");
    board.type_text("build");
    board.keys("enter");
    assert!(board.find(text_contains("rename sent")).is_some());
}

#[gpui::test]
fn dropping_a_workspace_tab_moves_it_to_that_index(cx: &mut TestAppContext) {
    let mut board = two_tab_workspace(cx);
    board.add_fault(
        "terminalTabs.moveTerminalTab",
        FaultMode::Fail,
        Some("move sent"),
        Some("\"t2\",0"),
    );
    let start = board.find(role_name("tab", "t2")).unwrap().center();
    let end = board.find(role_name("tab", "t1")).unwrap().center();

    board
        .cx
        .simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    let step = gpui_kit::point(start.x - gpui_kit::px(20.0), start.y);
    board
        .cx
        .simulate_mouse_move(step, MouseButton::Left, Modifiers::default());
    board
        .cx
        .simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    board
        .cx
        .simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    board.draw();

    assert!(board.find(text_contains("move sent")).is_some());
}

#[gpui::test]
fn closing_a_workspace_tab_asks_with_its_processes_and_keeps_the_board_open(
    cx: &mut TestAppContext,
) {
    let mut board = two_tab_workspace(cx);
    board.add_result_fault(
        "terminalTabs.getTerminalTabCloseInfo",
        serde_json::json!({"ok": true, "shouldCloseWindow": false, "processNames": ["claude", "node"]}),
        None,
    );
    // 마지막 탭을 닫았다는 응답이어도 워크스페이스는 보드 창을 닫지 않는다(TS는 `shouldCloseWindow`를 쓰지 않았다)
    board.add_result_fault(
        "terminalTabs.closeTerminalTab",
        serde_json::json!({"ok": true, "shouldCloseWindow": true}),
        None,
    );
    board
        .cx
        .update(|_, cx| cx.remove_global::<kanvibe_app::qa::QaRuntime>());
    let tab = board.find(role_name("tab", "t1")).unwrap();
    board
        .cx
        .simulate_mouse_move(tab.center(), None, Modifiers::default());
    board.draw();

    board.click(role_name("button", "t1 탭 닫기"));
    let message = board
        .cx
        .pending_prompt()
        .map(|(message, _)| message)
        .expect("close confirmation");
    assert!(message.contains("claude, node"), "{message}");

    replace_tab_list(&mut board, &[]);
    board.cx.simulate_prompt_answer("OK");
    board.draw();

    assert!(board.find(loader_new_tab_button()).is_some());
    assert!(!board.has_role_name("tab", "t1"));
}

#[gpui::test]
fn picking_another_terminal_task_replaces_the_tab_bar(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let demo = project(&database, "demo", None);
    let terminal_task = |title: &str, status: TaskStatus| {
        task(&database, title, status, |task| {
            task.project_id = Some(demo.id.clone());
            task.session_type = Some(SessionType::Terminal);
            task.session_name = Some(format!("{title}-terminal"));
        })
    };
    let first = terminal_task("첫 터미널 작업", TaskStatus::Todo);
    let second = terminal_task("둘째 터미널 작업", TaskStatus::Progress);
    std::mem::forget(dir);
    for (task_id, tab_id) in [(&first.id, "first-tab"), (&second.id, "second-tab")] {
        add_fault_before_open(
            cx,
            ServiceFault {
                argument_match: Some(task_id.clone()),
                ..result_fault("terminalTabs.listTerminalTabs", tab_list(&[(tab_id, true)]))
            },
        );
    }
    let mut board = open_board(cx, "/ko");
    assert!(board.has_role_name("tab", "first-tab"));

    board.click_text("둘째 터미널 작업");
    assert!(board.has_role_name("tab", "second-tab"));
    assert!(!board.has_role_name("tab", "first-tab"));

    board.click_text("첫 터미널 작업");
    assert!(board.has_role_name("tab", "first-tab"));
    assert!(!board.has_role_name("tab", "second-tab"));
}

#[gpui::test]
fn an_unreadable_workspace_tab_list_shows_the_reason_and_retries(cx: &mut TestAppContext) {
    let unreadable = result_fault(
        "terminalTabs.listTerminalTabs",
        serde_json::json!({ "ok": false, "tabs": [], "error": "session missing" }),
    );
    let mut board = terminal_session_workspace(cx, vec![unreadable]);
    assert!(board.find(text_contains("session missing")).is_some());

    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().expect("qa faults");
        faults.clear("terminalTabs.listTerminalTabs");
    });
    board.add_result_fault(
        "terminalTabs.listTerminalTabs",
        serde_json::json!({ "ok": true, "tabs": [] }),
        None,
    );
    board.click(role_name("button", "다시 시도"));

    assert!(board.find(text_contains("session missing")).is_none());
    assert!(board.has_role_name("button", "새 탭"));
}

#[gpui::test]
fn picking_another_task_switches_the_terminal_pane(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault("kanban.connectTerminalSession", FaultMode::Hang, None, None);
    board.click_text("세션 없는 작업");
    assert!(board.has_role_name("current", "세션 없는 작업"));
}

#[gpui::test]
fn picking_a_task_without_a_session_starts_connecting(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault("kanban.connectTerminalSession", FaultMode::Hang, None, None);
    board.click_text("세션 없는 작업");
    assert!(board.has_text("연결 중..."));
}

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

#[gpui::test]
fn clicking_a_list_task_selects_and_marks_it_current(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = workspace_board(cx, "/ko");
    board.add_fault("kanban.connectTerminalSession", FaultMode::Hang, None, None);
    board.click_text("세션 없는 작업");
    assert!(board.has_role_name("current", "세션 없는 작업"));
    assert!(!board.has_role_name("current", "세션 있는 작업"));
}

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

#[gpui::test]
fn waiting_branch_list_is_announced(cx: &mut TestAppContext) {
    let mut board = branch_todo_modal(cx, Some(FaultMode::Hang));
    assert!(board.has_text("브랜치 목록을 불러오는 중…"));
}

#[gpui::test]
fn failed_branch_list_shows_the_reason_and_a_retry(cx: &mut TestAppContext) {
    let mut board = branch_todo_modal(cx, Some(FaultMode::Fail));
    assert!(board.find(text_contains("git failed")).is_some());
    assert!(board.has_role_name("button", "다시 시도"));
}

#[gpui::test]
fn branch_name_input_has_no_example_placeholder(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click_text("+ 새 작업");
    assert!(board.has_text("브랜치 이름 *"));
    assert!(board.find(text_contains("feat/my-feature")).is_none());
}

#[gpui::test]
fn new_task_defaults_to_herdr_without_a_setting(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click_text("+ 새 작업");
    assert!(board.has_text("herdr"));
}

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

#[gpui::test]
fn registry_dialog_shows_the_scan_form_and_registered_projects(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    assert!(board.has_role_name("dialog", "프로젝트 스캔"));
    assert!(board.has_text("스캔 및 등록"));
    assert!(board.has_text("local-app"));
    assert!(board.has_text("remote-app"));
}

#[gpui::test]
fn escape_closes_the_registry_dialog(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.keys("escape");
    assert!(!board.has_role_name("dialog", "프로젝트 스캔"));
}

#[gpui::test]
fn closed_registry_dialog_renders_nothing(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    assert!(!board.has_role_name("dialog", "프로젝트 스캔"));
    assert!(!board.has_text("스캔 및 등록"));
}

#[gpui::test]
fn open_registry_dialog_blocks_board_shortcuts(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.keys("n");
    assert!(!board.has_text("새 작업 생성"));
}

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

#[gpui::test]
fn sort_picker_offers_only_the_five_fields(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.click(role_name("button", "정렬"));
    for field in ["우선순위", "생성일", "수정일", "제목", "프로젝트"] {
        assert!(board.has_role_name("option", field), "{field}");
    }
    assert!(board.find(text_contains("직접")).is_none());
}

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

#[gpui::test]
fn diff_stats_follow_the_stored_values_on_each_poll(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "변경 작업", TaskStatus::Review, |_| {});
    save_diff_stats(&database, &card.id, 2);
    let mut board = open_board(cx, "/ko");
    board.activate_window();
    assert!(board.has_role_name("status", "2개 파일 변경 · 4줄 추가 · 1줄 삭제"));
    save_diff_stats(&database, &card.id, 5);
    board.cx.executor().advance_clock(Duration::from_secs(6));
    board.draw();
    assert!(board.has_role_name("status", "5개 파일 변경 · 4줄 추가 · 1줄 삭제"));
}

#[gpui::test]
fn stored_diff_stats_show_on_cards_outside_progress(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "리뷰 작업", TaskStatus::Review, |_| {});
    save_diff_stats(&database, &card.id, 3);
    let mut board = open_board(cx, "/ko");
    assert!(board.has_role_name("status", "3개 파일 변경 · 4줄 추가 · 1줄 삭제"));
}

// ───────────── 2단계: 런타임 이벤트·전역 대화상자·프로젝트 등록 ─────────────

use kanvibe_app::shortcuts::{OpenCommandPalette, OpenCreateTask, OpenTaskSearch};
use kanvibe_domain::background_sync::{BackgroundTaskSyncPhase, BackgroundTaskSyncStatus};
use kanvibe_services::board_notifier::{
    BackgroundSyncStatusChangedPayload, BoardEvent, TaskHookInstallFailedPayload,
    TaskPeerDeletionFailedPayload,
};

impl Board {
    fn dispatch(&mut self, action: impl gpui_kit::Action) {
        self.cx.dispatch_action(action);
        self.draw();
    }

    fn fault(&mut self, target: &str, message: &str) {
        self.add_fault(target, FaultMode::Fail, Some(message), None);
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        self.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .runtime()
                .expect("runtime")
                .app_settings()
                .get_app_setting(key)
                .expect("setting")
        })
    }
}

fn sync_status(phase: BackgroundTaskSyncPhase, is_running: bool) -> BoardEvent {
    BoardEvent::BackgroundSyncStatusChanged(BackgroundSyncStatusChangedPayload {
        status: BackgroundTaskSyncStatus {
            phase,
            is_running,
            completed_stages: 1,
            review_needed: false,
            board_updated: false,
            error: None,
        },
    })
}

fn hook_failure() -> BoardEvent {
    BoardEvent::TaskHookInstallFailed(TaskHookInstallFailedPayload {
        task_id: "t1".into(),
        task_title: "보드 필터 추가".into(),
        error: "denied".into(),
    })
}

#[gpui::test]
fn board_event_alert_dismisses_itself_after_five_seconds(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.emit_board_event(hook_failure());
    assert!(board.find(text_contains("Hooks 자동 설치 실패")).is_some());
    board.advance(Duration::from_millis(5_100));
    assert!(board.find(text_contains("Hooks 자동 설치 실패")).is_none());
}

#[gpui::test]
fn board_event_alert_closes_with_the_close_button(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.emit_board_event(hook_failure());
    board.click(role_name("button", "닫기"));
    assert!(board.find(text_contains("Hooks 자동 설치 실패")).is_none());
}

#[gpui::test]
fn peer_deletion_alert_stays_until_closed(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.emit_board_event(BoardEvent::TaskPeerDeletionFailed(
        TaskPeerDeletionFailedPayload {
            task_id: "t1".into(),
            task_title: "로그인 후 리다이렉트 수정".into(),
            failed_peer_urls: vec!["http://192.0.2.10:9736".into()],
        },
    ));
    board.advance(Duration::from_secs(30));
    assert!(
        board
            .find(text_contains("다른 데스크탑에서는 삭제하지 못함"))
            .is_some()
    );
}

#[gpui::test]
fn board_updates_are_debounced_into_one_refresh(cx: &mut TestAppContext) {
    let (mut board, database, _dir) = seeded_board(cx, true);
    task(&database, "다른 창에서 만든 작업", TaskStatus::Todo, |_| {});
    for _ in 0..3 {
        board.emit_board_event(BoardEvent::BoardUpdated);
    }
    assert!(!board.has_text("다른 창에서 만든 작업"));
    board.advance(Duration::from_millis(300));
    assert!(board.has_text("다른 창에서 만든 작업"));
}

#[gpui::test]
fn notification_changes_recount_unread_badges(cx: &mut TestAppContext) {
    let (mut board, database, _dir) = seeded_board(cx, true);
    let target = kanvibe_db::repositories::tasks::list_tasks(&database.connection())
        .unwrap()
        .into_iter()
        .find(|task| task.title == "루트 작업")
        .unwrap();
    assert!(!board.has_role_name("status", "읽지 않음 1개"));
    board.cx.update(|_, cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        runtime
            .notification_store()
            .create_notification(&kanvibe_domain::notifications::DesktopNotificationPayload {
                title: "루트 작업".into(),
                body: "Review 상태로 바뀌었습니다".into(),
                task_id: Some(target.id.clone()),
                locale: "ko".into(),
                relative_path: Some(format!("/ko/task/{}", target.id)),
                dedupe_key: None,
                action: None,
            })
            .unwrap();
        kanvibe_app::notifications::reload_notification_center(cx);
        let bus = cx.global::<kanvibe_app::app_events::AppEvents>().0.clone();
        bus.update(cx, |_, cx| {
            cx.emit(kanvibe_app::app_events::NotificationsChanged)
        });
    });
    board.draw();
    assert!(board.has_role_name("status", "읽지 않음 1개"));
}

#[gpui::test]
fn sync_progress_shows_on_the_button_and_unlocks_after_completion(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.emit_board_event(sync_status(BackgroundTaskSyncPhase::PullRequests, true));
    assert!(board.has_role_name("status", "동기화 중…"));
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    assert!(board.has_text("2/3 · PR 상태 확인 중"));
    board.emit_board_event(sync_status(BackgroundTaskSyncPhase::Completed, false));
    assert!(!board.has_role_name("status", "동기화 중…"));
    assert!(board.has_role_name("button", "지금 동기화"));
}

#[gpui::test]
fn sync_start_failure_stays_visible_and_allows_retry(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.fault(
        "backgroundTaskSync.startBackgroundTaskSyncNow",
        "gh missing",
    );
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    board.click(role_name("button", "지금 동기화"));
    assert!(board.find(text_contains("gh missing")).is_some());
    assert!(board.has_role_name("button", "다시 시도"));
}

#[gpui::test]
fn palette_sync_command_runs_the_background_sync(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.fault(
        "backgroundTaskSync.startBackgroundTaskSyncNow",
        "sync was started",
    );
    board.dispatch(OpenCommandPalette);
    board.type_text("sync");
    board.keys("enter");
    assert!(!board.shell_read(|shell, _| shell.is_command_palette_open()));
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    assert!(board.find(text_contains("sync was started")).is_some());
}

#[gpui::test]
fn clicking_the_sync_row_starts_sync_and_closes(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.fault(
        "backgroundTaskSync.startBackgroundTaskSyncNow",
        "sync was started",
    );
    board.dispatch(OpenCommandPalette);
    board.click_text("sync");
    assert!(!board.shell_read(|shell, _| shell.is_command_palette_open()));
    assert!(board.cx.update(|_, cx| {
        background_sync_store(cx).read(cx).state().error.as_deref()
            == Some("Error invoking remote method 'kanvibe:invoke': Error: sync was started")
    }));
}

#[gpui::test]
fn palette_does_not_start_sync_while_another_sync_runs(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.fault(
        "backgroundTaskSync.startBackgroundTaskSyncNow",
        "should not start",
    );
    board.emit_board_event(sync_status(BackgroundTaskSyncPhase::Worktrees, true));
    board.dispatch(OpenCommandPalette);
    assert!(board.has_text("동기화하는 중…"));
    board.keys("enter");
    assert!(
        board
            .cx
            .update(|_, cx| { background_sync_store(cx).read(cx).state().error.is_none() })
    );
}

#[gpui::test]
fn palette_move_command_moves_the_focused_card(cx: &mut TestAppContext) {
    let (mut board, database, _dir) = seeded_board(cx, true);
    board.keys("down");
    board.dispatch(OpenCommandPalette);
    board.type_text("move review");
    board.keys("enter");
    let moved = kanvibe_db::repositories::tasks::list_tasks(&database.connection())
        .unwrap()
        .into_iter()
        .find(|task| task.title == "루트 작업")
        .unwrap();
    assert_eq!(moved.status, TaskStatus::Review);
}

#[gpui::test]
fn palette_review_row_moves_the_task_shown_in_the_detail_screen(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "상세 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    let app_window = board.app_window.clone();
    let detail_path = format!("/ko/task/{}", card.id);
    board.cx.update(|_, cx| {
        app_window.update(cx, |app_window, cx| app_window.navigate(&detail_path, cx))
    });
    board.draw();

    board.dispatch(OpenCommandPalette);
    board.click_text("move review");

    assert!(!board.shell_read(|shell, _| shell.is_command_palette_open()));
    let moved = find_task(&database.connection(), &card.id)
        .unwrap()
        .unwrap();
    assert_eq!(moved.status, TaskStatus::Review);
}

#[gpui::test]
fn palette_offers_moves_for_the_board_focused_card(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.keys("down");
    board.dispatch(OpenCommandPalette);
    assert!(board.has_text("move review"));
    assert!(board.has_text("Review 상태로 이동"));
}

#[gpui::test]
fn palette_without_a_move_target_has_no_move_rows(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCommandPalette);
    assert!(board.has_text("sync"));
    assert!(!board.has_text("move todo"));
    assert!(!board.has_text("move review"));
}

#[gpui::test]
fn palette_rows_render_command_and_description_separately(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCommandPalette);
    assert!(board.has_text("view kanban"));
    assert!(board.has_text("칸반 보드로 보기"));
}

#[gpui::test]
fn palette_query_narrows_the_rows(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCommandPalette);
    board.type_text("칸반");
    assert!(board.has_text("view kanban"));
    assert!(!board.has_text("sync"));
}

#[gpui::test]
fn palette_command_tokens_leave_only_that_row(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.keys("down");
    board.dispatch(OpenCommandPalette);
    board.type_text("move progress");
    assert!(board.has_text("move progress"));
    assert!(!board.has_text("move review"));
    assert!(!board.has_text("sync"));
}

#[gpui::test]
fn palette_without_matches_shows_the_empty_message(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCommandPalette);
    board.type_text("zzzz");
    assert!(board.has_text("일치하는 명령이 없습니다"));
}

#[gpui::test]
fn arrow_down_then_enter_runs_the_second_row(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCommandPalette);
    board.keys("down enter");
    assert_eq!(board.setting("board_view_mode").as_deref(), Some("kanban"));
}

#[gpui::test]
fn view_command_saves_the_board_view(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCommandPalette);
    board.type_text("view workspace");
    board.keys("enter");
    assert_eq!(
        board.setting("board_view_mode").as_deref(),
        Some("workspace")
    );
}

#[gpui::test]
fn open_palette_blocks_other_board_shortcuts(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCommandPalette);
    board.dispatch(OpenTaskSearch);
    assert!(!board.has_text("태스크 빠른 검색"));
    board.keys("escape");
    assert!(!board.shell_read(|shell, _| shell.is_command_palette_open()));
}

#[gpui::test]
fn quick_search_shortcut_opens_and_loads_tasks(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenTaskSearch);
    assert!(board.has_text("태스크 빠른 검색"));
    assert!(board.has_role_name("button", "feat/inherit"));
}

#[gpui::test]
fn quick_search_header_shows_no_shortcut_text(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenTaskSearch);
    assert!(board.has_text("태스크 빠른 검색"));
    assert!(!board.has_text("Ctrl+Shift+O"));
}

#[gpui::test]
fn quick_search_does_not_open_behind_a_shortcut_blocker(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.dispatch(OpenTaskSearch);
    assert!(!board.has_text("태스크 빠른 검색"));
}

#[gpui::test]
fn board_shortcuts_are_ignored_while_quick_search_is_open(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenTaskSearch);
    board.dispatch(OpenCommandPalette);
    assert!(!board.shell_read(|shell, _| shell.is_command_palette_open()));
}

#[gpui::test]
fn only_remote_results_show_the_remote_badge_and_host(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenTaskSearch);
    board.type_text("inherit");
    assert!(board.has_text("원격"));
    assert!(board.has_text("devbox"));
    board.keys("escape");
    board.dispatch(OpenTaskSearch);
    board.type_text("루트");
    assert!(!board.has_text("원격"));
}

#[gpui::test]
fn escape_closes_quick_search(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenTaskSearch);
    board.keys("escape");
    assert!(!board.has_text("태스크 빠른 검색"));
}

#[gpui::test]
fn new_task_shortcut_in_quick_search_requests_a_branch_todo(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenTaskSearch);
    board.type_text("inherit");
    board.dispatch(OpenCreateTask);
    assert!(!board.has_text("태스크 빠른 검색"));
    assert_eq!(
        board
            .view
            .read_with(&board.cx, |view, cx| view.create_task_base_branch(cx)),
        Some("feat/inherit".to_string())
    );
}

#[gpui::test]
fn quick_search_load_failure_shows_reason_and_retry(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.fault("kanban.getSearchableTasks", "database is locked");
    board.dispatch(OpenTaskSearch);
    assert!(board.find(text_contains("database is locked")).is_some());
    assert!(!board.has_text("일치하는 태스크가 없습니다."));
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanban.getSearchableTasks")
    });
    board.click(role_name("button", "다시 시도"));
    assert!(board.has_role_name("button", "feat/inherit"));
}

#[gpui::test]
fn workspace_quick_search_selects_the_task_in_place(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let target = task(&database, "찾을 작업", TaskStatus::Todo, |task| {
        task.branch_name = Some("feat/find-me".into());
    });
    task(&database, "다른 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.dispatch(OpenTaskSearch);
    board.type_text("find-me");
    board.keys("enter");
    assert!(!board.has_text("태스크 빠른 검색"));
    assert_eq!(
        board
            .view
            .read_with(&board.cx, |view, _| view.selected_workspace_task_id()),
        Some(target.id)
    );
}

#[gpui::test]
fn confirmed_deletion_removes_the_project(cx: &mut TestAppContext) {
    let (mut board, database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "삭제"));
    let names: Vec<String> =
        kanvibe_db::repositories::projects::list_projects(&database.connection())
            .unwrap()
            .into_iter()
            .map(|project| project.name)
            .collect();
    assert_eq!(names.len(), 1);
}

#[gpui::test]
fn deleting_project_keeps_the_scan_button_and_marks_the_row(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.add_fault("project.deleteProject", FaultMode::Hang, None, None);
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "삭제"));
    assert!(board.has_role_name("button", "삭제 중…"));
    assert!(board.has_role_name("button", "스캔 및 등록"));
}

#[gpui::test]
fn failed_deletion_keeps_the_reason_and_retry_deletes_the_same_project(cx: &mut TestAppContext) {
    let (mut board, database) = host_board(cx);
    board.fault("project.deleteProject", "project is busy");
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "삭제"));
    assert!(board.find(text_contains("project is busy")).is_some());
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("project.deleteProject")
    });
    board.click(role_name("button", "다시 시도"));
    let count = kanvibe_db::repositories::projects::list_projects(&database.connection())
        .unwrap()
        .len();
    assert_eq!(count, 1);
}

#[gpui::test]
fn deletion_failing_after_close_is_announced_by_toast(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "project.deleteProject".into(),
                mode: FaultMode::Fail,
                message: Some("project is busy".into()),
                ms: Some(1_000),
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "삭제"));
    assert!(board.has_role_name("button", "삭제 중…"), "deleting");
    board.keys("escape");
    assert!(!board.has_role_name("dialog", "프로젝트 스캔"), "closed");
    board.advance(Duration::from_millis(1_100));
    let toasts = board.shell_read(|shell, _| shell.toast_messages());
    assert_eq!(
        toasts,
        [
            "프로젝트를 삭제하지 못했습니다: Error invoking remote method 'kanvibe:invoke': Error: project is busy"
        ]
    );
}

#[gpui::test]
fn failed_scan_shows_the_reason_and_retry(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.fault("project.scanAndRegisterProjects", "permission denied");
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "스캔 및 등록"));
    assert!(board.find(text_contains("스캔하지 못했습니다")).is_some());
    assert!(board.find(text_contains("permission denied")).is_some());
    assert!(board.has_role_name("button", "다시 시도"));
}

#[gpui::test]
fn scan_submits_the_chosen_path_and_host(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.add_fault(
        "project.scanAndRegisterProjects",
        FaultMode::Fail,
        Some("received"),
        Some("[\"~/\",\"\"]"),
    );
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "스캔 및 등록"));
    assert!(board.find(text_contains("received")).is_some());
}

#[gpui::test]
fn scanning_clears_the_project_search(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.add_fault(
        "project.scanAndRegisterProjects",
        FaultMode::Hang,
        None,
        None,
    );
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("searchbox", "프로젝트 검색..."));
    board.type_text("zzz");
    assert!(board.has_text("일치하는 프로젝트가 없습니다."));
    board.click(role_name("button", "스캔 및 등록"));
    assert!(board.has_text("local-app"));
    assert!(board.has_role_name("button", "스캔 중..."));
}

#[gpui::test]
fn search_without_matches_shows_the_notice(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("searchbox", "프로젝트 검색..."));
    board.type_text("없는이름");
    assert!(board.has_text("일치하는 프로젝트가 없습니다."));
    assert!(!board.has_text("local-app"));
}

fn find_match_title(board: &mut Board) -> Option<(String, String)> {
    let found = board
        .view
        .read_with(&board.cx, |view, _| view.find_match())?;
    let title = kanvibe_db::repositories::tasks::list_tasks(
        &board
            .cx
            .update(|_, cx| cx.global::<AppServices>().database().unwrap().clone())
            .connection(),
    )
    .unwrap()
    .into_iter()
    .find(|task| task.id == found.task_id)
    .map(|task| task.title)?;
    let matched = title.get(found.range.clone()).map(str::to_string)?;
    Some((title, matched))
}

#[gpui::test]
fn slash_opens_find_and_enter_selects_the_matching_text(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("/");
    board.type_text("우선순위");
    board.keys("enter");
    let (title, matched) = find_match_title(&mut board).expect("a selected match");
    assert_eq!(matched, "우선순위");
    assert!(title.contains("우선순위"));
}

#[gpui::test]
fn enter_and_shift_enter_walk_the_matches_forward_and_backward(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, false);
    board.keys("/");
    board.type_text("작업");
    board.keys("enter");
    let first = find_match_title(&mut board).expect("first");
    board.keys("enter");
    let second = find_match_title(&mut board).expect("second");
    assert_ne!(first.0, second.0);
    board.keys("shift-enter");
    assert_eq!(find_match_title(&mut board), Some(first));
}

// ───────────── 릴리스 업데이트 대화상자 ─────────────

fn release_board(cx: &mut TestAppContext, dismissed: Option<&str>) -> (Board, tempfile::TempDir) {
    let dir = temp_dir();
    let database = init_app_with_network(
        cx,
        dir.path().to_path_buf(),
        true,
        Some("release-available"),
    );
    if let Some(versions) = dismissed {
        save_app_setting(
            &database.connection(),
            "release_update_dismissed_versions",
            versions,
        )
        .unwrap();
    }
    task(&database, "보드 카드", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    // 릴리스 확인은 창이 앞에 있을 때만 돈다(TS `document.hasFocus()`). 다른 시험은 창을 앞으로 두지 않아 바깥 요청을 하지 않는다
    board.cx.update(|window, _| window.activate_window());
    board.draw();
    (board, dir)
}

fn release_version(board: &mut Board) -> Option<String> {
    board.shell_read(|shell, _| shell.release_update_version())
}

#[gpui::test]
fn release_notes_show_when_an_update_is_available(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, None);
    assert!(board.has_role_name("heading", "KanVibe 9.9.0 업데이트"));
    assert!(board.has_role_name("button", "릴리스 보기"));
}

#[gpui::test]
fn dismissed_release_versions_are_not_shown(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, Some("[\"9.9.0\"]"));
    assert_eq!(release_version(&mut board), None);
}

#[gpui::test]
fn checked_dont_show_again_stores_the_version(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, None);
    board.click(role_name("button", "이 버전 다시 보지 않기"));
    board.click(role_name("button", "닫기"));
    assert_eq!(release_version(&mut board), None);
    assert_eq!(
        board
            .setting("release_update_dismissed_versions")
            .as_deref(),
        Some("[\"9.9.0\"]")
    );
}

#[gpui::test]
fn unchecked_close_does_not_store_the_version(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, None);
    board.click(role_name("button", "닫기"));
    assert_eq!(release_version(&mut board), None);
    assert_eq!(board.setting("release_update_dismissed_versions"), None);
}

#[gpui::test]
fn the_same_release_is_not_shown_twice_in_a_session(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, None);
    board.click(role_name("button", "닫기"));
    board.advance(Duration::from_secs(60 * 60 + 1));
    assert_eq!(release_version(&mut board), None);
}

#[gpui::test]
fn view_release_closes_the_dialog(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, None);
    board.click(role_name("button", "릴리스 보기"));
    assert_eq!(release_version(&mut board), None);
}

#[gpui::test]
fn a_release_claimed_by_another_window_is_not_shown(cx: &mut TestAppContext) {
    let dir = temp_dir();
    init_app_with_network(
        cx,
        dir.path().to_path_buf(),
        true,
        Some("release-available"),
    );
    cx.update(|cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        assert!(
            runtime
                .release_update()
                .claim_release_update_version("9.9.0")
        );
    });
    let mut board = open_board(cx, "/ko");
    assert_eq!(release_version(&mut board), None);
}

#[gpui::test]
fn release_dialog_keeps_board_shortcuts_from_running(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, None);
    board.dispatch(OpenCommandPalette);
    assert!(!board.shell_read(|shell, _| shell.is_command_palette_open()));
    board.keys("escape");
    assert_eq!(release_version(&mut board), None);
}

// ───────────── 보드 명령 전달 ─────────────

#[gpui::test]
fn new_task_shortcut_opens_the_create_task_modal(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenCreateTask);
    assert!(
        board
            .view
            .read_with(&board.cx, |view, _| view.is_create_task_open())
    );
}

#[gpui::test]
fn notification_shortcut_opens_only_the_notification_center(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    assert!(board.has_role_name("button", "지금 동기화"));
    assert!(
        !board
            .view
            .read_with(&board.cx, |view, _| view.is_create_task_open())
    );
}

#[gpui::test]
fn desktop_filter_shortcut_opens_the_desktop_filter(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(kanvibe_app::shortcuts::OpenDesktopFilter);
    assert!(board.has_text("로컬"));
}

#[gpui::test]
fn board_shortcuts_are_ignored_behind_a_blocker(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    board.click(role_name("button", "프로젝트 스캔"));
    board.dispatch(OpenCreateTask);
    board.dispatch(OpenCommandPalette);
    assert!(
        !board
            .view
            .read_with(&board.cx, |view, _| view.is_create_task_open())
    );
    assert!(!board.shell_read(|shell, _| shell.is_command_palette_open()));
}

#[gpui::test]
fn filter_and_notification_shortcuts_are_ignored_behind_a_context_menu(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.right_click_text("루트 작업");
    assert!(board.has_role_name("menuitem", "삭제"));

    board.dispatch(kanvibe_app::shortcuts::OpenProjectFilter);
    board.dispatch(kanvibe_app::shortcuts::OpenDesktopFilter);
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);

    assert!(!board.has_role_name("option", "kanvibe-demo"));
    assert!(!board.has_text("로컬"));
    assert!(!board.has_role_name("button", "지금 동기화"));
}

#[gpui::test]
fn branch_todo_requests_reach_the_board(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.dispatch(OpenTaskSearch);
    board.type_text("own");
    board.dispatch(OpenCreateTask);
    assert_eq!(
        board
            .view
            .read_with(&board.cx, |view, cx| view.create_task_base_branch(cx)),
        Some("feat/own".to_string())
    );
}

#[gpui::test]
fn rapid_sync_starts_are_blocked_while_one_is_starting(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.add_fault(
        "backgroundTaskSync.startBackgroundTaskSyncNow",
        FaultMode::Hang,
        None,
        None,
    );
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    board.click(role_name("button", "지금 동기화"));
    assert!(board.has_role_name("button", "동기화 중…"));
    assert!(board.has_role_name("status", "동기화 중…"));
}

#[gpui::test]
fn palette_sync_failure_is_shared_with_the_notification_center(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.fault(
        "backgroundTaskSync.startBackgroundTaskSyncNow",
        "gh missing",
    );
    board.dispatch(OpenCommandPalette);
    board.keys("enter");
    assert!(board.has_role_name("status", "동기화에 실패했습니다. 다시 시도해 주세요."));
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    assert!(board.find(text_contains("gh missing")).is_some());
}

#[gpui::test]
fn space_on_a_read_notification_does_not_save_again(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = seeded_board(cx, true);
    board.cx.update(|_, cx| {
        let runtime = cx.global::<AppServices>().runtime().unwrap().clone();
        let created = runtime
            .notification_store()
            .create_notification(&kanvibe_domain::notifications::DesktopNotificationPayload {
                title: "읽은 알림".into(),
                body: "이미 읽음".into(),
                task_id: None,
                locale: "ko".into(),
                relative_path: Some("/ko".into()),
                dedupe_key: None,
                action: None,
            })
            .unwrap();
        runtime
            .notification_store()
            .mark_notification_read(&created.notification.id)
            .unwrap();
    });
    board.emit_board_event(BoardEvent::BoardUpdated);
    board.advance(Duration::from_millis(300));
    board.fault("kanvibe:notifications-mark-read", "should not be called");
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    board.keys("space");
    let toasts = board.shell_read(|shell, _| shell.toast_messages());
    assert!(toasts.is_empty());
}

#[gpui::test]
fn deleting_card_blocks_the_context_menu(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_fault("kanban.deleteTask", FaultMode::Hang, None, None);
    board.right_click_text("지울 작업");
    board.click(role_name("menuitem", "삭제"));
    assert!(board.has_text("삭제 중…"));
    board.right_click_text("지울 작업");
    assert!(!board.has_role_name("menuitem", "삭제"));
}

// ───────────── CreateTaskModal / BranchTaskModal 요청·이동 ─────────────

/// 브랜치가 있는 카드의 메뉴로 새 작업 모달을 연다. 베이스 브랜치는 카드 브랜치(main)이고 브랜치 목록은 고정 응답이다
fn create_task_modal(
    cx: &mut TestAppContext,
    ssh_host: Option<&str>,
    session_type: &str,
) -> (Board, Arc<Database>, tempfile::TempDir) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "default_session_type", session_type).unwrap();
    let demo = project(&database, "demo", ssh_host);
    task(&database, "기준 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(demo.id.clone());
        task.branch_name = Some("main".into());
    });
    let mut board = open_board(cx, "/ko");
    board.add_result_fault(
        "project.getProjectBranches",
        serde_json::json!(["main", "develop"]),
        None,
    );
    board.right_click_text("기준 작업");
    board.click(role_name("menuitem", "브랜치 기반 TODO 생성"));
    (board, database, dir)
}

/// 베이스 브랜치 칸(처음 포커스)에서 Tab으로 브랜치 이름 칸에 가서 입력한다
fn type_branch_name(board: &mut Board, branch_name: &str) {
    board.keys("tab");
    board.type_text(branch_name);
}

/// 이 인자로 부를 때만 응답을 돌려주는 만들기 고장. 다른 인자면 실제 서비스가 돌아 실패한다
fn expect_create_request(board: &mut Board, task_id: &str, argument_match: &str) {
    board.add_result_fault(
        "kanban.createTask",
        serde_json::json!({ "id": task_id }),
        Some(argument_match),
    );
}

/// 세션 도구 확인을 부르면 실패하게 한다. 부르지 않는 흐름이면 아무 일도 없다
fn forbid_dependency_prompt(board: &mut Board) {
    board.add_fault(
        "sessionDependency.*",
        FaultMode::Fail,
        Some("dependency prompt must not run"),
        None,
    );
}

#[gpui::test]
fn local_project_create_sends_the_request_without_the_dependency_prompt(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    forbid_dependency_prompt(&mut board);
    expect_create_request(
        &mut board,
        "task-local-fast",
        r#""branchName":"fix/session-prompt""#,
    );
    type_branch_name(&mut board, "fix/session-prompt");
    board.click(role_name("button", "생성"));
    assert_eq!(board.current_path(), "/ko/task/task-local-fast");
}

#[gpui::test]
fn remote_project_create_sends_the_request_without_the_dependency_prompt(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, Some("remote-box"), "tmux");
    forbid_dependency_prompt(&mut board);
    expect_create_request(
        &mut board,
        "task-remote-fast",
        r#""branchName":"fix/remote-fast""#,
    );
    type_branch_name(&mut board, "fix/remote-fast");
    board.click(role_name("button", "생성"));
    assert_eq!(board.current_path(), "/ko/task/task-remote-fast");
    assert!(!board.find(text_contains("dependency prompt")).is_some());
}

#[gpui::test]
fn ready_dependency_lets_the_created_task_open(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, Some("remote-box"), "herdr");
    board.add_result_fault(
        "sessionDependency.getSessionDependencyStatus",
        serde_json::json!({"available": true, "toolName": "herdr", "isRemote": true, "sshHost": "remote-box"}),
        Some(r#"["herdr","remote-box"]"#),
    );
    // 이미 있으면 설치를 부르지 않는다(불렀다면 이 실패로 만들기가 멈춘다)
    board.add_fault(
        "sessionDependency.installSessionDependency",
        FaultMode::Fail,
        Some("install must not run"),
        None,
    );
    expect_create_request(&mut board, "task-2", r#""sessionType":"herdr""#);
    type_branch_name(&mut board, "fix/session-prompt");
    board.click(role_name("button", "생성"));
    assert_eq!(board.current_path(), "/ko/task/task-2");
}

/// Herdr가 없는 호스트. 확인(QA에서는 자동 수락) 뒤 설치 결과에 따라 만들지 정한다
fn herdr_missing_create(cx: &mut TestAppContext, is_install_ready: bool) -> Board {
    let (mut board, _database, _dir) = create_task_modal(cx, Some("remote-box"), "herdr");
    std::mem::forget(_dir);
    board.add_result_fault(
        "sessionDependency.getSessionDependencyStatus",
        serde_json::json!({"available": false, "toolName": "herdr", "isRemote": true, "sshHost": "remote-box"}),
        None,
    );
    board.add_result_fault(
        "sessionDependency.installSessionDependency",
        serde_json::json!({"available": is_install_ready, "toolName": "herdr", "isRemote": true, "sshHost": "remote-box", "blockedReason": "no curl"}),
        None,
    );
    expect_create_request(&mut board, "herdr-task", r#""branchName":"feature-herdr""#);
    type_branch_name(&mut board, "feature-herdr");
    board.click(role_name("button", "생성"));
    board
}

#[gpui::test]
fn herdr_task_is_created_only_after_the_dependency_is_ready(cx: &mut TestAppContext) {
    let mut board = herdr_missing_create(cx, true);
    assert_eq!(board.current_path(), "/ko/task/herdr-task");
}

#[gpui::test]
fn declining_the_install_prompt_neither_installs_nor_creates(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, Some("remote-a"), "herdr");
    board.add_result_fault(
        "sessionDependency.getSessionDependencyStatus",
        serde_json::json!({"available": false, "toolName": "herdr", "isRemote": true, "sshHost": "remote-a"}),
        None,
    );
    // 설치를 불렀다면 성공해 만들기가 이어지므로, 경로가 그대로면 설치하지 않은 것이다
    board.add_result_fault(
        "sessionDependency.installSessionDependency",
        serde_json::json!({"available": true, "toolName": "herdr", "isRemote": true, "sshHost": "remote-a"}),
        None,
    );
    expect_create_request(
        &mut board,
        "declined-task",
        r#""branchName":"feature-declined""#,
    );
    type_branch_name(&mut board, "feature-declined");
    // QA 자동 수락을 꺼서 실제 확인 대화상자를 띄운다
    board
        .cx
        .update(|_, cx| cx.remove_global::<kanvibe_app::qa::QaRuntime>());
    board.click(role_name("button", "생성"));
    assert!(board.cx.has_pending_prompt());
    board.cx.simulate_prompt_answer("Cancel");
    board.draw();
    assert_eq!(board.current_path(), "/ko");
    assert!(board.has_role_name("button", "생성"));
    assert!(!board.find(text_contains("설치에 실패했습니다")).is_some());
}

#[gpui::test]
fn herdr_task_is_not_created_when_the_dependency_is_not_ready(cx: &mut TestAppContext) {
    let mut board = herdr_missing_create(cx, false);
    assert_eq!(board.current_path(), "/ko");
    assert!(board.has_text("herdr 설치에 실패했습니다: no curl"));
}

#[gpui::test]
fn tab_and_shift_tab_walk_the_main_inputs_from_the_base_branch(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    board.keys("tab");
    board.type_text("b");
    board.keys("tab");
    board.type_text("d");
    board.keys("shift-tab");
    board.type_text("c");
    board.keys("shift-tab");
    board.type_text("x");
    let view = board.view.clone();
    let texts = board
        .cx
        .update(|_, cx| view.read(cx).create_task_field_texts(cx));
    assert_eq!(
        texts,
        Some(["mainx".to_string(), "bc".to_string(), "d".to_string()])
    );
}

#[gpui::test]
fn escape_in_a_modal_input_closes_the_modal(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    type_branch_name(&mut board, "fix/escape");
    board.keys("escape");
    let view = board.view.clone();
    assert!(!board.cx.update(|_, cx| view.read(cx).is_create_task_open()));
}

#[gpui::test]
fn enter_in_an_input_creates_the_task(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    expect_create_request(
        &mut board,
        "task-enter",
        r#""branchName":"fix/enter-submit""#,
    );
    type_branch_name(&mut board, "fix/enter-submit");
    board.keys("enter");
    assert_eq!(board.current_path(), "/ko/task/task-enter");
}

#[gpui::test]
fn board_refresh_keeps_the_chosen_base_branch(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    board.keys("backspace backspace backspace backspace");
    board.type_text("dev");
    board.keys("enter");
    let view = board.view.clone();
    let chosen = board
        .cx
        .update(|_, cx| view.read(cx).create_task_base_branch(cx));
    assert_eq!(chosen.as_deref(), Some("develop"));
    board
        .cx
        .update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
    board.draw();
    let kept = board
        .cx
        .update(|_, cx| view.read(cx).create_task_base_branch(cx));
    assert_eq!(kept.as_deref(), Some("develop"));
}

#[gpui::test]
fn repeated_enter_while_creating_sends_one_request(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().unwrap().clone();
        faults.add(ServiceFault {
            target: "kanban.createTask".into(),
            mode: FaultMode::Fail,
            message: Some("duplicate create request".into()),
            ms: None,
            value: None,
            after_calls: Some(1),
            argument_match: None,
        });
        faults.add(ServiceFault {
            target: "kanban.createTask".into(),
            mode: FaultMode::Result,
            message: None,
            ms: None,
            value: Some(serde_json::json!({"id": "task-once", "delayMs": 1000})),
            after_calls: None,
            argument_match: None,
        });
    });
    type_branch_name(&mut board, "fix/double-enter");
    board.keys("enter");
    board.keys("enter");
    board.keys("enter");
    board.advance(Duration::from_millis(1_100));
    assert_eq!(board.current_path(), "/ko/task/task-once");
}

/// 만들기를 보내고 응답이 오기 전에 "닫기 (백그라운드에서 계속)"로 모달을 닫는다
fn close_while_creating(board: &mut Board, value: serde_json::Value) {
    board.add_result_fault("kanban.createTask", value, None);
    type_branch_name(board, "feat/background");
    board.click(role_name("button", "생성"));
    board.click(role_name("button", "닫기 (백그라운드에서 계속)"));
    let view = board.view.clone();
    assert!(!board.cx.update(|_, cx| view.read(cx).is_create_task_open()));
}

#[gpui::test]
fn closing_while_creating_shows_an_open_toast_instead_of_navigating(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    close_while_creating(
        &mut board,
        serde_json::json!({"id": "task-background", "delayMs": 1000}),
    );
    board.advance(Duration::from_millis(1_100));
    assert_eq!(
        board.toast_messages(),
        vec!["\"feat/background\" 작업을 만들었습니다".to_string()]
    );
    assert_eq!(board.current_path(), "/ko");
    board.click(role_name("button", "열기"));
    assert_eq!(board.current_path(), "/ko/task/task-background");
}

#[gpui::test]
fn failure_after_closing_is_announced_by_a_failure_toast(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "kanban.createTask".into(),
                mode: FaultMode::Fail,
                message: Some("worktree exists".into()),
                ms: Some(1000),
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    type_branch_name(&mut board, "feat/broken");
    board.click(role_name("button", "생성"));
    board.click(role_name("button", "닫기 (백그라운드에서 계속)"));
    board.advance(Duration::from_millis(1_100));
    assert_eq!(
        board.toast_messages(),
        vec![
            "작업을 만들지 못했습니다: Error invoking remote method 'kanvibe:invoke': Error: worktree exists"
                .to_string()
        ]
    );
}

/// 브랜치가 없는 카드의 메뉴로 분기 모달을 연다
fn branch_task_modal(
    cx: &mut TestAppContext,
    ssh_host: Option<&str>,
    session_type: &str,
) -> (Board, tempfile::TempDir) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "default_session_type", session_type).unwrap();
    project(&database, "demo", ssh_host);
    task(&database, "분기할 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.add_result_fault(
        "project.getProjectBranches",
        serde_json::json!(["main"]),
        None,
    );
    board.right_click_text("분기할 작업");
    board.click(role_name("menuitem", "브랜치 분기"));
    (board, dir)
}

fn expect_branch_request(board: &mut Board, task_id: &str, argument_match: &str) {
    board.add_result_fault(
        "kanban.branchFromTask",
        serde_json::json!({ "id": task_id }),
        Some(argument_match),
    );
}

fn branch_modal_is_open(board: &mut Board) -> bool {
    let view = board.view.clone();
    board.cx.update(|_, cx| view.read(cx).is_create_task_open())
}

#[gpui::test]
fn remote_project_branch_sends_the_request_without_the_dependency_prompt(cx: &mut TestAppContext) {
    let (mut board, _dir) = branch_task_modal(cx, Some("remote-box"), "tmux");
    forbid_dependency_prompt(&mut board);
    expect_branch_request(
        &mut board,
        "branched",
        r#""main","feat/remote-fast","tmux"]"#,
    );
    type_branch_name(&mut board, "feat/remote-fast");
    board.click(role_name("button", "분기"));
    assert!(!branch_modal_is_open(&mut board));
}

#[gpui::test]
fn local_project_branch_sends_the_request_without_the_dependency_prompt(cx: &mut TestAppContext) {
    let (mut board, _dir) = branch_task_modal(cx, None, "tmux");
    forbid_dependency_prompt(&mut board);
    expect_branch_request(
        &mut board,
        "branched",
        r#""main","feat/local-fast","tmux"]"#,
    );
    type_branch_name(&mut board, "feat/local-fast");
    board.click(role_name("button", "분기"));
    assert!(!branch_modal_is_open(&mut board));
}

fn herdr_missing_branch(cx: &mut TestAppContext, is_install_ready: bool) -> Board {
    let (mut board, dir) = branch_task_modal(cx, Some("remote-box"), "herdr");
    std::mem::forget(dir);
    board.add_result_fault(
        "sessionDependency.getSessionDependencyStatus",
        serde_json::json!({"available": false, "toolName": "herdr", "isRemote": true, "sshHost": "remote-box"}),
        Some(r#"["herdr","remote-box"]"#),
    );
    board.add_result_fault(
        "sessionDependency.installSessionDependency",
        serde_json::json!({"available": is_install_ready, "toolName": "herdr", "blockedReason": "no curl"}),
        None,
    );
    expect_branch_request(&mut board, "herdr-task", r#""feature-herdr","herdr"]"#);
    type_branch_name(&mut board, "feature-herdr");
    board.click(role_name("button", "분기"));
    board
}

#[gpui::test]
fn herdr_branch_is_sent_only_after_the_dependency_is_ready(cx: &mut TestAppContext) {
    let mut board = herdr_missing_branch(cx, true);
    assert!(!branch_modal_is_open(&mut board));
}

#[gpui::test]
fn herdr_branch_is_not_sent_when_the_dependency_is_not_ready(cx: &mut TestAppContext) {
    let mut board = herdr_missing_branch(cx, false);
    assert!(branch_modal_is_open(&mut board));
    assert!(board.has_text("herdr 설치에 실패했습니다: no curl"));
}

#[gpui::test]
fn repeated_branch_submit_while_waiting_sends_one_request(cx: &mut TestAppContext) {
    let (mut board, _dir) = branch_task_modal(cx, None, "tmux");
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().unwrap().clone();
        faults.add(ServiceFault {
            target: "kanban.branchFromTask".into(),
            mode: FaultMode::Fail,
            message: Some("duplicate branch request".into()),
            ms: None,
            value: None,
            after_calls: Some(1),
            argument_match: None,
        });
        faults.add(ServiceFault {
            target: "kanban.branchFromTask".into(),
            mode: FaultMode::Hang,
            message: None,
            ms: None,
            value: None,
            after_calls: None,
            argument_match: None,
        });
    });
    type_branch_name(&mut board, "feat/once");
    board.keys("enter");
    board.keys("enter");
    assert!(
        board
            .find(text_contains("duplicate branch request"))
            .is_none()
    );
    assert!(branch_modal_is_open(&mut board));
}

#[gpui::test]
fn closing_while_branching_then_success_shows_an_open_toast(cx: &mut TestAppContext) {
    let (mut board, _dir) = branch_task_modal(cx, None, "tmux");
    board.add_result_fault(
        "kanban.branchFromTask",
        serde_json::json!({"id": "task-1", "delayMs": 1000}),
        None,
    );
    type_branch_name(&mut board, "feat/bg");
    board.click(role_name("button", "분기"));
    board.click(role_name("button", "닫기 (백그라운드에서 계속)"));
    board.advance(Duration::from_millis(1_100));
    assert_eq!(
        board.toast_messages(),
        vec!["\"feat/bg\" 브랜치로 분기했습니다".to_string()]
    );
    board.click(role_name("button", "열기"));
    assert_eq!(board.current_path(), "/ko/task/task-1");
}

#[gpui::test]
fn closing_while_branching_then_failure_shows_a_failure_toast(cx: &mut TestAppContext) {
    let (mut board, _dir) = branch_task_modal(cx, None, "tmux");
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "kanban.branchFromTask".into(),
                mode: FaultMode::Fail,
                message: Some("branch exists".into()),
                ms: Some(1000),
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    type_branch_name(&mut board, "feat/fail");
    board.click(role_name("button", "분기"));
    board.click(role_name("button", "닫기 (백그라운드에서 계속)"));
    board.advance(Duration::from_millis(1_100));
    assert_eq!(
        board.toast_messages(),
        vec![
            "분기하지 못했습니다: Error invoking remote method 'kanvibe:invoke': Error: branch exists"
                .to_string()
        ]
    );
}

/// 프로젝트 두 개(A·B)로 분기 모달을 연다. A 브랜치 응답은 1초 늦게 오고 B는 바로 온다
fn branch_modal_with_slow_first_project(cx: &mut TestAppContext, is_old_failure: bool) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let project_a = project(&database, "alpha", None);
    let project_b = project(&database, "beta", None);
    task(&database, "분기할 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    let old_match = format!(r#"["{}"]"#, project_a.id);
    if is_old_failure {
        board.cx.update(|_, cx| {
            cx.global::<AppServices>()
                .faults()
                .unwrap()
                .add(ServiceFault {
                    target: "project.getProjectBranches".into(),
                    mode: FaultMode::Fail,
                    message: Some("A failed".into()),
                    ms: Some(1000),
                    value: None,
                    after_calls: None,
                    argument_match: Some(old_match.clone()),
                })
        });
    } else {
        board.add_result_fault(
            "project.getProjectBranches",
            serde_json::json!({"delayMs": 1000}),
            Some(&old_match),
        );
    }
    board.add_result_fault(
        "project.getProjectBranches",
        serde_json::json!(["B-only"]),
        Some(&format!(r#"["{}"]"#, project_b.id)),
    );
    board.right_click_text("분기할 작업");
    board.click(role_name("menuitem", "브랜치 분기"));
    board.click(role_name("button", "프로젝트 선택"));
    board.type_text("beta");
    board.keys("enter");
    // 베이스 브랜치 칸을 비워 받아 온 브랜치를 모두 펼친다
    board.keys("backspace backspace backspace backspace");
    assert!(board.has_role_name("button", "B-only"));
    board.advance(Duration::from_millis(1_100));
    board
}

#[gpui::test]
fn late_success_for_the_previous_project_is_ignored(cx: &mut TestAppContext) {
    let mut board = branch_modal_with_slow_first_project(cx, false);
    assert!(board.has_role_name("button", "B-only"));
}

#[gpui::test]
fn late_failure_for_the_previous_project_is_ignored(cx: &mut TestAppContext) {
    let mut board = branch_modal_with_slow_first_project(cx, true);
    assert!(board.has_role_name("button", "B-only"));
    assert!(board.find(text_contains("A failed")).is_none());
}

#[gpui::test]
fn tab_moves_to_the_next_input_while_the_branch_list_is_open(cx: &mut TestAppContext) {
    let (mut board, _database, _dir) = create_task_modal(cx, None, "tmux");
    board.keys("down");
    assert!(board.has_role_name("button", "main"));
    board.keys("tab");
    board.type_text("next");
    let view = board.view.clone();
    let texts = board
        .cx
        .update(|_, cx| view.read(cx).create_task_field_texts(cx));
    assert_eq!(texts.map(|texts| texts[1].clone()).as_deref(), Some("next"));
    assert!(!board.has_role_name("button", "develop"));
}

/// `kanvibe` 프로젝트에서 베이스 브랜치 칸을 비우고 검색어를 넣는다
fn search_branches_with_project_name(cx: &mut TestAppContext, query: &str) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let kanvibe = project(&database, "kanvibe", None);
    task(&database, "기준 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(kanvibe.id.clone());
        task.branch_name = Some("main".into());
    });
    let mut board = open_board(cx, "/ko");
    board.add_result_fault(
        "project.getProjectBranches",
        serde_json::json!(["main", "dev", "feature/search"]),
        None,
    );
    board.right_click_text("기준 작업");
    board.click(role_name("menuitem", "브랜치 기반 TODO 생성"));
    board.keys("backspace backspace backspace backspace");
    board.type_text(query);
    board
}

#[gpui::test]
fn project_then_branch_query_finds_the_branch(cx: &mut TestAppContext) {
    let mut board = search_branches_with_project_name(cx, "kanvibe dev");
    assert!(board.has_role_name("button", "dev"));
    assert!(!board.has_role_name("button", "main"));
}

#[gpui::test]
fn branch_then_project_query_finds_the_branch(cx: &mut TestAppContext) {
    let mut board = search_branches_with_project_name(cx, "dev kanvibe");
    assert!(board.has_role_name("button", "dev"));
    assert!(!board.has_role_name("button", "main"));
}

// ───────────── TaskCard 실행중 AI 세션·변경 집계 ─────────────

fn test_id(id: &str) -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        selector: Some(format!("[data-testid='{id}']")),
        ..DescribedTarget::default()
    })
}

fn test_id_text(id: &str, text: &str) -> TargetQuery {
    TargetQuery::Described(DescribedTarget {
        selector: Some(format!("[data-testid='{id}']")),
        text: Some(text.into()),
        exact: Some(true),
        ..DescribedTarget::default()
    })
}

fn add_fault_before_open(cx: &mut TestAppContext, fault: ServiceFault) {
    cx.update(|cx| cx.global::<AppServices>().faults().unwrap().add(fault));
}

fn result_fault(target: &str, value: serde_json::Value) -> ServiceFault {
    ServiceFault {
        target: target.into(),
        mode: FaultMode::Result,
        message: None,
        ms: None,
        value: Some(value),
        after_calls: None,
        argument_match: None,
    }
}

fn running_pane(provider: &str, window_id: &str) -> serde_json::Value {
    serde_json::json!({
        "provider": provider,
        "worktreePath": "/repo/task",
        "sessionName": "kanvibe-task",
        "windowId": window_id,
        "windowName": provider,
    })
}

/// `/repo/task`에서 일하는 카드(TODO)와 다른 열 카드 하나. `panes`는 보드가 읽을 실행중 pane 목록이다
fn live_session_board(
    cx: &mut TestAppContext,
    worktree_path: &str,
    panes: serde_json::Value,
) -> (Board, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let card = task(&database, "세션 카드", TaskStatus::Todo, |task| {
        task.worktree_path = Some(worktree_path.into());
    });
    task(&database, "옆 카드", TaskStatus::Progress, |_| {});
    add_fault_before_open(cx, result_fault("project.getRunningAgentPanes", panes));
    let mut board = open_board(cx, "/ko");
    board.activate_window();
    (board, card)
}

fn live_sessions_value() -> serde_json::Value {
    serde_json::json!({"sessions": [{
        "provider": "claude",
        "sessionId": "s1",
        "currentTask": "리뷰 반영",
        "state": "running",
        "lastActiveAt": null,
        "runningSubtasks": [],
        "terminalWindow": {"sessionName": "kanvibe-task", "windowId": "@7", "windowName": "claude"},
    }]})
}

#[gpui::test]
fn agents_in_the_same_worktree_show_as_a_card_badge(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([running_pane("claude", "@7")]),
    );
    assert!(board.find(test_id("task-card-running-agents")).is_some());
}

#[gpui::test]
fn each_provider_shows_its_own_running_count(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([
            running_pane("claude", "@7"),
            running_pane("claude", "@8"),
            running_pane("codex", "@9")
        ]),
    );
    assert!(
        board
            .find(test_id_text("task-card-running-claude", "2"))
            .is_some()
    );
    assert!(
        board
            .find(test_id_text("task-card-running-codex", "1"))
            .is_some()
    );
}

#[gpui::test]
fn running_badges_keep_provider_name_order(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([
            running_pane("opencode", "@9"),
            running_pane("claude", "@7"),
            running_pane("codex", "@8")
        ]),
    );
    let x_of = |board: &mut Board, provider: &str| {
        board
            .find(test_id(&format!("task-card-running-{provider}")))
            .unwrap_or_else(|| panic!("{provider} badge"))
            .origin
            .x
    };
    let claude = x_of(&mut board, "claude");
    let codex = x_of(&mut board, "codex");
    let opencode = x_of(&mut board, "opencode");
    assert!(claude < codex && codex < opencode);
}

#[gpui::test]
fn agents_in_another_worktree_are_not_shown(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/other",
        serde_json::json!([running_pane("claude", "@7")]),
    );
    assert!(board.find(test_id("task-card-running-agents")).is_none());
}

/// 키보드로 첫 카드에 포커스를 준다(포커스가 없을 때 ↓는 첫 카드로 들어간다)
/// 시험 창은 활성 창이 아니라서 포커스 이벤트가 오지 않는다. 사용자가 보는 창처럼 먼저 활성으로 만든다
fn focus_first_card(board: &mut Board) {
    board.cx.update(|window, _| window.activate_window());
    board.draw();
    board.keys("down");
}

#[gpui::test]
fn session_panel_opens_only_after_focus_stays_a_moment(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([running_pane("claude", "@7")]),
    );
    focus_first_card(&mut board);
    board.advance(Duration::from_millis(1_000));
    assert!(
        board
            .find(test_id("task-card-live-session-popover"))
            .is_none()
    );
    board.advance(Duration::from_millis(500));
    assert!(
        board
            .find(test_id("task-card-live-session-popover"))
            .is_some()
    );
}

#[gpui::test]
fn open_session_panel_is_not_clipped_by_the_card(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([running_pane("claude", "@7")]),
    );
    focus_first_card(&mut board);
    board.advance(Duration::from_millis(1_500));
    let popover = board
        .cx
        .update(|window, cx| {
            cx.global::<QaTargetRegistry>().find_hit(
                window.window_handle().window_id(),
                &test_id("task-card-live-session-popover"),
            )
        })
        .expect("popover");
    let card = board.find(role_name("article", "세션 카드")).expect("card");
    assert!(popover.bounds.origin.y >= card.origin.y + card.size.height);
    assert!(popover.clip_bounds.size.height >= popover.bounds.size.height);
}

#[gpui::test]
fn moving_focus_off_the_card_closes_the_session_panel(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([running_pane("claude", "@7")]),
    );
    focus_first_card(&mut board);
    board.advance(Duration::from_millis(1_500));
    assert!(
        board
            .find(test_id("task-card-live-session-popover"))
            .is_some()
    );
    board.keys("right");
    assert!(
        board
            .find(test_id("task-card-live-session-popover"))
            .is_none()
    );
}

#[gpui::test]
fn closed_panel_does_not_read_sessions(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([running_pane("claude", "@7")]),
    );
    // 두 번째 조회부터는 다른 세션을 돌려준다. 닫혀 있는 동안 읽었다면 열자마자 두 번째 응답이 보인다
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().unwrap().clone();
        let mut later = result_fault(
            "project.getTaskLiveAiSessions",
            serde_json::json!({"sessions": [{"provider": "codex", "sessionId": null, "currentTask": "두 번째 조회", "state": "idle", "lastActiveAt": null, "runningSubtasks": [], "terminalWindow": null}]}),
        );
        later.after_calls = Some(1);
        faults.add(later);
        faults.add(result_fault("project.getTaskLiveAiSessions", live_sessions_value()));
    });
    board.advance(Duration::from_secs(12));
    focus_first_card(&mut board);
    board.advance(Duration::from_millis(1_300));
    assert!(board.has_role_name("button", "리뷰 반영"));
    assert!(!board.has_text("두 번째 조회"));
}

#[gpui::test]
fn clicking_a_session_selects_its_window_then_opens_the_task(cx: &mut TestAppContext) {
    let (mut board, card) = live_session_board(
        cx,
        "/repo/task",
        serde_json::json!([running_pane("claude", "@7")]),
    );
    board.add_result_fault("project.getTaskLiveAiSessions", live_sessions_value(), None);
    board.add_result_fault(
        "terminalTabs.selectTerminalTab",
        serde_json::json!({"ok": true}),
        Some(&format!(r#"["{}","@7"]"#, card.id)),
    );
    focus_first_card(&mut board);
    board.advance(Duration::from_millis(1_500));
    board.click(role_name("button", "리뷰 반영"));
    assert_eq!(board.current_path(), format!("/ko/task/{}", card.id));
}

#[gpui::test]
fn open_panel_without_anything_running_draws_no_popover(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(cx, "/repo/task", serde_json::json!([]));
    board.add_result_fault(
        "project.getTaskLiveAiSessions",
        serde_json::json!({"sessions": []}),
        None,
    );
    focus_first_card(&mut board);
    board.advance(Duration::from_millis(1_500));
    assert!(
        board
            .find(test_id("task-card-live-session-popover"))
            .is_none()
    );
}

#[gpui::test]
fn no_agents_and_no_open_panel_draw_nothing(cx: &mut TestAppContext) {
    let (mut board, _card) = live_session_board(cx, "/repo/task", serde_json::json!([]));
    assert!(board.find(test_id("task-card-running-agents")).is_none());
    assert!(
        board
            .find(test_id("task-card-live-session-popover"))
            .is_none()
    );
}

/// 진행 중 카드 하나와 그 집계. 보드가 5초마다 다시 읽는 집계는 `next`로 바뀐다
fn diff_stats_board(
    cx: &mut TestAppContext,
    first: serde_json::Value,
    is_motion_reduced: bool,
) -> (Board, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    cx.update(|cx| {
        cx.set_global(kanvibe_app::views::board::MotionPreference {
            is_reduced: is_motion_reduced,
        })
    });
    let card = task(&database, "집계 카드", TaskStatus::Progress, |task| {
        task.worktree_path = Some("/repo/stats__worktrees/a".into());
    });
    let value = serde_json::json!({ card.id.clone(): first });
    add_fault_before_open(cx, {
        let mut fault = result_fault("diff.getTaskDiffStats", value);
        fault.after_calls = Some(0);
        fault
    });
    let mut board = open_board(cx, "/ko");
    board.activate_window();
    (board, card)
}

fn stats(files: i64, additions: i64, deletions: i64) -> serde_json::Value {
    serde_json::json!({"fileCount": files, "additions": additions, "deletions": deletions})
}

/// 다음 주기 응답을 바꾼다
fn next_diff_stats(board: &mut Board, card: &KanbanTask, next: serde_json::Value) {
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().unwrap().clone();
        faults.clear("diff.getTaskDiffStats");
        faults.add(result_fault(
            "diff.getTaskDiffStats",
            serde_json::json!({ card.id.clone(): next }),
        ));
    });
}

#[gpui::test]
fn diff_badge_shows_file_count_and_line_changes(cx: &mut TestAppContext) {
    let (mut board, card) = diff_stats_board(cx, stats(0, 0, 0), false);
    next_diff_stats(&mut board, &card, stats(12, 348, 76));
    board.advance(Duration::from_millis(5_100));
    board.advance(Duration::from_millis(1_000));
    assert!(
        board
            .find(test_id_text("task-diff-file-count", "12"))
            .is_some()
    );
    assert!(
        board
            .find(test_id_text("task-diff-additions", "348"))
            .is_some()
    );
    assert!(
        board
            .find(test_id_text("task-diff-deletions", "76"))
            .is_some()
    );
}

#[gpui::test]
fn tasks_without_changes_have_no_diff_badge(cx: &mut TestAppContext) {
    let (mut board, _card) = diff_stats_board(cx, stats(0, 0, 0), false);
    board.advance(Duration::from_millis(5_100));
    assert!(board.find(test_id("task-diff-file-count")).is_none());
}

#[gpui::test]
fn tasks_without_received_stats_have_no_diff_badge(cx: &mut TestAppContext) {
    let (mut board, _card) = diff_stats_board(cx, serde_json::json!(null), false);
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().unwrap().clone();
        faults.clear("diff.getTaskDiffStats");
        faults.add(result_fault("diff.getTaskDiffStats", serde_json::json!({})));
    });
    board.advance(Duration::from_millis(5_100));
    assert!(board.find(test_id("task-diff-file-count")).is_none());
}

/// 첫 주기(5초)에 340을 받고, 두 번째 주기(10초) 직전까지 간다. 두 번째 주기는 348을 돌려준다
fn counting_board(cx: &mut TestAppContext, is_motion_reduced: bool) -> Board {
    let (mut board, card) = diff_stats_board(cx, stats(3, 340, 2), is_motion_reduced);
    board.advance(Duration::from_millis(5_100));
    next_diff_stats(&mut board, &card, stats(3, 348, 2));
    board.advance(Duration::from_millis(4_850));
    board
}

#[gpui::test]
fn first_value_is_shown_without_counting(cx: &mut TestAppContext) {
    let (mut board, _card) = diff_stats_board(cx, stats(1, 0, 0), false);
    board.advance(Duration::from_millis(5_100));
    assert!(
        board
            .find(test_id_text("task-diff-file-count", "1"))
            .is_some()
    );
}

#[gpui::test]
fn changed_value_counts_up_and_reaches_the_new_value(cx: &mut TestAppContext) {
    let mut board = counting_board(cx, false);
    let mut has_seen_between = false;
    for _ in 0..12 {
        board.advance(Duration::from_millis(50));
        has_seen_between |= (341..348).any(|value| {
            board
                .find(test_id_text("task-diff-additions", &value.to_string()))
                .is_some()
        });
    }
    assert!(has_seen_between, "중간 값을 지나야 한다");
    assert!(
        board
            .find(test_id_text("task-diff-additions", "348"))
            .is_some()
    );
}

#[gpui::test]
fn reduced_motion_jumps_straight_to_the_new_value(cx: &mut TestAppContext) {
    let mut board = counting_board(cx, true);
    board.advance(Duration::from_millis(100));
    assert!(
        board
            .find(test_id_text("task-diff-additions", "348"))
            .is_some()
    );
}

/// 배지가 많은 카드 하나(PR·agent·세션·원격·우선순위)
fn badge_heavy_board(cx: &mut TestAppContext, ssh_host: &str) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    task(&database, "배지 많은 작업", TaskStatus::Todo, |task| {
        task.session_type = Some(SessionType::Tmux);
        task.ssh_host = Some(ssh_host.into());
        task.pr_url = Some("https://github.com/example/pull/1".into());
        task.agent_type = Some("claude".into());
        task.priority = Some(TaskPriority::High);
    });
    open_board(cx, "/ko")
}

fn contains(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.origin.x + inner.size.width
            <= outer.origin.x + outer.size.width + gpui_kit::px(0.5)
        && inner.origin.y + inner.size.height
            <= outer.origin.y + outer.size.height + gpui_kit::px(0.5)
}

#[gpui::test]
fn overflowing_badges_flow_to_the_next_line(cx: &mut TestAppContext) {
    let mut board = badge_heavy_board(cx, "devbox-remote-build-host");
    let card = board.find(role_name("article", "배지 많은 작업")).unwrap();
    let pr = board.find(TargetQuery::ExactText("PR".into())).unwrap();
    let priority = board.find(TargetQuery::ExactText("P1".into())).unwrap();
    assert!(
        priority.origin.y > pr.origin.y,
        "우선순위 배지는 다음 줄로 흐른다"
    );
    for badge in [pr, priority] {
        assert!(contains(card, badge), "배지가 카드 안에 다 보인다");
    }
}

#[gpui::test]
fn long_remote_host_wraps_inside_its_badge(cx: &mut TestAppContext) {
    let host = "very-long-remote-development-host.internal.example.company.net";
    let mut board = badge_heavy_board(cx, host);
    let card = board.find(role_name("article", "배지 많은 작업")).unwrap();
    let ssh = board.find(TargetQuery::ExactText(host.into())).unwrap();
    assert!(contains(card, ssh), "호스트 배지가 카드 안에 다 보인다");
    assert!(
        f32::from(ssh.size.height) > 20.0,
        "한 줄을 넘기면 배지 안에서 줄을 바꾼다"
    );
}

fn window_count(board: &mut Board) -> usize {
    board.cx.update(|_, cx| cx.windows().len())
}

#[gpui::test]
fn shift_enter_on_a_card_opens_the_detail_in_a_new_window(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "새 창 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    let before = window_count(&mut board);
    board.keys("down");
    board.keys("shift-enter");
    assert_eq!(window_count(&mut board), before + 1);
    assert_eq!(board.current_path(), "/ko");
}

#[gpui::test]
fn clicking_a_card_opens_the_detail_in_the_same_window(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "같은 창 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    let before = window_count(&mut board);
    board.click(role_name("link", "같은 창 작업"));
    assert_eq!(board.current_path(), format!("/ko/task/{}", card.id));
    assert_eq!(window_count(&mut board), before);
}

#[gpui::test]
fn clicking_a_card_focuses_the_window_already_showing_it(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    // 프로젝트 없는 작업이라 상세 창이 세션 연결을 시작하지 않는다
    let card = task(&database, "이미 열린 작업", TaskStatus::Todo, |_| {});
    let detail =
        cx.add_window(|window, cx| AppWindow::new(&format!("/ko/task/{}", card.id), window, cx));
    cx.run_until_parked();
    let mut board = open_board(cx, "/ko");
    board.click(role_name("link", "이미 열린 작업"));
    assert_eq!(board.current_path(), "/ko");
    let active = board.cx.update(|_, cx| cx.active_window());
    assert_eq!(
        active.map(|handle| handle.window_id()),
        Some(detail.window_id())
    );
}

#[gpui::test]
fn diff_stats_poll_sends_the_tasks_to_refresh_in_one_request(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let in_branch = |path: &'static str| {
        move |task: &mut NewKanbanTask| {
            task.branch_name = Some(path.into());
            task.worktree_path = Some(format!("/repo__worktrees/{path}"));
        }
    };
    let first = task(&database, "첫 진행", TaskStatus::Progress, in_branch("a"));
    let second = task(&database, "둘째 진행", TaskStatus::Progress, in_branch("b"));
    let mut board = open_board(cx, "/ko");
    board.activate_window();
    let mut ids = [first.id.clone(), second.id.clone()];
    ids.sort();
    let both = [
        format!(r#"["{}","{}"]"#, ids[0], ids[1]),
        format!(r#"["{}","{}"]"#, ids[1], ids[0]),
    ];
    for argument_match in &both {
        board.add_result_fault(
            "diff.getTaskDiffStats",
            serde_json::json!({ first.id.clone(): stats(1, 340, 0) }),
            Some(argument_match),
        );
    }
    board.advance(Duration::from_millis(5_100));
    assert!(
        board
            .find(test_id_text("task-diff-additions", "340"))
            .is_some()
    );
}

#[gpui::test]
fn diff_stats_poll_reads_stored_stats_without_tasks_to_refresh(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "할 일", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.activate_window();
    board.add_result_fault(
        "diff.getTaskDiffStats",
        serde_json::json!({ card.id.clone(): stats(2, 340, 1) }),
        Some("[]"),
    );
    board.advance(Duration::from_millis(5_100));
    assert!(
        board
            .find(test_id_text("task-diff-additions", "340"))
            .is_some()
    );
}

// ───────────── NotificationCenterButton 열기·실패·검토 ─────────────

fn task_notification(
    id: &str,
    task_id: &str,
    created_at: &str,
    is_read: bool,
) -> serde_json::Value {
    serde_json::json!({
        "id": id, "title": format!("{id} title"), "body": format!("{id} body"), "taskId": task_id,
        "relativePath": format!("/task/{task_id}"), "locale": "ko", "isRead": is_read,
        "createdAt": created_at, "dedupeKey": id
    })
}

fn review_notification_value(merged_tasks: &[&KanbanTask]) -> serde_json::Value {
    let merged: Vec<serde_json::Value> = merged_tasks
        .iter()
        .map(|task| {
            serde_json::json!({
                "taskId": task.id, "taskTitle": task.title, "branchName": "feature/merged",
                "prUrl": "https://github.com/test/repo/pull/1", "mergedAt": "2026-01-15T02:00:00.000Z"
            })
        })
        .collect();
    serde_json::json!({
        "id": "review", "title": "Background sync review", "body": "검토할 항목", "taskId": null,
        "relativePath": "/ko", "locale": "ko", "isRead": false,
        "createdAt": "2026-01-15T02:58:00.000Z", "dedupeKey": "review",
        "action": {"type": "background-sync-review", "payload": {
            "mergedPullRequests": merged, "registeredWorktrees": []
        }}
    })
}

fn store_notifications(database: &Database, notifications: serde_json::Value) {
    save_app_setting(
        &database.connection(),
        "app_notifications",
        &notifications.to_string(),
    )
    .unwrap();
}

fn open_notification_panel(board: &mut Board) {
    board.cx.update(|window, cx| {
        window.dispatch_action(
            Box::new(kanvibe_app::shortcuts::ToggleNotificationCenter),
            cx,
        )
    });
    board.draw();
}

/// 작업 하나와 그 작업 알림 하나가 있는 보드
fn single_notification_board(cx: &mut TestAppContext) -> (Board, Arc<Database>, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let card = task(&database, "알림 대상", TaskStatus::Todo, |_| {});
    store_notifications(
        &database,
        serde_json::json!([task_notification(
            "n1",
            &card.id,
            "2026-01-15T02:57:00.000Z",
            false
        )]),
    );
    let board = open_board(cx, "/ko");
    (board, database, card)
}

#[gpui::test]
fn unreadable_notifications_show_the_reason_and_retry_loads_them(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "알림 대상", TaskStatus::Todo, |_| {});
    store_notifications(
        &database,
        serde_json::json!([task_notification(
            "n1",
            &card.id,
            "2026-01-15T02:57:00.000Z",
            false
        )]),
    );
    add_fault_before_open(
        cx,
        ServiceFault {
            target: "kanvibe:notifications-list".into(),
            mode: FaultMode::Fail,
            message: Some("db closed".into()),
            ms: None,
            value: None,
            after_calls: None,
            argument_match: None,
        },
    );
    let mut board = open_board(cx, "/ko");
    open_notification_panel(&mut board);
    assert!(board.find(text_contains("db closed")).is_some());
    assert!(!board.has_text("아직 알림이 없습니다."));
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanvibe:notifications-list")
    });
    board.click(role_name("button", "다시 시도"));
    assert!(board.has_role_name("button", "n1 body"));
    assert!(board.find(text_contains("db closed")).is_none());
}

#[gpui::test]
fn opening_notification_is_not_repeated_and_failure_is_toasted(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = single_notification_board(cx);
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().unwrap().clone();
        faults.add(ServiceFault {
            target: "kanvibe:notifications-mark-read".into(),
            mode: FaultMode::Fail,
            message: Some("db locked".into()),
            ms: Some(1000),
            value: None,
            after_calls: None,
            argument_match: None,
        });
    });
    open_notification_panel(&mut board);
    board.click(role_name("button", "n1 body"));
    board.click(role_name("button", "n1 body"));
    board.advance(Duration::from_millis(1_100));
    assert_eq!(
        board.toast_messages(),
        vec![
            "알림을 열지 못했습니다: Error invoking remote method 'kanvibe:notifications-mark-read': Error: db locked"
                .to_string()
        ]
    );
    assert_eq!(board.current_path(), "/ko");
}

#[gpui::test]
fn task_notification_navigates_when_the_task_exists(cx: &mut TestAppContext) {
    let (mut board, _database, card) = single_notification_board(cx);
    open_notification_panel(&mut board);
    board.click(role_name("button", "n1 body"));
    assert_eq!(board.current_path(), format!("/ko/task/{}", card.id));
}

#[gpui::test]
fn task_notification_focuses_the_window_already_showing_the_task(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let card = task(&database, "알림 대상", TaskStatus::Todo, |_| {});
    store_notifications(
        &database,
        serde_json::json!([task_notification(
            "n1",
            &card.id,
            "2026-01-15T02:57:00.000Z",
            false
        )]),
    );
    let detail =
        cx.add_window(|window, cx| AppWindow::new(&format!("/ko/task/{}", card.id), window, cx));
    cx.run_until_parked();
    let mut board = open_board(cx, "/ko");
    open_notification_panel(&mut board);
    board.click(role_name("button", "n1 body"));
    assert_eq!(board.current_path(), "/ko");
    let active = board.cx.update(|_, cx| cx.active_window());
    assert_eq!(
        active.map(|handle| handle.window_id()),
        Some(detail.window_id())
    );
}

#[gpui::test]
fn shift_click_opens_the_task_notification_in_a_new_window(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = single_notification_board(cx);
    open_notification_panel(&mut board);
    let before = window_count(&mut board);
    let row = board.find(role_name("button", "n1 body")).unwrap();
    board.cx.simulate_click(row.center(), Modifiers::shift());
    board.draw();
    assert_eq!(window_count(&mut board), before + 1);
    assert_eq!(board.current_path(), "/ko");
}

#[gpui::test]
fn shift_enter_opens_the_highlighted_notification_in_a_new_window(cx: &mut TestAppContext) {
    let (mut board, _database, _card) = single_notification_board(cx);
    open_notification_panel(&mut board);
    let before = window_count(&mut board);
    board.keys("shift-enter");
    assert_eq!(window_count(&mut board), before + 1);
    assert_eq!(board.current_path(), "/ko");
}

#[gpui::test]
fn unread_only_keyboard_navigation_skips_read_notifications(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let newest = task(&database, "최신", TaskStatus::Todo, |_| {});
    let middle = task(&database, "읽은 것", TaskStatus::Todo, |_| {});
    let oldest = task(&database, "오래된 것", TaskStatus::Todo, |_| {});
    store_notifications(
        &database,
        serde_json::json!([
            task_notification(
                "unread-newest",
                &newest.id,
                "2026-01-15T02:57:00.000Z",
                false
            ),
            task_notification("read-middle", &middle.id, "2026-01-15T02:50:00.000Z", true),
            task_notification(
                "unread-oldest",
                &oldest.id,
                "2026-01-15T02:40:00.000Z",
                false
            ),
        ]),
    );
    save_app_setting(&database.connection(), "notification_unread_only", "true").unwrap();
    let mut board = open_board(cx, "/ko");
    open_notification_panel(&mut board);
    assert!(!board.has_role_name("button", "read-middle body"));
    board.keys("down enter");
    assert_eq!(board.current_path(), format!("/ko/task/{}", oldest.id));
}

/// merge된 PR 작업과 그 검토 알림. 알림 센터를 열어 둔다
fn review_board(
    cx: &mut TestAppContext,
    titles: &[&str],
) -> (Board, Arc<Database>, Vec<KanbanTask>) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let tasks: Vec<KanbanTask> = titles
        .iter()
        .map(|title| task(&database, title, TaskStatus::Review, |_| {}))
        .collect();
    let refs: Vec<&KanbanTask> = tasks.iter().collect();
    store_notifications(
        &database,
        serde_json::json!([review_notification_value(&refs)]),
    );
    let mut board = open_board(cx, "/ko");
    open_notification_panel(&mut board);
    (board, database, tasks)
}

fn delete_fault(
    task: &KanbanTask,
    mode: FaultMode,
    message: Option<&str>,
    ms: Option<u64>,
) -> ServiceFault {
    ServiceFault {
        target: "kanban.deleteTask".into(),
        mode,
        message: message.map(str::to_string),
        ms,
        value: None,
        after_calls: None,
        argument_match: Some(format!(r#"["{}"]"#, task.id)),
    }
}

fn add_faults(board: &mut Board, faults: Vec<ServiceFault>) {
    board.cx.update(|_, cx| {
        let registry = cx.global::<AppServices>().faults().unwrap().clone();
        faults.into_iter().for_each(|fault| registry.add(fault));
    });
}

fn task_exists(database: &Database, task: &KanbanTask) -> bool {
    find_task(&database.connection(), &task.id)
        .unwrap()
        .is_some()
}

fn review_dialog_is_open(board: &mut Board) -> bool {
    board.has_role_name("dialog", "백그라운드 sync 검토")
}

#[gpui::test]
fn review_notification_opens_the_review_dialog_without_navigating(cx: &mut TestAppContext) {
    let (mut board, _database, _tasks) = review_board(cx, &["Merged task"]);
    board.click(role_name("button", "검토할 항목"));
    assert!(review_dialog_is_open(&mut board));
    assert!(board.has_text("Merged task"));
    assert_eq!(board.current_path(), "/ko");
}

#[gpui::test]
fn deleting_keeps_the_review_and_announces_progress(cx: &mut TestAppContext) {
    let (mut board, database, tasks) = review_board(cx, &["task-a", "task-b"]);
    add_faults(
        &mut board,
        vec![delete_fault(&tasks[0], FaultMode::Delay, None, Some(1000))],
    );
    board.click(role_name("button", "검토할 항목"));
    board.click(role_name("button", "백그라운드에서 삭제"));
    assert!(!review_dialog_is_open(&mut board));
    open_notification_panel(&mut board);
    board.click(role_name("button", "정리 중"));
    assert!(board.has_role_name("status", "2개 중 0개 처리 중…"));
    assert!(board.has_text(
        "삭제는 백그라운드에서 처리됩니다. 이 창을 닫아도 계속 진행되며, 정리 중 버튼으로 다시 확인할 수 있습니다."
    ));
    board.keys("escape");
    board.advance(Duration::from_millis(1_100));
    assert!(!review_dialog_is_open(&mut board));
    assert!(!task_exists(&database, &tasks[0]));
    assert!(!task_exists(&database, &tasks[1]));
}

/// 세 작업 가운데 두 번째 삭제만 실패하게 지운다
fn delete_with_one_failure(cx: &mut TestAppContext) -> (Board, Arc<Database>, Vec<KanbanTask>) {
    let (mut board, database, tasks) = review_board(cx, &["task-a", "task-b", "task-c"]);
    add_faults(
        &mut board,
        vec![delete_fault(
            &tasks[1],
            FaultMode::Fail,
            Some("worktree busy"),
            None,
        )],
    );
    board.click(role_name("button", "검토할 항목"));
    board.click(role_name("button", "백그라운드에서 삭제"));
    board.draw();
    (board, database, tasks)
}

#[gpui::test]
fn partial_failure_keeps_results_and_retries_only_failed_tasks(cx: &mut TestAppContext) {
    let (mut board, database, tasks) = delete_with_one_failure(cx);
    open_notification_panel(&mut board);
    board.click(role_name("button", "탐지 항목 1개"));
    assert!(board.has_role_name("alert", "3개 중 2개 삭제, 1개 실패"));
    assert!(board.find(text_contains("worktree busy")).is_some());
    assert!(board.has_role_name("status", "삭제 실패"));
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanban.deleteTask")
    });
    board.click(role_name("button", "실패한 항목 다시 시도"));
    assert!(!review_dialog_is_open(&mut board));
    assert!(!task_exists(&database, &tasks[1]));
}

#[gpui::test]
fn starting_cleanup_closes_the_dialog_and_reopening_keeps_it_running(cx: &mut TestAppContext) {
    let (mut board, database, tasks) = review_board(cx, &["a", "b"]);
    // 같은 작업을 두 번째로 지우려 하면 실패하게 둔다. 정리 중 다시 눌러도 요청이 한 번뿐이면 실패가 없다
    let mut duplicate = delete_fault(&tasks[0], FaultMode::Fail, Some("duplicate"), None);
    duplicate.after_calls = Some(1);
    add_faults(
        &mut board,
        vec![
            duplicate,
            delete_fault(&tasks[0], FaultMode::Delay, None, Some(1000)),
        ],
    );
    board.click(role_name("button", "검토할 항목"));
    board.click(role_name("button", "백그라운드에서 삭제"));
    assert!(!review_dialog_is_open(&mut board));
    open_notification_panel(&mut board);
    board.click(role_name("button", "정리 중"));
    assert!(review_dialog_is_open(&mut board));
    if board.has_role_name("button", "백그라운드에서 삭제") {
        board.click(role_name("button", "백그라운드에서 삭제"));
    }
    board.keys("escape");
    board.advance(Duration::from_millis(1_100));
    open_notification_panel(&mut board);
    assert!(!board.has_role_name("button", "정리 중"));
    assert!(board.has_role_name("button", "탐지 항목 0개"));
    assert!(!task_exists(&database, &tasks[0]) && !task_exists(&database, &tasks[1]));
}

#[gpui::test]
fn failed_cleanup_is_kept_for_reopening_and_retry_clears_it(cx: &mut TestAppContext) {
    let (mut board, database, tasks) = review_board(cx, &["a", "b"]);
    assert!(board.has_role_name("button", "탐지 항목 2개"));
    add_faults(
        &mut board,
        vec![delete_fault(&tasks[1], FaultMode::Fail, Some("busy"), None)],
    );
    board.click(role_name("button", "검토할 항목"));
    board.click(role_name("button", "백그라운드에서 삭제"));
    open_notification_panel(&mut board);
    board.click(role_name("button", "탐지 항목 1개"));
    assert!(board.find(text_contains("Error: busy")).is_some());
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("kanban.deleteTask")
    });
    board.click(role_name("button", "실패한 항목 다시 시도"));
    assert!(!task_exists(&database, &tasks[1]));
    open_notification_panel(&mut board);
    assert!(board.has_role_name("button", "탐지 항목 0개"));
}

// ───────────── ProjectSelector ─────────────

/// 이름순으로 정렬되는 프로젝트 `count`개와 각 프로젝트 카드
fn many_projects_board(cx: &mut TestAppContext, count: usize) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    for index in 0..count {
        let created = project(&database, &format!("project-{index:02}"), None);
        task(
            &database,
            &format!("작업 {index:02}"),
            TaskStatus::Todo,
            |task| {
                task.project_id = Some(created.id.clone());
            },
        );
    }
    open_board(cx, "/ko")
}

/// 목록에서 보이는 칸 안에 그 항목이 다 들어와 있는지
fn is_option_visible(board: &mut Board, name: &str) -> bool {
    let hit = board.cx.update(|window, cx| {
        cx.global::<QaTargetRegistry>().find_hit(
            window.window_handle().window_id(),
            &role_name("option", name),
        )
    });
    hit.is_some_and(|hit| contains(hit.clip_bounds, hit.bounds))
}

#[gpui::test]
fn three_or_more_selected_projects_show_two_chips_and_a_count(cx: &mut TestAppContext) {
    let mut board = many_projects_board(cx, 5);
    board.click_text("전체 프로젝트");
    for index in 0..5 {
        board.click(role_name("option", &format!("project-{index:02}")));
    }
    board.keys("escape");
    assert!(board.has_text("+3"));
    let view = board.view.clone();
    let (chips, hidden_count) = board
        .cx
        .update(|_, cx| view.read(cx).selected_project_chips());
    assert_eq!(chips.len(), 2);
    assert_eq!(hidden_count, 3);
}

#[gpui::test]
fn multi_select_keyboard_highlight_scrolls_into_view(cx: &mut TestAppContext) {
    let mut board = many_projects_board(cx, 20);
    board.click_text("전체 프로젝트");
    assert!(!is_option_visible(&mut board, "project-15"));
    for _ in 0..15 {
        board.keys("down");
    }
    assert!(is_option_visible(&mut board, "project-15"));
}

#[gpui::test]
fn single_select_keyboard_highlight_scrolls_into_view(cx: &mut TestAppContext) {
    let mut board = many_projects_board(cx, 20);
    board.click_text("+ 새 작업");
    board.keys("enter");
    assert!(!is_option_visible(&mut board, "project-15"));
    for _ in 0..15 {
        board.keys("down");
    }
    assert!(is_option_visible(&mut board, "project-15"));
}

#[gpui::test]
fn single_select_trigger_takes_keyboard_focus(cx: &mut TestAppContext) {
    let mut board = many_projects_board(cx, 2);
    board.click_text("+ 새 작업");
    // 모달이 열리면 첫 입력인 프로젝트 선택 버튼이 포커스를 받고, Enter로 목록을 연다
    board.keys("enter");
    assert!(board.has_role_name("option", "project-00"));
    assert!(board.has_role_name("option", "project-01"));
}

// ───────────── WorkspaceTaskList 목록 포커스·선택 요청·프로젝트 표식 ─────────────

/// 로그인·결제 두 작업이 있는 워크스페이스. 로그인을 고른 채 연다
fn login_billing_workspace(cx: &mut TestAppContext) -> (Board, KanbanTask, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let login = task(&database, "Login page", TaskStatus::Todo, |_| {});
    let billing = task(&database, "Billing export", TaskStatus::Todo, |_| {});
    let board = open_board(cx, &format!("/ko?taskId={}", login.id));
    (board, login, billing)
}

fn selected_workspace_task(board: &mut Board) -> Option<String> {
    board
        .view
        .read_with(&board.cx, |view, _| view.selected_workspace_task_id())
}

/// `Ctrl+Alt`을 눌렀다가 Alt, Ctrl 순서로 뗀다(Linux의 `Mod+Alt` 단독 입력)
fn release_mod_alt(board: &mut Board) {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    let ctrl_alt = Modifiers {
        control: true,
        alt: true,
        ..Modifiers::default()
    };
    for modifiers in [ctrl, ctrl_alt, ctrl, Modifiers::default()] {
        board.cx.simulate_modifiers_change(modifiers);
    }
    board.draw();
}

fn click_workspace_search(board: &mut Board) {
    let search = board
        .find(TargetQuery::Described(DescribedTarget {
            role: Some("textbox".into()),
            ..DescribedTarget::default()
        }))
        .expect("workspace search");
    board
        .cx
        .simulate_click(search.center(), Modifiers::default());
    board.draw();
}

#[gpui::test]
fn mod_alt_release_focuses_the_selected_list_card(cx: &mut TestAppContext) {
    let (mut board, login, billing) = login_billing_workspace(cx);
    click_workspace_search(&mut board);
    release_mod_alt(&mut board);
    assert_eq!(selected_workspace_task(&mut board), Some(login.id));
    // 목록에 포커스가 있으면 ↓가 다음 작업을 고른다
    board.keys("down");
    assert_eq!(selected_workspace_task(&mut board), Some(billing.id));
}

#[gpui::test]
fn hidden_selection_makes_the_focus_shortcut_select_the_first_listed_task(cx: &mut TestAppContext) {
    let (mut board, _login, billing) = login_billing_workspace(cx);
    click_workspace_search(&mut board);
    board.type_text("Billing");
    assert!(!board.has_role_name("current", "Login page"));
    release_mod_alt(&mut board);
    assert_eq!(selected_workspace_task(&mut board), Some(billing.id));
}

#[gpui::test]
fn searching_the_same_task_again_shows_the_hidden_selection(cx: &mut TestAppContext) {
    let (mut board, _login, _billing) = login_billing_workspace(cx);
    click_workspace_search(&mut board);
    board.type_text("Billing");
    assert!(!board.has_role_name("current", "Login page"));
    board.dispatch(OpenTaskSearch);
    board.type_text("Login");
    board.keys("enter");
    assert!(board.has_role_name("current", "Login page"));
}

#[gpui::test]
fn list_focus_requests_are_ignored_while_capturing_shortcuts(cx: &mut TestAppContext) {
    let (mut board, login, _billing) = login_billing_workspace(cx);
    click_workspace_search(&mut board);
    board.cx.update(|window, cx| {
        kanvibe_app::shortcuts::begin_shortcut_capture(window.window_handle().window_id(), cx)
    });
    release_mod_alt(&mut board);
    board.keys("down");
    assert_eq!(selected_workspace_task(&mut board), Some(login.id.clone()));
    board.cx.update(|window, cx| {
        kanvibe_app::shortcuts::end_shortcut_capture(window.window_handle().window_id(), cx)
    });
    release_mod_alt(&mut board);
    board.keys("down");
    assert_ne!(selected_workspace_task(&mut board), Some(login.id));
}

#[gpui::test]
fn projects_without_color_or_icon_show_the_initial_marker(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    let api = project(&database, "api", None);
    task(&database, "Login page", TaskStatus::Todo, |task| {
        task.project_id = Some(api.id.clone())
    });
    let mut board = open_board(cx, "/ko");
    assert!(
        board
            .find(test_id_text("project-initial-icon", "A"))
            .is_some()
    );
}

#[gpui::test]
fn worktree_project_cards_use_the_main_project_name_and_icon(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    insert_project(
        &database.connection(),
        &NewProject {
            name: "api".into(),
            repo_path: "/repos/api".into(),
            default_branch: "main".into(),
            ssh_host: None,
            is_worktree: false,
            color: Some("#0064ff".into()),
            icon_data_url: Some("data:image/png;base64,aGVsbG8=".into()),
        },
    )
    .unwrap();
    let worktree = insert_project(
        &database.connection(),
        &NewProject {
            name: "api-worktree".into(),
            repo_path: "/repos/api__worktrees/feature".into(),
            default_branch: "main".into(),
            ssh_host: None,
            is_worktree: true,
            color: None,
            icon_data_url: None,
        },
    )
    .unwrap();
    task(&database, "Worktree task", TaskStatus::Todo, |task| {
        task.project_id = Some(worktree.id.clone())
    });
    let mut board = open_board(cx, "/ko");
    assert!(board.has_role_name("text", "api"));
    assert!(!board.has_role_name("text", "api-worktree"));
    assert!(board.has_role_name("img", "api icon"));
}

// ───────────── TaskWorkspaceRoute 완료 작업 더 보기 ─────────────

/// 완료 작업 60개가 있는 워크스페이스
fn many_done_workspace(cx: &mut TestAppContext) -> Board {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    for index in 0..60 {
        task(
            &database,
            &format!("Completed {index:02}"),
            TaskStatus::Done,
            |_| {},
        );
    }
    open_board(cx, "/ko")
}

fn listed_done_count(board: &mut Board) -> usize {
    (0..60)
        .filter(|index| board.has_role_name("link", &format!("Completed {index:02}")))
        .count()
}

/// 같은 페이지를 두 번 달라고 하면 실패하게 둔다
fn forbid_repeated_page(board: &mut Board, offset: usize) {
    add_faults(
        board,
        vec![ServiceFault {
            target: "kanban.getMoreDoneTasks".into(),
            mode: FaultMode::Fail,
            message: Some(format!("duplicate page {offset}")),
            ms: None,
            value: None,
            after_calls: Some(1),
            argument_match: Some(format!("[{offset}]")),
        }],
    );
}

/// 목록 끝의 더 보기 버튼까지 한 화면에 들어오게 창을 늘린다
fn show_whole_workspace_list(board: &mut Board) {
    board
        .cx
        .simulate_resize(gpui_kit::size(gpui_kit::px(1600.0), gpui_kit::px(8000.0)));
    board.draw();
}

#[gpui::test]
fn repeated_load_more_requests_the_next_page_once_without_skipping(cx: &mut TestAppContext) {
    let mut board = many_done_workspace(cx);
    show_whole_workspace_list(&mut board);
    assert_eq!(listed_done_count(&mut board), 20);
    forbid_repeated_page(&mut board, 20);
    let button = board
        .find(role_name("button", "완료된 작업 더 보기"))
        .unwrap();
    board
        .cx
        .simulate_click(button.center(), Modifiers::default());
    board
        .cx
        .simulate_click(button.center(), Modifiers::default());
    board.draw();
    assert!(board.find(text_contains("duplicate page")).is_none());
    assert_eq!(listed_done_count(&mut board), 40);
    board.click(role_name("button", "완료된 작업 더 보기"));
    assert_eq!(listed_done_count(&mut board), 60);
}

#[gpui::test]
fn failed_load_more_shows_the_reason_and_retry_requests_the_same_page(cx: &mut TestAppContext) {
    let mut board = many_done_workspace(cx);
    show_whole_workspace_list(&mut board);
    board.add_fault(
        "kanban.getMoreDoneTasks",
        FaultMode::Fail,
        Some("db locked"),
        None,
    );
    board.click(role_name("button", "완료된 작업 더 보기"));
    assert!(board.find(text_contains("db locked")).is_some());
    board.cx.update(|_, cx| {
        let faults = cx.global::<AppServices>().faults().unwrap().clone();
        faults.clear("kanban.getMoreDoneTasks");
        faults.add(ServiceFault {
            target: "kanban.getMoreDoneTasks".into(),
            mode: FaultMode::Fail,
            message: Some("wrong page".into()),
            ms: None,
            value: None,
            after_calls: None,
            argument_match: Some("[40]".into()),
        });
    });
    board.click(role_name("button", "다시 시도"));
    assert!(board.find(text_contains("db locked")).is_none());
    assert!(board.find(text_contains("wrong page")).is_none());
    assert_eq!(listed_done_count(&mut board), 40);
}

#[gpui::test]
fn workspace_search_selection_stays_on_the_workspace_route(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(&database.connection(), "board_view_mode", "workspace").unwrap();
    task(&database, "Billing export", TaskStatus::Review, |task| {
        task.branch_name = Some("feat/billing".into());
    });
    let mut board = open_board(cx, "/ko");
    board.dispatch(OpenTaskSearch);
    board.type_text("billing");
    board.keys("enter");
    assert_eq!(board.current_path(), "/ko");
    assert!(board.has_role_name("current", "Billing export"));
}

// ───────────── Board 머리·dd·돌아온 포커스·검토 활성화 ─────────────

#[gpui::test]
fn settings_button_opens_the_settings_of_the_current_locale(cx: &mut TestAppContext) {
    let dir = temp_dir();
    init_app(cx, dir.path().to_path_buf(), true);
    let mut board = open_board(cx, "/ko");
    board.click(role_name("button", "설정"));
    assert_eq!(board.current_path(), "/ko/settings");
}

#[gpui::test]
fn mac_desktop_places_board_controls_in_the_titlebar_row(cx: &mut TestAppContext) {
    let linux_top = {
        let dir = temp_dir();
        init_app(cx, dir.path().to_path_buf(), true);
        let mut board = open_board(cx, "/ko");
        board
            .find(TargetQuery::ExactText("+ 새 작업".into()))
            .unwrap()
            .origin
    };
    let mut mac_cx = TestAppContext::single();
    let dir = temp_dir();
    init_app(&mut mac_cx, dir.path().to_path_buf(), true);
    mac_cx.update(|cx| {
        cx.global_mut::<AppServices>().shortcut_platform = ShortcutPlatform::Mac;
    });
    let mut board = open_board(&mut mac_cx, "/ko");
    let mac_top = board
        .find(TargetQuery::ExactText("+ 새 작업".into()))
        .unwrap()
        .origin;
    // 신호등 버튼 자리(pt-10)만큼 아래, 같은 한 줄에 놓인다
    assert!(f32::from(mac_top.y) >= f32::from(linux_top.y) + 28.0);
    assert!(f32::from(mac_top.y) < 80.0);
}

#[gpui::test]
fn dd_keeps_the_card_until_the_delete_finishes(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .add(ServiceFault {
                target: "kanban.deleteTask".into(),
                mode: FaultMode::Delay,
                message: None,
                ms: Some(1000),
                value: None,
                after_calls: None,
                argument_match: None,
            })
    });
    board.keys("down d d");
    assert!(board.has_text("지울 작업"));
    board.advance(Duration::from_millis(1_100));
    assert!(!board.has_text("지울 작업"));
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

#[gpui::test]
fn dd_survives_a_board_refresh_between_the_two_presses(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let card = task(&database, "지울 작업", TaskStatus::Todo, |_| {});
    task(&database, "남을 작업", TaskStatus::Todo, |_| {});
    let mut board = open_board(cx, "/ko");
    board.keys("down d");
    let view = board.view.clone();
    board
        .cx
        .update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
    board.draw();
    board.keys("d");
    board.draw();
    assert!(
        find_task(&database.connection(), &card.id)
            .unwrap()
            .is_none()
    );
}

#[gpui::test]
fn returning_from_the_detail_starts_focus_on_that_task_once(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    task(&database, "할 일", TaskStatus::Todo, |_| {});
    let target = task(&database, "진행 작업", TaskStatus::Progress, |_| {});
    let mut board = open_board(cx, "/ko");
    // 상세 화면이 이 창의 복귀 대상으로 적어 둔 것처럼 두고 다른 화면을 거쳐 보드로 돌아온다
    board.cx.update(|window, cx| {
        kanvibe_app::board_focus_target::remember_board_focus_task(
            window.window_handle().window_id(),
            &target.id,
            cx,
        )
    });
    board.return_to_board_via("/ko/settings");
    // 포커스된 카드에서 Enter는 그 작업 상세로 간다
    board.keys("enter");
    assert_eq!(board.current_path(), format!("/ko/task/{}", target.id));
    // 한 번 꺼낸 기록은 남지 않는다
    let mut next = open_board(cx, "/ko");
    next.return_to_board_via("/ko/settings");
    next.keys("enter");
    assert_eq!(next.current_path(), "/ko");
}

#[gpui::test]
fn entering_the_board_does_not_open_a_stored_review(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    let merged = task(&database, "Merged task", TaskStatus::Review, |_| {});
    store_notifications(
        &database,
        serde_json::json!([review_notification_value(&[&merged])]),
    );
    let mut board = open_board(cx, "/ko");
    assert!(!review_dialog_is_open(&mut board));
    assert!(board.has_text("Merged task"));
}

// ───────────── TaskQuickSearchDialog 단축키·이동·스크롤 ─────────────

/// 프로젝트 `api`의 브랜치 작업 `count`개(`feat/api-search-00`…)
fn quick_search_board(cx: &mut TestAppContext, count: usize) -> (Board, Vec<KanbanTask>) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let api = project(&database, "api", None);
    let tasks = (0..count)
        .map(|index| {
            task(
                &database,
                &format!("검색 작업 {index:02}"),
                TaskStatus::Todo,
                |task| {
                    task.project_id = Some(api.id.clone());
                    task.branch_name = Some(format!("feat/api-search-{index:02}"));
                },
            )
        })
        .collect();
    let mut board = open_board(cx, "/ko");
    // 결과로 연 상세 화면이 세션 없는 프로젝트 작업을 실제 tmux에 자동 연결하지 않게 막는다
    board.add_result_fault(
        "sessionDependency.getSessionDependencyStatus",
        serde_json::json!({ "available": true }),
        None,
    );
    board.add_result_fault(
        "kanban.connectTerminalSession",
        serde_json::Value::Null,
        None,
    );
    (board, tasks)
}

fn is_quick_search_open(board: &mut Board) -> bool {
    board.has_text("태스크 빠른 검색")
}

#[gpui::test]
fn quick_search_opens_with_the_stored_shortcut(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    save_app_setting(
        &database.connection(),
        "shortcut_bindings",
        r#"{"taskSearch":"Mod+Shift+Y"}"#,
    )
    .unwrap();
    cx.update(kanvibe_app::shortcuts::reload_shortcut_bindings);
    let mut board = open_board(cx, "/ko");
    board.keys("ctrl-shift-y");
    assert!(is_quick_search_open(&mut board));
}

#[gpui::test]
fn project_and_branch_query_then_enter_opens_the_detail(cx: &mut TestAppContext) {
    let (mut board, tasks) = quick_search_board(cx, 1);
    board.dispatch(OpenTaskSearch);
    board.type_text("api search");
    board.keys("enter");
    assert_eq!(board.current_path(), format!("/ko/task/{}", tasks[0].id));
    assert!(!is_quick_search_open(&mut board));
}

#[gpui::test]
fn enter_focuses_the_window_already_showing_the_task(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let target = task(&database, "열린 작업", TaskStatus::Todo, |task| {
        task.branch_name = Some("feat/api-open".into());
    });
    let detail =
        cx.add_window(|window, cx| AppWindow::new(&format!("/ko/task/{}", target.id), window, cx));
    cx.run_until_parked();
    let mut board = open_board(cx, "/ko");
    board.dispatch(OpenTaskSearch);
    board.type_text("api-open");
    board.keys("enter");
    assert_eq!(board.current_path(), "/ko");
    assert!(!is_quick_search_open(&mut board));
    let active = board.cx.update(|_, cx| cx.active_window());
    assert_eq!(
        active.map(|handle| handle.window_id()),
        Some(detail.window_id())
    );
}

#[gpui::test]
fn shift_enter_opens_the_result_in_a_new_window(cx: &mut TestAppContext) {
    let (mut board, _tasks) = quick_search_board(cx, 1);
    let before = window_count(&mut board);
    board.dispatch(OpenTaskSearch);
    board.type_text("api");
    board.keys("shift-enter");
    assert_eq!(window_count(&mut board), before + 1);
    assert_eq!(board.current_path(), "/ko");
    assert!(!is_quick_search_open(&mut board));
}

#[gpui::test]
fn keyboard_selected_result_scrolls_into_view(cx: &mut TestAppContext) {
    let (mut board, _tasks) = quick_search_board(cx, 30);
    board.dispatch(OpenTaskSearch);
    let is_visible = |board: &mut Board, branch: &str| {
        let hit = board.cx.update(|window, cx| {
            cx.global::<QaTargetRegistry>().find_hit(
                window.window_handle().window_id(),
                &role_name("button", branch),
            )
        });
        hit.is_some_and(|hit| contains(hit.clip_bounds, hit.bounds))
    };
    // 같은 점수의 결과는 만든 시각·id 순이라 이름 순서와 다를 수 있으므로 화면에서 가장 아래 결과를 고른다
    let last = (0..30)
        .map(|index| format!("feat/api-search-{index:02}"))
        .filter_map(|branch| {
            let bounds = board.find(role_name("button", &branch))?;
            Some((f32::from(bounds.origin.y), branch))
        })
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, branch)| branch)
        .expect("results");
    assert!(!is_visible(&mut board, &last));
    for _ in 0..29 {
        board.keys("down");
    }
    assert!(is_visible(&mut board, &last));
}

// ───────────── ReleaseUpdateDialog 창 활성·Tab·재확인 ─────────────

/// 새 릴리스(v9.9.0) 응답이 있는 보드. 창을 앞으로 가져오지 않은 채 연다
fn inactive_release_board(cx: &mut TestAppContext) -> Board {
    let dir = temp_dir();
    let database = init_app_with_network(
        cx,
        dir.path().to_path_buf(),
        true,
        Some("release-available"),
    );
    std::mem::forget(dir);
    task(&database, "보드 카드", TaskStatus::Todo, |_| {});
    open_board(cx, "/ko")
}

#[gpui::test]
fn release_waits_until_the_window_can_present_it(cx: &mut TestAppContext) {
    let mut board = inactive_release_board(cx);
    assert_eq!(release_version(&mut board), None);
    board.cx.update(|window, _| window.activate_window());
    board.draw();
    assert_eq!(release_version(&mut board).as_deref(), Some("9.9.0"));
}

#[gpui::test]
fn tab_focus_stays_inside_the_release_dialog(cx: &mut TestAppContext) {
    let (mut board, _dir) = release_board(cx, None);
    let focused =
        |board: &mut Board| board.shell_read(|shell, _| shell.release_focused_control_label());
    board.keys("tab tab tab");
    assert_eq!(focused(&mut board), Some("viewRelease"));
    board.keys("tab");
    assert_eq!(focused(&mut board), Some("dontShowVersionAgain"));
    board.keys("shift-tab");
    assert_eq!(focused(&mut board), Some("viewRelease"));
    assert_eq!(release_version(&mut board).as_deref(), Some("9.9.0"));
}

#[gpui::test]
fn release_is_checked_again_after_an_hour(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app_with_network(
        cx,
        dir.path().to_path_buf(),
        true,
        Some("release-available"),
    );
    std::mem::forget(dir);
    task(&database, "보드 카드", TaskStatus::Todo, |_| {});
    add_fault_before_open(
        cx,
        result_fault(
            "releaseUpdates.checkForReleaseUpdate",
            serde_json::json!({"isUpdateAvailable": false}),
        ),
    );
    let mut board = open_board(cx, "/ko");
    board.cx.update(|window, _| window.activate_window());
    board.draw();
    assert_eq!(release_version(&mut board), None);
    board.cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .unwrap()
            .clear("releaseUpdates.checkForReleaseUpdate")
    });
    board.advance(Duration::from_secs(60 * 60));
    assert_eq!(release_version(&mut board).as_deref(), Some("9.9.0"));
}

// ───────────── HerdrServerHandoffPrompt ─────────────

fn handoff_event(host: &str) -> kanvibe_services::board_notifier::BoardEvent {
    kanvibe_services::board_notifier::BoardEvent::HerdrServerHandoffRequired(
        kanvibe_services::board_notifier::HerdrServerHandoffRequiredPayload {
            ssh_host: host.into(),
            herdr_session: "kanvibe".into(),
        },
    )
}

fn replacement_hosts(board: &mut Board) -> Vec<String> {
    board.shell_read(|shell, _| {
        shell
            .herdr_replacement_requests()
            .into_iter()
            .map(|request| request.ssh_host)
            .collect()
    })
}

/// QA 모드 보드. 실제 교체(ssh)는 고장으로 막는다
fn handoff_board(cx: &mut TestAppContext) -> Board {
    let dir = temp_dir();
    init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let mut board = open_board(cx, "/ko");
    board.add_fault(
        "sessionDependency.replaceHerdrServer",
        FaultMode::Fail,
        Some("replacement blocked in test"),
        None,
    );
    board
}

#[gpui::test]
fn handoff_notice_asks_for_the_host_and_replaces_after_approval(cx: &mut TestAppContext) {
    let mut board = handoff_board(cx);
    board.emit_board_event(handoff_event("dev-box"));
    assert_eq!(replacement_hosts(&mut board), ["dev-box"]);
}

#[gpui::test]
fn declined_handoff_does_not_replace_the_server(cx: &mut TestAppContext) {
    let dir = temp_dir();
    init_app(cx, dir.path().to_path_buf(), false);
    let mut board = open_board(cx, "/ko");
    board.emit_board_event(handoff_event("dev-box"));
    board.cx.simulate_prompt_answer("Cancel");
    board.draw();
    assert!(replacement_hosts(&mut board).is_empty());
}

#[gpui::test]
fn the_same_server_is_asked_only_once(cx: &mut TestAppContext) {
    let mut board = handoff_board(cx);
    board.emit_board_event(handoff_event("dev-box"));
    board.emit_board_event(handoff_event("dev-box"));
    assert_eq!(replacement_hosts(&mut board), ["dev-box"]);
}

#[gpui::test]
fn other_board_events_do_not_ask_for_a_handoff(cx: &mut TestAppContext) {
    let mut board = handoff_board(cx);
    board.emit_board_event(kanvibe_services::board_notifier::BoardEvent::BoardUpdated);
    assert!(replacement_hosts(&mut board).is_empty());
}

// ───────────── ActionStatusToast ─────────────

fn show_toast(
    board: &mut Board,
    kind: kanvibe_app::components::toast::ToastKind,
    message: &str,
    open_task_id: Option<&str>,
) {
    let shell = board.shell.clone();
    let message = message.to_string();
    let open_task_id = open_task_id.map(str::to_string);
    board.cx.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.show_action_status(kind, message, open_task_id, cx)
        })
    });
    board.draw();
}

fn plain_board(cx: &mut TestAppContext) -> Board {
    let dir = temp_dir();
    init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    open_board(cx, "/ko")
}

#[gpui::test]
fn toasts_show_by_kind_and_close_with_their_close_button(cx: &mut TestAppContext) {
    use kanvibe_app::components::toast::ToastKind;
    let mut board = plain_board(cx);
    show_toast(&mut board, ToastKind::Progress, "syncing", None);
    show_toast(&mut board, ToastKind::Failure, "sync failed", None);
    // 진행은 `status`, 실패는 `alert`로 읽힌다
    assert!(board.has_role_name("status", "syncing"));
    assert!(board.has_role_name("alert", "sync failed"));
    assert!(!board.has_role_name("status", "sync failed"));
    board.click(TargetQuery::Described(DescribedTarget {
        role: Some("button".into()),
        name: Some("닫기".into()),
        exact: Some(true),
        nth: Some(1),
        ..DescribedTarget::default()
    }));
    assert!(board.has_role_name("status", "syncing"));
    assert!(!board.has_role_name("alert", "sync failed"));
}

#[gpui::test]
fn toast_action_runs_and_closes_the_toast(cx: &mut TestAppContext) {
    use kanvibe_app::components::toast::ToastKind;
    let mut board = plain_board(cx);
    show_toast(&mut board, ToastKind::Success, "created", Some("task-1"));
    board.click(role_name("button", "열기"));
    assert_eq!(board.current_path(), "/ko/task/task-1");
}

#[gpui::test]
fn success_toasts_fade_while_failure_toasts_stay(cx: &mut TestAppContext) {
    use kanvibe_app::components::toast::ToastKind;
    let mut board = plain_board(cx);
    show_toast(&mut board, ToastKind::Success, "done", None);
    show_toast(&mut board, ToastKind::Failure, "broken", None);
    assert!(board.has_role_name("status", "done"));
    board.advance(Duration::from_secs(10));
    assert!(!board.has_role_name("status", "done"));
    assert!(board.has_role_name("alert", "broken"));
}

#[gpui::test]
fn unregistered_repositories_show_their_reasons(cx: &mut TestAppContext) {
    let (mut board, _database) = host_board(cx);
    let reason = "/home/tester/c/work/kanvibe: 같은 PC에 이름과 상위 폴더가 모두 같은 프로젝트가 이미 있습니다.";
    board.add_result_fault(
        "project.scanAndRegisterProjects",
        serde_json::json!({"registered": [], "skipped": [], "errors": [reason], "worktreeTasks": []}),
        None,
    );
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "스캔 및 등록"));
    assert!(board.has_text(reason));
    assert!(!board.find(text_contains("git 저장소")).is_some());
}

#[gpui::test]
fn late_status_snapshot_does_not_hide_a_sync_started_elsewhere(cx: &mut TestAppContext) {
    let dir = temp_dir();
    init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    add_fault_before_open(
        cx,
        result_fault(
            "backgroundTaskSync.getBackgroundTaskSyncStatus",
            serde_json::json!({
                "phase": "idle", "isRunning": false, "completedStages": 0,
                "reviewNeeded": false, "boardUpdated": false, "error": null, "delayMs": 1000
            }),
        ),
    );
    let mut board = open_board(cx, "/ko");
    board.emit_board_event(sync_status(BackgroundTaskSyncPhase::PullRequests, true));
    board.advance(Duration::from_millis(1_100));
    assert!(board.has_role_name("status", "동기화 중…"));
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    assert!(!board.has_role_name("button", "지금 동기화"));
}

#[gpui::test]
fn board_commands_reach_their_board_handlers(cx: &mut TestAppContext) {
    let mut board = plain_board(cx);
    board.dispatch(OpenCreateTask);
    let view = board.view.clone();
    assert!(board.cx.update(|_, cx| view.read(cx).is_create_task_open()));
    board.keys("escape");
    board.dispatch(kanvibe_app::shortcuts::ToggleNotificationCenter);
    assert!(board.has_role_name("dialog", "알림"));
}

#[gpui::test]
fn project_registration_refreshes_the_board(cx: &mut TestAppContext) {
    let (mut board, database) = host_board(cx);
    board.add_result_fault(
        "project.scanAndRegisterProjects",
        serde_json::json!({"registered": ["kanvibe"], "skipped": [], "errors": [], "worktreeTasks": []}),
        None,
    );
    task(&database, "등록 뒤 보일 작업", TaskStatus::Todo, |_| {});
    assert!(!board.has_role_name("link", "등록 뒤 보일 작업"));
    board.click(role_name("button", "프로젝트 스캔"));
    board.click(role_name("button", "스캔 및 등록"));
    board.keys("escape");
    assert!(board.has_role_name("link", "등록 뒤 보일 작업"));
}

// ───────────── 카드 순서 전환(TS `useFlipReflow`) ─────────────

/// 진행 중 열 카드 순서는 최근 수정 순이다. 시각을 직접 정해 순서를 고정한다
fn set_updated_at(database: &Database, task_id: &str, updated_at: &str) {
    database
        .connection()
        .execute(
            "UPDATE kanban_tasks SET updated_at = ?1 WHERE id = ?2",
            [updated_at, task_id],
        )
        .expect("update updated_at");
}

/// 진행 중 열에 위(첫째)·아래(둘째) 두 카드, 할 일 열에는 페이지가 스크롤되도록 카드를 많이 둔다
fn reflow_board(cx: &mut TestAppContext) -> (Board, Arc<Database>, KanbanTask, KanbanTask) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    for index in 0..24 {
        task(
            &database,
            &format!("채움 {index:02}"),
            TaskStatus::Todo,
            |_| {},
        );
    }
    let first = task(&database, "첫째 카드", TaskStatus::Progress, |_| {});
    let second = task(&database, "둘째 카드", TaskStatus::Progress, |_| {});
    set_updated_at(&database, &first.id, "2030-01-01 00:00:02");
    set_updated_at(&database, &second.id, "2030-01-01 00:00:01");
    let board = open_board(cx, "/ko");
    (board, database, first, second)
}

impl Board {
    fn card_top(&mut self, title: &str) -> f32 {
        let bounds = self
            .find(TargetQuery::ExactText(title.into()))
            .unwrap_or_else(|| panic!("{title} card"));
        f32::from(bounds.origin.y)
    }

    fn scroll_page_down(&mut self, distance: f32) {
        self.cx.simulate_event(gpui_kit::ScrollEvent {
            position: gpui_kit::point(gpui_kit::px(400.0), gpui_kit::px(400.0)),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                gpui_kit::px(0.0),
                gpui_kit::px(-distance),
            )),
            ..Default::default()
        });
        self.draw();
    }

    fn refresh_board(&mut self) {
        let view = self.view.clone();
        self.cx
            .update(|_, cx| view.update(cx, |view, cx| view.refresh(cx)));
        self.draw();
    }
}

#[gpui::test]
fn reorder_after_scrolling_slides_by_the_position_change_only(cx: &mut TestAppContext) {
    let (mut board, database, first, second) = reflow_board(cx);
    let resting_top = board.card_top("첫째 카드");
    board.scroll_page_down(200.0);
    let first_top = board.card_top("첫째 카드");
    let second_top = board.card_top("둘째 카드");
    assert!(first_top < resting_top, "보드가 스크롤되어야 한다");
    set_updated_at(&database, &first.id, "2030-01-01 00:00:01");
    set_updated_at(&database, &second.id, "2030-01-01 00:00:02");
    board.refresh_board();
    // 순서가 바뀐 프레임에는 두 카드가 이전 자리에서 출발한다(스크롤한 거리는 섞이지 않는다)
    assert_eq!(board.card_top("첫째 카드"), first_top);
    assert_eq!(board.card_top("둘째 카드"), second_top);
    board.advance(Duration::from_millis(300));
    assert_eq!(board.card_top("둘째 카드"), first_top);
    assert!(board.card_top("첫째 카드") > second_top - 1.0);
}

#[gpui::test]
fn a_card_that_grew_in_place_starts_the_next_reorder_from_its_new_spot(cx: &mut TestAppContext) {
    let (mut board, database, first, second) = reflow_board(cx);
    // 순서는 그대로인데 첫째 카드에 설명이 붙어 커졌다
    database
        .connection()
        .execute(
            "UPDATE kanban_tasks SET description = ?1 WHERE id = ?2",
            ["설명이 붙어 카드가 한 줄 더 커진다", first.id.as_str()],
        )
        .expect("update description");
    let resting_second = board.card_top("둘째 카드");
    board.refresh_board();
    let grown_second = board.card_top("둘째 카드");
    assert!(
        grown_second > resting_second,
        "둘째 카드가 아래로 밀려야 한다"
    );
    set_updated_at(&database, &first.id, "2030-01-01 00:00:01");
    set_updated_at(&database, &second.id, "2030-01-01 00:00:02");
    board.refresh_board();
    assert_eq!(board.card_top("둘째 카드"), grown_second);
}

#[gpui::test]
fn cards_that_keep_their_spot_get_no_transition(cx: &mut TestAppContext) {
    let (mut board, database, first, second) = reflow_board(cx);
    board.scroll_page_down(120.0);
    let first_top = board.card_top("첫째 카드");
    let second_top = board.card_top("둘째 카드");
    let third = task(&database, "셋째 카드", TaskStatus::Progress, |_| {});
    set_updated_at(&database, &third.id, "2030-01-01 00:00:00");
    board.refresh_board();
    assert!(board.has_text("셋째 카드"));
    assert_eq!(board.card_top("첫째 카드"), first_top);
    assert_eq!(board.card_top("둘째 카드"), second_top);
    let _ = (first, second);
}

/// TS `useFlipReflow`는 `prefers-reduced-motion`을 보지 않는다. 움직임 줄이기를 켜도 재정렬은 이전 자리에서 출발한다
#[gpui::test]
fn reorder_slides_even_when_the_os_reduces_motion(cx: &mut TestAppContext) {
    let (mut board, database, first, second) = reflow_board(cx);
    cx.update(|cx| cx.set_global(kanvibe_app::views::board::MotionPreference { is_reduced: true }));
    let second_top = board.card_top("둘째 카드");
    set_updated_at(&database, &first.id, "2030-01-01 00:00:01");
    set_updated_at(&database, &second.id, "2030-01-01 00:00:02");
    board.refresh_board();
    assert_eq!(board.card_top("둘째 카드"), second_top);
}

// ───────────── ProjectIcon ─────────────

#[gpui::test]
fn a_project_with_a_github_icon_shows_the_icon_instead_of_the_initial(cx: &mut TestAppContext) {
    let dir = temp_dir();
    let database = init_app(cx, dir.path().to_path_buf(), true);
    std::mem::forget(dir);
    let api = insert_project(
        &database.connection(),
        &NewProject {
            name: "api".into(),
            repo_path: "/repos/api".into(),
            default_branch: "main".into(),
            ssh_host: None,
            is_worktree: false,
            color: Some("#0064ff".into()),
            icon_data_url: Some("data:image/png;base64,aGVsbG8=".into()),
        },
    )
    .unwrap();
    task(&database, "아이콘 작업", TaskStatus::Todo, |task| {
        task.project_id = Some(api.id.clone())
    });
    let mut board = open_board(cx, "/ko");
    assert!(board.has_role_name("img", "api icon"));
    assert!(board.find(test_id("project-initial-icon")).is_none());
}
