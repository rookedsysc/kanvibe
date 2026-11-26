//! Herdr pane 화면을 `terminal session observe` 프레임으로 따라간다(TS `startHerdrFrameStream`).
//!
//! 변경 프레임은 화면 좌표에 글자를 덮어쓰기만 해서, 위로 밀려난 줄이 모바일 스크롤에 쌓이지 않는다.
//! 그래서 출력이 잠잠해졌을 때 지난 출력 줄 수가 늘었으면 이력을 다시 보내고 observe를 새로 띄운다.
//! 새 observe의 첫 프레임은 항상 전체 화면이라, 이력을 받은 화면과 커서를 끊김 없이 맞출 수 있다.
//! Herdr의 스크롤백이 상한에 닿거나 프로그램이 제자리에서 다시 그리면 줄 수가 늘지 않는다.
//! 그런 이력은 모바일이 위로 올려 보기 시작할 때 요청하는 갱신으로 맞춘다.

use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use kanvibe_terminal::herdr::HerdrPaneExtent;
use kanvibe_terminal::pane_mirror::{
    HerdrObserveRecord, MirrorPane, build_herdr_observe_pane_command,
    build_herdr_remote_observe_pane_command, parse_herdr_observe_record,
};

use crate::error::{MirrorResult, PaneMirrorError};
use crate::mirror::{MirrorSessionTarget, MirrorShared, lock};
use crate::process::{LineSplitter, WatchedProcess};
use crate::subscription::{PaneStream, StreamAudience};

/// 원격 observe는 SSH 연결이 끊겨도 원격에 남으므로, SSH stdin이 닫히면 끝나는 명령으로 띄운다. 로컬 observe는 그대로 띄운다
pub(crate) fn build_observe_command(
    target: &MirrorSessionTarget,
    pane_id: &str,
    cols: i64,
    rows: i64,
) -> Result<String, String> {
    match target.ssh_host() {
        Some(_) => {
            build_herdr_remote_observe_pane_command(&target.session_name, pane_id, cols, rows)
        }
        None => build_herdr_observe_pane_command(&target.session_name, pane_id, cols, rows),
    }
}

pub(crate) fn start_herdr_frame_stream(
    shared: &Arc<MirrorShared>,
    target: &MirrorSessionTarget,
    pane: &MirrorPane,
    audience: &Arc<StreamAudience>,
    on_dead: Arc<dyn Fn() + Send + Sync>,
) -> MirrorResult<Arc<dyn PaneStream>> {
    let extent =
        shared
            .herdr
            .read_pane_extent(&target.session_name, &pane.id, target.ssh_host())?;
    let stream = Arc::new_cyclic(|weak_self| HerdrFrameStream {
        weak_self: weak_self.clone(),
        shared: Arc::downgrade(shared),
        target: target.clone(),
        pane_id: pane.id.clone(),
        // 모바일이 크기를 정하기 전에 비출 pane 본래 크기. observe를 띄울 때마다 pane 목록을 다시 읽지 않는다
        pane_size: (pane.width, pane.height),
        quiet_period: shared.timings.dump_interval(target.ssh_host()),
        refresh_min_interval: shared.timings.herdr_history_refresh_min_interval,
        audience: Arc::clone(audience),
        on_dead,
        state: Mutex::new(HerdrStreamState {
            observer: None,
            observer_generation: 0,
            is_stopped: false,
            is_refreshing: false,
            viewport_size: None,
            extent,
            last_requested_refresh_at: None,
        }),
        quiet_timer: Arc::new(QuietTimer::default()),
    });
    stream.start_observer()?;
    stream.start_quiet_timer();
    stream.schedule_quiet_check();
    Ok(stream)
}

struct HerdrStreamState {
    /// 지금 화면을 보내는 observe와 그 세대. 세대가 다른 observe의 프레임과 종료는 무시한다
    observer: Option<(u64, Arc<WatchedProcess>)>,
    observer_generation: u64,
    is_stopped: bool,
    is_refreshing: bool,
    viewport_size: Option<(i64, i64)>,
    extent: HerdrPaneExtent,
    last_requested_refresh_at: Option<Instant>,
}

