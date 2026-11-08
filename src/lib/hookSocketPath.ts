/**
 * 원격 호스트의 hook·CLI가 데스크탑으로 돌아오는 Unix 소켓 터널의 경로 규칙.
 *
 * 소켓 이름은 데스크탑 설치마다 한 번 만드는 client ID로 정한다. URL의 IP로 정하면 PC의 IP가 바뀐 뒤
 * targets.json에 남은 옛 주소로 소켓을 찾지 못해 토큰이 평문 HTTP로 나간다.
 * 데스크탑이 만드는 소켓과 원격 스크립트가 찾는 소켓의 이름이 같아야 하므로 두 쪽의 규칙을 이 파일에 함께 둔다.
 * NFS 공유 홈과 Unix 소켓 경로 길이 제한을 피하려고 홈이 아니라 /tmp 아래에 둔다.
 */
const HOOK_SOCKET_DIR_SHELL = '/tmp/kv-$(id -u)';
const HOOK_CLIENT_ID_PATTERN = /^[A-Za-z0-9-]+$/;

/** targets.json은 브랜치 checkout으로 바뀔 수 있으므로 경로 문자가 섞인 ID는 소켓 이름으로 쓰지 않는다 */
export function isValidHookClientId(clientId: unknown): clientId is string {
  return typeof clientId === "string" && HOOK_CLIENT_ID_PATTERN.test(clientId);
}

export function buildHookSocketFileName(clientId: string): string {
  if (!isValidHookClientId(clientId)) {
    throw new Error(`소켓 이름으로 쓸 수 없는 client ID입니다: ${clientId}`);
  }
  return `${clientId}.sock`;
}

/**
 * 이 소켓으로 task 토큰이 나가므로, 본인 소유이고 0700인 디렉터리 안의 본인 소유 소켓만 인정한다.
 * 셸 함수와 한 줄짜리 원격 명령이 같은 조건을 쓰도록 한곳에서 만든다.
 */
function buildShellTrustedSocketCondition(directoryVariable: string, socketVariable: string): string {
  const directory = `"\${${directoryVariable}}"`;
  const socket = `"\${${socketVariable}}"`;
  return [
    `[ ! -L ${directory} ]`,
    `[ -O ${directory} ]`,
    `[ "$(ls -ld ${directory} 2>/dev/null | cut -c1-10)" = "drwx------" ]`,
    `[ ! -L ${socket} ]`,
    `[ -S ${socket} ]`,
    `[ -O ${socket} ]`,
  ].join(" && ");
}

/**
 * 데스크탑이 원격에 보내는 한 줄짜리 명령(설치 스크립트 내려받기, 상태 확인)에서 쓰는 소켓 경로 대입.
 * 믿을 수 있는 소켓일 때만 경로를 담고, 아니면 빈 값이라 호출부가 HTTP로 넘어간다.
 */
export function buildShellOwnedHookSocketAssignment(variableName: string, clientId: string): string {
  const directoryVariable = `${variableName}_dir`;
  return [
    `${directoryVariable}="${HOOK_SOCKET_DIR_SHELL}"`,
    `${variableName}="\${${directoryVariable}}/${buildHookSocketFileName(clientId)}"`,
    `{ ${buildShellTrustedSocketCondition(directoryVariable, variableName)}; } || ${variableName}=`,
  ].join("; ");
}

/**
 * 터널 소켓을 쓸 준비를 하고 경로를 출력하는 원격 명령.
 * 이전 실행이 남긴 소켓이 있으면 sshd가 bind를 거부하므로 먼저 지운다.
 * 다른 사용자가 미리 만든 디렉터리에는 소켓을 두지 않는다.
 */
export function buildRemoteHookSocketPrepareCommand(clientId: string): string {
  const fileName = buildHookSocketFileName(clientId);
  return [
    `kanvibe_socket_dir="${HOOK_SOCKET_DIR_SHELL}"`,
    'mkdir -p "${kanvibe_socket_dir}"',
    '[ ! -L "${kanvibe_socket_dir}" ]',
    '[ -O "${kanvibe_socket_dir}" ]',
    'chmod 700 "${kanvibe_socket_dir}"',
    `rm -f "\${kanvibe_socket_dir}/${fileName}"`,
    `printf '%s' "\${kanvibe_socket_dir}/${fileName}"`,
  ].join(" && ");
}

/**
 * targets.json 항목의 client ID로 터널 소켓 경로를 출력하는 POSIX 셸 함수.
 * ID가 없는 옛 항목이거나 쓸 수 없는 소켓이면 아무것도 출력하지 않아 호출부가 HTTP로 넘어간다.
 */
export function buildShellHookSocketResolver(): string {
  return `kanvibe_hook_socket() {
  case "$1" in
    ""|*[!A-Za-z0-9-]*) return 0 ;;
  esac
  kanvibe_socket_dir="${HOOK_SOCKET_DIR_SHELL}"
  kanvibe_socket_path="\${kanvibe_socket_dir}/$1.sock"
  if ${buildShellTrustedSocketCondition("kanvibe_socket_dir", "kanvibe_socket_path")}; then
    printf '%s' "\${kanvibe_socket_path}"
  fi
}`;
}
