//! AI 사용량 패널(TS `AiUsagePanel` + `useAiUsage`). dock 사용량 버튼이나 `Mod+0`으로 열 때 만들고 닫으면 버린다.
//! 열면 저장된 결과를 먼저 한 번 읽어 보여 주고, 창이 앞에 있는 동안 결과가 만들어진 시각에서 5분마다 다시 조회한다.
//! 조회 중 들어온 새로고침은 그 조회가 끝난 뒤 최근 결과를 나눠 쓰지 않는 조회로 한 번 더 읽는다.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div,
};
use kanvibe_ai::javascript::parse_date_ms;
use kanvibe_ai::usage::shared::AI_USAGE_REFRESH_INTERVAL_MS;
use kanvibe_ai::usage::types::AiUsageSnapshot;
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_runtime::DesktopRuntime;

use super::data::desktop_runtime;
use super::usage_cards::{
    UsageCardContext, group_accounts_by_provider, render_provider_card, visible_text,
};
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::{AppServices, ServiceResult};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::WindowNavigator;
use crate::views::ai_accounts::model::UsageState;
use crate::views::ai_accounts::usage_block::usage_status_text;
use crate::views::settings::service_call::{call_service, failure};

/// 데이터 디렉터리 없이 뜬 실행에서 사용량 서비스를 쓸 수 없는 사유
const USAGE_SERVICE_UNAVAILABLE: &str =
    "데이터 디렉터리 없이 실행되어 사용량 서비스를 쓸 수 없습니다.";

/// 패널이 낼 사용량 조회 하나
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageFetch {
    pub can_reuse_recent_snapshot: bool,
}

/// `useAiUsage`의 조회 상태. 시각은 밖에서 받아 타이머 없이도 판정할 수 있다
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsagePolling {
    pub usage: UsageState,
    is_refresh_queued: bool,
    has_requested_cache: bool,
    /// 다음 자동 조회 시각. 처음은 0이라 열자마자 조회한다
    next_auto_load_at_ms: i64,
}

impl UsagePolling {
    /// 저장된 결과는 패널을 연 동안 한 번만 읽는다
    pub fn take_cache_request(&mut self) -> bool {
        !std::mem::replace(&mut self.has_requested_cache, true)
    }

    /// 다음 자동 조회까지 남은 시간. 이미 지났으면 0이다
    pub fn auto_load_delay(&self, now_ms: i64) -> Duration {
        Duration::from_millis((self.next_auto_load_at_ms - now_ms).max(0) as u64)
    }

    /// 조회를 시작한다. 이미 조회 중이면 아무것도 내지 않는다
    pub fn begin_fetch(&mut self, can_reuse_recent_snapshot: bool) -> Option<UsageFetch> {
        if self.usage.is_refreshing {
            return None;
        }
        self.usage.is_refreshing = true;
        Some(UsageFetch {
            can_reuse_recent_snapshot,
        })
    }

    /// 새로고침 버튼. 조회 중이면 끝난 뒤 한 번 더 읽도록 미룬다
    pub fn request_refresh(&mut self) -> Option<UsageFetch> {
        if self.usage.is_refreshing {
            self.is_refresh_queued = true;
            return None;
        }
        self.begin_fetch(false)
    }

    /// 조회 결과를 반영한다. 미룬 새로고침이 있으면 사람이 방금 바꾼 상태를 보려는 것이라 새로 조회한다
    pub fn finish_fetch(
        &mut self,
        result: Result<AiUsageSnapshot, String>,
        now_ms: i64,
    ) -> Option<UsageFetch> {
        self.next_auto_load_at_ms = match &result {
            Ok(snapshot) => next_auto_load_at_ms(snapshot, now_ms),
            Err(_) => now_ms + AI_USAGE_REFRESH_INTERVAL_MS as i64,
        };
        self.usage.finish_refresh(result);
        if !std::mem::replace(&mut self.is_refresh_queued, false) {
            return None;
        }
        self.begin_fetch(false)
    }
}

/// 결과가 만들어진 시각에서 세어야 같은 결과를 나눠 쓰는 창들이 같은 순간에 다시 조회한다.
/// 시계가 틀어져 그 시각이 이미 지나 있으면 받자마자 다시 조회하는 일이 끝없이 되풀이되므로, 그때는 지금부터 센다
fn next_auto_load_at_ms(snapshot: &AiUsageSnapshot, now_ms: i64) -> i64 {
    let interval_ms = AI_USAGE_REFRESH_INTERVAL_MS as i64;
    match parse_date_ms(&snapshot.fetched_at) {
        Some(fetched_at) if fetched_at as i64 + interval_ms > now_ms => {
            fetched_at as i64 + interval_ms
        }
        _ => now_ms + interval_ms,
    }
}

