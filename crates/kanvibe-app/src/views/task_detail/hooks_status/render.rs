//! hooks 카드 그리기(TS `HooksStatusCard` 마크업): 제목·현재 taskId·전체 배지, 읽는 중·설치 결과 안내, provider 네 줄.

use gpui_kit::{
    AnyElement, Context, Div, InteractiveElement, IntoElement, ParentElement, Render, Rgba,
    SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_ai::providers::AiSessionProvider;

use super::{
    HOOK_TOOLS, HookTool, HooksStatusCard, InstallMessage, MessageKind, OverallHookStatus,
    overall_status,
};
use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::task_detail::ai_provider_icon::ai_provider_icon;
use crate::views::task_detail::icons::{DetailIcon, detail_icon};
use crate::views::task_detail::usage_cards::visible_text;

fn session_provider(tool: HookTool) -> AiSessionProvider {
    match tool {
        HookTool::Claude => AiSessionProvider::Claude,
        HookTool::Gemini => AiSessionProvider::Gemini,
        HookTool::Codex => AiSessionProvider::Codex,
        HookTool::OpenCode => AiSessionProvider::Opencode,
    }
}

/// 설치됨은 `status-done`, 아니면 `status-error`
fn tone(theme: &ThemeTokens, is_installed: bool) -> Rgba {
    match is_installed {
        true => theme.status.done,
        false => theme.status.error,
    }
}

/// Chromium은 줄바꿈 자리로 `-` 뒤를 쓰지만 GPUI 줄 나눔(UAX#14)은 `-` 다음이 숫자면 끊지 않는다.
/// 폭 없는 공백을 끼워 TS와 같은 자리에서 끊기게 한다
fn break_after_hyphens(text: &str) -> String {
    text.replace('-', "-\u{200B}")
}

/// 성공은 `status`, 실패는 `alert`
fn message_role(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::Success => "status",
        MessageKind::Error => "alert",
    }
}

/// 글자 하나짜리 버튼. `name`이 비면 보이는 글자를 접근 이름으로 쓴다
fn text_button(id: impl Into<SharedString>, label: String, name: String) -> Stateful<Div> {
    let name = match name.is_empty() {
        true => label.clone(),
        false => name,
    };
    div()
        .id(id.into())
        .relative()
        .child(aligned_text(
            label.clone(),
            text_scale::XS,
            TextFamily::Sans,
        ))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: name.into(),
            text: label.into(),
        }))
}

