//! 백그라운드 동기화 상태(TS `useBackgroundTaskSync`). TS는 렌더러 모듈 하나가 상태를 들고 알림 센터·커맨드 팔레트가
//! 함께 읽었다. 여기서는 앱 전역 엔티티 하나가 그 자리다. 처음 쓰일 때 상태를 한 번 읽고, 런타임이 내는
//! `background-sync-status-changed` 보드 이벤트로 바꾼다. "지금 동기화"는 런타임 `BackgroundTaskSync`의 같은 인스턴스로
//! 한 사이클을 백그라운드에서 돌린다.

use std::sync::Arc;

use gpui_kit::{App, AppContext, Context, Entity, Global, Subscription};
use kanvibe_domain::background_sync::{BackgroundTaskSyncPhase, BackgroundTaskSyncStatus};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_services::board_notifier::BoardEvent;

use crate::app_events::{AppEvents, BoardEventReceived};
use crate::views::board::services::{BoardServiceResult, BoardServices, board_services};

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

/// 앱 전역 동기화 상태 엔티티
pub struct BackgroundSyncStore {
    state: BackgroundSyncState,
    services: Option<Arc<BoardServices>>,
    _board_events: Option<Subscription>,
}

#[derive(Clone)]
struct BackgroundSyncGlobal(Entity<BackgroundSyncStore>);

impl Global for BackgroundSyncGlobal {}

/// 전역 동기화 상태. 처음 부를 때 만들고 상태를 한 번 읽는다
pub fn background_sync_store(cx: &mut App) -> Entity<BackgroundSyncStore> {
    if let Some(global) = cx.try_global::<BackgroundSyncGlobal>() {
        return global.0.clone();
    }
    let store = cx.new(|cx| {
        let mut store = BackgroundSyncStore {
            state: BackgroundSyncState::default(),
            services: board_services(cx),
            _board_events: subscribe_to_status_events(cx),
        };
        store.load_status(cx);
        store
    });
    cx.set_global(BackgroundSyncGlobal(store.clone()));
    store
}

fn subscribe_to_status_events(cx: &mut Context<BackgroundSyncStore>) -> Option<Subscription> {
    let bus = cx.try_global::<AppEvents>()?.0.clone();
    Some(
        cx.subscribe(&bus, |store, _, event: &BoardEventReceived, cx| {
            if let BoardEvent::BackgroundSyncStatusChanged(payload) = &event.0 {
                store.receive_status(payload.status.clone(), cx);
            }
        }),
    )
}

impl BackgroundSyncStore {
    pub fn state(&self) -> &BackgroundSyncState {
        &self.state
    }

    /// 상태를 다시 읽는다(TS `refresh`)
    fn load_status(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        let revision = self.state.next_revision();
        let load = services.load_background_sync_status(cx);
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| this.apply_result(revision, result, cx));
        })
        .detach();
    }

    /// 런타임 보드 이벤트로 온 새 상태
    fn receive_status(&mut self, status: BackgroundTaskSyncStatus, cx: &mut Context<Self>) {
        self.state.next_revision();
        self.state.status = Some(status);
        self.state.error = None;
        cx.notify();
    }

    /// "지금 동기화"(실패 뒤에는 "다시 시도"). 상태를 못 읽은 채 실패했으면 상태부터 다시 읽는다
    pub fn press_button(&mut self, cx: &mut Context<Self>) {
        if self.state.error.is_some() && self.state.status.is_none() {
            self.load_status(cx);
            return;
        }
        self.start(cx);
    }

    /// 한 사이클을 시작한다. 이미 돌고 있으면 아무것도 하지 않는다(TS `start`)
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.state.is_running() {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.state.is_starting = true;
        let revision = self.state.next_revision();
        let start = services.start_background_sync(cx);
        cx.spawn(async move |this, cx| {
            let result = start.await;
            let _ = this.update(cx, |this, cx| {
                this.state.is_starting = false;
                this.apply_result(revision, result, cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_result(
        &mut self,
        revision: u64,
        result: BoardServiceResult<BackgroundTaskSyncStatus>,
        cx: &mut Context<Self>,
    ) {
        if revision == self.state.revision {
            match result {
                Ok(status) => {
                    self.state.status = Some(status);
                    self.state.error = None;
                }
                Err(failure) => self.state.error = Some(failure.reason),
            }
        }
        cx.notify();
    }
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
