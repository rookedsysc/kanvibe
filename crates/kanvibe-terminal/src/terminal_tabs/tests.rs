use std::process::Command;

use kanvibe_process::exec::ExecGitOptions;

use super::*;
use crate::test_support::ScriptedShellRunner;

const INJECTION_SESSION_NAME: &str = "a'; rm -rf /; '";

fn tab(id: &str, native_index: i64, name: &str, is_active: bool) -> TerminalTab {
    TerminalTab {
        id: id.to_string(),
        native_index,
        name: name.to_string(),
        is_active,
    }
}

fn layout_with_focus_on_second_tab() -> String {
    [
        "layout {",
        "    tab name=\"first\" {",
        "        pane",
        "    }",
        "    tab name=\"second\" focus=true {",
        "        pane",
        "    }",
        "}",
    ]
    .join("\n")
}

#[test]
fn tmux_pane_list_reads_session_window_command_and_path() {
    let output = [
        "kanvibe-task\t@155\t0\tclaude\t/repo/task\tclaude",
        "kanvibe-dev\t@1\t2\tzsh\t/repo\tzsh",
    ]
    .join("\n");

    assert_eq!(
        parse_tmux_pane_list(&output),
        vec![
            TmuxPaneSnapshot {
                session_name: "kanvibe-task".into(),
                window_id: "@155".into(),
                window_index: 0,
                window_name: "claude".into(),
                command: "claude".into(),
                current_path: "/repo/task".into(),
            },
            TmuxPaneSnapshot {
                session_name: "kanvibe-dev".into(),
                window_id: "@1".into(),
                window_index: 2,
                window_name: "zsh".into(),
                command: "zsh".into(),
                current_path: "/repo".into(),
            },
        ]
    );
}

#[test]
fn tmux_pane_list_keeps_tab_characters_in_window_name() {
    assert_eq!(
        parse_tmux_pane_list("s\t@1\t0\tzsh\t/repo\ta\tb")[0].window_name,
        "a\tb"
    );
}

#[test]
fn tmux_pane_list_skips_empty_output_and_short_lines() {
    assert!(parse_tmux_pane_list("").is_empty());
    assert!(parse_tmux_pane_list("s\t@1\t0\tzsh").is_empty());
}

#[test]
fn tmux_pane_list_ignores_crlf_line_endings() {
    assert_eq!(
        parse_tmux_pane_list("s\t@1\t0\tclaude\t/repo\tclaude\r\n")[0].window_name,
        "claude"
    );
}

#[test]
fn tmux_window_list_reads_id_index_name_and_active_state() {
    let output = ["@1\t0\t0\tzsh", "@2\t1\t1\tvim"].join("\n");

    assert_eq!(
        parse_tmux_window_list(&output),
        vec![tab("@1", 0, "zsh", false), tab("@2", 1, "vim", true)]
    );
}

#[test]
fn tmux_window_list_preserves_non_delimiter_special_characters() {
    assert_eq!(
        parse_tmux_window_list("@1\t0\t0\ta|b:c"),
        vec![tab("@1", 0, "a|b:c", false)]
    );
}

#[test]
fn tmux_window_list_keeps_tab_characters_in_name() {
    assert_eq!(
        parse_tmux_window_list("@1\t0\t0\ta\tb"),
        vec![tab("@1", 0, "a\tb", false)]
    );
}

#[test]
fn tmux_window_list_skips_empty_output_and_short_lines() {
    assert!(parse_tmux_window_list("").is_empty());
    assert!(parse_tmux_window_list("@1\t0\t0").is_empty());
    assert!(parse_tmux_window_list("\n\n").is_empty());
}

#[test]
fn tmux_window_list_ignores_crlf_line_endings() {
    assert_eq!(
        parse_tmux_window_list("@1\t0\t1\tzsh\r\n"),
        vec![tab("@1", 0, "zsh", true)]
    );
}

