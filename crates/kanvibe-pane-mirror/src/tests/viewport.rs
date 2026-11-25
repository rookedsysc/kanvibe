//! 모바일 화면 크기

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use kanvibe_domain::task::SessionType;
use kanvibe_terminal::pane_mirror::build_screen_redraw;
use kanvibe_terminal::terminal_registry::TerminalClient;
use serde_json::{Value, json};

use super::fakes::*;
use super::*;
use crate::error::{PaneMirrorError, SurfaceUnavailableReason};
use crate::surfaces::SurfaceQuery;

/// zellij 세션 상태. 탭 크기와 floating pane 좌표를 명령에 맞춰 바꾼다
struct ZellijSession {
    floating_geometry: Mutex<BTreeMap<i64, (i64, i64, i64, i64)>>,
    tab_sizes: Mutex<BTreeMap<i64, (i64, i64)>>,
    focused_pane: AtomicI64,
}

fn number_after(command: &str, prefix: &str) -> Option<i64> {
    command
        .split(prefix)
        .nth(1)?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn first_pty_size(ptys: &FakePtySpawner) -> (i64, i64) {
    match ptys.count() {
        0 => (120, 30),
        _ => {
            let (columns, rows) = ptys.pty(0).size();
            (i64::from(columns), i64::from(rows))
        }
    }
}

impl ZellijSession {
    fn panes(&self, ptys: &FakePtySpawner, is_floating: bool, has_multiple_tabs: bool) -> Value {
        if is_floating {
            let geometry = self.floating_geometry.lock().unwrap();
            return Value::Array(geometry.iter().map(|(id, (left, top, cols, rows))| json!({
                "id": id, "is_plugin": false, "is_floating": true, "tab_id": 0, "pane_x": left, "pane_y": top,
                "pane_columns": cols, "pane_rows": rows, "pane_content_columns": cols - 2, "pane_content_rows": rows - 2,
            })).collect());
        }
        let ids: &[i64] = if has_multiple_tabs { &[1, 2] } else { &[1] };
        let sizes = self.tab_sizes.lock().unwrap();
        Value::Array(ids.iter().map(|id| {
            let (cols, rows) = if has_multiple_tabs { sizes[&(id - 1)] } else { first_pty_size(ptys) };
            json!({
                "id": id, "is_plugin": false, "tab_id": id - 1, "pane_x": 0, "pane_y": 1, "pane_content_x": 0, "pane_content_y": 2,
                "pane_columns": cols / 2, "pane_rows": rows - 2, "pane_content_columns": cols / 2 - 1, "pane_content_rows": rows - 3,
            })
        }).collect())
    }

    fn tabs(&self, ptys: &FakePtySpawner, has_multiple_tabs: bool) -> Value {
        let ids: &[i64] = if has_multiple_tabs { &[0, 1] } else { &[0] };
        let sizes = self.tab_sizes.lock().unwrap();
        let focused = self.focused_pane.load(Ordering::SeqCst);
        Value::Array(ids.iter().map(|tab_id| {
            let (cols, rows) = if has_multiple_tabs { sizes[tab_id] } else { first_pty_size(ptys) };
            json!({
                "tab_id": tab_id, "active": *tab_id == focused - 1, "viewport_columns": cols, "viewport_rows": rows - 2,
                "display_area_columns": cols, "display_area_rows": rows,
            })
        }).collect())
    }

    fn change_floating(&self, command: &str) {
        let id = number_after(command, "--pane-id 'terminal_").unwrap();
        let geometry = (
            number_after(command, "--x ").unwrap(),
            number_after(command, "--y ").unwrap(),
            number_after(command, "--width ").unwrap(),
            number_after(command, "--height ").unwrap(),
        );
        self.floating_geometry.lock().unwrap().insert(id, geometry);
    }
}

fn stub_zellij_viewport(
    harness: &Harness,
    is_floating: bool,
    has_multiple_tabs: bool,
) -> Arc<ZellijSession> {
    let session = Arc::new(ZellijSession {
        floating_geometry: Mutex::new(BTreeMap::from([(1, (30, 7, 60, 14)), (2, (3, 2, 40, 12))])),
        tab_sizes: Mutex::new(BTreeMap::from([(0, (120, 30)), (1, (120, 30))])),
        focused_pane: AtomicI64::new(1),
    });
    harness.set_task(task(
        "zellij-viewport",
        Some(SessionType::Zellij),
        Some("zellij-viewport"),
    ));
    let responding = Arc::clone(&session);
    let ptys = Arc::clone(&harness.ptys);
    harness.runner.replace(move |command, _| {
        let output = if command.contains("--version") {
            "zellij 0.45.1".to_string()
        } else if command.contains("list-sessions") {
            "zellij-viewport [Created 1m ago]".to_string()
        } else if command.contains("go-to-tab-by-id") {
            let focused = number_after(command, "go-to-tab-by-id '").unwrap() + 1;
            responding.focused_pane.store(focused, Ordering::SeqCst);
            let size = first_pty_size(&ptys);
            responding
                .tab_sizes
                .lock()
                .unwrap()
                .insert(focused - 1, size);
            String::new()
        } else if command.contains("change-floating-pane-coordinates") {
            responding.change_floating(command);
            String::new()
        } else if command.contains("list-tabs") {
            json!([
                responding.panes(&ptys, is_floating, has_multiple_tabs),
                responding.tabs(&ptys, has_multiple_tabs)
            ])
            .to_string()
        } else if command.contains("list-panes") {
            responding
                .panes(&ptys, is_floating, has_multiple_tabs)
                .to_string()
        } else {
            "zellij resized screen".to_string()
        };
        Some(Ok(output))
    });
    let registry = Arc::clone(&harness.registry);
    let resizing = Arc::clone(&session);
    let ptys = Arc::clone(&harness.ptys);
    harness
        .attacher
        .set_override(move |task, _tab_id, client: Arc<dyn TerminalClient>| {
            let request = kanvibe_terminal::terminal_registry::LocalAttachRequest {
                task_id: task.id.clone(),
                tab_id: None,
                session_type: SessionType::Zellij,
                session_name: task.session_name.clone().unwrap_or_default(),
                working_directory: None,
                columns: Some(120),
                rows: Some(30),
                pane_layout: None,
            };
            let attachment = registry.attach_local_session(request, client)?;
            let session = Arc::clone(&resizing);
            *ptys.pty(0).on_resize.lock().unwrap() = Some(Arc::new(move |columns, rows| {
                let focused = session.focused_pane.load(Ordering::SeqCst);
                session
                    .tab_sizes
                    .lock()
                    .unwrap()
                    .insert(focused - 1, (i64::from(columns), i64::from(rows)));
            }));
            Ok(attachment)
        });
    session
}

fn last_resize(harness: &Harness) -> (u16, u16) {
    *harness.ptys.pty(0).resizes().last().expect("크기 변경")
}

fn has_layout_override(harness: &Harness) -> bool {
    harness
        .commands()
        .iter()
        .any(|command| command.contains("fullscreen") || command.contains("override-layout"))
}

// parity: TS-main-mobileBridgeService-06a1de
#[test]
fn zellij_split_pane_matches_the_real_grid_and_restores_the_desktop_grid_after_the_last_client() {
    let harness = Harness::new();
    stub_zellij_viewport(&harness, false, false);

    let surfaces = harness
        .mirror
        .get_task_surfaces("zellij-viewport", &SurfaceQuery::default())
        .unwrap();
    assert!(surfaces.can_resize);
    let viewport = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();
    assert!(
        viewport
            .resize(44, 70)
            .unwrap()
            .unwrap()
            .contains("zellij resized screen")
    );
    assert_eq!(last_resize(&harness), (90, 73));
    viewport.release().unwrap();

    assert_eq!(last_resize(&harness), (120, 30));
    assert!(
        harness
            .commands()
            .contains(&"zellij --session 'zellij-viewport' action go-to-tab-by-id '0'".to_string())
    );
    assert!(!has_layout_override(&harness));
}

// parity: TS-main-mobileBridgeService-ac09f6
#[test]
fn zellij_prefers_the_latest_mobile_request_and_returns_to_the_remaining_client_grid() {
    let harness = Harness::new();
    stub_zellij_viewport(&harness, false, false);
    let first = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();
    let second = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();

    first.resize(44, 70).unwrap();
    second.resize(80, 35).unwrap();
    assert_eq!(last_resize(&harness), (162, 38));
    second.release().unwrap();
    assert_eq!(last_resize(&harness), (90, 73));
    first.release().unwrap();

    assert_eq!(last_resize(&harness), (120, 30));
    assert_eq!(harness.attacher.call_count(), 1);
}

// parity: TS-main-mobileBridgeService-f8965c
#[test]
fn zellij_multiple_tabs_open_the_selected_pane_tab_on_the_desktop_and_fit_the_phone_grid() {
    let harness = Harness::new();
    let session = stub_zellij_viewport(&harness, false, true);

    let surfaces = harness
        .mirror
        .get_task_surfaces("zellij-viewport", &SurfaceQuery::default())
        .unwrap();
    assert_eq!(surfaces.tabs.len(), 2);
    assert!(surfaces.can_resize);
    let first = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();
    let second = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_2")
        .unwrap();
    first.resize(44, 70).unwrap();
    second.resize(80, 35).unwrap();
    assert_eq!(session.focused_pane.load(Ordering::SeqCst), 2);
    assert_eq!(last_resize(&harness), (162, 38));
    second.release().unwrap();
    assert_eq!(session.focused_pane.load(Ordering::SeqCst), 1);
    assert_eq!(last_resize(&harness), (90, 73));
    first.release().unwrap();

    assert_eq!(last_resize(&harness), (120, 30));
}

// parity: TS-main-mobileBridgeService-073481
#[test]
fn zellij_opens_the_newly_selected_tab_even_when_the_grid_already_matches() {
    let harness = Harness::new();
    let session = stub_zellij_viewport(&harness, false, true);
    let viewport = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_2")
        .unwrap();

    viewport.resize(59, 27).unwrap();

    assert_eq!(session.focused_pane.load(Ordering::SeqCst), 2);
    viewport.release().unwrap();
}

// parity: TS-main-mobileBridgeService-03c43e
#[test]
fn zellij_keeps_adjusting_the_grid_when_the_same_tab_is_selected_again() {
    let harness = Harness::new();
    let session = stub_zellij_viewport(&harness, false, true);
    let viewport = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_2")
        .unwrap();

    viewport.resize(59, 27).unwrap();
    viewport.resize(80, 35).unwrap();

    assert_eq!(session.focused_pane.load(Ordering::SeqCst), 2);
    assert_eq!(last_resize(&harness), (162, 38));
    assert_eq!(
        harness.runner.commands_containing("go-to-tab-by-id"),
        vec!["zellij --session 'zellij-viewport' action go-to-tab-by-id '1'"; 2]
    );
    viewport.release().unwrap();
}

// parity: TS-main-mobileBridgeService-dd0fb4
#[test]
fn zellij_tab_switch_failure_is_not_success_and_release_restores_the_desktop_size() {
    let harness = Harness::new();
    stub_zellij_viewport(&harness, false, true);
    harness.runner.respond(|command, _| {
        command
            .contains("go-to-tab-by-id")
            .then(|| Err("tab closed".to_string()))
    });
    let viewport = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_2")
        .unwrap();

    let error = viewport.resize(59, 27).unwrap_err();
    viewport.release().unwrap();

    assert!(error.to_string().contains("tab closed"));
    assert_eq!(last_resize(&harness), (120, 30));
}

fn floating_commands(harness: &Harness) -> Vec<String> {
    harness
        .runner
        .commands_containing("change-floating-pane-coordinates")
}

// parity: TS-main-mobileBridgeService-7d0a21
#[test]
fn zellij_pinned_floating_pane_fits_exactly_and_restores_its_position_and_size() {
    let harness = Harness::new();
    stub_zellij_viewport(&harness, true, false);
    let first = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();
    let second = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();

    first.resize(44, 70).unwrap();
    assert_eq!(last_resize(&harness), (120, 81));
    second.resize(80, 24).unwrap();
    assert_eq!(last_resize(&harness), (120, 35));
    second.release().unwrap();
    assert_eq!(last_resize(&harness), (120, 81));
    first.release().unwrap();

    assert_eq!(last_resize(&harness), (120, 30));
    let prefix = "zellij --session 'zellij-viewport' action change-floating-pane-coordinates --pane-id 'terminal_1'";
    assert_eq!(
        floating_commands(&harness),
        vec![
            format!("{prefix} --x 30 --y 7 --width 46 --height 72"),
            format!("{prefix} --x 30 --y 7 --width 82 --height 26"),
            format!("{prefix} --x 30 --y 7 --width 46 --height 72"),
            format!("{prefix} --x 30 --y 7 --width 60 --height 14"),
        ]
    );
    assert!(!has_layout_override(&harness));
}

// parity: TS-main-mobileBridgeService-b8d97c
#[test]
fn zellij_alternating_floating_panes_each_restore_their_original_layout_at_the_end() {
    let harness = Harness::new();
    stub_zellij_viewport(&harness, true, false);
    let first = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();
    let second = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_2")
        .unwrap();
    first.resize(44, 70).unwrap();
    second.resize(80, 24).unwrap();
    second.release().unwrap();
    let count_before_final = floating_commands(&harness).len();

    first.release().unwrap();

    let restores = floating_commands(&harness)[count_before_final..].to_vec();
    assert_eq!(
        restores,
        vec![
            "zellij --session 'zellij-viewport' action change-floating-pane-coordinates --pane-id 'terminal_1' --x 30 --y 7 --width 60 --height 14",
            "zellij --session 'zellij-viewport' action change-floating-pane-coordinates --pane-id 'terminal_2' --x 3 --y 2 --width 40 --height 12",
        ]
    );
    assert_eq!(last_resize(&harness), (120, 30));
}

fn stub_tmux_viewport(harness: &Harness) {
    harness.set_task(tmux_task());
    harness.runner.replace(|command, _| {
        Some(Ok(if command.contains("list-panes") {
            MIRROR_PANE_LINE.to_string()
        } else if command.contains("window_layout") {
            "80\t24\tlayout-original\n".to_string()
        } else if command.contains("window_width") {
            "80 24 80 24".to_string()
        } else if command.contains("capture-pane") {
            "history-through-180\n43 69".to_string()
        } else {
            String::new()
        }))
    });
}

fn position_of(commands: &[String], needle: &str) -> usize {
    commands
        .iter()
        .position(|command| command.contains(needle))
        .expect(needle)
}

// parity: TS-main-mobileBridgeService-29e113
#[test]
fn tmux_takes_the_phone_size_and_restores_layout_and_size_policy_after_the_last_mobile() {
    let harness = Harness::new();
    stub_tmux_viewport(&harness);

    let viewport = harness.mirror.claim_pane_viewport("task-1", "%19").unwrap();
    assert!(
        viewport
            .resize(44, 70)
            .unwrap()
            .unwrap()
            .contains("history-through-180")
    );
    let commands = harness.commands();
    assert!(position_of(&commands, "capture-pane") > position_of(&commands, "resize-window"));
    assert!(
        commands
            .iter()
            .any(|command| command.contains("resize-pane") && command.contains("-x 44 -y 70"))
    );
    viewport.release().unwrap();

    let commands = harness.commands();
    assert!(commands.iter().any(|command| command.contains("select-layout") && command.contains("layout-original")));
    assert!(
        commands
            .iter()
            .any(|command| command.contains("window-size") && command.contains("-u"))
    );
}

/// tmux 기본 휠 바인딩의 갈림길: copy-mode, 대체 화면, 마우스 보고, SGR, UTF-8
fn stub_tmux_wheel(harness: &Harness, states: &[&str], views: &[&str]) {
    harness.set_task(tmux_task());
    let states = Mutex::new(
        states
            .iter()
            .map(|state| state.to_string())
            .collect::<VecDeque<_>>(),
    );
    let views = Mutex::new(
        views
            .iter()
            .map(|view| view.to_string())
            .collect::<VecDeque<_>>(),
    );
    harness.runner.replace(move |command, _| {
        Some(Ok(if command.contains("list-panes") {
            MIRROR_PANE_LINE.to_string()
        } else if command.contains("window_layout") {
            "80\t24\tlayout-original\n".to_string()
        } else if command.contains("window_width") {
            "80 24 80 24".to_string()
        } else if command.contains("#{scroll_position}") {
            views
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| "0  24".to_string())
        } else if command.contains("#{pane_in_mode}") {
            states.lock().unwrap().pop_front().unwrap_or_default()
        } else if command.contains("capture-pane") && command.contains(" -E ") {
            "copy-mode-screen".to_string()
        } else if command.contains("capture-pane") {
            "program-screen\n43 69".to_string()
        } else {
            String::new()
        }))
    });
}

