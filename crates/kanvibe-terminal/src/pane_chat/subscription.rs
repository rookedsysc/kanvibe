//! pane 채팅 push 구독(TS `paneChatSubscriptionService.ts`).
//!
//! 구독자가 있는 pane만 주기적으로 다시 읽고, 대화 기록에 순번을 매겨 새로 붙은 줄과 바뀐 상태만 보낸다.
//! 로컬 기록 파일은 OS 파일 이벤트로 감시해 바뀌는 즉시 읽고, 연달아 바뀌면 한 번의 읽기로 묶는다.

mod file_watch;
mod push_state;
#[cfg(test)]
mod tests;
mod timers;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatFreshness, PaneChatPushEvent, PaneChatSnapshot, PaneChatSubscriptionDetail,
    PaneChatSubscriptionFreshness, PaneChatSubscriptionTarget, PaneChatTranscriptStatus,
};
use kanvibe_ai::pane_chat::timeline::{
    PaneChatCursor, PaneChatTimeline, PaneChatTimelineRow, advance_pane_chat_timeline,
    is_pane_chat_cursor_current, read_pane_chat_timeline_seq,
};

pub use file_watch::{
    NotifyTranscriptFileWatchers, TranscriptFileSignal, TranscriptFileWatch, TranscriptFileWatchers,
};
pub use timers::{PaneChatTimerCallback, PaneChatTimerId, PaneChatTimers, ThreadPaneChatTimers};

use super::service::{PaneChatError, PaneChatReadOptions, PaneChatReading};
use push_state::{WatchedPaneState, build_subscriber_state};

/// 대화를 보는 구독자가 있는 pane을 다시 읽는 주기. 입력 상태와 CLI 화면은 파일이 아니라 터미널에서만 알 수 있다
const CHAT_READ_INTERVAL_MS: f64 = 1_000.0;
/// 에이전트가 돌고 있는지만 궁금한 구독자뿐인 pane을 다시 확인하는 주기
const AGENT_PROBE_INTERVAL_MS: f64 = 3_000.0;
/// 기록 파일이 연달아 바뀔 때 다시 읽는 횟수를 이 간격에 한 번으로 묶는다. 조용하다가 온 첫 변화는 바로 읽는다
const READ_COALESCE_WINDOW_MS: f64 = 60.0;
/// KanVibe가 pane에 입력을 치는 동안 읽기를 미뤘다가 다시 확인하는 간격
const ACTION_SETTLE_RETRY_MS: f64 = 200.0;
const MAX_REMEMBERED_TIMELINES: usize = 256;

/// 구독이 읽는 pane chat 서비스의 부분. 서비스가 구현하고 테스트는 가짜를 넣는다
pub trait PaneChatSubscriptionSource: Send + Sync {
    fn read_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        options: PaneChatReadOptions,
    ) -> Result<PaneChatReading, PaneChatError>;
    /// 다른 구독 감시가 방금 읽은 결과가 있으면 그것을 재사용한다
    fn read_recent_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> Result<PaneChatReading, PaneChatError>;
    fn get_pane_chat_agent_provider(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Option<AiSessionProvider>;
    fn is_pane_chat_action_in_progress(&self, task_id: &str) -> bool;
}

/// 구독의 수명을 쥔 쪽(TS의 소켓 객체). 소켓 하나가 구독자 하나다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneChatSubscriberOwner(pub u64);

/// 구독자에게 이벤트를 보내는 함수
pub type PaneChatPushSender = Arc<dyn Fn(PaneChatPushEvent) + Send + Sync>;

/// 구독할 pane과, 구독자가 그 pane의 대화를 어디까지 받았는지
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatSubscriptionRequest {
    pub target: PaneChatSubscriptionTarget,
    pub cursor: Option<PaneChatCursor>,
}

struct PaneSubscriber {
    id: u64,
    target: PaneChatSubscriptionTarget,
    send: PaneChatPushSender,
    /// 마지막으로 보낸 상태. 같은 상태를 되풀이해 보내지 않으려고 직렬화한 값으로 견준다
    last_sent_state: Option<String>,
}

