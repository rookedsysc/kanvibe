//! `MobileBridge`·`MobileDeviceAccess`·`HookBackend`를 모두 흉내 내는 가짜 서비스.
//!
//! 메서드마다 [`Recorder`]에 이름과 인자를 남기고 응답을 받는다. 콜백을 받는 메서드는 그 콜백을 모아 두어
//! 테스트가 서비스 쪽 신호(기기 해제, pane 출력, 이벤트 방송 …)를 직접 보낼 수 있게 한다.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use kanvibe_server::bridge::{
    AiHistoryQuery, AiLoginEvents, BridgeError, BridgeResult, CloseSink, DeviceSink, EventSink,
    FeatureCall, MobileDeviceAccess, PaneChatAction, PaneChatSuggestionQuery, PaneViewport,
    RemotePaneRelay, RemotePaneTarget, SignalSink, Subscription, TextSink,
};
use kanvibe_server::routes::hooks::{HookBackend, TaskCliFailure, TaskCliRoute};
use serde_json::{Map, Value, json};

use super::recorder::{Listeners, Recorder, Responder, lock};

#[derive(Default)]
pub struct FakeBridge {
    pub recorder: Arc<Recorder>,
    pub feature_calls: Mutex<Vec<FeatureCall>>,
    pub pane_chat_actions: Mutex<Vec<(String, String, PaneChatAction)>>,
    pub unpaired_listeners: Listeners<DeviceSink>,
    pub notification_listeners: Listeners<SignalSink>,
    pub event_listeners: Listeners<EventSink>,
    pub session_watchers: Listeners<(String, SignalSink)>,
    pub pane_sinks: Listeners<(TextSink, SignalSink)>,
    pub pane_chat_senders: Mutex<Vec<EventSink>>,
    pub ai_login_events: Mutex<Vec<Arc<dyn AiLoginEvents>>>,
    pub relay_sinks: Mutex<Vec<(TextSink, CloseSink)>>,
}

impl FakeBridge {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// TS `mockImplementation`
    pub fn respond(
        &self,
        name: &str,
        responder: impl Fn(&Value) -> BridgeResult<Value> + Send + Sync + 'static,
    ) {
        let responder: Responder = Arc::new(responder);
        self.recorder.respond(name, responder);
    }

    /// TS `mockResolvedValue`
    pub fn returns(&self, name: &str, value: Value) {
        self.respond(name, move |_| Ok(value.clone()));
    }

    /// TS `mockRejectedValue`
    pub fn fails(&self, name: &str, error: BridgeError) {
        self.respond(name, move |_| Err(error.clone()));
    }

    pub fn calls(&self, name: &str) -> Vec<Value> {
        self.recorder.calls(name)
    }

    pub fn count(&self, name: &str) -> usize {
        self.calls(name).len()
    }

    pub fn feature_calls(&self) -> Vec<FeatureCall> {
        lock(&self.feature_calls).clone()
    }

    pub fn pane_chat_actions(&self) -> Vec<(String, String, PaneChatAction)> {
        lock(&self.pane_chat_actions).clone()
    }

    /// 서비스가 기기 해제를 알린다(`mobileBridgeService`의 두 해제 경로)
    pub fn notify_unpaired(&self, device_id: &str) {
        for listener in self.unpaired_listeners.snapshot() {
            listener(device_id.to_string());
        }
    }

    pub fn notify_task_session_changed(&self, task_id: &str) {
        for (watched, on_changed) in self.session_watchers.snapshot() {
            if watched == task_id {
                on_changed();
            }
        }
    }

    pub fn broadcast_event(&self, event: Value) {
        for listener in self.event_listeners.snapshot() {
            listener(event.clone());
        }
    }

    pub fn emit_pane_chunk(&self, chunk: &str) {
        for (on_chunk, _) in self.pane_sinks.snapshot() {
            on_chunk(chunk.to_string());
        }
    }

    pub fn end_pane_streams(&self) {
        for (_, on_end) in self.pane_sinks.snapshot() {
            on_end();
        }
    }

    pub fn ai_login(&self, index: usize) -> Arc<dyn AiLoginEvents> {
        lock(&self.ai_login_events)[index].clone()
    }

    pub(super) fn call(&self, name: &str, args: Value) -> BridgeResult<Value> {
        self.recorder.call(name, args)
    }
}

pub fn optional(value: Value) -> Option<Value> {
    (!value.is_null()).then_some(value)
}

pub(super) fn optional_text(value: Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

pub(super) fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}

pub(super) fn truthy(value: Value) -> bool {
    value.as_bool().unwrap_or(false)
}

