//! terminal 세션, tmux, zellij의 크기 조정과 휠.

use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use kanvibe_terminal::pane_mirror::{
    FloatingPaneGeometry, TmuxViewportState, ZellijViewportState, build_screen_redraw,
    build_tmux_capture_copy_mode_view_command, build_tmux_copy_mode_view_command,
    build_tmux_copy_mode_wheel_command, build_tmux_enter_wheel_copy_mode_command,
    build_tmux_read_pane_geometry_command, build_tmux_read_viewport_command,
    build_tmux_resize_viewport_command, build_tmux_restore_viewport_command,
    build_tmux_send_keys_command, build_tmux_wheel_state_command,
    build_zellij_read_viewport_command, build_zellij_resize_floating_pane_command,
    build_zellij_select_tab_command, parse_tmux_copy_mode_view, parse_tmux_viewport_state,
    parse_tmux_wheel_state, parse_zellij_viewport_state,
};
use kanvibe_terminal::remote_terminal_protocol::{
    MouseReportEncoding, ScrollDirection, TerminalScrollRequest, build_mouse_wheel_report,
};
use kanvibe_terminal::terminal_bridge::CallbackTerminalClient;
use kanvibe_terminal::terminal_registry::{TerminalAttachment, TerminalClient};

use crate::error::{MirrorResult, PaneMirrorError};
use crate::mirror::{MirrorSessionTarget, MirrorShared, lock};
use crate::viewport::ViewportDriver;

const MAX_PTY_DIMENSION: i64 = 65_535;

fn failure(message: &str) -> PaneMirrorError {
    PaneMirrorError::Failed(message.to_string())
}

fn upgrade(shared: &Weak<MirrorShared>) -> MirrorResult<Arc<MirrorShared>> {
    shared
        .upgrade()
        .ok_or_else(|| failure("데스크탑이 종료 중입니다."))
}

fn pty_size(columns: i64, rows: i64) -> (u16, u16) {
    let clamp = |value: i64| value.clamp(1, MAX_PTY_DIMENSION) as u16;
    (clamp(columns), clamp(rows))
}

/// terminal 세션은 KanVibe가 가진 PTY 크기를 바꾸고, 떠날 때는 처음 크기(또는 그동안 바뀐 데스크탑 크기)로 돌린다
pub(crate) fn terminal_driver(
    shared: &Arc<MirrorShared>,
    task_id: &str,
    pane_id: &str,
) -> Arc<dyn ViewportDriver> {
    let original = shared.terminals.terminal_tab_size(task_id, pane_id);
    Arc::new(TerminalViewportDriver {
        shared: Arc::downgrade(shared),
        task_id: task_id.to_string(),
        pane_id: pane_id.to_string(),
        original,
    })
}

struct TerminalViewportDriver {
    shared: Weak<MirrorShared>,
    task_id: String,
    pane_id: String,
    original: (u16, u16),
}

impl ViewportDriver for TerminalViewportDriver {
    fn resize(&self, pane_id: &str, columns: i64, rows: i64) -> MirrorResult<()> {
        let (columns, rows) = pty_size(columns, rows);
        Ok(upgrade(&self.shared)?.terminals.resize_terminal_tab(
            &self.task_id,
            pane_id,
            columns,
            rows,
        )?)
    }

    fn restore(&self) -> MirrorResult<()> {
        let (columns, rows) = self.original;
        upgrade(&self.shared)?
            .terminals
            .release_terminal_tab_viewport(&self.task_id, &self.pane_id, columns, rows);
        Ok(())
    }
}

/// tmux는 window 배치와 크기 정책을 읽어 두고, pane이 요청 크기가 되도록 window 크기를 바꾼 뒤 떠날 때 되돌린다
pub(crate) fn tmux_driver(
    shared: &Arc<MirrorShared>,
    target: &MirrorSessionTarget,
    window_id: &str,
) -> MirrorResult<Arc<dyn ViewportDriver>> {
    let output = shared.run_command(
        &build_tmux_read_viewport_command(window_id),
        target.ssh_host(),
    )?;
    let state = parse_tmux_viewport_state(&output)?;
    Ok(Arc::new(TmuxViewportDriver {
        shared: Arc::downgrade(shared),
        target: target.clone(),
        window_id: window_id.to_string(),
        state,
    }))
}