struct PaneWatcher {
    /// 같은 pane의 감시를 멈췄다 다시 만들었을 때, 예전 감시의 예약과 읽기가 새 감시를 건드리지 않게 가르는 값
    generation: u64,
    task_id: String,
    pane_id: String,
    subscribers: Vec<PaneSubscriber>,
    read_timer: Option<PaneChatTimerId>,
    /// 아직 읽은 적이 없으면 `None`이고, 첫 요청은 바로 읽는다
    last_read_at: Option<f64>,
    is_reading: bool,
    /// 읽는 도중에 다시 읽으라는 요청이 왔는지
    is_read_requested: bool,
    transcript_file: Option<String>,
    /// 걸어 둔 감시와 그 감시를 가리는 순번
    transcript_watch: Option<(u64, Box<dyn TranscriptFileWatch>)>,
}

impl PaneWatcher {
    fn has_chat_subscriber(&self) -> bool {
        self.subscribers
            .iter()
            .any(|subscriber| subscriber.target.detail == PaneChatSubscriptionDetail::Chat)
    }

    fn has_screen_subscriber(&self) -> bool {
        self.subscribers.iter().any(|subscriber| {
            subscriber.target.detail == PaneChatSubscriptionDetail::Chat
                && subscriber.target.should_include_screen
        })
    }
}

#[derive(Default)]
struct SubscriptionRegistry {
    watchers: HashMap<String, PaneWatcher>,
    subscribers_by_owner: HashMap<PaneChatSubscriberOwner, Vec<(String, u64)>>,
    /// 구독자가 떠난 뒤에도 순번을 기억해야 돌아온 구독자가 자기 위치가 최신인지 물을 수 있다. 오래 쓰지 않은 것부터 앞에 있다
    timelines: Vec<(String, PaneChatTimeline)>,
    next_subscriber_id: u64,
    next_generation: u64,
}

/// 보낼 이벤트들. 잠금을 푼 뒤 이 순서대로 보낸다
type PendingEvents = Vec<(PaneChatPushSender, PaneChatPushEvent)>;

/// 감시 한 번이 읽을 것
enum WatchedRead {
    Chat { should_include_screen: bool },
    Agent,
}

enum WatchedOutcome {
    /// 스냅샷이 커서 다른 갈래와 크기를 맞추려고 상자에 담는다
    Chat(Result<Box<PaneChatReading>, PaneChatError>),
    Agent(Option<AiSessionProvider>),
}

fn pane_key(task_id: &str, pane_id: &str) -> String {
    serde_json::json!([task_id, pane_id]).to_string()
}

fn dispatch(events: PendingEvents) {
    for (send, event) in events {
        send(event);
    }
}

fn delay_of(milliseconds: f64) -> Duration {
    Duration::from_secs_f64(milliseconds.max(0.0) / 1_000.0)
}

struct SubscriptionInner {
    source: Arc<dyn PaneChatSubscriptionSource>,
    timers: Arc<dyn PaneChatTimers>,
    file_watchers: Arc<dyn TranscriptFileWatchers>,
    registry: Mutex<SubscriptionRegistry>,
    /// 걸어 둔 기록 파일 감시마다 매기는 순번. 죽은 감시가 그 뒤에 새로 건 감시를 내려놓지 않게 가른다
    next_watch_serial: AtomicU64,
}

/// pane 채팅 구독 서비스. 앱이 한 번 만들어 모바일 소켓 서버가 함께 쓴다
pub struct PaneChatSubscriptions {
    inner: Arc<SubscriptionInner>,
}

impl PaneChatSubscriptions {
    pub fn new(
        source: Arc<dyn PaneChatSubscriptionSource>,
        timers: Arc<dyn PaneChatTimers>,
        file_watchers: Arc<dyn TranscriptFileWatchers>,
    ) -> Self {
        Self {
            inner: Arc::new(SubscriptionInner {
                source,
                timers,
                file_watchers,
                registry: Mutex::new(SubscriptionRegistry::default()),
                next_watch_serial: AtomicU64::new(0),
            }),
        }
    }

