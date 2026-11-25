//! zellij KDL 레이아웃과 세션 조회·정리 명령 문자열.

use std::collections::HashMap;
use std::sync::LazyLock;

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use regex::Regex;

use super::naming::quote_for_posix_shell;

/// Zellij KDL 레이아웃 파일의 기본 파일명
pub const ZELLIJ_LAYOUT_FILENAME: &str = ".zellij-layout.kdl";

/// ANSI 색 지정(SGR) 시퀀스. zellij list-sessions는 파이프로 받아도 세션 이름에 색을 입혀 내보낸다
static ANSI_SGR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("\x1b\\[[0-9;]*m").expect("유효한 정규식"));

/// 색은 awk 안에서 지운다. 원격 호스트의 sed가 `\x1b`를 알아듣는다고 기대할 수 없지만 awk의 `\033`은 POSIX다
const AWK_STRIP_COLOR: &str = r#"{ gsub(/\033\[[0-9;]*m/, "") }"#;

/// KDL 문자열 안의 특수문자를 이스케이프한다
fn escape_kdl(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

struct KdlPaneRenderer<'a> {
    commands_by_position: HashMap<i64, &'a str>,
    escaped_directory: String,
}

impl KdlPaneRenderer<'_> {
    /// position에 해당하는 pane의 KDL 노드
    fn render(&self, position: i64, indent: &str) -> String {
        let command = self
            .commands_by_position
            .get(&position)
            .map(|command| command.trim())
            .filter(|command| !command.is_empty());
        let directory = &self.escaped_directory;
        match command {
            None => format!("{indent}pane cwd=\"{directory}\""),
            Some(command) => [
                format!("{indent}pane command=\"bash\" {{"),
                format!("{indent}    args \"-c\" \"{}\"", escape_kdl(command)),
                format!("{indent}    cwd \"{directory}\""),
                format!("{indent}}}"),
            ]
            .join("\n"),
        }
    }
}

const VERTICAL_CONTAINER: &str = "    pane split_direction=\"vertical\" {";
const INNER_INDENT: &str = "            ";

/// 레이아웃 종류별 pane 배치. 바깥 `layout { }`은 호출부가 감싼다
fn render_layout_body(layout_type: PaneLayoutType, renderer: &KdlPaneRenderer<'_>) -> Vec<String> {
    let pane = |position: i64, indent: &str| renderer.render(position, indent);
    match layout_type {
        PaneLayoutType::Single => vec![pane(0, "    ")],
        PaneLayoutType::Horizontal2 => vec![pane(0, "    "), pane(1, "    ")],
        PaneLayoutType::Vertical2 => vec![
            VERTICAL_CONTAINER.into(),
            pane(0, "        "),
            pane(1, "        "),
            "    }".into(),
        ],
        PaneLayoutType::LeftRightTb => vec![
            VERTICAL_CONTAINER.into(),
            pane(0, "        "),
            "        pane {".into(),
            pane(1, INNER_INDENT),
            pane(2, INNER_INDENT),
            "        }".into(),
            "    }".into(),
        ],
        PaneLayoutType::LeftTbRight => vec![
            VERTICAL_CONTAINER.into(),
            "        pane {".into(),
            pane(0, INNER_INDENT),
            pane(1, INNER_INDENT),
            "        }".into(),
            pane(2, "        "),
            "    }".into(),
        ],
        PaneLayoutType::Quad => vec![
            VERTICAL_CONTAINER.into(),
            "        pane {".into(),
            pane(0, INNER_INDENT),
            pane(2, INNER_INDENT),
            "        }".into(),
            "        pane {".into(),
            pane(1, INNER_INDENT),
            pane(3, INNER_INDENT),
            "        }".into(),
            "    }".into(),
        ],
    }
}

