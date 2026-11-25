//! 알림 센터 맨 위 백그라운드 동기화 칸(TS `BackgroundSyncStatus` + `useBackgroundTaskSync`).
//! 보드가 열리면 상태를 한 번 읽고, 런타임이 내는 `background-sync-status-changed` 보드 이벤트로 바꾼다.
//! "지금 동기화"는 런타임 `BackgroundTaskSync`의 같은 인스턴스로 한 사이클을 백그라운드에서 돌린다.

use gpui_kit::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, px,
};
use kanvibe_domain::background_sync::{BackgroundTaskSyncPhase, BackgroundTaskSyncStatus};
use kanvibe_domain::i18n::{Locale, Translator};

use super::BoardView;
use super::header::{qa_button_layer, qa_text_layer};
use super::services::BoardServiceResult;
use crate::components::{TailwindText, fade};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// TS `useBackgroundTaskSync` 스냅샷. `revision`은 늦게 온 응답이 새 상태를 덮지 않게 한다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackgroundSyncState {
    pub status: Option<BackgroundTaskSyncStatus>,
    pub error: Option<String>,
    pub is_starting: bool,
    revision: u64,
}

impl BackgroundSyncState {
    pub fn is_running(&self) -> bool {
        self.is_starting || self.status.as_ref().is_some_and(|status| status.is_running)
    }

    pub fn is_failed(&self) -> bool {
        self.error.is_some()
            || self
                .status
                .as_ref()
                .is_some_and(|status| status.phase == BackgroundTaskSyncPhase::Failed)
    }

    /// 버튼을 누를 수 없는 동안(돌고 있거나 아직 상태를 모를 때)
    pub fn is_button_disabled(&self) -> bool {
        self.is_running() || (self.status.is_none() && self.error.is_none())
    }

    /// 상태 줄 글자(TS `BackgroundSyncStatus`의 `message`)
    pub fn message(&self, locale: Locale) -> String {
        let labels = Translator::new(locale, "common.backgroundSyncStatus");
        if let Some(error) = self.error.clone() {
            return error;
        }
        let Some(status) = &self.status else {
            return labels.translate("loading");
        };
        if let Some(error) = status.error.clone() {
            return error;
        }
        if self.is_starting && !status.is_running {
            return labels.translate("starting");
        }
        let key = match status.phase {
            BackgroundTaskSyncPhase::Idle => "idle",
            BackgroundTaskSyncPhase::Worktrees => "worktrees",
            BackgroundTaskSyncPhase::PullRequests => "pull-requests",
            BackgroundTaskSyncPhase::Pulls => "pulls",
            BackgroundTaskSyncPhase::Failed => "failed",
            BackgroundTaskSyncPhase::Completed if status.review_needed => "reviewNeeded",
            BackgroundTaskSyncPhase::Completed if status.board_updated => "boardUpdated",
            BackgroundTaskSyncPhase::Completed => "noChanges",
        };
        labels.translate(key)
    }

    /// 버튼 글자: 도는 중 / 실패 뒤 다시 시도 / 지금 동기화
    pub fn button_label(&self, locale: Locale) -> String {
        let key = if self.is_running() {
            "running"
        } else if self.is_failed() {
            "retry"
        } else {
            "start"
        };
        Translator::new(locale, "common.backgroundSyncStatus").translate(key)
    }

    fn next_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }
}

impl BoardView {
    pub fn background_sync_state(&self) -> &BackgroundSyncState {
        &self.background_sync
    }

