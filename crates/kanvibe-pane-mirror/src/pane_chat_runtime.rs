//! pane chat 서비스와 push 구독을 실제 협력자로 조립한다. 모바일 pane chat 경로와 events 소켓 명령이 이 메서드를 부른다.
//!
//! 요청 본문에서 온 값은 TS처럼 형식부터 확인한다. 형식이 틀리면 [`PaneChatError::Input`]이고(400),
//! 대상이 바뀌었으면 [`PaneChatError::TargetChanged`](409 `target-changed`), 전송 결과를 알 수 없으면
//! [`PaneChatError::DeliveryUncertain`](409 `delivery-uncertain`)이다.

use std::sync::Arc;

use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::pane_chat::snapshot::{PaneChatSnapshot, PaneChatSubscriptionFreshness};
use kanvibe_process::exec::ShellCommandRunner;
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_services::kanban::KanbanService;
use kanvibe_services::remote_image_paste::RemoteImagePaste;
use kanvibe_terminal::pane_chat::attachments::{
    PaneChatAttachment, PaneChatAttachmentInput, PaneChatAttachmentStore, RemoteAttachmentUploader,
};
use kanvibe_terminal::pane_chat::composer_catalog::PaneChatSuggestionKind;
use kanvibe_terminal::pane_chat::connection_signals::{
    PaneChatConnectionListener, PaneChatConnectionScope, PaneChatConnectionSignals,
    PaneChatConnectionSubscription,
};
use kanvibe_terminal::pane_chat::mode_store::{PaneChatModeStore, PaneChatModes};
use kanvibe_terminal::pane_chat::service::{
    PaneChatClock, PaneChatError, PaneChatService, PaneChatServiceDependencies,
    PaneChatSessionRecords, PaneChatSuggestionSource, PaneChatSuggestions,
};
use kanvibe_terminal::pane_chat::subscription::{
    PaneChatPushSender, PaneChatSubscriberOwner, PaneChatSubscriptionRequest,
    PaneChatSubscriptions, PaneChatTimers, TranscriptFileWatchers,
};
use kanvibe_terminal::pane_chat::surface::{
    PaneChatHerdr, PaneChatSurfaceReader, PaneChatTask, PaneChatTaskLookup,
};
use kanvibe_terminal::terminal_registry::TerminalRegistry;
use serde_json::Value;

use crate::mirror::PaneMirror;

/// pane chat 런타임이 쓰는 협력자
pub struct PaneChatRuntimeParts {
    /// pane에 입력을 쓰고 Herdr 화면을 읽는 미러
    pub mirror: Arc<PaneMirror>,
    pub tasks: Arc<dyn PaneChatTaskLookup>,
    pub runner: Arc<dyn ShellCommandRunner>,
    pub terminals: Arc<TerminalRegistry>,
    pub herdr: Arc<dyn PaneChatHerdr>,
    pub sessions: Arc<dyn PaneChatSessionRecords>,
    pub suggestions: Arc<dyn PaneChatSuggestionSource>,
    pub attachments: Arc<PaneChatAttachmentStore>,
    pub modes: Arc<PaneChatModeStore>,
    pub clock: Arc<dyn PaneChatClock>,
    pub timers: Arc<dyn PaneChatTimers>,
    pub file_watchers: Arc<dyn TranscriptFileWatchers>,
    /// CLI가 새 대화 ID를 알린 순간(hook 바인딩, 세션 기록 파일)의 알림
    pub connection_signals: Arc<PaneChatConnectionSignals>,
}

pub struct PaneChatRuntime {
    service: Arc<PaneChatService>,
    subscriptions: PaneChatSubscriptions,
    modes: Arc<PaneChatModeStore>,
    tasks: Arc<dyn PaneChatTaskLookup>,
    connection_signals: Arc<PaneChatConnectionSignals>,
}

fn input_error(message: &str) -> PaneChatError {
    PaneChatError::Input(message.to_string())
}

impl PaneChatRuntime {
    pub fn new(parts: PaneChatRuntimeParts) -> Self {
        let mobile_bridge = parts.mirror;
        let tasks = parts.tasks;
        let surfaces = Arc::new(PaneChatSurfaceReader::new(
            Arc::clone(&tasks),
            Arc::clone(&parts.runner),
            parts.terminals.clone(),
            parts.herdr,
            mobile_bridge.clone(),
        ));
        let service = Arc::new(PaneChatService::new(PaneChatServiceDependencies {
            surfaces,
            runner: parts.runner,
            terminals: parts.terminals,
            mobile_bridge,
            sessions: parts.sessions,
            suggestions: parts.suggestions,
            attachments: parts.attachments,
            clock: parts.clock,
        }));
        let subscriptions =
            PaneChatSubscriptions::new(service.clone(), parts.timers, parts.file_watchers);
        Self {
            service,
            subscriptions,
            modes: parts.modes,
            tasks,
            connection_signals: parts.connection_signals,
        }
    }