struct HerdrFrameStream {
    weak_self: Weak<HerdrFrameStream>,
    shared: Weak<MirrorShared>,
    target: MirrorSessionTarget,
    pane_id: String,
    pane_size: (i64, i64),
    quiet_period: Duration,
    refresh_min_interval: Duration,
    audience: Arc<StreamAudience>,
    on_dead: Arc<dyn Fn() + Send + Sync>,
    state: Mutex<HerdrStreamState>,
    quiet_timer: Arc<QuietTimer>,
}

impl HerdrFrameStream {
    fn shared(&self) -> MirrorResult<Arc<MirrorShared>> {
        self.shared
            .upgrade()
            .ok_or_else(|| PaneMirrorError::Failed("데스크탑이 종료 중입니다.".to_string()))
    }

    fn start_observer(&self) -> MirrorResult<()> {
        let (generation, size) = {
            let mut state = lock(&self.state);
            state.observer_generation += 1;
            let size = state.viewport_size.unwrap_or(self.pane_size);
            (state.observer_generation, size)
        };
        let command = build_observe_command(&self.target, &self.pane_id, size.0, size.1)?;
        let process = self
            .shared()?
            .processes
            .spawn(&command, self.target.ssh_host())?;
        // 프레임을 읽는 스레드가 이 observe를 알아보기 전에 첫 프레임을 버리지 않도록 잠금을 쥔 채 붙인다
        let mut state = lock(&self.state);
        let weak_output = self.weak_self.clone();
        let weak_exit = self.weak_self.clone();
        let mut lines = LineSplitter::default();
        let child = WatchedProcess::start(
            process,
            move |bytes| {
                let Some(stream) = weak_output.upgrade() else {
                    return;
                };
                for line in lines.push(bytes) {
                    stream.handle_record(generation, &line);
                }
            },
            move || {
                if let Some(stream) = weak_exit.upgrade() {
                    stream.handle_observer_end(generation);
                }
            },
        );
        if state.is_stopped || generation != state.observer_generation {
            child.stop();
            return Ok(());
        }
        // 재시작이 겹치면 먼저 뜬 observe가 주인 없이 남으므로 항상 하나만 남긴다
        if let Some((_, previous)) = state.observer.replace((generation, child)) {
            previous.stop();
        }
        Ok(())
    }

    /// 먼저 비워야 곧이어 오는 종료가 스트림의 죽음으로 읽히지 않는다
    fn stop_observer(&self) {
        let stopping = lock(&self.state).observer.take();
        if let Some((_, observer)) = stopping {
            observer.stop();
        }
    }

    fn restart_observer(&self) -> MirrorResult<()> {
        self.stop_observer();
        self.start_observer()
    }

    fn handle_record(&self, generation: u64, line: &str) {
        let is_current = lock(&self.state)
            .observer
            .as_ref()
            .is_some_and(|(current, _)| *current == generation);
        let Some(HerdrObserveRecord::Frame {
            is_full_screen,
            output,
            ..
        }) = parse_herdr_observe_record(line)
        else {
            return;
        };
        if !is_current {
            return;
        }
        self.audience.broadcast(&output);
        // 전체 프레임은 observe가 시작하며 보내는 것이라 새 출력이 아니다. 여기서도 예약하면 재시작이 끝없이 반복된다
        if !is_full_screen {
            self.schedule_quiet_check();
        }
    }

    fn handle_observer_end(&self, generation: u64) {
        let is_current = lock(&self.state)
            .observer
            .as_ref()
            .is_some_and(|(current, _)| *current == generation);
        if is_current {
            self.end_stream();
        }
    }

    fn stop_stream(&self) {
        {
            let mut state = lock(&self.state);
            state.is_stopped = true;
            state.observer_generation += 1;
        }
        self.quiet_timer.stop();
        self.stop_observer();
    }

    /// 끝을 알리면 구독 자리가 먼저 지워져 해제 쪽이 `stop`을 부르지 않으므로, 알리기 전에 스스로 멈춘다
    fn end_stream(&self) {
        self.stop_stream();
        (self.on_dead)();
    }

    fn fail_on_restart_error(&self, error: &PaneMirrorError) {
        eprintln!("[kanvibe] Herdr observe restart failed: {error}");
        if !lock(&self.state).is_stopped {
            self.end_stream();
        }
    }

    fn is_stopped(&self) -> bool {
        lock(&self.state).is_stopped
    }

