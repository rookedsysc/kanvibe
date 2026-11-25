//! pane 출력 구독(TS `subscribeToPane`). 첫 화면을 한 번 보내고 그 뒤로는 바뀌는 부분만 흘려보낸다.
//!
//! tmux는 pane 하나에 파이프를 하나만 유지하므로 pane마다 스트림을 하나만 걸고 구독자에게 나눠 보낸다.
//! 두 번째 구독자가 파이프를 다시 걸면 `-o` 토글이 첫 번째 파이프를 꺼 버려 둘 다 조용해진다.
//!
//! 구독 자리는 만들고 있는 결과([`Flight`])를 담는다. 두 기기가 같은 pane을 동시에 열면 완성값을 담을 경우
//! 둘 다 "아직 없다"를 보고 파이프를 두 번 걸게 된다. 자리를 잡는 것과 구독자 수를 세는 것은 지도 잠금 안에서 함께 끝낸다.
//! 그래야 앞선 구독자가 나가는 순간 뒤에 자리를 잡은 구독자를 아무도 안 보는 것으로 세지 않는다.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_terminal::pane_mirror::MirrorPane;
use kanvibe_terminal::terminal_registry::{
    ClientClosure, TerminalAttachment, TerminalClient, TerminalRegistry,
};

use crate::error::{MirrorResult, PaneMirrorError, session_not_running};
use crate::flight::Flight;
use crate::mirror::{
    MirrorSessionTarget, MirrorShared, PaneMirror, lock, subscription_key, to_mirror_target,
};
use crate::surfaces::PaneListScope;

/// 구독자에게 출력을 넘기는 콜백
pub type PaneOutput = Arc<dyn Fn(String) + Send + Sync>;
/// 서버 쪽 스트림이 스스로 죽었을 때만 부르는 콜백. 구독자가 스스로 나가는 경우에는 부르지 않는다
pub type PaneEnd = Arc<dyn Fn() + Send + Sync>;

/// pane 하나를 보고 있는 구독자들. 출력과 별개로 끝을 따로 알려, 멈춘 터미널을 살아 있는 것으로 착각하지 않게 한다
#[derive(Default)]
pub(crate) struct StreamAudience {
    listeners: Mutex<Vec<(u64, PaneOutput)>>,
    end_listeners: Mutex<Vec<(u64, PaneEnd)>>,
}

impl StreamAudience {
    pub fn broadcast(&self, chunk: &str) {
        let listeners: Vec<PaneOutput> = lock(&self.listeners)
            .iter()
            .map(|(_, listener)| Arc::clone(listener))
            .collect();
        for listener in listeners {
            listener(chunk.to_string());
        }
    }

    /// 지금 구독자에게 끝을 알리고 모두 내보낸다
    pub fn end(&self) {
        lock(&self.listeners).clear();
        let ending: Vec<(u64, PaneEnd)> = std::mem::take(&mut *lock(&self.end_listeners));
        for (_, listener) in ending {
            listener();
        }
    }

    /// 이미 출력을 받는 구독자가 있었으면 참이다
    fn add(&self, id: u64, on_output: PaneOutput, on_end: PaneEnd) -> bool {
        let mut listeners = lock(&self.listeners);
        let was_live = !listeners.is_empty();
        listeners.push((id, on_output));
        lock(&self.end_listeners).push((id, on_end));
        was_live
    }

    fn remove(&self, id: u64) {
        lock(&self.listeners).retain(|(listener_id, _)| *listener_id != id);
        lock(&self.end_listeners).retain(|(listener_id, _)| *listener_id != id);
    }
}