struct TmuxViewportDriver {
    shared: Weak<MirrorShared>,
    target: MirrorSessionTarget,
    window_id: String,
    state: TmuxViewportState,
}

impl TmuxViewportDriver {
    fn run(&self, command: &str) -> MirrorResult<String> {
        upgrade(&self.shared)?.run_command(command, self.target.ssh_host())
    }

    /// tmux 기본 휠 바인딩이 고르는 길을 그대로 밟는다. 바깥에서 tmux에 마우스 이벤트를 넣는 명령이 없어 갈림길만 옮겨 온다.
    /// copy-mode는 휠 폭만큼 움직이고, 마우스 보고를 켠 프로그램은 휠 보고를 받으며, 보고를 켜지 않은 대체 화면 프로그램에는 아무것도 가지 않는다.
    /// 그 밖의 위로 굴린 휠은 copy-mode로 들어간다. 아래로 굴린 휠에는 기본 바인딩이 없다
    fn scroll_like_wheel(
        &self,
        pane_id: &str,
        request: &TerminalScrollRequest,
    ) -> MirrorResult<Option<String>> {
        let state_output = self.run(&build_tmux_wheel_state_command(pane_id))?;
        let Some(wheel_state) = parse_tmux_wheel_state(&state_output) else {
            return Ok(None);
        };
        if wheel_state.is_in_copy_mode {
            self.run(&build_tmux_copy_mode_wheel_command(
                pane_id,
                request.direction,
            ))?;
            return self.read_screen_after_copy_mode_wheel(pane_id).map(Some);
        }
        if wheel_state.is_mouse_reporting {
            let report = build_mouse_wheel_report(
                request.direction,
                request.column,
                request.row,
                wheel_state.encoding,
            );
            if let Some(report) = report {
                self.run(&build_tmux_send_keys_command(pane_id, &report))?;
            }
            return Ok(None);
        }
        if wheel_state.is_alternate_screen || request.direction == ScrollDirection::Down {
            return Ok(None);
        }
        self.run(&build_tmux_enter_wheel_copy_mode_command(pane_id))?;
        self.read_screen_after_copy_mode_wheel(pane_id).map(Some)
    }

    /// 맨 아래까지 내려 copy-mode가 끝났으면 프로그램 화면과 그 모드를 다시 보내고, 아직 copy-mode면 그 화면을 보낸다
    fn read_screen_after_copy_mode_wheel(&self, pane_id: &str) -> MirrorResult<String> {
        let view_output = self.run(&build_tmux_copy_mode_view_command(pane_id))?;
        let Some(view) = parse_tmux_copy_mode_view(&view_output) else {
            return upgrade(&self.shared)?.read_pane_snapshot(&self.target, pane_id);
        };
        let screen = self.run(&build_tmux_capture_copy_mode_view_command(pane_id, &view))?;
        Ok(build_screen_redraw(&screen, None))
    }
}

impl ViewportDriver for TmuxViewportDriver {
    fn resize(&self, pane_id: &str, columns: i64, rows: i64) -> MirrorResult<()> {
        let geometry = self.run(&build_tmux_read_pane_geometry_command(
            &self.window_id,
            pane_id,
        ))?;
        let command = build_tmux_resize_viewport_command(pane_id, &geometry, columns, rows)?;
        self.run(&command).map(|_| ())
    }

    fn restore(&self) -> MirrorResult<()> {
        self.run(&build_tmux_restore_viewport_command(
            &self.window_id,
            &self.state,
        ))
        .map(|_| ())
    }