pub struct AiUsagePanel {
    polling: UsagePolling,
    usage: Translator,
    navigator: WindowNavigator,
    locale: Locale,
    is_window_active: bool,
    _cache: Option<Task<()>>,
    _fetch: Option<Task<()>>,
    _auto_load: Option<Task<()>>,
    _activation: Subscription,
}

fn now_ms(cx: &gpui_kit::App) -> i64 {
    cx.global::<AppServices>().clock.now_ms()
}

impl AiUsagePanel {
    pub fn new(
        locale: Locale,
        navigator: WindowNavigator,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // 볼 사람이 없는 동안 밀린 조회는 예약 시각이 이미 지나 있으므로 창이 돌아오는 순간 바로 나간다
        let activation = cx.observe_window_activation(window, |panel, window, cx| {
            panel.is_window_active = window.is_window_active();
            panel.schedule_auto_load(cx)
        });
        let mut panel = Self {
            polling: UsagePolling::default(),
            usage: Translator::new(locale, "taskDetail.aiUsage"),
            navigator,
            locale,
            _cache: None,
            _fetch: None,
            _auto_load: None,
            is_window_active: window.is_window_active(),
            _activation: activation,
        };
        panel.show_cached_snapshot(cx);
        panel.schedule_auto_load(cx);
        panel
    }

    /// 캐시를 읽는 사이에 새 조회가 끝났을 수 있으므로 비어 있을 때만 채운다. 읽지 못하면 새 조회만 기다린다
    fn show_cached_snapshot(&mut self, cx: &mut Context<Self>) {
        if !self.polling.take_cache_request() {
            return;
        }
        let runtime = desktop_runtime(cx);
        let read = call_service(
            cx,
            "aiUsage.getCachedAiUsageSnapshot",
            "[]".into(),
            move || read_cached_snapshot(runtime),
        );
        self._cache = Some(cx.spawn(async move |this, cx| {
            let cached = read.await.ok().flatten();
            let _ = this.update(cx, |panel, cx| {
                panel.polling.usage.show_cached(cached);
                cx.notify();
            });
        }));
    }

    /// 화면이 열려 있고 창이 앞에 있는 동안만 다음 조회를 예약한다. 조회가 끝날 때마다 새로 건다
    fn schedule_auto_load(&mut self, cx: &mut Context<Self>) {
        if !self.is_window_active {
            self._auto_load = None;
            return;
        }
        let delay = self.polling.auto_load_delay(now_ms(cx));
        self._auto_load = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |panel, cx| {
                if let Some(fetch) = panel.polling.begin_fetch(true) {
                    panel.start_fetch(fetch, cx);
                }
            });
        }));
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(fetch) = self.polling.request_refresh() {
            self.start_fetch(fetch, cx);
        }
    }

    fn start_fetch(&mut self, fetch: UsageFetch, cx: &mut Context<Self>) {
        let runtime = desktop_runtime(cx);
        let can_reuse = fetch.can_reuse_recent_snapshot;
        let arguments = serde_json::json!([{ "canReuseRecentSnapshot": can_reuse }]).to_string();
        let request = call_service(cx, "aiUsage.getAiUsageSnapshot", arguments, move || {
            fetch_snapshot(runtime, can_reuse)
        });
        cx.notify();
        self._fetch = Some(cx.spawn(async move |this, cx| {
            let result = request.await.map_err(|failure| failure.display_message());
            let _ = this.update(cx, |panel, cx| panel.finish_fetch(result, cx));
        }));
    }

    fn finish_fetch(&mut self, result: Result<AiUsageSnapshot, String>, cx: &mut Context<Self>) {
        match self.polling.finish_fetch(result, now_ms(cx)) {
            Some(fetch) => self.start_fetch(fetch, cx),
            None => {
                self._fetch = None;
                self.schedule_auto_load(cx);
            }
        }
        cx.notify();
    }
}

fn read_cached_snapshot(
    runtime: Option<Arc<DesktopRuntime>>,
) -> ServiceResult<Option<AiUsageSnapshot>> {
    let runtime = runtime.ok_or_else(|| failure(USAGE_SERVICE_UNAVAILABLE))?;
    runtime
        .ai_usage_service()
        .get_cached_ai_usage_snapshot()
        .map_err(failure)
}

fn fetch_snapshot(
    runtime: Option<Arc<DesktopRuntime>>,
    can_reuse_recent_snapshot: bool,
) -> ServiceResult<AiUsageSnapshot> {
    let runtime = runtime.ok_or_else(|| failure(USAGE_SERVICE_UNAVAILABLE))?;
    runtime
        .ai_usage_service()
        .get_ai_usage_snapshot(can_reuse_recent_snapshot)
        .map(|snapshot| (*snapshot).clone())
        .map_err(failure)
}

