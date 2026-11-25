//! [`DesktopServices`]가 라우트 경계를 구현한다. 각 메서드는 서비스 하나를 부르고 결과를 TS JSON 모양으로 옮긴다.

use std::sync::Arc;

use kanvibe_ai::pane_chat::snapshot::PaneChatSubscriptionTarget;
use kanvibe_ai::pane_chat::timeline::PaneChatCursor;
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::AiMessageRole;
use kanvibe_mobile::bridge::{
    MobileAiHistoryOptions, MobileBridge as MobileBoardBridge, MobileTaskDraft,
};
use kanvibe_mobile::error::MobileResult;
use kanvibe_mobile::ports::TaskPaneAccess;
use kanvibe_pane_mirror::{PaneMirror, PaneViewportClaim, SurfaceQuery};
use kanvibe_terminal::pane_chat::subscription::{
    PaneChatSubscriberOwner, PaneChatSubscriptionRequest,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::DesktopServices;
use super::errors::{from_mirror, from_mobile, from_pane_chat, from_serde};
use crate::bridge::{
    AiHistoryQuery, AiLoginEvents, BridgeError, BridgeResult, CloseSink, DeviceSink, EventSink,
    FeatureCall, MobileBridge, MobileDeviceAccess, PaneChatAction, PaneChatSuggestionQuery,
    PaneViewport, RemotePaneRelay, RemotePaneTarget, SignalSink, Subscription, TextSink,
};

/// 네트워크로 나가는 값. DB 시각 문자열은 TS처럼 ISO로 바꾼다
fn to_json<T: Serialize>(value: T) -> BridgeResult<Value> {
    let mut value = serde_json::to_value(value).map_err(from_serde)?;
    super::dates::normalize_database_dates(&mut value);
    Ok(value)
}

fn to_object<T: Serialize>(value: T) -> BridgeResult<Map<String, Value>> {
    match to_json(value)? {
        Value::Object(fields) => Ok(fields),
        _ => Err(BridgeError::Internal("result is not an object".to_string())),
    }
}

fn mobile<T>(result: MobileResult<T>) -> BridgeResult<T> {
    result.map_err(from_mobile)
}

fn parse_provider(provider: &str) -> BridgeResult<AiSessionProvider> {
    AiSessionProvider::parse(provider)
        .ok_or_else(|| BridgeError::TaskInput("지원하지 않는 CLI입니다".to_string()))
}

fn history_options(query: &AiHistoryQuery) -> MobileAiHistoryOptions {
    let roles = query.roles.as_ref().map(|roles| {
        roles
            .iter()
            .filter_map(|role| {
                serde_json::from_value::<AiMessageRole>(Value::String(role.clone())).ok()
            })
            .collect()
    });
    MobileAiHistoryOptions {
        query: query.query.clone(),
        cursor: query.cursor.clone(),
        limit: query.limit as usize,
        roles,
    }
}

/// 원격 이미지 붙여넣기가 pane을 확인하고 경로를 입력하는 길(모바일 기능 계층의 포트)
pub struct MirrorPaneAccess(pub Arc<PaneMirror>);

impl TaskPaneAccess for MirrorPaneAccess {
    fn has_task_pane(&self, task_id: &str, pane_id: &str) -> MobileResult<bool> {
        let surfaces = self.0.get_task_surfaces(task_id, &SurfaceQuery::default());
        Ok(surfaces.is_ok_and(|surfaces| {
            surfaces
                .tabs
                .iter()
                .any(|tab| tab.panes.iter().any(|pane| pane.id == pane_id))
        }))
    }

    fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> MobileResult<()> {
        self.0
            .write_to_pane(task_id, pane_id, input)
            .map_err(|error| kanvibe_mobile::error::MobileError::Internal(error.to_string()))
    }
}

impl MobileDeviceAccess for DesktopServices {
    fn authorize_mobile_request(
        &self,
        authorization: Option<&str>,
    ) -> BridgeResult<Option<String>> {
        mobile(self.devices.authorize_mobile_request(authorization))
    }

    fn start_mobile_pairing(&self) -> BridgeResult<Value> {
        to_json(self.devices.start_mobile_pairing())
    }

    fn pair_mobile_device(&self, code: &str, device_name: &str) -> BridgeResult<Option<String>> {
        mobile(self.devices.pair_mobile_device(code, device_name))
    }

    fn unpair_mobile_device_by_token(&self, authorization: Option<&str>) -> BridgeResult<bool> {
        mobile(self.devices.unpair_mobile_device_by_token(authorization))
    }

    fn update_mobile_notification_subscription(
        &self,
        authorization: Option<&str>,
        subscription: Value,
    ) -> BridgeResult<bool> {
        mobile(
            self.devices
                .update_mobile_notification_subscription(authorization, &subscription),
        )
    }

    fn delete_mobile_notification_subscription(
        &self,
        authorization: Option<&str>,
    ) -> BridgeResult<bool> {
        mobile(
            self.devices
                .delete_mobile_notification_subscription(authorization),
        )
    }

    /// 기기를 끊으면 그 기기의 원격 직접 연결 권한도 거둔다(TS `revokeRemoteTerminalDevice`)
    fn on_mobile_device_unpaired(&self, listener: DeviceSink) -> Subscription {
        let mirror = self.mirror.clone();
        let handle = self
            .devices
            .on_mobile_device_unpaired(Arc::new(move |device_id: &str| {
                mirror.handle_mobile_device_unpaired(device_id);
                listener(device_id.to_string());
            }));
        Subscription::new(move || drop(handle))
    }
}

impl MobileBridge for DesktopServices {
    fn get_mobile_board(&self, is_deduplicated: bool) -> BridgeResult<Value> {
        to_json(mobile(self.board.get_mobile_board(is_deduplicated))?)
    }

    fn get_mobile_directories(&self, path: &str) -> BridgeResult<Value> {
        to_json(mobile(self.board.get_mobile_directories(path))?)
    }

    fn get_mobile_remote_directories(&self, path: &str, ssh_host: &str) -> BridgeResult<Value> {
        to_json(mobile(self.features.get_mobile_remote_directories(
            path,
            &Value::String(ssh_host.to_string()),
        ))?)
    }

    fn scan_mobile_projects(&self, root_path: &str) -> BridgeResult<Value> {
        to_json(mobile(self.board.scan_mobile_projects(root_path))?)
    }

    fn scan_mobile_remote_projects(
        &self,
        root_path: &str,
        ssh_host: &Value,
    ) -> BridgeResult<Value> {
        to_json(mobile(
            self.features
                .scan_mobile_remote_projects(root_path, ssh_host),
        )?)
    }

    fn get_mobile_notifications(&self) -> BridgeResult<Value> {
        to_json(mobile(self.board.get_mobile_notifications())?)
    }

    fn get_mobile_notification_settings(&self) -> BridgeResult<Value> {
        to_json(mobile(self.board.get_mobile_notification_settings())?)
    }

    fn update_mobile_notification_settings(&self, body: &Value) -> BridgeResult<Value> {
        to_json(mobile(
            self.board.update_mobile_notification_settings(body),
        )?)
    }

    fn mark_all_mobile_notifications_read(&self) -> BridgeResult<()> {
        mobile(self.board.mark_all_mobile_notifications_read())
    }

    fn mark_mobile_notification_read(&self, notification_id: &str) -> BridgeResult<Option<Value>> {
        mobile(self.board.mark_mobile_notification_read(notification_id))?
            .map(to_json)
            .transpose()
    }

    fn on_mobile_notifications_changed(&self, listener: SignalSink) -> Subscription {
        let handle = self.board.on_mobile_notifications_changed(listener);
        Subscription::new(move || drop(handle))
    }

    fn subscribe_to_mobile_events(&self, listener: EventSink) -> Subscription {
        let handle = self
            .notifications
            .subscribe_to_mobile_events(Arc::new(move |event| {
                if let Ok(payload) = serde_json::to_value(event) {
                    listener(payload);
                }
            }));
        Subscription::new(move || drop(handle))
    }

    fn is_mobile_ai_session_provider(&self, provider: &str) -> bool {
        MobileBoardBridge::is_mobile_ai_session_provider(provider)
    }

    fn get_mobile_ai_session_attachment(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
        attachment_id: &str,
        source_ref: Option<&str>,
    ) -> BridgeResult<Option<Value>> {
        let provider = parse_provider(provider)?;
        let attachment = self.board.get_mobile_ai_session_attachment(
            task_id,
            provider,
            session_id,
            attachment_id,
            source_ref,
        );
        mobile(attachment)?.map(to_json).transpose()
    }

    fn get_mobile_task_file(&self, task_id: &str, file_path: &str) -> BridgeResult<Value> {
        to_json(mobile(self.board.get_mobile_task_file(task_id, file_path))?)
    }

    fn get_mobile_task(&self, task_id: &str) -> BridgeResult<Option<Value>> {
        mobile(self.board.get_mobile_task(task_id))?
            .map(to_json)
            .transpose()
    }

    fn get_mobile_ai_sessions(&self, task_id: &str, query: &AiHistoryQuery) -> BridgeResult<Value> {
        to_json(mobile(
            self.board
                .get_mobile_ai_sessions(task_id, &history_options(query)),
        )?)
    }

    fn get_mobile_ai_session_detail(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
        query: &AiHistoryQuery,
    ) -> BridgeResult<Option<Value>> {
        let provider = parse_provider(provider)?;
        mobile(self.board.get_mobile_ai_session_detail(
            task_id,
            provider,
            session_id,
            &history_options(query),
        ))?
        .map(to_json)
        .transpose()
    }

    fn create_mobile_task(&self, draft: &Value) -> BridgeResult<Value> {
        let text = |name: &str| draft.get(name).and_then(Value::as_str).map(str::to_string);
        let draft = MobileTaskDraft {
            project_id: text("projectId").unwrap_or_default(),
            branch_name: text("branchName").unwrap_or_default(),
            base_branch: text("baseBranch"),
            description: text("description"),
            priority: text("priority"),
            session_type: draft.get("sessionType").cloned(),
        };
        to_json(mobile(self.board.create_mobile_task(&draft))?)
    }

    fn update_mobile_task_status(
        &self,
        task_id: &str,
        status: &str,
    ) -> BridgeResult<Option<Value>> {
        mobile(self.board.update_mobile_task_status(task_id, status))?
            .map(to_json)
            .transpose()
    }

    fn change_mobile_task_session_type(
        &self,
        task_id: &str,
        session_type: &str,
        should_close_existing_session: bool,
    ) -> BridgeResult<Option<Value>> {
        let changed = self.board.change_mobile_task_session_type(
            task_id,
            session_type,
            should_close_existing_session,
        );
        mobile(changed)?.map(to_json).transpose()
    }

    fn is_mobile_task_session_alive(&self, task_id: &str) -> BridgeResult<bool> {
        mobile(self.board.is_mobile_task_session_alive(task_id))
    }

    fn get_mobile_project_branches(&self, project_id: &str) -> BridgeResult<Value> {
        to_json(mobile(self.board.get_mobile_project_branches(project_id))?)
    }

    fn run_mobile_feature(&self, call: FeatureCall) -> BridgeResult<Map<String, Value>> {
        self.run_feature_call(call)
    }

    fn render_mobile_diagram(&self, body: &Value) -> BridgeResult<Map<String, Value>> {
        self.diagram.render(body)
    }

    fn get_mobile_running_agent_panes(&self) -> BridgeResult<Value> {
        to_json(self.ai.get_mobile_running_agent_panes())
    }

    fn get_mobile_live_ai_sessions(&self, task_id: &str) -> BridgeResult<Value> {
        to_json(mobile(self.ai.get_mobile_live_ai_sessions(task_id))?)
    }

    fn get_mobile_agent_call_graph(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
    ) -> BridgeResult<Value> {
        to_json(mobile(
            self.ai
                .get_mobile_agent_call_graph(task_id, provider, session_id),
        )?)
    }

    fn list_mobile_ai_accounts(&self) -> BridgeResult<Value> {
        to_json(mobile(self.ai.list_mobile_ai_accounts())?)
    }

    fn add_mobile_ai_account(&self, body: &Value) -> BridgeResult<Map<String, Value>> {
        to_object(mobile(self.ai.add_mobile_ai_account(body))?)
    }

    fn remove_mobile_ai_account(&self, account_id: &str) -> BridgeResult<Map<String, Value>> {
        to_object(mobile(self.ai.remove_mobile_ai_account(account_id))?)
    }

    fn get_mobile_ai_usage_snapshot(&self) -> BridgeResult<Value> {
        to_json(mobile(self.ai.get_mobile_ai_usage_snapshot())?)
    }

    fn open_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        account_id: &str,
        size: (u16, u16),
        events: Arc<dyn AiLoginEvents>,
    ) -> BridgeResult<()> {
        let socket = Arc::new(LoginSocketEvents(events));
        mobile(
            self.ai
                .open_mobile_ai_login(device_id, socket_id, account_id, size, socket),
        )
    }

    fn write_mobile_ai_login(&self, device_id: &str, socket_id: &str, data: &str) {
        let _ = self.ai.write_mobile_ai_login(device_id, socket_id, data);
    }

    fn resize_mobile_ai_login(&self, device_id: &str, socket_id: &str, cols: u16, rows: u16) {
        let _ = self
            .ai
            .resize_mobile_ai_login(device_id, socket_id, (cols, rows));
    }

    fn close_mobile_ai_login(&self, device_id: &str, socket_id: &str) {
        self.ai.close_mobile_ai_login(device_id, socket_id);
    }

    fn close_mobile_ai_logins_for_device(&self, device_id: &str) {
        self.ai.close_mobile_ai_logins_for_device(device_id);
    }

    fn close_all_mobile_ai_logins(&self) {
        self.ai.close_all_mobile_ai_logins();
    }

    fn get_pane_chat_suggestions(
        &self,
        task_id: &str,
        query: &PaneChatSuggestionQuery,
    ) -> BridgeResult<Map<String, Value>> {
        let suggestions = self.pane_chat.get_pane_chat_suggestions(
            task_id,
            &query.target_token,
            query.kind.as_deref(),
            &query.query,
            query.limit,
        );
        to_object(suggestions.map_err(from_pane_chat)?)
    }

    fn get_pane_chat_agent_provider(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value> {
        to_json(
            self.pane_chat
                .get_pane_chat_agent_provider(task_id, pane_id),
        )
    }

    fn get_sequenced_pane_chat_snapshot(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> BridgeResult<Value> {
        let snapshot = self.pane_chat.get_sequenced_pane_chat_snapshot(
            task_id,
            pane_id,
            should_include_screen,
        );
        to_json(snapshot.map_err(from_pane_chat)?)
    }

    fn run_pane_chat_action(
        &self,
        task_id: &str,
        target_token: &str,
        action: PaneChatAction,
    ) -> BridgeResult<Value> {
        self.run_pane_chat_action_call(task_id, target_token, action)
    }

    fn set_pane_chat_subscriptions(
        &self,
        owner: u64,
        targets: &Value,
        send: EventSink,
    ) -> BridgeResult<Value> {
        let requests = read_subscription_requests(targets)?;
        let push = Arc::new(move |event| {
            if let Ok(payload) = serde_json::to_value(&event) {
                send(payload);
            }
        });
        to_json(self.pane_chat.set_pane_chat_subscriptions(
            PaneChatSubscriberOwner(owner),
            &requests,
            push,
        ))
    }

    fn clear_pane_chat_subscriptions(&self, owner: u64) {
        self.pane_chat
            .clear_pane_chat_subscriptions(PaneChatSubscriberOwner(owner));
    }

    fn get_pane_chat_modes(&self, task_id: &str) -> BridgeResult<Value> {
        to_json(
            self.pane_chat
                .get_pane_chat_modes(task_id)
                .map_err(from_pane_chat)?,
        )
    }

    fn set_pane_chat_mode(
        &self,
        task_id: &str,
        pane_id: &str,
        is_enabled: bool,
    ) -> BridgeResult<()> {
        self.pane_chat
            .set_pane_chat_mode(task_id, pane_id, is_enabled)
            .map_err(from_pane_chat)
    }

    fn prepare_mobile_pane_connection(
        &self,
        task_id: &str,
        pane_id: &str,
        device_id: &str,
    ) -> BridgeResult<Value> {
        to_json(
            self.mirror
                .prepare_mobile_pane_connection(task_id, pane_id, device_id)
                .map_err(from_mirror)?,
        )
    }

    fn get_task_surfaces(
        &self,
        task_id: &str,
        minimal: Option<Option<String>>,
    ) -> BridgeResult<Value> {
        let query = SurfaceQuery {
            is_minimal: minimal.is_some(),
            tab_id: minimal.flatten(),
        };
        to_json(
            self.mirror
                .get_task_surfaces(task_id, &query)
                .map_err(from_mirror)?,
        )
    }

    fn execute_mobile_terminal_shortcut(
        &self,
        task_id: &str,
        pane_id: &str,
        action: &str,
    ) -> BridgeResult<Map<String, Value>> {
        to_object(
            self.mirror
                .execute_mobile_terminal_shortcut(task_id, pane_id, action)
                .map_err(from_mirror)?,
        )
    }

    fn create_mobile_terminal_tab(&self, task_id: &str) -> BridgeResult<Map<String, Value>> {
        to_object(
            self.mirror
                .create_mobile_terminal_tab(task_id)
                .map_err(from_mirror)?,
        )
    }

    fn get_mobile_pane_close_info(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value> {
        to_json(
            self.mirror
                .get_mobile_pane_close_info(task_id, pane_id)
                .map_err(from_mirror)?,
        )
    }

    fn close_mobile_pane(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value> {
        let closed = self
            .mirror
            .close_mobile_pane(task_id, pane_id)
            .map_err(from_mirror)?;
        Ok(to_json(closed)?
            .get("surfaces")
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn delete_mobile_task(&self, task_id: &str, is_forced: bool) -> BridgeResult<Option<u64>> {
        let deletion = mobile(self.board.delete_mobile_task(task_id, is_forced))?;
        deletion
            .map(|deletion| {
                Ok(to_json(deletion)?
                    .get("failedPeerCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0))
            })
            .transpose()
    }

    fn remove_mobile_task_registration(&self, task_id: &str) -> BridgeResult<bool> {
        mobile(self.board.remove_mobile_task_registration(task_id))
    }

    fn watch_task_session_change(&self, task_id: &str, on_changed: SignalSink) -> Subscription {
        let watch = self.board.watch_task_session_change(task_id, on_changed);
        Subscription::new(move || drop(watch))
    }

    fn subscribe_to_pane(
        &self,
        task_id: &str,
        pane_id: &str,
        on_chunk: TextSink,
        on_end: SignalSink,
    ) -> BridgeResult<Subscription> {
        let subscription = self
            .mirror
            .subscribe_to_pane(task_id, pane_id, on_chunk, on_end)
            .map_err(from_mirror)?;
        Ok(Subscription::new(move || subscription.stop()))
    }

    fn claim_pane_viewport(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> BridgeResult<Box<dyn PaneViewport>> {
        let claim = self
            .mirror
            .claim_pane_viewport(task_id, pane_id)
            .map_err(from_mirror)?;
        Ok(Box::new(MirrorViewport(claim)))
    }

    fn refresh_pane_history(&self, task_id: &str, pane_id: &str) -> BridgeResult<()> {
        self.mirror.refresh_pane_history(task_id, pane_id);
        Ok(())
    }

    fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> BridgeResult<()> {
        self.mirror
            .write_to_pane(task_id, pane_id, input)
            .map_err(from_mirror)
    }

    fn open_remote_mobile_pane_stream(
        &self,
        target: RemotePaneTarget,
        on_chunk: TextSink,
        on_close: CloseSink,
    ) -> BridgeResult<Option<Box<dyn RemotePaneRelay>>> {
        let relay = self
            .mirror
            .open_remote_mobile_pane_stream(
                &target.task_id,
                &target.pane_id,
                &target.device_id,
                target.grant_id.as_deref(),
                on_chunk,
                on_close,
            )
            .map_err(from_mirror)?;
        Ok(relay.map(|relay| Box::new(MirrorRelay(relay)) as Box<dyn RemotePaneRelay>))
    }
}

impl DesktopServices {
    fn run_pane_chat_action_call(
        &self,
        task_id: &str,
        target_token: &str,
        action: PaneChatAction,
    ) -> BridgeResult<Value> {
        let chat = &self.pane_chat;
        match action {
            PaneChatAction::StageAttachment { attachment } => {
                let staged = chat
                    .stage_pane_chat_attachment(task_id, target_token, &attachment)
                    .map_err(from_pane_chat)?;
                Ok(json!({ "success": true, "attachment": to_json(staged)? }))
            }
            PaneChatAction::RemoveAttachment { id } => to_json(
                chat.remove_pane_chat_attachment(task_id, target_token, &id)
                    .map_err(from_pane_chat)?,
            ),
            PaneChatAction::Prompt { text, extra } => {
                let (attachment_ids, request_id) =
                    extra.map_or((None, None), |(ids, request)| (Some(ids), Some(request)));
                let sent = chat.send_pane_chat_prompt(
                    task_id,
                    target_token,
                    &text,
                    attachment_ids.as_ref(),
                    request_id.as_ref(),
                );
                to_json(sent.map_err(from_pane_chat)?)
            }
            PaneChatAction::Interrupt => to_json(
                chat.interrupt_pane_chat(task_id, target_token)
                    .map_err(from_pane_chat)?,
            ),
            PaneChatAction::ModelPicker => to_json(
                chat.open_pane_chat_model_picker(task_id, target_token)
                    .map_err(from_pane_chat)?,
            ),
            PaneChatAction::ModelSetting { change } => to_json(
                chat.change_pane_chat_model_setting(task_id, target_token, &change)
                    .map_err(from_pane_chat)?,
            ),
            PaneChatAction::Key { key } => to_json(
                chat.send_pane_chat_key(task_id, target_token, &key)
                    .map_err(from_pane_chat)?,
            ),
        }
    }
}

/// 명령 계층이 정규화한 `[{taskId, paneId, detail, wantsScreen, cursor}]`
fn read_subscription_requests(targets: &Value) -> BridgeResult<Vec<PaneChatSubscriptionRequest>> {
    let Value::Array(targets) = targets else {
        return Ok(Vec::new());
    };
    targets
        .iter()
        .map(|target| {
            let cursor: Option<PaneChatCursor> =
                serde_json::from_value(target.get("cursor").cloned().unwrap_or(Value::Null))
                    .map_err(from_serde)?;
            let target: PaneChatSubscriptionTarget =
                serde_json::from_value(target.clone()).map_err(from_serde)?;
            Ok(PaneChatSubscriptionRequest { target, cursor })
        })
        .collect()
}

struct LoginSocketEvents(Arc<dyn AiLoginEvents>);

impl kanvibe_ai::accounts::mobile_login_bridge::MobileLoginSocket for LoginSocketEvents {
    fn is_open(&self) -> bool {
        self.0.is_open()
    }

    fn send_event(&self, event: kanvibe_ai::accounts::mobile_login_bridge::MobileAiLoginEvent) {
        if let Ok(payload) = serde_json::to_value(&event) {
            self.0.on_event(payload);
        }
    }
}

struct MirrorViewport(PaneViewportClaim);

impl PaneViewport for MirrorViewport {
    fn resize(&self, cols: u16, rows: u16) -> BridgeResult<Option<String>> {
        self.0
            .resize(i64::from(cols), i64::from(rows))
            .map_err(from_mirror)
    }

    fn scroll(&self, request: &Value) -> BridgeResult<Option<String>> {
        self.0.scroll(request).map_err(from_mirror)
    }

    fn release(&self) -> BridgeResult<()> {
        self.0.release().map_err(from_mirror)
    }
}

struct MirrorRelay(Box<dyn kanvibe_pane_mirror::RemotePaneRelay>);

impl RemotePaneRelay for MirrorRelay {
    fn write(&self, data: &[u8], is_binary: bool) {
        self.0.write(data, is_binary);
    }

    fn close(&self) {
        self.0.close();
    }
}
