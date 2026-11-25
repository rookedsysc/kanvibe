//! 칸반 보드 페이지 배치와 로딩 스켈레톤(`Board.tsx`, `BoardRoute.tsx`의 `BoardRouteSkeleton`).

use gpui_kit::{AnyElement, Div, IntoElement, ParentElement, Rgba, Styled, div, px, relative};
use kanvibe_domain::i18n::Translator;

use crate::components::scroll_area::page_scrollbar;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::TailwindText;

/// 헤더 아래 칸반 영역 문서 높이 계산용 값. 스켈레톤 열 본문은 `min-h-[calc(100vh-132px)]`다
const SKELETON_COLUMN_BODY_VIEWPORT_OFFSET: f32 = 132.0;
const SKELETON_TASK_COUNTS: [usize; 5] = [3, 2, 3, 2, 2];
/// `w-11/12`, `w-4/5`, `w-3/4`
const SKELETON_TITLE_FRACTIONS: [f32; 3] = [11.0 / 12.0, 4.0 / 5.0, 3.0 / 4.0];
/// `w-2/3`, `w-1/2`, `w-3/5`
const SKELETON_DESCRIPTION_FRACTIONS: [f32; 3] = [2.0 / 3.0, 1.0 / 2.0, 3.0 / 5.0];
/// `w-9`, `w-7`, `w-9`
const SKELETON_BADGE_WIDTHS: [f32; 3] = [36.0, 28.0, 36.0];
/// 애니메이션을 끈 `animate-pulse` 대신 남는 `opacity-80`의 8비트 알파
const SKELETON_OPACITY_ALPHA: i32 = 204;
/// `bg-brand-primary/40`
const SKELETON_PRIMARY_BUTTON_ALPHA: f32 = 0.4;

/// 스켈레톤 헤더 높이(py-3 + 32px 상자 + border-b)
const SKELETON_HEADER_HEIGHT: f32 = 57.0;

/// 보드 문서 전체 높이(빈 열 기준). 스크롤바 thumb 길이를 Chromium과 같게 하려면 문서 높이가 같아야 한다.
pub fn board_document_height(header_height: f32, viewport_height: f32) -> f32 {
    let column_body = viewport_height - SKELETON_COLUMN_BODY_VIEWPORT_OFFSET;
    // main p-6 위아래 + 열 머리 40 + 열 테두리 2 + 목록 pb-2
    header_height + 24.0 + 40.0 + column_body + 2.0 + 8.0 + 24.0
}

/// `opacity-80` 묶음을 페이지 배경 위에 미리 섞는다. Chromium 합성은 8비트 알파 a로 `d + ((s - d) * (a + 1)) >> 8`(내림)을
/// 쓰므로 GPUI `opacity()`(실수 혼합 후 반올림)를 쓰면 채널마다 1씩 어긋난다. 스켈레톤은 페이지 배경 위에만 놓여 미리 섞어도 같다.
fn skeleton_tone(color: Rgba, page: Rgba) -> Rgba {
    let blend = |source: f32, backdrop: f32| {
        let source = (source * 255.0).round() as i32;
        let backdrop = (backdrop * 255.0).round() as i32;
        let mixed = backdrop + (((source - backdrop) * (SKELETON_OPACITY_ALPHA + 1)) >> 8);
        mixed as f32 / 255.0
    };
    Rgba {
        r: blend(color.r, page.r),
        g: blend(color.g, page.g),
        b: blend(color.b, page.b),
        a: 1.0,
    }
}

/// 스켈레톤 열 안에서 쓰는 색(배경과 미리 섞은 값)
struct SkeletonPalette {
    border_default: Rgba,
    border_subtle: Rgba,
    border_strong: Rgba,
    page: Rgba,
    status_dots: [Rgba; 5],
}

impl SkeletonPalette {
    fn new(theme: &ThemeTokens) -> Self {
        let page = theme.background.page;
        let tone = |color: Rgba| skeleton_tone(color, page);
        Self {
            border_default: tone(theme.border.default),
            border_subtle: tone(theme.border.subtle),
            border_strong: tone(theme.border.strong),
            page,
            status_dots: status_dot_colors(theme).map(tone),
        }
    }
}

fn status_dot_colors(theme: &ThemeTokens) -> [Rgba; 5] {
    [
        theme.status.todo,
        theme.status.progress,
        theme.status.pending,
        theme.status.review,
        theme.status.done,
    ]
}

