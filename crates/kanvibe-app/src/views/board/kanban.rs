//! 칸반 보드의 헤더와 로딩 스켈레톤(`Board.tsx` 헤더, `BoardRoute.tsx`의 `BoardRouteSkeleton`).
//! 칸반 열·카드 본체는 M7에서 채운다.

use gpui_kit::{
    AnyElement, Div, InteractiveElement, IntoElement, ParentElement, Rgba, SharedString, Styled,
    div, img, px, relative, svg,
};
use kanvibe_domain::i18n::{Locale, Translator};

use crate::assets::{icon_paths, image_paths};
use crate::components::scroll_area::page_scrollbar;
use crate::notifications::unread_badge_label;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeMode, ThemeTokens, radius, spacing, text_scale};
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
/// 실제 헤더 높이(py-3 + 34px 컨트롤 + border-b)
const BOARD_HEADER_HEIGHT: f32 = 59.0;

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
pub fn render_board_skeleton(theme: &ThemeTokens, viewport_height: f32) -> impl IntoElement {
    let document_height = board_document_height(SKELETON_HEADER_HEIGHT, viewport_height);
    div()
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
}

/// 데이터를 읽은 보드. 지금은 헤더와 빈 열 머리까지 그리고, 카드는 M7에서 채운다.
pub fn render_loaded_board(
    theme: &ThemeTokens,
    model: &BoardHeaderModel,
    status_counts: [usize; 5],
    viewport: gpui_kit::Size<gpui_kit::Pixels>,
) -> impl IntoElement {
    let viewport_height = f32::from(viewport.height);
    let document_height = board_document_height(BOARD_HEADER_HEIGHT, viewport_height);
    let columns = board_columns(theme, model.locale, status_counts, viewport_height);
    let content_width =
        f32::from(viewport.width) - f32::from(crate::components::scroll_area::PAGE_SCROLLBAR_WIDTH);
    div()
        .size_full()
        .flex()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .bg(theme.background.page)
                .child(render_board_header(theme, model, content_width))
                .child(
                    div().p(spacing(6.0)).child(
                        div()
                            .flex()
                            .gap(spacing(3.0))
                            .pb(spacing(2.0))
                            .children(columns),
                    ),
                ),
        )
        .child(page_scrollbar(
            document_height,
            0.0,
            theme.border.strong.into(),
        ))
}

/// 상태 순서대로 열 다섯 개. 제목은 CSS `uppercase`처럼 대문자로 바꾼다
fn board_columns(
    theme: &ThemeTokens,
    locale: Locale,
    status_counts: [usize; 5],
    viewport_height: f32,
) -> Vec<AnyElement> {
    let labels = Translator::new(locale, "board.columns");
    ["todo", "progress", "pending", "review", "done"]
        .into_iter()
        .zip(status_dot_colors(theme))
        .zip(status_counts)
        .map(|((label_key, dot_color), count)| {
            let label = labels.translate(label_key).to_uppercase();
            render_board_column(theme, label, dot_color, count, viewport_height).into_any_element()
        })
        .collect()
}

/// `Column.tsx`의 테두리·머리와 빈 목록 영역
fn render_board_column(
    theme: &ThemeTokens,
    label: String,
    dot_color: Rgba,
    count: usize,
    viewport_height: f32,
) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(284.0))
        .max_w(px(340.0))
        .overflow_hidden()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .child(
            div()
                .flex()
                .h(spacing(10.0))
                .items_center()
                .gap(spacing(2.0))
                .border_b_1()
                .border_color(theme.border.subtle)
                .px(spacing(3.0))
                .child(div().size(spacing(2.0)).rounded(radius::FULL).bg(dot_color))
                .child(
                    div()
                        .text_scale(text_scale::PX_11)
                        .css_weight(600)
                        .text_color(theme.text.secondary)
                        .child(label),
                )
                .child(
                    div()
                        .ml_auto()
                        .px(px(6.0))
                        .py(px(2.0))
                        .text_scale(text_scale::PX_11)
                        .text_color(theme.text.muted)
                        .child(count.to_string()),
                ),
        )
        .child(
            div()
                .min_h(px(viewport_height - SKELETON_COLUMN_BODY_VIEWPORT_OFFSET))
                .p(spacing(2.0)),
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

/// 헤더 세그먼트 `Project/Task/All`의 값
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKindFilter {
    Project,
    Task,
    All,
}

impl TaskKindFilter {
    pub const ORDER: [TaskKindFilter; 3] = [Self::Project, Self::Task, Self::All];

    fn message_key(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Task => "task",
            Self::All => "all",
        }
    }
}

