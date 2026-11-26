//! 계정 행(TS `AccountRow`)과 계정 추가 줄(TS `AddAccountForm`).

use gpui_kit::base::input::InputState;
use gpui_kit::{
    AnyElement, App, Context, Entity, Focusable, InteractiveElement, IntoElement, ParentElement,
    Styled, div, px,
};
use kanvibe_ai::accounts::account_service::AiAccountSummary;
use kanvibe_ai::usage::types::AiUsageProvider;
use kanvibe_domain::i18n::Translator;

use crate::components::TailwindText;
use crate::qa::targets::qa_text_input_layer;
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::ai_accounts::model::{mutation_outcome_key, provider_index};
use crate::views::ai_accounts::usage_block::render_account_usage;
use crate::views::ai_accounts::{AiAccountsView, outline_text_button};
use crate::views::settings::controls::{inline_load_error, keyed_id, make_pressable, pressable};
use crate::views::settings::text_field::themed_text_field;

impl AiAccountsView {
    /// `flex flex-wrap items-center gap-2 rounded-md border border-border-subtle bg-bg-page px-3 py-2`
    pub(super) fn render_account_row(
        &self,
        theme: &ThemeTokens,
        account: &AiAccountSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let usage = self.model.usage.usage_for(account).cloned();
        let now_ms = cx.global::<AppServices>().clock.now_ms();
        let usage_translator = Translator::new(self.screen.route.locale, "taskDetail.aiUsage");
        let summary = self.render_account_summary(theme, account, cx);
        let removal_failure = self.render_removal_failure(theme, account, cx);
        div()
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.page)
            .px(spacing(3.0))
            .py(spacing(2.0))
            .child(summary)
            .children(
                usage.map(|usage| render_account_usage(theme, &usage_translator, &usage, now_ms)),
            )
            .children(removal_failure)
            .into_any_element()
    }

    /// 계정 이름·로그인 상태·등급 배지와 로그인·제거 버튼 한 줄
    fn render_account_summary(
        &self,
        theme: &ThemeTokens,
        account: &AiAccountSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let accounts = self.translator("aiAccounts");
        let requires_login = self.model.requires_login(account);
        let status_key = if requires_login {
            "loggedOut"
        } else {
            "loggedIn"
        };
        let identity = div()
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .truncate()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.primary)
                    .child(account.label.clone()),
            )
            .child(
                div()
                    .mt(px(2.0))
                    .text_scale(text_scale::PX_11)
                    .text_color(theme.text.muted)
                    .child(accounts.translate(status_key)),
            );
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .child(identity)
            .children(
                self.plan_name(account)
                    .map(|plan| render_plan_badge(theme, plan)),
            )
            .child(self.render_login_button(theme, account, requires_login, cx))
            .children(
                account
                    .is_removable
                    .then(|| self.render_remove_button(theme, account, cx)),
            )
            .into_any_element()
    }

    /// 사용량 응답이 알려 준 등급이 있으면 그것을, 없으면 계정 목록의 등급
    fn plan_name(&self, account: &AiAccountSummary) -> Option<String> {
        self.model
            .usage
            .usage_for(account)
            .and_then(|usage| usage.plan_name.clone())
            .or_else(|| account.plan_name.clone())
    }

    fn render_removal_failure(
        &self,
        theme: &ThemeTokens,
        account: &AiAccountSummary,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let reason = self.model.removal_failure_for(account)?.to_string();
        let retry_target = account.clone();
        Some(
            inline_load_error(
                theme,
                keyed_id("ai-account-remove-retry", &account.account_root),
                self.translator("aiAccounts")
                    .translate_with("removeFailed", &[("reason", &reason)]),
                self.translator("common").translate("retry"),
                cx.listener(move |this, _, _, cx| this.remove_account(retry_target.clone(), cx)),
            )
            .into_any_element(),
        )
    }

    /// `shrink-0 rounded-md bg-brand-primary px-2 py-1 text-[11px] text-white hover:bg-brand-hover`
    fn render_login_button(
        &self,
        theme: &ThemeTokens,
        account: &AiAccountSummary,
        requires_login: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self.translator("aiAccounts").translate(if requires_login {
            "login"
        } else {
            "relogin"
        });
        let hover = theme.brand.hover;
        let button = pressable(
            keyed_id("ai-account-login", &account.account_root),
            "button",
            label.clone(),
            label.clone(),
        )
        .flex_none()
        .rounded(radius::MD)
        .bg(theme.brand.primary)
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::PX_11)
        .text_color(gpui_kit::white())
        .hover(move |style| style.bg(hover))
        .child(label);
        let is_open = self
            .model
            .active_login
            .as_ref()
            .is_some_and(|login| login.account_root == account.account_root);
        let (provider, account_root) = (account.provider, account.account_root.clone());
        make_pressable(
            button,
            theme,
            is_open,
            cx.listener(move |this, _, window, cx| {
                this.open_login(provider, account_root.clone(), window, cx)
            }),
        )
    }

    /// `shrink-0 rounded-md border px-2 py-1 text-[11px] text-text-secondary hover:text-text-primary`
    fn render_remove_button(
        &self,
        theme: &ThemeTokens,
        account: &AiAccountSummary,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_removing =
            self.model.removing_root.as_deref() == Some(account.account_root.as_str());
        let label = self.translator("aiAccounts").translate(if is_removing {
            "removing"
        } else {
            "remove"
        });
        let button = outline_text_button(
            theme,
            keyed_id("ai-account-remove", &account.account_root),
            label,
            text_scale::PX_11,
        );
        let account = account.clone();
        make_pressable(
            button,
            theme,
            self.model.removing_root.is_some(),
            cx.listener(move |this, _, window, cx| {
                this.confirm_remove(account.clone(), window, cx)
            }),
        )
    }

    /// `mt-2 flex items-center gap-2`: 이름 입력(`rounded-md border bg-bg-page px-2 py-1 text-xs`)과 "계정 추가"
    pub(super) fn render_add_form(
        &self,
        theme: &ThemeTokens,
        provider: AiUsageProvider,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(input) = self
            .name_inputs
            .as_ref()
            .map(|inputs| inputs[provider_index(provider)].clone())
        else {
            return div();
        };
        let has_name = !input.read(cx).value().trim().is_empty();
        let placeholder = self
            .translator("aiAccounts")
            .translate("accountNamePlaceholder");
        let field = themed_text_field(&input, theme, theme.text.primary, cx);
        div()
            .mt(spacing(2.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing(2.0))
                    .child(render_name_field(theme, field, &input, placeholder, cx))
                    .child(self.render_add_button(theme, provider, has_name, cx)),
            )
            .children(self.render_add_feedback(theme, provider, cx))
    }

    fn render_add_button(
        &self,
        theme: &ThemeTokens,
        provider: AiUsageProvider,
        has_name: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_pending = self.model.add_states[provider_index(provider)].is_pending;
        let label_key = if is_pending { "adding" } else { "addAccount" };
        let label = self.translator("aiAccounts").translate(label_key);
        let add = outline_text_button(
            theme,
            keyed_id("ai-account-add", provider.id()),
            label,
            text_scale::PX_11,
        );
        make_pressable(
            add,
            theme,
            !has_name || is_pending,
            cx.listener(move |this, _, window, cx| this.add_account(provider, window, cx)),
        )
    }

    /// 서비스가 거절한 사유(`errors.<사유>`)나 요청 실패와 재시도
    fn render_add_feedback(
        &self,
        theme: &ThemeTokens,
        provider: AiUsageProvider,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let accounts = self.translator("aiAccounts");
        let state = self.model.add_states[provider_index(provider)].clone();
        let rejected = state.rejected_outcome.map(|outcome| {
            div()
                .mt(spacing(1.0))
                .text_scale(text_scale::PX_11)
                .text_color(theme.status.error)
                .child(accounts.translate(&format!("errors.{}", mutation_outcome_key(outcome))))
                .into_any_element()
        });
        let failure = state.failure.map(|(_, reason)| {
            inline_load_error(
                theme,
                keyed_id("ai-account-add-retry", provider.id()),
                accounts.translate_with("addFailed", &[("reason", &reason)]),
                self.translator("common").translate("retry"),
                cx.listener(move |this, _, window, cx| this.retry_add(provider, window, cx)),
            )
            .mt(spacing(1.0))
            .into_any_element()
        });
        rejected.into_iter().chain(failure).collect()
    }
}

/// `shrink-0 rounded bg-bg-surface px-1.5 py-0.5 text-[10px] uppercase text-text-muted`
fn render_plan_badge(theme: &ThemeTokens, plan: String) -> impl IntoElement {
    div()
        .flex_none()
        .rounded(radius::SM)
        .bg(theme.background.surface)
        .px(spacing(1.5))
        .py(spacing(0.5))
        .text_scale(text_scale::PX_10)
        .text_color(theme.text.muted)
        .child(plan.to_uppercase())
}

/// 계정 이름 입력란. 재생 드라이버가 자리 표시 글자로 찾아 포커스를 주거나 값을 바꾼다
fn render_name_field(
    theme: &ThemeTokens,
    field: impl IntoElement,
    input: &Entity<InputState>,
    placeholder: String,
    cx: &App,
) -> impl IntoElement {
    let focus_handle = input.read(cx).focus_handle(cx);
    let fill_target = input.downgrade();
    div()
        .relative()
        .min_w_0()
        .flex_1()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .child(field)
        .child(qa_text_input_layer(
            placeholder,
            focus_handle,
            move |value, window, cx| {
                let _ = fill_target.update(cx, |input, cx| {
                    input.set_value(value.to_string(), window, cx)
                });
            },
        ))
}
