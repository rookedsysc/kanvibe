//! 가짜 서비스의 `MobileBridge` 구현. 메서드 이름이 곧 기록 이름이다.

use std::sync::Arc;

use kanvibe_server::bridge::{
    AiHistoryQuery, AiLoginEvents, BridgeResult, CloseSink, EventSink, FeatureCall, MobileBridge,
    PaneChatAction, PaneChatSuggestionQuery, PaneViewport, RemotePaneRelay, RemotePaneTarget,
    SignalSink, Subscription, TextSink,
};
use serde_json::{Map, Value, json};

use super::fake_bridge::{
    FakeBridge, FakeRelay, FakeViewport, history_query, object, optional, remote_target,
    suggestion_args, truthy, variant_name,
};
use super::recorder::lock;

/// TS 호출 모양: 기본은 `("task-1")`, 최소 요청은 `("task-1", {metadata, tabId?})`
pub fn surfaces_args(task_id: &str, minimal: &Option<Option<String>>) -> Value {
    match minimal {
        None => json!([task_id]),
        Some(None) => json!([task_id, { "metadata": "minimal" }]),
        Some(Some(tab_id)) => json!([task_id, { "metadata": "minimal", "tabId": tab_id }]),
    }
}

impl MobileBridge for FakeBridge {
    fn get_mobile_board(&self, is_deduplicated: bool) -> BridgeResult<Value> {
        self.call("get_mobile_board", json!(is_deduplicated))
    }

    fn get_mobile_directories(&self, path: &str) -> BridgeResult<Value> {
        self.call("get_mobile_directories", json!(path))
    }

    fn get_mobile_remote_directories(&self, path: &str, ssh_host: &str) -> BridgeResult<Value> {
        self.call("get_mobile_remote_directories", json!([path, ssh_host]))
    }

    fn scan_mobile_projects(&self, root_path: &str) -> BridgeResult<Value> {
        self.call("scan_mobile_projects", json!(root_path))
    }

    fn scan_mobile_remote_projects(
        &self,
        root_path: &str,
        ssh_host: &Value,
    ) -> BridgeResult<Value> {
        self.call("scan_mobile_remote_projects", json!([root_path, ssh_host]))
    }

    fn get_mobile_notifications(&self) -> BridgeResult<Value> {
        self.call("get_mobile_notifications", Value::Null)
    }

    fn get_mobile_notification_settings(&self) -> BridgeResult<Value> {
        self.call("get_mobile_notification_settings", Value::Null)
    }

    fn update_mobile_notification_settings(&self, body: &Value) -> BridgeResult<Value> {
        self.call("update_mobile_notification_settings", body.clone())
    }

    fn mark_all_mobile_notifications_read(&self) -> BridgeResult<()> {
        self.call("mark_all_mobile_notifications_read", Value::Null)
            .map(drop)
    }

    fn mark_mobile_notification_read(&self, notification_id: &str) -> BridgeResult<Option<Value>> {
        self.call("mark_mobile_notification_read", json!(notification_id))
            .map(optional)
    }

    fn on_mobile_notifications_changed(&self, listener: SignalSink) -> Subscription {
        self.notification_listeners.add(listener)
    }

    fn subscribe_to_mobile_events(&self, listener: EventSink) -> Subscription {
        self.event_listeners.add(listener)
    }

    fn is_mobile_ai_session_provider(&self, provider: &str) -> bool {
        self.call("is_mobile_ai_session_provider", json!(provider))
            .is_ok_and(truthy)
    }

    fn get_mobile_ai_session_attachment(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
        attachment_id: &str,
        source_ref: Option<&str>,
    ) -> BridgeResult<Option<Value>> {
        let args = json!([task_id, provider, session_id, attachment_id, source_ref]);
        self.call("get_mobile_ai_session_attachment", args)
            .map(optional)
    }

    fn get_mobile_task_file(&self, task_id: &str, file_path: &str) -> BridgeResult<Value> {
        self.call("get_mobile_task_file", json!([task_id, file_path]))
    }

    fn get_mobile_task(&self, task_id: &str) -> BridgeResult<Option<Value>> {
        self.call("get_mobile_task", json!(task_id)).map(optional)
    }

    fn get_mobile_ai_sessions(&self, task_id: &str, query: &AiHistoryQuery) -> BridgeResult<Value> {
        self.call(
            "get_mobile_ai_sessions",
            json!([task_id, history_query(query)]),
        )
    }

    fn get_mobile_ai_session_detail(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
        query: &AiHistoryQuery,
    ) -> BridgeResult<Option<Value>> {
        let args = json!([task_id, provider, session_id, history_query(query)]);
        self.call("get_mobile_ai_session_detail", args)
            .map(optional)
    }

    fn create_mobile_task(&self, draft: &Value) -> BridgeResult<Value> {
        self.call("create_mobile_task", draft.clone())
    }