impl AiUsagePanel {
    /// `flex items-center justify-between`: 조회 상태 한 줄과 새로고침 버튼
    fn render_header(
        &self,
        theme: &ThemeTokens,
        now_ms: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let status =
            usage_status_text(&self.usage, &self.polling.usage, now_ms).unwrap_or_default();
        let label = self.usage.translate("refresh");
        let is_refreshing = self.polling.usage.is_refreshing;
        let mut button = div()
            .id("ai-usage-refresh")
            .relative()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .px(spacing(2.0))
            .py(spacing(1.0))
            .text_scale(text_scale::PX_11)
            .text_color(theme.text.secondary)
            .child(aligned_text(
                label.clone(),
                text_scale::PX_11,
                TextFamily::Sans,
            ))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone().into(),
                text: label.into(),
            }));
        button = match is_refreshing {
            true => button.opacity(0.5),
            false => {
                let hover_background = theme.background.page;
                let hover_text = theme.text.primary;
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_background).text_color(hover_text))
                    .on_click(cx.listener(|panel, _, _, cx| panel.refresh(cx)))
            }
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(visible_text(status, text_scale::PX_11).text_color(theme.text.muted))
            .child(button)
    }
}

impl Render for AiUsagePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let now_ms = now_ms(cx);
        let navigator = self.navigator.clone();
        let accounts_href = format!("/{}/ai-accounts", self.locale.as_str());
        let context = UsageCardContext {
            theme: &theme,
            usage: &self.usage,
            now_ms,
            on_manage_accounts: Rc::new(move |_, cx| navigator.navigate(&accounts_href, cx)),
        };
        let has_failed_without_values =
            self.polling.usage.has_failed && self.polling.usage.snapshot.is_none();
        let cards = self.polling.usage.snapshot.as_ref().map(|snapshot| {
            group_accounts_by_provider(&snapshot.accounts)
                .iter()
                .map(|group| render_provider_card(&context, group))
                .collect::<Vec<_>>()
        });
        div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .child(self.render_header(&theme, now_ms, cx))
            .children(has_failed_without_values.then(|| {
                visible_text(self.usage.translate("snapshotFailed"), text_scale::XS)
                    .text_color(theme.status.error)
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing(2.0))
                    .children(cards.into_iter().flatten()),
            )
    }
}

