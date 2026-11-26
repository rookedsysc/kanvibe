use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use regex::Regex;
use serde_json::{Value, json};

use super::*;
use crate::remote_terminal_protocol::{
    HerdrFrameSize, TerminalScrollRequest, build_herdr_scroll_control_record,
    build_mouse_wheel_report, build_wheel_delegated_screen_redraw, parse_terminal_scroll_request,
};

const INJECTION_PANE_ID: &str = "%1'; rm -rf /; '";
const QUOTED_INJECTION_PANE_ID: &str = "'%1'\"'\"'; rm -rf /; '\"'\"''";
const CLEAR: &str = "\x1b[H\x1b[2J\x1b[3J";

fn herdr(arguments: &[&str]) -> String {
    build_herdr_command("session", arguments).unwrap()
}

fn zellij_viewport_pane() -> Value {
    json!({ "id": 7, "is_plugin": false, "tab_id": 2, "pane_columns": 60, "pane_rows": 28,
        "pane_content_columns": 59, "pane_content_rows": 27 })
}

fn zellij_viewport_tab() -> Value {
    json!({ "tab_id": 2, "viewport_columns": 120, "viewport_rows": 28,
        "display_area_columns": 120, "display_area_rows": 30 })
}

fn with_fields(base: Value, overrides: Value) -> Value {
    let mut merged = base;
    for (key, value) in overrides.as_object().unwrap() {
        merged[key] = value.clone();
    }
    merged
}

#[test]
fn zellij_select_tab_names_session_and_tab() {
    assert_eq!(
        build_zellij_select_tab_command("task", "2"),
        "zellij --session 'task' action go-to-tab-by-id '2'"
    );
}

#[test]
fn zellij_floating_viewport_separates_position_and_bordered_size() {
    let output = json!([[{ "id": 7, "is_plugin": false, "is_floating": true, "tab_id": 0,
        "pane_x": 30, "pane_y": 7, "pane_columns": 60, "pane_rows": 14,
        "pane_content_columns": 58, "pane_content_rows": 12 }],
        [{ "tab_id": 0, "viewport_columns": 120, "viewport_rows": 28,
            "display_area_columns": 120, "display_area_rows": 30 }]]);

    let state = parse_zellij_viewport_state(&output.to_string(), "terminal_7").unwrap();

    assert_eq!(state.floating, Some(CellPosition { left: 30, top: 7 }));
    assert_eq!((state.pane_cols, state.pane_rows), (60, 14));
    let geometry = FloatingPaneGeometry {
        left: 30,
        top: 7,
        cols: 46,
        rows: 72,
    };
    assert_eq!(
        build_zellij_resize_floating_pane_command("task", "terminal_7", &geometry),
        "zellij --session 'task' action change-floating-pane-coordinates --pane-id 'terminal_7' --x 30 --y 7 --width 46 --height 72"
    );
}

#[test]
fn zellij_viewport_separates_program_grid_from_split_and_decoration_sizes() {
    let plugin = with_fields(zellij_viewport_pane(), json!({ "is_plugin": true }));
    let output = json!([[zellij_viewport_pane(), plugin], [zellij_viewport_tab()]]);

    assert_eq!(
        parse_zellij_viewport_state(&output.to_string(), "terminal_7"),
        Some(ZellijViewportState {
            tab_id: "2".to_string(),
            cols: 59,
            rows: 27,
            pane_cols: 60,
            pane_rows: 28,
            viewport_cols: 120,
            viewport_rows: 28,
            display_cols: 120,
            display_rows: 30,
            content_origin: None,
            floating: None,
        })
    );
}

#[test]
fn zellij_viewport_rejects_incomplete_geometry() {
    let zero_rows = with_fields(zellij_viewport_pane(), json!({ "pane_content_rows": 0 }));
    let outputs = [
        "bad json".to_string(),
        "[]".to_string(),
        json!([[zellij_viewport_pane()], []]).to_string(),
        json!([[zero_rows], [zellij_viewport_tab()]]).to_string(),
    ];
    for output in outputs {
        assert_eq!(
            parse_zellij_viewport_state(&output, "terminal_7"),
            None,
            "{output}"
        );
    }
}