/// 헤더가 그리는 데이터. 보드 데이터를 읽은 뒤 채운다
#[derive(Debug, Clone, PartialEq)]
pub struct BoardHeaderModel {
    pub locale: Locale,
    pub task_kind_filter: TaskKindFilter,
    pub selected_project_names: Vec<SharedString>,
    pub sort_active_count: usize,
    pub unread_notification_count: usize,
}

struct HeaderLabels {
    board: Translator,
    settings: Translator,
    common: Translator,
    sort: Translator,
}

impl HeaderLabels {
    fn new(locale: Locale) -> Self {
        Self {
            board: Translator::new(locale, "board"),
            settings: Translator::new(locale, "settings"),
            common: Translator::new(locale, "common"),
            sort: Translator::new(locale, "board.sort"),
        }
    }
}

/// `Board.tsx` 헤더(Linux 배치). 로고는 창 폭 1280 이상(`xl`)에서만 보인다.
pub fn render_board_header(
    theme: &ThemeTokens,
    model: &BoardHeaderModel,
    window_width: f32,
) -> impl IntoElement {
    let labels = HeaderLabels::new(model.locale);
    div()
        .flex()
        .items_center()
        .justify_end()
        .border_b_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .px(spacing(6.0))
        .py(spacing(3.0))
        .when_logo_visible(window_width, theme.mode)
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_end()
                .gap(spacing(3.0))
                .children(header_controls(theme, &labels, model)),
        )
}

/// 헤더 오른쪽 컨트롤을 TS와 같은 순서로 늘어놓는다
fn header_controls(
    theme: &ThemeTokens,
    labels: &HeaderLabels,
    model: &BoardHeaderModel,
) -> Vec<AnyElement> {
    let scan_button = render_icon_button(
        "board-scan",
        icon_paths::FOLDER_PLUS,
        labels.settings.translate("scanTitle"),
        theme.text.secondary,
    )
    .border_1()
    .border_color(theme.border.default)
    .bg(theme.background.surface);
    let settings_button = render_icon_button(
        "board-settings",
        icon_paths::SETTINGS,
        labels.common.translate("settings"),
        theme.text.muted,
    )
    .border_1()
    .border_color(gpui_kit::transparent_black());
    vec![
        render_task_kind_filter(theme, labels, model.task_kind_filter).into_any_element(),
        render_project_filter(theme, labels, &model.selected_project_names).into_any_element(),
        render_desktop_filter(theme, labels).into_any_element(),
        render_sort_button(theme, labels, model.sort_active_count).into_any_element(),
        scan_button.into_any_element(),
        render_new_task_button(theme, labels).into_any_element(),
        settings_button.into_any_element(),
        render_notification_button(theme, labels, model.unread_notification_count)
            .into_any_element(),
    ]
}

