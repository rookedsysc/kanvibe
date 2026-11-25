//! 경로마다 정해진 화면 뷰가 만들어지는지, 공용 UI 요소가 그려지는지 확인한다.

use std::any::TypeId;

use gpui_kit::gpui;
use gpui_kit::{
    Context, IntoElement, ParentElement, Render, ScrollHandle, Styled, TestAppContext, WeakEntity,
    Window, div, point, px,
};
use kanvibe_app::components::badge::{Badge, BadgeShape, BadgeTone};
use kanvibe_app::components::button::{Button, ButtonSize, ButtonVariant, IconButton};
use kanvibe_app::components::context_menu::{context_menu_at, context_menu_item};
use kanvibe_app::components::dialog::{
    DialogWidth, dialog_actions, dialog_description, dialog_panel, dialog_title, modal_layer,
};
use kanvibe_app::components::icon::IconName;
use kanvibe_app::components::popover::{popover_option, popover_panel};
use kanvibe_app::components::scroll_area::scroll_area;
use kanvibe_app::components::segmented_control::SegmentedControl;
use kanvibe_app::components::toast::{Toast, ToastKind, toast_stack};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::theme::ThemeTokens;
use kanvibe_app::views::{ScreenContext, WindowNavigator, build_screen_view};

fn init_app(cx: &mut TestAppContext) {
    let launch = LaunchOptions {
        data_dir: None,
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: None,
    };
    cx.update(|cx| kanvibe_app::init_app(&launch, cx).expect("app init"));
}

#[gpui::test]
fn every_route_builds_its_own_screen_view(cx: &mut TestAppContext) {
    use kanvibe_app::views::{
        ai_accounts::AiAccountsView, board::BoardView, diff::TaskDiffView, not_found::NotFoundView,
        pane_layout::PaneLayoutView, settings::SettingsView,
        shortcut_settings::ShortcutSettingsView, task_detail::TaskDetailView,
    };
    init_app(cx);
    let expectations = [
        ("/ko", TypeId::of::<BoardView>()),
        ("/ko/task/t1", TypeId::of::<TaskDetailView>()),
        ("/ko/task/t1/diff", TypeId::of::<TaskDiffView>()),
        ("/ko/settings", TypeId::of::<SettingsView>()),
        (
            "/ko/settings/shortcuts",
            TypeId::of::<ShortcutSettingsView>(),
        ),
        ("/ko/ai-accounts", TypeId::of::<AiAccountsView>()),
        ("/ko/pane-layout", TypeId::of::<PaneLayoutView>()),
        ("/ko/nowhere", TypeId::of::<NotFoundView>()),
    ];
    for (path, expected_view) in expectations {
        let RouteResolution::Render(route) = resolve_route_path(path) else {
            panic!("{path} must render");
        };
        let view = cx.update(|cx| {
            build_screen_view(
                ScreenContext {
                    route,
                    navigator: WindowNavigator::new(WeakEntity::new_invalid()),
                },
                cx,
            )
        });
        assert_eq!(view.entity_type(), expected_view, "{path}");
    }
}

/// 공용 요소를 한 화면에 모두 그린다
struct ComponentGallery {
    scroll_handle: ScrollHandle,
}

impl Render for ComponentGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let theme = *cx.global::<ThemeTokens>();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(Button::new("primary", "확인").variant(ButtonVariant::Primary))
                    .child(Button::new("outline", "취소").size(ButtonSize::Large))
                    .child(Button::new("neutral", "실행").variant(ButtonVariant::Neutral))
                    .child(
                        Button::new("danger", "삭제")
                            .variant(ButtonVariant::Danger)
                            .disabled(true),
                    )
                    .child(IconButton::new("settings", IconName::Settings, "설정"))
                    .child(Badge::new("main", BadgeTone::Branch).mono())
                    .child(Badge::new("kanvibe", BadgeTone::Project).shape(BadgeShape::Pill))
                    .child(
                        SegmentedControl::new("kind", ["Project", "Task", "All"], 2)
                            .width(px(180.0)),
                    ),
            )
            .child(div().h(px(200.0)).child(scroll_area(
                "list",
                &self.scroll_handle,
                theme.border.strong.into(),
                div().h(px(600.0)).child("긴 목록"),
            )))
            .child(
                div().relative().child(popover_panel(
                    &theme,
                    div()
                        .child(popover_option("p1", &theme, "프로젝트 1", true, Some(true)))
                        .child(popover_option("p2", &theme, "프로젝트 2", false, None)),
                )),
            )
            .child(context_menu_at(
                &theme,
                point(px(40.0), px(40.0)),
                [context_menu_item("delete", &theme, "삭제", true)],
            ))
            .child(modal_layer(
                &theme,
                viewport,
                dialog_panel(&theme, DialogWidth::Small)
                    .child(dialog_title(&theme, "완료로 옮길까요?"))
                    .child(dialog_description(&theme, "작업을 완료로 옮깁니다."))
                    .child(dialog_actions().child(Button::new("ok", "확인"))),
            ))
            .child(toast_stack(
                viewport,
                [
                    Toast::new("t1", ToastKind::Progress, "만드는 중", "닫기"),
                    Toast::new("t2", ToastKind::Failure, "실패", "닫기")
                        .action("다시", |_, _, _| {}),
                ],
            ))
    }
}

/// 레이아웃·그리기 도중 panic이 없으면 통과다(픽셀은 화면 diff가 본다)
#[gpui::test]
fn shared_components_lay_out_and_paint(cx: &mut TestAppContext) {
    init_app(cx);
    let (_, window) = cx.add_window_view(|_, _| ComponentGallery {
        scroll_handle: ScrollHandle::new(),
    });
    window.update(|window, _| window.refresh());
    window.run_until_parked();
}