    fn update_mobile_task_status(
        &self,
        task_id: &str,
        status: &str,
    ) -> BridgeResult<Option<Value>> {
        self.call("update_mobile_task_status", json!([task_id, status]))
            .map(optional)
    }

    fn change_mobile_task_session_type(
        &self,
        task_id: &str,
        session_type: &str,
        should_close_existing_session: bool,
    ) -> BridgeResult<Option<Value>> {
        let args = json!([task_id, session_type, should_close_existing_session]);
        self.call("change_mobile_task_session_type", args)
            .map(optional)
    }

    fn is_mobile_task_session_alive(&self, task_id: &str) -> BridgeResult<bool> {
        self.call("is_mobile_task_session_alive", json!(task_id))
            .map(truthy)
    }

    fn get_mobile_project_branches(&self, project_id: &str) -> BridgeResult<Value> {
        self.call("get_mobile_project_branches", json!(project_id))
    }

    fn run_mobile_feature(&self, call: FeatureCall) -> BridgeResult<Map<String, Value>> {
        let debug = format!("{call:?}");
        lock(&self.feature_calls).push(call);
        self.call(&format!("feature.{}", variant_name(&debug)), json!(debug))
            .map(object)
    }

    fn render_mobile_diagram(&self, body: &Value) -> BridgeResult<Map<String, Value>> {
        self.call("render_mobile_diagram", body.clone()).map(object)
    }

    fn get_mobile_running_agent_panes(&self) -> BridgeResult<Value> {
        self.call("get_mobile_running_agent_panes", Value::Null)
    }

    fn get_mobile_live_ai_sessions(&self, task_id: &str) -> BridgeResult<Value> {
        self.call("get_mobile_live_ai_sessions", json!(task_id))
    }

    fn get_mobile_agent_call_graph(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
    ) -> BridgeResult<Value> {
        self.call(
            "get_mobile_agent_call_graph",
            json!([task_id, provider, session_id]),
        )
    }

    fn list_mobile_ai_accounts(&self) -> BridgeResult<Value> {
        self.call("list_mobile_ai_accounts", Value::Null)
    }

    fn add_mobile_ai_account(&self, body: &Value) -> BridgeResult<Map<String, Value>> {
        self.call("add_mobile_ai_account", body.clone()).map(object)
    }

    fn remove_mobile_ai_account(&self, account_id: &str) -> BridgeResult<Map<String, Value>> {
        self.call("remove_mobile_ai_account", json!(account_id))
            .map(object)
    }

    fn get_mobile_ai_usage_snapshot(&self) -> BridgeResult<Value> {
        self.call("get_mobile_ai_usage_snapshot", Value::Null)
    }