/// `h-8 w-8 rounded-md border` 상태 색 상자 안의 provider 아이콘
fn tool_icon_box(tool: HookTool, color: Rgba) -> Div {
    div()
        .size(spacing(8.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::MD)
        .border_1()
        .border_color(fade(color, 0.2))
        .bg(fade(color, 0.1))
        .child(ai_provider_icon(session_provider(tool), 18.0))
}

impl HooksStatusCard {
    /// 전체 배지 `rounded-full border px-2 py-1 text-[11px] font-medium`
    fn render_overall_badge(&self, theme: &ThemeTokens) -> Div {
        let installed_count = self.installed_count();
        let (color, icon) = match overall_status(installed_count, HOOK_TOOLS.len()) {
            OverallHookStatus::Complete => (theme.status.done, DetailIcon::CheckmarkCircle),
            OverallHookStatus::Partial => (theme.status.warning, DetailIcon::Clock),
            OverallHookStatus::Missing => (theme.status.error, DetailIcon::AlertCircle),
        };
        let label = format!("{installed_count}/{}", HOOK_TOOLS.len());
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(spacing(1.5))
            .rounded(radius::FULL)
            .border_1()
            .border_color(fade(color, 0.2))
            .bg(fade(color, 0.15))
            .px(spacing(2.0))
            .py(spacing(1.0))
            .css_weight(500)
            .text_color(color)
            .child(detail_icon(icon, color, px(13.0)))
            .child(visible_text(label, text_scale::PX_11))
    }

    fn render_header(&self, theme: &ThemeTokens) -> Div {
        let title = self.translator.translate("hooksStatus");
        let task_line = self
            .translator
            .translate_with("hooksCurrentTaskId", &[("taskId", &self.task_id)]);
        div()
            .mb(spacing(3.0))
            .flex()
            .items_start()
            .justify_between()
            .gap(spacing(3.0))
            .child(
                div()
                    .min_w_0()
                    .child(
                        visible_text(title.to_uppercase(), text_scale::XS)
                            .css_weight(600)
                            .text_color(theme.text.muted),
                    )
                    .child(
                        div()
                            .mt(spacing(1.0))
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.muted)
                            .child(break_after_hyphens(&task_line)),
                    ),
            )
            .child(self.render_overall_badge(theme))
    }

    fn render_message(
        &self,
        theme: &ThemeTokens,
        message: &InstallMessage,
        cx: &mut Context<Self>,
    ) -> Div {
        let color = match message.kind {
            MessageKind::Success => theme.status.done,
            MessageKind::Error => theme.status.error,
        };
        let retry = message.retry_tool.map(|tool| {
            text_button(
                "hooks-install-retry",
                self.common.translate("retry"),
                String::new(),
            )
            .ml(spacing(2.0))
            .rounded(radius::BASE)
            .px(spacing(1.5))
            .py(spacing(0.5))
            .css_weight(500)
            .cursor_pointer()
            .on_click(cx.listener(move |card, _, _, cx| card.install(tool, cx)))
        });
        div()
            .relative()
            .mb(spacing(3.0))
            .flex()
            .flex_wrap()
            .items_center()
            .rounded(radius::MD)
            .border_1()
            .border_color(fade(color, 0.2))
            .bg(fade(color, 0.1))
            .px(spacing(3.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(color)
            .child(aligned_text(
                message.text.clone(),
                text_scale::XS,
                TextFamily::Sans,
            ))
            .children(retry)
            .child(qa_target_layer(QaTargetDescription {
                role: message_role(message.kind),
                name: message.text.clone().into(),
                text: message.text.clone().into(),
            }))
    }

    fn render_tool_row(&self, theme: &ThemeTokens, tool: HookTool, cx: &mut Context<Self>) -> Div {
        let status = self.statuses[tool.index()].as_ref();
        let is_installed = status.is_some_and(|status| status.installed);
        let has_registration_failed = tool == HookTool::OpenCode
            && status.is_some_and(|status| status.plugin_registration_error.is_some());
        let badge_key = match (has_registration_failed, is_installed) {
            (true, _) => "hooksVerificationFailed",
            (false, true) => "hooksInstalled",
            (false, false) => "hooksNotInstalled",
        };
        let color = tone(theme, is_installed);
        div()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(fade(theme.background.page, 0.6))
            .p(spacing(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing(3.0))
                    .child(tool_icon_box(tool, color))
                    .child(self.render_tool_title(theme, tool, badge_key, color))
                    .child(self.render_install_button(theme, tool, is_installed, cx)),
            )
    }

    /// 도구 이름과 설치 상태 배지(`rounded-full px-2 py-0.5 text-[10px] font-medium`)
    fn render_tool_title(
        &self,
        theme: &ThemeTokens,
        tool: HookTool,
        badge_key: &str,
        color: Rgba,
    ) -> Div {
        let badge = visible_text(self.translator.translate(badge_key), text_scale::PX_10)
            .rounded(radius::FULL)
            .px(spacing(2.0))
            .py(spacing(0.5))
            .css_weight(500)
            .bg(fade(color, 0.15))
            .text_color(color);
        div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .child(
                visible_text(tool.title(), text_scale::SM)
                    .truncate()
                    .css_weight(600)
                    .text_color(theme.text.primary),
            )
            .child(badge)
    }

    fn render_install_button(
        &self,
        theme: &ThemeTokens,
        tool: HookTool,
        is_installed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_installing = self.installing_tools.contains(&tool);
        let label = match (is_installing, is_installed) {
            (true, _) => self.translator.translate("installingHooks"),
            (false, true) => self.translator.translate("hooksStatusDialog.reinstall"),
            (false, false) => self.translator.translate("installHooks"),
        };
        let name = format!("{label} {}", tool.title());
        let mut button = text_button(format!("hooks-install-{}", tool.title()), label, name)
            .flex_none()
            .rounded(radius::MD)
            .px(spacing(3.0))
            .py(spacing(1.5))
            .text_scale(text_scale::XS)
            .css_weight(500);
        button = match is_installed {
            true => {
                let hover_border = theme.brand.primary;
                let hover_text = theme.text.primary;
                button
                    .border_1()
                    .border_color(theme.border.default)
                    .bg(theme.background.surface)
                    .text_color(theme.text.secondary)
                    .hover(move |style| style.border_color(hover_border).text_color(hover_text))
            }
            false => {
                let hover_background = theme.brand.hover;
                button
                    .bg(theme.brand.primary)
                    .text_color(theme.text.inverse)
                    .hover(move |style| style.bg(hover_background))
            }
        };
        match is_installing {
            true => button.opacity(0.6).into_any_element(),
            false => button
                .cursor_pointer()
                .on_click(cx.listener(move |card, _, _, cx| card.install(tool, cx)))
                .into_any_element(),
        }
    }
}

impl Render for HooksStatusCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let message = self
            .message
            .clone()
            .map(|message| self.render_message(&theme, &message, cx));
        let rows: Vec<Div> = HOOK_TOOLS
            .iter()
            .map(|tool| self.render_tool_row(&theme, *tool, cx))
            .collect();
        div()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.sm))
            .p(spacing(4.0))
            .child(self.render_header(&theme))
            .children(self.is_loading.then(|| {
                visible_text(self.common.translate("loading"), text_scale::XS)
                    .mb(spacing(3.0))
                    .text_color(theme.text.muted)
            }))
            .children(message)
            .child(div().flex().flex_col().gap(spacing(2.0)).children(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // parity: TS-components-HooksStatusCard-eae0b4
    #[test]
    fn every_hook_row_uses_its_provider_brand_icon() {
        let providers: Vec<_> = HOOK_TOOLS
            .iter()
            .map(|tool| session_provider(*tool))
            .collect();

        assert_eq!(
            providers,
            [
                AiSessionProvider::Claude,
                AiSessionProvider::Gemini,
                AiSessionProvider::Codex,
                AiSessionProvider::Opencode,
            ]
        );
        assert_eq!(overall_status(0, 4), OverallHookStatus::Missing);
        assert_eq!(overall_status(2, 4), OverallHookStatus::Partial);
        assert_eq!(overall_status(4, 4), OverallHookStatus::Complete);
    }

    #[test]
    fn task_ids_may_wrap_after_hyphens() {
        assert_eq!(break_after_hyphens("a-b"), "a-\u{200B}b");
    }
}
