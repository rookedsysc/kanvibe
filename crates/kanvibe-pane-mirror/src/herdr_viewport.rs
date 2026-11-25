//! Herdr pane 크기 제어(TS `createHerdrViewportDriver`).
//!
//! 모바일의 첫 크기 요청이 `terminal session control`을 열고, 그 뒤 요청은 같은 연결에 `terminal.resize`를 쓴다.
//! 요청 격자의 전체 프레임이 돌아와야 크기가 바뀐 것으로 본다. 제어 연결이 끊기면 관찰 스트림도 닫아
//! 맞지 않는 격자를 계속 보여 주지 않는다. 마지막 모바일이 나가면 `terminal.release`로 제어를 반납한다.

use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Instant;

use kanvibe_terminal::pane_mirror::{
    HerdrObserveRecord, build_herdr_control_pane_command, parse_herdr_observe_record,
};
use kanvibe_terminal::remote_terminal_protocol::{
    TerminalScrollRequest, build_herdr_scroll_control_record,
};
use serde_json::json;

use crate::error::{MirrorResult, PaneMirrorError};
use crate::mirror::{MirrorSessionTarget, MirrorShared, lock, subscription_key};
use crate::process::{LineSplitter, WatchedProcess};
use crate::viewport::ViewportDriver;

pub(crate) fn herdr_driver(
    shared: &Arc<MirrorShared>,
    target: &MirrorSessionTarget,
    task_id: &str,
    pane_id: &str,
) -> Arc<dyn ViewportDriver> {
    Arc::new_cyclic(|weak_self| HerdrViewportDriver {
        weak_self: weak_self.clone(),
        shared: Arc::downgrade(shared),
        target: target.clone(),
        pane_id: pane_id.to_string(),
        subscription_key: subscription_key(task_id, pane_id),
        state: Mutex::new(ControlState::default()),
        frame_settled: Condvar::new(),
    })
}

/// 크기 응답을 기다리는 요청 하나
struct PendingFrame {
    columns: i64,
    rows: i64,
    outcome: Option<MirrorResult<()>>,
}

#[derive(Default)]
struct ControlState {
    controller: Option<(u64, Arc<WatchedProcess>)>,
    next_generation: u64,
    failure: Option<PaneMirrorError>,
    pending_frame: Option<PendingFrame>,
}

struct HerdrViewportDriver {
    weak_self: Weak<HerdrViewportDriver>,
    shared: Weak<MirrorShared>,
    target: MirrorSessionTarget,
    pane_id: String,
    subscription_key: String,
    state: Mutex<ControlState>,
    frame_settled: Condvar,
}

fn failure(message: &str) -> PaneMirrorError {
    PaneMirrorError::Failed(message.to_string())
}

impl HerdrViewportDriver {
    fn shared(&self) -> MirrorResult<Arc<MirrorShared>> {
        self.shared
            .upgrade()
            .ok_or_else(|| failure("데스크탑이 종료 중입니다."))
    }

    fn controller(&self) -> Option<(u64, Arc<WatchedProcess>)> {
        lock(&self.state).controller.clone()
    }

    /// 지금 제어 연결이 `generation`일 때만 실패로 끝낸다. 실패를 기억해 다음 요청도 거절하고, 관찰 스트림까지 닫는다
    fn fail_controller(&self, generation: u64, error: PaneMirrorError) {
        let stopping = {
            let mut state = lock(&self.state);
            if !state
                .controller
                .as_ref()
                .is_some_and(|(current, _)| *current == generation)
            {
                return;
            }
            state.failure = Some(error.clone());
            if let Some(pending) = state.pending_frame.as_mut() {
                pending.outcome.get_or_insert(Err(error));
            }
            state.controller.take()
        };
        self.frame_settled.notify_all();
        if let Some((_, controller)) = stopping {
            controller.stop();
        }
        self.end_observation();
    }

    /// 관찰 스트림의 구독자에게 끝을 알린다. 소켓을 닫는 쪽이 다시 이 줄로 들어오지 않도록 다른 스레드에서 알린다
    fn end_observation(&self) {
        let Ok(shared) = self.shared() else {
            return;
        };
        let key = self.subscription_key.clone();
        std::thread::spawn(move || {
            let Some(claim) = lock(&shared.subscriptions).get(&key).cloned() else {
                return;
            };
            let Ok(subscription) = claim.subscription.wait() else {
                return;
            };
            if shared.forget_pane_claim(&key, &claim) {
                subscription.stream.stop();
                subscription.audience.end();
            }
        });
    }

    fn handle_line(&self, generation: u64, line: &str) {
        match parse_herdr_observe_record(line) {
            Some(HerdrObserveRecord::Closed) => {
                self.fail_controller(
                    generation,
                    failure("Herdr 터미널 제어 연결이 종료됐습니다."),
                );
            }
            Some(HerdrObserveRecord::Frame {
                is_full_screen: true,
                size: Some(size),
                ..
            }) => self.acknowledge_frame(generation, size.width, size.height),
            _ => {}
        }
    }

    fn acknowledge_frame(&self, generation: u64, width: f64, height: f64) {
        let mut state = lock(&self.state);
        let is_current = state
            .controller
            .as_ref()
            .is_some_and(|(current, _)| *current == generation);
        if let Some(pending) = state.pending_frame.as_mut().filter(|_| is_current)
            && pending.columns as f64 == width
            && pending.rows as f64 == height
        {
            pending.outcome.get_or_insert(Ok(()));
            self.frame_settled.notify_all();
        }
    }

