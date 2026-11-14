import path from "path";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import { buildShellHookSocketResolver, buildShellHookTransportResolver } from "@/lib/hookSocketPath";
import { KANVIBE_DIR_NAME } from "@/lib/kanvibeProjectState";
import type { ShellHookProviderFile } from "@/lib/shellHookProvider";

export const TASK_CLI_FILE_NAME = "task.sh";

/** KanVibe client가 다른 client에 전파한 결과를 task CLI 출력에서 골라낼 때 쓰는 줄 머리 */
export const TASK_CLI_PEER_RESULT_PREFIX = "__KANVIBE_PEER_RESULT__";

const TASK_CLI_FILE_MODE = 0o755;

/**
 * worktree의 `.kanvibe/task.sh` 본문.
 * 어느 client에 보낼지와 자격 증명은 실행 시점에 같은 디렉터리의 targets.json에서 읽으므로
 * 본문은 task나 client에 따라 달라지지 않고, 여러 client가 번갈아 설치해도 서로 덮어쓰는 값이 없다.
 */
const TASK_CLI_SCRIPT = `#!/bin/bash

# KanVibe task CLI
# 이 worktree에 등록된 KanVibe client에 task 생성·삭제를 요청한다. 사용법: bash .kanvibe/task.sh help

set -u

KANVIBE_CLI_DIR="$(cd "$(dirname "\${BASH_SOURCE[0]:-$0}")" && pwd)"
KANVIBE_REPO_ROOT="$(dirname "\${KANVIBE_CLI_DIR}")"
KANVIBE_TARGETS_FILE="\${KANVIBE_CLI_DIR}/targets.json"
KANVIBE_CONNECT_TIMEOUT_SECONDS=3
KANVIBE_REQUEST_TIMEOUT_SECONDS=300
KANVIBE_CONNECTION_FAILED=75
KANVIBE_USAGE_ERROR=2
RESPONSE_BODY=""
RESPONSE_STATUS=""
SELECTED_TARGET_ROWS=""

print_usage() {
  cat <<'USAGE'
사용법:
  bash .kanvibe/task.sh create --branch <새 브랜치> [--title <제목>] [--from-task <task ID>]
                               [--session-type tmux|zellij|herdr|terminal] [--target-url <client URL>]
  bash .kanvibe/task.sh delete --yes [--branch <삭제할 task 브랜치>] [--force] [--target-url <client URL>]

create  현재 worktree의 브랜치(또는 --from-task로 지정한 task의 브랜치)에서 새 worktree와 task를 만든다.
        요청을 받은 client만 worktree를 만들고, 나머지 client에는 task 등록만 전파된다.
delete  task와 그 worktree·브랜치·세션을 등록된 모든 client에서 삭제한다.
        커밋하지 않은 변경이 있으면 거부하며, --force를 붙이면 그래도 삭제한다.
--target-url을 생략하면 targets.json의 첫 client로 보내고, 연결이 안 될 때만 다음 client로 넘어간다.
USAGE
}

fail() {
  printf 'kanvibe task: %s\\n' "$1" >&2
  exit "\${2:-1}"
}

# JSON 문자열로 감싼다. 따옴표·백슬래시와 함께 개행·탭 같은 모든 제어 문자를 이스케이프해 client가 본문을 거절하지 않게 한다
json_string() {
  local value=$1 escaped="" char char_code index
  for (( index = 0; index < \${#value}; index++ )); do
    char=\${value:index:1}
    case "\${char}" in
      '"') escaped+='\\"' ;;
      '\\') escaped+='\\\\' ;;
      *)
        printf -v char_code '%d' "'\${char}"
        if [ "\${char_code}" -ge 0 ] && [ "\${char_code}" -lt 32 ]; then
          printf -v char '\\\\u%04x' "\${char_code}"
        fi
        escaped+=\${char}
        ;;
    esac
  done
  printf '"%s"' "\${escaped}"
}

json_field() {
  printf ',"%s":%s' "$1" "$(json_string "$2")"
}

# targets.json의 client마다 "url<TAB>taskId<TAB>clientId<TAB>token<TAB>transport" 한 줄을 출력한다.
# 탭이 이어지면 read가 빈 칸을 건너뛰므로, client ID가 없는 옛 항목은 소켓 이름으로 쓸 수 없는 _ 를 채운다
# targets.json은 브랜치 checkout으로 바뀔 수 있으므로 http(s) 주소가 아닌 항목은 curl에 넘기지 않는다
read_target_rows() {
  if [ ! -f "\${KANVIBE_TARGETS_FILE}" ]; then
    return 0
  fi

  grep -oE '"(url|taskId|token|clientId|remoteHostNamespace)"[[:space:]]*:[[:space:]]*"[^"]*"|"transport"[[:space:]]*:[[:space:]]*("[^"]*"|[^,}[:space:]]+)|[{}]' "\${KANVIBE_TARGETS_FILE}" 2>/dev/null \\
    | awk '
        function emit() { if (url ~ /^https?:\\/\\/[^[:space:]]+$/ && taskId != "") print url "\\t" taskId "\\t" (clientId == "" ? "_" : clientId) "\\t" (token == "" ? "_" : token) "\\t" transport "\\t" namespace }
        function clear_target() { url = ""; taskId = ""; clientId = ""; token = ""; transport = ""; namespace = "" }
        $0 == "{" { clear_target(); next }
        $0 == "}" { emit(); clear_target(); next }
        {
          key = $0; sub(/^"/, "", key); sub(/".*$/, "", key)
          value = $0; sub(/^"[^"]*"[[:space:]]*:[[:space:]]*"/, "", value); sub(/"$/, "", value)
        }
        key == "url" { url = value; sub(/\\/+$/, "", url); next }
        key == "taskId" { taskId = value; next }
        key == "clientId" { clientId = value; next }
        key == "remoteHostNamespace" { namespace = value; next }
        key == "token" { token = value; next }
        key == "transport" { transport = (value == "" ? "invalid" : value); next }
        END { emit() }
      '
}

# 요청을 보낼 client 줄을 SELECTED_TARGET_ROWS에 담는다. 실패를 subshell에 가두지 않도록 출력 대신 변수를 쓴다
select_target_rows() {
  local requested_url="\${1%/}"
  SELECTED_TARGET_ROWS="$(read_target_rows)"

  if [ -z "\${SELECTED_TARGET_ROWS}" ]; then
    fail "\${KANVIBE_TARGETS_FILE}에 등록된 KanVibe client가 없습니다. KanVibe에서 이 task의 hooks를 다시 설치하세요."
  fi

  if [ -n "\${requested_url}" ]; then
    SELECTED_TARGET_ROWS="$(printf '%s\\n' "\${SELECTED_TARGET_ROWS}" | awk -F '\\t' -v url="\${requested_url}" '$1 == url')"
    if [ -z "\${SELECTED_TARGET_ROWS}" ]; then
      fail "--target-url \${requested_url} 은(는) targets.json에 등록된 client가 아닙니다." "\${KANVIBE_USAGE_ERROR}"
    fi
  fi
}

# 꺼진 PC처럼 패킷을 버리는 주소는 연결 거부(6·7) 대신 시간 초과(28)로 끝난다.
# 연결이 맺어지기 전에 끝났으면 time_connect가 0이므로 연결 실패로 본다. 요청이 나간 뒤의 시간 초과는 이미 처리됐을 수 있어 여기에 넣지 않는다.
is_connect_timeout() {
  [ "$1" -eq 28 ] || return 1
  case "$2" in
    ""|*[1-9]*) return 1 ;;
    *) return 0 ;;
  esac
}

${buildShellHookSocketResolver()}
${buildShellHookTransportResolver()}

# 로컬 loopback 이외의 등록은 터널 소켓으로만 보낸다.
post_json() {
  local url=$1 token=$2 path=$3 body=$4 client_id=$5 transport=$6 namespace=$7 socket_path
  if kanvibe_hook_uses_local_http "\${url}" "\${transport}"; then
    send_json_request "\${url}" "\${token}" "\${path}" "\${body}"
    return $?
  fi
  socket_path="$(kanvibe_hook_socket "\${client_id}" "\${namespace}")"
  if [ -z "\${socket_path}" ]; then
    RESPONSE_STATUS="000"
    RESPONSE_BODY="신뢰할 수 있는 SSH 터널 소켓에 연결할 수 없습니다"
    return "\${KANVIBE_CONNECTION_FAILED}"
  fi
  send_json_request "\${url}" "\${token}" "\${path}" "\${body}" --unix-socket "\${socket_path}" -H "x-kanvibe-client-id: \${client_id}"
}

# 성공 응답이면 0, 연결 자체가 안 되면 KANVIBE_CONNECTION_FAILED, 그 밖의 실패는 1을 돌려준다.
# 토큰은 명령줄 대신 표준 입력으로 curl에 넘겨 ps에 드러나지 않게 한다.
send_json_request() {
  local url=$1 token=$2 path=$3 body=$4 response curl_status connect_seconds
  shift 4
  response="$(printf 'Authorization: Bearer %s\\n' "\${token}" | curl -sS \\
    --connect-timeout "\${KANVIBE_CONNECT_TIMEOUT_SECONDS}" \\
    --max-time "\${KANVIBE_REQUEST_TIMEOUT_SECONDS}" \\
    "$@" -X POST --url "\${url}\${path}" \\
    -H @- \\
    -H 'Content-Type: application/json' \\
    --data-binary "\${body}" \\
    -w '\\n%{time_connect}\\n%{http_code}' 2>/dev/null)"
  curl_status=$?
  RESPONSE_STATUS="\${response##*$'\\n'}"
  response="\${response%$'\\n'*}"
  connect_seconds="\${response##*$'\\n'}"
  RESPONSE_BODY="\${response%$'\\n'*}"

  # 2는 curl이 --unix-socket을 모르는 옛 버전일 때의 종료 코드로, 요청이 나가지 않았다
  if [ "\${curl_status}" -eq 2 ] || [ "\${curl_status}" -eq 6 ] || [ "\${curl_status}" -eq 7 ] || is_connect_timeout "\${curl_status}" "\${connect_seconds}"; then
    RESPONSE_STATUS="000"
    RESPONSE_BODY="연결할 수 없습니다"
    return "\${KANVIBE_CONNECTION_FAILED}"
  fi

  if [ "\${curl_status}" -ne 0 ]; then
    RESPONSE_STATUS="000"
    RESPONSE_BODY="요청이 실패했습니다(curl \${curl_status}). 응답을 받지 못했으므로 다른 client로 다시 보내지 않습니다."
    return 1
  fi

  case "\${RESPONSE_STATUS}" in
    2*) return 0 ;;
    *) return 1 ;;
  esac
}

# 생성·삭제는 client 한 곳에서만 실행되어야 하므로, 연결이 안 된 경우에만 다음 client로 넘어간다
send_to_one_target() {
  local path=$1 fields=$2 target_url=$3
  local url task_id client_id token transport namespace request_status
  select_target_rows "\${target_url}"

  while IFS=$'\\t' read -r url task_id client_id token transport namespace; do
    if [ -z "\${token}" ] || [ "\${token}" = "_" ]; then
      printf 'kanvibe task: %s 의 CLI 토큰이 없습니다. KanVibe에서 hooks를 다시 설치하세요.\\n' "\${url}" >&2
      continue
    fi

    post_json "\${url}" "\${token}" "\${path}" "{\\"contextTaskId\\":$(json_string "\${task_id}")\${fields}}" "\${client_id}" "\${transport}" "\${namespace}"
    request_status=$?

    if [ "\${request_status}" -eq "\${KANVIBE_CONNECTION_FAILED}" ]; then
      printf 'kanvibe task: %s 에 연결할 수 없어 다음 client를 시도합니다.\\n' "\${url}" >&2
      continue
    fi

    if [ "\${request_status}" -eq 0 ]; then
      printf '%s\\n' "\${RESPONSE_BODY}"
      return 0
    fi

    printf 'kanvibe task: %s 요청 실패 (HTTP %s)\\n%s\\n' "\${url}" "\${RESPONSE_STATUS}" "\${RESPONSE_BODY}" >&2
    return 1
  done <<< "\${SELECTED_TARGET_ROWS}"

  fail "요청을 받을 수 있는 KanVibe client가 없습니다."
}

# KanVibe client가 다른 client에 생성 등록·삭제를 전파할 때 쓰는 내부 명령. 결과를 client마다 한 줄씩 남긴다
propagate_to_targets() {
  local path=$1 fields=$2 exclude_url="\${3%/}"
  local url task_id client_id token transport namespace message

  while IFS=$'\\t' read -r url task_id client_id token transport namespace; do
    if [ -z "\${url}" ] || [ "\${url}" = "\${exclude_url}" ]; then
      continue
    fi

    if [ -z "\${token}" ] || [ "\${token}" = "_" ]; then
      RESPONSE_STATUS="000"
      RESPONSE_BODY="CLI 토큰이 없습니다"
    else
      post_json "\${url}" "\${token}" "\${path}" "{\\"contextTaskId\\":$(json_string "\${task_id}")\${fields}}" "\${client_id}" "\${transport}" "\${namespace}"
    fi

    message="$(printf '%s' "\${RESPONSE_BODY}" | tr '\\t\\r\\n' '   ' | cut -c1-300)"
    printf '__KANVIBE_PEER_RESULT__\\t%s\\t%s\\t%s\\n' "\${url}" "\${RESPONSE_STATUS}" "\${message}"
  done <<< "$(read_target_rows)"
}

require_value() {
  if [ "$#" -lt 2 ] || [ -z "$2" ]; then
    fail "$1 에 값이 필요합니다." "\${KANVIBE_USAGE_ERROR}"
  fi
}

create_task() {
  local branch_name="" title="" from_task_id="" session_type="" target_url="" base_branch="" fields=""

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --branch) require_value "$1" "\${2:-}"; branch_name=$2; shift 2 ;;
      --title) require_value "$1" "\${2:-}"; title=$2; shift 2 ;;
      --from-task) require_value "$1" "\${2:-}"; from_task_id=$2; shift 2 ;;
      --session-type) require_value "$1" "\${2:-}"; session_type=$2; shift 2 ;;
      --target-url) require_value "$1" "\${2:-}"; target_url=$2; shift 2 ;;
      *) fail "알 수 없는 옵션입니다: $1" "\${KANVIBE_USAGE_ERROR}" ;;
    esac
  done

  if [ -z "\${branch_name}" ]; then
    fail "--branch는 필수입니다." "\${KANVIBE_USAGE_ERROR}"
  fi

  fields="$(json_field branchName "\${branch_name}")"
  if [ -n "\${title}" ]; then
    fields="\${fields}$(json_field title "\${title}")"
  fi
  if [ -n "\${session_type}" ]; then
    fields="\${fields}$(json_field sessionType "\${session_type}")"
  fi

  if [ -n "\${from_task_id}" ]; then
    fields="\${fields}$(json_field fromTaskId "\${from_task_id}")"
  else
    base_branch="$(git -C "\${KANVIBE_REPO_ROOT}" symbolic-ref --quiet --short HEAD 2>/dev/null)" \\
      || fail "현재 checkout이 브랜치가 아닙니다(detached HEAD). 브랜치를 checkout하거나 --from-task로 기준 task를 지정하세요." "\${KANVIBE_USAGE_ERROR}"
    fields="\${fields}$(json_field baseBranch "\${base_branch}")"
  fi

  send_to_one_target "/api/cli/tasks" "\${fields}" "\${target_url}"
}

delete_task() {
  local branch_name="" target_url="" is_confirmed=false is_forced=false fields=""

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --branch) require_value "$1" "\${2:-}"; branch_name=$2; shift 2 ;;
      --target-url) require_value "$1" "\${2:-}"; target_url=$2; shift 2 ;;
      --yes) is_confirmed=true; shift ;;
      --force) is_forced=true; shift ;;
      *) fail "알 수 없는 옵션입니다: $1" "\${KANVIBE_USAGE_ERROR}" ;;
    esac
  done

  if [ "\${is_confirmed}" != true ]; then
    fail "삭제하려면 --yes를 붙이세요. task와 worktree·브랜치·세션이 등록된 모든 KanVibe client에서 삭제됩니다." "\${KANVIBE_USAGE_ERROR}"
  fi

  fields=",\\"confirmed\\":true,\\"force\\":\${is_forced}"
  if [ -n "\${branch_name}" ]; then
    fields="\${fields}$(json_field branchName "\${branch_name}")"
  fi

  if ! send_to_one_target "/api/cli/tasks/delete" "\${fields}" "\${target_url}"; then
    return 1
  fi
  warn_unpropagated_peers
}

# 받은 client는 지웠지만 꺼져 있던 client에는 task가 남으므로, 응답의 peers에서 실패한 수를 세어 알린다
warn_unpropagated_peers() {
  local failed_peer_count
  failed_peer_count="$(printf '%s' "\${RESPONSE_BODY}" | grep -oE '"isSuccess"[[:space:]]*:[[:space:]]*false' | wc -l | tr -d ' ')"
  if [ "\${failed_peer_count}" -gt 0 ]; then
    printf 'kanvibe task: 다른 client %s곳에는 삭제를 전파하지 못했습니다. 그 client에는 task가 남아 있으니 출력의 peers를 확인하고 그곳에서 직접 지우세요.\\n' "\${failed_peer_count}" >&2
  fi
}

propagate_command() {
  local command_name=$1 exclude_url="" branch_name=""
  shift

  while [ "$#" -gt 0 ]; do
    case "$1" in
      --exclude-url) require_value "$1" "\${2:-}"; exclude_url=$2; shift 2 ;;
      --branch) require_value "$1" "\${2:-}"; branch_name=$2; shift 2 ;;
      *) fail "알 수 없는 옵션입니다: $1" "\${KANVIBE_USAGE_ERROR}" ;;
    esac
  done

  if [ "\${command_name}" = "propagate-delete" ]; then
    propagate_to_targets "/api/cli/tasks/delete" ',"confirmed":true,"force":true,"propagateToPeers":false' "\${exclude_url}"
    return 0
  fi

  if [ -z "\${branch_name}" ]; then
    fail "--branch는 필수입니다." "\${KANVIBE_USAGE_ERROR}"
  fi
  propagate_to_targets "/api/cli/tasks/register" "$(json_field branchName "\${branch_name}")" "\${exclude_url}"
}

case "\${1:-help}" in
  create) shift; create_task "$@" ;;
  delete) shift; delete_task "$@" ;;
  propagate-delete|propagate-register) propagate_command "$@" ;;
  help|-h|--help) print_usage ;;
  *) print_usage >&2; exit "\${KANVIBE_USAGE_ERROR}" ;;
esac
`;

