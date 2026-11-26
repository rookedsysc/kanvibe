//! TS `paneChatSubscriptionService.test.ts`. 서비스 조회와 파일 감시는 가짜로, 시간은 가상 시계로 돌린다.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatAgentActivity, PaneChatFreshness, PaneChatInputState, PaneChatModelPicker,
    PaneChatModelPickerMode, PaneChatPushEvent, PaneChatPushedState, PaneChatSnapshot,
    PaneChatSubscriptionDetail, PaneChatSubscriptionTarget, PaneChatTranscript,
    PaneChatTranscriptStatus,
};
use kanvibe_ai::pane_chat::timeline::PaneChatCursor;
use kanvibe_ai::types::{AggregatedAiMessage, AiMessageRole};

use super::*;

/// vitest 가짜 타이머처럼 가상 시간을 앞당기며 때가 된 예약을 정해진 순서로 돌린다
struct ManualTimers {
    now: Mutex<f64>,
    pending: Mutex<Vec<(f64, PaneChatTimerId, PaneChatTimerCallback)>>,
    next_id: Mutex<PaneChatTimerId>,
}

impl ManualTimers {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new(1_700_000_000_000.0),
            pending: Mutex::new(Vec::new()),
            next_id: Mutex::new(0),
        })
    }

    fn take_due(&self, until: f64) -> Option<(f64, PaneChatTimerCallback)> {
        let mut pending = self.pending.lock().unwrap();
        let index = pending
            .iter()
            .enumerate()
            .filter(|(_, (due, _, _))| *due <= until)
            .min_by(|(_, left), (_, right)| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)))
            .map(|(index, _)| index)?;
        let (due, _, callback) = pending.remove(index);
        Some((due, callback))
    }

    /// `vi.advanceTimersByTimeAsync(milliseconds)`
    fn advance(&self, milliseconds: f64) {
        let until = *self.now.lock().unwrap() + milliseconds;
        while let Some((due, callback)) = self.take_due(until) {
            *self.now.lock().unwrap() = due;
            callback();
        }
        *self.now.lock().unwrap() = until;
    }

    /// `vi.getTimerCount()`
    fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
}

impl PaneChatTimers for ManualTimers {
    fn now_ms(&self) -> f64 {
        *self.now.lock().unwrap()
    }

    fn schedule(&self, delay: Duration, callback: PaneChatTimerCallback) -> PaneChatTimerId {
        let mut next_id = self.next_id.lock().unwrap();
        *next_id += 1;
        let due = self.now_ms() + delay.as_secs_f64() * 1_000.0;
        self.pending.lock().unwrap().push((due, *next_id, callback));
        *next_id
    }

    fn cancel(&self, timer: PaneChatTimerId) {
        self.pending
            .lock()
            .unwrap()
            .retain(|(_, id, _)| *id != timer);
    }
}

/// 서비스 조회 기록. TS는 `readPaneChat`과 `readRecentPaneChat`을 같은 mock으로 묶었다
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReadCall {
    Watched(String, Option<String>, PaneChatReadOptions),
    Recent(String, Option<String>, bool),
}

#[derive(Default)]
struct FakeSource {
    served: Mutex<Option<PaneChatReading>>,
    provider: Mutex<Option<AiSessionProvider>>,
    is_action_in_progress: AtomicBool,
    reads: Mutex<Vec<ReadCall>>,
}

impl FakeSource {
    fn read_count(&self) -> usize {
        self.reads.lock().unwrap().len()
    }

    fn last_read(&self) -> Option<ReadCall> {
        self.reads.lock().unwrap().last().cloned()
    }

    fn served(&self) -> Result<PaneChatReading, PaneChatError> {
        self.served
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| PaneChatError::Other("응답 없음".to_string()))
    }
}

impl PaneChatSubscriptionSource for FakeSource {
    fn read_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        options: PaneChatReadOptions,
    ) -> Result<PaneChatReading, PaneChatError> {
        let call = ReadCall::Watched(task_id.into(), pane_id.map(Into::into), options);
        self.reads.lock().unwrap().push(call);
        self.served()
    }

    fn read_recent_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> Result<PaneChatReading, PaneChatError> {
        let call = ReadCall::Recent(
            task_id.into(),
            pane_id.map(Into::into),
            should_include_screen,
        );
        self.reads.lock().unwrap().push(call);
        self.served()
    }

    fn get_pane_chat_agent_provider(&self, _: &str, _: &str) -> Option<AiSessionProvider> {
        *self.provider.lock().unwrap()
    }

    fn is_pane_chat_action_in_progress(&self, _: &str) -> bool {
        self.is_action_in_progress.load(Ordering::SeqCst)
    }
}

