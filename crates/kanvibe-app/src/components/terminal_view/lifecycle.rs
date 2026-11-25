//! 터미널 연결의 일생(TS `Terminal.tsx`의 `connect`): 처음 칸 수를 알면 열고, SSH 전송 쿨다운 힌트가 오면 한 번만
//! 기다렸다가 다시 열고, 숨겨진 동안에는 그 기다림을 멈춘다. 창이 다시 앞으로 오면 크기를 다시 알린다.

use std::time::Duration;

use gpui_kit::{AppContext, Context};
use kanvibe_terminal::terminal_bridge::{TerminalBridgeError, TerminalOpenResult};
use serde_json::Value;

use super::pane::{TerminalPane, TerminalPaneEvent};
use super::status::TerminalConnection;
use crate::qa::faults::FaultOutcome;
use crate::services::{AppServices, apply_fault, format_service_error_for};

/// TS preload `openTerminal`이 부르던 IPC 채널. QA 고장 주입의 대상 이름이다
pub const TERMINAL_OPEN_CHANNEL: &str = "kanvibe:terminal-open";

impl TerminalPane {
    /// 처음 칸 수를 알게 되면 연결하고, 그 뒤에는 크기만 알린다
    pub(super) fn apply_grid_size(&mut self, columns: u16, rows: u16, cx: &mut Context<Self>) {
        if self.reported_size == Some((columns, rows)) {
            return;
        }
        let is_first_size = self.reported_size.is_none();
        self.reported_size = Some((columns, rows));
        if is_first_size {
            self.open(cx);
            return;
        }
        self.reassert_size();
    }

    /// PTY에 지금 칸 수를 다시 알린다. 다른 창이 같은 세션 크기를 바꿨을 수 있어 창이 앞으로 올 때도 부른다
    pub(super) fn reassert_size(&self) {
        if let (Some(transport), Some(window), Some((columns, rows))) = (
            self.transport.as_ref(),
            self.window.as_ref(),
            self.reported_size,
        ) {
            transport.resize(window.id(), &self.target, columns, rows);
        }
    }

    /// QA 모드에서는 TS 채널 이름(`kanvibe:terminal-open`)으로 고장 주입 판정을 먼저 거친다
    pub(super) fn open(&mut self, cx: &mut Context<Self>) {
        let (Some(transport), Some(window), Some((columns, rows))) = (
            self.transport.clone(),
            self.window.clone(),
            self.reported_size,
        ) else {
            return;
        };
        let target = self.target.clone();
        let arguments = serde_json::json!([target.task_id, target.tab_id, columns, rows]);
        let fault = cx
            .global::<AppServices>()
            .check_fault(TERMINAL_OPEN_CHANNEL, &arguments.to_string());
        let executor = cx.background_executor().clone();
        let open = cx.background_spawn(async move {
            if let FaultOutcome::Result(value) = &fault {
                return open_result_from_fault_value(value);
            }
            apply_fault(fault, &executor).await.map_err(|failure| {
                // TS는 IPC 거절 메시지(`Error invoking remote method '<채널>': …`)를 연결 오류로 보였다
                TerminalBridgeError::new(format_service_error_for(
                    TERMINAL_OPEN_CHANNEL,
                    &failure.reason,
                ))
            })?;
            transport.open(window, &target, columns, rows)
        });
        self._open_task = Some(cx.spawn(async move |this, cx| {
            let result = open.await;
            let _ = this.update(cx, |pane, cx| pane.finish_open(result, cx));
        }));
    }

    fn finish_open(&mut self, result: TerminalOpenResult, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.connection = TerminalConnection::Ready;
                self.closed_reason = None;
                cx.emit(TerminalPaneEvent::Connected);
            }
            Err(error) => match error.retry_after_ms {
                Some(wait) if !self.has_retried_after_cooldown => {
                    self.schedule_cooldown_retry(Duration::from_millis(u64::from(wait.get())), cx);
                }
                _ => {
                    let message = match error.message.is_empty() {
                        true => self.translator.translate("terminalConnectionFailed"),
                        false => error.message,
                    };
                    self.connection = TerminalConnection::Failed(message);
                }
            },
        }
        cx.notify();
    }

    /// 전송 쿨다운 힌트가 오면 한 번만 기다렸다가 다시 연다. 숨겨져 있으면 기다리지 않고 수동 재시도를 남긴다
    fn schedule_cooldown_retry(&mut self, wait: Duration, cx: &mut Context<Self>) {
        self.has_retried_after_cooldown = true;
        if self.is_hidden {
            self.pause_reconnect();
            return;
        }
        self.connection = TerminalConnection::Reconnecting;
        self._cooldown_wait = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |pane, cx| {
                pane._cooldown_wait = None;
                pane.open(cx);
            });
        }));
    }

    fn pause_reconnect(&mut self) {
        self._cooldown_wait = None;
        self.connection =
            TerminalConnection::Failed(self.translator.translate("terminalReconnectPaused"));
    }

    /// 다시 연결 버튼과 Herdr 서버 교체 뒤 재연결
    pub fn reconnect(&mut self, cx: &mut Context<Self>) {
        if let (Some(transport), Some(window)) = (self.transport.as_ref(), self.window.as_ref()) {
            transport.close(window.id(), &self.target);
        }
        self._cooldown_wait = None;
        self.connection = TerminalConnection::Connecting;
        self.closed_reason = None;
        self.has_retried_after_cooldown = false;
        self.open(cx);
        cx.notify();
    }

    /// 화면이 이 터미널을 가리거나(비활성 탭·채팅 기록) 다시 보인다. 가려지면 예약한 재연결을 멈춘다
    pub fn set_hidden(&mut self, is_hidden: bool, cx: &mut Context<Self>) {
        if self.is_hidden == is_hidden {
            return;
        }
        self.is_hidden = is_hidden;
        if is_hidden && self._cooldown_wait.is_some() {
            self.pause_reconnect();
        }
        cx.notify();
    }
}

/// QA 고장 주입 값(TS 응답 `{ ok, error, retryAfterMs }`)을 열기 결과로 바꾼다
fn open_result_from_fault_value(value: &Value) -> TerminalOpenResult {
    if value.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(());
    }
    let message = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let retry_after_ms = value
        .get("retryAfterMs")
        .and_then(Value::as_u64)
        .and_then(|wait| u32::try_from(wait).ok())
        .and_then(std::num::NonZeroU32::new);
    Err(TerminalBridgeError {
        message,
        retry_after_ms,
    })
}