/// pane 하나의 서버 쪽 스트림
pub(crate) trait PaneStream: Send + Sync {
    fn stop(&self);
    /// 스트림이 첫 화면을 스스로 보내는 경우에만 있다. 뒤늦게 붙은 구독자가 화면을 받도록 다시 보낸다
    fn resend_screen(&self) {}
    fn set_viewport(&self, _size: Option<(i64, i64)>) -> MirrorResult<()> {
        Ok(())
    }
    /// 화면 프레임만으로는 쌓이지 않는 지난 출력을 구독자의 요청으로 다시 보낸다
    fn refresh_history(&self) {}
}

pub(crate) struct PaneSubscription {
    pub audience: Arc<StreamAudience>,
    pub stream: Arc<dyn PaneStream>,
}

type SubscriptionOutcome = Result<Arc<PaneSubscription>, PaneMirrorError>;

/// pane 하나에 걸린 구독과 그것을 보고 있는 구독자 수. 수는 지도 잠금 안에서만 바뀐다
pub(crate) struct PaneClaim {
    pub subscription: Arc<Flight<SubscriptionOutcome>>,
    subscriber_count: AtomicUsize,
}

/// 스트림이 스스로 죽었을 때 자기 자리를 거두는 길. 그 사이 다른 자리로 갈렸다면 건드리지 않는다
#[derive(Clone)]
pub(crate) struct ClaimRelease {
    shared: Weak<MirrorShared>,
    key: String,
    claim: Weak<PaneClaim>,
}

impl ClaimRelease {
    pub fn forget(&self) {
        let (Some(shared), Some(claim)) = (self.shared.upgrade(), self.claim.upgrade()) else {
            return;
        };
        shared.forget_pane_claim(&self.key, &claim);
    }
}

/// 구독 해제 핸들. `stop`을 부르거나 버리면 구독이 끝나고, 마지막 구독자가 나가면 서버 쪽 파이프와 폴링도 멈춘다
pub struct PaneStreamSubscription {
    inner: SubscriptionKind,
    is_stopped: AtomicBool,
}

enum SubscriptionKind {
    Multiplexer {
        shared: Weak<MirrorShared>,
        key: String,
        claim: Arc<PaneClaim>,
        subscription: Arc<PaneSubscription>,
        listener_id: u64,
    },
    TerminalTab {
        client: Arc<MirrorTerminalClient>,
        attachment: Option<TerminalAttachment>,
        terminals: Arc<TerminalRegistry>,
    },
}

impl PaneStreamSubscription {
    /// 소켓이 닫히는 경로와 구독 실패 경로가 모두 부를 수 있어, 두 번 불려도 남의 몫을 깎지 않는다
    pub fn stop(&self) {
        if self.is_stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        match &self.inner {
            SubscriptionKind::Multiplexer {
                shared,
                key,
                claim,
                subscription,
                listener_id,
            } => {
                subscription.audience.remove(*listener_id);
                let is_last = shared
                    .upgrade()
                    .is_some_and(|shared| shared.release_pane_claim(key, claim));
                if is_last {
                    subscription.stream.stop();
                }
            }
            SubscriptionKind::TerminalTab {
                client,
                attachment,
                terminals,
            } => {
                client.mark_released();
                client.close(None);
                if let Some(attachment) = attachment {
                    terminals.client_closed(attachment);
                }
            }
        }
    }
}

impl Drop for PaneStreamSubscription {
    fn drop(&mut self) {
        self.stop();
    }
}

impl PaneMirror {
    /// pane 하나를 구독한다. `on_end`는 `ssh`가 끊기거나 파이프가 실패해 더 이상 아무것도 오지 않을 때만 돈다
    pub fn subscribe_to_pane(
        &self,
        task_id: &str,
        pane_id: &str,
        on_output: PaneOutput,
        on_end: PaneEnd,
    ) -> MirrorResult<PaneStreamSubscription> {
        let shared = &self.shared;
        let task = shared.find_mirror_task(task_id, false)?;
        if task.session_type == Some(SessionType::Terminal) {
            return shared.subscribe_to_terminal_tab(&task, pane_id, on_output, on_end);
        }
        let target = to_mirror_target(&task);
        let pane = shared.ensure_pane_belongs_to_session(&target, pane_id)?;
        on_output(shared.read_pane_snapshot(&target, pane_id)?);

        let key = subscription_key(task_id, pane_id);
        let (claim, subscription) = shared.join_pane_subscription(&key, &target, &pane)?;
        let listener_id = shared.next_id();
        if subscription.audience.add(listener_id, on_output, on_end) {
            subscription.stream.resend_screen();
        }
        Ok(PaneStreamSubscription {
            inner: SubscriptionKind::Multiplexer {
                shared: Arc::downgrade(shared),
                key,
                claim,
                subscription,
                listener_id,
            },
            is_stopped: AtomicBool::new(false),
        })
    }