    fn open_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        account_id: &str,
        size: (u16, u16),
        events: Arc<dyn AiLoginEvents>,
    ) -> BridgeResult<()> {
        lock(&self.ai_login_events).push(events);
        let args = json!([device_id, socket_id, account_id, size.0, size.1]);
        self.call("open_mobile_ai_login", args).map(drop)
    }

    /// 응답기를 걸면 쓰기 도중을 붙잡아 순서를 볼 수 있다
    fn write_mobile_ai_login(&self, device_id: &str, socket_id: &str, data: &str) {
        let args = json!([device_id, socket_id, data]);
        drop(self.call("write_mobile_ai_login", args));
    }

    fn resize_mobile_ai_login(&self, device_id: &str, socket_id: &str, cols: u16, rows: u16) {
        self.recorder.record(
            "resize_mobile_ai_login",
            json!([device_id, socket_id, cols, rows]),
        );
    }

    fn close_mobile_ai_login(&self, device_id: &str, socket_id: &str) {
        self.recorder
            .record("close_mobile_ai_login", json!([device_id, socket_id]));
    }

    fn close_mobile_ai_logins_for_device(&self, device_id: &str) {
        self.recorder
            .record("close_mobile_ai_logins_for_device", json!(device_id));
    }

    fn close_all_mobile_ai_logins(&self) {
        self.recorder
            .record("close_all_mobile_ai_logins", Value::Null);
    }

    fn get_pane_chat_suggestions(
        &self,
        task_id: &str,
        query: &PaneChatSuggestionQuery,
    ) -> BridgeResult<Map<String, Value>> {
        self.call("get_pane_chat_suggestions", suggestion_args(task_id, query))
            .map(object)
    }

    fn get_pane_chat_agent_provider(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value> {
        self.call("get_pane_chat_agent_provider", json!([task_id, pane_id]))
    }

    fn get_sequenced_pane_chat_snapshot(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> BridgeResult<Value> {
        self.call(
            "get_sequenced_pane_chat_snapshot",
            json!([task_id, pane_id, should_include_screen]),
        )
    }

    fn run_pane_chat_action(
        &self,
        task_id: &str,
        target_token: &str,
        action: PaneChatAction,
    ) -> BridgeResult<Value> {
        let name = format!("pane_chat.{}", variant_name(&format!("{action:?}")));
        lock(&self.pane_chat_actions).push((task_id.to_string(), target_token.to_string(), action));
        self.call(&name, json!([task_id, target_token]))
    }

    fn set_pane_chat_subscriptions(
        &self,
        owner: u64,
        targets: &Value,
        send: EventSink,
    ) -> BridgeResult<Value> {
        lock(&self.pane_chat_senders).push(send);
        self.call("set_pane_chat_subscriptions", json!([owner, targets]))
    }

    fn clear_pane_chat_subscriptions(&self, owner: u64) {
        self.recorder
            .record("clear_pane_chat_subscriptions", json!(owner));
    }

    fn get_pane_chat_modes(&self, task_id: &str) -> BridgeResult<Value> {
        self.call("get_pane_chat_modes", json!(task_id))
    }

    fn set_pane_chat_mode(
        &self,
        task_id: &str,
        pane_id: &str,
        is_enabled: bool,
    ) -> BridgeResult<()> {
        self.call("set_pane_chat_mode", json!([task_id, pane_id, is_enabled]))
            .map(drop)
    }

    fn prepare_mobile_pane_connection(
        &self,
        task_id: &str,
        pane_id: &str,
        device_id: &str,
    ) -> BridgeResult<Value> {
        self.call(
            "prepare_mobile_pane_connection",
            json!([task_id, pane_id, device_id]),
        )
    }

    fn get_task_surfaces(
        &self,
        task_id: &str,
        minimal: Option<Option<String>>,
    ) -> BridgeResult<Value> {
        self.call("get_task_surfaces", surfaces_args(task_id, &minimal))
    }

    fn execute_mobile_terminal_shortcut(
        &self,
        task_id: &str,
        pane_id: &str,
        action: &str,
    ) -> BridgeResult<Map<String, Value>> {
        self.call(
            "execute_mobile_terminal_shortcut",
            json!([task_id, pane_id, action]),
        )
        .map(object)
    }

    fn create_mobile_terminal_tab(&self, task_id: &str) -> BridgeResult<Map<String, Value>> {
        self.call("create_mobile_terminal_tab", json!(task_id))
            .map(object)
    }

    fn get_mobile_pane_close_info(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value> {
        self.call("get_mobile_pane_close_info", json!([task_id, pane_id]))
    }

    fn close_mobile_pane(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value> {
        self.call("close_mobile_pane", json!([task_id, pane_id]))
    }

    fn delete_mobile_task(&self, task_id: &str, is_forced: bool) -> BridgeResult<Option<u64>> {
        self.call("delete_mobile_task", json!([task_id, is_forced]))
            .map(|count| count.as_u64())
    }

    fn remove_mobile_task_registration(&self, task_id: &str) -> BridgeResult<bool> {
        self.call("remove_mobile_task_registration", json!(task_id))
            .map(truthy)
    }

    fn watch_task_session_change(&self, task_id: &str, on_changed: SignalSink) -> Subscription {
        self.recorder
            .record("watch_task_session_change", json!(task_id));
        self.session_watchers.add((task_id.to_string(), on_changed))
    }

    /// 구독을 멈추면 `pane_unsubscribe`가 남는다(TS의 `stop` 스파이)
    fn subscribe_to_pane(
        &self,
        task_id: &str,
        pane_id: &str,
        on_chunk: TextSink,
        on_end: SignalSink,
    ) -> BridgeResult<Subscription> {
        self.call("subscribe_to_pane", json!([task_id, pane_id]))?;
        let registration = self.pane_sinks.add((on_chunk, on_end));
        let recorder = self.recorder.clone();
        Ok(Subscription::new(move || {
            registration.stop();
            recorder.record("pane_unsubscribe", Value::Null);
        }))
    }

    fn claim_pane_viewport(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> BridgeResult<Box<dyn PaneViewport>> {
        self.call("claim_pane_viewport", json!([task_id, pane_id]))?;
        Ok(Box::new(FakeViewport {
            recorder: self.recorder.clone(),
        }))
    }

    fn refresh_pane_history(&self, task_id: &str, pane_id: &str) -> BridgeResult<()> {
        self.call("refresh_pane_history", json!([task_id, pane_id]))
            .map(drop)
    }

    fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> BridgeResult<()> {
        self.call("write_to_pane", json!([task_id, pane_id, input]))
            .map(drop)
    }

    /// 응답이 `null`이 아니면 중계가 열린 것으로 본다
    fn open_remote_mobile_pane_stream(
        &self,
        target: RemotePaneTarget,
        on_chunk: TextSink,
        on_close: CloseSink,
    ) -> BridgeResult<Option<Box<dyn RemotePaneRelay>>> {
        lock(&self.relay_sinks).push((on_chunk, on_close));
        let relay = self.call("open_remote_mobile_pane_stream", remote_target(&target))?;
        let relay: Option<Box<dyn RemotePaneRelay>> = optional(relay).map(|_| {
            Box::new(FakeRelay {
                recorder: self.recorder.clone(),
            }) as Box<dyn RemotePaneRelay>
        });
        Ok(relay)
    }
}
