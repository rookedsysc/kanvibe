use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use kanvibe_domain::notifications::{
    BackgroundSyncPulledTaskPayload, BackgroundSyncRegisteredWorktreePayload,
    TaskPrMergedDetectedPayload,
};

use super::timer::{TimerCallback, TimerId};
use super::*;
use crate::board_notifier::BoardEvent;
use crate::test_support::{RecordedBoardEvents, wait_until};

/// 테스트가 시간을 직접 넘기는 타이머(TS `vi.useFakeTimers`). 발화한 예약은 새 스레드에서 돌려 막힌 단계가 테스트를 막지 않게 한다
#[derive(Default)]
struct ManualTimer {
    now: Mutex<Duration>,
    next_id: AtomicU64,
    scheduled: Mutex<Vec<(TimerId, Duration, TimerCallback)>>,
}

impl ManualTimer {
    fn pending_count(&self) -> usize {
        self.scheduled.lock().unwrap().len()
    }

    fn take_due(&self, target: Duration) -> Option<TimerCallback> {
        let mut scheduled = self.scheduled.lock().unwrap();
        let index = scheduled
            .iter()
            .enumerate()
            .filter(|(_, (_, due, _))| *due <= target)
            .min_by_key(|(_, (_, due, _))| *due)
            .map(|(index, _)| index)?;
        let (_, due, callback) = scheduled.remove(index);
        *self.now.lock().unwrap() = due;
        Some(callback)
    }

    fn advance(&self, by: Duration) {
        let target = *self.now.lock().unwrap() + by;
        while let Some(callback) = self.take_due(target) {
            std::thread::spawn(callback);
        }
        *self.now.lock().unwrap() = target;
    }
}

impl SyncTimer for ManualTimer {
    fn schedule(&self, delay: Duration, callback: TimerCallback) -> TimerId {
        let timer_id = TimerId(self.next_id.fetch_add(1, Ordering::SeqCst));
        let due = *self.now.lock().unwrap() + delay;
        self.scheduled
            .lock()
            .unwrap()
            .push((timer_id, due, callback));
        timer_id
    }

    fn cancel(&self, timer_id: TimerId) {
        self.scheduled
            .lock()
            .unwrap()
            .retain(|(id, _, _)| *id != timer_id);
    }
}

/// 단계 호출을 기록하고, 정해 둔 결과를 돌려준다. 문이 걸린 단계는 테스트가 열어 줄 때까지 끝나지 않는다
#[derive(Default)]
struct FakeStages {
    calls: Mutex<Vec<String>>,
    status_result: Mutex<Option<Result<TaskStatusSyncResult, String>>>,
    worktree_results: Mutex<VecDeque<Result<WorktreeSyncOutcome, String>>>,
    pull_request_result: Mutex<Option<Result<PullRequestSyncOutcome, String>>>,
    pull_result: Mutex<Option<Result<TaskPullSyncOutcome, String>>>,
    worktree_gate: Mutex<Option<mpsc::Receiver<()>>>,
    pull_request_gate: Mutex<Option<mpsc::Receiver<()>>>,
}

impl FakeStages {
    fn record(&self, call: &str) {
        self.calls.lock().unwrap().push(call.to_string());
    }

    fn count(&self, name: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.as_str() == name)
            .count()
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn gate_worktrees(&self) -> mpsc::Sender<()> {
        let (sender, receiver) = mpsc::channel();
        *self.worktree_gate.lock().unwrap() = Some(receiver);
        sender
    }

    fn gate_pull_requests(&self) -> mpsc::Sender<()> {
        let (sender, receiver) = mpsc::channel();
        *self.pull_request_gate.lock().unwrap() = Some(receiver);
        sender
    }
}

fn wait_for_gate(gate: &Mutex<Option<mpsc::Receiver<()>>>) {
    let receiver = gate.lock().unwrap().take();
    if let Some(receiver) = receiver {
        receiver.recv().unwrap();
    }
}

