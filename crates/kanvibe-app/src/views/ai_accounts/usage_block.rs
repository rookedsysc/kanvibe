//! 계정 행 안의 사용량 막대와 조회 상태 줄(TS `AiUsagePanel`의 `AccountUsageBlock showIdentity={false}`·`UsageStatusLine`),
//! provider 아이콘(`AiProviderIcon`). 시각은 en-US 표기를 UTC로 읽는다.

use gpui_kit::{Div, ParentElement, Rgba, Styled, div, px, relative};
use kanvibe_ai::javascript::{iso_string_from_ms, parse_date_ms};
use kanvibe_ai::usage::types::{
    AiUsageAccountResult, AiUsageFailureReason, AiUsageProvider, AiUsageWindow, AiUsageWindowKind,
};
use kanvibe_domain::i18n::Translator;

use crate::components::TailwindText;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::ai_accounts::model::UsageState;

/// 이 위부터는 강조색이 아니라 심각도 색으로 알린다
const HIGH_USAGE_PERCENT: f64 = 90.0;
const ELEVATED_USAGE_PERCENT: f64 = 75.0;

/// ISO 시각의 날짜·시각 조각(UTC)
struct DateParts {
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

fn date_parts(epoch_ms: f64) -> Option<DateParts> {
    let iso = iso_string_from_ms(epoch_ms)?;
    let number = |range: std::ops::Range<usize>| iso.get(range)?.parse::<u32>().ok();
    Some(DateParts {
        year: number(0..4)?,
        month: number(5..7)?,
        day: number(8..10)?,
        hour: number(11..13)?,
        minute: number(14..16)?,
    })
}

fn twelve_hour(parts: &DateParts) -> String {
    let meridiem = if parts.hour < 12 { "AM" } else { "PM" };
    let hour = match parts.hour % 12 {
        0 => 12,
        other => other,
    };
    format!("{hour}:{:02} {meridiem}", parts.minute)
}

/// 오늘이면 `5:00 AM`, 아니면 `1/20, 12:00 AM`(TS `formatResetTime`)
pub fn format_reset_time(resets_at: &str, now_ms: i64) -> Option<String> {
    let target = date_parts(parse_date_ms(resets_at)?)?;
    let today = date_parts(now_ms as f64)?;
    let is_today = (target.year, target.month, target.day) == (today.year, today.month, today.day);
    Some(if is_today {
        twelve_hour(&target)
    } else {
        format!("{}/{}, {}", target.month, target.day, twelve_hour(&target))
    })
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

/// provider 아이콘(16px). 브랜드 그림이 앱 자산에 없어 글자 모양으로 대신한다
pub fn render_provider_icon(theme: &ThemeTokens, provider: AiUsageProvider) -> Div {
    let icon = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.0));
    match provider {
        AiUsageProvider::Claude => icon
            .text_size(px(16.0))
            .line_height(px(16.0))
            .text_color(theme.agent.claude)
            .child("✳"),
        AiUsageProvider::Codex => icon.rounded(radius::SM).bg(gpui_kit::white()).child(
            div()
                .size(px(10.0))
                .rounded(radius::FULL)
                .bg(theme.agent.gemini),
        ),
        AiUsageProvider::Gemini => icon
            .text_size(px(16.0))
            .line_height(px(16.0))
            .text_color(theme.agent.gemini)
            .child("✦"),
    }
}