fn wheel_commands(harness: &Harness) -> Vec<String> {
    harness
        .commands()
        .into_iter()
        .filter(|command| {
            ["send-keys", "copy-mode", "capture-pane"]
                .iter()
                .any(|needle| command.contains(needle))
        })
        .collect()
}

fn scroll(direction: &str, column: i64, row: i64) -> Value {
    json!({ "type": "scroll", "direction": direction, "column": column, "row": row })
}

// parity: TS-main-mobileBridgeService-66d531
#[test]
fn tmux_drops_the_wheel_for_an_alternate_screen_program_without_mouse_reporting() {
    let harness = Harness::new();
    stub_tmux_wheel(&harness, &["0 1 0 0 0"], &[]);
    let viewport = harness.mirror.claim_pane_viewport("task-1", "%19").unwrap();

    assert_eq!(viewport.scroll(&scroll("up", 4, 2)).unwrap(), None);
    assert!(wheel_commands(&harness).is_empty());
    viewport.release().unwrap();
}

// parity: TS-main-mobileBridgeService-344d75
#[test]
fn tmux_passes_the_wheel_report_in_the_program_mouse_encoding() {
    for (state, report) in [
        ("0 1 1 1 0", "\x1b[<64;5;3M"),
        ("0 1 1 0 1", "\x1b[M`%#"),
        ("0 0 1 0 0", "\x1b[M`%#"),
    ] {
        let harness = Harness::new();
        stub_tmux_wheel(&harness, &[state], &[]);
        let viewport = harness.mirror.claim_pane_viewport("task-1", "%19").unwrap();

        assert_eq!(viewport.scroll(&scroll("up", 4, 2)).unwrap(), None);
        assert_eq!(
            wheel_commands(&harness),
            vec![format!("tmux send-keys -t '%19' -l -- '{report}'")]
        );
        viewport.release().unwrap();
    }
}