impl BackgroundSyncStages for FakeStages {
    fn sync_task_statuses_from_files(&self) -> Result<TaskStatusSyncResult, String> {
        self.record("status");
        self.status_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(TaskStatusSyncResult::default()))
    }

    fn sync_registered_project_worktrees(&self) -> Result<WorktreeSyncOutcome, String> {
        self.record("worktree:start");
        wait_for_gate(&self.worktree_gate);
        self.record("worktree:end");
        let mut results = self.worktree_results.lock().unwrap();
        let result = results
            .front()
            .cloned()
            .unwrap_or(Ok(WorktreeSyncOutcome::default()));
        if results.len() > 1 {
            results.pop_front();
        }
        result
    }

    fn sync_active_task_pull_requests(
        &self,
        _keys: &mut HashSet<String>,
    ) -> Result<PullRequestSyncOutcome, String> {
        self.record("pr:start");
        wait_for_gate(&self.pull_request_gate);
        self.record("pr:end");
        self.pull_request_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(PullRequestSyncOutcome::default()))
    }

    fn sync_active_task_pulls(&self) -> Result<TaskPullSyncOutcome, String> {
        self.record("pull:start");
        self.pull_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(Ok(TaskPullSyncOutcome::default()))
    }
}

struct FakeSettings {
    is_enabled: Mutex<bool>,
    interval_ms: Mutex<i64>,
    enabled_reads: AtomicUsize,
    interval_reads: AtomicUsize,
}

impl FakeSettings {
    fn new(is_enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            is_enabled: Mutex::new(is_enabled),
            interval_ms: Mutex::new(10 * 60_000),
            enabled_reads: AtomicUsize::new(0),
            interval_reads: AtomicUsize::new(0),
        })
    }
}

impl BackgroundSyncSettings for FakeSettings {
    fn is_enabled(&self) -> Result<bool, String> {
        self.enabled_reads.fetch_add(1, Ordering::SeqCst);
        Ok(*self.is_enabled.lock().unwrap())
    }

    fn interval_ms(&self) -> Result<i64, String> {
        self.interval_reads.fetch_add(1, Ordering::SeqCst);
        Ok(*self.interval_ms.lock().unwrap())
    }

    fn save_enabled(&self, is_enabled: bool) -> Result<(), String> {
        *self.is_enabled.lock().unwrap() = is_enabled;
        Ok(())
    }

    fn save_interval_ms(&self, interval_ms: i64) -> Result<(), String> {
        *self.interval_ms.lock().unwrap() = interval_ms;
        Ok(())
    }
}

struct Fixture {
    stages: Arc<FakeStages>,
    settings: Arc<FakeSettings>,
    timer: Arc<ManualTimer>,
    events: RecordedBoardEvents,
    service: BackgroundTaskSync,
}

fn fixture_with(is_enabled: bool) -> Fixture {
    let stages = Arc::new(FakeStages::default());
    let settings = FakeSettings::new(is_enabled);
    let timer = Arc::new(ManualTimer::default());
    let board = BoardNotifier::new();
    let events = RecordedBoardEvents::subscribe(&board);
    let service = BackgroundTaskSync::new(
        Arc::clone(&stages) as Arc<dyn BackgroundSyncStages>,
        Arc::clone(&settings) as Arc<dyn BackgroundSyncSettings>,
        board,
        Arc::clone(&timer) as Arc<dyn SyncTimer>,
    );
    Fixture {
        stages,
        settings,
        timer,
        events,
        service,
    }
}

fn fixture() -> Fixture {
    fixture_with(true)
}

const INITIAL: Duration = Duration::from_millis(20_000);
const TEN_MINUTES: Duration = Duration::from_millis(10 * 60_000);

impl Fixture {
    fn review_payloads(&self) -> Vec<BackgroundSyncReviewPayload> {
        self.events
            .events()
            .into_iter()
            .filter_map(|event| match event {
                BoardEvent::BackgroundSyncReviewNeeded(review) => Some(review),
                _ => None,
            })
            .collect()
    }

    fn is_cycle_finished(&self) -> bool {
        !self.service.get_background_task_sync_status().is_running
            && self.stages.count("pull:start") > 0
    }

    /// 예약 사이클이 끝나고 다음 예약까지 걸릴 때까지 기다린다
    fn wait_for_next_schedule(&self, cycles: usize) {
        wait_until(|| self.stages.count("pull:start") == cycles && self.timer.pending_count() == 1);
    }
}

fn pulled_task() -> BackgroundSyncPulledTaskPayload {
    BackgroundSyncPulledTaskPayload {
        task_id: "task-pull".into(),
        task_title: "Pull target".into(),
        branch_name: "feature/pull".into(),
        worktree_path: "/workspace/repo__worktrees/feature-pull".into(),
        ssh_host: None,
        status: PulledTaskStatus::Updated,
        summary: "Fast-forward".into(),
    }
}