    /// 구독자 하나가 보려는 pane 전체를 통째로 바꾼다. 빈 목록이면 전부 해제한다.
    ///
    /// 구독을 먼저 붙인 뒤에 위치를 견준다. 견준 직후에 생긴 줄이 이 구독자에게 push로 가거나 "뒤처짐"으로 잡혀야 놓치지 않는다
    pub fn set_pane_chat_subscriptions(
        &self,
        owner: PaneChatSubscriberOwner,
        requests: &[PaneChatSubscriptionRequest],
        send: PaneChatPushSender,
    ) -> Vec<PaneChatSubscriptionFreshness> {
        let mut registry = self.inner.lock();
        let changed_keys =
            self.inner
                .replace_owner_subscribers(&mut registry, owner, requests, &send);
        let now = self.inner.timers.now_ms();
        for key in changed_keys {
            if let Some(watcher) = registry.watchers.get_mut(&key) {
                self.inner.request_read(watcher, now);
            }
        }
        requests
            .iter()
            .map(|request| freshness_of(&registry, request))
            .collect()
    }

    pub fn clear_pane_chat_subscriptions(&self, owner: PaneChatSubscriberOwner) {
        let mut registry = self.inner.lock();
        let Some(subscribers) = registry.subscribers_by_owner.remove(&owner) else {
            return;
        };
        for (key, subscriber_id) in subscribers {
            self.inner
                .remove_subscriber(&mut registry, &key, subscriber_id);
        }
    }

    /// 순번이 붙은 스냅샷. 구독자는 여기서 받은 위치부터 push를 이어 받는다.
    /// push가 끊긴 휴대폰은 이 조회로 CLI 화면도 받으므로, 화면이 필요 없다고 알리지 않은 조회는 화면을 함께 읽는다
    pub fn get_sequenced_pane_chat_snapshot(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: Option<bool>,
    ) -> Result<PaneChatSnapshot, PaneChatError> {
        let reading = self.inner.source.read_recent_pane_chat(
            task_id,
            pane_id,
            should_include_screen.unwrap_or(true),
        )?;
        let mut events = PendingEvents::new();
        let snapshot = {
            let mut registry = self.inner.lock();
            sequence_snapshot(&mut registry, reading.snapshot, &mut events)
        };
        dispatch(events);
        Ok(snapshot)
    }
}

fn freshness_of(
    registry: &SubscriptionRegistry,
    request: &PaneChatSubscriptionRequest,
) -> PaneChatSubscriptionFreshness {
    let key = pane_key(&request.target.task_id, &request.target.pane_id);
    let timeline = registry
        .timelines
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, timeline)| timeline);
    let is_current = is_pane_chat_cursor_current(timeline, request.cursor.as_ref());
    PaneChatSubscriptionFreshness {
        task_id: request.target.task_id.clone(),
        pane_id: request.target.pane_id.clone(),
        freshness: if is_current {
            PaneChatFreshness::Current
        } else {
            PaneChatFreshness::Behind
        },
    }
}

fn remember_timeline(registry: &mut SubscriptionRegistry, key: &str, timeline: PaneChatTimeline) {
    registry.timelines.retain(|(candidate, _)| candidate != key);
    registry.timelines.push((key.to_string(), timeline));
    if registry.timelines.len() > MAX_REMEMBERED_TIMELINES {
        let watchers = &registry.watchers;
        let oldest_idle = registry
            .timelines
            .iter()
            .position(|(candidate, _)| !watchers.contains_key(candidate));
        if let Some(index) = oldest_idle {
            registry.timelines.remove(index);
        }
    }
}

fn emit_to_chat_subscribers(
    watcher: &PaneWatcher,
    event: &PaneChatPushEvent,
    events: &mut PendingEvents,
) {
    for subscriber in &watcher.subscribers {
        if subscriber.target.detail == PaneChatSubscriptionDetail::Chat {
            events.push((Arc::clone(&subscriber.send), event.clone()));
        }
    }
}

fn emit_rows(
    watcher: &PaneWatcher,
    epoch: &str,
    rows: Vec<PaneChatTimelineRow>,
    events: &mut PendingEvents,
) {
    let event = PaneChatPushEvent::Rows {
        task_id: watcher.task_id.clone(),
        pane_id: watcher.pane_id.clone(),
        epoch: epoch.to_string(),
        rows,
    };
    emit_to_chat_subscribers(watcher, &event, events);
}

