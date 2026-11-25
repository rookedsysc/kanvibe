//! 세션 타입 `<select>`(TS `SessionTypeOptions`를 담은 select). 기본 세션 타입과 일괄 변경이 같은 선택지를 쓴다.
//! 닫힌 모양은 `px-2 py-1 text-sm bg-bg-page border border-border-default rounded-md`에 Chromium 화살표를 더한 것이다.
//! Chromium select처럼 포커스된 채 위·아래 키로 값을 바로 바꾸고, Enter·Space·클릭으로 목록을 연다. Esc로 닫는다.

use gpui_kit::{
    Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::task::SessionType;

use crate::components::icon::{IconName, icon};
use crate::components::popover::{popover_option, popover_panel};
use crate::components::{DISABLED_OPACITY, TailwindText, focus_ring};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::controls::keyed_id;

/// Chromium이 가장 긴 선택지(`terminal`)와 화살표로 잡는 닫힌 select 폭
const SELECT_WIDTH: f32 = 94.0;
pub const SELECT_HEIGHT: f32 = 32.0;

/// 사용자가 다른 세션 타입을 골랐다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionTypeSelected(pub SessionType);

pub struct SessionTypeSelect {
    id: &'static str,
    selected: SessionType,
    is_open: bool,
    is_disabled: bool,
    focus_handle: FocusHandle,
}

impl EventEmitter<SessionTypeSelected> for SessionTypeSelect {}

impl SessionTypeSelect {
    pub fn new(id: &'static str, selected: SessionType, cx: &mut Context<Self>) -> Self {
        Self {
            id,
            selected,
            is_open: false,
            is_disabled: false,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn selected(&self) -> SessionType {
        self.selected
    }

    /// 바깥(저장 실패 되돌리기·새로 고침)에서 값을 바꾼다. 이벤트는 내지 않는다
    pub fn set_selected(&mut self, selected: SessionType, cx: &mut Context<Self>) {
        self.selected = selected;
        cx.notify();
    }

    pub fn set_disabled(&mut self, is_disabled: bool, cx: &mut Context<Self>) {
        self.is_disabled = is_disabled;
        if is_disabled {
            self.is_open = false;
        }
        cx.notify();
    }

    /// 목록을 열거나 닫는다(클릭·Enter·Space)
    pub fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.is_open = !self.is_open && !self.is_disabled;
        cx.notify();
    }

    fn choose(&mut self, session_type: SessionType, cx: &mut Context<Self>) {
        self.is_open = false;
        if session_type != self.selected {
            self.selected = session_type;
            cx.emit(SessionTypeSelected(session_type));
        }
        cx.notify();
    }

    fn step(&mut self, offset: isize, cx: &mut Context<Self>) {
        let index = SessionType::ALL
            .iter()
            .position(|session_type| *session_type == self.selected)
            .unwrap_or(0) as isize;
        let last = SessionType::ALL.len() as isize - 1;
        let next = (index + offset).clamp(0, last) as usize;
        self.choose(SessionType::ALL[next], cx);
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_disabled {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" => self.step(-1, cx),
            "down" => self.step(1, cx),
            "enter" | "space" => {
                self.is_open = !self.is_open;
                cx.notify();
            }
            "escape" if self.is_open => {
                self.is_open = false;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_options(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let options = SessionType::ALL.map(|session_type| {
            popover_option(
                keyed_id(self.id, session_type.as_str()),
                theme,
                session_type.as_str(),
                session_type == self.selected,
                None,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose(session_type, cx)))
        });
        popover_panel(
            theme,
            div()
                .id(keyed_id(self.id, "options"))
                .w(px(SELECT_WIDTH))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.is_open = false;
                    cx.notify();
                }))
                .children(options),
        )
    }
}

impl Render for SessionTypeSelect {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let label = self.selected.as_str();
        let focus_border = theme.brand.primary;
        let ring = focus_ring(&theme);
        let trigger = div()
            .id(self.id)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .w(px(SELECT_WIDTH))
            .h(px(SELECT_HEIGHT))
            .pl(spacing(3.0))
            .pr(spacing(1.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .text_scale(text_scale::SM)
            .text_color(theme.text.primary)
            .child(label)
            .child(icon(IconName::ChevronDownSmall, px(16.0)).text_color(theme.text.primary))
            .child(qa_target_layer(QaTargetDescription {
                role: "combobox",
                name: label.into(),
                text: label.into(),
            }));
        if self.is_disabled {
            return trigger.opacity(DISABLED_OPACITY).into_any_element();
        }
        trigger
            .track_focus(&self.focus_handle)
            .tab_index(0)
            .cursor_pointer()
            .focus(move |style| style.border_color(focus_border))
            .focus_visible(move |style| style.shadow(ring))
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_click(cx.listener(|this, _, window, cx| {
                this.focus_handle.focus(window, cx);
                this.is_open = !this.is_open;
                cx.notify();
            }))
            .children(self.is_open.then(|| self.render_options(&theme, cx)))
            .into_any_element()
    }
}
