//! TS `paneChatSurface.test.ts`. 멀티플렉서·레지스트리·Herdr·모바일 브리지는 호출을 기록하는 가짜로 바꾼다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::pane_chat::snapshot::{PaneChatBounds, PaneChatLayout, PaneChatLayoutPane};
use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;
use serde_json::{Value, json};

use super::*;
use crate::herdr::{HerdrAgentSession, HerdrError, HerdrTaskPaneTab};
use crate::screen::ScreenReading;
use crate::terminal_registry::{HerdrSurfaceBounds, TaskTerminalAttachment, TerminalTabSnapshot};
use crate::test_support::ScriptedShellRunner;

struct FakeTasks {
    task: Mutex<Option<PaneChatTask>>,
}

impl PaneChatTaskLookup for FakeTasks {
    fn find_pane_chat_task(&self, _task_id: &str) -> Result<Option<PaneChatTask>, String> {
        Ok(self.task.lock().unwrap().clone())
    }
}

#[derive(Default)]
struct FakeTerminals {
    tabs: Mutex<Vec<TerminalTab>>,
    snapshot: Mutex<Option<TerminalTabSnapshot>>,
    snapshot_calls: Mutex<Vec<(String, String)>>,
    attachment: Mutex<Option<TaskTerminalAttachment>>,
    herdr_focus: Mutex<Option<String>>,
    herdr_bounds: Mutex<Option<HerdrSurfaceBounds>>,
}

impl PaneChatTerminals for FakeTerminals {
    fn list_local_terminal_tabs(&self, _task_id: &str) -> Vec<TerminalTab> {
        self.tabs.lock().unwrap().clone()
    }

    fn read_local_terminal_tab_snapshot(
        &self,
        task_id: &str,
        tab_id: &str,
    ) -> Option<TerminalTabSnapshot> {
        self.snapshot_calls
            .lock()
            .unwrap()
            .push((task_id.to_string(), tab_id.to_string()));
        self.snapshot.lock().unwrap().clone()
    }

    fn read_task_terminal_attachment(&self, _task_id: &str) -> Option<TaskTerminalAttachment> {
        self.attachment.lock().unwrap().clone()
    }

    fn herdr_task_active_pane_id(&self, _task_id: &str) -> Option<String> {
        self.herdr_focus.lock().unwrap().clone()
    }

    fn herdr_task_surface_bounds(&self, _task_id: &str) -> Option<HerdrSurfaceBounds> {
        *self.herdr_bounds.lock().unwrap()
    }

    fn focus_herdr_task_pane(&self, _task_id: &str, _pane_id: &str) -> Result<(), String> {
        Ok(())
    }
}

type HerdrResponder = Box<dyn Fn(&[&str]) -> Value + Send + Sync>;

struct FakeHerdr {
    panes: Mutex<Vec<HerdrTaskPaneTab>>,
    pane_list_calls: AtomicUsize,
    responder: Mutex<HerdrResponder>,
    command_calls: AtomicUsize,
}

impl PaneChatHerdr for FakeHerdr {
    fn list_task_pane_tab_ids(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrTaskPaneTab>, HerdrError> {
        self.pane_list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.panes.lock().unwrap().clone())
    }

    fn run_command(
        &self,
        _session_name: &str,
        arguments: &[&str],
        _ssh_host: Option<&str>,
    ) -> Result<Value, HerdrError> {
        self.command_calls.fetch_add(1, Ordering::SeqCst);
        Ok((self.responder.lock().unwrap())(arguments))
    }
}

#[derive(Default)]
struct FakeBridge {
    frame: Mutex<Option<HerdrPaneFrame>>,
    frame_calls: Mutex<Vec<(String, String)>>,
}

impl PaneChatMobileBridge for FakeBridge {
    fn read_herdr_pane_frame_for_chat(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Result<HerdrPaneFrame, String> {
        self.frame_calls
            .lock()
            .unwrap()
            .push((task_id.to_string(), pane_id.to_string()));
        self.frame
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "frame 없음".to_string())
    }

    fn write_to_pane(&self, _task_id: &str, _pane_id: &str, _bytes: &str) -> Result<(), String> {
        Ok(())
    }
}

/// TS `beforeEach`. tmux 세션 `kv`에 PID 9000 클라이언트가 붙어 있다
struct Harness {
    tasks: Arc<FakeTasks>,
    runner: Arc<ScriptedShellRunner>,
    terminals: Arc<FakeTerminals>,
    herdr: Arc<FakeHerdr>,
    bridge: Arc<FakeBridge>,
    reader: PaneChatSurfaceReader,
}