/// 읽어 온 스냅샷의 대화에 순번을 매기고, 그 pane을 보는 구독자에게 달라진 줄을 내보낸다.
///
/// HTTP 조회와 감시가 같은 순번을 쓰도록 둘 다 이 함수를 지난다. 한쪽만 순번을 올리면 다른 쪽 구독자가 그 줄을 놓친다
fn sequence_snapshot(
    registry: &mut SubscriptionRegistry,
    mut snapshot: PaneChatSnapshot,
    events: &mut PendingEvents,
) -> PaneChatSnapshot {
    let Some(pane_id) = snapshot.pane_id.clone() else {
        return snapshot;
    };
    if snapshot.transcript.status != PaneChatTranscriptStatus::Available {
        return snapshot;
    }
    let key = pane_key(&snapshot.task_id, &pane_id);
    let oldest_first_messages: Vec<_> =
        snapshot.transcript.messages.iter().rev().cloned().collect();
    let previous = registry
        .timelines
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, timeline)| timeline);
    let advance = advance_pane_chat_timeline(
        previous,
        snapshot.transcript.session_id.as_deref(),
        &oldest_first_messages,
    );
    let epoch = advance.timeline.epoch.clone();
    let seq = read_pane_chat_timeline_seq(&advance.timeline);
    remember_timeline(registry, &key, advance.timeline);
    if let Some(watcher) = registry.watchers.get(&key) {
        if advance.is_replaced {
            let event = PaneChatPushEvent::Replaced {
                task_id: watcher.task_id.clone(),
                pane_id: watcher.pane_id.clone(),
                epoch: epoch.clone(),
            };
            emit_to_chat_subscribers(watcher, &event, events);
        } else if !advance.changed_rows.is_empty() {
            emit_rows(watcher, &epoch, advance.changed_rows, events);
        }
    }
    snapshot.transcript.epoch = Some(epoch);
    snapshot.transcript.seq = Some(seq);
    snapshot
}

fn emit_changed_state(
    watcher: &mut PaneWatcher,
    state: &WatchedPaneState<'_>,
    events: &mut PendingEvents,
) {
    let (task_id, pane_id) = (watcher.task_id.clone(), watcher.pane_id.clone());
    for subscriber in &mut watcher.subscribers {
        let pushed = build_subscriber_state(&subscriber.target, state);
        let serialized_state = serde_json::to_string(&pushed).unwrap_or_default();
        if subscriber.last_sent_state.as_deref() == Some(serialized_state.as_str()) {
            continue;
        }
        subscriber.last_sent_state = Some(serialized_state);
        let event = PaneChatPushEvent::State {
            task_id: task_id.clone(),
            pane_id: pane_id.clone(),
            state: Box::new(pushed),
        };
        events.push((Arc::clone(&subscriber.send), event));
    }
}

impl SubscriptionInner {
    fn lock(&self) -> MutexGuard<'_, SubscriptionRegistry> {
        self.registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 주인의 구독을 요청대로 바꾸고, 새 구독자가 붙은 pane의 키를 돌려준다
    fn replace_owner_subscribers(
        self: &Arc<Self>,
        registry: &mut SubscriptionRegistry,
        owner: PaneChatSubscriberOwner,
        requests: &[PaneChatSubscriptionRequest],
        send: &PaneChatPushSender,
    ) -> Vec<String> {
        let previous_subscribers = registry
            .subscribers_by_owner
            .remove(&owner)
            .unwrap_or_default();
        let mut next_subscribers: Vec<(String, u64)> = Vec::new();
        let mut changed_keys: Vec<String> = Vec::new();
        for request in requests {
            let target = &request.target;
            let key = pane_key(&target.task_id, &target.pane_id);
            let previous = previous_subscribers
                .iter()
                .find(|(candidate, _)| *candidate == key);
            let subscriber_id = match previous {
                Some((_, id)) if has_same_subscription(registry, &key, *id, target) => *id,
                _ => {
                    if let Some((_, previous_id)) = previous {
                        remove_subscriber_keeping_watcher(registry, &key, *previous_id);
                    }
                    let id = add_subscriber(registry, &key, target, send);
                    if !changed_keys.contains(&key) {
                        changed_keys.push(key.clone());
                    }
                    id
                }
            };
            next_subscribers.retain(|(candidate, _)| *candidate != key);
            next_subscribers.push((key, subscriber_id));
        }
        for (key, previous_id) in previous_subscribers {
            if !next_subscribers
                .iter()
                .any(|(candidate, _)| *candidate == key)
            {
                self.remove_subscriber(registry, &key, previous_id);
            }
        }
        if !next_subscribers.is_empty() {
            registry
                .subscribers_by_owner
                .insert(owner, next_subscribers);
        }
        changed_keys
    }

