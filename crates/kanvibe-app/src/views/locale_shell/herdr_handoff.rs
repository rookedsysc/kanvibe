//! 원격 Herdr 서버 교체 확인(TS `HerdrServerHandoffPrompt`). 원격 서버가 이전 방식으로 떠 있다는 보드 알림을 받으면
//! 호스트를 밝혀 교체할지 묻고, 승인하면 교체를 요청한다. 같은 서버는 앱이 실행되는 동안 처음 받은 창 하나만 묻는다.
//! 교체 결과(실패 포함)는 런타임이 보드 알림으로 따로 알리므로 여기서는 요청만 보낸다.

use gpui_kit::{AppContext, Context};
use kanvibe_domain::i18n::Translator;
use kanvibe_services::board_notifier::{BoardEvent, HerdrServerHandoffRequiredPayload};

use super::LocaleShell;
use crate::qa::prompts::confirm_action;
use crate::services::{AppServices, apply_fault};

const CLAIM_SERVICE: &str = "sessionDependency.claimHerdrServerHandoffPrompt";
const REPLACE_SERVICE: &str = "sessionDependency.replaceHerdrServer";

impl LocaleShell {
    /// 교체가 필요하다는 알림만 받는다
    pub(super) fn receive_herdr_handoff_event(
        &mut self,
        event: &BoardEvent,
        cx: &mut Context<Self>,
    ) {
        let BoardEvent::HerdrServerHandoffRequired(payload) = event else {
            return;
        };
        let Some(runtime) = cx.global::<AppServices>().runtime().cloned() else {
            return;
        };
        let arguments = serde_json::json!([payload.ssh_host, payload.herdr_session]).to_string();
        let services = cx.global::<AppServices>();
        let claim_fault = services.check_fault(CLAIM_SERVICE, &arguments);
        let replace_fault = services.check_fault(REPLACE_SERVICE, &arguments);
        let payload = payload.clone();
        let message = Translator::new(self.locale, "common").translate_with(
            "sessionDependency.herdrServerHandoffPrompt",
            &[("host", &payload.ssh_host)],
        );
        let window_handle = self.window_handle;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let is_claimed = apply_fault(claim_fault, &executor).await.is_ok()
                && runtime
                    .session_dependency()
                    .claim_herdr_server_handoff_prompt(&payload.ssh_host, &payload.herdr_session);
            if !is_claimed {
                return;
            }
            let Ok(confirmation) = window_handle.update(cx, |_, window, cx| {
                confirm_action(&message, None, window, cx)
            }) else {
                return;
            };
            if !confirmation.await {
                return;
            }
            let _ = this.update(cx, |this, _| this.herdr_replacements.push(payload.clone()));
            let replace = cx.background_spawn(replace_herdr_server(
                runtime,
                payload,
                replace_fault,
                executor,
            ));
            if let Err(reason) = replace.await {
                eprintln!("[kanvibe] Herdr 서버 교체 요청 실패: {reason}");
            }
        })
        .detach();
    }

    /// 이 창이 교체를 요청한 서버(시험이 읽는다)
    pub fn herdr_replacement_requests(&self) -> Vec<HerdrServerHandoffRequiredPayload> {
        self.herdr_replacements.clone()
    }
}

/// 교체를 요청한다. 결과는 런타임이 보드 알림으로 따로 알린다
async fn replace_herdr_server(
    runtime: std::sync::Arc<kanvibe_runtime::DesktopRuntime>,
    payload: HerdrServerHandoffRequiredPayload,
    fault: crate::qa::faults::FaultOutcome,
    executor: gpui_kit::BackgroundExecutor,
) -> Result<(), String> {
    apply_fault(fault, &executor)
        .await
        .map_err(|failure| failure.reason)?;
    runtime
        .session_dependency()
        .replace_herdr_server(&payload.ssh_host, &payload.herdr_session)
        .map_err(|error| error.to_string())
}