fn base_task() -> PaneChatTask {
    PaneChatTask {
        id: "task".to_string(),
        session_type: Some(SessionType::Tmux),
        session_name: Some("kv".to_string()),
        ssh_host: None,
        worktree_path: None,
        project_repo_path: None,
    }
}

impl Harness {
    fn new() -> Self {
        let tasks = Arc::new(FakeTasks {
            task: Mutex::new(Some(base_task())),
        });
        let runner = ScriptedShellRunner::new();
        let terminals = Arc::new(FakeTerminals::default());
        *terminals.attachment.lock().unwrap() = Some(TaskTerminalAttachment {
            pid: 9000,
            session_type: SessionType::Tmux,
            session_name: "kv".to_string(),
        });
        let herdr = Arc::new(FakeHerdr {
            panes: Mutex::new(Vec::new()),
            pane_list_calls: AtomicUsize::new(0),
            responder: Mutex::new(Box::new(|_| Value::Null)),
            command_calls: AtomicUsize::new(0),
        });
        let bridge = Arc::new(FakeBridge::default());
        let reader = PaneChatSurfaceReader::new(
            tasks.clone(),
            runner.clone(),
            terminals.clone(),
            herdr.clone(),
            bridge.clone(),
        );
        Self {
            tasks,
            runner,
            terminals,
            herdr,
            bridge,
            reader,
        }
    }

    fn set_task(&self, change: impl FnOnce(&mut PaneChatTask)) {
        let mut task = base_task();
        change(&mut task);
        *self.tasks.task.lock().unwrap() = Some(task);
    }

    fn set_session_type(&self, session_type: SessionType) {
        self.set_task(|task| task.session_type = Some(session_type));
    }

    /// 명령마다 답을 정한다. 앞서 등록한 응답기를 덮는다
    fn respond(&self, resolve: impl Fn(&str) -> String + Send + Sync + 'static) {
        self.runner
            .respond(move |command, _| Some(Ok(resolve(command))));
    }

    /// TS `withAttachedClient`. 붙은 client와 canonical session 조회에는 같은 세션으로 답한다
    fn respond_with_attached_client(
        &self,
        resolve: impl Fn(&str) -> String + Send + Sync + 'static,
    ) {
        self.respond(move |command| {
            if command.starts_with("tmux list-clients") {
                return "9000\tkv\t$1\t\t%3".to_string();
            }
            if command.starts_with("tmux display-message -p -t 'kv'") {
                return "$1\t".to_string();
            }
            resolve(command)
        });
    }

    fn set_herdr_responder(&self, responder: impl Fn(&[&str]) -> Value + Send + Sync + 'static) {
        *self.herdr.responder.lock().unwrap() = Box::new(responder);
    }

    fn surface(&self, pane_id: Option<&str>) -> Option<PaneChatSurface> {
        self.reader
            .read_pane_chat_surface("task", pane_id, true)
            .unwrap()
    }

    fn layout(&self, terminal_tab_id: Option<&str>) -> PaneChatLayout {
        self.reader
            .read_pane_chat_layout("task", terminal_tab_id)
            .unwrap()
    }

    fn screen(&self, surface: &PaneChatSurface) -> String {
        self.reader.read_pane_chat_screen(surface).unwrap()
    }

    fn commands(&self) -> Vec<String> {
        self.runner.commands()
    }
}

fn unavailable() -> PaneChatLayout {
    PaneChatLayout {
        active_pane_id: None,
        panes: Vec::new(),
    }
}

fn layout_pane(id: &str, tab_id: &str, bounds: (i64, i64, i64, i64)) -> PaneChatLayoutPane {
    let (left, top, width, height) = bounds;
    PaneChatLayoutPane {
        id: id.to_string(),
        tab_id: tab_id.to_string(),
        bounds: Some(PaneChatBounds {
            left,
            top,
            width,
            height,
        }),
    }
}

fn terminal_tab(id: &str, is_active: bool) -> TerminalTab {
    TerminalTab {
        id: id.to_string(),
        native_index: 0,
        name: id.to_string(),
        is_active,
    }
}

fn terminal_snapshot(pid: u32, screen: &str) -> TerminalTabSnapshot {
    TerminalTabSnapshot {
        pid,
        reading: ScreenReading {
            screen: screen.to_string(),
            input_line: String::new(),
            cursor_column: 0,
            cursor_row: 0,
            viewport_lines: Vec::new(),
            viewport_columns: 80,
            is_input_placeholder: false,
        },
    }
}