    fn refresh_history(&self, is_requested: bool) {
        {
            let mut state = lock(&self.state);
            if state.is_refreshing {
                return;
            }
            state.is_refreshing = true;
        }
        if let Err(error) = self.resend_history(is_requested) {
            self.fail_on_restart_error(&error);
        }
        lock(&self.state).is_refreshing = false;
    }

    fn resend_history(&self, is_requested: bool) -> MirrorResult<()> {
        let shared = self.shared()?;
        let next_extent = shared.herdr.read_pane_extent(
            &self.target.session_name,
            &self.pane_id,
            self.target.ssh_host(),
        )?;
        let is_unchanged = lock(&self.state).extent == next_extent;
        if self.is_stopped() || (is_unchanged && !is_requested) {
            return Ok(());
        }
        let snapshot = shared.read_pane_snapshot(&self.target, &self.pane_id)?;
        if self.is_stopped() {
            return Ok(());
        }
        lock(&self.state).extent = next_extent;
        self.stop_observer();
        self.audience.broadcast(&snapshot);
        if !self.is_stopped() {
            self.start_observer()?;
        }
        Ok(())
    }

    fn schedule_quiet_check(&self) {
        self.quiet_timer
            .schedule(Instant::now() + self.quiet_period);
    }

    fn start_quiet_timer(&self) {
        let timer = Arc::clone(&self.quiet_timer);
        let weak_stream = self.weak_self.clone();
        std::thread::spawn(move || {
            while timer.wait_until_due() {
                match weak_stream.upgrade() {
                    Some(stream) => stream.refresh_history(false),
                    None => return,
                }
            }
        });
    }
}

/// 멈추지 않고 버려져도 타이머 스레드와 observe가 남지 않게 한다
impl Drop for HerdrFrameStream {
    fn drop(&mut self) {
        self.quiet_timer.stop();
        if let Some((_, observer)) = lock(&self.state).observer.take() {
            observer.stop();
        }
    }
}

impl PaneStream for HerdrFrameStream {
    fn stop(&self) {
        self.stop_stream();
    }

    /// 뒤늦게 붙은 구독자에게 전체 화면을 주려면 observe를 새로 띄우는 수밖에 없다. 이미 보던 기기에는 같은 화면이 한 번 더 그려질 뿐이다
    fn resend_screen(&self) {
        if let Err(error) = self.restart_observer() {
            self.fail_on_restart_error(&error);
        }
    }

    fn set_viewport(&self, size: Option<(i64, i64)>) -> MirrorResult<()> {
        lock(&self.state).viewport_size = size;
        if self.is_stopped() {
            return Ok(());
        }
        self.restart_observer()
    }

    fn refresh_history(&self) {
        let now = Instant::now();
        {
            let mut state = lock(&self.state);
            let is_too_soon = state
                .last_requested_refresh_at
                .is_some_and(|last| now.duration_since(last) < self.refresh_min_interval);
            if is_too_soon {
                return;
            }
            state.last_requested_refresh_at = Some(now);
        }
        HerdrFrameStream::refresh_history(self, true);
    }
}

/// 출력이 잠잠해졌는지 보는 타이머. 프레임마다 마감을 뒤로 미루고, 마감이 지나면 한 번 깨운다
#[derive(Default)]
struct QuietTimer {
    state: Mutex<QuietTimerState>,
    changed: Condvar,
}

#[derive(Default)]
struct QuietTimerState {
    due_at: Option<Instant>,
    is_stopped: bool,
}

impl QuietTimer {
    fn schedule(&self, due_at: Instant) {
        let mut state = self.state.lock().unwrap();
        if state.is_stopped {
            return;
        }
        state.due_at = Some(due_at);
        self.changed.notify_all();
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.is_stopped = true;
        state.due_at = None;
        self.changed.notify_all();
    }

    /// 마감이 지나면 참, 타이머가 멈추면 거짓을 돌려준다
    fn wait_until_due(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.is_stopped {
                return false;
            }
            match state.due_at {
                Some(due_at) if Instant::now() >= due_at => {
                    state.due_at = None;
                    return true;
                }
                Some(due_at) => {
                    let remaining = due_at.saturating_duration_since(Instant::now());
                    state = self.changed.wait_timeout(state, remaining).unwrap().0;
                }
                None => state = self.changed.wait(state).unwrap(),
            }
        }
    }
}