    /// 앱을 끄기 전에 열려 있는 미러를 전부 멈추고 모바일이 소유하던 임시 크기를 복원한다.
    /// 멈추지 않으면 사용자의 pane에 `pipe-pane`이 걸린 채로 남아 임시 파일이 계속 자란다
    pub fn stop_all_pane_mirrors(&self, is_shutdown: bool) {
        let shared = &self.shared;
        if is_shutdown {
            shared.is_shutting_down.store(true, Ordering::SeqCst);
        }
        lock(&shared.remote_session_watch).take();
        shared.remote_grants.stop_agents();
        shared.restore_all_viewports();
        let claims: Vec<Arc<PaneClaim>> = lock(&shared.subscriptions)
            .drain()
            .map(|(_, claim)| claim)
            .collect();
        for claim in claims {
            match claim.subscription.peek() {
                Some(Ok(subscription)) => subscription.stream.stop(),
                Some(Err(_)) => {}
                None => {
                    std::thread::spawn(move || {
                        if let Ok(subscription) = claim.subscription.wait() {
                            subscription.stream.stop();
                        }
                    });
                }
            }
        }
    }
}

impl MirrorShared {
    /// 이 pane이 태스크의 세션 안에 있는지 확인한다. tmux pane id는 서버 전역이라 `-t %17`이 세션 경계를 넘는다
    pub fn ensure_pane_belongs_to_session(
        &self,
        target: &MirrorSessionTarget,
        pane_id: &str,
    ) -> MirrorResult<MirrorPane> {
        self.list_mirror_panes(target, &PaneListScope::selected(pane_id))?
            .into_iter()
            .find(|candidate| candidate.id == pane_id)
            .ok_or_else(session_not_running)
    }

    /// pane 구독 자리를 잡는다. 자리를 새로 만든 쪽이 스트림을 띄우고, 나머지는 그 결과를 함께 기다린다
    fn join_pane_subscription(
        self: &Arc<Self>,
        key: &str,
        target: &MirrorSessionTarget,
        pane: &MirrorPane,
    ) -> MirrorResult<(Arc<PaneClaim>, Arc<PaneSubscription>)> {
        let (claim, is_creator) = self.claim_pane_subscription(key);
        if is_creator {
            let release = ClaimRelease {
                shared: Arc::downgrade(self),
                key: key.to_string(),
                claim: Arc::downgrade(&claim),
            };
            let outcome = self.start_pane_subscription(target, pane, release);
            if outcome.is_err() {
                self.forget_pane_claim(key, &claim);
            }
            claim.subscription.settle(outcome);
        }
        match claim.subscription.wait() {
            Ok(subscription) => Ok((claim, subscription)),
            Err(error) => {
                self.release_pane_claim(key, &claim);
                Err(error)
            }
        }
    }

