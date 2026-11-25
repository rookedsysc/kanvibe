//! `/:locale/settings/shortcuts` 단축키 설정 화면(TS `ShortcutSettingsRoute`). 27개 명령을 그룹별로 보여 주고,
//! "녹화"를 누른 뒤 누른 조합을 정규 표기(`Mod+…`)로 저장한다. 녹화 중에는 이 창의 앱 단축키 처리를 멈춰,
//! 새 조합을 누르는 순간 그 조합의 원래 명령이 함께 실행되지 않게 한다. 저장한 뒤에는 열린 창 모두의 단축키 표를 다시 읽는다.

pub mod model;

use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, KeystrokeEvent, ParentElement,
    Render, ScrollHandle, Styled, Subscription, Task, Window, WindowId, div, px,
};
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::shortcuts::bindings::{
    ShortcutBindings, ShortcutCommandDefinition, ShortcutCommandId, shortcut_command_definitions,
};
use kanvibe_domain::shortcuts::notation::format_shortcut_for_display;
use kanvibe_domain::task_detail_dock::resolve_task_detail_dock_label;

use crate::components::scroll_area::scroll_area;
use crate::components::{TailwindText, fade};
use crate::services::{AppServices, ServiceResult};
use crate::shortcuts::{
    begin_shortcut_capture, end_shortcut_capture, reload_shortcut_bindings,
    shortcut_event_from_keystroke,
};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::ScreenContext;
use crate::views::settings::controls::set_window_title_once;
use crate::views::settings::controls::{keyed_id, pressable, scrollbar_color, visible_text};
use crate::views::settings::service_call::{call_database_read, call_database_service, failure};
use model::{RecordingKeyOutcome, SHORTCUT_GROUP_ORDER, ShortcutNotice, ShortcutSettingsModel};

/// `max-w-3xl`
const CONTENT_MAX_WIDTH: f32 = 768.0;
/// TS 비활성 버튼(`disabled:opacity-40`)
const DISABLED_OPACITY: f32 = 0.4;

pub struct ShortcutSettingsView {
    screen: ScreenContext,
    /// 창 제목을 걸었는지(TS `document.title`)
    is_title_set: bool,
    pub model: ShortcutSettingsModel,
    scroll_handle: ScrollHandle,
    /// 녹화 중인 창. 녹화가 끝나거나 화면이 사라지면 표시를 지운다
    capturing_window: Option<WindowId>,
    _recording_keys: Option<Subscription>,
    _requests: Vec<Task<()>>,
}

