//! pane 채팅 화면 테스트의 가짜 서비스와 화면 여는 도구. 가짜 서비스는 TS 테스트의 `vi.mock("actions/paneChat")`처럼
//! TS IPC 이름마다 답을 정하고, 받은 호출을 그 이름과 JSON 인자로 기록한다.
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::{
    AnyView, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, div,
};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
use kanvibe_app::views::task_detail::pane_chat::backend::{
    PaneChatBackend, PaneChatCall, PaneChatCallError, PaneChatConnectionSignal,
    PaneChatConnectionWatch, PaneChatRequest,
};
use kanvibe_app::views::task_detail::pane_chat::client::PaneChatClient;
use kanvibe_app::views::task_detail::pane_chat::preferences::PaneChatPreferences;
use kanvibe_app::views::task_detail::pane_chat::{
    PaneChatLayer, PaneChatLayerProps, PaneChatView, PaneChatViewProps, TerminalChatGrid,
};
use kanvibe_domain::i18n::{Locale, Translator};
use serde_json::{Value, json};

pub type Reply = Result<Value, PaneChatCallError>;
type Handler = Box<dyn Fn(&[Value]) -> Reply + Send>;

enum Queued {
    Ready(Reply),
    Held(oneshot::Receiver<Reply>),
    Forever,
}

#[derive(Default)]
struct FakeState {
    handlers: HashMap<String, Handler>,
    queued: HashMap<String, VecDeque<Queued>>,
    is_held_forever: HashMap<String, bool>,
    calls: Vec<(String, Vec<Value>)>,
    /// Task마다 걸린 연결 신호 구독(살아 있는지 확인하는 표식과 알림)
    connection_watchers: Vec<(String, std::sync::Weak<()>, PaneChatConnectionSignal)>,
}

/// TS IPC 이름마다 답을 정하는 가짜 pane chat 서비스
#[derive(Default, Clone)]
pub struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
}

impl FakeBackend {
    pub fn new() -> Self {
        let backend = Self::default();
        backend.respond("appSettings.getAppSetting", Value::Null);
        backend.respond("appSettings.setAppSetting", Value::Null);
        backend
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap()
    }

    /// 기본 답(값)
    pub fn respond(&self, target: &str, value: Value) {
        self.respond_with(target, move |_| Ok(value.clone()));
    }

    /// 인자에 따라 정하는 기본 답
    pub fn respond_with(&self, target: &str, handler: impl Fn(&[Value]) -> Reply + Send + 'static) {
        let mut state = self.lock();
        state.handlers.insert(target.to_string(), Box::new(handler));
        state.is_held_forever.remove(target);
    }

    /// 기본 답을 오류로
    pub fn fail(&self, target: &str, message: &str) {
        let message = message.to_string();
        self.respond_with(target, move |_| {
            Err(PaneChatCallError::new(message.clone()))
        });
    }

    /// 앞으로의 호출이 모두 끝나지 않는다
    pub fn hang(&self, target: &str) {
        self.lock().is_held_forever.insert(target.to_string(), true);
    }

    /// 다음 호출 한 번의 답
    pub fn once(&self, target: &str, reply: Reply) {
        self.lock()
            .queued
            .entry(target.to_string())
            .or_default()
            .push_back(Queued::Ready(reply));
    }

    /// 다음 호출 한 번을 붙잡아 두고, 돌려준 것으로 나중에 답한다
    pub fn hold(&self, target: &str) -> oneshot::Sender<Reply> {
        let (sender, receiver) = oneshot::channel();
        self.lock()
            .queued
            .entry(target.to_string())
            .or_default()
            .push_back(Queued::Held(receiver));
        sender
    }

    /// 다음 호출 한 번이 끝나지 않는다
    pub fn hold_once_forever(&self, target: &str) {
        self.lock()
            .queued
            .entry(target.to_string())
            .or_default()
            .push_back(Queued::Forever);
    }