// parity: TS-main-mobileBridgeService-e70cf8
#[test]
fn tmux_normal_screen_enters_copy_mode_on_wheel_up_and_ignores_wheel_down() {
    let harness = Harness::new();
    stub_tmux_wheel(&harness, &["0 0 0 0 0", "0 0 0 0 0"], &["1 0 24"]);
    let viewport = harness.mirror.claim_pane_viewport("task-1", "%19").unwrap();

    assert_eq!(
        viewport.scroll(&scroll("up", 0, 0)).unwrap(),
        Some(build_screen_redraw("copy-mode-screen", None))
    );
    assert_eq!(
        wheel_commands(&harness),
        vec![
            "tmux copy-mode -e -t '%19'",
            "tmux capture-pane -pe -S -0 -E 23 -t '%19'"
        ]
    );
    assert_eq!(viewport.scroll(&scroll("down", 0, 0)).unwrap(), None);
    assert_eq!(wheel_commands(&harness).len(), 2);
    viewport.release().unwrap();
}

// parity: TS-main-mobileBridgeService-d0df09
#[test]
fn tmux_copy_mode_moves_by_the_default_binding_and_resends_the_program_screen_at_the_bottom() {
    let harness = Harness::new();
    stub_tmux_wheel(&harness, &["1 0 0 0 0", "1 0 0 0 0"], &["1 5 24", "0  24"]);
    let viewport = harness.mirror.claim_pane_viewport("task-1", "%19").unwrap();

    assert_eq!(
        viewport.scroll(&scroll("up", 0, 0)).unwrap(),
        Some(build_screen_redraw("copy-mode-screen", None))
    );
    assert!(
        wheel_commands(&harness)
            .contains(&"tmux capture-pane -pe -S -5 -E 18 -t '%19'".to_string())
    );
    let left_copy_mode = viewport.scroll(&scroll("down", 0, 0)).unwrap().unwrap();

    let send_keys: Vec<String> = wheel_commands(&harness)
        .into_iter()
        .filter(|command| command.contains("send-keys"))
        .collect();
    assert_eq!(
        send_keys,
        vec![
            "tmux send-keys -X -N 5 -t '%19' scroll-up",
            "tmux send-keys -X -N 5 -t '%19' scroll-down"
        ]
    );
    assert!(left_copy_mode.contains("program-screen"));
    assert!(!left_copy_mode.contains("copy-mode-screen"));
    viewport.release().unwrap();
}