#[test]
fn zellij_viewport_reads_content_origin_only_when_listed() {
    let placed = with_fields(
        zellij_viewport_pane(),
        json!({ "pane_content_x": 61, "pane_content_y": 2 }),
    );
    let placed_output = json!([[placed], [zellij_viewport_tab()]]).to_string();
    let plain_output = json!([[zellij_viewport_pane()], [zellij_viewport_tab()]]).to_string();

    assert_eq!(
        parse_zellij_viewport_state(&placed_output, "terminal_7")
            .unwrap()
            .content_origin,
        Some(CellPosition { left: 61, top: 2 })
    );
    assert_eq!(
        parse_zellij_viewport_state(&plain_output, "terminal_7")
            .unwrap()
            .content_origin,
        None
    );
}

#[test]
fn zellij_viewport_does_not_use_another_panes_geometry() {
    let output = json!([[zellij_viewport_pane()], [zellij_viewport_tab()]]).to_string();
    assert_eq!(parse_zellij_viewport_state(&output, "terminal_8"), None);
}

#[test]
fn zellij_read_viewport_quotes_session_and_reads_panes_and_tabs_together() {
    let command = build_zellij_read_viewport_command("task's session");
    assert!(command.contains(&build_zellij_list_mirror_panes_command("task's session")));
    assert!(command.contains("action list-tabs --json -a"));
    assert!(command.contains("printf '['"));
}

fn mirror_pane(
    id: &str,
    tab_id: &str,
    tab_name: &str,
    command: &str,
    geometry: [i64; 4],
) -> MirrorPane {
    let [left, top, width, height] = geometry;
    MirrorPane {
        id: id.to_string(),
        tab_id: tab_id.to_string(),
        tab_name: tab_name.to_string(),
        command: command.to_string(),
        left,
        top,
        width,
        height,
    }
}

#[test]
fn tmux_pane_list_reads_every_window_pane_with_geometry() {
    let output = [
        "%19\t@10\tbash\t0\t0\t50\t30\tbash",
        "%20\t@10\tclaude\t51\t0\t49\t30\tbash",
        "%21\t@11\tbash\t0\t0\t100\t30\tsecond win",
    ]
    .join("\n");

    assert_eq!(
        parse_tmux_mirror_pane_list(&output),
        vec![
            mirror_pane("%19", "@10", "bash", "bash", [0, 0, 50, 30]),
            mirror_pane("%20", "@10", "bash", "claude", [51, 0, 49, 30]),
            mirror_pane("%21", "@11", "second win", "bash", [0, 0, 100, 30]),
        ]
    );
}

#[test]
fn tmux_pane_list_keeps_tab_characters_in_window_name() {
    let output = "%1\t@1\tbash\t0\t0\t80\t24\tname\twith\ttabs";
    assert_eq!(
        parse_tmux_mirror_pane_list(output)[0].tab_name,
        "name\twith\ttabs"
    );
}

#[test]
fn tmux_pane_list_skips_lines_with_missing_fields() {
    let output = ["%1\t@1\tbash", "%2\t@1\tbash\t0\t0\t80\t24\tbash"].join("\n");
    let pane_ids: Vec<String> = parse_tmux_mirror_pane_list(&output)
        .into_iter()
        .map(|pane| pane.id)
        .collect();
    assert_eq!(pane_ids, vec!["%2"]);
}

#[test]
fn tmux_pane_list_drops_zero_sized_panes() {
    assert_eq!(
        parse_tmux_mirror_pane_list("%1\t@1\tbash\t0\t0\t0\t24\tbash"),
        vec![]
    );
}

#[test]
fn tmux_pane_list_of_empty_output_is_empty() {
    assert_eq!(parse_tmux_mirror_pane_list(""), vec![]);
}

#[test]
fn tmux_list_mirror_panes_reads_one_session_with_geometry() {
    assert_eq!(
        build_tmux_list_mirror_panes_command("kanvibe-task"),
        "tmux list-panes -s -t 'kanvibe-task' -F '#{pane_id}\t#{window_id}\t#{pane_current_command}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{window_name}'"
    );
}

#[test]
fn tmux_snapshot_keeps_ansi() {
    assert_eq!(
        build_tmux_capture_pane_command("%19"),
        "tmux capture-pane -pe -S -2000 -t '%19'"
    );
}

#[test]
fn tmux_stream_pipes_into_temp_file_and_follows_it() {
    let command = build_tmux_pane_stream_command("%19");
    assert!(command.contains("tmux pipe-pane -o -t '%19' \"cat >> '$sink'\""));
    assert!(command.contains("tail -n +1 -f \"$sink\""));
}

