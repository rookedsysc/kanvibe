import { SessionType } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";

const blockedHosts = new Map<string, string>();

export function getBlockedRemoteHostReason(sshHost: string): string | null {
  return blockedHosts.get(sshHost) ?? null;
}

export async function ensureRemoteSessionDependency(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<void> {
  if (!sshHost) {
    return;
  }

  const blockedReason = getBlockedRemoteHostReason(sshHost);
  if (blockedReason) {
    throw new Error(blockedReason);
  }

  const toolName = sessionType === SessionType.TMUX ? "tmux" : "zellij";

  try {
    await execGit(`command -v ${toolName} >/dev/null 2>&1`, sshHost);
    return;
  } catch {
    // 설치 시도로 진행한다.
  }

  try {
    await execGit(buildInstallCommand(toolName), sshHost);
    await execGit(`command -v ${toolName} >/dev/null 2>&1`, sshHost);
  } catch (error) {
    const reason = `${sshHost} 호스트에서 ${toolName} 설치를 완료하지 못해 원격 접근을 차단했습니다. ${error instanceof Error ? error.message : "원격 설치 실패"}`;
    blockedHosts.set(sshHost, reason);
    throw new Error(reason);
  }
}

function buildInstallCommand(toolName: string): string {
  const installWithPrivilege = toolName === "zellij"
    ? [
        "cargo install --locked zellij",
        "brew install zellij",
        "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y zellij",
        "dnf install -y zellij",
        "yum install -y zellij",
        "pacman -Sy --noconfirm zellij",
        "zypper --non-interactive install zellij",
        "apk add zellij",
      ]
    : [
        "brew install tmux",
        "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y tmux",
        "dnf install -y tmux",
        "yum install -y tmux",
        "pacman -Sy --noconfirm tmux",
        "zypper --non-interactive install tmux",
        "apk add tmux",
      ];

  const installBranches = installWithPrivilege.map((command) => buildPackageManagerBranch(command));

  return [
    `if command -v ${toolName} >/dev/null 2>&1; then exit 0; fi`,
    'run_install() {',
    '  if [ "$(id -u)" -eq 0 ]; then',
    '    sh -lc "$1"',
    '    return $? ',
    '  fi',
    '  if command -v sudo >/dev/null 2>&1; then',
    '    sudo -n sh -lc "$1"',
    '    return $? ',
    '  fi',
    '  return 1',
    '}',
    ...installBranches,
    `command -v ${toolName} >/dev/null 2>&1 || { echo "${toolName} 설치에 실패했습니다." >&2; exit 1; }`,
  ].join("; ");
}

function buildPackageManagerBranch(command: string): string {
  const executable = command.split(" ")[0];
  if (executable === "cargo") {
    return `if command -v cargo >/dev/null 2>&1; then ${command}; fi`;
  }

  if (executable === "brew") {
    return `if command -v brew >/dev/null 2>&1; then ${command}; fi`;
  }

  return `if command -v ${executable} >/dev/null 2>&1; then run_install '${command}' && exit 0; fi`;
}