    fn remove_subscriber(
        &self,
        registry: &mut SubscriptionRegistry,
        key: &str,
        subscriber_id: u64,
    ) {
        let Some(watcher) = registry.watchers.get_mut(key) else {
            return;
        };
        watcher
            .subscribers
            .retain(|subscriber| subscriber.id != subscriber_id);
        if watcher.subscribers.is_empty() {
            self.stop_watcher(registry, key);
        }
    }

    fn stop_watcher(&self, registry: &mut SubscriptionRegistry, key: &str) {
        if let Some(mut watcher) = registry.watchers.remove(key) {
            if let Some(timer) = watcher.read_timer.take() {
                self.timers.cancel(timer);
            }
            watcher.transcript_watch = None;
        }
    }

    fn schedule_read(self: &Arc<Self>, watcher: &mut PaneWatcher, delay_ms: f64) {
        if let Some(timer) = watcher.read_timer.take() {
            self.timers.cancel(timer);
        }
        let inner = Arc::downgrade(self);
        let key = pane_key(&watcher.task_id, &watcher.pane_id);
        let generation = watcher.generation;
        let callback = Box::new(move || run_scheduled_read(&inner, &key, generation));
        watcher.read_timer = Some(self.timers.schedule(delay_of(delay_ms), callback));
    }

    /// 지금 읽어 달라는 요청. 직전에 읽었으면 묶음 간격 뒤로 미루고, 읽는 중이면 끝난 뒤 한 번 더 읽는다
    fn request_read(self: &Arc<Self>, watcher: &mut PaneWatcher, now: f64) {
        if watcher.is_reading {
            watcher.is_read_requested = true;
            return;
        }
        let elapsed_ms = watcher.last_read_at.map_or(f64::INFINITY, |at| now - at);
        let delay_ms = if elapsed_ms >= READ_COALESCE_WINDOW_MS {
            0.0
        } else {
            READ_COALESCE_WINDOW_MS - elapsed_ms
        };
        self.schedule_read(watcher, delay_ms);
    }

    /// 읽기를 시작할 수 있으면 읽는 중으로 표시하고 무엇을 읽을지 돌려준다
    fn begin_watched_read(
        self: &Arc<Self>,
        key: &str,
        generation: u64,
    ) -> Option<(String, String, WatchedRead)> {
        let mut registry = self.lock();
        let watcher = registry
            .watchers
            .get_mut(key)
            .filter(|watcher| watcher.generation == generation)?;
        watcher.read_timer = None;
        if watcher.is_reading || watcher.subscribers.is_empty() {
            return None;
        }
        // 프롬프트를 치는 도중의 입력란을 "CLI 입력 중"으로 내보내면 보낸 사람의 입력창이 잠깐 사라졌다 돌아온다.
        // 다 치고 난 상태만 의미가 있으므로 끝날 때까지 읽지 않는다
        if self
            .source
            .is_pane_chat_action_in_progress(&watcher.task_id)
        {
            self.schedule_read(watcher, ACTION_SETTLE_RETRY_MS);
            return None;
        }
        watcher.is_reading = true;
        watcher.is_read_requested = false;
        let read = match watcher.has_chat_subscriber() {
            true => WatchedRead::Chat {
                should_include_screen: watcher.has_screen_subscriber(),
            },
            false => WatchedRead::Agent,
        };
        Some((watcher.task_id.clone(), watcher.pane_id.clone(), read))
    }

