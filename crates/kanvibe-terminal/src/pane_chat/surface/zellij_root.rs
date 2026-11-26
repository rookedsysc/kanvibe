//! Zellij pane의 root PID. `list-panes` JSON에 PID가 없는 Zellij(0.46.0)에서, 서버가 pane 프로세스에 넣은 환경 변수로 찾는다.

use kanvibe_process::worktree::quote_for_posix_shell;

/// Zellij pane root 조회 스크립트. `$1`은 세션 이름, `$2`는 pane 번호다.
///
/// 명령줄에 `--server`가 있는 `zellij` 프로세스를 서버로 보고, 그 직접 자식 가운데 환경의 `ZELLIJ_PANE_ID`와
/// `ZELLIJ_SESSION_NAME`이 모두 맞는 PID를 한 줄씩 출력한다. pane 안에서 띄운 자손도 같은 값을 물려받으므로 직접 자식만 본다.
/// 0.46.0의 서버 명령줄에는 세션 이름이 없어(소켓 이름이 불투명 ID다) 자식의 세션 이름으로 맞춘다. `/proc`만 읽고 아무것도
/// 바꾸지 않으며, `/proc`가 없으면(macOS) 아무것도 출력하지 않는다.
const ZELLIJ_PANE_ROOT_SCRIPT: &str = r#"session=$1
pane=$2
for comm in $(grep -lxF -e zellij /proc/[0-9]*/comm 2>/dev/null); do
  server=${comm#/proc/}
  server=${server%/comm}
  tr '\0' '\n' < "/proc/$server/cmdline" 2>/dev/null | grep -qxF -e --server || continue
  environs=
  for child in $(cat /proc/$server/task/*/children 2>/dev/null); do
    environs="$environs /proc/$child/environ"
  done
  [ -n "$environs" ] || continue
  panes=$(grep -lzxF -e "ZELLIJ_PANE_ID=$pane" $environs 2>/dev/null)
  [ -n "$panes" ] || continue
  for environ in $(grep -lzxF -e "ZELLIJ_SESSION_NAME=$session" $panes 2>/dev/null); do
    root=${environ#/proc/}
    echo "${root%/environ}"
  done
done"#;

/// 원격 로그인 셸이 zsh여도 단어 나누기가 같도록 POSIX `sh`로 돌리는 명령 하나
pub(super) fn build_zellij_pane_root_command(session_name: &str, pane_number: u64) -> String {
    format!(
        "sh -c {} kanvibe-zellij-pane-root {} {pane_number}",
        quote_for_posix_shell(ZELLIJ_PANE_ROOT_SCRIPT),
        quote_for_posix_shell(session_name)
    )
}

/// 후보가 정확히 하나이고 양의 정수일 때만 그 PID다. 하나로 좁혀지지 않으면 어느 것도 고르지 않는다
pub(super) fn parse_zellij_pane_root(output: &str) -> Option<i64> {
    let mut candidates = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let candidate = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    candidate.parse::<i64>().ok().filter(|pid| *pid > 0)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    use super::*;

    fn run_lookup(session_name: &str, pane_number: u64) -> String {
        let output = Command::new("sh")
            .args([
                "-c",
                &build_zellij_pane_root_command(session_name, pane_number),
            ])
            .output()
            .expect("sh 실행");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// 이름이 `zellij`이고 인자에 `--server`가 있는 가짜 서버. pane 환경을 가진 직접 자식과, 같은 값을 물려받은 손자를 띄운다
    fn spawn_fake_server(directory: &std::path::Path, session_name: &str) -> Child {
        let server = directory.join("zellij");
        std::fs::write(
            &server,
            format!(
                "#!/bin/sh\nZELLIJ_PANE_ID=5 ZELLIJ_SESSION_NAME={session_name} sh -c 'sleep 30 & wait' &\nwait\n"
            ),
        )
        .expect("가짜 서버 스크립트");
        std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755))
            .expect("실행 권한");
        Command::new(&server)
            .args(["--server", "x"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("가짜 서버")
    }

    fn direct_children(pid: u32) -> Vec<i64> {
        std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|child| child.parse().ok())
            .collect()
    }

    /// 실제 `sh`로 돌리면 두 환경 변수가 모두 맞는 서버의 직접 자식 PID 한 줄만 출력하고, 손자와 다른 세션은 출력하지 않는다
    #[test]
    fn lookup_prints_only_the_direct_child_with_both_pane_variables() {
        if !std::path::Path::new("/proc/self/task").exists() {
            return;
        }
        let directory = tempfile::tempdir().expect("임시 폴더");
        let session_name = format!("kanvibe-root-test-{}", std::process::id());
        let mut server = spawn_fake_server(directory.path(), &session_name);
        let mut output = String::new();
        let mut children = Vec::new();
        for _ in 0..100 {
            children = direct_children(server.id());
            output = run_lookup(&session_name, 5);
            if !children.is_empty() && !output.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let other_session = run_lookup("another-session", 5);
        let other_pane = run_lookup(&session_name, 6);
        for child in &children {
            let _ = Command::new("pkill")
                .args(["-P", &child.to_string()])
                .status();
        }
        let _ = Command::new("pkill")
            .args(["-P", &server.id().to_string()])
            .status();
        let _ = server.kill();
        let _ = server.wait();

        assert_eq!(children.len(), 1, "가짜 서버의 직접 자식: {children:?}");
        assert_eq!(output, format!("{}\n", children[0]));
        assert_eq!(other_session, "");
        assert_eq!(other_pane, "");
    }

    #[test]
    fn single_positive_pid_line_is_the_root() {
        assert_eq!(parse_zellij_pane_root("2380671\n"), Some(2380671));
        assert_eq!(parse_zellij_pane_root("\n 42 \n\n"), Some(42));
    }

    #[test]
    fn output_that_does_not_narrow_to_one_pid_is_not_a_root() {
        assert_eq!(parse_zellij_pane_root(""), None);
        assert_eq!(parse_zellij_pane_root("100\n200\n"), None);
        assert_eq!(parse_zellij_pane_root("zellij screen"), None);
        assert_eq!(parse_zellij_pane_root("0\n"), None);
        assert_eq!(parse_zellij_pane_root("-3\n"), None);
    }
}
