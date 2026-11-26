//! 왼쪽 dock(TS `TaskDetailContent`의 `<aside>`). 보드 복귀 버튼, dock 항목, 아래쪽 사용량 버튼으로 이뤄진다.
//! 툴팁의 단축키 표기는 dock 항목 순서에서 나온 `Mod+{n}`을 지금 단축키 설정으로 표시한 값이다.

use gpui_kit::{
    AnyElement, App, ClickEvent, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::{ShortcutBindings, ShortcutCommandId};
use kanvibe_domain::shortcuts::notation::format_shortcut_for_display;
use kanvibe_domain::task_detail_dock::{TaskDetailDockItemId, resolve_task_detail_dock_label};

use super::icons::{DetailIcon, detail_icon};
use super::layout_state::DockEntry;
use crate::components::spinner::spinner_ring;
use crate::components::tooltip::text_tooltip;
use crate::components::{box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing};

/// `w-12`
pub const DOCK_WIDTH: f32 = 48.0;
const ICON_SIZE: f32 = 17.0;

/// dock이 그리는 데 필요한 값
pub struct DockModel<'a> {
    pub entries: &'a [DockEntry],
    pub is_usage_active: bool,
    pub bindings: &'a ShortcutBindings,
    pub platform: ShortcutPlatform,
    pub translator: &'a Translator,
    /// macOS 신호등 버튼 자리(`pt-10`)
    pub needs_mac_header_offset: bool,
    /// 기다리는 자리의 고리를 돌릴지(QA 재생은 멈춘다)
    pub is_spinner_animated: bool,
}

/// 눌렀을 때 화면이 할 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockAction {
    BackToBoard,
    Activate(u8),
    ToggleUsage,
}

pub fn dock_icon_kind(id: TaskDetailDockItemId) -> DetailIcon {
    match id {
        TaskDetailDockItemId::Overview => DetailIcon::InformationCircle,
        TaskDetailDockItemId::Status => DetailIcon::AntennaSignal,
        TaskDetailDockItemId::Chat => DetailIcon::Chatting,
        TaskDetailDockItemId::PullRequest => DetailIcon::PullRequest,
        TaskDetailDockItemId::LiveSessions => DetailIcon::Activity,
        TaskDetailDockItemId::Vscode => DetailIcon::SourceCode,
    }
}

/// 툴팁 문구 `라벨 (단축키)`
pub fn dock_entry_title(entry: &DockEntry, model: &DockModel<'_>) -> String {
    let label = resolve_task_detail_dock_label(|key| model.translator.translate(key), entry.id);
    match ShortcutCommandId::task_detail_dock(entry.shortcut_position) {
        Some(command) => {
            let shortcut =
                format_shortcut_for_display(model.bindings.shortcut(command), model.platform);
            format!("{label} ({shortcut})")
        }
        None => label,
    }
}

pub fn render_dock(
    theme: &ThemeTokens,
    model: &DockModel<'_>,
    on_action: impl Fn(DockAction, &mut Window, &mut App) + Clone + 'static,
) -> gpui_kit::Div {
    let usage_title = format!(
        "{} ({})",
        model.translator.translate("aiUsage.title"),
        format_shortcut_for_display(
            model.bindings.shortcut(ShortcutCommandId::TaskDetailUsage),
            model.platform
        )
    );
    let mut aside = div()
        .absolute()
        .top(spacing(3.0))
        .bottom(spacing(3.0))
        .left(spacing(3.0))
        .w(px(DOCK_WIDTH))
        .flex()
        .flex_col()
        .items_center()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .bg(fade(theme.background.surface, 0.95))
        .shadow(box_shadows(&theme.shadow.sm))
        .p(spacing(1.5));
    if model.needs_mac_header_offset {
        aside = aside.pt(spacing(10.0));
    }
    let back_label = model.translator.translate("backToBoard");
    aside
        .child(
            dock_button(
                theme,
                "dock-back",
                DockButtonLook::Plain,
                &back_label,
                back_label.clone(),
            )
            .mb(spacing(2.0))
            .on_click(click(on_action.clone(), DockAction::BackToBoard))
            .child(detail_icon(
                DetailIcon::BackChevron,
                theme.text.muted,
                px(ICON_SIZE),
            )),
        )
        .children(
            model
                .entries
                .iter()
                .map(|entry| render_dock_entry(theme, entry, model, on_action.clone())),
        )
        .child(div().mt_auto())
        .child(render_usage_button(theme, model, usage_title, on_action))
}