#[test]
fn tmux_list_windows_quotes_session_name_as_single_argument() {
    let command = build_tmux_list_windows_command(INJECTION_SESSION_NAME);
    let session_argument = Regex::new(r"^tmux list-windows -t (.+) -F ")
        .unwrap()
        .captures(&command)
        .map(|captures| captures[1].to_string())
        .unwrap_or_default();

    // 실제 셸에 통과시켜, 인용이 세션 이름을 인자 하나로 되돌리는지 확인한다
    let echoed = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("printf '%s' {session_argument}"))
        .env_clear()
        .output()
        .unwrap();

    assert!(echoed.status.success());
    assert_eq!(
        String::from_utf8(echoed.stdout).unwrap(),
        INJECTION_SESSION_NAME
    );
}

#[test]
fn tmux_window_commands_target_window_id_without_session_prefix() {
    assert_eq!(
        build_tmux_select_window_command("@3"),
        "tmux select-window -t '@3'"
    );
    assert_eq!(
        build_tmux_kill_window_command("@3"),
        "tmux kill-window -t '@3'"
    );
}

#[test]
fn tmux_new_window_appends_colon_target_and_working_directory() {
    assert_eq!(
        build_tmux_new_window_command("proj-main", Some("/work/tree")),
        "tmux new-window -t 'proj-main': -P -F '#{window_id}' -c '/work/tree'"
    );
}

#[test]
fn tmux_new_window_omits_working_directory_when_unknown() {
    assert_eq!(
        build_tmux_new_window_command("proj-main", None),
        "tmux new-window -t 'proj-main': -P -F '#{window_id}'"
    );
}

#[test]
fn tmux_rename_window_places_name_after_double_dash() {
    assert_eq!(
        build_tmux_rename_window_command("@3", "-n"),
        "tmux rename-window -t '@3' -- '-n'"
    );
}

#[test]
fn tmux_move_left_inserts_before_target_and_renumbers() {
    assert_eq!(
        build_tmux_move_window_commands("proj-main", "@3", 2, 0),
        vec![
            "tmux move-window -b -s '@3' -t 'proj-main':0",
            "tmux move-window -r -t 'proj-main'",
        ]
    );
}

#[test]
fn tmux_move_right_inserts_after_target() {
    assert_eq!(
        build_tmux_move_window_commands("proj-main", "@3", 0, 2),
        vec![
            "tmux move-window -a -s '@3' -t 'proj-main':2",
            "tmux move-window -r -t 'proj-main'",
        ]
    );
}

#[test]
fn tmux_move_to_same_index_emits_no_commands() {
    assert!(build_tmux_move_window_commands("proj-main", "@3", 1, 1).is_empty());
}

#[test]
fn tmux_move_never_uses_kill_flag() {
    let kill_flag = Regex::new(r" -k( |$)").unwrap();
    let all_commands = [
        build_tmux_move_window_commands("proj-main", "@3", 2, 0),
        build_tmux_move_window_commands("proj-main", "@3", 0, 2),
    ]
    .concat();

    assert!(
        !all_commands
            .iter()
            .any(|command| kill_flag.is_match(command))
    );
}

#[test]
fn zellij_tab_id_commands_start_at_version_0_44_0() {
    assert!(!supports_zellij_tab_id_commands("zellij 0.43.1"));
    assert!(supports_zellij_tab_id_commands("zellij 0.44.0"));
    assert!(supports_zellij_tab_id_commands("zellij 0.44.0 (main)"));
    assert!(supports_zellij_tab_id_commands("zellij 1.0.0"));
}

#[test]
fn unreadable_zellij_version_counts_as_old() {
    assert!(!supports_zellij_tab_id_commands(""));
    assert!(!supports_zellij_tab_id_commands("command not found"));
}

