//! Herdr CLI 명령 문자열. 사용자 셸의 `HERDR_*` 변수가 KanVibe 서버 선택을 바꾸지 않도록 명령마다 지운다.

use kanvibe_process::host_file_access::quote_shell_argument;

const SHARED_SERVER_PREFIX: &str = "kanvibe:";
const SHARED_SERVER_NAME: &str = "kanvibe";
const HERDR_EXECUTABLE: &str = r#""$(command -v herdr || printf '%s' "$HOME/.local/bin/herdr")""#;

/// 세션 이름이 가리키는 Herdr 서버와 공유 서버 안의 Space 키
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrSessionTarget {
    pub server_name: String,
    /// `kanvibe:<key>` 공유 서버 세션일 때만 있다
    pub workspace_key: Option<String>,
}

/// `kanvibe:<key>`는 공유 서버 `kanvibe` 안의 Space, 그 밖의 이름은 독립 서버다. 키가 비면 오류다
pub fn parse_herdr_session_target(session_name: &str) -> Result<HerdrSessionTarget, String> {
    let Some(workspace_key) = session_name.strip_prefix(SHARED_SERVER_PREFIX) else {
        return Ok(HerdrSessionTarget {
            server_name: session_name.to_string(),
            workspace_key: None,
        });
    };
    if workspace_key.is_empty() {
        return Err("Herdr Space 식별자가 비어 있습니다.".to_string());
    }
    Ok(HerdrSessionTarget {
        server_name: SHARED_SERVER_NAME.to_string(),
        workspace_key: Some(workspace_key.to_string()),
    })
}

pub fn build_herdr_command(session_name: &str, arguments: &[&str]) -> Result<String, String> {
    let target = parse_herdr_session_target(session_name)?;
    let quoted_arguments: Vec<String> = arguments
        .iter()
        .map(|argument| quote_shell_argument(argument))
        .collect();
    Ok(format!(
        "env -u HERDR_ENV -u HERDR_SESSION -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID {HERDR_EXECUTABLE} --session {} {}",
        quote_shell_argument(&target.server_name),
        quoted_arguments.join(" ")
    ))
}

/// 독립 Herdr 서버를 멈추고 지운다. 공유 서버는 다른 task가 함께 쓰므로 거부한다
pub fn build_herdr_session_cleanup_command(session_name: &str) -> Result<String, String> {
    if parse_herdr_session_target(session_name)?
        .workspace_key
        .is_some()
    {
        return Err("공유 Herdr 서버는 task와 함께 종료할 수 없습니다.".to_string());
    }
    Ok(format!(
        "{} >/dev/null 2>&1 || true; {}",
        build_herdr_command(session_name, &["session", "stop", session_name])?,
        build_herdr_command(session_name, &["session", "delete", session_name])?
    ))
}

/// `--session`은 client socket override를 무시하므로 이 클라이언트의 env로만 서버와 소켓을 지정한다
pub fn build_herdr_client_command(session_name: &str, socket_path: &str) -> Result<String, String> {
    let target = parse_herdr_session_target(session_name)?;
    Ok(format!(
        "env -u HERDR_ENV -u HERDR_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID HERDR_SESSION={} HERDR_CLIENT_SOCKET_PATH={} {HERDR_EXECUTABLE} client",
        quote_shell_argument(&target.server_name),
        quote_shell_argument(socket_path)
    ))
}
