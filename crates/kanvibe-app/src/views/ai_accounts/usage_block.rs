//! 계정 행 안의 사용량 막대와 조회 상태 줄(TS `AiUsagePanel`의 `AccountUsageBlock showIdentity={false}`·`UsageStatusLine`),
//! provider 아이콘(`AiProviderIcon`). 시각은 en-US 표기를 현지 시간대로 읽는다.

use gpui_kit::{Div, Img, ParentElement, Rgba, Styled, div, img, px, relative};
use kanvibe_ai::javascript::parse_date_ms;
use kanvibe_ai::usage::types::{
    AiUsageAccountResult, AiUsageFailureReason, AiUsageProvider, AiUsageWindow, AiUsageWindowKind,
};
use kanvibe_domain::i18n::Translator;

use crate::assets::provider_icon_paths;
use crate::components::TailwindText;
use crate::locale_format::{format_month_day_time_hm, format_time_hm};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::ai_accounts::model::UsageState;
use crate::views::settings::local_time::LocalDateTime;

/// 이 위부터는 강조색이 아니라 심각도 색으로 알린다
const HIGH_USAGE_PERCENT: f64 = 90.0;
const ELEVATED_USAGE_PERCENT: f64 = 75.0;

/// 오늘이면 `5:00 AM`, 아니면 `1/20, 12:00 AM`(TS `formatResetTime`, en-US). 시스템 locale·현지 시간대로 적는다
pub fn format_reset_time(resets_at: &str, now_ms: i64) -> Option<String> {
    let target = LocalDateTime::from_epoch_ms(parse_date_ms(resets_at)?)?;
    let today = LocalDateTime::from_epoch_ms(now_ms as f64)?;
    let is_today = (target.year, target.month, target.day) == (today.year, today.month, today.day);
    if is_today {
        format_time_hm(target)
    } else {
        format_month_day_time_hm(target)
    }
}

fn usage_bar_color(theme: &ThemeTokens, used_percent: f64) -> Rgba {
    if used_percent >= HIGH_USAGE_PERCENT {
        theme.status.error
    } else if used_percent >= ELEVATED_USAGE_PERCENT {
        theme.status.warning
    } else {
        theme.brand.primary
    }
}

fn window_kind_key(kind: AiUsageWindowKind) -> &'static str {
    match kind {
        AiUsageWindowKind::Session => "session",
        AiUsageWindowKind::Weekly => "weekly",
        AiUsageWindowKind::Monthly => "monthly",
        AiUsageWindowKind::Model => "model",
    }
}

fn reason_key(reason: AiUsageFailureReason) -> String {
    let value = serde_json::to_value(reason).ok();
    value
        .as_ref()
        .and_then(|value| value.as_str())
        .unwrap_or("fetch-failed")
        .to_string()
}

/// 막대 하나: `flex items-baseline justify-between text-xs` 라벨·퍼센트, `mt-1 h-1.5 rounded-full bg-bg-page` 막대,
/// 초기화 시각 `mt-1 text-[11px] text-text-muted`
fn render_window_row(
    theme: &ThemeTokens,
    usage: &Translator,
    window: &AiUsageWindow,
    now_ms: i64,
) -> Div {
    let label = window
        .model_name
        .clone()
        .unwrap_or_else(|| usage.translate(window_kind_key(window.kind)));
    let reset_text = window
        .resets_at
        .as_deref()
        .and_then(|resets_at| format_reset_time(resets_at, now_ms));
    div()
        .child(
            div()
                .flex()
                .justify_between()
                .text_scale(text_scale::XS)
                .child(div().text_color(theme.text.secondary).child(label))
                .child(
                    div()
                        .css_weight(500)
                        .text_color(theme.text.primary)
                        .child(format!("{}%", window.used_percent)),
                ),
        )
        .child(render_usage_bar(theme, window.used_percent))
        .children(reset_text.map(|time| {
            div()
                .mt(spacing(1.0))
                .text_scale(text_scale::PX_11)
                .text_color(theme.text.muted)
                .child(usage.translate_with("resetsAt", &[("time", &time)]))
        }))
}

/// `mt-1 h-1.5 overflow-hidden rounded-full bg-bg-page` 막대와 사용 비율만큼 채운 안쪽 막대
fn render_usage_bar(theme: &ThemeTokens, used_percent: f64) -> Div {
    let fraction = (used_percent / 100.0).clamp(0.0, 1.0) as f32;
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
                .bg(usage_bar_color(theme, used_percent)),
        )
}

/// 계정 하나의 사용량. 같은 기간의 모델별 창은 그 기간 한도 아래 들여 세운다
pub fn render_account_usage(
    theme: &ThemeTokens,
    usage: &Translator,
    result: &AiUsageAccountResult,
    now_ms: i64,
) -> Div {
    let mut rows = Vec::new();
    for window in &result.windows {
        let row = render_window_row(theme, usage, window, now_ms);
        rows.push(if window.model_name.is_some() {
            row.border_l_1()
                .border_color(theme.border.subtle)
                .pl(spacing(2.0))
        } else {
            row
        });
    }
    let has_windows = !rows.is_empty();
    div()
        .children(has_windows.then(|| div().flex().flex_col().gap(spacing(2.0)).children(rows)))
        .children(result.reason.map(|reason| {
            let carried = (has_windows)
                .then(|| format_reset_time(&result.fetched_at, now_ms))
                .flatten()
                .map(|time| usage.translate_with("carriedValues", &[("time", &time)]) + " ");
            let line = div()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(format!(
                    "{}{}",
                    carried.unwrap_or_default(),
                    usage.translate(&format!("reasons.{}", reason_key(reason)))
                ));
            // 막대 아래에 붙는 사유는 `mt-2`
            if has_windows {
                line.mt(spacing(2.0))
            } else {
                line
            }
        }))
}

/// 조회 상태 한 줄(`text-[11px] text-text-muted`): 불러오는 중 / 갱신하는 중 / `{시각} 기준`
pub fn usage_status_text(usage: &Translator, state: &UsageState, now_ms: i64) -> Option<String> {
    if state.is_loading() {
        return Some(usage.translate("loading"));
    }
    if state.is_refreshing {
        return Some(usage.translate("refreshing"));
    }
    let snapshot = state.snapshot.as_ref()?;
    let time = format_reset_time(&snapshot.fetched_at, now_ms)?;
    Some(usage.translate_with("updatedAt", &[("time", &time)]))
}

/// provider 색 아이콘(16px, TS `AiProviderIcon size={16}`의 `block object-contain` 그림)
pub fn render_provider_icon(provider: AiUsageProvider) -> Img {
    let path = match provider {
        AiUsageProvider::Claude => provider_icon_paths::CLAUDE,
        AiUsageProvider::Codex => provider_icon_paths::CODEX,
        AiUsageProvider::Gemini => provider_icon_paths::GEMINI,
    };
    img(path).flex_none().size(px(16.0))
}