    /// 자리를 잡고 구독자 수를 하나 올린다. 자리가 없어 새로 만든 쪽이면 둘째 값이 참이다
    fn claim_pane_subscription(&self, key: &str) -> (Arc<PaneClaim>, bool) {
        let mut subscriptions = lock(&self.subscriptions);
        let (claim, is_creator) = match subscriptions.get(key) {
            Some(claim) => (Arc::clone(claim), false),
            None => {
                let claim = Arc::new(PaneClaim {
                    subscription: Flight::new(),
                    subscriber_count: AtomicUsize::new(0),
                });
                subscriptions.insert(key.to_string(), Arc::clone(&claim));
                (claim, true)
            }
        };
        claim.subscriber_count.fetch_add(1, Ordering::SeqCst);
        (claim, is_creator)
    }

    /// 마지막 구독자였고 지도가 아직 이 자리를 들고 있을 때만 참이다. 그 사이 새 자리가 생겼다면 정리는 그쪽 몫이다
    fn release_pane_claim(&self, key: &str, claim: &Arc<PaneClaim>) -> bool {
        let mut subscriptions = lock(&self.subscriptions);
        let remaining = claim.subscriber_count.fetch_sub(1, Ordering::SeqCst) - 1;
        if remaining > 0 {
            return false;
        }
        forget_claim_in(&mut subscriptions, key, claim)
    }

    pub fn forget_pane_claim(&self, key: &str, claim: &Arc<PaneClaim>) -> bool {
        forget_claim_in(&mut lock(&self.subscriptions), key, claim)
    }

    pub fn has_pane_subscription(&self, key: &str) -> bool {
        lock(&self.subscriptions).contains_key(key)
    }

    /// 지금 걸린 구독. 아직 만드는 중이면 끝날 때까지 기다린다
    pub fn wait_pane_subscription(&self, key: &str) -> Option<Arc<PaneSubscription>> {
        let claim = lock(&self.subscriptions).get(key).cloned()?;
        claim.subscription.wait().ok()
    }

    fn start_pane_subscription(
        self: &Arc<Self>,
        target: &MirrorSessionTarget,
        pane: &MirrorPane,
        release: ClaimRelease,
    ) -> SubscriptionOutcome {
        let audience = Arc::new(StreamAudience::default());
        let dying_audience = Arc::clone(&audience);
        // 서버 쪽 스트림이 스스로 죽었다. 자리를 거둬야 다음 구독자가 죽은 구독을 물려받지 않는다
        let on_dead: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            release.forget();
            dying_audience.end();
        });
        let stream: Arc<dyn PaneStream> = match target.session_type {
            SessionType::Herdr => crate::herdr_stream::start_herdr_frame_stream(
                self, target, pane, &audience, on_dead,
            )?,
            SessionType::Zellij => {
                crate::streams::start_zellij_dump_polling(self, target, &pane.id, &audience)
            }
            _ => {
                crate::streams::start_tmux_pane_stream(self, target, &pane.id, &audience, on_dead)?
            }
        };
        Ok(Arc::new(PaneSubscription { audience, stream }))
    }

    /// terminal 세션 탭 하나를 구독한다. PTY가 이미 있으면 지금 화면을 먼저 받고, 없으면 데스크탑이 탭을 열 때와 같은 길로 만든다
    fn subscribe_to_terminal_tab(
        &self,
        task: &KanbanTask,
        tab_id: &str,
        on_output: PaneOutput,
        on_end: PaneEnd,
    ) -> MirrorResult<PaneStreamSubscription> {
        self.refresh_remote_terminal_tabs(task)?;
        let has_tab = self
            .terminals
            .list_local_terminal_tabs(&task.id)
            .iter()
            .any(|tab| tab.id == tab_id);
        if !has_tab {
            return Err(session_not_running());
        }
        let client = Arc::new(MirrorTerminalClient::new(on_output, on_end));
        let attachment = self.attach_terminal_tab_client(task, tab_id, &client);
        let subscription = PaneStreamSubscription {
            inner: SubscriptionKind::TerminalTab {
                client: Arc::clone(&client),
                attachment: attachment.as_ref().ok().cloned().flatten(),
                terminals: Arc::clone(&self.terminals),
            },
            is_stopped: AtomicBool::new(false),
        };
        attachment?;
        // 화면을 기다리는 사이 셸이 끝났으면 붙을 PTY가 없다
        if !client.is_open() {
            return Err(session_not_running());
        }
        client.mark_attached();
        Ok(subscription)
    }

    fn attach_terminal_tab_client(
        &self,
        task: &KanbanTask,
        tab_id: &str,
        client: &Arc<MirrorTerminalClient>,
    ) -> MirrorResult<Option<TerminalAttachment>> {
        let terminal_client: Arc<dyn TerminalClient> = client.clone();
        if let Some((attachment, screen_until_now)) = self.terminals.attach_client_to_terminal_tab(
            &task.id,
            tab_id,
            Arc::clone(&terminal_client),
        ) {
            client.release_held_output(&screen_until_now);
            return Ok(Some(attachment));
        }
        client.release_held_output("");
        Ok(self
            .attacher
            .attach_task_terminal(task, Some(tab_id), terminal_client, None, None)?)
    }
}