// parity: TS-main-mobileBridgeService-654d40
#[test]
fn zellij_wheel_goes_to_the_desktop_client_as_sgr_relative_to_the_pane_text_area() {
    let harness = Harness::new();
    stub_zellij_viewport(&harness, false, false);
    let viewport = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();
    viewport.resize(44, 70).unwrap();
    harness.ptys.pty(0).writes.lock().unwrap().clear();

    let screen = viewport.scroll(&scroll("up", 4, 2)).unwrap().unwrap();
    viewport.scroll(&scroll("down", 0, 0)).unwrap();

    assert_eq!(
        harness.ptys.pty(0).writes(),
        vec!["\x1b[<64;5;5M", "\x1b[<65;1;3M"]
    );
    assert!(screen.contains("zellij resized screen"));
    assert!(screen.contains("\x1b[?1049h"));
    assert!(harness.runner.commands_containing("go-to-tab-by-id").len() >= 3);
    viewport.release().unwrap();
}

// parity: TS-main-mobileBridgeService-07238e
#[test]
fn zellij_wheel_is_dropped_when_the_pane_list_omits_the_text_area_position() {
    let harness = Harness::new();
    stub_zellij_viewport(&harness, true, false);
    let viewport = harness
        .mirror
        .claim_pane_viewport("zellij-viewport", "terminal_1")
        .unwrap();
    viewport.resize(44, 70).unwrap();
    harness.ptys.pty(0).writes.lock().unwrap().clear();

    assert_eq!(viewport.scroll(&scroll("up", 4, 2)).unwrap(), None);
    assert!(harness.ptys.pty(0).writes().is_empty());
    viewport.release().unwrap();
}