impl ShortcutSettingsView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let platform = cx.global::<AppServices>().shortcut_platform;
        let mut view = Self {
            screen,
            is_title_set: false,
            model: ShortcutSettingsModel::new(platform),
            scroll_handle: ScrollHandle::new(),
            capturing_window: None,
            _recording_keys: None,
            _requests: Vec::new(),
        };
        view.load_bindings(cx);
        cx.on_release(|this, cx| this.stop_capture(cx)).detach();
        view
    }

    fn translator(&self, namespace: &'static str) -> Translator {
        Translator::new(self.screen.route.locale, namespace)
    }

    /// 저장된 표를 읽는다(TS `loadShortcutBindings`)
    fn load_bindings(&mut self, cx: &mut Context<Self>) {
        let platform = self.model.platform;
        let request = call_database_read(cx, "appSettings.getShortcutBindings", move |database| {
            AppSettingsService::new(database, platform)
                .get_shortcut_bindings()
                .map_err(failure)
        });
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.model
                    .finish_load(result.map_err(|failure| failure.reason));
                cx.notify();
            });
        }));
    }

    fn track(&mut self, task: Task<()>) {
        self._requests.retain(|request| !request.is_ready());
        self._requests.push(task);
    }

    /// 표를 통째로 저장하고, 성공하면 열린 창 모두의 단축키를 다시 읽힌다
    fn persist(&mut self, bindings: ShortcutBindings, cx: &mut Context<Self>) {
        let platform = self.model.platform;
        let overrides =
            serde_json::to_string(&bindings.collect_overrides(platform)).unwrap_or_default();
        let saved = bindings.clone();
        let request: Task<ServiceResult<()>> = call_database_service(
            cx,
            "appSettings.setShortcutBindings",
            format!("[{overrides}]"),
            move |database| {
                AppSettingsService::new(database, platform)
                    .set_shortcut_bindings(&bindings)
                    .map_err(failure)
            },
        );
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                let is_saved = result.is_ok();
                this.model
                    .finish_save(saved, result.map_err(|failure| failure.reason));
                if is_saved {
                    reload_shortcut_bindings(cx);
                }
                cx.notify();
            });
        }));
    }

    pub fn toggle_recording(
        &mut self,
        command_id: ShortcutCommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.model.toggle_recording(command_id);
        if self.model.recording.is_some() {
            self.start_capture(window, cx);
        } else {
            self.stop_capture(cx);
        }
        cx.notify();
    }

    /// 이 창의 키 입력을 녹화로만 받는다. 앱 단축키 가로채기는 녹화 중인 창을 건너뛴다
    fn start_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let window_id = window.window_handle().window_id();
        begin_shortcut_capture(window_id, cx);
        self.capturing_window = Some(window_id);
        let view = cx.entity().downgrade();
        self._recording_keys = Some(cx.intercept_keystrokes(
            move |event: &KeystrokeEvent, window, cx| {
                if window.window_handle().window_id() != window_id {
                    return;
                }
                cx.stop_propagation();
                let _ = view.update(cx, |this, cx| this.handle_recording_keystroke(event, cx));
            },
        ));
    }

    fn stop_capture(&mut self, cx: &mut App) {
        self._recording_keys = None;
        if let Some(window_id) = self.capturing_window.take() {
            end_shortcut_capture(window_id, cx);
        }
    }

    fn handle_recording_keystroke(&mut self, event: &KeystrokeEvent, cx: &mut Context<Self>) {
        let key_event = shortcut_event_from_keystroke(&event.keystroke);
        match self.model.handle_recording_key(&key_event) {
            RecordingKeyOutcome::Save(bindings) => {
                self.stop_capture(cx);
                self.persist(bindings, cx);
            }
            RecordingKeyOutcome::Cancelled => self.stop_capture(cx),
            RecordingKeyOutcome::Continue => {}
        }
        cx.notify();
    }

    pub fn reset_command(&mut self, command_id: ShortcutCommandId, cx: &mut Context<Self>) {
        if let Some(bindings) = self.model.reset_command(command_id) {
            self.persist(bindings, cx);
        }
        cx.notify();
    }

    pub fn reset_all(&mut self, cx: &mut Context<Self>) {
        self.stop_capture(cx);
        if let Some(bindings) = self.model.reset_all() {
            self.persist(bindings, cx);
        }
        cx.notify();
    }
}

/// 명령 라벨(`settings.shortcuts.commands.<labelKey>`). 도크 명령은 도크가 쓰는 항목 이름을 그대로 끼운다
pub fn describe_command(
    definition: &ShortcutCommandDefinition,
    locale: kanvibe_domain::i18n::Locale,
) -> String {
    let shortcuts = Translator::new(locale, "settings");
    let task_detail = Translator::new(locale, "taskDetail");
    let item = definition
        .dock_item_id
        .map(|item_id| resolve_task_detail_dock_label(|key| task_detail.translate(key), item_id))
        .unwrap_or_default();
    let index = definition.label_index.unwrap_or(0).to_string();
    shortcuts.translate_with(
        &format!("shortcuts.commands.{}", definition.label_key),
        &[("index", &index), ("item", &item)],
    )
}

impl Render for ShortcutSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.translator("settings").translate("shortcuts.title");
        set_window_title_once(&mut self.is_title_set, &title, window);
        let theme = *cx.global::<ThemeTokens>();
        let mut sections = Vec::new();
        for group in SHORTCUT_GROUP_ORDER {
            sections.push(self.render_group(&theme, group, cx));
        }
        // 스크롤바 홈은 페이지 바탕이 아니라 창 바탕(html 캔버스)을 비춘다
        let page = div()
            .w_full()
            .min_h_full()
            .bg(theme.background.page)
            .px(spacing(6.0))
            .py(spacing(8.0))
            .child(
                div()
                    .mx_auto()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH))
                    .child(self.render_back_link(&theme))
                    .child(self.render_header(&theme, cx))
                    .children(self.render_notice(&theme))
                    .children(sections),
            );
        div().size_full().child(scroll_area(
            "shortcut-settings-scroll",
            &self.scroll_handle,
            scrollbar_color(&theme),
            page,
        ))
    }
}

