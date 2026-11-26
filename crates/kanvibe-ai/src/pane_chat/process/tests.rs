use super::*;

const SHELL: &str = "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh";

fn row(pid: i64, parent: i64, command: &str, args: &str) -> String {
    format!("{pid} {parent} 200 200 ttys010 Thu Nov 12 19:00:00 2026 {command} {args}")
}

fn find(rows: &[String]) -> Option<PaneChatProcess> {
    let mut lines = vec![SHELL.to_string()];
    lines.extend(rows.iter().cloned());
    find_pane_chat_process(&lines.join("\n"), 100)
}

fn found(pid: i64, provider: PaneChatProvider) -> Option<(i64, PaneChatProvider)> {
    Some((pid, provider))
}

fn pid_and_provider(process: Option<PaneChatProcess>) -> Option<(i64, PaneChatProvider)> {
    process.map(|process| (process.pid, process.provider))
}

#[test]
fn recognizes_real_cli_launch_forms() {
    let cases = [
        (
            "2.1.325",
            "claude --dangerously-skip-permissions",
            PaneChatProvider::Claude,
        ),
        (
            "node",
            "node --no-warnings /opt/node_modules/@google/gemini-cli/bundle/gemini.js",
            PaneChatProvider::Gemini,
        ),
        (
            "node",
            "node --max-old-space-size 4096 /opt/node_modules/@google/gemini-cli/dist/index.js",
            PaneChatProvider::Gemini,
        ),
        ("bun", "bun /opt/bin/opencode", PaneChatProvider::Opencode),
        (
            "opencode.exe",
            "/opt/node_modules/opencode-ai/bin/opencode.exe",
            PaneChatProvider::Opencode,
        ),
    ];
    for (command, args, provider) in cases {
        assert_eq!(
            pid_and_provider(find(&[row(200, 100, command, args)])),
            found(200, provider),
            "{command} {args}"
        );
    }
}

#[test]
fn picks_gemini_memory_relaunch_child() {
    let script = "/opt/node_modules/@google/gemini-cli/bundle/gemini.js";
    let wrapper = row(200, 100, "node", &format!("node {script}"));
    let relaunched = row(
        201,
        200,
        "node",
        &format!("node --max-old-space-size=4096 {script}"),
    );
    assert_eq!(
        pid_and_provider(find(&[wrapper.clone(), relaunched])),
        found(201, PaneChatProvider::Gemini)
    );
    let plain_child = row(201, 200, "node", &format!("node {script}"));
    assert_eq!(find(&[wrapper.clone(), plain_child.clone()]), None);
    assert_eq!(
        find(&[wrapper, plain_child, row(202, 201, "gemini", "gemini")]),
        None
    );
}

#[test]
fn eval_and_option_values_are_not_scripts() {
    assert_eq!(
        find(&[row(
            200,
            100,
            "node",
            "node -e /opt/node_modules/@google/gemini-cli/dist/index.js"
        )]),
        None
    );
    assert_eq!(
        find(&[row(
            200,
            100,
            "node",
            "node --require /opt/bin/gemini /tmp/other.js"
        )]),
        None
    );
}

#[test]
fn recognizes_claude_native_version_executable() {
    let shell = "40181 1 40181 40185 ttys138 Fri Nov 13 11:29:03 2026 zsh /bin/zsh";
    let native = "40185 40181 40185 40185 ttys138 Fri Nov 13 11:29:04 2026 2.1.323 /Users/rookedsysc/.local/bin/claude --bare --tools --strict-mcp-config --mcp-config {\"mcpServers\":{}} --model qa-local";
    assert_eq!(
        find_pane_chat_process(&format!("{shell}\n{native}"), 40181),
        Some(PaneChatProcess {
            pid: 40185,
            process_start_time: "Fri Nov 13 11:29:04 2026".to_string(),
            provider: PaneChatProvider::Claude,
            codex_server_socket: None,
        })
    );
    let native_args = native.split_once("2.1.323 ").unwrap().1;
    assert!(is_pane_chat_provider_process(
        "2.1.323",
        native_args,
        PaneChatProvider::Claude
    ));
    assert!(is_pane_chat_provider_process(
        "2.1.323",
        "/Users/user/.local/share/claude/versions/2.1.323 --bare",
        PaneChatProvider::Claude
    ));
}

