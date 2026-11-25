//! 터미널 탭 바(TS `TerminalTabBar`). tmux window·zellij tab·terminal 탭을 같은 모양으로 그리고,
//! 조작은 모두 `TabCommand`로 화면에 넘긴다. 앞 5개 탭에는 `Mod+Alt+{n}` 표기를 붙인다.

use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::ShortcutCommandId;
use kanvibe_domain::shortcuts::notation::format_shortcut_for_display;
use kanvibe_domain::terminal_tabs::TerminalTab;

use super::TaskDetailView;
use super::tabs::{TabCommand, TerminalTabsState};
use crate::components::{TailwindText, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::shortcuts::ShortcutState;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 단축키 표기를 붙이는 탭 수(`TERMINAL_TAB_SHORTCUT_INDEXES`)
const SHORTCUT_TAB_COUNT: usize = 5;
/// `min-w-44`
const TAB_MIN_WIDTH: f32 = 176.0;

/// 끌고 있는 탭
#[derive(Debug, Clone)]
pub struct DraggedTab {
    pub tab_id: String,
    pub name: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeTokens>();
        div()
            .rounded(radius::MD)
            .px(spacing(3.0))
            .py(spacing(1.0))
            .text_scale(text_scale::XS)
            .mono(400)
            .bg(theme.brand.primary)
            .text_color(theme.text.inverse)
            .child(self.name.clone())
    }
}

/// 구분선: 첫 탭이 아니고 자기와 앞 탭이 모두 비활성일 때만
pub fn has_tab_divider(tabs: &[TerminalTab], index: usize) -> bool {
    index > 0 && !tabs[index].is_active && !tabs[index - 1].is_active
}

/// 앞 5개 탭의 이동 단축키 표기
pub fn tab_shortcut_label(index: usize, state: &ShortcutState) -> Option<String> {
    if index >= SHORTCUT_TAB_COUNT {
        return None;
    }
    let command = ShortcutCommandId::terminal_tab(index as u8 + 1)?;
    Some(format_shortcut_for_display(
        state.bindings.shortcut(command),
        state.platform,
    ))
}

impl TaskDetailView {
    pub(super) fn render_tab_bar(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let tabs = &self.terminal_tabs;
        let mut bar = div()
            .id("terminal-tab-bar")
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .overflow_x_scroll();
        for (index, tab) in tabs.tabs.iter().enumerate() {
            bar = bar.child(self.render_tab(theme, tabs, tab, index, cx));
        }
        bar.child(render_new_tab_button(theme, tabs, &self.translator, cx))
            .children(render_tab_action_error(
                theme,
                tabs,
                &self.translator,
                &self.common,
                cx,
            ))
            .into_any_element()
    }

    fn render_tab(
        &self,
        theme: &ThemeTokens,
        tabs: &TerminalTabsState,
        tab: &TerminalTab,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_busy = tabs.pending.as_ref().and_then(TabCommand::tab_id) == Some(tab.id.as_str());
        let border = match has_tab_divider(&tabs.tabs, index) {
            true => fade(gpui_kit::rgb(0xffffff), 0.1),
            false => gpui_kit::transparent_black().into(),
        };
        let shortcut = tab_shortcut_label(index, cx.global::<ShortcutState>());
        let tab_id = tab.id.clone();
        let is_active = tab.is_active;
        let mut element = div()
            .id(SharedString::from(format!("terminal-tab-{}", tab.id)))
            .group("terminal-tab")
            .relative()
            .flex()
            .flex_1()
            .min_w(px(TAB_MIN_WIDTH))
            .items_center()
            .gap(spacing(2.0))
            .rounded(radius::MD)
            .px(spacing(3.0))
            .py(spacing(1.0))
            .border_l_1()
            .border_color(border)
            .text_scale(text_scale::XS)
            .mono(400)
            .cursor_pointer();
        element = match is_active {
            true => element
                .bg(theme.brand.primary)
                .text_color(theme.text.inverse),
            false => element
                .text_color(theme.terminal.text)
                .hover(|style| style.bg(fade(gpui_kit::rgb(0xffffff), 0.05))),
        };
        if is_busy {
            element = element.opacity(0.6);
        }
        element
            .child(self.render_tab_name(tab, cx))
            .children(shortcut.map(|label| {
                div()
                    .flex_none()
                    .text_size(px(10.0))
                    .line_height(px(15.0))
                    .opacity(0.6)
                    .child(label)
            }))
            .child(render_close_button(tab, &self.translator, cx))
            .child(qa_target_layer(QaTargetDescription {
                role: "tab",
                name: tab.name.clone().into(),
                text: tab.name.clone().into(),
            }))
            .on_click(
                cx.listener(move |view, event: &gpui_kit::ClickEvent, window, cx| {
                    if event.click_count() >= 2 {
                        view.begin_tab_rename(&tab_id, window, cx);
                    } else if !is_active {
                        view.run_tab_command(TabCommand::Select(tab_id.clone()), cx);
                    }
                }),
            )
            .on_drag(
                DraggedTab {
                    tab_id: tab.id.clone(),
                    name: tab.name.clone().into(),
                },
                |dragged, _, _, cx| cx.new(|_| dragged.clone()),
            )
            .on_drop(cx.listener(move |view, dragged: &DraggedTab, _, cx| {
                let is_same_tab = view
                    .terminal_tabs
                    .tabs
                    .get(index)
                    .is_some_and(|target| target.id == dragged.tab_id);
                if !is_same_tab {
                    view.run_tab_command(TabCommand::Move(dragged.tab_id.clone(), index), cx);
                }
            }))
            .into_any_element()
    }

