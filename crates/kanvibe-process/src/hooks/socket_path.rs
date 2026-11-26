//! 원격 hook·CLI가 데스크탑을 찾는 소켓 경로 규칙(TS `hookSocketPath.ts`).
//! 원격 hook·CLI는 namespace의 안정 소켓과 client ID로 데스크탑을 찾는다. 이전 전송은 client ID별 터널 소켓을 유지한다.
//! 생성 셸과 Rust가 같은 경로·신뢰 검사를 쓰고, NFS 공유 홈과 Unix 소켓 길이 제한을 피하도록 /tmp의 사용자 전용 디렉터리에 둔다.
//! 여기서 만드는 셸 문자열은 원격에 그대로 설치되므로 TS 출력과 바이트 단위로 같아야 한다.

use std::sync::LazyLock;

use regex::Regex;

const HOOK_SOCKET_DIR_SHELL: &str = "/tmp/kv-$(id -u)";

pub const HOOK_TRANSPORT_VERSION_MARKER: &str = "KANVIBE_HOOK_TRANSPORT_VERSION=4";
pub const HOOK_LOOPBACK_URL_PATTERN: &str =
    r"^https?://(localhost|127[.]0[.]0[.]1|\[::1\])(:[0-9]+)?/?$";

static HOOK_CLIENT_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[A-Za-z0-9-]+$").expect("유효한 정규식"));
static HOST_NAMESPACE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^(dev|prod|qa-[a-z0-9-]{1,24})$").expect("유효한 정규식"));

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookSocketPathError {
    #[error("소켓 이름으로 쓸 수 없는 client ID입니다: {0}")]
    InvalidClientId(String),
    #[error("Invalid hook namespace")]
    InvalidNamespace,
}

/// 원격 host daemon namespace(TS `remoteHostProtocol.isValidHostNamespace`)
pub fn is_valid_host_namespace(namespace: &str) -> bool {
    HOST_NAMESPACE_PATTERN.is_match(namespace)
}

/// targets.json은 브랜치 checkout으로 바뀔 수 있으므로 경로 문자가 섞인 ID는 소켓 이름으로 쓰지 않는다
pub fn is_valid_hook_client_id(client_id: &str) -> bool {
    HOOK_CLIENT_ID_PATTERN.is_match(client_id)
}

pub fn build_hook_socket_file_name(client_id: &str) -> Result<String, HookSocketPathError> {
    if !is_valid_hook_client_id(client_id) {
        return Err(HookSocketPathError::InvalidClientId(client_id.to_string()));
    }
    Ok(format!("{client_id}.sock"))
}

/// transport가 없는 구형 등록도 loopback 주소만 TCP를 허용한다
pub fn build_shell_hook_transport_resolver() -> String {
    [
        format!("# {HOOK_TRANSPORT_VERSION_MARKER}"),
        "kanvibe_hook_uses_local_http() {".into(),
        "  case \"$2\" in".into(),
        "    \"\"|local) ;;".into(),
        "    *) return 1 ;;".into(),
        "  esac".into(),
        "  case \"$1\" in".into(),
        "    *[![:print:]]*) return 1 ;;".into(),
        "  esac".into(),
        format!("  printf '%s\\n' \"$1\" | grep -Eiq '{HOOK_LOOPBACK_URL_PATTERN}'"),
        "}".into(),
    ]
    .join("\n")
}

/// 이 소켓으로 task 토큰이 나가므로, 본인 소유이고 0700인 디렉터리 안의 본인 소유 소켓만 인정한다.
/// 셸 함수와 한 줄짜리 원격 명령이 같은 조건을 쓰도록 한곳에서 만든다.
fn build_shell_trusted_socket_condition(directory_variable: &str, socket_variable: &str) -> String {
    let directory = format!("\"${{{directory_variable}}}\"");
    let socket = format!("\"${{{socket_variable}}}\"");
    [
        format!("[ ! -L {directory} ]"),
        format!("[ -O {directory} ]"),
        format!("[ \"$(ls -ld {directory} 2>/dev/null | cut -c1-10)\" = \"drwx------\" ]"),
        format!("[ ! -L {socket} ]"),
        format!("[ -S {socket} ]"),
        format!("[ -O {socket} ]"),
    ]
    .join(" && ")
}

