//! 화면 코드가 돌기 전 모습. TS 앱은 `index.html`의 정적 스켈레톤을 스크립트 실행 전에 먼저 그렸고(SCR-BOOT-01),
//! lazy 라우트 번들을 받는 동안 Suspense fallback `Loading...`을 그렸다(SCR-BOOT-02).
//! GPUI 앱은 화면을 곧바로 만들므로 이 둘은 QA 재생이 그 순간을 붙잡을 때(`--hold-first-frame`, `--hold-request`)만 그린다.

use gpui_kit::{
    Context, Div, IntoElement, ParentElement, Render, Rgba, Styled, Window, div, px, rgb, rgba,
};

use crate::routes::AppRoute;
use crate::theme::ThemeTokens;
use crate::views::route_loading_fallback;

/// TS `App.tsx`가 `lazy(() => import(...))`로 나눈 라우트의 번들 이름. 보드는 처음부터 실린다
pub fn lazy_route_bundle(route: &AppRoute) -> Option<&'static str> {
    match route {
        AppRoute::Board => None,
        AppRoute::TaskDiff { .. } => Some("DiffRoute"),
        AppRoute::NotFound { .. } => Some("NotFoundRoute"),
        AppRoute::AiAccounts => Some("AiAccountsRoute"),
        AppRoute::PaneLayout => Some("PaneLayoutRoute"),
        AppRoute::Settings => Some("SettingsRoute"),
        AppRoute::ShortcutSettings => Some("ShortcutSettingsRoute"),
        AppRoute::TaskDetail { .. } => Some("TaskDetailRoute"),
    }
}

/// 번들이 끝나지 않은 lazy 라우트 자리(TS `RouteLoadingFallback`)
pub struct HeldRouteView;

impl Render for HeldRouteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        route_loading_fallback(cx.global::<ThemeTokens>())
    }
}

/// `index.html`의 `.kanvibe-initial-board`. 테마를 읽기 전이라 HTML에 박힌 다크 색을 그대로 쓴다
pub struct BootSkeletonView;

/// `index.html` 인라인 스타일의 색
mod colors {
    pub const PAGE: u32 = 0x090a0d;
    pub const HEADER: u32 = 0x111216;
    pub const LINE: u32 = 0x24262d;
    pub const FILL: u32 = 0x1a1c22;
    pub const PROJECT_DOT: u32 = 0x353842;
    /// `rgba(0, 100, 255, 0.26)`
    pub const PRIMARY_BUTTON: u32 = 0x0064ff42;
    /// `:nth-child(1..5) .kanvibe-skeleton-status`
    pub const STATUS: [u32; 5] = [0x747985, 0xf6c35f, 0x9b8cff, 0x0064ff, 0x65d08a];
}

/// 열마다 카드의 배지 수(HTML 그대로)
const BADGES_PER_COLUMN: [&[usize]; 5] = [&[3, 2, 2], &[2, 2], &[3, 2, 2], &[2, 2], &[2, 2]];
/// 카드 제목 폭(`82%`, `:nth-child(2) 68%`, `:nth-child(3) 76%`)
const TITLE_WIDTHS: [f32; 3] = [0.82, 0.68, 0.76];
/// `.kanvibe-initial-header` 높이와 `.kanvibe-initial-main` 여백
const HEADER_HEIGHT: f32 = 56.0;
const MAIN_PADDING: f32 = 24.0;
/// `.kanvibe-skeleton-column` 폭과 열 간격
const COLUMN_WIDTH: f32 = 284.0;
const COLUMN_GAP: f32 = 12.0;
/// `min-height: calc(100vh - 132px)`
const COLUMN_HEIGHT_INSET: f32 = 132.0;

impl Render for BootSkeletonView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let viewport_height = f32::from(window.viewport_size().height);
        let column_height = viewport_height - COLUMN_HEIGHT_INSET;
        div()
            .size_full()
            .bg(rgb(colors::PAGE))
            .child(skeleton_header())
            .child(
                div().p(px(MAIN_PADDING)).child(
                    div().flex().gap(px(COLUMN_GAP)).children(
                        BADGES_PER_COLUMN
                            .iter()
                            .enumerate()
                            .map(|(index, badges)| skeleton_column(index, badges, column_height)),
                    ),
                ),
            )
    }
}