/// 데이터를 기다리는 동안의 보드. 페이지가 창보다 길어 오른쪽에 스크롤바 홈이 생긴다.
/// 조회가 오래 걸리면 아래쪽 가운데에 지연 안내(`delay_notice`)를 띄운다
pub fn render_board_skeleton(
    theme: &ThemeTokens,
    viewport_height: f32,
    delay_notice: Option<String>,
) -> impl IntoElement {
    let document_height = board_document_height(SKELETON_HEADER_HEIGHT, viewport_height);
    div()
        .relative()
        .size_full()
        .flex()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .bg(theme.background.page)
                .child(render_skeleton_header(theme))
                .child(render_skeleton_columns(theme, viewport_height)),
        )
        .child(page_scrollbar(
            document_height,
            0.0,
            theme.border.strong.into(),
        ))
        .children(delay_notice.map(|notice| render_load_delay_notice(theme, notice)))
}

/// `fixed bottom-6 left-1/2 -translate-x-1/2 rounded-md border bg-bg-surface px-3 py-2 text-xs shadow-sm`
fn render_load_delay_notice(theme: &ThemeTokens, notice: String) -> impl IntoElement {
    div()
        .absolute()
        .bottom(spacing(6.0))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .px(spacing(3.0))
                .py(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .shadow(crate::components::box_shadows(&theme.shadow.sm))
                .relative()
                .child(notice.clone())
                .child(crate::qa::targets::qa_target_layer(
                    crate::qa::targets::QaTargetDescription {
                        role: "status",
                        name: notice.clone().into(),
                        text: notice.into(),
                    },
                )),
        )
}

fn skeleton_box(theme: &ThemeTokens, width: f32) -> Div {
    div()
        .h(spacing(8.0))
        .w(px(width))
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
}