    fn perform_watched_read(
        &self,
        task_id: &str,
        pane_id: &str,
        read: WatchedRead,
    ) -> WatchedOutcome {
        match read {
            WatchedRead::Chat {
                should_include_screen,
            } => {
                let options = PaneChatReadOptions {
                    should_include_screen,
                    is_shared_with_other_viewers: true,
                };
                WatchedOutcome::Chat(
                    self.source
                        .read_pane_chat(task_id, Some(pane_id), options)
                        .map(Box::new),
                )
            }
            WatchedRead::Agent => {
                WatchedOutcome::Agent(self.source.get_pane_chat_agent_provider(task_id, pane_id))
            }
        }
    }

    /// 읽은 결과를 구독자에게 내보내고 다음 읽기를 예약한다. 한 번의 읽기 실패로 구독을 끊지 않는다
    fn finish_watched_read(
        self: &Arc<Self>,
        key: &str,
        generation: u64,
        outcome: WatchedOutcome,
    ) -> PendingEvents {
        let mut events = PendingEvents::new();
        let mut registry = self.lock();
        let is_current = |registry: &SubscriptionRegistry| {
            registry.watchers.get(key).is_some_and(|watcher| {
                watcher.generation == generation && !watcher.subscribers.is_empty()
            })
        };
        if is_current(&registry) {
            self.publish_outcome(&mut registry, key, outcome, &mut events);
        }
        let now = self.timers.now_ms();
        let Some(watcher) = registry
            .watchers
            .get_mut(key)
            .filter(|watcher| watcher.generation == generation)
        else {
            return events;
        };
        watcher.is_reading = false;
        watcher.last_read_at = Some(now);
        if watcher.subscribers.is_empty() {
            return events;
        }
        if watcher.is_read_requested {
            self.request_read(watcher, now);
        } else {
            let interval = if watcher.has_chat_subscriber() {
                CHAT_READ_INTERVAL_MS
            } else {
                AGENT_PROBE_INTERVAL_MS
            };
            self.schedule_read(watcher, interval);
        }
        events
    }

    fn publish_outcome(
        self: &Arc<Self>,
        registry: &mut SubscriptionRegistry,
        key: &str,
        outcome: WatchedOutcome,
        events: &mut PendingEvents,
    ) {
        match outcome {
            WatchedOutcome::Chat(Ok(reading)) => {
                let snapshot = sequence_snapshot(registry, reading.snapshot, events);
                if let Some(watcher) = registry.watchers.get_mut(key) {
                    self.watch_transcript_file(watcher, reading.local_transcript_file);
                    emit_changed_state(watcher, &WatchedPaneState::Chat(&snapshot), events);
                }
            }
            WatchedOutcome::Chat(Err(_)) => {}
            WatchedOutcome::Agent(provider) => {
                if let Some(watcher) = registry.watchers.get_mut(key) {
                    self.watch_transcript_file(watcher, None);
                    emit_changed_state(watcher, &WatchedPaneState::Agent(provider), events);
                }
            }
        }
    }

    fn watch_transcript_file(
        self: &Arc<Self>,
        watcher: &mut PaneWatcher,
        transcript_file: Option<String>,
    ) {
        if watcher.transcript_file == transcript_file {
            return;
        }
        watcher.transcript_watch = None;
        watcher.transcript_file = transcript_file;
        let Some(file_path) = watcher.transcript_file.clone() else {
            return;
        };
        let serial = self.next_watch_serial.fetch_add(1, Ordering::Relaxed) + 1;
        let key = pane_key(&watcher.task_id, &watcher.pane_id);
        let generation = watcher.generation;
        let on_change = transcript_change_signal(Arc::downgrade(self), key.clone(), generation);
        let on_error = transcript_error_signal(Arc::downgrade(self), key, generation, serial);
        match self.file_watchers.watch(&file_path, on_change, on_error) {
            Ok(watch) => watcher.transcript_watch = Some((serial, watch)),
            Err(_) => watcher.transcript_file = None,
        }
    }