impl ShortcutSettingsView {
    /// `mb-8 inline-flex items-center gap-3 text-xs font-medium text-text-muted`(body 줄 안이라 3px 내려앉는다)
    fn render_back_link(&self, theme: &ThemeTokens) -> impl IntoElement {
        let title = self.translator("settings").translate("shortcuts.title");
        let navigator = self.screen.navigator.clone();
        let hover = theme.text.primary;
        let link = pressable(
            "shortcut-settings-back",
            "link",
            title.clone(),
            title.clone(),
        )
        .flex()
        .items_center()
        .gap(spacing(3.0))
        .text_scale(text_scale::XS)
        .css_weight(500)
        .text_color(theme.text.muted)
        .hover(move |style| style.text_color(hover))
        .child("←")
        .child(title);
        div().flex().pt(px(3.0)).mb(px(33.0)).child(
            crate::views::settings::controls::make_pressable(
                link,
                theme,
                false,
                move |_, _, cx| navigator.navigate("/settings", cx),
            ),
        )
    }

    /// `shrink-0 rounded-md border bg-button-neutral px-3 py-2 text-xs text-text-primary hover:border-brand-primary`
    fn render_reset_all_button(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = settings.translate("shortcuts.resetAll");
        let hover = theme.brand.primary;
        let reset_all = pressable("shortcut-reset-all", "button", label.clone(), label.clone())
            .flex_none()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.button_neutral.base)
            .px(spacing(3.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.primary)
            .hover(move |style| style.border_color(hover))
            .child(label);
        shortcut_button(
            reset_all,
            theme,
            !self.model.is_loaded,
            cx.listener(|this, _, _, cx| this.reset_all(cx)),
        )
    }