    /// 상태를 다시 읽는다(TS `refresh`)
    pub(super) fn load_background_sync_status(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let revision = self.background_sync.next_revision();
        let load = services.load_background_sync_status(cx);
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                this.apply_background_sync_result(revision, result, cx)
            });
        })
        .detach();
    }

    /// 런타임 보드 이벤트로 온 새 상태
    pub(super) fn receive_background_sync_status(
        &mut self,
        status: BackgroundTaskSyncStatus,
        cx: &mut Context<Self>,
    ) {
        self.background_sync.next_revision();
        self.background_sync.status = Some(status);
        self.background_sync.error = None;
        cx.notify();
    }

    /// "지금 동기화"(실패 뒤에는 "다시 시도"). 상태를 못 읽은 채 실패했으면 상태부터 다시 읽는다
    pub(super) fn press_background_sync_button(&mut self, cx: &mut Context<Self>) {
        if self.background_sync.error.is_some() && self.background_sync.status.is_none() {
            self.load_background_sync_status(cx);
            return;
        }
        self.start_background_sync(cx);
    }

    fn start_background_sync(&mut self, cx: &mut Context<Self>) {
        if self.background_sync.is_running() {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.background_sync.is_starting = true;
        let revision = self.background_sync.next_revision();
        let start = services.start_background_sync(cx);
        cx.spawn(async move |this, cx| {
            let result = start.await;
            let _ = this.update(cx, |this, cx| {
                this.background_sync.is_starting = false;
                this.apply_background_sync_result(revision, result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_background_sync_result(
        &mut self,
        revision: u64,
        result: BoardServiceResult<BackgroundTaskSyncStatus>,
        cx: &mut Context<Self>,
    ) {
        if revision == self.background_sync.revision {
            match result {
                Ok(status) => {
                    self.background_sync.status = Some(status);
                    self.background_sync.error = None;
                }
                Err(failure) => self.background_sync.error = Some(failure.reason),
            }
        }
        cx.notify();
    }

    /// 백그라운드 동기화 칸
    pub(super) fn render_background_sync_status(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let locale = self.locale();
        let sync = &self.background_sync;
        let labels = Translator::new(locale, "common.backgroundSyncStatus");
        let title = labels.translate("title");
        let message = sync.message(locale);
        let message_color = if sync.is_failed() {
            theme.status.error
        } else {
            theme.text.secondary
        };
        let summary = div()
            .min_w_0()
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(title),
            )
            .child(
                div()
                    .relative()
                    .mt(spacing(1.0))
                    .text_scale(text_scale::XS)
                    .text_color(message_color)
                    .child(message.clone())
                    .child(qa_text_layer("status", &message)),
            );
        let progress = sync
            .is_running()
            .then(|| sync_progress(theme, sync, &labels));
        div()
            .border_b_1()
            .border_color(theme.border.default)
            .bg(fade(theme.brand.primary, 0.05))
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(spacing(3.0))
                    .child(summary)
                    .child(self.render_background_sync_button(theme, cx)),
            )
            .children(progress)
    }

    fn render_background_sync_button(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = self.background_sync.button_label(self.locale());
        let is_disabled = self.background_sync.is_button_disabled();
        let hover_background = theme.background.page;
        let button = div()
            .id("background-sync-start")
            .relative()
            .flex_none()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .px(spacing(3.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.primary)
            .child(label.clone())
            .child(qa_button_layer(&label, &label));
        if is_disabled {
            return button.opacity(0.6);
        }
        button
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .on_click(cx.listener(|this, _, _, cx| this.press_background_sync_button(cx)))
    }
}

/// 세 단계 진행 막대와 "닫아도 계속" 안내
fn sync_progress(
    theme: &ThemeTokens,
    sync: &BackgroundSyncState,
    labels: &Translator,
) -> impl IntoElement {
    let completed = sync
        .status
        .as_ref()
        .map(|status| status.completed_stages)
        .unwrap_or(0);
    let segments = (0..3u32).map(|stage| {
        let color = if stage < completed {
            theme.brand.primary
        } else if stage == completed {
            fade(theme.brand.primary, 0.5)
        } else {
            theme.border.default
        };
        div().h(px(4.0)).flex_1().rounded(radius::FULL).bg(color)
    });
    div()
        .child(
            div()
                .mt(spacing(3.0))
                .flex()
                .gap(spacing(1.0))
                .children(segments),
        )
        .child(
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(labels.translate("continues")),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(phase: BackgroundTaskSyncPhase, is_running: bool) -> BackgroundTaskSyncStatus {
        BackgroundTaskSyncStatus {
            phase,
            is_running,
            completed_stages: 1,
            review_needed: false,
            board_updated: false,
            error: None,
        }
    }

    #[test]
    fn message_follows_the_ts_phase_rules() {
        let mut state = BackgroundSyncState::default();
        assert_eq!(state.message(Locale::Ko), "상태 확인 중…");
        assert!(state.is_button_disabled());
        state.status = Some(status(BackgroundTaskSyncPhase::PullRequests, true));
        assert_eq!(state.message(Locale::Ko), "2/3 · PR 상태 확인 중");
        assert_eq!(state.button_label(Locale::Ko), "동기화 중…");
        let mut completed = status(BackgroundTaskSyncPhase::Completed, false);
        completed.board_updated = true;
        state.status = Some(completed);
        assert_eq!(
            state.message(Locale::Ko),
            "동기화를 마치고 보드를 갱신했습니다."
        );
        assert_eq!(state.button_label(Locale::Ko), "지금 동기화");
        assert!(!state.is_button_disabled());
    }

    #[test]
    fn failure_shows_the_reason_and_offers_retry() {
        let state = BackgroundSyncState {
            error: Some("boom".into()),
            ..BackgroundSyncState::default()
        };
        assert_eq!(state.message(Locale::Ko), "boom");
        assert_eq!(state.button_label(Locale::Ko), "다시 시도");
        assert!(!state.is_button_disabled());
    }
}