    fn render_tab_name(&self, tab: &TerminalTab, cx: &mut Context<Self>) -> AnyElement {
        if let Some((renaming_id, input)) = self.tab_rename.as_ref().filter(|(id, _)| *id == tab.id)
        {
            let _ = renaming_id;
            let theme = *cx.global::<ThemeTokens>();
            return div()
                .min_w_0()
                .flex_1()
                .child(gpui_kit::base::input::Input::new(input))
                .text_color(theme.text.inverse)
                .into_any_element();
        }
        div()
            .min_w_0()
            .flex_1()
            .truncate()
            .text_center()
            .child(tab.name.clone())
            .into_any_element()
    }

    fn begin_tab_rename(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.terminal_tabs.tabs.iter().find(|tab| tab.id == tab_id) else {
            return;
        };
        let name = SharedString::from(tab.name.clone());
        let input =
            cx.new(|cx| gpui_kit::base::input::InputState::new(window, cx).default_value(name));
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        let renaming_tab = tab_id.to_string();
        self._tab_rename_events =
            Some(
                cx.subscribe_in(&input, window, move |view, input, event, window, cx| {
                    view.handle_rename_event(&renaming_tab, input, event, window, cx)
                }),
            );
        self.tab_rename = Some((tab_id.to_string(), input));
        cx.notify();
    }

    fn handle_rename_event(
        &mut self,
        tab_id: &str,
        input: &Entity<gpui_kit::base::input::InputState>,
        event: &gpui_kit::base::input::InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_kit::base::input::InputEvent;
        if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            return;
        }
        let draft = input.read(cx).value().to_string();
        self.tab_rename = None;
        if let Some(name) = super::tabs::renamed_tab_name(&self.terminal_tabs.tabs, tab_id, &draft)
        {
            self.run_tab_command(TabCommand::Rename(tab_id.to_string(), name), cx);
        }
        self.focus_terminal(window, cx);
        cx.notify();
    }

    /// Esc로 이름 바꾸기를 그만둔다(변경 요청을 보내지 않는다)
    pub(super) fn cancel_tab_rename(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.tab_rename.take().is_none() {
            return false;
        }
        self._tab_rename_events = None;
        self.focus_terminal(window, cx);
        cx.notify();
        true
    }
}

/// `absolute right-2 opacity-0 group-hover:opacity-100`. 누르면 선택으로 번지지 않는다
fn render_close_button(
    tab: &TerminalTab,
    translator: &Translator,
    cx: &mut Context<TaskDetailView>,
) -> impl IntoElement {
    let tab_id = tab.id.clone();
    let label = translator.translate_with("closeTerminalTab", &[("name", &tab.name)]);
    div()
        .id(SharedString::from(format!("terminal-tab-close-{}", tab.id)))
        .absolute()
        .right(spacing(2.0))
        .invisible()
        .group_hover("terminal-tab", |style| style.visible())
        .child("×")
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.into(),
            text: "×".into(),
        }))
        .on_click(cx.listener(move |view, _, window, cx| {
            cx.stop_propagation();
            view.close_terminal_tab(&tab_id, window, cx);
        }))
}

/// `shrink-0 rounded-md px-2 py-1 text-xs text-terminal-text hover:bg-white/10`. 명령 처리 중에는 막는다
fn render_new_tab_button(
    theme: &ThemeTokens,
    tabs: &TerminalTabsState,
    translator: &Translator,
    cx: &mut Context<TaskDetailView>,
) -> impl IntoElement {
    let label = translator.translate("newTerminalTab");
    let is_disabled = tabs.pending.is_some();
    // 추가를 기다리는 동안 `+` 대신 진행 표시를 보인다(TS `aria-busy`)
    let glyph = match tabs.pending {
        Some(TabCommand::Create) => "…",
        _ => "+",
    };
    let mut button = div()
        .id("terminal-tab-new")
        .relative()
        .flex_none()
        .rounded(radius::MD)
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .text_color(theme.terminal.text)
        .child(glyph)
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.into(),
            text: glyph.into(),
        }));
    if is_disabled {
        button = button.opacity(0.6);
    } else {
        button = button
            .cursor_pointer()
            .hover(|style| style.bg(fade(gpui_kit::rgb(0xffffff), 0.1)))
            .on_click(cx.listener(|view, _, _, cx| view.run_tab_command(TabCommand::Create, cx)));
    }
    button
}