    /// 감시가 죽어도 주기적인 읽기가 남아 있으므로 조용히 내려놓고, 다음 읽기에서 다시 건다
    fn drop_failed_transcript_watch(&self, key: &str, generation: u64, serial: u64) {
        let mut registry = self.lock();
        let Some(watcher) = registry
            .watchers
            .get_mut(key)
            .filter(|watcher| watcher.generation == generation)
        else {
            return;
        };
        if watcher
            .transcript_watch
            .as_ref()
            .is_some_and(|(current, _)| *current == serial)
        {
            let failed = watcher.transcript_watch.take();
            watcher.transcript_file = None;
            // 감시 스레드의 콜백 안에서 그 감시를 닫지 않도록 다른 스레드에서 버린다
            std::thread::spawn(move || drop(failed));
        }
    }

    fn file_changed(self: &Arc<Self>, key: &str, generation: u64) {
        let mut registry = self.lock();
        let now = self.timers.now_ms();
        if let Some(watcher) = registry
            .watchers
            .get_mut(key)
            .filter(|watcher| watcher.generation == generation)
        {
            self.request_read(watcher, now);
        }
    }
}

fn transcript_change_signal(
    inner: Weak<SubscriptionInner>,
    key: String,
    generation: u64,
) -> TranscriptFileSignal {
    Arc::new(move || {
        if let Some(inner) = inner.upgrade() {
            inner.file_changed(&key, generation);
        }
    })
}

fn transcript_error_signal(
    inner: Weak<SubscriptionInner>,
    key: String,
    generation: u64,
    serial: u64,
) -> TranscriptFileSignal {
    Arc::new(move || {
        if let Some(inner) = inner.upgrade() {
            inner.drop_failed_transcript_watch(&key, generation, serial);
        }
    })
}

/// 예약한 읽기를 돌린다. 감시가 그사이 멈췄거나 다시 만들어졌으면 아무것도 하지 않는다
fn run_scheduled_read(inner: &Weak<SubscriptionInner>, key: &str, generation: u64) {
    let Some(inner) = inner.upgrade() else {
        return;
    };
    let Some((task_id, pane_id, read)) = inner.begin_watched_read(key, generation) else {
        return;
    };
    let outcome = inner.perform_watched_read(&task_id, &pane_id, read);
    dispatch(inner.finish_watched_read(key, generation, outcome));
}

fn has_same_subscription(
    registry: &SubscriptionRegistry,
    key: &str,
    subscriber_id: u64,
    target: &PaneChatSubscriptionTarget,
) -> bool {
    registry
        .watchers
        .get(key)
        .and_then(|watcher| {
            watcher
                .subscribers
                .iter()
                .find(|subscriber| subscriber.id == subscriber_id)
        })
        .is_some_and(|subscriber| {
            subscriber.target.detail == target.detail
                && subscriber.target.should_include_screen == target.should_include_screen
        })
}

/// 같은 pane의 구독 조건만 바뀔 때 감시를 끊었다 다시 걸지 않도록 구독자만 뺀다
fn remove_subscriber_keeping_watcher(
    registry: &mut SubscriptionRegistry,
    key: &str,
    subscriber_id: u64,
) {
    if let Some(watcher) = registry.watchers.get_mut(key) {
        watcher
            .subscribers
            .retain(|subscriber| subscriber.id != subscriber_id);
    }
}

fn add_subscriber(
    registry: &mut SubscriptionRegistry,
    key: &str,
    target: &PaneChatSubscriptionTarget,
    send: &PaneChatPushSender,
) -> u64 {
    registry.next_subscriber_id += 1;
    let id = registry.next_subscriber_id;
    if !registry.watchers.contains_key(key) {
        registry.next_generation += 1;
        let watcher = PaneWatcher {
            generation: registry.next_generation,
            task_id: target.task_id.clone(),
            pane_id: target.pane_id.clone(),
            subscribers: Vec::new(),
            read_timer: None,
            last_read_at: None,
            is_reading: false,
            is_read_requested: false,
            transcript_file: None,
            transcript_watch: None,
        };
        registry.watchers.insert(key.to_string(), watcher);
    }
    let subscriber = PaneSubscriber {
        id,
        target: target.clone(),
        send: Arc::clone(send),
        last_sent_state: None,
    };
    if let Some(watcher) = registry.watchers.get_mut(key) {
        watcher.subscribers.push(subscriber);
    }
    id
}