fn outlined_box(width: f32, height: f32, background: Rgba) -> Div {
    div()
        .w(px(width))
        .h(px(height))
        .flex_none()
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(colors::LINE))
        .bg(background)
}

fn skeleton_header() -> Div {
    div()
        .h(px(HEADER_HEIGHT))
        .flex()
        .items_center()
        .justify_end()
        .gap(px(12.0))
        .px(px(24.0))
        .border_b_1()
        .border_color(rgb(colors::LINE))
        .bg(rgb(colors::HEADER))
        .child(outlined_box(256.0, 32.0, rgb(colors::FILL)))
        .child(outlined_box(80.0, 32.0, rgba(colors::PRIMARY_BUTTON)))
        .child(outlined_box(32.0, 32.0, rgb(colors::FILL)))
        .child(outlined_box(32.0, 32.0, rgb(colors::FILL)))
}

fn pill(width: f32, height: f32, color: u32) -> Div {
    div()
        .w(px(width))
        .h(px(height))
        .flex_none()
        .rounded_full()
        .bg(rgb(color))
}

fn skeleton_column(index: usize, badges: &[usize], height: f32) -> Div {
    let header = div()
        .h(px(40.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .border_b_1()
        .border_color(rgb(colors::FILL))
        .child(pill(8.0, 8.0, colors::STATUS[index]))
        .child(pill(80.0, 12.0, colors::LINE))
        .child(div().ml_auto().child(pill(20.0, 12.0, colors::LINE)));
    div()
        .w(px(COLUMN_WIDTH))
        .min_h(px(height))
        .flex_none()
        .rounded(px(8.0))
        .border_1()
        .border_color(rgb(colors::LINE))
        .overflow_hidden()
        .child(header)
        .child(
            div().flex().flex_col().gap(px(6.0)).p(px(8.0)).children(
                badges
                    .iter()
                    .enumerate()
                    .map(|(row, badge_count)| skeleton_task(row, *badge_count)),
            ),
        )
}

fn skeleton_task(row: usize, badge_count: usize) -> Div {
    // 카드 안쪽 폭: 열 284 - 테두리 2 - 목록 여백 16 - 카드 테두리 2 - 카드 여백 20
    let inner_width = COLUMN_WIDTH - 2.0 - 16.0 - 2.0 - 20.0;
    let badges = (0..badge_count).map(|badge| {
        let width = if badge == 1 { 28.0 } else { 36.0 };
        div()
            .w(px(width))
            .h(px(16.0))
            .flex_none()
            .rounded_full()
            .border_1()
            .border_color(rgb(colors::LINE))
            .bg(rgb(colors::HEADER))
    });
    div()
        .min_h(px(86.0))
        .p(px(10.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(colors::FILL))
        .child(
            div()
                .mb(px(9.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(pill(6.0, 6.0, colors::PROJECT_DOT))
                .child(pill(96.0, 10.0, colors::LINE)),
        )
        .child(pill(inner_width * TITLE_WIDTHS[row], 14.0, colors::LINE))
        .child(
            div()
                .mt(px(9.0))
                .child(pill(inner_width * 0.58, 10.0, colors::FILL)),
        )
        .child(div().mt(px(12.0)).flex().gap(px(6.0)).children(badges))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::{RouteResolution, resolve_route_path};

    fn route(path: &str) -> AppRoute {
        let RouteResolution::Render(route) = resolve_route_path(path) else {
            panic!("{path} must render");
        };
        route.route
    }

    #[test]
    fn only_lazy_routes_have_a_bundle_name() {
        assert_eq!(lazy_route_bundle(&route("/ko")), None);
        assert_eq!(
            lazy_route_bundle(&route("/ko/settings")),
            Some("SettingsRoute")
        );
        assert_eq!(
            lazy_route_bundle(&route("/ko/task/t1/diff")),
            Some("DiffRoute")
        );
    }
}