fn render_skeleton_header(theme: &ThemeTokens) -> impl IntoElement {
    let primary_placeholder = Rgba {
        a: SKELETON_PRIMARY_BUTTON_ALPHA,
        ..theme.brand.primary
    };
    div()
        .flex()
        .items_center()
        .justify_end()
        .border_b_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .px(spacing(6.0))
        .py(spacing(3.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(spacing(3.0))
                .child(skeleton_box(theme, 256.0))
                .child(
                    div()
                        .h(spacing(8.0))
                        .w(px(80.0))
                        .rounded(radius::MD)
                        .bg(primary_placeholder),
                )
                .child(skeleton_box(theme, 32.0))
                .child(skeleton_box(theme, 32.0)),
        )
}

fn render_skeleton_columns(theme: &ThemeTokens, viewport_height: f32) -> impl IntoElement {
    let palette = SkeletonPalette::new(theme);
    let columns = SKELETON_TASK_COUNTS
        .into_iter()
        .zip(palette.status_dots)
        .enumerate()
        .map(|(column_index, (task_count, dot_color))| {
            render_skeleton_column(
                &palette,
                column_index,
                task_count,
                dot_color,
                viewport_height,
            )
        });
    div().p(spacing(6.0)).child(
        div()
            .flex()
            .gap(spacing(3.0))
            .pb(spacing(2.0))
            .children(columns),
    )
}

/// 스켈레톤 열 머리 `h-10`에서 border-b 1px을 뺀 내용 높이
const COLUMN_HEADER_CONTENT_HEIGHT: f32 = 39.0;

/// `items-center`가 만드는 위쪽 여백. Chromium은 .5 위치를 위로 반올림하지만 GPUI 레이아웃 스냅은 0 쪽으로 내려
/// 1px 어긋나므로, 정수 높이 부모 안에서는 여백을 미리 Chromium 방식으로 반올림해 둔다.
pub(crate) fn chromium_centered_offset(
    container_height: f32,
    child_height: f32,
) -> gpui_kit::Pixels {
    px(((container_height - child_height) / 2.0 + 0.5).floor())
}

fn skeleton_bar(height: f32, color: Rgba) -> Div {
    div().h(px(height)).rounded(radius::BASE).bg(color)
}

fn render_skeleton_column(
    palette: &SkeletonPalette,
    column_index: usize,
    task_count: usize,
    dot_color: Rgba,
    viewport_height: f32,
) -> impl IntoElement {
    let cards =
        (0..task_count).map(|task_index| render_skeleton_card(palette, task_index + column_index));
    div()
        .min_w(px(284.0))
        .max_w(px(340.0))
        .flex_1()
        .overflow_hidden()
        .rounded(radius::LG)
        .border_1()
        .border_color(palette.border_default)
        .child(render_skeleton_column_header(palette, dot_color))
        .child(
            div()
                .min_h(px(viewport_height - SKELETON_COLUMN_BODY_VIEWPORT_OFFSET))
                .p(spacing(2.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .children(cards),
        )
}

/// 스켈레톤 열 머리: 상태 점과 제목·개수 자리
fn render_skeleton_column_header(palette: &SkeletonPalette, dot_color: Rgba) -> impl IntoElement {
    let bar_offset = chromium_centered_offset(COLUMN_HEADER_CONTENT_HEIGHT, 12.0);
    div()
        .flex()
        .h(spacing(10.0))
        .items_start()
        .gap(spacing(2.0))
        .border_b_1()
        .border_color(palette.border_subtle)
        .px(spacing(3.0))
        .child(
            div()
                .mt(chromium_centered_offset(COLUMN_HEADER_CONTENT_HEIGHT, 8.0))
                .size(spacing(2.0))
                .rounded(radius::FULL)
                .bg(dot_color),
        )
        .child(
            skeleton_bar(12.0, palette.border_subtle)
                .mt(bar_offset)
                .w(spacing(20.0)),
        )
        .child(
            skeleton_bar(12.0, palette.border_subtle)
                .mt(bar_offset)
                .ml_auto()
                .w(spacing(5.0)),
        )
}

fn render_skeleton_card(palette: &SkeletonPalette, index: usize) -> impl IntoElement {
    let title_fraction = SKELETON_TITLE_FRACTIONS[index % SKELETON_TITLE_FRACTIONS.len()];
    let description_fraction =
        SKELETON_DESCRIPTION_FRACTIONS[index % SKELETON_DESCRIPTION_FRACTIONS.len()];
    let badges = SKELETON_BADGE_WIDTHS.into_iter().map(|width| {
        div()
            .h(spacing(4.0))
            .w(px(width))
            .rounded(radius::BASE)
            .border_1()
            .border_color(palette.border_subtle)
            .bg(palette.page)
    });
    div()
        .rounded(radius::MD)
        .border_1()
        .border_color(palette.border_subtle)
        .px(px(10.0))
        .py(spacing(2.0))
        .child(
            div()
                .mb(spacing(2.0))
                .flex()
                .items_center()
                .gap(spacing(2.0))
                .child(
                    div()
                        .size(px(14.0))
                        .flex_none()
                        .rounded(radius::SM)
                        .bg(palette.border_strong),
                )
                .child(skeleton_bar(12.0, palette.border_subtle).w(spacing(24.0))),
        )
        .child(skeleton_bar(14.0, palette.border_default).w(relative(title_fraction)))
        .child(
            skeleton_bar(10.0, palette.border_subtle)
                .mt(spacing(2.0))
                .w(relative(description_fraction)),
        )
        .child(div().mt(spacing(3.0)).flex().gap(px(6.0)).children(badges))
}

impl super::BoardView {
    /// 데이터를 읽은 칸반: 헤더, 다섯 열, 그 위의 메뉴·대화상자·배너
    pub(super) fn render_kanban(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = *cx.global::<ThemeTokens>();
        let displayed = self.displayed_tasks();
        let header = self.render_board_header(&theme, super::header::content_width(window), cx);
        let columns = self.render_columns(&theme, &displayed, window, cx);
        let page = div()
            .min_h_full()
            .flex()
            .flex_col()
            .bg(theme.background.page)
            .child(header)
            .child(
                div().p(spacing(6.0)).child(
                    div()
                        .flex()
                        .gap(spacing(3.0))
                        .pb(spacing(2.0))
                        .children(columns),
                ),
            );
        let overlays: Vec<AnyElement> = [
            self.render_refresh_failure_banner(&theme, cx),
            self.render_find_bar(&theme, window, cx),
            self.render_context_menu(&theme, cx),
            self.render_done_confirm(&theme, window, cx),
            self.render_board_dialogs(&theme, window, cx),
        ]
        .into_iter()
        .flatten()
        .collect();
        div()
            .relative()
            .size_full()
            .bg(theme.background.page)
            .child(crate::components::scroll_area::scroll_area(
                "board-page",
                &self.page_scroll,
                theme.border.strong.into(),
                page,
            ))
            .children(overlays)
    }

    /// 재조회가 실패하면 이전 데이터를 둔 채 위쪽 가운데에 알린다(TS `RouteRefreshFailureBanner`)
    fn render_refresh_failure_banner(
        &mut self,
        theme: &ThemeTokens,
        cx: &mut gpui_kit::Context<Self>,
    ) -> Option<AnyElement> {
        let reason = self.load_status.failure_reason.clone()?;
        let message = Translator::new(self.locale(), "common")
            .translate_with("refreshFailed", &[("reason", &reason)]);
        let banner = super::inline_load_error(
            theme,
            self.locale(),
            message,
            "board-refresh-retry",
            cx.listener(|this, _, _, cx| this.retry_load(cx)),
        )
        .w(px(512.0))
        .shadow(crate::components::box_shadows(&theme.shadow.md));
        Some(
            div()
                .absolute()
                .top(spacing(3.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(banner)
                .into_any_element(),
        )
    }
}
