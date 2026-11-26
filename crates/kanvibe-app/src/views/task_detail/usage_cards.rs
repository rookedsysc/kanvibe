//! AI 사용량 패널의 provider 카드(TS `AiUsagePanel`의 `ProviderUsageCard`·`AccountUsageBlock`·`UsageWindowRow`).
//! 카드는 provider가 처음 나타난 순서를 지키고, 계정마다 라벨·등급·기간별 막대·실패 사유를 보인다.
//! 시각 표기는 AI 계정 화면과 같은 `format_reset_time`(en-US, UTC)을 쓴다.

use std::rc::Rc;

use gpui_kit::{
    AnyElement, App, Div, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::usage::shared::SIGN_IN_FAILURE_REASONS;
use kanvibe_ai::usage::types::{
    AiUsageAccountResult, AiUsageFailureReason, AiUsageProvider, AiUsageWindow, AiUsageWindowKind,
};
use kanvibe_domain::i18n::Translator;

use super::ai_provider_icon::ai_provider_icon;
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{TextScale, ThemeTokens, radius, spacing, text_scale};
use crate::views::ai_accounts::usage_block::format_reset_time;

/// 한도가 가까워지는 구간. 이 위부터는 강조색이 아니라 심각도 색으로 알린다
const HIGH_USAGE_PERCENT: f64 = 90.0;
const ELEVATED_USAGE_PERCENT: f64 = 75.0;

/// 막대 색의 뜻
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageBarTone {
    Brand,
    Warning,
    Error,
}

pub fn usage_bar_tone(used_percent: f64) -> UsageBarTone {
    if used_percent >= HIGH_USAGE_PERCENT {
        UsageBarTone::Error
    } else if used_percent >= ELEVATED_USAGE_PERCENT {
        UsageBarTone::Warning
    } else {
        UsageBarTone::Brand
    }
}

/// provider 하나의 카드에 들어갈 계정들
pub struct ProviderAccountGroup<'a> {
    pub provider: AiUsageProvider,
    pub results: Vec<&'a AiUsageAccountResult>,
}

/// provider가 나타난 순서를 그대로 지켜 조회할 때마다 카드가 자리를 바꾸지 않게 한다
pub fn group_accounts_by_provider(
    accounts: &[AiUsageAccountResult],
) -> Vec<ProviderAccountGroup<'_>> {
    let mut groups: Vec<ProviderAccountGroup<'_>> = Vec::new();
    for account in accounts {
        match groups
            .iter_mut()
            .find(|group| group.provider == account.provider)
        {
            Some(group) => group.results.push(account),
            None => groups.push(ProviderAccountGroup {
                provider: account.provider,
                results: vec![account],
            }),
        }
    }
    groups
}

/// 같은 기간의 창 묶음. `total`은 기간 전체 한도이고 모델 몫만 주는 provider는 비어 있다
pub struct UsageWindowGroup<'a> {
    pub kind: AiUsageWindowKind,
    pub total: Option<&'a AiUsageWindow>,
    pub scoped: Vec<&'a AiUsageWindow>,
}

/// 모델별 한도는 이름만으로는 어느 기간에 딸린 것인지 드러나지 않아, 그 기간 한도 아래에 세워야 읽힌다
pub fn group_windows_by_period(windows: &[AiUsageWindow]) -> Vec<UsageWindowGroup<'_>> {
    let mut groups: Vec<UsageWindowGroup<'_>> = Vec::new();
    for window in windows {
        let index = match groups.iter().position(|group| group.kind == window.kind) {
            Some(index) => index,
            None => {
                groups.push(UsageWindowGroup {
                    kind: window.kind,
                    total: None,
                    scoped: Vec::new(),
                });
                groups.len() - 1
            }
        };
        match window.model_name {
            Some(_) => groups[index].scoped.push(window),
            None => groups[index].total = Some(window),
        }
    }
    groups
}

/// 라벨이 provider 이름뿐이면 카드 제목과 같은 말이라 자리만 차지한다
pub fn has_distinct_account_label(result: &AiUsageAccountResult) -> bool {
    result.label.to_lowercase() != provider_key(result.provider)
}

/// 조회에 실패했는데도 막대가 있으면 그 값은 직전 조회에서 이어 붙인 것이다. 옛 값임을 밝힐 시각
pub fn carried_usage_time(result: &AiUsageAccountResult, now_ms: i64) -> Option<String> {
    (result.reason.is_some() && !result.windows.is_empty())
        .then(|| format_reset_time(&result.fetched_at, now_ms))
        .flatten()
}