impl super::TaskDetailView {
    /// 사용량 패널은 보이는 동안만 있다. 닫으면 버려 더 조회하지 않고, 다시 열면 저장된 결과부터 다시 읽는다
    pub(super) fn sync_usage_panel(
        &mut self,
        is_visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (is_visible, self.usage_panel.is_some()) {
            (true, false) => {
                let locale = self.screen.route.locale;
                let navigator = self.screen.navigator.clone();
                self.usage_panel =
                    Some(cx.new(|cx| AiUsagePanel::new(locale, navigator, window, cx)));
            }
            (false, true) => self.usage_panel = None,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_ai::usage::types::{
        AiUsageAccountResult, AiUsageProvider, AiUsageStatus, AiUsageWindow, AiUsageWindowKind,
    };

    const FETCHED_AT_MS: i64 = 1_786_341_600_000;
    const INTERVAL_MS: i64 = AI_USAGE_REFRESH_INTERVAL_MS as i64;

    fn snapshot(used_percent: f64) -> AiUsageSnapshot {
        AiUsageSnapshot {
            fetched_at: "2026-08-10T06:00:00.000Z".into(),
            accounts: vec![AiUsageAccountResult {
                provider: AiUsageProvider::Claude,
                account_id: "account".into(),
                label: "me@example.com".into(),
                status: AiUsageStatus::Ok,
                plan_name: None,
                windows: vec![AiUsageWindow {
                    kind: AiUsageWindowKind::Session,
                    model_name: None,
                    used_percent,
                    resets_at: None,
                }],
                reason: None,
                fetched_at: "2026-08-10T06:00:00.000Z".into(),
            }],
        }
    }

    fn shown_percent(polling: &UsagePolling) -> Option<f64> {
        let snapshot = polling.usage.snapshot.as_ref()?;
        Some(snapshot.accounts[0].windows[0].used_percent)
    }

    fn reuse(can_reuse_recent_snapshot: bool) -> Option<UsageFetch> {
        Some(UsageFetch {
            can_reuse_recent_snapshot,
        })
    }

    /// 패널을 열어 첫 자동 조회를 낸 상태
    fn opened() -> UsagePolling {
        let mut polling = UsagePolling::default();
        assert!(polling.take_cache_request());
        assert_eq!(polling.auto_load_delay(FETCHED_AT_MS), Duration::ZERO);
        assert_eq!(polling.begin_fetch(true), reuse(true));
        polling
    }

    #[test]
    fn saved_result_shows_while_the_new_read_is_pending() {
        let mut polling = opened();
        polling.usage.show_cached(Some(snapshot(22.0)));
        assert_eq!(shown_percent(&polling), Some(22.0));
        assert!(polling.usage.is_refreshing && !polling.usage.is_loading());

        polling.finish_fetch(Ok(snapshot(80.0)), FETCHED_AT_MS);

        assert_eq!(shown_percent(&polling), Some(80.0));
        assert!(!polling.usage.is_refreshing);
    }

    #[test]
    fn loading_only_without_cache() {
        let mut polling = opened();
        polling.usage.show_cached(None);
        assert!(polling.usage.is_loading());

        polling.finish_fetch(Ok(snapshot(80.0)), FETCHED_AT_MS);

        assert!(!polling.usage.is_loading());
    }

    #[test]
    fn late_cache_does_not_overwrite_a_new_result() {
        let mut polling = opened();
        polling.finish_fetch(Ok(snapshot(80.0)), FETCHED_AT_MS);

        polling.usage.show_cached(Some(snapshot(22.0)));

        assert_eq!(shown_percent(&polling), Some(80.0));
    }

    #[test]
    fn failed_read_keeps_the_cache() {
        let mut polling = opened();
        polling.usage.show_cached(Some(snapshot(22.0)));

        polling.finish_fetch(Err("ipc down".into()), FETCHED_AT_MS);

        assert!(polling.usage.has_failed);
        assert_eq!(shown_percent(&polling), Some(22.0));
    }

    #[test]
    fn refresh_during_a_read_reads_once_more_without_reuse() {
        let mut polling = opened();
        assert_eq!(polling.request_refresh(), None);
        assert_eq!(polling.request_refresh(), None);

        let follow_up = polling.finish_fetch(Ok(snapshot(80.0)), FETCHED_AT_MS);
        assert_eq!(follow_up, reuse(false));
        assert_eq!(
            polling.finish_fetch(Ok(snapshot(10.0)), FETCHED_AT_MS),
            None
        );

        assert_eq!(shown_percent(&polling), Some(10.0));
        assert!(!polling.usage.is_refreshing);
    }

    #[test]
    fn refresh_reads_again_regardless_of_the_interval() {
        let mut polling = opened();
        polling.finish_fetch(Ok(snapshot(22.0)), FETCHED_AT_MS);

        assert_eq!(polling.request_refresh(), reuse(false));
    }

    #[test]
    fn own_reads_share_recent_results_and_refresh_reads_fresh() {
        let mut polling = UsagePolling::default();
        assert_eq!(polling.begin_fetch(true), reuse(true));
        polling.finish_fetch(Ok(snapshot(22.0)), FETCHED_AT_MS);

        assert_eq!(polling.request_refresh(), reuse(false));
    }

    #[test]
    fn reads_again_every_five_minutes_from_the_result_time() {
        let mut polling = opened();
        polling.finish_fetch(Ok(snapshot(22.0)), FETCHED_AT_MS);

        assert_eq!(
            polling.auto_load_delay(FETCHED_AT_MS),
            Duration::from_millis(INTERVAL_MS as u64)
        );
        assert_eq!(
            polling.auto_load_delay(FETCHED_AT_MS + INTERVAL_MS),
            Duration::ZERO
        );
        assert_eq!(polling.begin_fetch(true), reuse(true));
    }

    #[test]
    fn overdue_read_runs_as_soon_as_the_window_returns() {
        let mut polling = opened();
        polling.finish_fetch(Ok(snapshot(22.0)), FETCHED_AT_MS);

        // 창이 뒤에 있던 동안 예약 시각이 지나, 돌아오는 순간 남은 시간이 0이다
        let returned_at = FETCHED_AT_MS + INTERVAL_MS + 60_000;
        assert_eq!(polling.auto_load_delay(returned_at), Duration::ZERO);
    }

    #[test]
    fn stale_result_waits_a_full_interval_before_reading_again() {
        let mut polling = opened();
        let now = FETCHED_AT_MS + INTERVAL_MS * 3;

        polling.finish_fetch(Ok(snapshot(22.0)), now);

        assert_eq!(
            polling.auto_load_delay(now),
            Duration::from_millis(INTERVAL_MS as u64)
        );
    }

    #[test]
    fn failed_read_retries_on_the_next_interval() {
        let mut polling = opened();

        polling.finish_fetch(Err("ipc down".into()), FETCHED_AT_MS);

        assert_eq!(
            polling.auto_load_delay(FETCHED_AT_MS + INTERVAL_MS - 1),
            Duration::from_millis(1)
        );
        assert_eq!(
            polling.auto_load_delay(FETCHED_AT_MS + INTERVAL_MS),
            Duration::ZERO
        );
    }

    #[test]
    fn cache_is_requested_once_per_open_panel() {
        let mut polling = UsagePolling::default();
        assert!(polling.take_cache_request());
        assert!(!polling.take_cache_request());
    }
}
