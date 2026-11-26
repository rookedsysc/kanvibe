//! pane 레이아웃 화면(TS `PaneLayoutRoute` 읽기 안전 규칙). 실제 DB와 고장 주입으로 늦음·실패·새로 고침을 만든다.

use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, WeakEntity};
use kanvibe_app::qa::faults::ServiceFault;
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::AppServices;
use kanvibe_app::views::pane_layout::PaneLayoutView;
use kanvibe_app::views::{ScreenContext, WindowNavigator};
use kanvibe_db::repositories::pane_layouts::{
    NewPaneLayoutConfig, find_global_pane_layout, insert_pane_layout,
};
use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use serde_json::json;

fn init_app_with_database(cx: &mut TestAppContext) -> tempfile::TempDir {
    let data_dir = tempfile::tempdir().expect("temp dir");
    let launch = LaunchOptions {
        data_dir: Some(data_dir.path().to_path_buf()),
        initial_route: "/ko/pane-layout".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: 1_768_446_000_000,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
            loading_holds: Default::default(),
        }),
    };
    cx.update(|cx| kanvibe_app::init_app(&launch, cx).expect("app init"));
    data_dir
}

fn add_fault(cx: &mut TestAppContext, fault: serde_json::Value) {
    let fault: ServiceFault = serde_json::from_value(fault).expect("fault");
    cx.update(|cx| cx.global::<AppServices>().faults().expect("QA").add(fault));
}

fn clear_fault(cx: &mut VisualTestContext, target: &str) {
    cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .expect("QA")
            .clear(target)
    });
}

fn add_fault_in(cx: &mut VisualTestContext, fault: serde_json::Value) {
    let fault: ServiceFault = serde_json::from_value(fault).expect("fault");
    cx.update(|_, cx| cx.global::<AppServices>().faults().expect("QA").add(fault));
}

fn insert_layout(cx: &mut TestAppContext, layout_type: PaneLayoutType, project_id: Option<String>) {
    cx.update(|cx| {
        let database = cx.global::<AppServices>().database().unwrap().clone();
        insert_pane_layout(
            &database.connection(),
            &NewPaneLayoutConfig {
                layout_type,
                panes: vec![PaneCommand {
                    position: 0,
                    command: "pnpm dev".into(),
                }],
                is_global: project_id.is_none(),
                project_id,
            },
        )
        .unwrap();
    });
}

fn insert_demo_project(cx: &mut TestAppContext) -> String {
    cx.update(|cx| {
        let database = cx.global::<AppServices>().database().unwrap().clone();
        insert_project(
            &database.connection(),
            &NewProject {
                name: "kanvibe-demo".into(),
                repo_path: "/tmp/kanvibe-demo".into(),
                default_branch: "main".into(),
                ssh_host: None,
                is_worktree: false,
                color: None,
                icon_data_url: None,
            },
        )
        .unwrap()
        .id
    })
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn open_pane_layout(cx: &mut TestAppContext) -> (Entity<PaneLayoutView>, &mut VisualTestContext) {
    let RouteResolution::Render(route) = resolve_route_path("/ko/pane-layout") else {
        panic!("route");
    };
    let screen = ScreenContext {
        route,
        navigator: WindowNavigator::new(WeakEntity::new_invalid()),
    };
    let (view, cx) = cx.add_window_view(|_, cx| PaneLayoutView::new(screen, cx));
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn reload(view: &Entity<PaneLayoutView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.reload(cx));
    cx.run_until_parked();
    draw(cx);
}

#[gpui::test]
fn slow_load_shows_the_error_but_hydrates_the_saved_layout_when_it_arrives(
    cx: &mut TestAppContext,
) {
    let _data_dir = init_app_with_database(cx);
    insert_layout(cx, PaneLayoutType::Quad, None);
    add_fault(
        cx,
        json!({"target": "paneLayout.getGlobalPaneLayout", "mode": "delay", "ms": 6000}),
    );
    let (view, cx) = open_pane_layout(cx);
    cx.executor().advance_clock(Duration::from_millis(5_100));
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |view, _| {
        assert!(view.is_load_failed);
        assert!(view.global_editor().is_none(), "빈 편집기를 띄우지 않는다");
    });
    cx.executor().advance_clock(Duration::from_millis(1_000));
    cx.run_until_parked();
    draw(cx);
    let editor = view
        .read_with(cx, |view, _| view.global_editor())
        .expect("hydrated editor");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.draft.layout_type),
        PaneLayoutType::Quad
    );
    assert!(!view.read_with(cx, |view, _| view.is_load_failed));
}