    fn render_header(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.translator("settings");
        let reset_all = self.render_reset_all_button(theme, &settings, cx);
        div()
            .mb(spacing(6.0))
            .flex()
            .items_start()
            .justify_between()
            .gap(spacing(4.0))
            .child(
                div()
                    .child(
                        div()
                            .text_scale(text_scale::LG)
                            .css_weight(600)
                            .text_color(theme.text.primary)
                            .child(settings.translate("shortcuts.title")),
                    )
                    .child(
                        div()
                            .mt(spacing(1.0))
                            .text_scale(text_scale::SM)
                            .text_color(theme.text.muted)
                            .child(settings.translate("shortcuts.pageDescription")),
                    )
                    .child(
                        div()
                            .mt(spacing(1.0))
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.muted)
                            .child(settings.translate("shortcuts.recordHint")),
                    ),
            )
            .child(reset_all)
    }

    /// `mb-4 rounded-md border border-status-warning/20 bg-status-warning/15 px-3 py-2 text-xs text-status-warning`
    fn render_notice(&self, theme: &ThemeTokens) -> Option<impl IntoElement> {
        let notice = self.model.notice?;
        let settings = self.translator("settings");
        let message = match notice {
            ShortcutNotice::Conflict(command_id) => {
                let definition = shortcut_command_definitions()
                    .iter()
                    .find(|definition| definition.id == command_id)?;
                settings.translate_with(
                    "shortcuts.conflict",
                    &[(
                        "command",
                        &describe_command(definition, self.screen.route.locale),
                    )],
                )
            }
            ShortcutNotice::Unsupported => settings.translate("shortcuts.unsupported"),
            ShortcutNotice::SaveFailed => settings.translate("shortcuts.saveFailed"),
            ShortcutNotice::LoadFailed => settings.translate("shortcuts.loadFailed"),
        };
        Some(
            div()
                .mb(spacing(4.0))
                .rounded(radius::MD)
                .border_1()
                .border_color(fade(theme.status.warning, 0.2))
                .bg(fade(theme.status.warning, 0.15))
                .px(spacing(3.0))
                .py(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.status.warning)
                .child(visible_text(message)),
        )
    }

    /// `mb-6 rounded-lg border bg-bg-surface`, 제목 `border-b px-4 py-3 text-xs uppercase tracking-wide text-text-muted`
    fn render_group(
        &self,
        theme: &ThemeTokens,
        group: kanvibe_domain::shortcuts::bindings::ShortcutCommandGroup,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.translator("settings");
        let group_key = match group {
            kanvibe_domain::shortcuts::bindings::ShortcutCommandGroup::TaskDetailDock => {
                "taskDetailDock"
            }
            kanvibe_domain::shortcuts::bindings::ShortcutCommandGroup::TaskDetail => "taskDetail",
            kanvibe_domain::shortcuts::bindings::ShortcutCommandGroup::Board => "board",
            kanvibe_domain::shortcuts::bindings::ShortcutCommandGroup::Terminal => "terminal",
        };
        let definitions: Vec<_> = shortcut_command_definitions()
            .iter()
            .filter(|definition| definition.group == group)
            .collect();
        let last_index = definitions.len().saturating_sub(1);
        let mut rows = Vec::new();
        for (index, definition) in definitions.into_iter().enumerate() {
            rows.push(self.render_row(theme, definition, index == last_index, cx));
        }
        div()
            .mb(spacing(6.0))
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .child(
                div()
                    .border_b_1()
                    .border_color(theme.border.default)
                    .px(spacing(4.0))
                    .py(spacing(3.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(settings.translate(&format!("shortcuts.groups.{group_key}"))),
            )
            .children(rows)
            .into_any_element()
    }

    /// `flex items-center justify-between gap-3 border-b px-4 py-3 last:border-b-0`
    fn render_row(
        &self,
        theme: &ThemeTokens,
        definition: &ShortcutCommandDefinition,
        is_last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = describe_command(definition, self.screen.route.locale);
        let display = format_shortcut_for_display(
            self.model.bindings.shortcut(definition.id),
            self.model.platform,
        );
        let row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(spacing(3.0))
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(
                visible_text(label)
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.primary),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing(2.0))
                    .child(render_shortcut_code(theme, display))
                    .child(self.render_record_button(theme, definition.id, cx))
                    .child(self.render_reset_button(theme, definition.id, cx)),
            );
        let row = if is_last {
            row
        } else {
            row.border_b_1().border_color(theme.border.default)
        };
        row.into_any_element()
    }

    /// 녹화 중 `border-brand-primary bg-brand-subtle text-brand-primary`, 아니면 `border-border-default bg-button-neutral text-text-primary`
    fn render_record_button(
        &self,
        theme: &ThemeTokens,
        command_id: ShortcutCommandId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let settings = self.translator("settings");
        let is_recording = self.model.recording == Some(command_id);
        let label = settings.translate(if is_recording {
            "shortcuts.recording"
        } else {
            "shortcuts.record"
        });
        let button = pressable(
            keyed_id("shortcut-record", &format!("{command_id:?}")),
            "button",
            label.clone(),
            label.clone(),
        )
        .rounded(radius::MD)
        .border_1()
        .px(spacing(3.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS);
        let hover = theme.brand.primary;
        let button = if is_recording {
            button
                .border_color(theme.brand.primary)
                .bg(theme.brand.subtle)
                .text_color(theme.brand.primary)
        } else {
            button
                .border_color(theme.border.default)
                .bg(theme.button_neutral.base)
                .text_color(theme.text.primary)
                .hover(move |style| style.border_color(hover))
        };
        shortcut_button(
            button.child(label),
            theme,
            !self.model.is_loaded,
            cx.listener(move |this, _, window, cx| this.toggle_recording(command_id, window, cx)),
        )
    }

    /// `rounded-md border px-3 py-1 text-xs text-text-muted hover:border-brand-primary hover:text-text-primary`
    fn render_reset_button(
        &self,
        theme: &ThemeTokens,
        command_id: ShortcutCommandId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self.translator("settings").translate("shortcuts.reset");
        let (hover_border, hover_text) = (theme.brand.primary, theme.text.primary);
        let button = pressable(
            keyed_id("shortcut-reset", &format!("{command_id:?}")),
            "button",
            label.clone(),
            label.clone(),
        )
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .px(spacing(3.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.muted)
        .hover(move |style| style.border_color(hover_border).text_color(hover_text))
        .child(label);
        let is_disabled = !self.model.is_customized(command_id) || !self.model.is_loaded;
        shortcut_button(
            button,
            theme,
            is_disabled,
            cx.listener(move |this, _, _, cx| this.reset_command(command_id, cx)),
        )
    }
}

/// `<code class="rounded-md border bg-bg-page px-2 py-1 text-xs text-text-secondary">`(code 요소라 고정폭 글꼴)
fn render_shortcut_code(theme: &ThemeTokens, display: String) -> impl IntoElement {
    div()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .mono(400)
        .text_color(theme.text.secondary)
        .child(display)
}

/// TS 단축키 화면 버튼은 비활성일 때 `opacity-40`이다
fn shortcut_button(
    element: gpui_kit::Stateful<gpui_kit::Div>,
    theme: &ThemeTokens,
    is_disabled: bool,
    on_click: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    if is_disabled {
        return element.opacity(DISABLED_OPACITY);
    }
    crate::views::settings::controls::make_pressable(element, theme, false, on_click)
}