export function getTaskCliPath(repoPath: string, sshHost?: string | null): string {
  const pathModule = sshHost ? path.posix : path;
  return pathModule.join(repoPath, KANVIBE_DIR_NAME, TASK_CLI_FILE_NAME);
}

/**
 * KanVibe client가 직접 실행할 task CLI 명령을 만든다.
 * worktree의 task.sh는 브랜치 checkout으로 덮어써질 수 있으므로 실행하지 않고, 앱이 가진 본문을 `bash -c`로 넘긴다.
 * `$0`에 task.sh 경로를 주면 파일로 실행할 때와 같은 위치의 targets.json을 읽는다.
 */
export function buildTaskCliCommand(repoPath: string, commandArguments: string[], sshHost?: string | null): string {
  const scriptArguments = [getTaskCliPath(repoPath, sshHost), ...commandArguments].map(quoteShellArgument).join(" ");
  return `bash -c ${quoteShellArgument(TASK_CLI_SCRIPT)} ${scriptArguments}`;
}

/** hook 설치 산출물과 같은 writer로 로컬·원격에 기록할 수 있도록 파일 정의로 돌려준다 */
export function buildTaskCliFile(repoPath: string, sshHost?: string | null): ShellHookProviderFile {
  return {
    filePath: getTaskCliPath(repoPath, sshHost),
    content: TASK_CLI_SCRIPT,
    mode: TASK_CLI_FILE_MODE,
  };
}