#[test]
fn tmux_stream_cleans_pipe_and_temp_file_on_exit() {
    assert!(build_tmux_pane_stream_command("%19").contains(
        "trap 'tmux pipe-pane -t '%19'; rm -f \"$sink\"; kill $tailpid 2>/dev/null' EXIT INT TERM"
    ));
}

#[test]
fn tmux_stream_waits_on_background_tail_so_trap_survives() {
    let command = build_tmux_pane_stream_command("%19");
    assert!(command.contains("tail -n +1 -f \"$sink\" & tailpid=$!"));
    assert!(command.contains("wait $tailpid"));
    assert!(!command.ends_with("tail -n +1 -f \"$sink\""));
}

#[test]
fn tmux_cancel_pipe_passes_no_command() {
    assert_eq!(
        build_tmux_cancel_pipe_pane_command("%19"),
        "tmux pipe-pane -t '%19'"
    );
}

#[test]
fn tmux_send_keys_sends_input_literally() {
    assert_eq!(
        build_tmux_send_keys_command("%19", "Enter"),
        "tmux send-keys -t '%19' -l -- 'Enter'"
    );
}

#[test]
fn tmux_send_keys_sends_nul_as_ctrl_space_in_order() {
    let cases = [
        ("\0", "tmux send-keys -t '%19' -- C-Space"),
        (
            "\0\0",
            "tmux send-keys -t '%19' -- C-Space && tmux send-keys -t '%19' -- C-Space",
        ),
        (
            "한\0글",
            "tmux send-keys -t '%19' -l -- '한' && tmux send-keys -t '%19' -- C-Space && tmux send-keys -t '%19' -l -- '글'",
        ),
        (
            "\0--version\0",
            "tmux send-keys -t '%19' -- C-Space && tmux send-keys -t '%19' -l -- '--version' && tmux send-keys -t '%19' -- C-Space",
        ),
    ];
    for (input, expected) in cases {
        let command = build_tmux_send_keys_command("%19", input);
        assert!(!command.contains('\0'));
        assert_eq!(command, expected);
    }
}

#[test]
fn tmux_send_keys_does_not_treat_dash_input_as_option() {
    assert!(build_tmux_send_keys_command("%19", "--version").contains("-l -- '--version'"));
}

#[test]
fn tmux_capture_quotes_shell_metacharacters_in_pane_id() {
    let command = build_tmux_capture_pane_command(INJECTION_PANE_ID);
    assert_eq!(
        command,
        "tmux capture-pane -pe -S -2000 -t '%1'\"'\"'; rm -rf /; '\"'\"''"
    );
    assert!(
        !Regex::new(r";\s*rm -rf /;?\s*$")
            .unwrap()
            .is_match(&command)
    );
}

#[test]
fn screen_redraw_clears_returns_each_line_and_places_cursor() {
    let cursor = TerminalCursorPosition {
        x: 2,
        y: 2,
        pane_height: 3,
    };
    assert_eq!(
        build_screen_redraw("$ ls\nREADME.md\n", Some(&cursor)),
        format!("{CLEAR}$ ls\r\nREADME.md\r\n\x1b[3G")
    );
}

#[test]
fn screen_redraw_moves_cursor_up_from_last_visible_line_after_scrollback() {
    let cursor = TerminalCursorPosition {
        x: 2,
        y: 0,
        pane_height: 3,
    };
    assert_eq!(
        build_screen_redraw("old-1\nold-2\n$ \n\n", Some(&cursor)),
        format!("{CLEAR}old-1\r\nold-2\r\n$ \r\n\r\n\x1b[2A\x1b[3G")
    );
}

#[test]
fn screen_redraw_keeps_only_last_2000_lines() {
    let screen: Vec<String> = (0..2100).map(|index| format!("line-{index}")).collect();
    let expected: Vec<String> = (0..2000)
        .map(|index| format!("line-{}", index + 100))
        .collect();
    assert_eq!(
        build_screen_redraw(&screen.join("\n"), None),
        format!("{CLEAR}{}", expected.join("\r\n"))
    );
}

#[test]
fn screen_redraw_does_not_double_existing_crlf() {
    assert_eq!(
        build_screen_redraw("a\r\nb", None),
        format!("{CLEAR}a\r\nb")
    );
}