    /// 요청 격자의 전체 프레임을 기다린다. 상한 안에 오지 않으면 제어 프로세스를 정리하고 실패한다
    fn await_frame(&self, generation: u64, timeout: std::time::Duration) -> MirrorResult<()> {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.state);
        loop {
            if let Some(outcome) = state
                .pending_frame
                .as_mut()
                .and_then(|pending| pending.outcome.take())
            {
                state.pending_frame = None;
                return outcome;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                drop(state);
                let error = failure("Herdr 터미널 크기 응답이 없습니다.");
                self.fail_controller(generation, error.clone());
                lock(&self.state).pending_frame = None;
                return Err(error);
            }
            state = self.frame_settled.wait_timeout(state, remaining).unwrap().0;
        }
    }

    fn open_controller(&self, shared: &MirrorShared, columns: i64, rows: i64) -> MirrorResult<()> {
        let command = build_herdr_control_pane_command(
            &self.target.session_name,
            &self.pane_id,
            columns,
            rows,
        )?;
        let process = shared.processes.spawn(&command, self.target.ssh_host())?;
        let generation = {
            // 첫 프레임이 기다림보다 먼저 와도 놓치지 않도록 기다릴 크기를 먼저 적고 읽기를 붙인다
            let mut state = lock(&self.state);
            state.next_generation += 1;
            let generation = state.next_generation;
            state.pending_frame = Some(PendingFrame {
                columns,
                rows,
                outcome: None,
            });
            let controller = self.watch_controller(process, generation);
            state.controller = Some((generation, controller));
            generation
        };
        self.await_frame(generation, shared.timings.command_timeout)
    }

    fn watch_controller(
        &self,
        process: crate::process::MirrorProcess,
        generation: u64,
    ) -> Arc<WatchedProcess> {
        let weak_lines = self.weak_self.clone();
        let weak_exit = self.weak_self.clone();
        let mut lines = LineSplitter::default();
        WatchedProcess::start(
            process,
            move |bytes| {
                let Some(driver) = weak_lines.upgrade() else {
                    return;
                };
                for line in lines.push(bytes) {
                    driver.handle_line(generation, &line);
                }
            },
            move || {
                if let Some(driver) = weak_exit.upgrade() {
                    driver.fail_controller(
                        generation,
                        failure("Herdr 터미널 제어 프로세스가 종료됐습니다."),
                    );
                }
            },
        )
    }

    fn resize_open_controller(
        &self,
        shared: &MirrorShared,
        generation: u64,
        controller: &WatchedProcess,
        columns: i64,
        rows: i64,
    ) -> MirrorResult<()> {
        lock(&self.state).pending_frame = Some(PendingFrame {
            columns,
            rows,
            outcome: None,
        });
        let record = json!({ "type": "terminal.resize", "cols": columns, "rows": rows });
        if let Err(error) = controller.write_line(&record.to_string()) {
            self.fail_controller(generation, PaneMirrorError::Failed(error));
        }
        self.await_frame(generation, shared.timings.command_timeout)
    }

    fn set_observation_viewport(
        &self,
        shared: &MirrorShared,
        size: Option<(i64, i64)>,
    ) -> MirrorResult<()> {
        match shared.wait_pane_subscription(&self.subscription_key) {
            Some(subscription) => subscription.stream.set_viewport(size),
            None => Ok(()),
        }
    }
}

impl ViewportDriver for HerdrViewportDriver {
    fn resize(&self, _pane_id: &str, columns: i64, rows: i64) -> MirrorResult<()> {
        if let Some(failure) = lock(&self.state).failure.clone() {
            return Err(failure);
        }
        let shared = self.shared()?;
        self.set_observation_viewport(&shared, Some((columns, rows)))?;
        match self.controller() {
            None => self.open_controller(&shared, columns, rows),
            Some((generation, controller)) => {
                self.resize_open_controller(&shared, generation, &controller, columns, rows)
            }
        }
    }

    /// 제어 연결은 모바일의 첫 크기 요청으로 열린다. 그 전에 온 휠은 보낼 곳이 없어 버린다
    fn scroll(
        &self,
        _pane_id: &str,
        request: &TerminalScrollRequest,
    ) -> MirrorResult<Option<String>> {
        if let Some(failure) = lock(&self.state).failure.clone() {
            return Err(failure);
        }
        let Some((generation, controller)) = self.controller() else {
            return Ok(None);
        };
        let record = build_herdr_scroll_control_record(request);
        if let Err(error) = controller.write_line(&record.to_string()) {
            self.fail_controller(generation, PaneMirrorError::Failed(error));
        }
        Ok(None)
    }

    fn restore(&self) -> MirrorResult<()> {
        let Some((_, controller)) = lock(&self.state).controller.take() else {
            return Ok(());
        };
        let shared = self.shared()?;
        let release = json!({ "type": "terminal.release" }).to_string();
        if let Err(error) = controller.finish_input(&release) {
            controller.stop();
            return Err(PaneMirrorError::Failed(error));
        }
        if !controller.wait_for_exit(shared.timings.command_timeout) {
            controller.stop();
            return Err(failure("Herdr 터미널 제어 반납 응답이 없습니다."));
        }
        self.set_observation_viewport(&shared, None)
    }
}