/// `Prompt { .. }` → `Prompt`
pub(super) fn variant_name(debug: &str) -> String {
    debug
        .split(|character: char| !character.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_string()
}

pub(super) fn history_query(query: &AiHistoryQuery) -> Value {
    json!({ "query": query.query, "cursor": query.cursor, "limit": query.limit, "roles": query.roles })
}

impl MobileDeviceAccess for FakeBridge {
    fn authorize_mobile_request(
        &self,
        authorization: Option<&str>,
    ) -> BridgeResult<Option<String>> {
        self.call("authorize_mobile_request", json!(authorization))
            .map(optional_text)
    }

    fn start_mobile_pairing(&self) -> BridgeResult<Value> {
        self.call("start_mobile_pairing", Value::Null)
    }

    fn pair_mobile_device(&self, code: &str, device_name: &str) -> BridgeResult<Option<String>> {
        self.call("pair_mobile_device", json!([code, device_name]))
            .map(optional_text)
    }

    fn unpair_mobile_device_by_token(&self, authorization: Option<&str>) -> BridgeResult<bool> {
        self.call("unpair_mobile_device_by_token", json!(authorization))
            .map(truthy)
    }

    fn update_mobile_notification_subscription(
        &self,
        authorization: Option<&str>,
        subscription: Value,
    ) -> BridgeResult<bool> {
        self.call(
            "update_mobile_notification_subscription",
            json!([authorization, subscription]),
        )
        .map(truthy)
    }

    fn delete_mobile_notification_subscription(
        &self,
        authorization: Option<&str>,
    ) -> BridgeResult<bool> {
        self.call(
            "delete_mobile_notification_subscription",
            json!(authorization),
        )
        .map(truthy)
    }

    fn on_mobile_device_unpaired(&self, listener: DeviceSink) -> Subscription {
        self.unpaired_listeners.add(listener)
    }
}

/// 크기 요청으로 잡은 가짜 viewport. 호출은 같은 기록에 `viewport.*` 이름으로 남는다
pub(super) struct FakeViewport {
    pub(super) recorder: Arc<Recorder>,
}

impl PaneViewport for FakeViewport {
    fn resize(&self, cols: u16, rows: u16) -> BridgeResult<Option<String>> {
        self.recorder
            .call("viewport.resize", json!([cols, rows]))
            .map(optional_text)
    }

    fn scroll(&self, request: &Value) -> BridgeResult<Option<String>> {
        self.recorder
            .call("viewport.scroll", request.clone())
            .map(optional_text)
    }

    fn release(&self) -> BridgeResult<()> {
        self.recorder
            .call("viewport.release", Value::Null)
            .map(drop)
    }
}

/// 원격 B로 잇는 가짜 중계. 쓰기는 `[내용, 이진 여부]`로 남는다
pub(super) struct FakeRelay {
    pub(super) recorder: Arc<Recorder>,
}

impl RemotePaneRelay for FakeRelay {
    fn write(&self, data: &[u8], is_binary: bool) {
        self.recorder.record(
            "relay.write",
            json!([String::from_utf8_lossy(data), is_binary]),
        );
    }

    fn close(&self) {
        self.recorder.record("relay.close", Value::Null);
    }
}

impl HookBackend for FakeBridge {
    fn update_hook_task_status(
        &self,
        body: &Value,
        authorization: Option<&str>,
        _: Option<&HashSet<String>>,
    ) -> (u16, Value) {
        match self.call(
            "update_hook_task_status",
            json!({ "body": body, "authorization": authorization }),
        ) {
            Ok(payload) => (200, payload),
            Err(error) => (500, json!({ "success": false, "error": error.to_string() })),
        }
    }

    /// `Feature` 오류는 서비스가 거절한 요청(`TaskCliRequestError`), 그 밖은 예상하지 못한 실패로 바꾼다
    fn run_task_cli(
        &self,
        route: TaskCliRoute,
        authorization: Option<&str>,
        body: &Map<String, Value>,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<Value, TaskCliFailure> {
        let args =
            json!({ "authorization": authorization, "body": body, "hosts": source_ssh_hosts });
        self.call(&format!("task_cli.{route:?}"), args)
            .map_err(|error| match error {
                BridgeError::Feature { status, message } => {
                    TaskCliFailure::Request { status, message }
                }
                other => TaskCliFailure::Internal(other.to_string()),
            })
    }

    fn read_hook_install_script(&self, token: Option<&str>) -> Option<String> {
        self.call("read_hook_install_script", json!(token))
            .ok()
            .and_then(optional_text)
    }
}

/// 경계를 넘는 원격 중계 대상(테스트에서 인자를 비교하기 쉽게 JSON으로)
pub(super) fn remote_target(target: &RemotePaneTarget) -> Value {
    json!([
        target.task_id,
        target.pane_id,
        target.device_id,
        target.grant_id
    ])
}

pub(super) fn suggestion_args(task_id: &str, query: &PaneChatSuggestionQuery) -> Value {
    json!([
        task_id,
        query.target_token,
        query.kind,
        query.query,
        query.limit
    ])
}
