//! 보드 이벤트 상단 경고(TS `BoardEventAlert`). hook 자동 설치 실패·다른 데스크탑 삭제 전파 실패·원격 Herdr machine
//! 등록 실패를 화면 위쪽 가운데에 알린다. 5초 뒤 사라지지만, 다른 데스크탑에 남은 task 경고는 사용자가 직접 지워야
//! 하므로 닫을 때까지 남긴다.

use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Task, Window, deferred, div, px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_services::board_notifier::BoardEvent;

use super::BoardView;
use super::header::{qa_button_layer, qa_text_layer};
use crate::components::{TailwindText, box_shadows, fade};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// TS `AUTO_DISMISS_MS`
const AUTO_DISMISS_AFTER: Duration = Duration::from_millis(5_000);
/// `max-w-xl`
const ALERT_MAX_WIDTH: f32 = 576.0;
/// `z-[700]`: 토스트·대화상자보다 위
const ALERT_LAYER_PRIORITY: usize = 700;

/// 경고 한 건의 글자
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoardEventAlert {
    pub title: String,
    pub body: String,
    /// 닫을 때까지 남기는 경고(다른 데스크탑에 남은 task)
    pub is_sticky: bool,
}

/// 경고로 알릴 보드 이벤트만 글자로 바꾼다. 나머지 이벤트는 `None`이다
pub(crate) fn board_event_alert(event: &BoardEvent, locale: Locale) -> Option<BoardEventAlert> {
    let labels = Translator::new(locale, "task");
    match event {
        BoardEvent::TaskHookInstallFailed(payload) => Some(BoardEventAlert {
            title: labels.translate("createHooksBackgroundFailedTitle"),
            body: labels.translate_with(
                "createHooksBackgroundFailedBody",
                &[("taskTitle", &payload.task_title)],
            ),
            is_sticky: false,
        }),
        BoardEvent::TaskPeerDeletionFailed(payload) => Some(BoardEventAlert {
            title: labels.translate("peerDeletionFailedTitle"),
            body: labels.translate_with(
                "peerDeletionFailedBody",
                &[
                    ("taskTitle", &payload.task_title),
                    ("peerUrls", &payload.failed_peer_urls.join(", ")),
                ],
            ),
            is_sticky: true,
        }),
        BoardEvent::HerdrMachineRegistrationFailed(payload) => Some(BoardEventAlert {
            title: labels.translate_with(
                "herdrMachineRegistrationFailedTitle",
                &[("sshHost", &payload.ssh_host)],
            ),
            body: labels.translate_with(
                "herdrMachineRegistrationFailedBody",
                &[
                    ("reason", &payload.reason),
                    ("sshHost", &payload.ssh_host),
                    ("herdrSession", &payload.herdr_session),
                ],
            ),
            is_sticky: false,
        }),
        _ => None,
    }
}

/// 보이는 경고와 자동 닫기 타이머
#[derive(Default)]
pub(super) struct BoardEventAlertState {
    pub alert: Option<BoardEventAlert>,
    dismiss_timer: Option<Task<()>>,
}