    fn scroll(
        &self,
        pane_id: &str,
        request: &TerminalScrollRequest,
    ) -> MirrorResult<Option<String>> {
        self.scroll_like_wheel(pane_id, request)
    }
}

/// zellij는 데스크탑이 붙인 것과 같은 클라이언트 PTY의 크기로 배치를 다시 계산하게 만든다.
/// 모바일이 보는 탭을 그 클라이언트에서 먼저 띄우고, pane 글자 영역이 요청 크기가 될 때까지 PTY 크기를 맞춘다
pub(crate) fn zellij_driver(
    shared: &Arc<MirrorShared>,
    target: &MirrorSessionTarget,
    task_id: &str,
    pane_id: &str,
) -> MirrorResult<Arc<dyn ViewportDriver>> {
    let initial = read_zellij_geometry(shared, target, pane_id)?;
    let original = shared.terminals.task_terminal_size(task_id).map_or(
        (initial.display_cols, initial.display_rows),
        |(columns, rows)| (i64::from(columns), i64::from(rows)),
    );
    let client = Arc::new(CallbackTerminalClient::new(|_| {}));
    let attachment = attach_zellij_client(shared, task_id, &client, original)?;
    Ok(Arc::new(ZellijViewportDriver {
        shared: Arc::downgrade(shared),
        target: target.clone(),
        task_id: task_id.to_string(),
        original,
        floating_originals: Mutex::new(Vec::new()),
        client,
        attachment,
    }))
}

fn attach_zellij_client(
    shared: &Arc<MirrorShared>,
    task_id: &str,
    client: &Arc<CallbackTerminalClient>,
    original: (i64, i64),
) -> MirrorResult<Option<TerminalAttachment>> {
    let (columns, rows) = pty_size(original.0, original.1);
    let terminal_client: Arc<dyn TerminalClient> = client.clone();
    let attached = shared.find_mirror_task(task_id, false).and_then(|task| {
        Ok(shared.attacher.attach_task_terminal(
            &task,
            None,
            terminal_client,
            Some(columns),
            Some(rows),
        )?)
    });
    let attachment = match attached {
        Ok(attachment) => attachment,
        Err(error) => {
            client.close(None);
            return Err(error);
        }
    };
    if shared.terminals.task_terminal_size(task_id).is_none() {
        client.close(None);
        if let Some(attachment) = &attachment {
            shared.terminals.client_closed(attachment);
        }
        return Err(failure("Zellij 터미널 연결이 종료됐습니다."));
    }
    Ok(attachment)
}

fn read_zellij_geometry(
    shared: &MirrorShared,
    target: &MirrorSessionTarget,
    pane_id: &str,
) -> MirrorResult<ZellijViewportState> {
    let command = build_zellij_read_viewport_command(&target.session_name);
    let output = shared.run_command(&command, target.ssh_host())?;
    parse_zellij_viewport_state(&output, pane_id)
        .ok_or_else(|| failure("Zellij pane 크기를 확인하지 못했습니다."))
}

struct ZellijViewportDriver {
    shared: Weak<MirrorShared>,
    target: MirrorSessionTarget,
    task_id: String,
    original: (i64, i64),
    /// 처음 본 floating pane의 원래 좌표와 테두리 포함 크기. 넣은 순서대로 복원한다
    floating_originals: Mutex<Vec<(String, FloatingPaneGeometry)>>,
    client: Arc<CallbackTerminalClient>,
    attachment: Option<TerminalAttachment>,
}

/// JS `Math.round`. 반올림 경계는 양의 무한대 쪽으로 간다
fn js_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

impl ZellijViewportDriver {
    fn select_pane_tab(
        &self,
        shared: &MirrorShared,
        pane_id: &str,
    ) -> MirrorResult<ZellijViewportState> {
        let selected = read_zellij_geometry(shared, &self.target, pane_id)?;
        let command = build_zellij_select_tab_command(&self.target.session_name, &selected.tab_id);
        shared.run_command(&command, self.target.ssh_host())?;
        Ok(selected)
    }