fn render_tab_action_error(
    theme: &ThemeTokens,
    tabs: &TerminalTabsState,
    translator: &Translator,
    common: &Translator,
    cx: &mut Context<TaskDetailView>,
) -> Option<AnyElement> {
    let reason = tabs.action_error.as_ref()?;
    let retry_label = common.translate("retry");
    Some(
        div()
            .ml(spacing(2.0))
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing(1.5))
            .text_scale(text_scale::PX_11)
            .text_color(theme.status.error)
            .child(render_tab_action_message(translator.translate_with(
                "terminalTabActionFailed",
                &[("reason", reason)],
            )))
            .child(
                div()
                    .id("terminal-tab-retry")
                    .relative()
                    .rounded(radius::BASE)
                    .px(spacing(1.5))
                    .py(spacing(0.5))
                    .css_weight(500)
                    .text_color(theme.terminal.text)
                    .cursor_pointer()
                    .child(retry_label.clone())
                    .child(qa_target_layer(QaTargetDescription {
                        role: "button",
                        name: retry_label.clone().into(),
                        text: retry_label.into(),
                    }))
                    .on_click(cx.listener(|view, _, _, cx| view.retry_tab_command(cx))),
            )
            .into_any_element(),
    )
}

/// `max-w-64 truncate` 실패 사유(TS `role="alert"`)
fn render_tab_action_message(message: String) -> impl IntoElement {
    div()
        .relative()
        .max_w(px(256.0))
        .truncate()
        .child(message.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "alert",
            name: message.clone().into(),
            text: message.into(),
        }))
}

/// 앱 컨텍스트가 필요 없는 확인 질문 문구(TS `terminalTabCloseConfirm`)
pub fn tab_close_confirm_message(
    translator: &Translator,
    process_names: Option<&[String]>,
    close_window_shortcut: &str,
    platform: ShortcutPlatform,
) -> String {
    let processes = match process_names.filter(|names| !names.is_empty()) {
        Some(names) => names.join(", "),
        None => translator.translate("terminalTabProcessesUnknown"),
    };
    translator.translate_with(
        "terminalTabCloseConfirm",
        &[
            ("processes", &processes),
            (
                "shortcut",
                &format_shortcut_for_display(close_window_shortcut, platform),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_domain::i18n::Locale;
    use kanvibe_domain::shortcuts::bindings::ShortcutBindings;

    fn tab(id: &str, is_active: bool) -> TerminalTab {
        TerminalTab {
            id: id.into(),
            native_index: 0,
            name: id.into(),
            is_active,
        }
    }

    // parity: TS-renderer-TerminalTabBar-8322b3
    #[test]
    fn no_divider_next_to_the_active_tab() {
        let tabs = vec![tab("1", false), tab("2", true), tab("3", false)];
        assert!(!has_tab_divider(&tabs, 1));
        assert!(!has_tab_divider(&tabs, 2));
    }

    // parity: TS-renderer-TerminalTabBar-d023e5
    #[test]
    fn divider_between_two_inactive_tabs_after_the_first() {
        let tabs = vec![tab("1", true), tab("2", false), tab("3", false)];
        assert!(!has_tab_divider(&tabs, 0));
        assert!(has_tab_divider(&tabs, 2));
    }

    fn shortcut_state(platform: ShortcutPlatform) -> ShortcutState {
        ShortcutState {
            bindings: ShortcutBindings::defaults(),
            platform,
            capturing_windows: Default::default(),
        }
    }

    // parity: TS-renderer-TerminalTabBar-3349d1
    #[test]
    fn first_five_tabs_show_their_move_shortcut() {
        let state = shortcut_state(ShortcutPlatform::Linux);
        assert_eq!(tab_shortcut_label(0, &state).as_deref(), Some("Ctrl+Alt+1"));
        assert_eq!(tab_shortcut_label(4, &state).as_deref(), Some("Ctrl+Alt+5"));
    }

    // parity: TS-renderer-TerminalTabBar-cea316
    #[test]
    fn sixth_tab_and_later_have_no_shortcut_hint() {
        let state = shortcut_state(ShortcutPlatform::Linux);
        assert_eq!(tab_shortcut_label(5, &state), None);
    }

    #[test]
    fn close_confirmation_lists_processes_or_says_unknown() {
        let translator = Translator::new(Locale::Ko, "taskDetail");
        let names = vec!["vim".to_string(), "node".to_string()];
        let message = tab_close_confirm_message(
            &translator,
            Some(&names),
            "Mod+Shift+W",
            ShortcutPlatform::Linux,
        );
        assert!(message.contains("vim, node"), "{message}");
        let unknown =
            tab_close_confirm_message(&translator, None, "Mod+Shift+W", ShortcutPlatform::Linux);
        assert!(unknown.contains(&translator.translate("terminalTabProcessesUnknown")));
    }
}