#[test]
fn refuses_processes_that_only_mention_claude() {
    let cases = [
        (
            "2.1.323",
            "/usr/bin/echo /Users/user/.local/bin/claude --model qa-local",
        ),
        (
            "2.1.323",
            "/tmp/2.1.323 --claude /Users/user/.local/bin/claude",
        ),
        (
            "2.1.323",
            "/Users/user/.local/share/claude/versions/2.1.322 --bare",
        ),
        ("2.1.323", "/tmp/claude/versions/2.1.323 --bare"),
        ("vim", "/Users/user/.local/bin/claude"),
        ("echo", "echo /Users/user/.local/bin/claude"),
    ];
    for (command, args) in cases {
        assert_eq!(
            find(&[row(200, 100, command, args)]),
            None,
            "{command} {args}"
        );
        assert!(!is_pane_chat_provider_process(
            command,
            args,
            PaneChatProvider::Claude
        ));
    }
}

#[test]
fn nested_codex_launched_by_codex_is_ambiguous() {
    let rows = [
        row(200, 100, "codex", "/usr/local/bin/codex"),
        row(210, 200, "sh", "sh -c codex exec"),
        row(220, 210, "codex", "/usr/local/bin/codex exec prompt"),
    ];
    assert_eq!(find(&rows), None);
}

const CODEX_WRAPPER_ARGS: &str = "node /usr/local/lib/node_modules/@openai/codex/bin/codex.js";
const CODEX_NATIVE_ARGS: &str =
    "/usr/local/lib/node_modules/@openai/codex/vendor/aarch64-apple-darwin/codex/codex";

#[test]
fn picks_native_codex_under_verified_wrapper() {
    let rows = [
        row(200, 100, "node", CODEX_WRAPPER_ARGS),
        row(201, 200, "codex", CODEX_NATIVE_ARGS),
    ];
    assert_eq!(
        pid_and_provider(find(&rows)),
        found(201, PaneChatProvider::Codex)
    );
}

#[test]
fn wrapper_with_nested_codex_is_ambiguous() {
    let rows = [
        row(200, 100, "node", CODEX_WRAPPER_ARGS),
        row(201, 200, "codex", CODEX_NATIVE_ARGS),
        row(220, 201, "codex", "/usr/local/bin/codex exec prompt"),
    ];
    assert_eq!(find(&rows), None);
}

#[test]
fn codex_remote_socket_is_only_kept_for_unix_addresses() {
    let unix = find(&[row(
        200,
        100,
        "codex",
        "codex --remote unix:///tmp/codex.sock",
    )])
    .unwrap();
    assert_eq!(
        unix.codex_server_socket,
        Some(Some("/tmp/codex.sock".to_string()))
    );
    let tcp = find(&[row(200, 100, "codex", "codex --remote=ws://127.0.0.1:1")]).unwrap();
    assert_eq!(
        serde_json::to_value(&tcp).unwrap()["codexServerSocket"],
        serde_json::Value::Null
    );
    let local = find(&[row(200, 100, "codex", "codex")]).unwrap();
    assert!(
        serde_json::to_value(&local)
            .unwrap()
            .get("codexServerSocket")
            .is_none()
    );
}

#[test]
fn descendant_table_rows_on_other_ttys_do_not_link_the_cli_to_the_pane() {
    // root와 그 자손만 읽은 표(`ps -q`)에는 다른 tty로 옮긴 중간 프로세스가 들어 있을 수 있다.
    // tty로 읽은 표에는 그 줄이 없어 CLI가 pane의 자손으로 이어지지 않았으므로, 자손 표에서도 같은 판정이어야 한다
    let detached_parent = "150 100 200 200 ttys011 Thu Nov 12 19:00:00 2026 tmux tmux new-session";
    let claude_under_detached_parent = row(250, 150, "claude", "claude");
    assert_eq!(
        find(&[detached_parent.to_string(), claude_under_detached_parent]),
        None
    );

    let claude_on_pane_tty = row(300, 100, "claude", "claude");
    assert_eq!(
        pid_and_provider(find(&[detached_parent.to_string(), claude_on_pane_tty])),
        found(300, PaneChatProvider::Claude)
    );
}