    pub fn calls(&self, target: &str) -> Vec<Vec<Value>> {
        self.lock()
            .calls
            .iter()
            .filter(|(called, _)| called == target)
            .map(|(_, arguments)| arguments.clone())
            .collect()
    }

    pub fn call_count(&self, target: &str) -> usize {
        self.calls(target).len()
    }

    pub fn last_call(&self, target: &str) -> Option<Vec<Value>> {
        self.calls(target).pop()
    }

    pub fn clear_calls(&self) {
        self.lock().calls.clear();
    }

    /// CLI가 그 Task에서 새 대화 ID를 알렸다(hook·세션 기록 파일). 아직 구독 중인 화면에만 간다
    pub fn signal_connection(&self, task_id: &str) {
        let signals: Vec<PaneChatConnectionSignal> = self
            .lock()
            .connection_watchers
            .iter()
            .filter(|(watched, alive, _)| watched == task_id && alive.strong_count() > 0)
            .map(|(_, _, signal)| signal.clone())
            .collect();
        for signal in signals {
            signal();
        }
    }

    /// 아직 살아 있는 연결 신호 구독 수
    pub fn connection_watch_count(&self, task_id: &str) -> usize {
        self.lock()
            .connection_watchers
            .iter()
            .filter(|(watched, alive, _)| watched == task_id && alive.strong_count() > 0)
            .count()
    }

    pub fn client(&self) -> PaneChatClient {
        PaneChatClient::new(Arc::new(self.clone()))
    }
}

impl PaneChatBackend for FakeBackend {
    fn call(&self, request: PaneChatRequest) -> PaneChatCall {
        let mut state = self.lock();
        state
            .calls
            .push((request.target.to_string(), request.arguments.clone()));
        let queued = state
            .queued
            .get_mut(request.target)
            .and_then(VecDeque::pop_front);
        let is_forever = state
            .is_held_forever
            .get(request.target)
            .copied()
            .unwrap_or(false);
        let reply = match (queued, is_forever) {
            (Some(queued), _) => queued,
            (None, true) => Queued::Forever,
            (None, false) => Queued::Ready(match state.handlers.get(request.target) {
                Some(handler) => handler(&request.arguments),
                None => Err(PaneChatCallError::new(format!(
                    "unhandled {}",
                    request.target
                ))),
            }),
        };
        drop(state);
        match reply {
            Queued::Ready(reply) => Box::pin(async move { reply }),
            Queued::Held(receiver) => Box::pin(async move {
                match receiver.await {
                    Ok(reply) => reply,
                    Err(_) => std::future::pending().await,
                }
            }),
            Queued::Forever => Box::pin(std::future::pending()),
        }
    }

    fn watch_connection(
        &self,
        task_id: &str,
        signal: PaneChatConnectionSignal,
    ) -> Option<PaneChatConnectionWatch> {
        let alive = Arc::new(());
        self.lock()
            .connection_watchers
            .push((task_id.to_string(), Arc::downgrade(&alive), signal));
        Some(Box::new(alive))
    }
}

pub fn init(cx: &mut TestAppContext) {
    let launch = LaunchOptions {
        data_dir: None,
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: None,
    };
    cx.update(|cx| kanvibe_app::init_app(&launch, cx).expect("app init"));
}

pub fn translator() -> Translator {
    Translator::new(Locale::Ko, "taskDetail")
}

/// `taskDetail.paneChat.<key>` 문구
pub fn t(key: &str) -> String {
    translator().translate(&format!("paneChat.{key}"))
}

pub fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

pub fn advance(cx: &mut VisualTestContext, milliseconds: u64) {
    cx.executor()
        .advance_clock(Duration::from_millis(milliseconds));
    settle(cx);
}

pub fn message(role: &str, text: &str) -> Value {
    json!({ "role": role, "timestamp": null, "text": text, "fullText": text, "isTruncated": false })
}