/// 레이아웃 종류와 pane 명령으로 Zellij KDL 레이아웃을 만든다.
/// 세션 생성 시 --layout으로 넘겨 pane 분할과 명령 실행을 원자적으로 처리한다.
pub fn generate_zellij_layout_kdl(
    layout_type: PaneLayoutType,
    panes: &[PaneCommand],
    worktree_path: &str,
) -> String {
    // 같은 position이 여러 번 오면 JS Map처럼 마지막 값이 남는다
    let renderer = KdlPaneRenderer {
        commands_by_position: panes
            .iter()
            .map(|pane| (pane.position, pane.command.as_str()))
            .collect(),
        escaped_directory: escape_kdl(worktree_path),
    };
    let lines = render_layout_body(layout_type, &renderer);
    std::iter::once("layout {".to_string())
        .chain(lines)
        .chain(std::iter::once("}".to_string()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 살아 있는 zellij 세션에 대상 이름이 있으면 성공(0)을 반환하는 조건 명령.
/// `parse_alive_zellij_session_names`와 같은 규칙으로 색을 지우고 종료된 세션을 제외한다.
pub fn build_zellij_alive_session_check_command(session_name: &str) -> String {
    format!(
        "zellij list-sessions 2>/dev/null | awk '{AWK_STRIP_COLOR} $1 != \"EXITED:\" {{ print $1 }}' | grep -qFx -- {}",
        quote_for_posix_shell(session_name)
    )
}

/// zellij list-sessions는 종료된 세션을 `EXITED: <name>`으로 함께 출력하므로 이름만 뽑아 살아 있는 세션과 구분한다
pub fn parse_alive_zellij_session_names(list_sessions_output: &str) -> Vec<String> {
    let uncolored = ANSI_SGR_PATTERN.replace_all(list_sessions_output, "");
    uncolored
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("EXITED:"))
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

pub(crate) fn build_zellij_session_cleanup_command(
    session_name: &str,
    should_verify_cleanup: bool,
) -> String {
    let target = quote_for_posix_shell(session_name);
    // kill-session은 비동기로 끝나 바로 뒤의 delete-session이 실행 중이라며 거절하므로 --force로 남은 세션까지 지운다
    let commands = [
        format!("zellij kill-session {target} 2>/dev/null || true"),
        format!("zellij delete-session --force {target} 2>/dev/null || true"),
    ];
    if !should_verify_cleanup {
        return commands.join("; ");
    }
    let verify_command = format!(
        "if zellij list-sessions 2>/dev/null | awk '{AWK_STRIP_COLOR} {{ if ($1 == \"EXITED:\") print $2; else print $1 }}' | grep -Fx -- {target} >/dev/null; then exit 1; fi"
    );
    [
        "command -v zellij >/dev/null 2>&1 || exit 1".to_string(),
        commands[0].clone(),
        commands[1].clone(),
        verify_command,
    ]
    .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const WORKTREE_PATH: &str = "/home/user/kanvibe__worktrees/feat-branch";

    fn panes(commands: &[(i64, &str)]) -> Vec<PaneCommand> {
        commands
            .iter()
            .map(|(position, command)| PaneCommand {
                position: *position,
                command: command.to_string(),
            })
            .collect()
    }

    /// 원격 호스트가 받는 것과 같은 셸에서, zellij 0.45.1이 파이프로 내보낸 실제 출력을 흉내 내는 함수로 판정한다
    fn run_alive_check(list_sessions_output: &str, session_name: &str) -> Option<i32> {
        let fake_zellij = format!(
            "zellij() {{ printf '%b' {}; }}",
            serde_json::to_string(list_sessions_output).unwrap()
        );
        let script = format!(
            "{fake_zellij}; {}",
            build_zellij_alive_session_check_command(session_name)
        );
        Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .status()
            .unwrap()
            .code()
    }

    // parity: TS-lib-worktree-7b1e2c
    #[test]
    fn alive_check_excludes_exited_sessions_like_the_parser() {
        let command = build_zellij_alive_session_check_command("feat-login");

        assert!(command.contains(r#"$1 != "EXITED:" { print $1 }"#));
        assert!(command.contains("grep -qFx -- 'feat-login'"));
        assert!(command.contains("zellij list-sessions 2>/dev/null"));
        assert!(!command.contains("attach"));
    }

    // parity: TS-lib-worktree-4759b9
    #[test]
    fn alive_check_in_shell_accepts_colored_session_name() {
        let output = r"\033[32;1mfeat-login\033[m [Created \033[35;1m2s\033[m ago] \n";

        assert_eq!(run_alive_check(output, "feat-login"), Some(0));
    }

    // parity: TS-lib-worktree-6aed77
    #[test]
    fn alive_check_in_shell_rejects_other_sessions() {
        let output = r"\033[32;1mfeat-login-extra\033[m [Created 2s ago] \n";

        assert_eq!(run_alive_check(output, "feat-login"), Some(1));
    }

    // parity: TS-lib-worktree-91fb3e
    #[test]
    fn single_pane_layout_with_command() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::Single,
            &panes(&[(0, "echo hello")]),
            WORKTREE_PATH,
        );

        assert!(kdl.contains("layout {"));
        assert!(kdl.contains(r#"command="bash""#));
        assert!(kdl.contains(r#""echo hello""#));
        assert!(kdl.contains(&format!("cwd \"{WORKTREE_PATH}\"")));
        assert!(!kdl.contains("split_direction"));
    }

    // parity: TS-lib-worktree-539426
    #[test]
    fn single_pane_layout_without_command() {
        let kdl =
            generate_zellij_layout_kdl(PaneLayoutType::Single, &panes(&[(0, "")]), WORKTREE_PATH);

        assert!(kdl.contains(&format!("pane cwd=\"{WORKTREE_PATH}\"")));
        assert!(!kdl.contains(r#"command="bash""#));
    }

    // parity: TS-lib-worktree-425f32
    #[test]
    fn horizontal_two_pane_layout_uses_default_direction() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::Horizontal2,
            &panes(&[(0, "pnpm dev"), (1, "pnpm test")]),
            WORKTREE_PATH,
        );

        assert!(kdl.contains(r#""pnpm dev""#));
        assert!(kdl.contains(r#""pnpm test""#));
        assert!(!kdl.contains("split_direction"));
    }

    // parity: TS-lib-worktree-66a9bb
    #[test]
    fn vertical_two_pane_layout() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::Vertical2,
            &panes(&[(0, "pnpm dev"), (1, "pnpm test")]),
            WORKTREE_PATH,
        );

        assert!(kdl.contains(r#"split_direction="vertical""#));
        assert!(kdl.contains(r#""pnpm dev""#));
        assert!(kdl.contains(r#""pnpm test""#));
    }

    // parity: TS-lib-worktree-7931fe
    #[test]
    fn left_right_tb_layout() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::LeftRightTb,
            &panes(&[
                (0, "claude --dangerously-skip-permissions"),
                (1, "pnpm dev"),
                (2, "pnpm test:watch"),
            ]),
            WORKTREE_PATH,
        );

        assert!(kdl.contains(r#"split_direction="vertical""#));
        assert!(kdl.contains(r#""claude --dangerously-skip-permissions""#));
        assert!(kdl.contains(r#""pnpm dev""#));
        assert!(kdl.contains(r#""pnpm test:watch""#));
        assert!(
            kdl.lines()
                .any(|line| line.contains(r#"split_direction="vertical""#))
        );
    }

    // parity: TS-lib-worktree-716e48
    #[test]
    fn left_tb_right_layout() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::LeftTbRight,
            &panes(&[(0, "pnpm dev"), (1, "pnpm test"), (2, "claude")]),
            WORKTREE_PATH,
        );

        assert!(kdl.contains(r#"split_direction="vertical""#));
        for command in [r#""pnpm dev""#, r#""pnpm test""#, r#""claude""#] {
            assert!(kdl.contains(command));
        }
    }

    // parity: TS-lib-worktree-d2cffb
    #[test]
    fn quad_layout() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::Quad,
            &panes(&[(0, "cmd0"), (1, "cmd1"), (2, "cmd2"), (3, "cmd3")]),
            WORKTREE_PATH,
        );

        assert!(kdl.contains(r#"split_direction="vertical""#));
        for command in [r#""cmd0""#, r#""cmd1""#, r#""cmd2""#, r#""cmd3""#] {
            assert!(kdl.contains(command));
        }
    }

    // parity: TS-lib-worktree-a95cfd
    #[test]
    fn empty_command_panes_become_plain_shell_panes() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::Vertical2,
            &panes(&[(0, "pnpm dev"), (1, "")]),
            WORKTREE_PATH,
        );

        assert!(kdl.contains(r#""pnpm dev""#));
        let plain_panes = kdl
            .lines()
            .filter(|line| line.trim().starts_with("pane"))
            .filter(|line| {
                line.contains("cwd=")
                    && !line.contains("command=")
                    && !line.contains("split_direction")
            })
            .count();
        assert!(plain_panes >= 1);
    }

    // parity: TS-lib-worktree-fb68a7
    #[test]
    fn escapes_backslashes_and_quotes() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::Single,
            &panes(&[(0, r#"echo "hello world""#)]),
            r"/home/user/my\project",
        );

        assert!(kdl.contains(r"my\\project"));
        assert!(kdl.contains(r#"\"hello world\""#));
    }

    // parity: TS-lib-worktree-687fc2
    #[test]
    fn every_pane_gets_the_working_directory() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::LeftRightTb,
            &panes(&[(0, "cmd0"), (1, ""), (2, "cmd2")]),
            WORKTREE_PATH,
        );

        assert_eq!(kdl.matches(WORKTREE_PATH).count(), 3);
    }

    #[test]
    fn parses_only_alive_session_names() {
        let output = "\x1b[32;1mfeat\x1b[m [Created 2s ago]\nEXITED: old\n\nother  x\n";

        assert_eq!(parse_alive_zellij_session_names(output), ["feat", "other"]);
    }

    #[test]
    fn layout_text_matches_the_ts_generator_byte_for_byte() {
        let kdl = generate_zellij_layout_kdl(
            PaneLayoutType::LeftRightTb,
            &panes(&[(0, "a"), (1, " "), (2, "b")]),
            "/w",
        );

        let expected = [
            "layout {",
            "    pane split_direction=\"vertical\" {",
            "        pane command=\"bash\" {",
            "            args \"-c\" \"a\"",
            "            cwd \"/w\"",
            "        }",
            "        pane {",
            "            pane cwd=\"/w\"",
            "            pane command=\"bash\" {",
            "                args \"-c\" \"b\"",
            "                cwd \"/w\"",
            "            }",
            "        }",
            "    }",
            "}",
        ]
        .join("\n");
        assert_eq!(kdl, expected);
    }
}