fn herdr_pane(pane_id: &str, tab_id: &str) -> HerdrTaskPaneTab {
    HerdrTaskPaneTab {
        pane_id: pane_id.to_string(),
        tab_id: tab_id.to_string(),
        agent_session: None,
    }
}

/// TS `tmuxLayout`. 같은 창 `@1`에 pane 둘, 다른 창 `@2`에 pane 하나가 있다
fn respond_with_tmux_layout(harness: &Harness, is_zoomed: bool, status_position: &'static str) {
    harness.respond_with_attached_client(move |command| {
        if command.contains("#{pane_left}") {
            return "%2\t@1\tclaude\t0\t0\t40\t24\tfirst\n%3\t@1\tcodex\t41\t0\t59\t24\tfirst\n%9\t@2\tcodex\t0\t0\t100\t24\tother".to_string();
        }
        if command.starts_with("tmux list-panes") {
            return "%2\t200\t1\t0\n%3\t300\t1\t1\n%9\t900\t0\t1".to_string();
        }
        format!("@1\t{}\t100\t24\t{status_position}\ton", u8::from(is_zoomed))
    });
}

// parity: TS-main-paneChatSurface-eee032
#[test]
fn tmux_layout_returns_attached_client_window_panes_without_selecting_or_resizing() {
    let harness = Harness::new();
    respond_with_tmux_layout(&harness, false, "bottom");

    let layout = harness.layout(None);

    assert_eq!(
        layout,
        PaneChatLayout {
            active_pane_id: Some("%3".to_string()),
            panes: vec![
                layout_pane("%2", "@1", (0, 0, 40, 24)),
                layout_pane("%3", "@1", (41, 0, 59, 24)),
            ],
        }
    );
    let is_mutating = |command: &String| {
        ["select-pane", "resize-", "new-session"]
            .iter()
            .any(|word| command.contains(word))
    };
    assert!(!harness.commands().iter().any(is_mutating));
}

// parity: TS-main-paneChatSurface-6f53d5
#[test]
fn tmux_layout_hides_siblings_under_zoom_and_offsets_top_status_row() {
    let harness = Harness::new();
    respond_with_tmux_layout(&harness, true, "top");

    assert_eq!(
        harness.layout(None),
        PaneChatLayout {
            active_pane_id: Some("%3".to_string()),
            panes: vec![layout_pane("%3", "@1", (0, 1, 100, 24))],
        }
    );
}

// parity: TS-main-paneChatSurface-e94f33
#[test]
fn plain_terminal_layout_fills_requested_tab_without_cell_geometry() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Terminal);
    *harness.terminals.tabs.lock().unwrap() =
        vec![terminal_tab("term-a", true), terminal_tab("term-b", false)];
    *harness.terminals.snapshot.lock().unwrap() = Some(terminal_snapshot(400, ""));

    let layout = harness.layout(Some("term-b"));

    assert_eq!(
        layout,
        PaneChatLayout {
            active_pane_id: Some("term-b".to_string()),
            panes: vec![PaneChatLayoutPane {
                id: "term-b".to_string(),
                tab_id: "term-b".to_string(),
                bounds: None,
            }],
        }
    );
    assert_eq!(
        *harness.terminals.snapshot_calls.lock().unwrap(),
        vec![("task".to_string(), "term-b".to_string())]
    );
    assert!(harness.commands().is_empty());
}

// parity: TS-main-paneChatSurface-3c6816
#[test]
fn layout_is_unavailable_when_attached_runtime_disappears() {
    let harness = Harness::new();
    *harness.terminals.attachment.lock().unwrap() = None;

    assert_eq!(harness.layout(None), unavailable());
    assert!(harness.commands().is_empty());

    harness.set_task(|task| task.session_name = None);
    assert_eq!(harness.layout(None), unavailable());
}

/// Zellij 조회 명령 가운데 `printf`로 시작하는 것은 pane과 탭을 함께 읽는 화면 배치 조회다
fn respond_with_zellij(harness: &Harness, panes: Value, tabs: Value) {
    harness.respond(move |command| {
        let response = match command.starts_with("printf") {
            true => json!([panes, tabs]),
            false => panes.clone(),
        };
        response.to_string()
    });
}