#[test]
fn tab_and_pane_axes_share_one_version_check_per_host() {
    let cache = ZellijIdTargetingSupportCache::new();
    let tab_service_runner = ScriptedShellRunner::new();
    tab_service_runner.respond_when_contains("zellij --version", "zellij 0.44.3");
    let mirror_service_runner = ScriptedShellRunner::new();
    mirror_service_runner.respond_when_contains("zellij --version", "zellij 0.43.1");
    let options = ExecGitOptions::default();

    assert!(cache.resolve_zellij_id_targeting_support(
        Some("shared-host"),
        tab_service_runner.as_ref(),
        &options
    ));
    assert!(cache.resolve_zellij_id_targeting_support(
        Some("shared-host"),
        mirror_service_runner.as_ref(),
        &options
    ));

    assert_eq!(tab_service_runner.commands(), vec!["zellij --version"]);
    assert!(mirror_service_runner.commands().is_empty());
}

#[test]
fn host_with_failing_version_command_counts_as_old() {
    let cache = ZellijIdTargetingSupportCache::new();
    let runner = ScriptedShellRunner::new();
    runner.fail_when_contains("zellij --version", "zellij: command not found");

    assert!(!cache.resolve_zellij_id_targeting_support(
        Some("missing-zellij-host"),
        runner.as_ref(),
        &ExecGitOptions::default()
    ));
}

#[test]
fn zellij_tab_list_reads_tab_id_position_and_active() {
    let output = serde_json::json!([
        { "position": 0, "tab_id": 0, "name": "shell", "active": false },
        { "position": 1, "tab_id": 1, "name": "logs", "active": true },
    ])
    .to_string();

    assert_eq!(
        parse_zellij_tab_list(&output),
        vec![tab("0", 0, "shell", false), tab("1", 1, "logs", true)]
    );
}

#[test]
fn zellij_tab_list_uses_tab_id_after_positions_shift() {
    let output_after_middle_tab_closed = serde_json::json!([
        { "position": 0, "tab_id": 0, "name": "Tab #1", "active": false },
        { "position": 1, "tab_id": 2, "name": "logs", "active": true },
    ])
    .to_string();

    assert_eq!(
        parse_zellij_tab_list(&output_after_middle_tab_closed),
        vec![tab("0", 0, "Tab #1", false), tab("2", 1, "logs", true)]
    );
}