#[gpui::test]
fn failed_global_read_stays_unsavable_until_retried(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    add_fault(
        cx,
        json!({"target": "paneLayout.getGlobalPaneLayout", "mode": "fail", "message": "database is locked"}),
    );
    let (view, cx) = open_pane_layout(cx);
    view.read_with(cx, |view, _| {
        assert!(view.is_load_failed);
        assert!(view.global_editor().is_none());
    });
    clear_fault(cx, "paneLayout.getGlobalPaneLayout");
    reload(&view, cx);
    assert!(view.read_with(cx, |view, _| view.global_editor().is_some()));
}

#[gpui::test]
fn failed_project_read_is_not_shown_as_an_absent_override(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let project_id = insert_demo_project(cx);
    add_fault(
        cx,
        json!({"target": "paneLayout.getProjectPaneLayout", "mode": "fail", "message": "locked"}),
    );
    let (view, cx) = open_pane_layout(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.project_override(&project_id), None);
        assert!(view.project_editor(&project_id).is_none());
    });
    clear_fault(cx, "paneLayout.getProjectPaneLayout");
    reload(&view, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.project_override(&project_id), Some(false));
        assert!(view.project_editor(&project_id).is_some());
    });
}

#[gpui::test]
fn explicitly_absent_global_setting_can_be_created(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_pane_layout(cx);
    let editor = view
        .read_with(cx, |view, _| view.global_editor())
        .expect("editor");
    editor.update_in(cx, |editor, window, cx| {
        editor.change_layout(PaneLayoutType::Vertical2, window, cx);
        editor.save(cx);
    });
    cx.run_until_parked();
    let saved = cx.update(|_, cx| {
        let database = cx.global::<AppServices>().database().unwrap().clone();
        find_global_pane_layout(&database.connection()).unwrap()
    });
    assert_eq!(
        saved.map(|config| config.layout_type),
        Some(PaneLayoutType::Vertical2)
    );
}

#[gpui::test]
fn edited_draft_survives_successful_and_failed_refreshes(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    insert_layout(cx, PaneLayoutType::Single, None);
    let (view, cx) = open_pane_layout(cx);
    let editor = view
        .read_with(cx, |view, _| view.global_editor())
        .expect("editor");
    editor.update_in(cx, |editor, window, cx| {
        editor.change_layout(PaneLayoutType::Quad, window, cx)
    });
    reload(&view, cx);
    add_fault_in(
        cx,
        json!({"target": "paneLayout.*", "mode": "fail", "message": "locked"}),
    );
    reload(&view, cx);
    let current = view
        .read_with(cx, |view, _| view.global_editor())
        .expect("editor");
    assert_eq!(current, editor, "같은 편집기를 지킨다");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.draft.layout_type),
        PaneLayoutType::Quad
    );
}

#[gpui::test]
fn loaded_project_draft_is_kept_when_its_refresh_fails(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let project_id = insert_demo_project(cx);
    insert_layout(cx, PaneLayoutType::Horizontal2, Some(project_id.clone()));
    let (view, cx) = open_pane_layout(cx);
    let editor = view
        .read_with(cx, |view, _| view.project_editor(&project_id))
        .expect("project editor");
    editor.update_in(cx, |editor, window, cx| {
        editor.change_layout(PaneLayoutType::LeftTbRight, window, cx)
    });
    add_fault_in(
        cx,
        json!({"target": "paneLayout.getProjectPaneLayout", "mode": "fail", "message": "locked"}),
    );
    reload(&view, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.project_override(&project_id),
            Some(true),
            "지난 값을 지킨다"
        );
        assert_eq!(view.project_editor(&project_id).as_ref(), Some(&editor));
    });
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.draft.layout_type),
        PaneLayoutType::LeftTbRight
    );
}