// parity: TS-main-paneChatSurface-bf8e8c
#[test]
fn zellij_layout_uses_content_bounds_and_excludes_other_tabs_and_fullscreen_siblings() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Zellij);
    let panes = json!([
        { "id": 2, "is_plugin": false, "is_focused": true, "is_fullscreen": true, "tab_id": 1, "pane_x": 0, "pane_y": 0, "pane_columns": 100, "pane_rows": 24, "pane_content_x": 1, "pane_content_y": 1, "pane_content_columns": 98, "pane_content_rows": 22 },
        { "id": 3, "is_plugin": false, "tab_id": 1, "pane_x": 51, "pane_y": 0, "pane_columns": 49, "pane_rows": 24 },
        { "id": 4, "is_plugin": false, "tab_id": 2, "pane_x": 0, "pane_y": 0, "pane_columns": 100, "pane_rows": 24 },
    ]);
    let tabs = json!([{ "tab_id": 1, "active": true, "is_fullscreen_active": true, "are_floating_panes_visible": false }]);
    respond_with_zellij(&harness, panes, tabs);

    assert_eq!(
        harness.layout(None),
        PaneChatLayout {
            active_pane_id: Some("terminal_2".to_string()),
            panes: vec![layout_pane("terminal_2", "1", (1, 1, 98, 22))],
        }
    );
}

// parity: TS-main-paneChatSurface-6b2cb2
#[test]
fn zellij_layout_does_not_cover_visible_floating_plugin_with_tiled_chat() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Zellij);
    let panes = json!([
        { "id": 2, "is_plugin": false, "is_focused": true, "tab_id": 1, "pane_x": 0, "pane_y": 0, "pane_columns": 100, "pane_rows": 24 },
        { "id": 3, "is_plugin": true, "is_focused": true, "is_floating": true, "tab_id": 1, "pane_x": 10, "pane_y": 3, "pane_columns": 40, "pane_rows": 15 },
    ]);
    let tabs = json!([{ "tab_id": 1, "active": true, "are_floating_panes_visible": true }]);
    respond_with_zellij(&harness, panes, tabs);

    assert_eq!(harness.layout(None), unavailable());
}

/// Herdr 태스크 client가 `w1:p*` pane을 보고 있고, 화면 영역은 (26, 1)부터 100x23이다
fn prepare_herdr_layout(harness: &Harness, focused_pane: &str, layout: Value) {
    harness.set_session_type(SessionType::Herdr);
    *harness.terminals.herdr_focus.lock().unwrap() = Some(focused_pane.to_string());
    *harness.terminals.herdr_bounds.lock().unwrap() = Some(HerdrSurfaceBounds {
        left: 26,
        top: 1,
        width: 100,
        height: 23,
    });
    harness.set_herdr_responder(move |arguments| match arguments[1] {
        "process-info" => json!({ "process_info": { "shell_pid": 300 } }),
        _ => json!({ "layout": layout }),
    });
}

// parity: TS-main-paneChatSurface-f7b2a5
#[test]
fn herdr_zoomed_layout_stays_inside_client_visible_tab_area() {
    let harness = Harness::new();
    let layout = json!({ "area": { "x": 0, "y": 0, "width": 100, "height": 23 }, "focused_pane_id": "w1:p2", "zoomed": true, "panes": [
        { "pane_id": "w1:p1", "rect": { "x": 0, "y": 0, "width": 49, "height": 23 } },
        { "pane_id": "w1:p2", "rect": { "x": 50, "y": 0, "width": 50, "height": 23 } },
    ] });
    prepare_herdr_layout(&harness, "w1:p2", layout);
    *harness.herdr.panes.lock().unwrap() = vec![
        herdr_pane("w1:p1", "w1:t1"),
        herdr_pane("w1:p2", "w1:t1"),
        herdr_pane("w1:p9", "w1:t2"),
    ];

    assert_eq!(
        harness.layout(None),
        PaneChatLayout {
            active_pane_id: Some("w1:p2".to_string()),
            panes: vec![layout_pane("w1:p2", "w1:t1", (26, 1, 100, 23))],
        }
    );
}

// parity: TS-main-paneChatSurface-b5225c
#[test]
fn herdr_layout_is_unavailable_when_client_surface_size_is_stale() {
    let harness = Harness::new();
    let layout = json!({ "area": { "x": 0, "y": 0, "width": 156, "height": 23 }, "focused_pane_id": "w1:p1", "zoomed": false, "panes": [
        { "pane_id": "w1:p1", "rect": { "x": 0, "y": 0, "width": 156, "height": 23 } },
    ] });
    prepare_herdr_layout(&harness, "w1:p1", layout);
    *harness.herdr.panes.lock().unwrap() = vec![herdr_pane("w1:p1", "w1:t1")];

    assert_eq!(harness.layout(None), unavailable());
}

