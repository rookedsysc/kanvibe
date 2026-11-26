//! hook이 상태를 status.json에 기록하고 targets.json의 모든 client에 통보하는 bash 조각(TS `shellHookScript.ts`).
//! 이 원문은 사용자 저장소의 hook 스크립트로 그대로 설치되므로 TS 출력과 바이트 단위로 같아야 한다.
//! 고정된 부분은 TS가 실제로 만든 문자열을 그대로 옮긴 원문 조각이고, 상태·provider·소켓 해석 함수만 끼워 넣는다.

use std::sync::LazyLock;

use regex::Regex;

use super::socket_path::{
    HOOK_TRANSPORT_VERSION_MARKER, build_shell_hook_socket_resolver,
    build_shell_hook_transport_resolver,
};
use crate::git_exclude::KANVIBE_STATE_DIR_EXCLUDE_PATTERN;

/// targets.json에는 이미 종료된 client의 주소가 남을 수 있다. 응답하지 않는 client 하나가 hook 전체를 붙잡지 않도록 통보에 상한을 둔다
const HOOK_NOTIFY_TIMEOUT_SECONDS: u32 = 3;
const HOOK_CHAT_PAYLOAD_MAX_BYTES: u32 = 64 * 1_024;

/// hook이 KanVibe에 통보할 수 있는 task 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellHookStatus {
    Progress,
    Pending,
    Review,
    Done,
}

impl ShellHookStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Pending => "pending",
            Self::Review => "review",
            Self::Done => "done",
        }
    }
}

/// hook 본문(stdin)을 대화 메타데이터로 함께 보내는 agent
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellHookAgentProvider {
    Claude,
    Codex,
    Gemini,
}

impl ShellHookAgentProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellTaskIdBindingStatus {
    pub has_task_id_binding: bool,
    pub bound_task_id: Option<String>,
}