impl BoardView {
    /// 새 경고가 오면 이전 경고와 타이머를 바꾼다
    pub(super) fn show_board_event_alert(&mut self, event: &BoardEvent, cx: &mut Context<Self>) {
        let Some(alert) = board_event_alert(event, self.locale()) else {
            return;
        };
        let is_sticky = alert.is_sticky;
        self.event_alert.alert = Some(alert);
        self.event_alert.dismiss_timer = (!is_sticky).then(|| {
            let timer = cx.background_executor().timer(AUTO_DISMISS_AFTER);
            cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |this, cx| this.dismiss_board_event_alert(cx));
            })
        });
        cx.notify();
    }

    fn dismiss_board_event_alert(&mut self, cx: &mut Context<Self>) {
        self.event_alert = BoardEventAlertState::default();
        cx.notify();
    }

    /// 보이는 경고 글자(시험과 QA가 읽는다)
    pub fn board_event_alert_text(&self) -> Option<(String, String)> {
        let alert = self.event_alert.alert.as_ref()?;
        Some((alert.title.clone(), alert.body.clone()))
    }

    pub(super) fn render_board_event_alert(
        &self,
        theme: &ThemeTokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let alert = self.event_alert.alert.as_ref()?;
        let close_label = Translator::new(self.locale(), "common").translate("close");
        let close = div()
            .id("board-event-alert-close")
            .relative()
            .flex_none()
            .text_scale(text_scale::BASE)
            .line_height(px(16.0))
            .text_color(theme.text.muted)
            .cursor_pointer()
            .hover(|style| style.text_color(theme.text.primary))
            .on_click(cx.listener(|this, _, _, cx| this.dismiss_board_event_alert(cx)))
            .child("×")
            .child(qa_button_layer(&close_label, "×"));
        let card = div()
            .relative()
            .w_full()
            .max_w(px(ALERT_MAX_WIDTH))
            .rounded(radius::XL)
            .border_1()
            .border_color(fade(theme.status.error, 0.2))
            .bg(fade(theme.background.surface, 0.95))
            .shadow(box_shadows(&theme.shadow.lg))
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(qa_text_layer(
                "alert",
                &format!("{} {}", alert.title, alert.body),
            ))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(spacing(3.0))
                    .child(alert_text(theme, alert))
                    .child(close),
            );
        let viewport = self.overlay_viewport(window);
        Some(
            deferred(
                div()
                    .absolute()
                    .top(spacing(4.0))
                    .left_0()
                    .w(viewport.width)
                    .flex()
                    .justify_center()
                    .px(spacing(4.0))
                    .child(card),
            )
            .with_priority(ALERT_LAYER_PRIORITY)
            .into_any_element(),
        )
    }
}

fn alert_text(theme: &ThemeTokens, alert: &BoardEventAlert) -> impl IntoElement {
    div()
        .min_w_0()
        .flex_1()
        .text_scale(text_scale::SM)
        .child(
            div()
                .css_weight(600)
                .text_color(theme.status.error)
                .child(alert.title.clone()),
        )
        .child(
            div()
                .mt(spacing(1.0))
                .text_color(theme.text.secondary)
                .child(alert.body.clone()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_services::board_notifier::{
        HerdrMachineRegistrationFailedPayload, TaskHookInstallFailedPayload,
        TaskPeerDeletionFailedPayload,
    };

    // parity: TS-renderer-BoardEventAlert-578b59
    #[test]
    fn hook_install_failure_becomes_a_top_alert_that_dismisses_itself() {
        let event = BoardEvent::TaskHookInstallFailed(TaskHookInstallFailedPayload {
            task_id: "t1".into(),
            task_title: "보드 필터 추가".into(),
            error: "denied".into(),
        });
        let alert = board_event_alert(&event, Locale::Ko).expect("alert");
        assert_eq!(alert.title, "Hooks 자동 설치 실패");
        assert!(
            alert
                .body
                .contains("\"보드 필터 추가\" hooks 자동 설치에 실패했습니다")
        );
        assert!(!alert.is_sticky);
    }

    // parity: TS-renderer-BoardEventAlert-6dae7a
    #[test]
    fn herdr_registration_failure_names_host_reason_and_session() {
        let event =
            BoardEvent::HerdrMachineRegistrationFailed(HerdrMachineRegistrationFailedPayload {
                ssh_host: "devbox".into(),
                herdr_session: "kanvibe".into(),
                reason: "auth refused".into(),
            });
        let alert = board_event_alert(&event, Locale::Ko).expect("alert");
        assert_eq!(alert.title, "devbox Herdr machine 등록 실패");
        assert!(alert.body.contains("auth refused"));
        assert!(
            alert
                .body
                .contains("herdr --remote devbox --session kanvibe")
        );
    }

    // parity: TS-renderer-BoardEventAlert-06a1cc
    #[test]
    fn peer_deletion_failure_lists_where_the_task_remains() {
        let event = BoardEvent::TaskPeerDeletionFailed(TaskPeerDeletionFailedPayload {
            task_id: "t1".into(),
            task_title: "로그인 후 리다이렉트 수정".into(),
            failed_peer_urls: vec!["http://192.0.2.10:9736".into(), "http://b".into()],
        });
        let alert = board_event_alert(&event, Locale::Ko).expect("alert");
        assert_eq!(alert.title, "다른 데스크탑에서는 삭제하지 못함");
        assert!(
            alert
                .body
                .contains("http://192.0.2.10:9736, http://b에는 남아 있습니다")
        );
        assert!(alert.is_sticky);
    }

    #[test]
    fn other_board_events_raise_no_alert() {
        assert_eq!(
            board_event_alert(&BoardEvent::BoardUpdated, Locale::Ko),
            None
        );
    }
}