/// tmux pane `%3`의 커서·크기와 화면 캡처를 `capture`로 답한다
fn respond_with_tmux_capture(harness: &Harness, capture: Arc<Mutex<String>>) {
    harness.respond(move |command| {
        if command.starts_with("tmux list-panes") {
            return "%3\t300\t1\t1".to_string();
        }
        if command.contains("#{cursor_x}") {
            return capture.lock().unwrap().clone();
        }
        "visible screen".to_string()
    });
}

fn codex_composer_capture(is_dim: bool) -> String {
    let dim = if is_dim { "\x1b[2m" } else { "" };
    format!("2\t0\t80\t4\n› {dim}Ask Codex to do anything\x1b[0m\n\n  footer\n\n")
}

// parity: TS-main-paneChatSurface-144dd0
#[test]
fn tmux_capture_reads_cursor_viewport_width_and_native_placeholder_style_together() {
    let harness = Harness::new();
    let capture = Arc::new(Mutex::new(codex_composer_capture(true)));
    respond_with_tmux_capture(&harness, capture.clone());

    let surface = harness.surface(Some("%3")).unwrap();
    assert_eq!(surface.cursor_row, Some(0));
    assert_eq!(surface.cursor_column, Some(2));
    assert_eq!(surface.viewport_columns, Some(80));
    assert!(surface.is_input_placeholder);
    assert_eq!(
        surface.input_line.as_deref(),
        Some("› Ask Codex to do anything")
    );
    assert_eq!(
        surface.viewport_lines,
        Some(vec![
            "› Ask Codex to do anything".to_string(),
            String::new(),
            "  footer".to_string(),
            String::new(),
        ])
    );

    *capture.lock().unwrap() = codex_composer_capture(false);
    let surface = harness.surface(Some("%3")).unwrap();
    assert!(!surface.is_input_placeholder);
    assert_eq!(
        surface.input_line.as_deref(),
        Some("› Ask Codex to do anything")
    );
}

// parity: TS-main-paneChatSurface-2ef6e6
#[test]
fn claude_dim_suggestion_after_reply_differs_from_typed_text() {
    // 실제 화면은 `❯` 뒤에 일반 공백이 아닌 no-break space(U+00A0)를 그린다
    let harness = Harness::new();
    let border = format!("\x1b[38;5;244m{}", "─".repeat(100));
    let status =
        "\x1b[39m  \x1b[38;5;220m⏵⏵ auto mode on\x1b[38;5;246m (shift+tab to cycle)\x1b[39m";
    let capture = Arc::new(Mutex::new(format!(
        "2\t1\t100\t4\n{border}\n\x1b[39m❯\u{a0}\x1b[2m숨김 폴더 포함하면 몇 개야?\x1b[0m\n{border}\n{status}"
    )));
    respond_with_tmux_capture(&harness, capture.clone());

    let surface = harness.surface(Some("%3")).unwrap();
    assert_eq!(surface.cursor_row, Some(1));
    assert_eq!(surface.cursor_column, Some(2));
    assert!(surface.is_input_placeholder);
    assert_eq!(
        surface.input_line.as_deref(),
        Some("❯\u{a0}숨김 폴더 포함하면 몇 개야?")
    );

    *capture.lock().unwrap() =
        format!("5\t1\t100\t4\n{border}\n\x1b[39m❯\u{a0}abc\n{border}\n{status}");
    let surface = harness.surface(Some("%3")).unwrap();
    assert_eq!(surface.cursor_column, Some(5));
    assert!(!surface.is_input_placeholder);
    assert_eq!(surface.input_line.as_deref(), Some("❯\u{a0}abc"));
}

// parity: TS-main-paneChatSurface-ec448f
#[test]
fn tmux_selects_focused_pane_in_active_window_instead_of_first_pane() {
    let harness = Harness::new();
    harness.respond_with_attached_client(|command| match command.starts_with("tmux list-panes") {
        true => "%1\t100\t0\t1\n%2\t200\t1\t0\n%3\t300\t1\t1".to_string(),
        false => "exact pane screen".to_string(),
    });

    let surface = harness.surface(None).unwrap();
    assert_eq!(surface.pane_id, "%3");
    assert_eq!(surface.root_pid, Some(300));
    let last_call = harness.runner.calls().pop().unwrap();
    assert!(last_call.command.contains(" -t '%3'"));
    assert_eq!(last_call.ssh_host, None);

    assert_eq!(harness.screen(&surface), "exact pane screen");
    let last_command = harness.commands().pop().unwrap();
    assert!(last_command.contains("capture-pane -pe -S -2000 -t '%3'"));
}