#[test]
fn screen_redraw_without_cursor_does_not_move_it() {
    assert_eq!(build_screen_redraw("a\nb", None), format!("{CLEAR}a\r\nb"));
}

#[test]
fn tmux_pane_state_is_read_with_display_message() {
    assert_eq!(
        build_tmux_pane_state_command("%19"),
        "tmux display-message -p -t '%19' '#{cursor_x} #{cursor_y} #{pane_height} #{alternate_on} #{mouse_standard_flag} #{mouse_button_flag} #{mouse_all_flag} #{mouse_sgr_flag} #{mouse_utf8_flag}'"
    );
}

#[test]
fn tmux_pane_state_parses_or_reports_unknown_for_malformed_output() {
    assert_eq!(
        parse_tmux_pane_state("8 1 24 0 0 0 0 0 0"),
        Some(TmuxPaneState {
            cursor: TerminalCursorPosition {
                x: 8,
                y: 1,
                pane_height: 24
            },
            restore_sequences: "\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l"
                .to_string(),
        })
    );
    for output in ["8 1 0 0 0 0 0 0", "8 1", "PANE_A", ""] {
        assert_eq!(parse_tmux_pane_state(output), None, "{output}");
    }
}

fn zellij_list_entry(overrides: Value) -> Value {
    let base = json!({ "id": 0, "is_plugin": false, "tab_id": 0, "tab_name": "Tab #1",
        "title": "Pane #1", "terminal_command": null, "pane_x": 0, "pane_y": 0,
        "pane_rows": 24, "pane_columns": 40 });
    with_fields(base, overrides)
}

#[test]
fn zellij_pane_list_reads_terminal_panes_with_geometry() {
    let output = json!([
        zellij_list_entry(
            json!({ "id": 0, "terminal_command": "claude", "pane_x": 0, "pane_columns": 40 })
        ),
        zellij_list_entry(
            json!({ "id": 1, "terminal_command": "zsh", "pane_x": 40, "pane_columns": 40 })
        ),
    ]);
    assert_eq!(
        parse_zellij_mirror_pane_list(&output.to_string()),
        vec![
            mirror_pane("terminal_0", "0", "Tab #1", "claude", [0, 0, 40, 24]),
            mirror_pane("terminal_1", "0", "Tab #1", "zsh", [40, 0, 40, 24]),
        ]
    );
}

#[test]
fn zellij_pane_list_skips_plugin_panes() {
    let output = json!([
        zellij_list_entry(json!({ "id": 0, "is_plugin": true, "title": "About Zellij" })),
        zellij_list_entry(json!({ "id": 1, "is_plugin": false })),
    ]);
    let pane_ids: Vec<String> = parse_zellij_mirror_pane_list(&output.to_string())
        .into_iter()
        .map(|pane| pane.id)
        .collect();
    assert_eq!(pane_ids, vec!["terminal_1"]);
}

#[test]
fn zellij_pane_list_uses_title_when_command_is_empty() {
    let output = json!([zellij_list_entry(
        json!({ "terminal_command": null, "title": "Pane #1" })
    )]);
    assert_eq!(
        parse_zellij_mirror_pane_list(&output.to_string())[0].command,
        "Pane #1"
    );
}

#[test]
fn zellij_pane_list_of_non_json_is_empty() {
    assert_eq!(
        parse_zellij_mirror_pane_list("TAB_ID  TAB_POS  PANE_ID"),
        vec![]
    );
}

#[test]
fn zellij_list_mirror_panes_queries_json() {
    assert_eq!(
        build_zellij_list_mirror_panes_command("kanvibe-task"),
        "zellij --session 'kanvibe-task' action list-panes --json -a"
    );
}

#[test]
fn zellij_dump_targets_named_pane_without_focus_change() {
    assert_eq!(
        build_zellij_dump_pane_command("kanvibe-task", "terminal_1"),
        "kanvibe_mobile_dump=$(zellij --session 'kanvibe-task' action dump-screen -p 'terminal_1' -a --full) && printf '%s\\n' \"$kanvibe_mobile_dump\" | tail -n 2000"
    );
}

#[test]
fn zellij_viewport_dump_reads_visible_screen_without_scrollback() {
    assert_eq!(
        build_zellij_dump_viewport_command("kanvibe-task", "terminal_1"),
        "zellij --session 'kanvibe-task' action dump-screen -p 'terminal_1' -a"
    );
}