/// 로그인으로 풀리는 사유인지. 그때만 계정 화면으로 가는 길을 붙인다
pub fn needs_sign_in(reason: AiUsageFailureReason) -> bool {
    SIGN_IN_FAILURE_REASONS.contains(&reason)
}

fn provider_key(provider: AiUsageProvider) -> &'static str {
    match provider {
        AiUsageProvider::Claude => "claude",
        AiUsageProvider::Codex => "codex",
        AiUsageProvider::Gemini => "gemini",
    }
}

/// `capitalize`
pub fn provider_title(provider: AiUsageProvider) -> String {
    let key = provider_key(provider);
    key[..1].to_uppercase() + &key[1..]
}

fn window_kind_key(kind: AiUsageWindowKind) -> &'static str {
    match kind {
        AiUsageWindowKind::Session => "session",
        AiUsageWindowKind::Weekly => "weekly",
        AiUsageWindowKind::Monthly => "monthly",
        AiUsageWindowKind::Model => "model",
    }
}

pub fn reason_key(reason: AiUsageFailureReason) -> String {
    let value = serde_json::to_value(reason).ok();
    let key = value.as_ref().and_then(|value| value.as_str());
    format!("reasons.{}", key.unwrap_or("fetch-failed"))
}

/// `{usedPercent}%`(JS 숫자 표기)
pub fn percent_text(used_percent: f64) -> String {
    format!("{used_percent}%")
}

/// 보이는 글자 한 덩이. 재생·테스트가 글자로 찾을 수 있게 대상 층을 붙인다
pub fn visible_text(text: impl Into<String>, scale: TextScale) -> Div {
    let text = text.into();
    div()
        .relative()
        .text_scale(scale)
        .child(aligned_text(text.clone(), scale, TextFamily::Sans))
        .child(qa_target_layer(QaTargetDescription {
            role: "text",
            name: text.clone().into(),
            text: text.into(),
        }))
}

/// `계정 관리` 링크를 눌렀을 때 할 일
pub type ManageAccountsHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// 카드를 그리는 데 필요한 값
pub struct UsageCardContext<'a> {
    pub theme: &'a ThemeTokens,
    pub usage: &'a Translator,
    pub now_ms: i64,
    /// `계정 관리`를 누르면 AI 계정 화면으로 간다
    pub on_manage_accounts: ManageAccountsHandler,
}

fn bar_color(theme: &ThemeTokens, used_percent: f64) -> Rgba {
    match usage_bar_tone(used_percent) {
        UsageBarTone::Brand => theme.brand.primary,
        UsageBarTone::Warning => theme.status.warning,
        UsageBarTone::Error => theme.status.error,
    }
}

/// `UsageWindowRow`: 라벨·비율, `mt-1 h-1.5 rounded-full bg-bg-page` 막대, 초기화 시각
fn render_window_row(context: &UsageCardContext<'_>, window: &AiUsageWindow) -> Div {
    let theme = context.theme;
    let label = window
        .model_name
        .clone()
        .unwrap_or_else(|| context.usage.translate(window_kind_key(window.kind)));
    let reset_text = window
        .resets_at
        .as_deref()
        .and_then(|resets_at| format_reset_time(resets_at, context.now_ms));
    let fraction = (window.used_percent / 100.0).clamp(0.0, 1.0) as f32;
    div()
        .child(
            div()
                .flex()
                .justify_between()
                .child(visible_text(label, text_scale::XS).text_color(theme.text.secondary))
                .child(
                    visible_text(percent_text(window.used_percent), text_scale::XS)
                        .css_weight(500)
                        .text_color(theme.text.primary),
                ),
        )
        .child(
            div()
                .mt(spacing(1.0))
                .h(px(6.0))
                .overflow_hidden()
                .rounded(radius::FULL)
                .bg(theme.background.page)
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction))
                        .rounded(radius::FULL)
                        .bg(bar_color(theme, window.used_percent)),
                ),
        )
        .children(reset_text.map(|time| {
            visible_text(
                context.usage.translate_with("resetsAt", &[("time", &time)]),
                text_scale::PX_11,
            )
            .mt(spacing(1.0))
            .text_color(theme.text.muted)
        }))
}

