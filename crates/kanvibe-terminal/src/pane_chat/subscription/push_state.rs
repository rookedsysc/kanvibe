//! 구독자에게 보낼 상태 만들기. 구독자가 요청한 만큼만 담는다.

use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatPushedState, PaneChatPushedTranscript, PaneChatSnapshot, PaneChatSubscriptionDetail,
    PaneChatSubscriptionTarget,
};

/// 한 번 읽은 pane의 상태. 대화를 보는 구독자가 없는 pane은 에이전트 이름만 확인하므로 스냅샷 전체가 없다
pub(super) enum WatchedPaneState<'a> {
    Chat(&'a PaneChatSnapshot),
    Agent(Option<AiSessionProvider>),
}

/// 화면은 실제로 보일 때만 보내야 작업 중인 CLI의 화면 변화가 계속 전송되지 않는다
pub(super) fn build_subscriber_state(
    target: &PaneChatSubscriptionTarget,
    state: &WatchedPaneState<'_>,
) -> PaneChatPushedState {
    let snapshot = match state {
        WatchedPaneState::Chat(snapshot) if target.detail == PaneChatSubscriptionDetail::Chat => {
            snapshot
        }
        WatchedPaneState::Chat(snapshot) => return provider_only(snapshot.provider),
        WatchedPaneState::Agent(provider) => return provider_only(*provider),
    };
    let transcript = &snapshot.transcript;
    PaneChatPushedState {
        task_id: Some(snapshot.task_id.clone()),
        pane_id: Some(snapshot.pane_id.clone()),
        session_type: Some(snapshot.session_type),
        provider: Some(snapshot.provider),
        target_token: Some(snapshot.target_token.clone()),
        conversation_key: snapshot.conversation_key.clone(),
        can_send: Some(snapshot.can_send),
        input_state: Some(snapshot.input_state),
        agent_activity: Some(snapshot.agent_activity),
        screen: target
            .should_include_screen
            .then(|| snapshot.screen.clone()),
        reason: Some(snapshot.reason.clone()),
        transcript: Some(PaneChatPushedTranscript {
            status: transcript.status,
            reason: transcript.reason.clone(),
            session_id: transcript.session_id.clone(),
            next_cursor: transcript.next_cursor.clone(),
        }),
        model_picker: Some(snapshot.model_picker.clone()),
        model_settings: snapshot.model_settings.clone(),
        prompt: snapshot.prompt.clone(),
        composer: snapshot.composer.clone(),
    }
}

fn provider_only(provider: Option<AiSessionProvider>) -> PaneChatPushedState {
    PaneChatPushedState {
        provider: Some(provider),
        ..PaneChatPushedState::default()
    }
}