#[test]
fn shares_one_run_across_clients_and_exposes_progress() {
    let fixture = fixture();
    let release = fixture.stages.gate_worktrees();
    assert!(!fixture.service.get_background_task_sync_status().is_running);

    std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.service.run_background_task_sync_now().unwrap());
        wait_until(|| fixture.stages.count("worktree:start") == 1);
        let second = scope.spawn(|| fixture.service.run_background_task_sync_now().unwrap());
        // 진행 중인 사이클은 자리 하나와 시작한 쪽이 붙잡는다. 두 번째 호출이 합류하면 하나 더 늘어난다
        wait_until(|| {
            lock(&fixture.service.shared.active_cycle)
                .as_ref()
                .is_some_and(|cycle| Arc::strong_count(cycle) >= 3)
        });
        let started = fixture.service.start_background_task_sync_now();
        assert!(started.is_running);
        assert_eq!(
            (started.phase, started.completed_stages),
            (BackgroundTaskSyncPhase::Worktrees, 0)
        );
        release.send(()).unwrap();
        assert_eq!(first.join().unwrap(), second.join().unwrap());
    });

    let status = fixture.service.get_background_task_sync_status();
    assert_eq!(
        (
            status.is_running,
            status.phase,
            status.completed_stages,
            status.review_needed
        ),
        (false, BackgroundTaskSyncPhase::Completed, 3, false)
    );
    for stage in ["status", "worktree:start", "pr:start", "pull:start"] {
        assert_eq!(fixture.stages.count(stage), 1, "{stage}");
    }
    fixture.service.run_background_task_sync_now().unwrap();
    assert_eq!(fixture.stages.count("worktree:start"), 2);
}

#[test]
fn a_status_only_correction_updates_the_board_and_status_reads_do_not_sync() {
    let fixture = fixture();
    *fixture.stages.status_result.lock().unwrap() = Some(Ok(TaskStatusSyncResult {
        has_changed: true,
        errors: Vec::new(),
    }));

    fixture.service.get_background_task_sync_status();
    assert_eq!(fixture.stages.count("status"), 0);
    let result = fixture.service.run_background_task_sync_now().unwrap();

    assert_eq!(
        result,
        BackgroundTaskSyncRunResult {
            board_updated: true,
            review_needed: false
        }
    );
    assert_eq!(fixture.stages.count("status"), 1);
    assert_eq!(fixture.events.board_update_count(), 1);
}

#[test]
fn reports_status_file_read_failures_and_continues() {
    let fixture = fixture();
    *fixture.stages.status_result.lock().unwrap() = Some(Ok(TaskStatusSyncResult {
        has_changed: false,
        errors: vec!["host-a 상태 읽기 실패".into()],
    }));

    let result = fixture.service.run_background_task_sync_now().unwrap();

    assert!(result.review_needed);
    assert_eq!(
        (
            fixture.stages.count("pr:start"),
            fixture.stages.count("pull:start")
        ),
        (1, 1)
    );
    let failures = fixture.review_payloads()[0].failures.clone().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].reason, "host-a 상태 읽기 실패");
}

#[test]
fn a_failed_status_sync_does_not_stop_the_other_stages() {
    let fixture = fixture();
    *fixture.stages.status_result.lock().unwrap() = Some(Err("snapshot unavailable".into()));

    assert!(
        fixture
            .service
            .run_background_task_sync_now()
            .unwrap()
            .review_needed
    );

    assert_eq!(fixture.stages.count("worktree:start"), 1);
    let failures = fixture.review_payloads()[0].failures.clone().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].reason, "snapshot unavailable");
}

fn registered_worktree() -> BackgroundSyncRegisteredWorktreePayload {
    BackgroundSyncRegisteredWorktreePayload {
        task_id: "task-worktree".into(),
        project_name: "api".into(),
        branch_name: "feature-sync".into(),
        worktree_path: "/workspace/api__worktrees/feature-sync".into(),
        ssh_host: None,
    }
}

fn merged_pull_request() -> TaskPrMergedDetectedPayload {
    TaskPrMergedDetectedPayload {
        task_id: "task-10".into(),
        task_title: "Merged PR task".into(),
        branch_name: "feature/merged-pr".into(),
        pr_url: "https://github.com/kanvibe/kanvibe/pull/211".into(),
        merged_at: "2026-04-30T02:00:00Z".into(),
    }
}