// parity: TS-main-paneChatSurface-0cdde1
#[test]
fn tmux_and_zellij_surface_checks_do_not_read_scrollback() {
    let harness = Harness::new();
    harness.respond(|command| match command.starts_with("tmux list-panes") {
        true => "%3\t300\t1\t1".to_string(),
        false => "2\t0\t80\t1\n› ".to_string(),
    });
    let surface = harness.surface(Some("%3")).unwrap();
    assert_eq!(
        (surface.pane_id.as_str(), surface.screen.as_str()),
        ("%3", "")
    );
    assert!(
        !harness
            .commands()
            .iter()
            .any(|command| command.contains("capture-pane -pe -S"))
    );

    harness.runner.clear_calls();
    harness.set_session_type(SessionType::Zellij);
    harness.respond(|_| {
        json!([[{ "id": 7, "is_plugin": false, "is_focused": true, "tab_id": 0 }], [{ "tab_id": 0, "active": true }]]).to_string()
    });
    let surface = harness.surface(None).unwrap();
    assert_eq!(
        (surface.pane_id.as_str(), surface.screen.as_str()),
        ("terminal_7", "")
    );
    assert!(
        !harness
            .commands()
            .iter()
            .any(|command| command.contains("dump-screen"))
    );
}

// parity: TS-main-paneChatSurface-d6fe40
#[test]
fn tmux_rejects_requested_pane_outside_task_session() {
    let harness = Harness::new();
    harness.respond(|_| "%1\t100\t1\t1".to_string());

    assert_eq!(harness.surface(Some("%999")), None);
    assert_eq!(harness.commands().len(), 1);
}

// parity: TS-main-paneChatSurface-8f4d9e
#[test]
fn inspecting_chat_does_not_create_missing_session() {
    let harness = Harness::new();
    harness.set_task(|task| task.session_name = None);

    assert_eq!(harness.surface(None), None);
    assert!(harness.commands().is_empty());
}

// parity: TS-main-paneChatSurface-3f0c87
#[test]
fn plain_terminal_reads_active_owned_tab_and_its_existing_pty() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Terminal);
    *harness.terminals.tabs.lock().unwrap() =
        vec![terminal_tab("task-1", false), terminal_tab("task-2", true)];
    *harness.terminals.snapshot.lock().unwrap() = Some(terminal_snapshot(400, "plain CLI screen"));

    let surface = harness.surface(None).unwrap();

    assert_eq!(surface.pane_id, "task-2");
    assert_eq!(surface.root_pid, Some(400));
    assert_eq!(surface.screen, "plain CLI screen");
    assert_eq!(
        *harness.terminals.snapshot_calls.lock().unwrap(),
        vec![("task".to_string(), "task-2".to_string())]
    );
}

fn single_zellij_viewport() -> String {
    json!([[{ "id": 7, "is_plugin": false, "is_focused": true, "tab_id": 0 }], [{ "tab_id": 0, "active": true }]]).to_string()
}

// parity: TS-main-paneChatSurface-ed4317
#[test]
fn zellij_screen_stays_available_without_process_identity() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Zellij);
    harness.respond(|command| match command.starts_with("printf") {
        true => single_zellij_viewport(),
        false => "zellij screen".to_string(),
    });

    let surface = harness.surface(None).unwrap();

    assert_eq!(surface.pane_id, "terminal_7");
    assert_eq!(surface.root_pid, None);
    assert_eq!(harness.screen(&surface), "zellij screen");
}

// parity: TS-main-paneChatSurface-54d006
#[test]
fn zellij_selects_active_tab_even_when_inactive_tabs_keep_a_focused_pane() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Zellij);
    let panes = json!([
        { "id": 0, "is_plugin": false, "is_focused": true, "tab_id": 0 },
        { "id": 1, "is_plugin": false, "is_focused": true, "tab_id": 1 },
    ]);
    harness.respond(move |command| {
        if command.starts_with("printf") {
            return json!([panes, [{ "tab_id": 0, "active": false }, { "tab_id": 1, "active": true }]]).to_string();
        }
        if command.contains("list-panes") {
            return panes.to_string();
        }
        "active tab screen".to_string()
    });

    let surface = harness.surface(None).unwrap();

    assert_eq!(surface.pane_id, "terminal_1");
    assert_eq!(harness.screen(&surface), "active tab screen");
}

const ZELLIJ_PANE_ROOT_LOOKUP: &str = "kanvibe-zellij-pane-root";