fn click(
    on_action: impl Fn(DockAction, &mut Window, &mut App) + 'static,
    action: DockAction,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| on_action(action, window, cx)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockButtonLook {
    Plain,
    Active,
    PullRequestLink,
}

/// `rounded-md p-2`, 활성이면 `bg-brand-subtle`. PR 링크는 `h-8 w-8 border border-tag-pr-text/30 bg-tag-pr-bg`
fn dock_button(
    theme: &ThemeTokens,
    id: &'static str,
    look: DockButtonLook,
    label: &str,
    title: String,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let hover_background = theme.background.page;
    let base = div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::MD)
        .cursor_pointer()
        .tooltip(text_tooltip(title))
        .child(qa_target_layer(QaTargetDescription {
            // PR 링크는 TS에서 `<a href>`였다
            role: match look {
                DockButtonLook::PullRequestLink => "link",
                _ => "button",
            },
            name: label.to_string().into(),
            text: String::new().into(),
        }));
    match look {
        DockButtonLook::Plain => base
            .p(spacing(2.0))
            .hover(move |style| style.bg(hover_background)),
        DockButtonLook::Active => base.p(spacing(2.0)).bg(theme.brand.subtle),
        DockButtonLook::PullRequestLink => base
            .size(spacing(8.0))
            .border_1()
            .border_color(fade(theme.tag.pull_request.text, 0.3))
            .bg(theme.tag.pull_request.background),
    }
}

/// 외부 명령을 기다리는 자리는 아이콘 대신 도는 고리(`h-[17px] w-[17px] … motion-safe:animate-spin`)를 보인다
fn render_dock_entry(
    theme: &ThemeTokens,
    entry: &DockEntry,
    model: &DockModel<'_>,
    on_action: impl Fn(DockAction, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let label = resolve_task_detail_dock_label(|key| model.translator.translate(key), entry.id);
    let look = match (entry.href.is_some(), entry.is_active) {
        (true, _) => DockButtonLook::PullRequestLink,
        (false, true) => DockButtonLook::Active,
        (false, false) => DockButtonLook::Plain,
    };
    let color: Rgba = match look {
        DockButtonLook::PullRequestLink => theme.tag.pull_request.text,
        DockButtonLook::Active => theme.text.brand,
        DockButtonLook::Plain => theme.text.muted,
    };
    let content: AnyElement = match entry.is_busy {
        true => div()
            .size(px(ICON_SIZE))
            .child(spinner_ring(color, model.is_spinner_animated))
            .into_any_element(),
        false => detail_icon(dock_icon_kind(entry.id), color, px(ICON_SIZE)).into_any_element(),
    };
    let title = dock_entry_title(entry, model);
    dock_button(theme, dock_element_id(entry.id), look, &label, title)
        .mb(spacing(1.0))
        .on_click(click(
            on_action,
            DockAction::Activate(entry.shortcut_position),
        ))
        .child(content)
        .into_any_element()
}

fn dock_element_id(id: TaskDetailDockItemId) -> &'static str {
    match id {
        TaskDetailDockItemId::Overview => "dock-overview",
        TaskDetailDockItemId::Status => "dock-status",
        TaskDetailDockItemId::Chat => "dock-chat",
        TaskDetailDockItemId::PullRequest => "dock-pull-request",
        TaskDetailDockItemId::LiveSessions => "dock-live-sessions",
        TaskDetailDockItemId::Vscode => "dock-vscode",
    }
}

fn render_usage_button(
    theme: &ThemeTokens,
    model: &DockModel<'_>,
    title: String,
    on_action: impl Fn(DockAction, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = model.translator.translate("aiUsage.title");
    let (look, color) = match model.is_usage_active {
        true => (DockButtonLook::Active, theme.text.brand),
        false => (DockButtonLook::Plain, theme.text.muted),
    };
    dock_button(theme, "dock-usage", look, &label, title)
        .on_click(click(on_action, DockAction::ToggleUsage))
        .child(detail_icon(
            DetailIcon::ChartHistogram,
            color,
            px(ICON_SIZE),
        ))
}

/// 상태 패널 제목(`상태 변경 · Hooks 상태`). dock 라벨과 같은 문구다
pub fn status_panel_label(translator: &Translator) -> String {
    resolve_task_detail_dock_label(
        |key| translator.translate(key),
        TaskDetailDockItemId::Status,
    )
}