    fn remember_floating(
        &self,
        pane_id: &str,
        geometry: &ZellijViewportState,
    ) -> Option<FloatingPaneGeometry> {
        let origin = geometry.floating.as_ref()?;
        let mut originals = lock(&self.floating_originals);
        if !originals.iter().any(|(id, _)| id == pane_id) {
            originals.push((
                pane_id.to_string(),
                FloatingPaneGeometry {
                    left: origin.left,
                    top: origin.top,
                    cols: geometry.pane_cols,
                    rows: geometry.pane_rows,
                },
            ));
        }
        originals
            .iter()
            .find(|(id, _)| id == pane_id)
            .map(|(_, original)| *original)
    }

    /// 요청한 글자 영역 크기에 맞는 다음 클라이언트 PTY 크기. floating pane은 원래 자리에서 요청 크기가 들어갈 만큼만 넓힌다
    fn next_client_size(
        &self,
        geometry: &ZellijViewportState,
        floating: Option<&FloatingPaneGeometry>,
        outer: (i64, i64),
    ) -> (i64, i64) {
        let (outer_cols, outer_rows) = outer;
        let chrome_cols = geometry.display_cols - geometry.viewport_cols;
        let chrome_rows = geometry.display_rows - geometry.viewport_rows;
        match floating {
            Some(floating) => (
                self.original
                    .0
                    .max(floating.left + outer_cols + chrome_cols),
                self.original.1.max(floating.top + outer_rows + chrome_rows),
            ),
            None => (
                js_round(
                    geometry.viewport_cols as f64 * outer_cols as f64 / geometry.pane_cols as f64,
                ) + chrome_cols,
                js_round(
                    geometry.viewport_rows as f64 * outer_rows as f64 / geometry.pane_rows as f64,
                ) + chrome_rows,
            ),
        }
    }

    /// 한 번 맞춰 본다. 이미 요청 크기면 참이다
    fn adjust_once(
        &self,
        shared: &MirrorShared,
        pane_id: &str,
        columns: i64,
        rows: i64,
    ) -> MirrorResult<bool> {
        let geometry = read_zellij_geometry(shared, &self.target, pane_id)?;
        if geometry.cols == columns && geometry.rows == rows {
            self.keep_current_client_size(shared)?;
            return Ok(true);
        }
        let outer = (
            columns + geometry.pane_cols - geometry.cols,
            rows + geometry.pane_rows - geometry.rows,
        );
        let floating = self.remember_floating(pane_id, &geometry);
        let (next_cols, next_rows) = self.next_client_size(&geometry, floating.as_ref(), outer);
        let is_valid = [next_cols, next_rows]
            .iter()
            .all(|size| (1..=MAX_PTY_DIMENSION).contains(size));
        if !is_valid {
            return Err(failure(
                "Zellij 분할 배치에서 요청한 크기를 적용할 수 없습니다.",
            ));
        }
        shared.terminals.resize_task_terminal_viewport(
            &self.task_id,
            next_cols as u16,
            next_rows as u16,
        )?;
        if let Some(floating) = floating {
            self.move_floating_pane(shared, pane_id, &floating, outer)?;
        }
        Ok(false)
    }

    /// 이미 요청 크기다. 지금 클라이언트 크기를 모바일 몫으로 기록해 데스크탑 변경이 끼어들지 않게 한다
    fn keep_current_client_size(&self, shared: &MirrorShared) -> MirrorResult<()> {
        let (columns, rows) = shared
            .terminals
            .task_terminal_size(&self.task_id)
            .ok_or_else(|| failure("Zellij 터미널 연결이 종료됐습니다."))?;
        Ok(shared
            .terminals
            .resize_task_terminal_viewport(&self.task_id, columns, rows)?)
    }