/// `NotificationCenterButton` 트리거. 읽지 않은 알림이 있으면 오른쪽 위에 brand 배지를 단다
fn render_notification_button(
    theme: &ThemeTokens,
    labels: &HeaderLabels,
    unread_count: usize,
) -> impl IntoElement {
    let button = render_icon_button(
        "board-notifications",
        icon_paths::BELL,
        labels.common.translate("notifications"),
        theme.text.muted,
    );
    match unread_badge_label(unread_count) {
        Some(badge_label) => button.child(
            div()
                .absolute()
                .top(px(-4.0))
                .right(px(-4.0))
                .min_w(spacing(4.0))
                .h(spacing(4.0))
                .rounded(radius::FULL)
                .bg(theme.brand.primary)
                .px(spacing(1.0))
                .text_scale(text_scale::PX_10)
                .css_weight(600)
                .text_color(theme.text.inverse)
                .flex()
                .items_center()
                .justify_center()
                .child(badge_label),
        ),
        None => button,
    }
}

/// Tailwind `xl` 경계
const LOGO_MIN_WINDOW_WIDTH: f32 = 1280.0;
/// 원본 1024x278 이미지를 높이 32로 줄인 폭
const LOGO_WIDTH: f32 = 1024.0 * 32.0 / 278.0;

trait BoardHeaderLogo: ParentElement + Sized {
    fn when_logo_visible(self, window_width: f32, mode: ThemeMode) -> Self;
}

impl BoardHeaderLogo for Div {
    fn when_logo_visible(self, window_width: f32, mode: ThemeMode) -> Self {
        if window_width < LOGO_MIN_WINDOW_WIDTH {
            return self;
        }
        let logo_path = match mode {
            ThemeMode::Dark => image_paths::LOGO_INVERTED,
            ThemeMode::Light => image_paths::LOGO,
        };
        self.child(
            div()
                .mr_auto()
                .flex_none()
                .pr(spacing(6.0))
                .child(img(logo_path).h(spacing(8.0)).w(px(LOGO_WIDTH))),
        )
    }
}

fn qa_button_layer(label: &str, text: &str) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role: "button",
        name: label.to_string().into(),
        text: text.to_string().into(),
    })
}

fn render_task_kind_filter(
    theme: &ThemeTokens,
    labels: &HeaderLabels,
    active_filter: TaskKindFilter,
) -> impl IntoElement {
    let buttons = TaskKindFilter::ORDER.into_iter().map(|filter| {
        let label = labels
            .board
            .translate(&format!("taskKindFilter.options.{}", filter.message_key()));
        let is_active = filter == active_filter;
        let button = div()
            .relative()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .rounded(radius::BASE)
            .text_scale(text_scale::SM)
            .css_weight(500)
            .child(label.clone())
            .child(qa_button_layer(&label, &label));
        if is_active {
            button
                .bg(theme.brand.primary)
                .text_color(theme.text.inverse)
                .shadow(shadow_sm(theme))
        } else {
            button.text_color(theme.text.secondary)
        }
    });
    div()
        .flex()
        .h(px(34.0))
        .w(px(180.0))
        .flex_none()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
        .p(px(2.0))
        .children(buttons)
}

fn shadow_sm(theme: &ThemeTokens) -> Vec<gpui_kit::BoxShadow> {
    theme
        .shadow
        .sm
        .iter()
        .map(|layer| gpui_kit::BoxShadow {
            color: layer.color.into(),
            offset: gpui_kit::point(px(0.0), px(layer.offset_y)),
            blur_radius: px(layer.blur),
            spread_radius: px(layer.spread),
            inset: false,
        })
        .collect()
}

