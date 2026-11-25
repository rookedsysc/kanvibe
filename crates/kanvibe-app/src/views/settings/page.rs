//! 설정 화면 그리기. TS `ProjectSettings variant="page"`의 Tailwind 클래스를 문서 주석에 적었다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{
    AnyElement, Bounds, Context, Div, InteractiveElement, IntoElement, ParentElement, Pixels,
    ScrollHandle, Styled, Window, canvas, div, point, px,
};
use kanvibe_domain::i18n::Translator;

use crate::components::scroll_area::scroll_area;
use crate::components::{TailwindText, box_shadows, fade};
use crate::theme::{ThemePreference, ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::controls::{
    SETTING_LABEL_HEIGHT, SWITCH_HEIGHT, centered_message, chromium_center_offset,
    inline_load_error, keyed_id, make_pressable, native_checkbox, navigation_row, pressable,
    scrollbar_color, section_heading, setting_label, toggle_switch,
};
use crate::views::settings::form::{
    NOTIFICATION_STATUS_OPTIONS, SettingChange, SettingKey, toggle_notification_status,
};
use crate::views::settings::session_type_select::SELECT_HEIGHT;
use crate::views::settings::text_field::themed_text_field;
use crate::views::settings::{LoadedSettings, SettingsView};

/// Tailwind `lg` 경계. 이보다 좁으면 nav가 위쪽 가로 줄이 된다
const WIDE_LAYOUT_MIN_WIDTH: f32 = 1024.0;
/// Tailwind `sm` 경계. 이보다 좁으면 본문 여백이 줄어든다
const SMALL_LAYOUT_MIN_WIDTH: f32 = 640.0;
/// `lg:grid-cols-[240px_minmax(0,1fr)]`
const ASIDE_WIDTH: f32 = 240.0;
/// 테마 선택 묶음 높이(테두리 2 + `p-0.5` 4 + 항목 `py-1` 8 + `text-xs` 16)
const THEME_CHOICES_HEIGHT: f32 = 30.0;
/// `min-h-11`
const PANE_CHAT_ROW_MIN_HEIGHT: f32 = 44.0;
/// 갱신 실패 배너 최대 폭(`w-[min(32rem,calc(100%-2rem))]`)
const BANNER_MAX_WIDTH: f32 = 512.0;

/// nav 위쪽 여백. macOS 데스크톱은 신호등 버튼 아래로 Board 링크를 내린다(`pt-16`), 그 밖은 `pt-4 lg:pt-6`
pub fn aside_top_padding(is_mac: bool, is_wide: bool) -> gpui_kit::Pixels {
    match (is_mac, is_wide) {
        (true, _) => spacing(16.0),
        (false, true) => spacing(6.0),
        (false, false) => spacing(4.0),
    }
}

/// 섹션 nav 항목: (섹션 id, 라벨 키)
const SECTION_NAV: [(&str, &str); 7] = [
    ("appearance", "appearanceSection"),
    ("detail", "detailPageSection"),
    ("creation", "taskCreationSection"),
    ("mobile", "mobilePairingSection"),
    ("notifications", "notificationSection"),
    ("background-sync", "backgroundSyncSection"),
    ("keyboard", "keyboardSection"),
];

/// 섹션 위치. nav를 누르면 그 섹션이 스크롤 영역 맨 위에 오도록 옮긴다(`scrollIntoView({block: "start"})`)
#[derive(Clone, Default)]
pub struct SectionPositions(Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>);

impl SectionPositions {
    fn marker(&self, section_id: &'static str) -> impl IntoElement {
        let positions = self.clone();
        div().absolute().inset_0().child(
            canvas(
                move |bounds, _, _| {
                    positions.0.borrow_mut().insert(section_id, bounds);
                },
                |_, _, _, _| {},
            )
            .size_full(),
        )
    }

    fn scroll_to(&self, section_id: &str, scroll_handle: &ScrollHandle) {
        let Some(section) = self.0.borrow().get(section_id).copied() else {
            return;
        };
        let viewport = scroll_handle.bounds();
        let offset = scroll_handle.offset();
        let content_top = section.origin.y - viewport.origin.y - offset.y;
        let max_scroll = scroll_handle.max_offset().y;
        let target = content_top.max(px(0.0)).min(max_scroll);
        scroll_handle.set_offset(point(offset.x, -target));
    }
}

pub fn render_settings_page(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let theme = *cx.global::<ThemeTokens>();
    if view.loaded.is_none() {
        return render_first_load(view, &theme, cx).into_any_element();
    }
    let width = f32::from(window.viewport_size().width);
    let is_wide = width >= WIDE_LAYOUT_MIN_WIDTH;
    let content = render_content(view, &theme, width, window, cx);
    let aside = render_aside(view, &theme, is_wide, cx);
    let color = scrollbar_color(&theme);
    let body = if is_wide {
        div()
            .flex()
            .size_full()
            .child(div().h_full().bg(theme.background.page).child(aside))
            .child(div().flex_1().min_w_0().h_full().child(scroll_area(
                "settings-scroll",
                &view.scroll_handle,
                color,
                content.min_h_full().bg(theme.background.page),
            )))
            .into_any_element()
    } else {
        scroll_area(
            "settings-scroll",
            &view.scroll_handle,
            color,
            div()
                .flex()
                .flex_col()
                .min_h_full()
                .bg(theme.background.page)
                .child(aside)
                .child(content),
        )
    };
    // 스크롤바 홈은 페이지 바탕이 아니라 창 바탕(html 캔버스)을 비추므로 바탕은 내용에만 칠한다
    div()
        .relative()
        .size_full()
        .text_color(theme.text.primary)
        .child(body)
        .children(render_refresh_banner(view, &theme, width, cx))
        .into_any_element()
}

/// 첫 조회 전: `Loading...`(5초가 지나면 지연 안내), 실패면 사유와 재시도
fn render_first_load(
    view: &SettingsView,
    theme: &ThemeTokens,
    cx: &mut Context<SettingsView>,
) -> Div {
    let common = view.translator("common");
    let content = match view.load.failure_reason.clone() {
        Some(reason) => inline_load_error(
            theme,
            "settings-load-retry",
            common.translate_with("loadFailed", &[("reason", &reason)]),
            common.translate("retry"),
            cx.listener(|this, _, _, cx| this.retry_load(cx)),
        )
        .w_full()
        .max_w(px(448.0))
        .into_any_element(),
        None => div()
            .flex()
            .flex_col()
            .items_center()
            .gap(spacing(3.0))
            .child("Loading...")
            .children(view.load.is_load_delayed.then(|| {
                div()
                    .text_scale(text_scale::XS)
                    .child(common.translate("loadDelayed"))
            }))
            .into_any_element(),
    };
    centered_message(theme, content)
}

/// 데이터가 있는데 다시 읽다 실패하면 위쪽 가운데 배너(`fixed left-1/2 top-3 z-[500] shadow-md`)
fn render_refresh_banner(
    view: &SettingsView,
    theme: &ThemeTokens,
    window_width: f32,
    cx: &mut Context<SettingsView>,
) -> Option<impl IntoElement> {
    let reason = view.load.failure_reason.clone()?;
    let common = view.translator("common");
    let banner_width = BANNER_MAX_WIDTH.min(window_width - 32.0);
    Some(
        div()
            .absolute()
            .top(spacing(3.0))
            .left(px((window_width - banner_width) / 2.0))
            .w(px(banner_width))
            .child(
                inline_load_error(
                    theme,
                    "settings-refresh-retry",
                    common.translate_with("refreshFailed", &[("reason", &reason)]),
                    common.translate("retry"),
                    cx.listener(|this, _, _, cx| this.retry_load(cx)),
                )
                .shadow(box_shadows(&theme.shadow.md)),
            ),
    )
}

/// 왼쪽(넓은 창) 또는 위쪽(좁은 창) 섹션 nav.
/// `border-b border-border-default bg-bg-surface/80 px-4 pb-4 pt-4 lg:h-screen lg:border-b-0 lg:border-r lg:pb-6 lg:pt-6`
fn render_aside(
    view: &SettingsView,
    theme: &ThemeTokens,
    is_wide: bool,
    cx: &mut Context<SettingsView>,
) -> Div {
    let settings = view.translator("settings");
    let aside = div()
        .flex_none()
        .bg(fade(theme.background.surface, 0.8))
        .border_color(theme.border.default)
        .px(spacing(4.0));
    let top_padding = aside_top_padding(cfg!(target_os = "macos"), is_wide);
    let aside = if is_wide {
        aside
            .w(px(ASIDE_WIDTH))
            .h_full()
            .border_r_1()
            .pt(top_padding)
            .pb(spacing(6.0))
    } else {
        // 좁은 창의 가로 nav 줄은 Chromium에서 1px 더 높다(overflow-x 스크롤 상자)
        aside.w_full().border_b_1().pt(top_padding).pb(px(17.0))
    };
    let mut buttons = Vec::new();
    for (section_id, label_key) in SECTION_NAV {
        buttons.push(
            render_nav_button(view, theme, section_id, settings.translate(label_key), cx)
                .into_any_element(),
        );
    }
    let nav = div().flex().gap(spacing(1.0)).text_scale(text_scale::SM);
    let nav = if is_wide {
        nav.flex_col()
    } else {
        nav.overflow_hidden()
    };
    aside
        .child(render_board_link(view, theme))
        .child(nav.children(buttons))
}

/// `mb-8 inline-flex items-center gap-3 text-xs font-medium text-text-muted hover:text-text-primary`
fn render_board_link(view: &SettingsView, theme: &ThemeTokens) -> impl IntoElement {
    let hover = theme.text.primary;
    let navigator = view.screen.navigator.clone();
    let link = pressable("settings-board-link", "link", "Board", "Board")
        .mb(spacing(8.0))
        .flex()
        .items_center()
        .gap(spacing(3.0))
        .text_scale(text_scale::XS)
        .css_weight(500)
        .text_color(theme.text.muted)
        .hover(move |style| style.text_color(hover))
        .child("←")
        .child("Board");
    // 링크는 body 줄(21px) 안의 inline-flex라 기준선을 맞추느라 3px 내려앉는다
    div()
        .flex()
        .pt(px(3.0))
        .child(make_pressable(link, theme, false, move |_, _, cx| {
            navigator.navigate("/", cx)
        }))
}

/// `block shrink-0 rounded-md px-3 py-2 text-text-secondary hover:bg-bg-page hover:text-text-primary`
fn render_nav_button(
    view: &SettingsView,
    theme: &ThemeTokens,
    section_id: &'static str,
    label: String,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let (hover_background, hover_text) = (theme.background.page, theme.text.primary);
    let button = pressable(
        keyed_id("settings-nav", section_id),
        "button",
        label.clone(),
        label.clone(),
    )
    .flex_none()
    .rounded(radius::MD)
    .px(spacing(3.0))
    .py(spacing(2.0))
    .text_color(theme.text.secondary)
    .hover(move |style| style.bg(hover_background).text_color(hover_text))
    .child(label);
    let positions = view.section_positions.clone();
    make_pressable(
        button,
        theme,
        false,
        cx.listener(move |this, _, _, cx| {
            positions.scroll_to(section_id, &this.scroll_handle);
            cx.notify();
        }),
    )
}

/// `min-h-screen min-w-0 px-4 py-5 sm:px-8 sm:py-6`
fn render_content(
    view: &mut SettingsView,
    theme: &ThemeTokens,
    window_width: f32,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Div {
    let is_small = window_width < SMALL_LAYOUT_MIN_WIDTH;
    let content = div().flex().flex_col().min_w_0();
    let content = if is_small {
        content.px(spacing(4.0)).py(spacing(5.0))
    } else {
        content.px(spacing(8.0)).py(spacing(6.0))
    };
    let settings = view.translator("settings");
    content
        .child(render_header(theme, &settings))
        .children(render_upper_sections(view, theme, &settings, cx))
        .children(render_lower_sections(view, theme, &settings, window, cx))
}

/// 외관, AI 계정·Pane 레이아웃 링크, 상세 페이지, 작업 생성
fn render_upper_sections(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> [AnyElement; 5] {
    let positions = view.section_positions.clone();
    let ai_accounts_label = settings.translate("aiAccountsLink");
    let pane_layout_label = settings.translate("paneLayoutLink");
    [
        section(&positions, "appearance", theme)
            .child(render_appearance(view, theme, settings, cx))
            .into_any_element(),
        link_section(
            view,
            theme,
            "settings-ai-accounts",
            ai_accounts_label,
            "/ai-accounts",
        )
        .into_any_element(),
        link_section(
            view,
            theme,
            "settings-pane-layout",
            pane_layout_label,
            "/pane-layout",
        )
        .into_any_element(),
        section(&positions, "detail", theme)
            .child(render_detail(view, theme, settings, cx))
            .into_any_element(),
        section(&positions, "creation", theme)
            .child(render_creation(view, theme, settings, cx))
            .into_any_element(),
    ]
}

/// 모바일 연결, 알림, 백그라운드 Sync, 키보드
fn render_lower_sections(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> [AnyElement; 4] {
    let positions = view.section_positions.clone();
    let mobile_pairing = loaded(view).map(|loaded| loaded.mobile_pairing.clone());
    [
        div()
            .relative()
            .child(positions.marker("mobile"))
            .children(mobile_pairing)
            .into_any_element(),
        section(&positions, "notifications", theme)
            .child(render_notifications(view, theme, settings, cx))
            .into_any_element(),
        section(&positions, "background-sync", theme)
            .child(render_background_sync(view, theme, settings, window, cx))
            .into_any_element(),
        section(&positions, "keyboard", theme)
            .child(render_keyboard(view, theme, settings, cx))
            .into_any_element(),
    ]
}

fn loaded(view: &SettingsView) -> Option<&LoadedSettings> {
    view.loaded.as_ref()
}

/// `p-4 border-b border-border-default` 섹션 상자. 위치를 nav 이동용으로 남긴다
fn section(positions: &SectionPositions, section_id: &'static str, theme: &ThemeTokens) -> Div {
    div()
        .relative()
        .p(spacing(4.0))
        .border_b_1()
        .border_color(theme.border.default)
        .child(positions.marker(section_id))
}

/// `flex items-center justify-between border-b px-4 py-4`, 제목 `text-sm font-semibold`, 설명 `mt-1 text-xs text-text-muted`
fn render_header(theme: &ThemeTokens, settings: &Translator) -> impl IntoElement {
    div()
        .border_b_1()
        .border_color(theme.border.default)
        .px(spacing(4.0))
        .py(spacing(4.0))
        .child(
            div()
                .text_scale(text_scale::SM)
                .css_weight(600)
                .text_color(theme.text.primary)
                .child(settings.translate("title")),
        )
        .child(
            div()
                .mt(spacing(1.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(settings.translate("pageDescription")),
        )
}

fn link_section(
    view: &SettingsView,
    theme: &ThemeTokens,
    id: &'static str,
    label: String,
    href: &'static str,
) -> impl IntoElement {
    let navigator = view.screen.navigator.clone();
    div()
        .p(spacing(4.0))
        .border_b_1()
        .border_color(theme.border.default)
        .child(navigation_row(theme, id, label, move |_, _, cx| {
            navigator.navigate(href, cx)
        }))
}

/// 저장 실패 줄(`renderSettingSaveFailure`, `mt-2`)
fn render_save_failure(
    view: &SettingsView,
    theme: &ThemeTokens,
    key: SettingKey,
    cx: &mut Context<SettingsView>,
) -> Option<impl IntoElement> {
    let failure = loaded(view)?.form.failure_for(key)?;
    let settings = view.translator("settings");
    let common = view.translator("common");
    Some(
        inline_load_error(
            theme,
            ("settings-save-retry", key as usize),
            settings.translate_with("settingSaveFailed", &[("reason", &failure.reason)]),
            common.translate("retry"),
            cx.listener(|this, _, _, cx| this.retry_failed_save(cx)),
        )
        .mt(spacing(2.0)),
    )
}

/// 설정 제목·설명 줄(39px) 옆에 컨트롤을 세로 가운데로 둔다(`flex items-center justify-between`)
fn toggle_row(label: Div, control: impl IntoElement, control_height: f32) -> Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .child(label)
        .child(
            div()
                .mt(chromium_center_offset(SETTING_LABEL_HEIGHT, control_height))
                .child(control),
        )
}

fn render_appearance(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> Div {
    div()
        .child(section_heading(
            theme,
            settings.translate("appearanceSection"),
        ))
        .child(toggle_row(
            setting_label(
                theme,
                settings.translate("themePreference"),
                settings.translate("themePreferenceDescription"),
            ),
            render_theme_choices(view, theme, settings, cx),
            THEME_CHOICES_HEIGHT,
        ))
        .children(render_save_failure(view, theme, SettingKey::Theme, cx))
}

/// `inline-flex rounded-md border bg-bg-page p-0.5`, 항목 `rounded-[5px] px-2.5 py-1 text-xs font-medium`,
/// 선택 `bg-bg-surface text-text-primary shadow-xs`, 나머지 `text-text-muted hover:text-text-primary`
fn render_theme_choices(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let Some(loaded) = loaded(view) else {
        return div();
    };
    let current = loaded.form.values.theme_preference;
    let is_saving = loaded.form.is_saving(SettingKey::Theme);
    let mut choices = Vec::new();
    for (preference, key) in [
        (ThemePreference::System, "system"),
        (ThemePreference::Dark, "dark"),
        (ThemePreference::Light, "light"),
    ] {
        let label = settings.translate(&format!("theme.{key}"));
        let choice = render_theme_choice(theme, key, label, preference == current);
        choices.push(make_pressable(
            choice,
            theme,
            is_saving,
            cx.listener(move |this, _, _, cx| {
                this.save_setting(SettingChange::Theme(preference), cx)
            }),
        ));
    }
    div()
        .flex()
        .flex_none()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
        .p(spacing(0.5))
        .children(choices)
}

fn render_theme_choice(
    theme: &ThemeTokens,
    key: &'static str,
    label: String,
    is_selected: bool,
) -> gpui_kit::Stateful<Div> {
    let hover = theme.text.primary;
    let choice = pressable(
        keyed_id("theme-choice", key),
        "button",
        label.clone(),
        label.clone(),
    )
    .rounded(px(5.0))
    .px(spacing(2.5))
    .py(spacing(1.0))
    .text_scale(text_scale::XS)
    .css_weight(500)
    .child(label);
    if is_selected {
        choice
            .bg(theme.background.surface)
            .text_color(theme.text.primary)
            .shadow(box_shadows(&theme.shadow.xs))
    } else {
        choice
            .text_color(theme.text.muted)
            .hover(move |style| style.text_color(hover))
    }
}

fn render_detail(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> Div {
    let Some(loaded) = loaded(view) else {
        return div();
    };
    let is_collapsed = loaded.form.values.is_sidebar_default_collapsed;
    let toggle = toggle_switch(
        theme,
        "sidebar-default-collapsed",
        settings.translate("sidebarDefaultCollapsed"),
        is_collapsed,
        loaded.form.is_saving(SettingKey::Sidebar),
        cx.listener(move |this, _, _, cx| {
            this.save_setting(SettingChange::SidebarDefaultCollapsed(!is_collapsed), cx)
        }),
    );
    div()
        .child(section_heading(
            theme,
            settings.translate("detailPageSection"),
        ))
        .child(toggle_row(
            setting_label(
                theme,
                settings.translate("sidebarDefaultCollapsed"),
                settings.translate("sidebarDefaultCollapsedDescription"),
            ),
            toggle,
            SWITCH_HEIGHT,
        ))
        .children(render_save_failure(view, theme, SettingKey::Sidebar, cx))
        .child(render_pane_chat_setting(view, theme, settings, cx))
}

/// TS `PaneChatSettings`: `mt-4 border-t border-border-subtle pt-4`, 줄 `flex min-h-11 items-center justify-between gap-4`,
/// 체크박스 `h-5 w-5 accent-brand-primary`
fn render_pane_chat_setting(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let preference = view.pane_chat;
    let label = settings.translate("defaultChatMode");
    let checkbox = native_checkbox(
        theme,
        "pane-chat-default",
        label.clone(),
        20.0,
        preference.is_default_enabled,
        !preference.is_loaded || preference.is_saving,
        cx.listener(move |this, _, _, cx| {
            this.set_pane_chat_default_enabled(!preference.is_default_enabled, cx)
        }),
    );
    let label_offset = chromium_center_offset(PANE_CHAT_ROW_MIN_HEIGHT, SETTING_LABEL_HEIGHT);
    let description = settings.translate("defaultChatModeDescription");
    let row = div()
        .flex()
        .min_h(px(PANE_CHAT_ROW_MIN_HEIGHT))
        .items_start()
        .justify_between()
        .gap(spacing(4.0))
        .child(setting_label(theme, label, description).mt(label_offset))
        .child(checkbox.mt(chromium_center_offset(PANE_CHAT_ROW_MIN_HEIGHT, 20.0)));
    div()
        .mt(spacing(4.0))
        .border_t_1()
        .border_color(theme.border.subtle)
        .pt(spacing(4.0))
        .child(row)
        .children(preference.has_error.then(|| {
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.status.error)
                .child(settings.translate("chatSettingsError"))
        }))
}

fn render_creation(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> Div {
    let Some(loaded) = loaded(view) else {
        return div();
    };
    let is_saving = loaded.form.is_saving(SettingKey::SessionType);
    loaded
        .default_session_type
        .update(cx, |select, cx| select.set_disabled(is_saving, cx));
    div()
        .child(section_heading(
            theme,
            settings.translate("taskCreationSection"),
        ))
        .child(toggle_row(
            setting_label(
                theme,
                settings.translate("defaultSessionType"),
                settings.translate("defaultSessionTypeDescription"),
            ),
            loaded.default_session_type.clone(),
            SELECT_HEIGHT,
        ))
        .children(render_save_failure(
            view,
            theme,
            SettingKey::SessionType,
            cx,
        ))
        .child(loaded.bulk_change.clone())
}

fn render_notifications(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> Div {
    let Some(loaded) = loaded(view) else {
        return div();
    };
    let notifications = loaded.form.values.notifications.clone();
    let is_saving = loaded.form.is_saving(SettingKey::Notifications);
    let is_enabled = notifications.is_enabled;
    let toggle = switch_row(
        theme,
        settings,
        (
            "notification-enabled",
            "notificationEnabled",
            "notificationEnabledDescription",
        ),
        (is_enabled, is_saving),
        cx.listener(move |this, _, _, cx| {
            this.save_setting(SettingChange::NotificationEnabled(!is_enabled), cx)
        }),
    );
    let filter = render_notification_filter(
        view,
        theme,
        settings,
        &notifications.enabled_statuses,
        is_saving || !is_enabled,
        cx,
    );
    div()
        .child(section_heading(
            theme,
            settings.translate("notificationSection"),
        ))
        .child(toggle)
        .child(dimmed_unless(filter, is_enabled))
        .children(render_save_failure(
            view,
            theme,
            SettingKey::Notifications,
            cx,
        ))
}

/// 꺼진 기능의 하위 설정은 흐리게(`opacity-40 pointer-events-none`)
fn dimmed_unless(element: Div, is_enabled: bool) -> Div {
    if is_enabled {
        element
    } else {
        element.opacity(0.4)
    }
}

/// 알림 받을 상태 제목과 상태 칩
fn render_notification_filter(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    statuses: &[String],
    is_disabled: bool,
    cx: &mut Context<SettingsView>,
) -> Div {
    div()
        .mt(spacing(4.0))
        .child(div().mb(spacing(2.0)).child(setting_label(
            theme,
            settings.translate("notificationStatusFilter"),
            settings.translate("notificationStatusFilterDescription"),
        )))
        .child(render_status_chips(view, theme, statuses, is_disabled, cx))
}

/// Sync 주기 제목과 입력란(`mt-4 flex items-center justify-between`)
fn render_interval_row(
    loaded: &LoadedSettings,
    theme: &ThemeTokens,
    settings: &Translator,
    is_enabled: bool,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Div {
    div()
        .mt(spacing(4.0))
        .flex()
        .items_center()
        .justify_between()
        .child(setting_label(
            theme,
            settings.translate("backgroundSyncInterval"),
            settings.translate("backgroundSyncIntervalDescription"),
        ))
        .child(render_interval_input(
            loaded, theme, settings, is_enabled, window, cx,
        ))
}

/// 제목·설명 왼쪽, 스위치 오른쪽인 설정 줄. `keys`는 (스위치 id, 제목 키, 설명 키), `state`는 (켜짐, 저장 중)
fn switch_row(
    theme: &ThemeTokens,
    settings: &Translator,
    keys: (&'static str, &str, &str),
    state: (bool, bool),
    on_toggle: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> Div {
    let (id, title_key, description_key) = keys;
    let (is_on, is_saving) = state;
    let title = settings.translate(title_key);
    let toggle = toggle_switch(theme, id, title.clone(), is_on, is_saving, on_toggle);
    toggle_row(
        setting_label(theme, title, settings.translate(description_key)),
        toggle,
        SWITCH_HEIGHT,
    )
}

/// 칩 `px-2.5 py-1 text-xs rounded-full border`, 선택 `bg-brand-primary/15 border-brand-primary text-brand-primary`,
/// 나머지 `bg-bg-page border-border-default text-text-muted hover:border-border-strong`
fn render_status_chips(
    view: &SettingsView,
    theme: &ThemeTokens,
    selected: &[String],
    is_disabled: bool,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let columns = view.translator("board");
    let chips = NOTIFICATION_STATUS_OPTIONS.map(|status| {
        let label = columns.translate(&format!("columns.{status}"));
        let is_selected = selected.iter().any(|value| value == status);
        let next = toggle_notification_status(selected, status);
        let hover_border = theme.border.strong;
        let chip = pressable(
            keyed_id("notification-status", status),
            "button",
            label.clone(),
            label.clone(),
        )
        .px(spacing(2.5))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .rounded(radius::FULL)
        .border_1();
        let chip = if is_selected {
            chip.bg(fade(theme.brand.primary, 0.15))
                .border_color(theme.brand.primary)
                .text_color(theme.brand.primary)
        } else {
            chip.bg(theme.background.page)
                .border_color(theme.border.default)
                .text_color(theme.text.muted)
                .hover(move |style| style.border_color(hover_border))
        };
        let chip = chip.child(label);
        if is_disabled {
            return chip;
        }
        make_pressable(
            chip,
            theme,
            false,
            cx.listener(move |this, _, _, cx| {
                this.save_setting(SettingChange::NotificationStatuses(next.clone()), cx)
            }),
        )
    });
    div().flex().flex_wrap().gap(spacing(1.5)).children(chips)
}

fn render_background_sync(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Div {
    let Some(loaded) = loaded(view) else {
        return div();
    };
    let is_enabled = loaded.form.values.background_sync.is_enabled;
    let toggle = switch_row(
        theme,
        settings,
        (
            "background-sync-enabled",
            "backgroundSyncEnabled",
            "backgroundSyncEnabledDescription",
        ),
        (
            is_enabled,
            loaded.form.is_saving(SettingKey::BackgroundSync),
        ),
        cx.listener(move |this, _, _, cx| {
            this.save_setting(SettingChange::BackgroundSyncEnabled(!is_enabled), cx)
        }),
    );
    let interval = render_interval_row(loaded, theme, settings, is_enabled, window, cx);
    div()
        .child(section_heading(
            theme,
            settings.translate("backgroundSyncSection"),
        ))
        .child(toggle)
        .child(dimmed_unless(interval, is_enabled))
        .children(render_save_failure(
            view,
            theme,
            SettingKey::BackgroundSync,
            cx,
        ))
}

/// `w-20 px-2 py-1 text-sm bg-bg-page border rounded-md text-right focus:border-brand-primary disabled:opacity-50` + "분"
fn render_interval_input(
    loaded: &LoadedSettings,
    theme: &ThemeTokens,
    settings: &Translator,
    is_enabled: bool,
    _: &mut Window,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let field = themed_text_field(&loaded.interval_input, theme, theme.text.primary, cx);
    let input = div()
        .w(px(80.0))
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::SM)
        .bg(theme.background.page)
        .border_1()
        .border_color(theme.border.default)
        .rounded(radius::MD)
        .flex()
        .justify_end()
        .child(field);
    div()
        .flex()
        .items_center()
        .gap(spacing(1.5))
        .child(if is_enabled {
            input
        } else {
            input.opacity(0.5)
        })
        .child(
            div()
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(settings.translate("backgroundSyncIntervalUnit")),
        )
}

fn render_keyboard(
    view: &SettingsView,
    theme: &ThemeTokens,
    settings: &Translator,
    cx: &mut Context<SettingsView>,
) -> Div {
    let Some(loaded) = loaded(view) else {
        return div();
    };
    let is_enabled = loaded.form.values.is_vim_mode_enabled;
    let toggle = toggle_switch(
        theme,
        "vim-mode-enabled",
        settings.translate("vimModeEnabled"),
        is_enabled,
        loaded.form.is_saving(SettingKey::VimMode),
        cx.listener(move |this, _, _, cx| {
            this.save_setting(SettingChange::VimModeEnabled(!is_enabled), cx)
        }),
    );
    let navigator = view.screen.navigator.clone();
    div()
        .child(section_heading(
            theme,
            settings.translate("keyboardSection"),
        ))
        .child(toggle_row(
            setting_label(
                theme,
                settings.translate("vimModeEnabled"),
                settings.translate("vimModeEnabledDescription"),
            ),
            toggle,
            SWITCH_HEIGHT,
        ))
        .children(render_save_failure(view, theme, SettingKey::VimMode, cx))
        .child(div().mt(spacing(4.0)).child(navigation_row(
            theme,
            "settings-shortcuts",
            settings.translate("shortcutsLink"),
            move |_, _, cx| navigator.navigate("/settings/shortcuts", cx),
        )))
}
