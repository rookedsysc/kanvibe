//! pane 미러와 pane chat(TS `paneMirrorService`, `paneChatService`). 데스크톱 채팅 화면, 모바일 HTTP·WebSocket,
//! 구독 감시가 같은 미러와 같은 pane chat 서비스(협력자 8개)를 함께 써야 같은 pane의 임대·입력 대기열이 하나로 유지된다.

use std::sync::Arc;

use kanvibe_pane_mirror::{
    KanbanMirrorTaskStore, KanbanPaneChatTaskLookup, MirrorTimings, PaneChatRuntime,
    PaneChatRuntimeParts, PaneMirror, PaneMirrorParts, RelayClose, RelayOutput,
    RemoteImagePasteUploader, RemotePaneGrants, RemotePaneRelay, SystemMirrorProcessSpawner,
};
use kanvibe_terminal::pane_chat::attachments::PaneChatAttachmentStore;
use kanvibe_terminal::pane_chat::composer_catalog::PaneChatComposerCatalog;
use kanvibe_terminal::pane_chat::connection_signals::PaneChatConnectionSignals;
use kanvibe_terminal::pane_chat::mode_store::PaneChatModeStore;
use kanvibe_terminal::pane_chat::service::{AiPaneChatSessionRecords, SystemPaneChatClock};
use kanvibe_terminal::pane_chat::subscription::{
    NotifyTranscriptFileWatchers, ThreadPaneChatTimers,
};
use kanvibe_terminal::remote_terminal_protocol::{RemoteTerminalGrant, RemoteTerminalGrantInput};
use kanvibe_terminal::terminal_bridge::TerminalBridge;

use crate::ai::AiParts;
use crate::board::BoardParts;
use crate::config::RuntimeConfig;
use crate::foundation::Foundation;
use crate::terminal::TerminalParts;

/// 원격 terminal agent가 없는 실행. 직접 연결 권한을 내지 않아 모바일은 로컬 미러로 비춘다
pub struct NoRemotePaneGrants;

impl RemotePaneGrants for NoRemotePaneGrants {
    fn prepare_grant(
        &self,
        _input: &RemoteTerminalGrantInput,
    ) -> Result<Option<RemoteTerminalGrant>, String> {
        Ok(None)
    }

    fn attach_relay(
        &self,
        _grant_id: &str,
        _device_id: &str,
        _task_id: &str,
        _pane_id: &str,
        _on_output: RelayOutput,
        _on_close: RelayClose,
    ) -> Result<Box<dyn RemotePaneRelay>, String> {
        Err("remote terminal agent is not available".to_string())
    }

    fn revoke_grant(&self, _grant_id: &str) {}
    fn revoke_device(&self, _device_id: &str) {}
    fn revoke_task(&self, _task_id: &str) {}
    fn stop_agents(&self) {}
}

pub(crate) struct PaneChatParts {
    pub mirror: Arc<PaneMirror>,
    pub pane_chat: Arc<PaneChatRuntime>,
}

/// 조립 재료. 보드·AI 부분이 생긴 뒤에 만든다
pub(crate) struct PaneChatInputs<'a> {
    pub config: &'a RuntimeConfig,
    pub foundation: &'a Foundation,
    pub terminal: &'a TerminalParts,
    pub terminal_bridge: &'a Arc<TerminalBridge>,
    pub board: &'a BoardParts,
    pub ai: &'a AiParts,
}

pub(crate) fn build_pane_chat(inputs: &PaneChatInputs<'_>) -> PaneChatParts {
    let mirror = build_mirror(inputs);
    let foundation = inputs.foundation;
    let attachments = PaneChatAttachmentStore::new(
        inputs.config.app_data_directory.clone(),
        Arc::clone(&foundation.runner),
        Arc::clone(&foundation.ssh_config),
        Arc::new(RemoteImagePasteUploader::new(Arc::clone(
            &inputs.terminal.remote_image_paste,
        ))),
    );
    let sessions = AiPaneChatSessionRecords::new(
        inputs.ai.session_host.clone(),
        None,
        Arc::clone(&inputs.ai.pane_chat_session_bindings),
    );
    let connection_signals = build_connection_signals(inputs);
    let pane_chat = Arc::new(PaneChatRuntime::new(PaneChatRuntimeParts {
        mirror: Arc::clone(&mirror),
        tasks: Arc::new(KanbanPaneChatTaskLookup::new(Arc::clone(
            &inputs.board.kanban,
        ))),
        runner: Arc::clone(&foundation.runner),
        terminals: Arc::clone(&inputs.terminal.registry),
        herdr: inputs.terminal.herdr.clone(),
        sessions: Arc::new(sessions),
        suggestions: Arc::new(PaneChatComposerCatalog::new(Arc::clone(&foundation.files))),
        attachments: Arc::new(attachments),
        modes: Arc::new(PaneChatModeStore::new(Arc::clone(&foundation.database))),
        clock: Arc::new(SystemPaneChatClock),
        timers: Arc::new(ThreadPaneChatTimers::new()),
        file_watchers: Arc::new(NotifyTranscriptFileWatchers),
        connection_signals,
    }));
    PaneChatParts { mirror, pane_chat }
}

/// hook 바인딩이 저장되면 그 Task의 pane 채팅이 바로 다시 확인하도록 바인딩 표와 연결 신호를 잇는다
fn build_connection_signals(inputs: &PaneChatInputs<'_>) -> Arc<PaneChatConnectionSignals> {
    let home = inputs.ai.session_host.home_directory().ok();
    let signals = Arc::new(PaneChatConnectionSignals::new(home.as_deref()));
    let notified = Arc::clone(&signals);
    inputs
        .ai
        .pane_chat_session_bindings
        .on_binding_recorded(Arc::new(move |task_id: &str| notified.notify_task(task_id)));
    signals
}

fn build_mirror(inputs: &PaneChatInputs<'_>) -> Arc<PaneMirror> {
    let foundation = inputs.foundation;
    let terminal = inputs.terminal;
    Arc::new(PaneMirror::new(PaneMirrorParts {
        tasks: Arc::new(KanbanMirrorTaskStore::new(
            Arc::clone(&inputs.board.kanban),
            Arc::clone(&foundation.database),
        )),
        runner: Arc::clone(&foundation.runner),
        processes: Arc::new(SystemMirrorProcessSpawner::new(
            Arc::clone(&foundation.environment),
            Arc::clone(&foundation.ssh_config),
        )),
        terminals: Arc::clone(&terminal.registry),
        attacher: inputs.terminal_bridge.clone(),
        tabs: terminal.tab_service.clone(),
        herdr: terminal.herdr.clone(),
        session_dependencies: Arc::clone(&terminal.session_dependencies),
        zellij_id_targeting_support: Arc::clone(&terminal.zellij_id_targeting_support),
        remote_grants: Arc::clone(&inputs.config.remote_pane_grants),
        board: Arc::clone(&foundation.board),
        timings: MirrorTimings::default(),
    }))
}
