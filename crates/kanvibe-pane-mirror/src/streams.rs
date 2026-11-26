//! tmux `pipe-pane` 출력 스트림과 zellij 화면 폴링.

use std::sync::{Arc, Condvar, Mutex, Weak};

use kanvibe_terminal::pane_mirror::{
    build_tmux_cancel_pipe_pane_command, build_tmux_pane_stream_command,
};

use crate::error::MirrorResult;
use crate::mirror::{MirrorSessionTarget, MirrorShared};
use crate::process::{Utf8ChunkDecoder, WatchedProcess};
use crate::subscription::{PaneStream, StreamAudience};

/// tmux pane 출력을 임시 파일과 `tail -f`로 따라간다. 자식이 죽으면 구독 자리를 거두고 구독자에게 끝을 알린다
pub(crate) fn start_tmux_pane_stream(
    shared: &Arc<MirrorShared>,
    target: &MirrorSessionTarget,
    pane_id: &str,
    audience: &Arc<StreamAudience>,
    on_dead: Arc<dyn Fn() + Send + Sync>,
) -> MirrorResult<Arc<dyn PaneStream>> {
    let process = shared
        .processes
        .spawn(&build_tmux_pane_stream_command(pane_id), target.ssh_host())?;
    let output_audience = Arc::clone(audience);
    let mut decoder = Utf8ChunkDecoder::default();
    let watched = WatchedProcess::start(
        process,
        move |bytes| {
            let chunk = decoder.push(bytes);
            if !chunk.is_empty() {
                output_audience.broadcast(&chunk);
            }
        },
        move || on_dead(),
    );
    Ok(Arc::new(TmuxPaneStream {
        shared: Arc::downgrade(shared),
        ssh_host: target.ssh_host().map(str::to_string),
        pane_id: pane_id.to_string(),
        process: watched,
    }))
}

struct TmuxPaneStream {
    shared: Weak<MirrorShared>,
    ssh_host: Option<String>,
    pane_id: String,
    process: Arc<WatchedProcess>,
}

impl PaneStream for TmuxPaneStream {
    fn stop(&self) {
        self.process.stop();
        // 프로세스가 트랩을 실행하지 못하고 죽는 경우가 있어 파이프를 한 번 더 끊는다.
        // 파이프가 남으면 pane이 계속 임시 파일에 쓰고, 다음 구독자가 `-o` 토글에 걸려 아무것도 못 받는다
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let ssh_host = self.ssh_host.clone();
        let command = build_tmux_cancel_pipe_pane_command(&self.pane_id);
        std::thread::spawn(move || {
            let _ = shared.run_command(&command, ssh_host.as_deref());
        });
    }
}

/// zellij에는 `pipe-pane`에 해당하는 출력 스트림이 없어 화면을 주기적으로 다시 뜬다.
/// 앞선 조회가 끝나야 다음 주기를 기다리므로 느린 호스트에서 요청이 밀려 쌓이지 않는다
pub(crate) fn start_zellij_dump_polling(
    shared: &Arc<MirrorShared>,
    target: &MirrorSessionTarget,
    pane_id: &str,
    audience: &Arc<StreamAudience>,
) -> Arc<dyn PaneStream> {
    let stream = Arc::new(ZellijDumpPolling {
        is_stopped: Mutex::new(false),
        stopped: Condvar::new(),
    });
    let interval = shared.timings.dump_interval(target.ssh_host());
    let polling = Arc::clone(&stream);
    let weak_shared = Arc::downgrade(shared);
    let target = target.clone();
    let pane_id = pane_id.to_string();
    let audience = Arc::clone(audience);
    std::thread::spawn(move || {
        let mut last_dump: Option<String> = None;
        while polling.wait_next_round(interval) {
            let Some(shared) = weak_shared.upgrade() else {
                return;
            };
            // 세션이 사라졌을 수 있다. 소켓을 끊는 판단은 화면이 하도록 두고 여기서는 다음 주기를 기다린다
            let Ok(dump) = shared.read_pane_snapshot(&target, &pane_id) else {
                continue;
            };
            if last_dump.as_deref() != Some(dump.as_str()) {
                audience.broadcast(&dump);
                last_dump = Some(dump);
            }
        }
    });
    stream
}

struct ZellijDumpPolling {
    is_stopped: Mutex<bool>,
    stopped: Condvar,
}

impl ZellijDumpPolling {
    /// 한 주기를 기다린다. 그 사이 멈췄으면 거짓이다
    fn wait_next_round(&self, interval: std::time::Duration) -> bool {
        let is_stopped = self.is_stopped.lock().unwrap();
        let (is_stopped, _) = self
            .stopped
            .wait_timeout_while(is_stopped, interval, |is_stopped| !*is_stopped)
            .unwrap();
        !*is_stopped
    }
}

impl PaneStream for ZellijDumpPolling {
    fn stop(&self) {
        *self.is_stopped.lock().unwrap() = true;
        self.stopped.notify_all();
    }
}
