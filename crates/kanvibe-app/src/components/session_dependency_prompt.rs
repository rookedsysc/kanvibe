//! 세션 도구 설치 확인(TS `ensureSessionDependencyWithPrompt`). 새 작업·분기 모달(보드와 작업 상세가 함께 띄운다), 작업 상세의
//! 자동 연결과 세션 타입 편집이 세션을 만들거나 바꾸기 전에 그 호스트에 도구가 있는지 보고, 없으면 설치할지 묻는다.
//! 거절하면 만들지 않고, 설치가 실패하면 사유로 멈춘다. 설치 질문 문구는 `gh` 설치 확인(TS `ensureGitHubCliWithPrompt`)도 함께 쓴다.

use std::sync::Arc;

use gpui_kit::AsyncWindowContext;
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task::SessionType;

use crate::qa::prompts::confirm_action;
use crate::views::board::services::{BoardServices, SessionDependencyCheck};

/// 만들기를 이어 가도 되면 `Ok(true)`, 사용자가 설치를 거절했으면 `Ok(false)`, 확인·설치가 실패하면 화면에 보일 사유
pub(crate) async fn ensure_session_dependency(
    services: Arc<BoardServices>,
    session_type: SessionType,
    ssh_host: Option<String>,
    locale: Locale,
    cx: &mut AsyncWindowContext,
) -> Result<bool, String> {
    let status = cx
        .update(|_, cx| services.session_dependency_status(session_type, ssh_host.clone(), cx))
        .map_err(|error| error.to_string())?
        .await
        .map_err(|failure| failure.reason)?;
    // 설치할 도구가 없는 세션 종류는 확인 없이 통과시킨다
    let Some(tool_name) = status.tool_name.clone().filter(|_| !status.is_available) else {
        return Ok(true);
    };
    let common = Translator::new(locale, "common");
    let message = install_prompt(&common, &status, &tool_name);
    let confirmation = cx
        .update(|window, cx| confirm_action(&message, None, window, cx))
        .map_err(|error| error.to_string())?;
    if !confirmation.await {
        return Ok(false);
    }
    let installed = cx
        .update(|_, cx| services.install_session_dependency(session_type, ssh_host, cx))
        .map_err(|error| error.to_string())?
        .await
        .map_err(|failure| failure.reason)?;
    if installed.is_available {
        return Ok(true);
    }
    let reason = installed
        .blocked_reason
        .unwrap_or_else(|| "unknown error".to_string());
    Err(common.translate_with(
        "sessionDependency.installFailed",
        &[("tool", &tool_name), ("error", &reason)],
    ))
}

/// "{대상}에 {도구}이(가) 설치되어 있지 않습니다. 지금 설치하시겠습니까?"
pub(crate) fn install_prompt(
    common: &Translator,
    status: &SessionDependencyCheck,
    tool_name: &str,
) -> String {
    let target = if status.is_remote {
        let host = status.ssh_host.as_deref().unwrap_or("remote");
        common.translate_with("sessionDependency.remoteTarget", &[("host", host)])
    } else {
        common.translate("sessionDependency.localTarget")
    };
    common.translate_with(
        "sessionDependency.installPrompt",
        &[("tool", tool_name), ("target", &target)],
    )
}
