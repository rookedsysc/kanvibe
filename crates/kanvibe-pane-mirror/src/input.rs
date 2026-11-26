//! 모바일 키 입력과 pane chat이 쓰는 Herdr 화면 한 장, 그리고 [`PaneChatMobileBridge`] 구현.

use std::sync::{Arc, Condvar, Mutex};

use kanvibe_domain::task::SessionType;
use kanvibe_terminal::pane_chat::surface::{HerdrPaneFrame, PaneChatMobileBridge};
use kanvibe_terminal::pane_mirror::{
    HerdrObserveRecord, build_herdr_write_pane_command, build_tmux_send_keys_command,
    build_zellij_write_pane_command, parse_herdr_observe_record,
};

use crate::error::{MirrorResult, PaneMirrorError, session_not_running};
use crate::herdr_stream::build_observe_command;
use crate::mirror::{MirrorShared, PaneMirror, subscription_key};
use crate::process::{LineSplitter, WatchedProcess};
use crate::surfaces::PaneListScope;

const MAX_CHAT_OBSERVE_FRAME_BYTES: usize = 1024 * 1024;

impl PaneMirror {
    /// 모바일에서 누른 키를 pane에 그대로 넣는다. pane은 태스크의 세션에 속해야 한다
    pub fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> MirrorResult<()> {
        self.shared.write_to_pane(task_id, pane_id, input)
    }

    /// pane chat이 Herdr pane의 지금 화면을 읽을 때 쓰는 첫 전체 프레임 한 장
    pub fn read_herdr_pane_frame_for_chat(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> MirrorResult<HerdrPaneFrame> {
        self.shared.read_herdr_pane_frame_for_chat(task_id, pane_id)
    }

    /// 데스크탑이 아는 미러 구독에서만 처리하는 이력 갱신. 구독이 없거나 이력을 스스로 쌓는 세션이면 아무 일도 하지 않는다
    pub fn refresh_pane_history(&self, task_id: &str, pane_id: &str) {
        if let Some(subscription) = self
            .shared
            .wait_pane_subscription(&subscription_key(task_id, pane_id))
        {
            subscription.stream.refresh_history();
        }
    }
}

impl MirrorShared {
    fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> MirrorResult<()> {
        let target = self.find_mirror_target(task_id)?;
        if target.session_type == SessionType::Terminal {
            if !self
                .terminals
                .write_terminal_tab_input(task_id, pane_id, input)
            {
                return Err(session_not_running());
            }
            return Ok(());
        }
        // 구독이 붙기 전에도 소켓은 입력을 받을 수 있어, 기억해 둔 확인이 없으면 여기서 직접 확인한다
        if !self.has_pane_subscription(&subscription_key(task_id, pane_id)) {
            self.ensure_pane_belongs_to_session(&target, pane_id)?;
        }
        let command = match target.session_type {
            SessionType::Herdr => {
                build_herdr_write_pane_command(&target.session_name, pane_id, input)?
            }
            SessionType::Zellij => {
                build_zellij_write_pane_command(&target.session_name, pane_id, input)
            }
            _ => build_tmux_send_keys_command(pane_id, input),
        };
        self.run_command(&command, target.ssh_host()).map(|_| ())
    }

    fn read_herdr_pane_frame_for_chat(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> MirrorResult<HerdrPaneFrame> {
        let target = self.find_mirror_target(task_id)?;
        if target.session_type != SessionType::Herdr {
            return Err(PaneMirrorError::Failed(
                "Herdr pane이 아닙니다.".to_string(),
            ));
        }
        let pane = self
            .list_mirror_panes(&target, &PaneListScope::selected(pane_id))?
            .into_iter()
            .find(|entry| entry.id == pane_id)
            .ok_or(PaneMirrorError::PaneNotFound)?;
        let command = build_observe_command(&target, pane_id, pane.width, pane.height)?;
        let process = self.processes.spawn(&command, target.ssh_host())?;
        let first_frame = Arc::new(FirstFrame::default());
        let frame_sink = Arc::clone(&first_frame);
        let exit_sink = Arc::clone(&first_frame);
        let mut lines = LineSplitter::default();
        let fallback_size = (pane.width, pane.height);
        let observer = WatchedProcess::start(
            process,
            move |bytes| frame_sink.accept(&mut lines, bytes, fallback_size),
            move || exit_sink.settle(Err("Herdr 화면 연결이 종료됐습니다.".to_string())),
        );
        let outcome = first_frame.wait(self.timings.command_timeout);
        observer.stop();
        outcome.map_err(PaneMirrorError::Failed)
    }
}

/// 채팅 조회가 기다리는 첫 전체 프레임. 처음 정해진 결과만 남긴다
#[derive(Default)]
struct FirstFrame {
    outcome: Mutex<Option<Result<HerdrPaneFrame, String>>>,
    settled: Condvar,
}

impl FirstFrame {
    fn settle(&self, outcome: Result<HerdrPaneFrame, String>) {
        let mut current = self.outcome.lock().unwrap();
        if current.is_none() {
            *current = Some(outcome);
            self.settled.notify_all();
        }
    }

    fn accept(&self, lines: &mut LineSplitter, bytes: &[u8], fallback_size: (i64, i64)) {
        if lines.pending_bytes() + bytes.len() > MAX_CHAT_OBSERVE_FRAME_BYTES {
            self.settle(Err("Herdr 화면 크기 초과".to_string()));
            return;
        }
        let complete_lines = lines.push(bytes);
        for line in complete_lines {
            if let Some(HerdrObserveRecord::Frame {
                is_full_screen: true,
                output,
                size,
            }) = parse_herdr_observe_record(&line)
            {
                let width = size.map_or(fallback_size.0 as f64, |size| size.width);
                let height = size.map_or(fallback_size.1 as f64, |size| size.height);
                self.settle(Ok(HerdrPaneFrame {
                    output,
                    width: width as u16,
                    height: height as u16,
                }));
                return;
            }
        }
    }

    fn wait(&self, timeout: std::time::Duration) -> Result<HerdrPaneFrame, String> {
        let outcome = self.outcome.lock().unwrap();
        let (mut outcome, _) = self
            .settled
            .wait_timeout_while(outcome, timeout, |outcome| outcome.is_none())
            .unwrap();
        outcome
            .take()
            .unwrap_or_else(|| Err("Herdr 화면 응답 시간 초과".to_string()))
    }
}

impl PaneChatMobileBridge for PaneMirror {
    fn read_herdr_pane_frame_for_chat(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Result<HerdrPaneFrame, String> {
        PaneMirror::read_herdr_pane_frame_for_chat(self, task_id, pane_id)
            .map_err(|error| error.to_string())
    }

    fn write_to_pane(&self, task_id: &str, pane_id: &str, bytes: &str) -> Result<(), String> {
        PaneMirror::write_to_pane(self, task_id, pane_id, bytes).map_err(|error| error.to_string())
    }
}