// parity: TS-main-mobileBridgeService-9e0db5
#[test]
fn terminal_session_uses_the_latest_mobile_size_and_returns_to_the_remaining_mobile_and_desktop_sizes()
 {
    let harness = Harness::new();
    harness.set_task(task(
        "viewport-task",
        Some(SessionType::Terminal),
        Some("kanvibe-viewport"),
    ));
    let _subscriber = harness.subscribe("viewport-task", "viewport-task-1");
    let first = harness
        .mirror
        .claim_pane_viewport("viewport-task", "viewport-task-1")
        .unwrap();
    let second = harness
        .mirror
        .claim_pane_viewport("viewport-task", "viewport-task-1")
        .unwrap();

    assert_eq!(first.resize(44, 70).unwrap(), None);
    second.resize(80, 35).unwrap();
    second.release().unwrap();
    first.release().unwrap();

    assert_eq!(
        harness.ptys.pty(0).resizes(),
        vec![(44, 70), (80, 35), (44, 70), (120, 30)]
    );
    assert!(harness.ptys.pty(0).writes().is_empty());
}

// parity: TS-main-mobileBridgeService-59ade2
#[test]
fn panes_of_other_sessions_cannot_be_resized() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let error = harness
        .mirror
        .claim_pane_viewport("task-1", "%3")
        .err()
        .unwrap();

    assert_eq!(
        error,
        PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::SessionNotRunning)
    );
    assert!(
        !harness
            .commands()
            .iter()
            .any(|command| command.contains("resize-"))
    );
}