#[test]
fn broadcasts_one_review_event_for_all_review_targets() {
    let fixture = fixture();
    fixture
        .stages
        .worktree_results
        .lock()
        .unwrap()
        .push_back(Ok(WorktreeSyncOutcome {
            registered_worktrees: vec![registered_worktree()],
            errors: Vec::new(),
            has_changed: true,
        }));
    *fixture.stages.pull_request_result.lock().unwrap() = Some(Ok(PullRequestSyncOutcome {
        merged_pull_requests: vec![merged_pull_request()],
        ..PullRequestSyncOutcome::default()
    }));

    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.is_cycle_finished());

    assert_eq!(
        fixture.review_payloads(),
        [BackgroundSyncReviewPayload {
            registered_worktrees: vec![registered_worktree()],
            merged_pull_requests: vec![merged_pull_request()],
            pulled_tasks: Some(Vec::new()),
            failures: None,
        }]
    );
    fixture.service.stop_background_task_sync();
}

#[test]
fn manual_sync_runs_the_same_cycle_even_when_disabled() {
    let fixture = fixture_with(false);

    fixture.service.run_background_task_sync_now().unwrap();

    assert_eq!(fixture.settings.enabled_reads.load(Ordering::SeqCst), 0);
    for stage in ["worktree:start", "pr:start", "pull:start"] {
        assert_eq!(fixture.stages.count(stage), 1, "{stage}");
    }
}

#[test]
fn manual_sync_joins_a_scheduled_cycle_in_flight() {
    let fixture = fixture();
    let release = fixture.stages.gate_worktrees();
    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.stages.count("worktree:start") == 1);

    std::thread::scope(|scope| {
        let manual = scope.spawn(|| fixture.service.run_background_task_sync_now().unwrap());
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(fixture.stages.count("pr:start"), 0);
        release.send(()).unwrap();
        manual.join().unwrap();
    });

    for stage in ["worktree:start", "pr:start", "pull:start"] {
        assert_eq!(fixture.stages.count(stage), 1, "{stage}");
    }
    fixture.service.stop_background_task_sync();
}

#[test]
fn starting_twice_runs_one_loop() {
    let fixture = fixture();

    fixture.service.start_background_task_sync();
    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.is_cycle_finished());

    for stage in ["worktree:start", "pr:start", "pull:start"] {
        assert_eq!(fixture.stages.count(stage), 1, "{stage}");
    }
    assert_eq!(fixture.settings.enabled_reads.load(Ordering::SeqCst), 1);
    fixture.service.stop_background_task_sync();
}

#[test]
fn a_running_cycle_blocks_additional_cycles() {
    let fixture = fixture();
    let release = fixture.stages.gate_worktrees();
    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.stages.count("worktree:start") == 1);

    fixture.timer.advance(TEN_MINUTES);
    assert_eq!(fixture.stages.count("worktree:start"), 1);
    assert_eq!(fixture.stages.count("pr:start"), 0);
    release.send(()).unwrap();
    fixture.wait_for_next_schedule(1);
    fixture.timer.advance(TEN_MINUTES);
    fixture.wait_for_next_schedule(2);

    for stage in ["worktree:start", "pr:start", "pull:start"] {
        assert_eq!(fixture.stages.count(stage), 2, "{stage}");
    }
    fixture.service.stop_background_task_sync();
}

#[test]
fn runs_expensive_stages_one_after_another() {
    let fixture = fixture();
    let release_worktrees = fixture.stages.gate_worktrees();
    let release_pull_requests = fixture.stages.gate_pull_requests();
    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.stages.count("worktree:start") == 1);
    std::thread::sleep(Duration::from_millis(20));

    let without_status = |calls: Vec<String>| {
        calls
            .into_iter()
            .filter(|call| call != "status")
            .collect::<Vec<_>>()
    };
    assert_eq!(without_status(fixture.stages.calls()), ["worktree:start"]);
    release_worktrees.send(()).unwrap();
    wait_until(|| fixture.stages.count("pr:start") == 1);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(
        without_status(fixture.stages.calls()),
        ["worktree:start", "worktree:end", "pr:start"]
    );
    release_pull_requests.send(()).unwrap();
    wait_until(|| fixture.stages.count("pull:start") == 1);

    assert_eq!(
        without_status(fixture.stages.calls()),
        [
            "worktree:start",
            "worktree:end",
            "pr:start",
            "pr:end",
            "pull:start"
        ]
    );
    fixture.service.stop_background_task_sync();
}