    /// 그 Task의 pane CLI가 새 대화 ID를 알리면 `listener`를 부른다. 로컬 Task는 세션 기록 파일도 감시하고,
    /// 원격 Task와 조회하지 못한 Task는 hook 신호만 받는다. 돌려받은 값을 버리면 멈춘다
    pub fn watch_pane_chat_connection(
        &self,
        task_id: &str,
        listener: PaneChatConnectionListener,
    ) -> PaneChatConnectionSubscription {
        let task = self.tasks.find_pane_chat_task(task_id).ok().flatten();
        let local_workspaces = task
            .filter(|task| task.ssh_host().is_none())
            .map(|task| {
                [task.worktree_path, task.project_repo_path]
                    .into_iter()
                    .flatten()
                    .filter(|path| !path.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let scope = PaneChatConnectionScope {
            task_id: task_id.to_string(),
            local_workspaces,
        };
        self.connection_signals.subscribe(scope, listener)
    }

    /// 데스크탑 화면도 같은 서비스(대상 lease, 입력 큐, 요청 영수증)를 써야 해서 함께 내보낸다
    pub fn service(&self) -> Arc<PaneChatService> {
        Arc::clone(&self.service)
    }

    /// `kind`는 질의 문자열 그대로, `limit`은 JS `Number(...)` 값이다
    pub fn get_pane_chat_suggestions(
        &self,
        task_id: &str,
        target_token: &str,
        kind: Option<&str>,
        query: &str,
        limit: Option<f64>,
    ) -> Result<PaneChatSuggestions, PaneChatError> {
        let kind = match kind {
            Some("skill") => PaneChatSuggestionKind::Skill,
            Some("command") => PaneChatSuggestionKind::Command,
            Some("file") => PaneChatSuggestionKind::File,
            _ => return Err(input_error("검색 종류·검색어가 올바르지 않습니다.")),
        };
        let limit = match limit {
            None => None,
            Some(limit) if limit.fract() == 0.0 && limit >= 1.0 => Some(limit as usize),
            Some(_) => return Err(input_error("제안 개수 상한이 올바르지 않습니다.")),
        };
        self.service
            .get_pane_chat_suggestions(task_id, target_token, kind, query, limit)
    }

    /// 에이전트 이름만 필요한 터미널 화면이 쓴다. 화면과 대화 기록은 읽지 않는다
    pub fn get_pane_chat_agent_provider(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Option<AiSessionProvider> {
        self.service.get_pane_chat_agent_provider(task_id, pane_id)
    }

    pub fn get_sequenced_pane_chat_snapshot(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> Result<PaneChatSnapshot, PaneChatError> {
        self.subscriptions.get_sequenced_pane_chat_snapshot(
            task_id,
            pane_id,
            Some(should_include_screen),
        )
    }

    pub fn stage_pane_chat_attachment(
        &self,
        task_id: &str,
        target_token: &str,
        attachment: &Value,
    ) -> Result<PaneChatAttachment, PaneChatError> {
        let input: PaneChatAttachmentInput = serde_json::from_value(attachment.clone())
            .map_err(|_| input_error("첨부 파일 이름·형식이 올바르지 않습니다."))?;
        self.service
            .stage_pane_chat_attachment(task_id, target_token, &input)
    }

    pub fn remove_pane_chat_attachment(
        &self,
        task_id: &str,
        target_token: &str,
        id: &Value,
    ) -> Result<(), PaneChatError> {
        let id = id
            .as_str()
            .ok_or_else(|| input_error("첨부 파일 식별자가 올바르지 않습니다."))?;
        self.service
            .remove_pane_chat_attachment(task_id, target_token, id)
    }

    /// 본문의 `text`, `attachmentIds`, `requestId`를 그대로 받는다. 빠진 `attachmentIds`는 빈 목록이다
    pub fn send_pane_chat_prompt(
        &self,
        task_id: &str,
        target_token: &str,
        text: &Value,
        attachment_ids: Option<&Value>,
        request_id: Option<&Value>,
    ) -> Result<(), PaneChatError> {
        let invalid = || input_error("메시지·첨부 파일·요청 식별자가 올바르지 않습니다.");
        let text = text.as_str().ok_or_else(invalid)?;
        let attachment_ids: Vec<String> = match attachment_ids {
            None => Vec::new(),
            Some(ids) => serde_json::from_value(ids.clone()).map_err(|_| invalid())?,
        };
        let request_id = match request_id {
            None => None,
            Some(id) => Some(id.as_str().ok_or_else(invalid)?),
        };
        self.service
            .send_pane_chat_prompt(task_id, target_token, text, &attachment_ids, request_id)
    }

    pub fn interrupt_pane_chat(
        &self,
        task_id: &str,
        target_token: &str,
    ) -> Result<(), PaneChatError> {
        self.service.interrupt_pane_chat(task_id, target_token)
    }

    pub fn open_pane_chat_model_picker(
        &self,
        task_id: &str,
        target_token: &str,
    ) -> Result<(), PaneChatError> {
        self.service
            .open_pane_chat_model_picker(task_id, target_token)
    }

    pub fn change_pane_chat_model_setting(
        &self,
        task_id: &str,
        target_token: &str,
        change: &Value,
    ) -> Result<(), PaneChatError> {
        self.service
            .change_pane_chat_model_setting(task_id, target_token, change)
    }

    pub fn send_pane_chat_key(
        &self,
        task_id: &str,
        target_token: &str,
        key: &Value,
    ) -> Result<(), PaneChatError> {
        let key = key
            .as_str()
            .ok_or_else(|| input_error("허용되지 않는 선택기 키입니다."))?;
        self.service.send_pane_chat_key(task_id, target_token, key)
    }

    /// 소켓 하나가 보려는 pane 전체를 통째로 바꾸고 각 pane의 대화 위치가 최신인지 돌려준다
    pub fn set_pane_chat_subscriptions(
        &self,
        owner: PaneChatSubscriberOwner,
        requests: &[PaneChatSubscriptionRequest],
        send: PaneChatPushSender,
    ) -> Vec<PaneChatSubscriptionFreshness> {
        self.subscriptions
            .set_pane_chat_subscriptions(owner, requests, send)
    }

    /// 닫힌 소켓의 구독이 남으면 아무도 받지 않는 pane을 계속 읽는다
    pub fn clear_pane_chat_subscriptions(&self, owner: PaneChatSubscriberOwner) {
        self.subscriptions.clear_pane_chat_subscriptions(owner);
    }

    pub fn get_pane_chat_modes(&self, task_id: &str) -> Result<PaneChatModes, PaneChatError> {
        self.modes
            .get_pane_chat_modes(task_id)
            .map_err(|error| PaneChatError::Other(error.to_string()))
    }

    pub fn set_pane_chat_mode(
        &self,
        task_id: &str,
        pane_id: &str,
        is_enabled: bool,
    ) -> Result<(), PaneChatError> {
        self.modes
            .set_pane_chat_mode(task_id, pane_id, is_enabled)
            .map_err(|error| PaneChatError::Other(error.to_string()))
    }
}

/// 보드 서비스의 태스크 조회로 pane chat 대상 태스크를 찾는다(TS `kanbanService.getTaskById`)
pub struct KanbanPaneChatTaskLookup {
    kanban: Arc<KanbanService>,
}

impl KanbanPaneChatTaskLookup {
    pub fn new(kanban: Arc<KanbanService>) -> Self {
        Self { kanban }
    }
}

impl PaneChatTaskLookup for KanbanPaneChatTaskLookup {
    fn find_pane_chat_task(&self, task_id: &str) -> Result<Option<PaneChatTask>, String> {
        let found = self
            .kanban
            .get_task_by_id(task_id)
            .map_err(|error| error.to_string())?;
        Ok(found.map(|found| PaneChatTask {
            id: found.task.id,
            session_type: found.task.session_type,
            session_name: found.task.session_name,
            ssh_host: found.task.ssh_host,
            worktree_path: found.task.worktree_path,
            project_repo_path: found.project.map(|project| project.repo_path),
        }))
    }
}

/// 원격 CLI가 읽을 첨부 파일을 scp로 올린다(TS `transferFileToRemoteHost`)
pub struct RemoteImagePasteUploader {
    paste: Arc<RemoteImagePaste>,
}

impl RemoteImagePasteUploader {
    pub fn new(paste: Arc<RemoteImagePaste>) -> Self {
        Self { paste }
    }
}

impl RemoteAttachmentUploader for RemoteImagePasteUploader {
    fn transfer_file_to_remote_host(
        &self,
        config: &SshHostConfig,
        content: &[u8],
        extension: &str,
    ) -> Result<String, String> {
        self.paste
            .transfer_file_to_remote_host(config, content, extension)
            .map_err(|error| error.message)
    }
}