struct FakeWatch {
    closed: Arc<AtomicUsize>,
}

impl TranscriptFileWatch for FakeWatch {}

impl Drop for FakeWatch {
    fn drop(&mut self) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct FakeFileWatchers {
    listeners: Mutex<Vec<TranscriptFileSignal>>,
    closed: Arc<AtomicUsize>,
}

impl FakeFileWatchers {
    fn watch_count(&self) -> usize {
        self.listeners.lock().unwrap().len()
    }

    fn change(&self, index: usize) {
        let listener = Arc::clone(&self.listeners.lock().unwrap()[index]);
        listener();
    }
}

impl TranscriptFileWatchers for FakeFileWatchers {
    fn watch(
        &self,
        _: &str,
        on_change: TranscriptFileSignal,
        _: TranscriptFileSignal,
    ) -> Result<Box<dyn TranscriptFileWatch>, String> {
        self.listeners.lock().unwrap().push(on_change);
        Ok(Box::new(FakeWatch {
            closed: Arc::clone(&self.closed),
        }))
    }
}

struct Harness {
    source: Arc<FakeSource>,
    timers: Arc<ManualTimers>,
    files: Arc<FakeFileWatchers>,
    service: PaneChatSubscriptions,
    next_owner: Mutex<u64>,
}

fn harness() -> Harness {
    let source = Arc::new(FakeSource::default());
    let timers = ManualTimers::new();
    let files = Arc::new(FakeFileWatchers::default());
    let service = PaneChatSubscriptions::new(source.clone(), timers.clone(), files.clone());
    Harness {
        source,
        timers,
        files,
        service,
        next_owner: Mutex::new(0),
    }
}

impl Harness {
    /// TS의 새 `{}` 소유자
    fn owner(&self) -> PaneChatSubscriberOwner {
        let mut next_owner = self.next_owner.lock().unwrap();
        *next_owner += 1;
        PaneChatSubscriberOwner(*next_owner)
    }

    fn serve(&self, snapshot: PaneChatSnapshot, local_transcript_file: Option<&str>) {
        *self.source.served.lock().unwrap() = Some(PaneChatReading {
            snapshot,
            local_transcript_file: local_transcript_file.map(Into::into),
        });
    }