#[test]
fn a_failed_worktree_stage_still_runs_pull_request_and_pull_sync() {
    let fixture = fixture();
    fixture
        .stages
        .worktree_results
        .lock()
        .unwrap()
        .push_back(Err("project repository unavailable".into()));
    *fixture.stages.pull_request_result.lock().unwrap() = Some(Ok(PullRequestSyncOutcome {
        updated_task_ids: vec!["task-pr".into()],
        ..PullRequestSyncOutcome::default()
    }));
    *fixture.stages.pull_result.lock().unwrap() = Some(Ok(TaskPullSyncOutcome {
        pulled_tasks: vec![pulled_task()],
    }));

    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.is_cycle_finished());

    assert_eq!(
        fixture.review_payloads(),
        [BackgroundSyncReviewPayload {
            registered_worktrees: Vec::new(),
            merged_pull_requests: Vec::new(),
            pulled_tasks: Some(vec![pulled_task()]),
            failures: Some(vec![failure(
                BackgroundSyncOperation::WorktreeSync,
                "등록 프로젝트 worktree sync",
                "project repository unavailable".into()
            )]),
        }]
    );
    assert_eq!(fixture.events.board_update_count(), 1);
    fixture.service.stop_background_task_sync();
}

#[test]
fn an_interval_change_during_manual_sync_reschedules_with_the_new_interval() {
    let fixture = fixture();
    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    fixture.wait_for_next_schedule(1);

    let release = fixture.stages.gate_worktrees();
    std::thread::scope(|scope| {
        let manual = scope.spawn(|| fixture.service.run_background_task_sync_now().unwrap());
        wait_until(|| fixture.stages.count("worktree:start") == 2);
        fixture
            .service
            .set_background_sync_interval_ms(60_000)
            .unwrap();
        release.send(()).unwrap();
        manual.join().unwrap();
    });
    assert_eq!(fixture.stages.count("worktree:start"), 2);
    fixture.timer.advance(Duration::from_millis(60_000));
    fixture.wait_for_next_schedule(3);

    assert_eq!(fixture.stages.count("worktree:start"), 3);
    fixture.service.stop_background_task_sync();
}

#[test]
fn repeats_every_ten_minutes_after_the_first_run() {
    let fixture = fixture();
    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    fixture.wait_for_next_schedule(1);

    fixture
        .timer
        .advance(TEN_MINUTES - Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(fixture.stages.count("worktree:start"), 1);
    fixture.timer.advance(Duration::from_millis(1));
    fixture.wait_for_next_schedule(2);

    for stage in ["worktree:start", "pr:start", "pull:start"] {
        assert_eq!(fixture.stages.count(stage), 2, "{stage}");
    }
    fixture.service.stop_background_task_sync();
}

#[test]
fn includes_task_pull_results_in_the_review_event() {
    let fixture = fixture();
    *fixture.stages.pull_result.lock().unwrap() = Some(Ok(TaskPullSyncOutcome {
        pulled_tasks: vec![pulled_task()],
    }));

    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.is_cycle_finished());

    assert_eq!(
        fixture.review_payloads(),
        [BackgroundSyncReviewPayload {
            registered_worktrees: Vec::new(),
            merged_pull_requests: Vec::new(),
            pulled_tasks: Some(vec![pulled_task()]),
            failures: None,
        }]
    );
    fixture.service.stop_background_task_sync();
}