/// 딸릴 기간 한도가 없는 모델 쿼터는 들여쓸 대상이 없어 그대로 나열한다
fn render_window_group(context: &UsageCardContext<'_>, group: &UsageWindowGroup<'_>) -> Div {
    let scoped_rows = group
        .scoped
        .iter()
        .map(|window| render_window_row(context, window));
    let column = div().flex().flex_col().gap(spacing(2.0));
    let Some(total) = group.total else {
        return column.children(scoped_rows);
    };
    let scoped: Vec<Div> = scoped_rows.collect();
    column
        .child(render_window_row(context, total))
        .children((!scoped.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(spacing(2.0))
                .border_l_1()
                .border_color(context.theme.border.subtle)
                .pl(spacing(2.0))
                .children(scoped)
        }))
}

/// 어느 계정의 사용량인지와 그 계정의 등급은 한 줄에 붙어야 계정마다 갈라 읽힌다
fn render_account_identity(theme: &ThemeTokens, result: &AiUsageAccountResult) -> Option<Div> {
    let has_label = has_distinct_account_label(result);
    if !has_label && result.plan_name.is_none() {
        return None;
    }
    Some(
        div()
            .mb(spacing(1.0))
            .flex()
            .items_end()
            .gap(spacing(2.0))
            .children(has_label.then(|| {
                visible_text(result.label.clone(), text_scale::PX_11)
                    .min_w_0()
                    .truncate()
                    .text_color(theme.text.muted)
            }))
            .children(result.plan_name.as_ref().map(|plan_name| {
                div().ml_auto().flex_none().child(
                    visible_text(plan_name.to_uppercase(), text_scale::PX_10)
                        .rounded(radius::BASE)
                        .bg(theme.background.page)
                        .px(spacing(1.5))
                        .py(spacing(0.5))
                        .text_color(theme.text.muted),
                )
            })),
    )
}

/// 직전 값을 이어 붙인 카드는 막대와 사유가 함께 보여야 옛 값이 새 값으로 읽히지 않는다
fn render_account_reason(
    context: &UsageCardContext<'_>,
    result: &AiUsageAccountResult,
    reason: AiUsageFailureReason,
) -> Div {
    let theme = context.theme;
    let carried = carried_usage_time(result, context.now_ms).map(|time| {
        context
            .usage
            .translate_with("carriedValues", &[("time", &time)])
    });
    let manage_label = context.usage.translate("manageAccounts");
    let line = div()
        .flex()
        .flex_wrap()
        .text_color(theme.text.muted)
        .children(carried.map(|text| visible_text(text, text_scale::XS).mr(spacing(1.0))))
        .child(visible_text(
            context.usage.translate(&reason_key(reason)),
            text_scale::XS,
        ))
        .children(needs_sign_in(reason).then(|| {
            manage_accounts_link(theme, manage_label, context.on_manage_accounts.clone())
        }));
    match result.windows.is_empty() {
        true => line,
        false => line.mt(spacing(2.0)),
    }
}

fn manage_accounts_link(
    theme: &ThemeTokens,
    label: String,
    on_manage: ManageAccountsHandler,
) -> AnyElement {
    div()
        .id("ai-usage-manage-accounts")
        .relative()
        .ml(spacing(1.0))
        .text_scale(text_scale::XS)
        .text_color(theme.brand.primary)
        .cursor_pointer()
        .child(aligned_text(
            label.clone(),
            text_scale::XS,
            TextFamily::Sans,
        ))
        .child(qa_target_layer(QaTargetDescription {
            role: "link",
            name: label.clone().into(),
            text: label.into(),
        }))
        .on_click(move |_, window, cx| on_manage(window, cx))
        .into_any_element()
}

/// `AccountUsageBlock`
fn render_account(context: &UsageCardContext<'_>, result: &AiUsageAccountResult) -> Div {
    let groups = group_windows_by_period(&result.windows);
    div()
        .children(render_account_identity(context.theme, result))
        .children((!groups.is_empty()).then(|| {
            div().flex().flex_col().gap(spacing(2.0)).children(
                groups
                    .iter()
                    .map(|group| render_window_group(context, group)),
            )
        }))
        .children(
            result
                .reason
                .map(|reason| render_account_reason(context, result, reason)),
        )
}

fn session_provider(provider: AiUsageProvider) -> AiSessionProvider {
    match provider {
        AiUsageProvider::Claude => AiSessionProvider::Claude,
        AiUsageProvider::Codex => AiSessionProvider::Codex,
        AiUsageProvider::Gemini => AiSessionProvider::Gemini,
    }
}