/// 데스크탑이 원격에 보내는 한 줄짜리 명령(설치 스크립트 내려받기, 상태 확인)에서 쓰는 소켓 경로 대입.
/// 믿을 수 있는 소켓일 때만 경로를 담고, 아니면 빈 값을 담는다.
pub fn build_shell_owned_hook_socket_assignment(
    variable_name: &str,
    client_id: &str,
    namespace: Option<&str>,
) -> Result<String, HookSocketPathError> {
    let directory_variable = format!("{variable_name}_dir");
    let directory = format!("\"${{{directory_variable}}}\"");
    if let Some(namespace) = namespace {
        if !is_valid_host_namespace(namespace) {
            return Err(HookSocketPathError::InvalidNamespace);
        }
        return Ok([
            format!("{directory_variable}=\"{HOOK_SOCKET_DIR_SHELL}\""),
            format!(
                "kanvibe_parent_trusted=; {{ [ ! -L {directory} ] && [ -O {directory} ] && [ \"$(ls -ld {directory} 2>/dev/null | cut -c1-10)\" = \"drwx------\" ]; }} && kanvibe_parent_trusted=1"
            ),
            format!("{directory_variable}=\"${{{directory_variable}}}/{namespace}\""),
            format!("{variable_name}=\"${{{directory_variable}}}/hooks-v1.sock\""),
            format!(
                "{{ [ \"$kanvibe_parent_trusted\" = 1 ] && {}; }} || {variable_name}=",
                build_shell_trusted_socket_condition(&directory_variable, variable_name)
            ),
        ]
        .join("; "));
    }
    let file_name = build_hook_socket_file_name(client_id)?;
    Ok([
        format!("{directory_variable}=\"{HOOK_SOCKET_DIR_SHELL}\""),
        format!("{variable_name}=\"${{{directory_variable}}}/{file_name}\""),
        format!(
            "{{ {}; }} || {variable_name}=",
            build_shell_trusted_socket_condition(&directory_variable, variable_name)
        ),
    ]
    .join("; "))
}

/// 터널 소켓을 쓸 준비를 하고 경로를 출력하는 원격 명령.
/// 이전 실행이 남긴 소켓이 있으면 sshd가 bind를 거부하므로 먼저 지운다. 다른 사용자가 미리 만든 디렉터리에는 소켓을 두지 않는다.
pub fn build_remote_hook_socket_prepare_command(
    client_id: &str,
) -> Result<String, HookSocketPathError> {
    let file_name = build_hook_socket_file_name(client_id)?;
    let socket = format!("${{kanvibe_socket_dir}}/{file_name}");
    Ok([
        format!("kanvibe_socket_dir=\"{HOOK_SOCKET_DIR_SHELL}\""),
        "mkdir -p \"${kanvibe_socket_dir}\"".into(),
        "[ ! -L \"${kanvibe_socket_dir}\" ]".into(),
        "[ -O \"${kanvibe_socket_dir}\" ]".into(),
        "chmod 700 \"${kanvibe_socket_dir}\"".into(),
        format!("[ ! -L \"{socket}\" ]"),
        format!(
            "{{ [ ! -e \"{socket}\" ] || {{ [ -S \"{socket}\" ] && [ -O \"{socket}\" ]; }}; }}"
        ),
        format!("rm -f \"{socket}\""),
        format!("printf '%s' \"{socket}\""),
    ]
    .join(" && "))
}

/// targets.json 항목의 client ID로 터널 소켓 경로를 출력하는 POSIX 셸 함수.
/// ID가 없는 옛 항목이거나 쓸 수 없는 소켓이면 아무것도 출력하지 않는다.
pub fn build_shell_hook_socket_resolver() -> String {
    [
        "kanvibe_hook_socket() {".to_string(),
        "  case \"$1\" in".into(),
        "    \"\"|*[!A-Za-z0-9-]*) return 0 ;;".into(),
        "  esac".into(),
        format!("  kanvibe_socket_dir=\"{HOOK_SOCKET_DIR_SHELL}\""),
        "  kanvibe_socket_path=\"${kanvibe_socket_dir}/$1.sock\"".into(),
        "  if [ -n \"$2\" ]; then".into(),
        "    case \"$2\" in".into(),
        "      dev|prod|qa-?*) ;;".into(),
        "      *) return 0 ;;".into(),
        "    esac".into(),
        "    case \"$2\" in *[!a-z0-9-]*|????????????????????????????*) return 0 ;; esac".into(),
        "    if [ -L \"${kanvibe_socket_dir}\" ] || [ ! -O \"${kanvibe_socket_dir}\" ] || [ \"$(ls -ld \"${kanvibe_socket_dir}\" 2>/dev/null | cut -c1-10)\" != \"drwx------\" ]; then return 0; fi".into(),
        "    kanvibe_socket_dir=\"${kanvibe_socket_dir}/$2\"".into(),
        "    kanvibe_socket_path=\"${kanvibe_socket_dir}/hooks-v1.sock\"".into(),
        "  fi".into(),
        format!(
            "  if {}; then",
            build_shell_trusted_socket_condition("kanvibe_socket_dir", "kanvibe_socket_path")
        ),
        "    printf '%s' \"${kanvibe_socket_path}\"".into(),
        "  fi".into(),
        "}".into(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_ids_with_path_characters_cannot_name_a_socket() {
        assert_eq!(
            build_hook_socket_file_name("client-1").unwrap(),
            "client-1.sock"
        );
        assert!(build_hook_socket_file_name("../client").is_err());
        assert!(build_hook_socket_file_name("").is_err());
    }

    #[test]
    fn host_namespaces_follow_the_remote_daemon_rule() {
        assert!(is_valid_host_namespace("dev"));
        assert!(is_valid_host_namespace("qa-run-1"));
        assert!(!is_valid_host_namespace("qa-"));
        assert!(!is_valid_host_namespace("staging"));
    }
}