/// Zellij `list-panes` JSON에는 PID가 없어, 고른 pane의 환경으로 찾은 root PID 하나를 쓴다
#[test]
fn zellij_surface_uses_the_single_root_found_from_pane_environment() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Zellij);
    harness.respond(|command| {
        if command.contains(ZELLIJ_PANE_ROOT_LOOKUP) {
            return "4242\n".to_string();
        }
        match command.starts_with("printf") {
            true => single_zellij_viewport(),
            false => json!([{ "id": 7, "is_plugin": false, "tab_id": 0 }]).to_string(),
        }
    });

    let focused = harness.surface(None).unwrap();
    let requested = harness.surface(Some("terminal_7")).unwrap();

    assert_eq!(focused.root_pid, Some(4242));
    assert_eq!(requested.root_pid, Some(4242));
    let lookups: Vec<String> = harness
        .commands()
        .into_iter()
        .filter(|command| command.contains(ZELLIJ_PANE_ROOT_LOOKUP))
        .collect();
    assert_eq!(lookups.len(), 2);
    assert!(lookups[0].ends_with("kanvibe-zellij-pane-root 'kv' 7"));
}

/// 조회가 PID 하나로 좁혀지지 않거나 실패하면 root는 모르는 채로 두고, pane과 화면은 그대로 읽는다
#[test]
fn zellij_root_stays_unknown_unless_the_lookup_narrows_to_one_pid() {
    for lookup in [
        Ok(""),
        Ok("100\n200\n"),
        Ok("not a pid"),
        Err("lookup failed"),
    ] {
        let harness = Harness::new();
        harness.set_session_type(SessionType::Zellij);
        harness.runner.respond(move |command, _| {
            if command.contains(ZELLIJ_PANE_ROOT_LOOKUP) {
                return Some(lookup.map(str::to_string).map_err(ExecGitError::new));
            }
            Some(Ok(match command.starts_with("printf") {
                true => single_zellij_viewport(),
                false => "zellij screen".to_string(),
            }))
        });

        let surface = harness.surface(None).unwrap();

        assert_eq!(surface.pane_id, "terminal_7", "{lookup:?}");
        assert_eq!(surface.root_pid, None, "{lookup:?}");
        assert_eq!(harness.screen(&surface), "zellij screen");
    }
}

/// 1초마다 도는 배치 조회는 PID가 필요 없으므로 root를 찾지 않는다
#[test]
fn zellij_layout_poll_does_not_look_up_the_pane_root() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Zellij);
    let panes = json!([
        { "id": 2, "is_plugin": false, "is_focused": true, "tab_id": 1, "pane_x": 0, "pane_y": 0, "pane_columns": 100, "pane_rows": 24 },
    ]);
    let tabs = json!([{ "tab_id": 1, "active": true }]);
    respond_with_zellij(&harness, panes, tabs);

    let layout = harness.layout(None);

    assert_eq!(layout.active_pane_id.as_deref(), Some("terminal_2"));
    assert!(
        !harness
            .commands()
            .iter()
            .any(|command| command.contains(ZELLIJ_PANE_ROOT_LOOKUP)),
        "{:?}",
        harness.commands()
    );
}

// parity: TS-main-paneChatSurface-721547
#[test]
fn tmux_and_zellij_screens_strip_ansi_colors_and_osc_links() {
    let harness = Harness::new();
    harness.respond_with_attached_client(|command| match command.starts_with("tmux list-panes") {
        true => "%3\t300\t1\t1".to_string(),
        false => {
            "\x1b[32mSelected model\x1b[0m\n\x1b]8;;https://example.com\x07Safe link\x1b]8;;\x07"
                .to_string()
        }
    });
    let surface = harness.surface(None).unwrap();
    assert_eq!(harness.screen(&surface), "Selected model\nSafe link");

    harness.set_session_type(SessionType::Zellij);
    harness.respond(|command| match command.starts_with("printf") {
        true => single_zellij_viewport(),
        false => "\x1b[32mZellij model\x1b[0m".to_string(),
    });
    let surface = harness.surface(None).unwrap();
    assert_eq!(harness.screen(&surface), "Zellij model");
}

// parity: TS-main-paneChatSurface-c3be5d
#[test]
fn grouped_tmux_client_active_pane_wins_over_canonical_session_pane() {
    let harness = Harness::new();
    harness.respond(|command| {
        if command.starts_with("tmux list-clients") {
            return "9000\tkv-group\t$2\tgroup-1\t%3".to_string();
        }
        if command.starts_with("tmux display-message -p -t 'kv'") {
            return "$1\tgroup-1".to_string();
        }
        if command.starts_with("tmux list-panes") {
            return "%2\t200\t1\t1\n%3\t300\t0\t1".to_string();
        }
        "exact client pane".to_string()
    });

    let surface = harness.surface(None).unwrap();

    assert_eq!(surface.pane_id, "%3");
    assert_eq!(surface.root_pid, Some(300));
}