#[test]
fn zellij_tab_list_is_empty_for_non_json_or_non_array() {
    assert!(parse_zellij_tab_list("not json").is_empty());
    assert!(parse_zellij_tab_list(r#"{"position":0}"#).is_empty());
}

#[test]
fn zellij_tab_list_skips_entries_without_tab_id_or_name() {
    let output = serde_json::json!([
        { "position": 0, "tab_id": 0 },
        { "position": 1, "name": "no-id" },
        { "position": 2, "tab_id": 2, "name": "logs" },
    ])
    .to_string();

    assert_eq!(
        parse_zellij_tab_list(&output),
        vec![tab("2", 2, "logs", false)]
    );
}

#[test]
fn legacy_zellij_marks_focused_layout_tab_active() {
    assert_eq!(
        parse_zellij_tab_names_with_focus("first\nsecond", &layout_with_focus_on_second_tab()),
        vec![tab("0", 0, "first", false), tab("1", 1, "second", true)]
    );
}

#[test]
fn legacy_zellij_assumes_first_tab_active_without_layout() {
    assert_eq!(
        parse_zellij_tab_names_with_focus("first\nsecond", ""),
        vec![tab("0", 0, "first", true), tab("1", 1, "second", false)]
    );
}

#[test]
fn legacy_zellij_returns_empty_list_without_names() {
    assert!(parse_zellij_tab_names_with_focus("", &layout_with_focus_on_second_tab()).is_empty());
}

#[test]
fn focused_tab_name_unescapes_quotes() {
    let layout = r#"tab name="say \"hi\"" focus=true {"#;

    assert_eq!(
        parse_zellij_focused_tab_name(layout).as_deref(),
        Some(r#"say "hi""#)
    );
}

#[test]
fn focused_tab_name_is_none_without_focus_marker() {
    assert_eq!(parse_zellij_focused_tab_name(r#"tab name="only" {"#), None);
}

#[test]
fn focused_tab_name_ignores_swap_layout_tabs_and_pane_focus() {
    let real_dump_layout = [
        "layout {",
        "    tab name=\"Tab #1\" {",
        "        pane",
        "    }",
        "    tab name=\"logs\" focus=true hide_floating_panes=true {",
        "        pane focus=true",
        "    }",
        "    swap_tiled_layout name=\"vertical\" {",
        "        tab max_panes=5 {",
        "            pane split_direction=\"vertical\"",
        "        }",
        "    }",
        "}",
    ]
    .join("\n");

    assert_eq!(
        parse_zellij_focused_tab_name(&real_dump_layout).as_deref(),
        Some("logs")
    );
}

#[test]
fn zellij_commands_target_session_with_session_flag() {
    assert_eq!(
        build_zellij_list_tabs_command("proj"),
        "zellij --session 'proj' action list-tabs --json"
    );
    assert_eq!(
        build_zellij_go_to_tab_by_id_command("proj", "2"),
        "zellij --session 'proj' action go-to-tab-by-id '2'"
    );
    assert_eq!(
        build_zellij_close_tab_by_id_command("proj", "2"),
        "zellij --session 'proj' action close-tab-by-id '2'"
    );
    assert_eq!(
        build_zellij_rename_tab_by_id_command("proj", "2", "logs"),
        "zellij --session 'proj' action rename-tab-by-id '2' 'logs'"
    );
}

#[test]
fn zellij_new_tab_passes_working_directory_only_when_known() {
    assert_eq!(
        build_zellij_new_tab_command("proj", Some("/work/tree")),
        "zellij --session 'proj' action new-tab --cwd '/work/tree'"
    );
    assert_eq!(
        build_zellij_new_tab_command("proj", None),
        "zellij --session 'proj' action new-tab"
    );
}

#[test]
fn legacy_zellij_focuses_target_tab_before_focused_tab_commands() {
    assert_eq!(
        build_zellij_close_focused_tab_commands("proj", "logs"),
        vec![
            "zellij --session 'proj' action go-to-tab-name 'logs'",
            "zellij --session 'proj' action close-tab",
        ]
    );
    assert_eq!(
        build_zellij_rename_focused_tab_commands("proj", "logs", "build"),
        vec![
            "zellij --session 'proj' action go-to-tab-name 'logs'",
            "zellij --session 'proj' action rename-tab 'build'",
        ]
    );
}

#[test]
fn legacy_zellij_move_repeats_direction_per_step() {
    assert_eq!(
        build_zellij_move_tab_commands("proj", "logs", 0, 2),
        vec![
            "zellij --session 'proj' action go-to-tab-name 'logs'",
            "zellij --session 'proj' action move-tab right",
            "zellij --session 'proj' action move-tab right",
        ]
    );
    assert_eq!(
        build_zellij_move_tab_commands("proj", "logs", 2, 1),
        vec![
            "zellij --session 'proj' action go-to-tab-name 'logs'",
            "zellij --session 'proj' action move-tab left",
        ]
    );
}

#[test]
fn zellij_move_to_same_position_emits_no_commands() {
    assert!(build_zellij_move_tab_commands("proj", "logs", 1, 1).is_empty());
    assert!(build_zellij_move_tab_by_id_commands("proj", "2", 1, 1).is_empty());
}

#[test]
fn zellij_move_by_id_moves_target_without_changing_focus() {
    let commands = build_zellij_move_tab_by_id_commands("proj", "2", 0, 2);

    assert_eq!(
        commands,
        vec![
            "zellij --session 'proj' action move-tab right -t '2'",
            "zellij --session 'proj' action move-tab right -t '2'",
        ]
    );
    assert!(!commands.iter().any(|command| command.contains("go-to-tab")));
}