static SHELL_TASK_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^TASK_ID="((?:\\.|[^"\\])*)"$"#).expect("유효한 정규식"));
static SHELL_ESCAPED_CHARACTER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\\([\\"$`])"#).expect("유효한 정규식"));
static PARALLEL_WAIT_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n\s*wait\n").expect("유효한 정규식"));

fn escape_shell_double_quoted_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn unescape_shell_double_quoted_value(value: &str) -> String {
    SHELL_ESCAPED_CHARACTER_PATTERN
        .replace_all(value, "$1")
        .into_owned()
}

pub fn build_shell_task_id_resolver(default_task_id: &str) -> String {
    format!(
        "TASK_ID=\"{}\"",
        escape_shell_double_quoted_value(default_task_id)
    )
}

pub fn extract_shell_task_id(content: &str) -> Option<String> {
    SHELL_TASK_ID_PATTERN
        .captures(content)
        .map(|captures| unescape_shell_double_quoted_value(&captures[1]))
}

/// provider가 있으면 hook stdin을 64 KiB까지 base64로 실어 보내는 조각. 남은 stdin도 비워야 큰 hook 본문을 쓰는 CLI에 broken pipe가 생기지 않는다
fn build_agent_session_metadata(provider: Option<ShellHookAgentProvider>) -> String {
    let Some(provider) = provider else {
        return "KANVIBE_AGENT_SESSION_JSON=".to_string();
    };
    [
        "KANVIBE_AGENT_SESSION_JSON=".to_string(),
        format!(
            "KANVIBE_HOOK_PAYLOAD_BASE64=\"$(if [ ! -t 0 ]; then {{ command -p head -c {HOOK_CHAT_PAYLOAD_MAX_BYTES}; command -p cat >/dev/null; }} 2>/dev/null | base64 | tr -d \"\\r\\n\"; fi)\""
        ),
        "if [ -n \"${KANVIBE_HOOK_PAYLOAD_BASE64}\" ]; then".to_string(),
        format!(
            "  KANVIBE_AGENT_SESSION_JSON=\",\\\"agentSession\\\":{{\\\"provider\\\":\\\"{}\\\",\\\"hookPid\\\":$$,\\\"payloadBase64\\\":\\\"${{KANVIBE_HOOK_PAYLOAD_BASE64}}\\\"}}\"",
            provider.as_str()
        ),
        "fi".to_string(),
    ]
    .join("\n")
}

/// hook이 상태를 status.json에 기록하고 targets.json의 모든 client에 병렬로 통보하는 shell 조각.
/// 프로젝트 색상은 project.json에 따로 있으므로 이 경로에서는 건드리지 않는다.
pub fn build_shell_kanvibe_status_updater(
    status: ShellHookStatus,
    provider: Option<ShellHookAgentProvider>,
) -> String {
    let status = status.as_str();
    [
        format!("KANVIBE_STATUS=\"{status}\"\n"),
        build_agent_session_metadata(provider),
        "\n".to_string(),
        STATUS_UPDATER_STATE_SECTION.to_string(),
        build_shell_hook_socket_resolver(),
        "\n".to_string(),
        build_shell_hook_transport_resolver(),
        "\n".to_string(),
        STATUS_UPDATER_POST_HEAD.to_string(),
        status.to_string(),
        STATUS_UPDATER_POST_TAIL.to_string(),
    ]
    .concat()
}

pub fn has_shell_kanvibe_status_json_persistence(content: &str) -> bool {
    [
        "status.json",
        "KANVIBE_TASK_STATE_FILE",
        "KANVIBE_GIT_COMMON_DIR",
        "--git-common-dir",
        "KANVIBE_GIT_EXCLUDE_FILE",
        KANVIBE_STATE_DIR_EXCLUDE_PATTERN,
        "KANVIBE_TASK_STATE_JSON",
        "\\\"schemaVersion\\\":1",
        "\\\"status\\\":\\\"${KANVIBE_STATUS}\\\"",
    ]
    .iter()
    .all(|fragment| content.contains(fragment))
}

pub fn has_shell_kanvibe_target_fanout(content: &str) -> bool {
    [
        HOOK_TRANSPORT_VERSION_MARKER,
        "targets.json",
        "KANVIBE_TARGETS_FILE",
        "KANVIBE_TARGET_ROWS",
        "KANVIBE_TARGET_URL",
        "KANVIBE_TARGET_TASK_ID",
        "while IFS=",
        "--url \"${KANVIBE_TARGET_URL%/}/api/hooks/status\"",
        "kanvibe_hook_socket",
        "KANVIBE_TARGET_CLIENT_ID",
        "taskId",
    ]
    .iter()
    .all(|fragment| content.contains(fragment))
}

/// 응답 없는 client가 hook을 붙잡지 못하도록 통보에 상한이 걸려 있는지 확인한다
pub fn has_shell_kanvibe_bounded_notify_timeout(content: &str) -> bool {
    content.contains(&format!("--max-time {HOOK_NOTIFY_TIMEOUT_SECONDS}"))
}

/// curl fan-out이 순차 실행이 아니라 병렬 실행 후 wait로 수렴하는지 확인한다
pub fn has_shell_kanvibe_parallel_target_fanout(content: &str) -> bool {
    has_shell_kanvibe_target_fanout(content)
        && content.contains("> /dev/null 2>&1 &")
        && PARALLEL_WAIT_PATTERN.is_match(content)
}

/// 모든 hook script가 같은 fallback task id를 고정하고 payload에서 사용하는지 확인한다.
/// TS처럼 빈 task id는 고정되지 않은 것으로 본다.
pub fn get_shell_task_id_binding_status(contents: &[&str]) -> ShellTaskIdBindingStatus {
    let has_task_id_payload_bindings = contents
        .iter()
        .all(|content| content.contains("taskId") && content.contains("${TASK_ID}"));
    let bound_task_ids: Vec<Option<String>> = contents
        .iter()
        .map(|content| extract_shell_task_id(content))
        .collect();
    let first_task_id = bound_task_ids
        .first()
        .cloned()
        .flatten()
        .filter(|task_id| !task_id.is_empty());
    let bound_task_id = first_task_id.filter(|first| {
        bound_task_ids
            .iter()
            .all(|task_id| task_id.as_deref() == Some(first.as_str()))
    });
    ShellTaskIdBindingStatus {
        has_task_id_binding: has_task_id_payload_bindings && bound_task_id.is_some(),
        bound_task_id,
    }
}

#[cfg(test)]
mod tests;

// 아래 원문 조각은 TS `buildShellKanvibeStatusUpdater`가 만든 문자열에서 그대로 떼어 왔다.
// 탭·줄바꿈이 원문에 그대로 들어 있으므로 편집기 설정으로 바꾸지 않는다.
const STATUS_UPDATER_STATE_SECTION: &str = r##"KANVIBE_REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd))"
KANVIBE_STATE_DIR="${KANVIBE_REPO_ROOT}/.kanvibe"
KANVIBE_TASK_STATE_FILE="${KANVIBE_STATE_DIR}/status.json"
KANVIBE_TARGETS_FILE="${KANVIBE_STATE_DIR}/targets.json"
KANVIBE_GIT_COMMON_DIR="$(git -C "${KANVIBE_REPO_ROOT}" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
if [ -n "${KANVIBE_GIT_COMMON_DIR}" ]; then
  KANVIBE_GIT_EXCLUDE_FILE="${KANVIBE_GIT_COMMON_DIR}/info/exclude"
  mkdir -p "$(dirname "${KANVIBE_GIT_EXCLUDE_FILE}")" 2>/dev/null || true
  touch "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
  if ! grep -qxF ".kanvibe/" "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null; then
    if ! grep -qxF "# KanVibe AI hooks (auto-generated)" "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null; then
      printf '
%s
' "# KanVibe AI hooks (auto-generated)" >> "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
    fi
    printf '%s
' ".kanvibe/" >> "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
  fi
fi

mkdir -p "${KANVIBE_STATE_DIR}" 2>/dev/null || true
KANVIBE_UPDATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || true)"

KANVIBE_TASK_STATE_JSON="{\"schemaVersion\":1,\"status\":\"${KANVIBE_STATUS}\""
if [ -n "${KANVIBE_UPDATED_AT}" ]; then
  KANVIBE_TASK_STATE_JSON="${KANVIBE_TASK_STATE_JSON},\"updatedAt\":\"${KANVIBE_UPDATED_AT}\""
fi
printf '%s}\n' "${KANVIBE_TASK_STATE_JSON}" > "${KANVIBE_TASK_STATE_FILE}" 2>/dev/null || true

KANVIBE_TARGET_ROWS="$(
  if [ -f "${KANVIBE_TARGETS_FILE}" ]; then
    grep -oE '"(url|taskId|clientId|token|remoteHostNamespace)"[[:space:]]*:[[:space:]]*"[^"]*"|"transport"[[:space:]]*:[[:space:]]*("[^"]*"|[^,}[:space:]]+)|[{}]' "${KANVIBE_TARGETS_FILE}" 2>/dev/null       | awk '
          function emit() { if(u!="" && t!="" && !(u in seen)){ seen[u]=1; print u "\t" t "\t" (c=="" ? "_" : c) "\t" (k=="" ? "_" : k) "\t" transport "\t" namespace } }
          function clear_target() { u=""; t=""; c=""; k=""; transport=""; namespace="" }
          $0=="{" { clear_target(); next }
          $0=="}" { emit(); clear_target(); next }
          { s=$0; sub(/^"[^"]*"[[:space:]]*:[[:space:]]*"/,"",s); sub(/"$/,"",s) }
          /^"url"/ { u=s; sub(/\/+$/,"",u); next }
          /^"taskId"/ { t=s; next }
          /^"clientId"/ { c=s; next }
          /^"token"/ { k=s; next }
          /^"remoteHostNamespace"/ { namespace=s; next }
          /^"transport"/ { transport=(s=="" ? "invalid" : s); next }
          END { emit() }
        ' 2>/dev/null || true
  else
    printf '%s	%s
' "${KANVIBE_URL%/}" "${TASK_ID}"
  fi
)"
if [ -z "${KANVIBE_TARGET_ROWS}" ]; then
  KANVIBE_TARGET_ROWS="$(printf '%s	%s
' "${KANVIBE_URL%/}" "${TASK_ID}")"
fi

"##;

const STATUS_UPDATER_POST_HEAD: &str = r##"
# 토큰은 명령줄 대신 표준 입력으로 curl에 넘겨 ps에 드러나지 않게 한다
kanvibe_post_status() {
  printf 'Authorization: Bearer %s
' "${KANVIBE_TARGET_TOKEN}" | curl -fsS --max-time 3 -X POST "$@" --url "${KANVIBE_TARGET_URL%/}/api/hooks/status"     -H @-     -H "x-kanvibe-client-id: ${KANVIBE_TARGET_CLIENT_ID}"     -H "Content-Type: application/json"     -d "{\"taskId\": \"${KANVIBE_TARGET_TASK_ID}\", \"status\": \""##;

const STATUS_UPDATER_POST_TAIL: &str = r##"\"${KANVIBE_AGENT_SESSION_JSON}}"
}

printf '%s
' "${KANVIBE_TARGET_ROWS}" | {
  while IFS="$(printf '\t')" read -r KANVIBE_TARGET_URL KANVIBE_TARGET_TASK_ID KANVIBE_TARGET_CLIENT_ID KANVIBE_TARGET_TOKEN KANVIBE_TARGET_TRANSPORT KANVIBE_TARGET_NAMESPACE; do
    if [ -z "${KANVIBE_TARGET_URL}" ] || [ -z "${KANVIBE_TARGET_TASK_ID}" ]; then
      continue
    fi
    case "${KANVIBE_TARGET_URL}" in
      http://*|https://*) ;;
      *) continue ;;
    esac
    KANVIBE_TARGET_SOCKET="$(kanvibe_hook_socket "${KANVIBE_TARGET_CLIENT_ID}" "${KANVIBE_TARGET_NAMESPACE}")"
    {
      if kanvibe_hook_uses_local_http "${KANVIBE_TARGET_URL}" "${KANVIBE_TARGET_TRANSPORT}"; then
        kanvibe_post_status
      elif [ -n "${KANVIBE_TARGET_SOCKET}" ]; then
        kanvibe_post_status --unix-socket "${KANVIBE_TARGET_SOCKET}"
      else
        false
      fi
    } > /dev/null 2>&1 &
  done
  wait
}"##;