/// TS `PaneChatView.test.tsx`의 `snapshot()`
pub fn snapshot(token: &str) -> Value {
    json!({
        "taskId": "task-1", "paneId": "%3", "sessionType": "tmux", "provider": "codex",
        "targetToken": token, "canSend": true, "inputState": "ready", "agentActivity": "idle",
        "screen": "Codex ready", "reason": null, "modelPicker": { "supported": true, "mode": "native-cli" },
        "transcript": { "status": "available", "reason": null, "sessionId": "session-a", "nextCursor": null,
            "messages": [message("assistant", "Exact pane response")] },
    })
}

/// 첨부·스킬·파일 언급을 받는 입력창
pub fn composer_capabilities() -> Value {
    json!({ "attachments": true, "skills": true, "fileMentions": true, "maxAttachments": 10, "maxAttachmentBytes": 10_485_760 })
}

/// 스냅샷 JSON의 한 자리를 바꾼다(`{ ...snapshot(), key: value }`)
pub fn with(mut base: Value, key: &str, value: Value) -> Value {
    base[key] = value;
    base
}

pub fn success() -> Value {
    json!({ "success": true })
}

/// 서비스가 쓴 안내만 있는 오류
pub fn error(message: &str) -> PaneChatCallError {
    PaneChatCallError::new(message)
}

/// 테스트 창의 뿌리. 창을 활성으로 만든 뒤에 화면을 붙여, TS 테스트처럼 처음부터 창이 활성인 상태로 시작한다
struct Host {
    child: Option<AnyView>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.child.clone())
    }
}

/// 활성 창을 열고 그 안에 화면을 붙인다
pub fn open_in_active_window<V: Render + 'static>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> (Entity<V>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, _| Host { child: None });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let view = host.update_in(cx, |host, window, cx| {
        let view = cx.new(|cx| build(window, cx));
        host.child = Some(view.clone().into());
        view
    });
    settle(cx);
    (view, cx)
}

/// pane 채팅 화면 하나를 창에 연다
pub fn open_view<'a>(
    cx: &'a mut TestAppContext,
    backend: &FakeBackend,
    pane_id: Option<&str>,
    task_title: Option<&str>,
) -> (Entity<PaneChatView>, &'a mut VisualTestContext) {
    let client = backend.client();
    let pane_id = pane_id.map(str::to_string);
    let task_title = task_title.map(str::to_string);
    open_in_active_window(cx, |window, cx| {
        let preferences = cx.new(|cx| PaneChatPreferences::new(client.clone(), cx));
        let props = PaneChatViewProps {
            client,
            task_id: "task-1".into(),
            pane_id,
            task_title,
            translator: translator(),
            preferences,
            initial_snapshot: None,
        };
        PaneChatView::new(props, window, cx)
    })
}

/// TS `TerminalPaneChatLayer.test.tsx`의 칸 영역(80×24칸, 800×480px)
pub fn grid() -> TerminalChatGrid {
    TerminalChatGrid {
        columns: 80,
        rows: 24,
        width: gpui_kit::px(800.0),
        height: gpui_kit::px(480.0),
    }
}

/// pane 채팅 층을 창에 열고 보이게 한다
pub fn open_layer<'a>(
    cx: &'a mut TestAppContext,
    backend: &FakeBackend,
    task_id: &str,
    is_visible: bool,
) -> (Entity<PaneChatLayer>, &'a mut VisualTestContext) {
    let client = backend.client();
    let task_id = task_id.to_string();
    let (layer, cx) = open_in_active_window(cx, |_, cx| {
        PaneChatLayer::new(
            PaneChatLayerProps {
                client,
                task_id,
                terminal_tab_id: Some("@1".into()),
                task_title: None,
                translator: translator(),
            },
            cx,
        )
    });
    set_layer_visible(&layer, cx, is_visible);
    (layer, cx)
}

pub fn set_layer_visible(
    layer: &Entity<PaneChatLayer>,
    cx: &mut VisualTestContext,
    is_visible: bool,
) {
    layer.update_in(cx, |layer, window, cx| {
        layer.set_visibility(is_visible, Some(grid()), window, cx)
    });
    settle(cx);
}