#[test]
fn failures_alone_trigger_a_review_event_with_target_and_reason() {
    let fixture = fixture();
    fixture
        .stages
        .worktree_results
        .lock()
        .unwrap()
        .push_back(Ok(WorktreeSyncOutcome {
            errors: vec!["api worktree 스캔 실패: git fetch failed".into()],
            ..WorktreeSyncOutcome::default()
        }));
    let pull_request_failure = BackgroundSyncFailurePayload {
        operation: BackgroundSyncOperation::PullRequestSync,
        target: "PR sync target (feature/pr-fail)".into(),
        reason: "gh auth failed".into(),
        task_id: Some("task-11".into()),
        branch_name: Some("feature/pr-fail".into()),
        ssh_host: None,
    };
    *fixture.stages.pull_request_result.lock().unwrap() = Some(Ok(PullRequestSyncOutcome {
        failures: vec![pull_request_failure.clone()],
        ..PullRequestSyncOutcome::default()
    }));

    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.is_cycle_finished());

    let review = &fixture.review_payloads()[0];
    assert_eq!(
        serde_json::to_value(review).unwrap(),
        serde_json::json!({
            "registeredWorktrees": [],
            "mergedPullRequests": [],
            "pulledTasks": [],
            "failures": [
                { "operation": "worktree-sync", "target": "등록 프로젝트 worktree sync", "reason": "api worktree 스캔 실패: git fetch failed" },
                { "operation": "pull-request-sync", "target": "PR sync target (feature/pr-fail)", "reason": "gh auth failed", "taskId": "task-11", "branchName": "feature/pr-fail" },
            ],
        })
    );
    fixture.service.stop_background_task_sync();
}

#[test]
fn waits_without_timers_while_disabled_and_still_runs_manual_sync() {
    let fixture = fixture_with(false);

    fixture.service.start_background_task_sync();
    assert_eq!(fixture.timer.pending_count(), 0);
    fixture.timer.advance(Duration::from_millis(30 * 60_000));

    assert_eq!(fixture.settings.enabled_reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.settings.interval_reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.stages.count("worktree:start"), 0);
    fixture.service.run_background_task_sync_now().unwrap();
    assert_eq!(fixture.stages.count("worktree:start"), 1);
    assert_eq!(fixture.timer.pending_count(), 0);
    fixture.service.stop_background_task_sync();
}

#[test]
fn disabling_cancels_the_schedule_and_ignores_interval_changes_while_off() {
    let fixture = fixture();
    fixture.service.start_background_task_sync();

    fixture.service.set_background_sync_enabled(false).unwrap();
    fixture
        .service
        .set_background_sync_interval_ms(60_000)
        .unwrap();
    assert_eq!(fixture.timer.pending_count(), 0);
    fixture.timer.advance(Duration::from_millis(60_000));
    assert_eq!(fixture.stages.count("worktree:start"), 0);
    fixture.service.set_background_sync_enabled(true).unwrap();
    fixture.timer.advance(INITIAL - Duration::from_millis(1));
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(fixture.stages.count("worktree:start"), 0);
    fixture.timer.advance(Duration::from_millis(1));

    wait_until(|| fixture.stages.count("worktree:start") == 1);
    fixture.service.stop_background_task_sync();
}

#[test]
fn disabling_during_a_run_does_not_schedule_the_next_one() {
    let fixture = fixture();
    let release = fixture.stages.gate_worktrees();
    fixture.service.start_background_task_sync();
    fixture.timer.advance(INITIAL);
    wait_until(|| fixture.stages.count("worktree:start") == 1);

    fixture.service.set_background_sync_enabled(false).unwrap();
    release.send(()).unwrap();
    wait_until(|| fixture.is_cycle_finished());
    std::thread::sleep(Duration::from_millis(20));

    assert_eq!(fixture.timer.pending_count(), 0);
    assert_eq!(fixture.settings.interval_reads.load(Ordering::SeqCst), 0);
    fixture.service.stop_background_task_sync();
}

#[test]
fn the_latest_user_setting_wins_over_the_startup_setting() {
    let fixture = fixture();

    fixture.service.start_background_task_sync();
    fixture.service.set_background_sync_enabled(false).unwrap();

    assert_eq!(fixture.timer.pending_count(), 0);
    fixture.service.stop_background_task_sync();
}

#[test]
fn the_thread_timer_fires_once_and_honours_cancellation() {
    let timer = timer::ThreadSyncTimer::default();
    let fired = Arc::new(AtomicUsize::new(0));
    let fired_by_first = Arc::clone(&fired);
    let fired_by_second = Arc::clone(&fired);

    timer.schedule(
        Duration::from_millis(5),
        Box::new(move || {
            fired_by_first.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let cancelled = timer.schedule(
        Duration::from_millis(50),
        Box::new(move || {
            fired_by_second.fetch_add(10, Ordering::SeqCst);
        }),
    );
    timer.cancel(cancelled);
    std::thread::sleep(Duration::from_millis(100));

    assert_eq!(fired.load(Ordering::SeqCst), 1);
}