fn forget_claim_in(
    subscriptions: &mut std::collections::HashMap<String, Arc<PaneClaim>>,
    key: &str,
    claim: &Arc<PaneClaim>,
) -> bool {
    if !subscriptions
        .get(key)
        .is_some_and(|current| Arc::ptr_eq(current, claim))
    {
        return false;
    }
    subscriptions.remove(key);
    true
}

/// terminal 세션 탭에 붙는 화면 없는 클라이언트. 붙기 전에 나온 출력은 모았다가 지금 화면 뒤에 이어 보낸다.
/// 레지스트리가 닫으면(PTY 종료) 붙어 있었고 구독자가 나가지 않았을 때만 끝을 알린다.
/// 붙기 전에 닫힌 것은 끝이 아니라 실패다. 실패는 던지는 쪽이 알리므로 끝까지 함께 알리면 사유가 덮인다
pub(crate) struct MirrorTerminalClient {
    on_output: PaneOutput,
    on_end: PaneEnd,
    state: Mutex<TerminalClientState>,
}

struct TerminalClientState {
    is_open: bool,
    is_attached: bool,
    is_released: bool,
    held_chunks: Option<Vec<String>>,
}

impl MirrorTerminalClient {
    fn new(on_output: PaneOutput, on_end: PaneEnd) -> Self {
        Self {
            on_output,
            on_end,
            state: Mutex::new(TerminalClientState {
                is_open: true,
                is_attached: false,
                is_released: false,
                held_chunks: Some(Vec::new()),
            }),
        }
    }

    /// 화면과 모아 둔 출력 사이에 새 출력이 끼지 않도록 콜백도 잠금 안에서 부른다
    fn release_held_output(&self, screen: &str) {
        let mut state = lock(&self.state);
        let held_chunks = state.held_chunks.take().unwrap_or_default();
        if !screen.is_empty() {
            (self.on_output)(screen.to_string());
        }
        for chunk in held_chunks {
            (self.on_output)(chunk);
        }
    }

    fn mark_attached(&self) {
        lock(&self.state).is_attached = true;
    }

    fn mark_released(&self) {
        lock(&self.state).is_released = true;
    }
}

impl TerminalClient for MirrorTerminalClient {
    fn is_open(&self) -> bool {
        lock(&self.state).is_open
    }

    fn send(&self, data: &str) {
        let mut state = lock(&self.state);
        if let Some(held_chunks) = &mut state.held_chunks {
            held_chunks.push(data.to_string());
            return;
        }
        (self.on_output)(data.to_string());
    }

    fn buffered_amount(&self) -> usize {
        0
    }

    fn close(&self, _closure: Option<ClientClosure>) {
        let should_notify_end = {
            let mut state = lock(&self.state);
            let was_open = state.is_open;
            state.is_open = false;
            was_open && state.is_attached && !state.is_released
        };
        if should_notify_end {
            (self.on_end)();
        }
    }
}