fn render_project_filter(
    theme: &ThemeTokens,
    labels: &HeaderLabels,
    selected_project_names: &[SharedString],
) -> impl IntoElement {
    let placeholder = labels.board.translate("allProjects");
    let content: AnyElement = if selected_project_names.is_empty() {
        div()
            .px(spacing(1.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.muted)
            .child(placeholder.clone())
            .into_any_element()
    } else {
        div()
            .px(spacing(1.0))
            .text_scale(text_scale::SM)
            .child(selected_project_names.join(", "))
            .into_any_element()
    };
    div().w(spacing(64.0)).flex_none().child(
        div()
            .relative()
            .w_full()
            .px(spacing(2.0))
            .py(spacing(1.0))
            .min_h(px(34.0))
            .bg(theme.background.page)
            .border_1()
            .border_color(theme.border.default)
            .rounded(radius::MD)
            .text_color(theme.text.primary)
            .flex()
            .items_center()
            .gap(spacing(1.0))
            .overflow_hidden()
            .child(content)
            .child(
                svg()
                    .path(icon_paths::CHEVRON_DOWN_SMALL)
                    .size(px(12.0))
                    .ml_auto()
                    .flex_none()
                    .text_color(theme.text.muted),
            )
            .child(qa_button_layer(&placeholder, "")),
    )
}

fn render_desktop_filter(theme: &ThemeTokens, labels: &HeaderLabels) -> impl IntoElement {
    let label = labels.board.translate("allDesktops");
    let aria_label = labels.board.translate("desktopFilter");
    div().relative().w(spacing(48.0)).flex_none().child(
        div()
            .relative()
            .flex()
            .h(px(34.0))
            .w_full()
            .items_center()
            .gap(spacing(2.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .px(spacing(3.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.primary)
            .child(
                svg()
                    .path(icon_paths::MONITOR)
                    .size(px(16.0))
                    .flex_none()
                    .text_color(theme.text.muted),
            )
            .child(div().child(label.clone()))
            .child(
                svg()
                    .path(icon_paths::CHEVRON_DOWN_SMALL)
                    .size(px(12.0))
                    .ml_auto()
                    .flex_none()
                    .text_color(theme.text.muted),
            )
            .child(qa_button_layer(&aria_label, &label)),
    )
}

fn render_sort_button(
    theme: &ThemeTokens,
    labels: &HeaderLabels,
    active_count: usize,
) -> impl IntoElement {
    let label = labels.sort.translate("label");
    let base = div()
        .relative()
        .flex()
        .flex_none()
        .h(px(34.0))
        .items_center()
        .gap(px(6.0))
        .rounded(radius::MD)
        .border_1()
        .px(px(10.0))
        .text_scale(text_scale::SM)
        .css_weight(500);
    let styled = if active_count > 0 {
        base.border_color(theme.border.brand)
            .bg(theme.brand.subtle)
            .text_color(theme.text.primary)
    } else {
        base.border_color(theme.border.default)
            .bg(theme.background.surface)
            .text_color(theme.text.secondary)
    };
    styled
        .child(
            svg()
                .path(icon_paths::SORT_LINES)
                .size(px(18.0))
                .flex_none()
                .text_color(if active_count > 0 {
                    theme.text.primary
                } else {
                    theme.text.secondary
                }),
        )
        .child(label.clone())
        .child(qa_button_layer(&label, &label))
}

fn render_new_task_button(theme: &ThemeTokens, labels: &HeaderLabels) -> impl IntoElement {
    let label = labels.board.translate("newTask");
    div()
        .id("board-new-task")
        .relative()
        .flex_none()
        .px(spacing(4.0))
        .py(px(6.0))
        .text_scale(text_scale::SM)
        .css_weight(500)
        .bg(theme.brand.primary)
        .hover({
            let hover = theme.brand.hover;
            move |style| style.bg(hover)
        })
        .text_color(theme.text.inverse)
        .rounded(radius::MD)
        .child(label.clone())
        .child(qa_button_layer(&label, &label))
}

/// `p-1.5 rounded-md` 아이콘 버튼(20px SVG)
fn render_icon_button(
    id: &'static str,
    icon_path: &'static str,
    label: String,
    icon_color: Rgba,
) -> gpui_kit::Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex_none()
        .rounded(radius::MD)
        .p(px(6.0))
        .child(svg().path(icon_path).size(px(20.0)).text_color(icon_color))
        .child(qa_button_layer(&label, ""))
}