    fn subscribe(
        &self,
        owner: PaneChatSubscriberOwner,
        targets: &[PaneChatSubscriptionTarget],
        send: PaneChatPushSender,
    ) -> Vec<PaneChatSubscriptionFreshness> {
        let requests: Vec<_> = targets
            .iter()
            .map(|target| PaneChatSubscriptionRequest {
                target: target.clone(),
                cursor: None,
            })
            .collect();
        self.service
            .set_pane_chat_subscriptions(owner, &requests, send)
    }
}

fn message(text: &str) -> AggregatedAiMessage {
    AggregatedAiMessage::plain(AiMessageRole::Assistant, None, text)
}

/// texts는 오래된 것부터 적는다. 실제 스냅샷은 최신순이라 뒤집어 담는다
fn snapshot_of(texts: &[&str]) -> PaneChatSnapshot {
    PaneChatSnapshot {
        task_id: "task-1".into(),
        pane_id: Some("%1".into()),
        session_type: None,
        provider: Some(AiSessionProvider::Claude),
        target_token: Some("token".into()),
        conversation_key: None,
        can_send: true,
        input_state: PaneChatInputState::Ready,
        agent_activity: PaneChatAgentActivity::Idle,
        screen: "화면 A".into(),
        reason: None,
        transcript: PaneChatTranscript {
            status: PaneChatTranscriptStatus::Available,
            reason: None,
            session_id: Some("session-a".into()),
            messages: texts.iter().rev().map(|text| message(text)).collect(),
            next_cursor: None,
            epoch: None,
            seq: None,
        },
        model_picker: PaneChatModelPicker {
            is_supported: true,
            mode: PaneChatModelPickerMode::NativeCli,
            is_api_supported: None,
        },
        model_settings: None,
        prompt: None,
        composer: None,
    }
}

fn target(
    detail: PaneChatSubscriptionDetail,
    should_include_screen: bool,
) -> PaneChatSubscriptionTarget {
    PaneChatSubscriptionTarget {
        task_id: "task-1".into(),
        pane_id: "%1".into(),
        detail,
        should_include_screen,
    }
}

fn chat_target() -> PaneChatSubscriptionTarget {
    target(PaneChatSubscriptionDetail::Chat, false)
}

type EventLog = Arc<Mutex<Vec<PaneChatPushEvent>>>;

fn collector() -> (PaneChatPushSender, EventLog) {
    let events: EventLog = Arc::default();
    let sink = Arc::clone(&events);
    (
        Arc::new(move |event| sink.lock().unwrap().push(event)),
        events,
    )
}

fn ignore() -> PaneChatPushSender {
    Arc::new(|_| {})
}

fn row_texts(events: &EventLog) -> Vec<String> {
    events
        .lock()
        .unwrap()
        .iter()
        .flat_map(|event| match event {
            PaneChatPushEvent::Rows { rows, .. } => rows
                .iter()
                .map(|row| format!("{}:{}", row.seq, row.message.full_text))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn states(events: &EventLog) -> Vec<PaneChatPushedState> {
    events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            PaneChatPushEvent::State { state, .. } => Some((**state).clone()),
            _ => None,
        })
        .collect()
}

fn provider_state(provider: Option<AiSessionProvider>) -> PaneChatPushEvent {
    PaneChatPushEvent::State {
        task_id: "task-1".into(),
        pane_id: "%1".into(),
        state: Box::new(PaneChatPushedState {
            provider: Some(provider),
            ..PaneChatPushedState::default()
        }),
    }
}

#[test]
fn subscribing_reads_immediately_then_pushes_only_appended_rows() {
    let given = harness();
    let (send, events) = collector();
    given.serve(snapshot_of(&["a", "b"]), None);
    given.subscribe(given.owner(), &[chat_target()], send);
    given.timers.advance(0.0);
    assert_eq!(states(&events).len(), 1);
    assert_eq!(row_texts(&events), ["1:a", "2:b"]);

    events.lock().unwrap().clear();
    given.serve(snapshot_of(&["a", "b", "c"]), None);
    given.timers.advance(1_000.0);
    assert_eq!(row_texts(&events), ["3:c"]);
    assert!(states(&events).is_empty());
}

#[test]
fn last_subscriber_leaving_stops_reads_and_file_watch() {
    let given = harness();
    let (first, second) = (given.owner(), given.owner());
    given.serve(
        snapshot_of(&["a"]),
        Some("/home/user/.claude/projects/p/session.jsonl"),
    );
    given.subscribe(first, &[chat_target()], ignore());
    given.subscribe(second, &[chat_target()], ignore());
    given.timers.advance(0.0);
    assert_eq!(given.files.watch_count(), 1);

    given.service.clear_pane_chat_subscriptions(first);
    let reads_with_one_left = given.source.read_count();
    given.timers.advance(1_000.0);
    assert!(given.source.read_count() > reads_with_one_left);
    assert_eq!(given.files.closed.load(Ordering::SeqCst), 0);

    given.subscribe(second, &[], ignore());
    let reads_after_all_left = given.source.read_count();
    given.timers.advance(5_000.0);
    assert_eq!(given.source.read_count(), reads_after_all_left);
    assert_eq!(given.files.closed.load(Ordering::SeqCst), 1);
    assert_eq!(given.timers.pending_count(), 0);
}

#[test]
fn burst_of_file_changes_is_coalesced_into_one_follow_up_read() {
    let given = harness();
    given.serve(
        snapshot_of(&["a"]),
        Some("/home/user/.claude/projects/p/session.jsonl"),
    );
    given.subscribe(given.owner(), &[chat_target()], ignore());
    given.timers.advance(0.0);
    given.timers.advance(200.0);
    let reads_before_burst = given.source.read_count();

    for _ in 0..5 {
        given.files.change(0);
        given.timers.advance(10.0);
    }
    assert_eq!(given.source.read_count(), reads_before_burst + 1);
    given.timers.advance(60.0);
    assert_eq!(given.source.read_count(), reads_before_burst + 2);
}

#[test]
fn without_a_local_file_periodic_reads_still_push_new_rows() {
    let given = harness();
    let (send, events) = collector();
    given.serve(snapshot_of(&["a"]), None);
    given.subscribe(given.owner(), &[chat_target()], send);
    given.timers.advance(0.0);
    assert_eq!(given.files.watch_count(), 0);
    events.lock().unwrap().clear();
    given.serve(snapshot_of(&["a", "b"]), None);
    given.timers.advance(1_000.0);
    assert_eq!(row_texts(&events), ["2:b"]);
}

#[test]
fn screen_goes_only_to_subscribers_that_asked_for_it() {
    let given = harness();
    let (with_screen_send, with_screen) = collector();
    let (without_screen_send, without_screen) = collector();
    given.serve(snapshot_of(&["a"]), None);
    let screen_target = target(PaneChatSubscriptionDetail::Chat, true);
    given.subscribe(given.owner(), &[screen_target], with_screen_send);
    given.subscribe(given.owner(), &[chat_target()], without_screen_send);
    given.timers.advance(0.0);
    let first_state = states(&without_screen).into_iter().next().unwrap();
    assert_eq!(first_state.screen, None);

    with_screen.lock().unwrap().clear();
    without_screen.lock().unwrap().clear();
    let mut changed = snapshot_of(&["a"]);
    changed.screen = "화면 B".into();
    given.serve(changed, None);
    given.timers.advance(1_000.0);
    let pushed = with_screen.lock().unwrap().clone();
    assert_eq!(pushed.len(), 1);
    assert_eq!(states(&with_screen)[0].screen.as_deref(), Some("화면 B"));
    assert!(without_screen.lock().unwrap().is_empty());
}

#[test]
fn screen_is_read_and_shared_only_while_a_screen_subscriber_exists() {
    let given = harness();
    let owner = given.owner();
    given.serve(snapshot_of(&["a"]), None);
    given.subscribe(owner, &[chat_target()], ignore());
    given.timers.advance(0.0);
    let shared = |should_include_screen| PaneChatReadOptions {
        should_include_screen,
        is_shared_with_other_viewers: true,
    };
    let watched = |options| {
        Some(ReadCall::Watched(
            "task-1".into(),
            Some("%1".into()),
            options,
        ))
    };
    assert_eq!(given.source.last_read(), watched(shared(false)));

    // 직전 읽기와 붙어 오는 요청은 묶음 간격 뒤에 읽는다
    given.subscribe(
        owner,
        &[target(PaneChatSubscriptionDetail::Chat, true)],
        ignore(),
    );
    given.timers.advance(100.0);
    assert_eq!(given.source.last_read(), watched(shared(true)));
}

#[test]
fn http_snapshot_reads_the_screen_by_default() {
    let given = harness();
    given.serve(snapshot_of(&["a"]), None);
    given
        .service
        .get_sequenced_pane_chat_snapshot("task-1", Some("%1"), None)
        .unwrap();
    let expected = ReadCall::Recent("task-1".into(), Some("%1".into()), true);
    assert_eq!(given.source.last_read(), Some(expected));
}

#[test]
fn http_snapshot_skips_the_screen_when_told_it_is_not_needed() {
    let given = harness();
    given.serve(snapshot_of(&["a"]), None);
    given
        .service
        .get_sequenced_pane_chat_snapshot("task-1", Some("%1"), Some(false))
        .unwrap();
    let expected = ReadCall::Recent("task-1".into(), Some("%1".into()), false);
    assert_eq!(given.source.last_read(), Some(expected));
}

#[test]
fn input_state_change_is_pushed_to_subscribers_without_screen() {
    let given = harness();
    let (send, events) = collector();
    given.serve(snapshot_of(&["a"]), None);
    given.subscribe(given.owner(), &[chat_target()], send);
    given.timers.advance(0.0);
    events.lock().unwrap().clear();
    let mut occupied = snapshot_of(&["a"]);
    occupied.input_state = PaneChatInputState::Occupied;
    given.serve(occupied, None);
    given.timers.advance(1_000.0);
    assert_eq!(events.lock().unwrap().len(), 1);
    assert_eq!(
        states(&events)[0].input_state,
        Some(PaneChatInputState::Occupied)
    );
}

#[test]
fn reads_wait_while_kanvibe_types_into_the_pane_and_push_only_the_settled_state() {
    let given = harness();
    let (send, events) = collector();
    given.serve(snapshot_of(&["a"]), None);
    given.subscribe(given.owner(), &[chat_target()], send);
    given.timers.advance(0.0);
    events.lock().unwrap().clear();

    given
        .source
        .is_action_in_progress
        .store(true, Ordering::SeqCst);
    let mut occupied = snapshot_of(&["a"]);
    occupied.input_state = PaneChatInputState::Occupied;
    given.serve(occupied, None);
    let reads_before_action = given.source.read_count();
    given.timers.advance(3_000.0);
    assert_eq!(given.source.read_count(), reads_before_action);
    assert!(events.lock().unwrap().is_empty());

    given
        .source
        .is_action_in_progress
        .store(false, Ordering::SeqCst);
    given.serve(snapshot_of(&["a", "b"]), None);
    given.timers.advance(200.0);
    assert_eq!(row_texts(&events), ["2:b"]);
    assert!(states(&events).is_empty());
}

#[test]
fn agent_subscribers_get_only_provider_changes_without_reading_chat() {
    let given = harness();
    let (send, events) = collector();
    let agent_target = target(PaneChatSubscriptionDetail::Agent, false);
    given.subscribe(given.owner(), &[agent_target], send);
    given.timers.advance(0.0);
    assert_eq!(*events.lock().unwrap(), [provider_state(None)]);
    assert_eq!(given.source.read_count(), 0);

    events.lock().unwrap().clear();
    given.timers.advance(3_000.0);
    assert!(events.lock().unwrap().is_empty());
    *given.source.provider.lock().unwrap() = Some(AiSessionProvider::Codex);
    given.timers.advance(3_000.0);
    assert_eq!(
        *events.lock().unwrap(),
        [provider_state(Some(AiSessionProvider::Codex))]
    );
}

#[test]
fn switching_to_another_session_pushes_only_a_replaced_signal() {
    let given = harness();
    let (send, events) = collector();
    given.serve(snapshot_of(&["a", "b"]), None);
    given.subscribe(given.owner(), &[chat_target()], send);
    given.timers.advance(0.0);
    events.lock().unwrap().clear();
    let mut other_session = snapshot_of(&["x"]);
    other_session.transcript.session_id = Some("session-b".into());
    given.serve(other_session, None);
    given.timers.advance(1_000.0);
    let non_state: Vec<_> = events
        .lock()
        .unwrap()
        .iter()
        .filter(|event| !matches!(event, PaneChatPushEvent::State { .. }))
        .cloned()
        .collect();
    assert_eq!(non_state.len(), 1);
    let PaneChatPushEvent::Replaced {
        task_id,
        pane_id,
        epoch,
    } = &non_state[0]
    else {
        panic!("교체 신호가 아님: {:?}", non_state[0]);
    };
    assert_eq!((task_id.as_str(), pane_id.as_str()), ("task-1", "%1"));
    assert!(!epoch.is_empty());
}

#[test]
fn cursor_is_current_only_at_the_snapshot_end() {
    let given = harness();
    given.serve(snapshot_of(&["a", "b"]), None);
    let snapshot = given
        .service
        .get_sequenced_pane_chat_snapshot("task-1", Some("%1"), None)
        .unwrap();
    assert_eq!(snapshot.transcript.seq, Some(2));
    let epoch = snapshot.transcript.epoch.unwrap();
    let freshness_of = |cursor: Option<PaneChatCursor>| {
        let request = PaneChatSubscriptionRequest {
            target: chat_target(),
            cursor,
        };
        given
            .service
            .set_pane_chat_subscriptions(given.owner(), &[request], ignore())[0]
            .freshness
    };
    let cursor = |epoch: &str, seq| {
        Some(PaneChatCursor {
            epoch: epoch.into(),
            seq,
        })
    };
    assert_eq!(freshness_of(cursor(&epoch, 2)), PaneChatFreshness::Current);
    assert_eq!(freshness_of(cursor(&epoch, 1)), PaneChatFreshness::Behind);
    assert_eq!(freshness_of(cursor("other", 2)), PaneChatFreshness::Behind);
    assert_eq!(freshness_of(None), PaneChatFreshness::Behind);
}

#[test]
fn rows_found_first_by_http_snapshot_still_reach_subscribers_once() {
    let given = harness();
    let (send, events) = collector();
    given.serve(snapshot_of(&["a"]), None);
    given.subscribe(given.owner(), &[chat_target()], send);
    given.timers.advance(0.0);
    events.lock().unwrap().clear();
    given.serve(snapshot_of(&["a", "b"]), None);
    given
        .service
        .get_sequenced_pane_chat_snapshot("task-1", Some("%1"), None)
        .unwrap();
    assert_eq!(row_texts(&events), ["2:b"]);
    given.timers.advance(1_000.0);
    assert_eq!(row_texts(&events), ["2:b"]);
}