// parity: TS-main-paneChatSurface-64b7bf
#[test]
fn tmux_client_switched_to_unrelated_session_is_not_used() {
    let harness = Harness::new();
    harness.respond(|command| {
        if command.starts_with("tmux list-clients") {
            return "9000\tunrelated\t$9\tother-group\t%90".to_string();
        }
        if command.starts_with("tmux display-message -p -t 'kv'") {
            return "$1\tgroup-1".to_string();
        }
        "%2\t200\t1\t1".to_string()
    });

    assert_eq!(harness.surface(None), None);
}

// parity: TS-main-paneChatSurface-166601
#[test]
fn missing_attached_client_refuses_desktop_guess_but_allows_explicit_mobile_pane() {
    let harness = Harness::new();
    *harness.terminals.attachment.lock().unwrap() = None;
    assert_eq!(harness.surface(None), None);
    assert!(harness.commands().is_empty());

    harness.respond(|command| match command.starts_with("tmux list-panes") {
        true => "%3\t300\t1\t1".to_string(),
        false => "mobile selected pane".to_string(),
    });
    let surface = harness.surface(Some("%3")).unwrap();
    assert_eq!(surface.pane_id, "%3");
    assert_eq!(surface.root_pid, Some(300));
    assert!(
        !harness
            .commands()
            .iter()
            .any(|command| command.starts_with("tmux list-clients"))
    );
}

// parity: TS-main-paneChatSurface-d7e37e
#[test]
fn herdr_reads_input_line_and_cursor_from_reported_pane_native_frame() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Herdr);
    *harness.terminals.herdr_focus.lock().unwrap() = Some("w1:p2".to_string());
    let mut reported_pane = herdr_pane("w1:p2", "w1:t2");
    reported_pane.agent_session = Some(HerdrAgentSession {
        provider: AiSessionProvider::Codex,
        session_id: "exact-thread".to_string(),
    });
    *harness.herdr.panes.lock().unwrap() =
        vec![herdr_pane("w1:p1", "w1:t1"), reported_pane.clone()];
    harness.set_herdr_responder(|_| {
        json!({ "process_info": { "pane_id": "w1:p2", "shell_pid": 300, "foreground_processes": [{ "pid": 300 }] } })
    });
    *harness.bridge.frame.lock().unwrap() = Some(HerdrPaneFrame {
        output: "\x1b[2J\x1b[HOpenAI Codex\r\nModel: native\r\n› \x1b[3;3H\x1b[?25h".to_string(),
        width: 80,
        height: 24,
    });

    let surface = harness.surface(None).unwrap();

    assert_eq!(surface.pane_id, "w1:p2");
    assert_eq!(surface.root_pid, Some(300));
    assert_eq!(surface.agent_session, reported_pane.agent_session);
    assert_eq!(surface.input_line.as_deref(), Some("› "));
    assert_eq!(surface.cursor_column, Some(2));
    assert_eq!(surface.screen, "OpenAI Codex\nModel: native\n›");
    assert_eq!(
        *harness.bridge.frame_calls.lock().unwrap(),
        vec![("task".to_string(), "w1:p2".to_string())]
    );
    // 폴링마다 도는 경로라 pane 식별자 목록 한 번과 선택한 pane의 프로세스 조회 한 번만 쓴다.
    assert_eq!(harness.herdr.pane_list_calls.load(Ordering::SeqCst), 1);
    assert_eq!(harness.herdr.command_calls.load(Ordering::SeqCst), 1);
}

// parity: TS-main-paneChatSurface-263817
#[test]
fn herdr_without_exact_client_focus_does_not_fall_back_to_server_focus() {
    let harness = Harness::new();
    harness.set_session_type(SessionType::Herdr);
    *harness.herdr.panes.lock().unwrap() = vec![herdr_pane("w1:p1", "w1:t1")];
    *harness.terminals.herdr_focus.lock().unwrap() = None;

    assert_eq!(harness.surface(None), None);
    assert!(harness.bridge.frame_calls.lock().unwrap().is_empty());
    // 확인할 pane이 없으면 원격 pane 목록을 읽지 않는다.
    assert_eq!(harness.herdr.pane_list_calls.load(Ordering::SeqCst), 0);
}