// parity: TS-main-mobileBridgeService-98a6d8
#[test]
fn desktop_resize_during_a_mobile_session_is_kept_and_applied_when_the_last_mobile_leaves() {
    let harness = Harness::new();
    harness.set_task(task(
        "priority-task",
        Some(SessionType::Terminal),
        Some("kanvibe-priority"),
    ));
    let _subscriber = harness.subscribe("priority-task", "priority-task-1");
    let desktop = Arc::new(RecordingClient::default());
    let (attachment, _) = harness
        .registry
        .attach_client_to_terminal_tab("priority-task", "priority-task-1", desktop.clone())
        .unwrap();
    let viewport = harness
        .mirror
        .claim_pane_viewport("priority-task", "priority-task-1")
        .unwrap();

    viewport.resize(44, 70).unwrap();
    harness.registry.client_message(
        &attachment,
        &format!(
            "\x01{}",
            json!({ "type": "resize", "cols": 100, "rows": 40 })
        ),
    );
    assert_eq!(harness.ptys.pty(0).resizes(), vec![(44, 70)]);
    viewport.release().unwrap();

    assert_eq!(harness.ptys.pty(0).resizes(), vec![(44, 70), (100, 40)]);
    harness.registry.client_closed(&attachment);
}

// parity: TS-main-mobileBridgeService-0ee52b
#[test]
fn reconnecting_after_two_mobiles_leave_together_is_not_overwritten_by_the_earlier_restore() {
    let harness = Harness::new();
    harness.set_task(tmux_task());
    let restore_gate = Gate::new();
    let restore_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let gate = Arc::clone(&restore_gate);
    let count = Arc::clone(&restore_count);
    harness.runner.replace(move |command, _| {
        if command.contains("select-layout") && count.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
            gate.pass();
        }
        Some(Ok(if command.contains("list-panes") {
            MIRROR_PANE_LINE.to_string()
        } else if command.contains("window_layout") {
            "80\t24\tlayout-original\n".to_string()
        } else if command.contains("window_width") {
            "80 24 80 24".to_string()
        } else {
            String::new()
        }))
    });
    let first = harness.mirror.claim_pane_viewport("task-1", "%19").unwrap();
    let second = Arc::new(harness.mirror.claim_pane_viewport("task-1", "%19").unwrap());
    first.resize(44, 70).unwrap();
    first.release().unwrap();
    let releasing = Arc::clone(&second);
    let second_release = std::thread::spawn(move || releasing.release());
    wait_until("두 번째 복원이 멈춘다", || {
        restore_gate.entered_count() == 1
    });

    let reconnected = Arc::new(harness.mirror.claim_pane_viewport("task-1", "%19").unwrap());
    let has_resized = Arc::new(AtomicBool::new(false));
    let resizing = Arc::clone(&reconnected);
    let resized = Arc::clone(&has_resized);
    let resize = std::thread::spawn(move || {
        resizing.resize(60, 50).unwrap();
        resized.store(true, Ordering::SeqCst);
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    let has_jumped_queue = has_resized.load(Ordering::SeqCst);
    restore_gate.open();
    second_release.join().unwrap().unwrap();
    resize.join().unwrap();
    reconnected.release().unwrap();

    assert!(!has_jumped_queue);
}