#[test]
fn zellij_write_spreads_input_as_decimal_utf8_bytes() {
    assert_eq!(
        build_zellij_write_pane_command("kanvibe-task", "terminal_1", "hi\n"),
        "zellij --session 'kanvibe-task' action write -p 'terminal_1' 104 105 10"
    );
}

#[test]
fn zellij_write_splits_korean_input_into_bytes() {
    assert_eq!(
        build_zellij_write_pane_command("s", "terminal_0", "한"),
        "zellij --session 's' action write -p 'terminal_0' 237 149 156"
    );
}

#[test]
fn tmux_shortcuts_target_named_pane_instead_of_prefix() {
    let cases = [
        (
            "split-right",
            "tmux split-window -h -d -t '%19' -c '#{pane_current_path}'",
        ),
        (
            "split-down",
            "tmux split-window -v -d -t '%19' -c '#{pane_current_path}'",
        ),
        ("toggle-fullscreen", "tmux resize-pane -Z -t '%19'"),
        ("copy-mode", "tmux copy-mode -t '%19'"),
        (
            "scroll-up",
            "tmux copy-mode -t '%19' && tmux send-keys -X -t '%19' page-up",
        ),
        (
            "scroll-down",
            "tmux copy-mode -t '%19' && tmux send-keys -X -t '%19' page-down",
        ),
        (
            "scroll-bottom",
            "tmux copy-mode -t '%19' && tmux send-keys -X -t '%19' cancel",
        ),
    ];
    for (action, expected) in cases {
        assert_eq!(
            build_tmux_pane_shortcut_command("%19", action).as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn zellij_split_creates_pane_in_selected_tab_without_focus() {
    for (action, direction) in [("split-right", "right"), ("split-down", "down")] {
        assert_eq!(
            build_zellij_pane_shortcut_command("session", "terminal_3", "4", action),
            Some(format!(
                "zellij --session 'session' action new-pane --direction {direction} --tab-id '4' --no-focus"
            ))
        );
    }
}

#[test]
fn zellij_shortcuts_use_pane_id_instead_of_client_focus() {
    let cases = [
        ("toggle-fullscreen", "toggle-fullscreen"),
        ("toggle-floating", "toggle-pane-embed-or-floating"),
        ("scroll-up", "page-scroll-up"),
        ("scroll-down", "page-scroll-down"),
        ("scroll-bottom", "scroll-to-bottom"),
    ];
    for (action, command) in cases {
        assert_eq!(
            build_zellij_pane_shortcut_command("session", "terminal_3", "4", action),
            Some(format!(
                "zellij --session 'session' action {command} --pane-id 'terminal_3'"
            ))
        );
    }
}

#[test]
fn shortcuts_reject_shell_commands_and_other_multiplexer_actions() {
    for action in ["; rm -rf /", "toString", "unknown"] {
        assert_eq!(build_tmux_pane_shortcut_command("%19", action), None);
        assert_eq!(
            build_zellij_pane_shortcut_command("session", "terminal_3", "4", action),
            None
        );
    }
    assert_eq!(
        build_tmux_pane_shortcut_command("%19", "toggle-floating"),
        None
    );
    assert_eq!(
        build_zellij_pane_shortcut_command("session", "terminal_3", "4", "copy-mode"),
        None
    );
}

#[test]
fn shortcuts_quote_shell_metacharacters_in_targets() {
    assert_eq!(
        build_tmux_pane_shortcut_command(INJECTION_PANE_ID, "toggle-fullscreen"),
        Some(format!("tmux resize-pane -Z -t {QUOTED_INJECTION_PANE_ID}"))
    );
    assert!(
        build_zellij_pane_shortcut_command("a'b", "terminal_3", "4", "scroll-up")
            .unwrap()
            .contains("--session 'a'\"'\"'b'")
    );
}

#[test]
fn tmux_close_kills_only_named_pane() {
    assert_eq!(
        build_tmux_close_pane_command("%19"),
        "tmux kill-pane -t '%19'"
    );
}

#[test]
fn zellij_close_targets_pane_id_instead_of_focus() {
    assert_eq!(
        build_zellij_close_pane_command("session", "terminal_3"),
        "zellij --session 'session' action close-pane --pane-id 'terminal_3'"
    );
}

#[test]
fn herdr_close_passes_pane_id_as_single_argument() {
    assert_eq!(
        build_herdr_close_pane_command("session", "w1:p1"),
        Ok(herdr(&["pane", "close", "w1:p1"]))
    );
}

#[test]
fn close_commands_quote_shell_metacharacters_in_targets() {
    assert_eq!(
        build_tmux_close_pane_command(INJECTION_PANE_ID),
        format!("tmux kill-pane -t {QUOTED_INJECTION_PANE_ID}")
    );
    assert!(build_zellij_close_pane_command("a'b", "terminal_3").contains("--session 'a'\"'\"'b'"));
}

#[test]
fn herdr_write_passes_control_and_dash_input_as_one_send_text_argument() {
    assert_eq!(
        build_herdr_write_pane_command("session", "w1:p1", "\x03"),
        Ok(herdr(&["pane", "send-text", "w1:p1", "\x03"]))
    );
    assert_eq!(
        build_herdr_write_pane_command("session", "w1:p1", "--help"),
        Ok(herdr(&["pane", "send-text", "w1:p1", "--help"]))
    );
}

#[test]
fn herdr_write_sends_nul_as_ctrl_space_in_order() {
    let expected = [
        herdr(&["pane", "send-text", "w1:p1", "a"]),
        herdr(&["pane", "send-keys", "w1:p1", "ctrl+space"]),
        herdr(&["pane", "send-text", "w1:p1", "b"]),
    ]
    .join(" && ");
    assert_eq!(
        build_herdr_write_pane_command("session", "w1:p1", "a\0b"),
        Ok(expected)
    );
}

#[test]
fn herdr_split_keeps_pc_focus_and_zoom_toggles() {
    assert_eq!(
        build_herdr_pane_shortcut_command("session", "w1:p1", "split-right"),
        Ok(Some(herdr(&[
            "pane",
            "split",
            "w1:p1",
            "--direction",
            "right",
            "--no-focus"
        ])))
    );
    assert_eq!(
        build_herdr_pane_shortcut_command("session", "w1:p1", "split-down"),
        Ok(Some(herdr(&[
            "pane",
            "split",
            "w1:p1",
            "--direction",
            "down",
            "--no-focus"
        ])))
    );
    assert_eq!(
        build_herdr_pane_shortcut_command("session", "w1:p1", "toggle-fullscreen"),
        Ok(Some(herdr(&["pane", "zoom", "w1:p1", "--toggle"])))
    );
}

#[test]
fn herdr_shortcuts_without_herdr_command_are_unsupported() {
    for action in [
        "scroll-up",
        "scroll-down",
        "scroll-bottom",
        "copy-mode",
        "toggle-floating",
    ] {
        assert_eq!(
            build_herdr_pane_shortcut_command("session", "w1:p1", action),
            Ok(None),
            "{action}"
        );
    }
}

#[test]
fn herdr_reads_up_to_2000_ansi_lines_and_observe_sets_grid() {
    assert_eq!(
        build_herdr_recent_screen_command("session", "w1:p1"),
        Ok(herdr(&[
            "pane", "read", "w1:p1", "--source", "recent", "--lines", "2000", "--format", "ansi"
        ]))
    );
    assert_eq!(
        build_herdr_observe_pane_command("session", "w1:p1", 60, 38),
        Ok(herdr(&[
            "terminal", "session", "observe", "w1:p1", "--cols", "60", "--rows", "38"
        ]))
    );
}

#[test]
fn herdr_observe_frame_reads_actual_grid_for_resize_ack() {
    let line =
        json!({ "type": "terminal.frame", "bytes": "", "full": true, "width": 44, "height": 70 });
    assert_eq!(
        parse_herdr_observe_record(&line.to_string()),
        Some(HerdrObserveRecord::Frame {
            is_full_screen: true,
            output: String::new(),
            size: Some(HerdrFrameSize {
                width: 44.0,
                height: 70.0
            }),
        })
    );
}

#[test]
fn herdr_control_stream_opens_with_explicit_size() {
    assert_eq!(
        build_herdr_control_pane_command("session", "w1:p1", 44, 70),
        Ok(herdr(&[
            "terminal", "session", "control", "w1:p1", "--cols", "44", "--rows", "70"
        ]))
    );
}

#[test]
fn herdr_observe_frame_decodes_base64_as_utf8_and_full_screen_flag() {
    let bytes = BASE64.encode("\x1b[1;1H한글");
    let full = json!({ "type": "terminal.frame", "bytes": bytes, "full": true });
    let partial = json!({ "type": "terminal.frame", "bytes": bytes, "full": false });

    assert_eq!(
        parse_herdr_observe_record(&full.to_string()),
        Some(HerdrObserveRecord::Frame {
            is_full_screen: true,
            output: "\x1b[1;1H한글".to_string(),
            size: None,
        })
    );
    assert!(matches!(
        parse_herdr_observe_record(&partial.to_string()),
        Some(HerdrObserveRecord::Frame {
            is_full_screen: false,
            ..
        })
    ));
}

#[test]
fn herdr_observe_distinguishes_closed_and_ignores_broken_or_unknown_lines() {
    assert_eq!(
        parse_herdr_observe_record(r#"{"type":"terminal.closed","reason":"gone"}"#),
        Some(HerdrObserveRecord::Closed)
    );
    assert_eq!(parse_herdr_observe_record("{broken"), None);
    assert_eq!(
        parse_herdr_observe_record(r#"{"type":"terminal.other"}"#),
        None
    );
}

#[test]
fn tmux_wheel_state_reads_copy_mode_alternate_screen_and_mouse_encoding() {
    assert_eq!(
        parse_tmux_wheel_state("1 0 1 1 0\n"),
        Some(TmuxWheelState {
            is_in_copy_mode: true,
            is_alternate_screen: false,
            is_mouse_reporting: true,
            encoding: MouseReportEncoding::Sgr,
        })
    );
    assert_eq!(
        parse_tmux_wheel_state("0 1 0 0 1"),
        Some(TmuxWheelState {
            is_in_copy_mode: false,
            is_alternate_screen: true,
            is_mouse_reporting: false,
            encoding: MouseReportEncoding::Utf8,
        })
    );
}

#[test]
fn tmux_wheel_state_is_unknown_for_unreadable_output() {
    for output in ["", "no server running", "0 1 0 0"] {
        assert_eq!(parse_tmux_wheel_state(output), None, "{output}");
    }
}

#[test]
fn tmux_copy_mode_capture_shifts_range_by_scroll_position() {
    let view = parse_tmux_copy_mode_view("1 10 24\n");
    assert_eq!(
        view,
        Some(TmuxCopyModeView {
            scroll_position: 10,
            pane_height: 24
        })
    );
    assert_eq!(
        build_tmux_capture_copy_mode_view_command("%3", &view.unwrap()),
        "tmux capture-pane -pe -S -10 -E 13 -t '%3'"
    );
}

#[test]
fn tmux_copy_mode_view_is_none_after_copy_mode_ends() {
    assert_eq!(parse_tmux_copy_mode_view("0  24"), None);
}

#[test]
fn mouse_wheel_report_sends_zero_based_cells_as_one_based() {
    let cases = [
        (
            MouseReportEncoding::Sgr,
            ScrollDirection::Up,
            "\x1b[<64;5;3M",
        ),
        (
            MouseReportEncoding::Sgr,
            ScrollDirection::Down,
            "\x1b[<65;5;3M",
        ),
        (MouseReportEncoding::Utf8, ScrollDirection::Up, "\x1b[M`%#"),
        (
            MouseReportEncoding::Normal,
            ScrollDirection::Down,
            "\x1b[Ma%#",
        ),
    ];
    for (encoding, direction, report) in cases {
        assert_eq!(
            build_mouse_wheel_report(direction, 4, 2, encoding).as_deref(),
            Some(report)
        );
    }
}

#[test]
fn mouse_wheel_report_skips_far_cells_in_normal_encoding_but_not_utf8() {
    assert_eq!(
        build_mouse_wheel_report(ScrollDirection::Up, 230, 0, MouseReportEncoding::Normal),
        None
    );
    assert_eq!(
        build_mouse_wheel_report(ScrollDirection::Up, 230, 0, MouseReportEncoding::Utf8),
        Some(format!("\x1b[M`{}!", char::from_u32(263).unwrap()))
    );
}

#[test]
fn wheel_delegated_redraw_uses_alternate_screen_without_mouse_reporting() {
    let reset_mouse = "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l";
    assert_eq!(
        build_wheel_delegated_screen_redraw("a\nb"),
        format!("{CLEAR}\x1b[?1049h{reset_mouse}{CLEAR}a\r\nb")
    );
}

#[test]
fn herdr_scroll_control_record_wraps_one_wheel_step() {
    let request = parse_terminal_scroll_request(
        &json!({ "type": "scroll", "direction": "up", "column": 4, "row": 2 }),
    );
    let expected_request = TerminalScrollRequest {
        direction: ScrollDirection::Up,
        column: 4,
        row: 2,
    };
    assert_eq!(request, Some(expected_request));
    assert_eq!(
        build_herdr_scroll_control_record(&expected_request),
        json!({ "type": "terminal.scroll", "direction": "up", "lines": 1, "column": 4, "row": 2 })
    );
}

#[test]
fn scroll_request_with_invalid_direction_or_cell_is_dropped() {
    let records = [
        json!({ "type": "resize", "direction": "up", "column": 0, "row": 0 }),
        json!({ "type": "scroll", "direction": "left", "column": 0, "row": 0 }),
        json!({ "type": "scroll", "direction": "down", "column": -1, "row": 0 }),
        json!({ "type": "scroll", "direction": "down", "column": 0, "row": 500 }),
        json!({ "type": "scroll", "direction": "down", "column": 1.5, "row": 0 }),
        json!({ "type": "scroll", "direction": "down", "column": "1", "row": 0 }),
    ];
    for record in records {
        assert_eq!(parse_terminal_scroll_request(&record), None, "{record}");
    }
}

/// 원격 observe 감싸기를 실제 `sh`로 돌린다. 데스크탑의 ssh 자식처럼 stdin과 stdout을 파이프로 잇는다
fn spawn_until_stdin_closes(fake_observe: &str) -> std::process::Child {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(end_when_stdin_closes(fake_observe))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

fn wait_for_exit(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> Option<std::process::ExitStatus> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = child.kill();
    None
}

fn is_process_alive(process_id: libc::pid_t) -> bool {
    // SAFETY: 신호 0은 프로세스가 있는지만 묻는다
    unsafe { libc::kill(process_id, 0) == 0 }
}

#[test]
fn remote_observe_wraps_the_plain_observe_command() {
    let observe = build_herdr_observe_pane_command("session", "w1:p1", 60, 38).unwrap();

    let remote = build_herdr_remote_observe_pane_command("session", "w1:p1", 60, 38).unwrap();

    assert_eq!(remote, end_when_stdin_closes(&observe));
    assert!(
        remote.contains(&format!("{observe} </dev/null &")),
        "{remote}"
    );
}

#[test]
fn an_observer_that_ignores_stdin_ends_soon_after_stdin_closes() {
    // stdin을 읽지 않고 오래 도는 가짜 observe. 시작하면 자기 pid를 한 줄로 알린다
    let mut wrapper = spawn_until_stdin_closes("sh -c 'echo $$; exec sleep 30'");
    let mut stdout = std::io::BufReader::new(wrapper.stdout.take().unwrap());
    let mut first_line = String::new();
    std::io::BufRead::read_line(&mut stdout, &mut first_line).unwrap();
    let observer_id: libc::pid_t = first_line.trim().parse().unwrap();
    assert!(is_process_alive(observer_id));

    drop(wrapper.stdin.take());

    let status = wait_for_exit(&mut wrapper, std::time::Duration::from_secs(5))
        .expect("stdin이 닫히면 곧 끝난다");
    assert!(!status.success(), "{status:?}");
    assert!(
        !is_process_alive(observer_id),
        "observe {observer_id}가 남았다"
    );
}

#[test]
fn an_observer_that_ends_by_itself_keeps_its_output_bytes_and_exit_code() {
    let mut wrapper = spawn_until_stdin_closes(r#"sh -c "printf 'frame\000\377\n'; exit 7""#);

    let status = wait_for_exit(&mut wrapper, std::time::Duration::from_secs(5))
        .expect("stdin이 열려 있어도 observe가 끝나면 끝난다");
    let mut output = Vec::new();
    std::io::Read::read_to_end(&mut wrapper.stdout.take().unwrap(), &mut output).unwrap();
    drop(wrapper.stdin.take());

    assert_eq!(output, b"frame\0\xff\n");
    assert_eq!(status.code(), Some(7));
}