/// `ProviderUsageCard`: 터미널이 반투명이면 배경 없는 카드는 뒤의 글자와 겹쳐 읽히지 않아 `bg-bg-surface`를 깐다
pub fn render_provider_card(
    context: &UsageCardContext<'_>,
    group: &ProviderAccountGroup<'_>,
) -> Div {
    let theme = context.theme;
    div()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.subtle)
        .bg(theme.background.surface)
        .p(spacing(2.5))
        .child(
            div()
                .mb(spacing(2.0))
                .flex()
                .items_center()
                .gap(spacing(1.5))
                .child(ai_provider_icon(session_provider(group.provider), 15.0))
                .child(
                    visible_text(provider_title(group.provider), text_scale::XS)
                        .css_weight(600)
                        .text_color(theme.text.primary),
                ),
        )
        .child(
            div().flex().flex_col().gap(spacing(2.5)).children(
                group
                    .results
                    .iter()
                    .map(|result| render_account(context, result)),
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_ai::usage::types::AiUsageStatus;

    fn window(
        kind: AiUsageWindowKind,
        model_name: Option<&str>,
        used_percent: f64,
    ) -> AiUsageWindow {
        AiUsageWindow {
            kind,
            model_name: model_name.map(str::to_string),
            used_percent,
            resets_at: None,
        }
    }

    fn account(
        provider: AiUsageProvider,
        label: &str,
        plan_name: Option<&str>,
    ) -> AiUsageAccountResult {
        AiUsageAccountResult {
            provider,
            account_id: label.to_string(),
            label: label.to_string(),
            status: AiUsageStatus::Ok,
            plan_name: plan_name.map(str::to_string),
            windows: vec![window(AiUsageWindowKind::Session, None, 22.0)],
            reason: None,
            fetched_at: "2026-08-10T06:00:00.000Z".into(),
        }
    }

    #[test]
    fn windows_near_the_limit_use_severity_colors() {
        assert_eq!(usage_bar_tone(22.0), UsageBarTone::Brand);
        assert_eq!(usage_bar_tone(80.0), UsageBarTone::Warning);
        assert_eq!(usage_bar_tone(95.0), UsageBarTone::Error);
    }

    #[test]
    fn plans_stay_with_their_account() {
        let accounts = [
            account(AiUsageProvider::Claude, "me@example.com", Some("max")),
            account(AiUsageProvider::Claude, "work@example.com", Some("pro")),
        ];
        let groups = group_accounts_by_provider(&accounts);

        assert_eq!(groups.len(), 1);
        let plans: Vec<_> = groups[0]
            .results
            .iter()
            .map(|result| result.plan_name.as_deref())
            .collect();
        assert_eq!(plans, [Some("max"), Some("pro")]);
    }

    #[test]
    fn label_equal_to_provider_name_is_not_repeated() {
        assert!(!has_distinct_account_label(&account(
            AiUsageProvider::Gemini,
            "Gemini",
            None
        )));
        assert!(has_distinct_account_label(&account(
            AiUsageProvider::Claude,
            "me@example.com",
            None
        )));
    }

    #[test]
    fn model_weekly_limits_sit_under_the_weekly_total() {
        let windows = [
            window(AiUsageWindowKind::Session, None, 22.0),
            window(AiUsageWindowKind::Weekly, None, 2.0),
            window(AiUsageWindowKind::Weekly, Some("Fable"), 4.0),
        ];
        let groups = group_windows_by_period(&windows);

        assert_eq!(groups.len(), 2);
        let weekly = &groups[1];
        assert_eq!(weekly.total.map(|total| total.used_percent), Some(2.0));
        assert_eq!(weekly.scoped[0].model_name.as_deref(), Some("Fable"));
    }

    #[test]
    fn model_windows_without_a_period_total_are_not_indented() {
        let windows = [
            window(AiUsageWindowKind::Model, Some("Pro"), 12.0),
            window(AiUsageWindowKind::Model, Some("Flash"), 3.0),
        ];
        let groups = group_windows_by_period(&windows);

        assert!(groups[0].total.is_none());
        assert_eq!(groups[0].scoped.len(), 2);
    }

    #[test]
    fn provider_titles_are_capitalized_and_percents_use_js_numbers() {
        assert_eq!(provider_title(AiUsageProvider::Codex), "Codex");
        assert_eq!(percent_text(42.0), "42%");
        assert_eq!(percent_text(42.5), "42.5%");
    }
}