    /// floating pane은 원래 자리에 두고 테두리 포함 크기만 요청 크기로 바꾼다
    fn move_floating_pane(
        &self,
        shared: &MirrorShared,
        pane_id: &str,
        floating: &FloatingPaneGeometry,
        outer: (i64, i64),
    ) -> MirrorResult<()> {
        let geometry = FloatingPaneGeometry {
            left: floating.left,
            top: floating.top,
            cols: outer.0,
            rows: outer.1,
        };
        let command = build_zellij_resize_floating_pane_command(
            &self.target.session_name,
            pane_id,
            &geometry,
        );
        shared.run_command(&command, self.target.ssh_host())?;
        Ok(())
    }
}

impl ViewportDriver for ZellijViewportDriver {
    fn resize(&self, pane_id: &str, columns: i64, rows: i64) -> MirrorResult<()> {
        let shared = upgrade(&self.shared)?;
        self.select_pane_tab(&shared, pane_id)?;
        let deadline = Instant::now() + shared.timings.command_timeout;
        loop {
            if self.adjust_once(&shared, pane_id, columns, rows)? {
                return Ok(());
            }
            // Zellij가 PTY resize 신호로 배치를 다시 계산한 뒤 실제 content 격자를 확인한다
            std::thread::sleep(shared.timings.viewport_settle_poll_interval);
            if Instant::now() >= deadline {
                return Err(failure(
                    "다른 Zellij 클라이언트나 탭 배치가 모바일 크기를 제한하고 있습니다.",
                ));
            }
        }
    }

    /// 데스크탑에 붙은 Zellij 클라이언트에 실제 휠을 넣어, Zellij가 pane 모드에 맞춰 휠 보고·화살표 키·자체 스크롤백 중 하나를 고르게 한다.
    /// 클라이언트는 활성 탭만 그리므로 크기를 맞출 때처럼 pane의 탭을 먼저 띄운다
    fn scroll(
        &self,
        pane_id: &str,
        request: &TerminalScrollRequest,
    ) -> MirrorResult<Option<String>> {
        let shared = upgrade(&self.shared)?;
        let selected = self.select_pane_tab(&shared, pane_id)?;
        let Some(origin) = selected.content_origin else {
            return Ok(None);
        };
        let column = u32::try_from(origin.left).unwrap_or(0) + request.column;
        let row = u32::try_from(origin.top).unwrap_or(0) + request.row;
        let Some(report) =
            build_mouse_wheel_report(request.direction, column, row, MouseReportEncoding::Sgr)
        else {
            return Ok(None);
        };
        shared
            .terminals
            .write_task_terminal_input(&self.task_id, &report)?;
        // 휠은 클라이언트를 거쳐 비동기로 반영되므로 잠깐 기다린 뒤 떠야 움직인 화면이 담긴다
        std::thread::sleep(shared.timings.zellij_wheel_settle);
        shared.read_pane_snapshot(&self.target, pane_id).map(Some)
    }

    fn restore(&self) -> MirrorResult<()> {
        let result = self.restore_client_and_floating_panes();
        self.client.close(None);
        if let (Some(shared), Some(attachment)) = (self.shared.upgrade(), &self.attachment) {
            shared.terminals.client_closed(attachment);
        }
        result
    }
}

impl ZellijViewportDriver {
    fn restore_client_and_floating_panes(&self) -> MirrorResult<()> {
        let shared = upgrade(&self.shared)?;
        let (columns, rows) = pty_size(self.original.0, self.original.1);
        shared
            .terminals
            .release_task_terminal_viewport(&self.task_id, columns, rows);
        let originals = lock(&self.floating_originals).clone();
        if originals.is_empty() {
            return Ok(());
        }
        std::thread::sleep(shared.timings.viewport_settle_poll_interval);
        let mut first_error = None;
        for (pane_id, geometry) in originals {
            let command = build_zellij_resize_floating_pane_command(
                &self.target.session_name,
                &pane_id,
                &geometry,
            );
            if let Err(error) = shared.run_command(&command, self.target.ssh_host()) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
