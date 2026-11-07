/**
 * 원격 호스트의 hook·CLI가 데스크탑으로 돌아오는 Unix 소켓 터널의 경로 규칙.
 *
 * targets.json의 형식을 바꾸면 옛 스크립트가 항목을 놓치므로, 소켓 위치는 client URL에서 계산한다.
 * 데스크탑이 만드는 소켓과 원격 스크립트가 찾는 소켓의 이름이 같아야 하므로 두 쪽의 규칙을 이 파일에 함께 둔다.
 * NFS 공유 홈과 Unix 소켓 경로 길이 제한을 피하려고 홈이 아니라 /tmp 아래에 둔다.
 */
const HOOK_SOCKET_DIR_SHELL = '/tmp/kv-$(id -u)';

export function buildHookSocketFileName(hookServerUrl: string): string {
  const address = hookServerUrl.replace(/^[a-zA-Z]*:\/\//, "").replace(/\/+$/, "");
  return `${address.replace(/[^A-Za-z0-9.-]/g, "_")}.sock`;
}

/**
 * 내용을 체크섬으로 확인하는 설치 스크립트 내려받기처럼 명령 길이가 중요한 곳에서 쓰는 짧은 점검.
 * 본인 소유의 소켓일 때만 경로를 담고, 아니면 빈 값이라 호출부가 HTTP로 넘어간다.
 */
export function buildShellOwnedHookSocketAssignment(variableName: string, hookServerUrl: string): string {
  const socketPath = `${HOOK_SOCKET_DIR_SHELL}/${buildHookSocketFileName(hookServerUrl)}`;
  return `${variableName}=${socketPath}; { [ -S "$${variableName}" ] && [ -O "$${variableName}" ]; } || ${variableName}=`;
}

/**
 * 터널 소켓을 쓸 준비를 하고 경로를 출력하는 원격 명령.
 * 이전 실행이 남긴 소켓이 있으면 sshd가 bind를 거부하므로 먼저 지운다.
 * 다른 사용자가 미리 만든 디렉터리에는 소켓을 두지 않는다.
 */
export function buildRemoteHookSocketPrepareCommand(hookServerUrl: string): string {
  const fileName = buildHookSocketFileName(hookServerUrl);
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
 * client URL의 터널 소켓 경로를 출력하는 POSIX 셸 함수. 쓸 수 없으면 아무것도 출력하지 않아 호출부가 HTTP로 넘어간다.
 * 이 소켓으로 task 토큰이 나가므로, 본인 소유이고 0700인 디렉터리 안의 본인 소유 소켓만 인정한다.
 */
export function buildShellHookSocketResolver(): string {
  return `kanvibe_hook_socket() {
  kanvibe_socket_dir="${HOOK_SOCKET_DIR_SHELL}"
  kanvibe_socket_path="\${kanvibe_socket_dir}/$(printf '%s' "$1" | sed -e 's#^[a-zA-Z]*://##' -e 's#/*$##' -e 's#[^A-Za-z0-9.-]#_#g').sock"
  if [ ! -L "\${kanvibe_socket_dir}" ] && [ -O "\${kanvibe_socket_dir}" ] \\
    && [ "$(ls -ld "\${kanvibe_socket_dir}" 2>/dev/null | cut -c1-10)" = "drwx------" ] \\
    && [ ! -L "\${kanvibe_socket_path}" ] && [ -S "\${kanvibe_socket_path}" ] && [ -O "\${kanvibe_socket_path}" ]; then
    printf '%s' "\${kanvibe_socket_path}"
  fi
}`;
}
