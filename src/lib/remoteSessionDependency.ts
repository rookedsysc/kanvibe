import { SessionType } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";

const blockedTargets = new Map<string, string>();

export interface SessionDependencyStatus {
  sessionType: SessionType;
  toolName: "tmux" | "zellij";
  sshHost: string | null;
  isRemote: boolean;
  available: boolean;
  blockedReason: string | null;
}

function getToolName(sessionType: SessionType): "tmux" | "zellij" {
  return sessionType === SessionType.TMUX ? "tmux" : "zellij";
}

function buildBlockedTargetKey(toolName: string, sshHost?: string | null): string {
  return `${sshHost || "local"}:${toolName}`;
}

export function getBlockedRemoteHostReason(sshHost: string): string | null {
  return blockedTargets.get(buildBlockedTargetKey("tmux", sshHost))
    ?? blockedTargets.get(buildBlockedTargetKey("zellij", sshHost))
    ?? null;
}

async function hasSessionDependency(toolName: string, sshHost?: string | null): Promise<boolean> {
  try {
    await execGit(`command -v ${toolName} >/dev/null 2>&1`, sshHost);
    return true;
  } catch {
    return false;
  }
}

export async function getSessionDependencyStatus(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<SessionDependencyStatus> {
  const toolName = getToolName(sessionType);
  const blockedReason = sshHost
    ? blockedTargets.get(buildBlockedTargetKey(toolName, sshHost)) ?? null
    : null;

  if (blockedReason) {
    return {
      sessionType,
      toolName,
      sshHost: sshHost ?? null,
      isRemote: Boolean(sshHost),
      available: false,
      blockedReason,
    };
  }

  return {
    sessionType,
    toolName,
    sshHost: sshHost ?? null,
    isRemote: Boolean(sshHost),
    available: await hasSessionDependency(toolName, sshHost),
    blockedReason: null,
  };
}

export async function installSessionDependency(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<void> {
  const toolName = getToolName(sessionType);
  const blockedTargetKey = buildBlockedTargetKey(toolName, sshHost);
  blockedTargets.delete(blockedTargetKey);

  try {
    await execGit(buildInstallCommand(toolName), sshHost);
    await execGit(`command -v ${toolName} >/dev/null 2>&1`, sshHost);
  } catch (error) {
    const subject = sshHost ? `${sshHost} 호스트` : "로컬 환경";
    const reason = sshHost
      ? `${subject}에서 ${toolName} 설치를 완료하지 못해 원격 접근을 차단했습니다. ${error instanceof Error ? error.message : "원격 설치 실패"}`
      : `${subject}에서 ${toolName} 설치를 완료하지 못했습니다. ${error instanceof Error ? error.message : "설치 실패"}`;
    if (sshHost) {
      blockedTargets.set(blockedTargetKey, reason);
    }
    throw new Error(reason);
  }
}

export async function ensureSessionDependency(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<void> {
  const status = await getSessionDependencyStatus(sessionType, sshHost);
  if (status.available) {
    return;
  }

  if (status.blockedReason) {
    throw new Error(status.blockedReason);
  }

  await installSessionDependency(sessionType, sshHost);
}

export async function ensureRemoteSessionDependency(
  sessionType: SessionType,
  sshHost?: string | null,
): Promise<void> {
  if (!sshHost) {
    return;
  }

  await ensureSessionDependency(sessionType, sshHost);
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
    `if ! command -v ${toolName} >/dev/null 2>&1; then`,
    `  echo "${toolName} 설치에 실패했습니다." >&2`,
    '  exit 1',
    'fi',
  ].join("\n");
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
