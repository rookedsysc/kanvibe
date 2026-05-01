import { execGit, isSSHTransportError } from "@/lib/gitOperations";

const blockedTargets = new Map<string, string>();

export interface GitHubCliStatus {
  toolName: "gh";
  sshHost: string | null;
  isRemote: boolean;
  available: boolean;
  blockedReason: string | null;
}

function buildBlockedTargetKey(sshHost?: string | null): string {
  return `${sshHost || "local"}:gh`;
}

async function hasGitHubCli(sshHost?: string | null): Promise<boolean> {
  try {
    await execGit("command -v gh >/dev/null 2>&1", sshHost);
    return true;
  } catch (error) {
    if (sshHost && isSSHTransportError(error)) {
      throw error;
    }

    return false;
  }
}

export async function getGitHubCliStatus(sshHost?: string | null): Promise<GitHubCliStatus> {
  const blockedReason = blockedTargets.get(buildBlockedTargetKey(sshHost)) ?? null;

  if (blockedReason) {
    return {
      toolName: "gh",
      sshHost: sshHost ?? null,
      isRemote: Boolean(sshHost),
      available: false,
      blockedReason,
    };
  }

  return {
    toolName: "gh",
    sshHost: sshHost ?? null,
    isRemote: Boolean(sshHost),
    available: await hasGitHubCli(sshHost),
    blockedReason: null,
  };
}

export async function installGitHubCli(sshHost?: string | null): Promise<void> {
  const blockedTargetKey = buildBlockedTargetKey(sshHost);
  blockedTargets.delete(blockedTargetKey);

  try {
    await execGit(buildInstallCommand(), sshHost);
    await execGit("command -v gh >/dev/null 2>&1", sshHost);
  } catch (error) {
    if (sshHost && isSSHTransportError(error)) {
      throw error;
    }

    const subject = sshHost ? `${sshHost} 호스트` : "로컬 환경";
    const reason = sshHost
      ? `${subject}에서 gh 설치를 완료하지 못해 원격 접근을 차단했습니다. ${error instanceof Error ? error.message : "원격 설치 실패"}`
      : `${subject}에서 gh 설치를 완료하지 못했습니다. ${error instanceof Error ? error.message : "설치 실패"}`;

    if (sshHost) {
      blockedTargets.set(blockedTargetKey, reason);
    }

    throw new Error(reason);
  }
}

function buildInstallCommand(): string {
  return [
    "if command -v gh >/dev/null 2>&1; then exit 0; fi",
    "run_install() {",
    '  if [ "$(id -u)" -eq 0 ]; then',
    '    sh -lc "$1"',
    "    return $?",
    "  fi",
    "  if command -v sudo >/dev/null 2>&1; then",
    '    sudo -n sh -lc "$1"',
    "    return $?",
    "  fi",
    "  return 1",
    "}",
    "if command -v brew >/dev/null 2>&1; then brew install gh && exit 0; fi",
    'if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then run_install "type -p wget >/dev/null || (apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y wget) && mkdir -p -m 755 /etc/apt/keyrings && out=\\$(mktemp) && wget -nv -O\\$out https://cli.github.com/packages/githubcli-archive-keyring.gpg && cat \\$out > /etc/apt/keyrings/githubcli-archive-keyring.gpg && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg && mkdir -p -m 755 /etc/apt/sources.list.d && echo \\"deb [arch=\\$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\\" > /etc/apt/sources.list.d/github-cli.list && apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y gh" && exit 0; fi',
    'if command -v dnf >/dev/null 2>&1; then run_install "dnf install -y dnf5-plugins || dnf install -y \\"dnf-command(config-manager)\\" || true; dnf config-manager addrepo --from-repofile=https://cli.github.com/packages/rpm/gh-cli.repo || dnf config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo; dnf install -y gh --repo gh-cli || dnf install -y gh" && exit 0; fi',
    'if command -v yum >/dev/null 2>&1; then run_install "type -p yum-config-manager >/dev/null || yum install -y yum-utils; yum-config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo && yum install -y gh" && exit 0; fi',
    'if command -v zypper >/dev/null 2>&1; then run_install "zypper --non-interactive addrepo https://cli.github.com/packages/rpm/gh-cli.repo || true; zypper --non-interactive ref && zypper --non-interactive install gh" && exit 0; fi',
    'if command -v pacman >/dev/null 2>&1; then run_install "pacman -Sy --noconfirm github-cli" && exit 0; fi',
    'if command -v apk >/dev/null 2>&1; then run_install "apk add github-cli" && exit 0; fi',
    'echo "gh 설치에 실패했습니다." >&2',
    "exit 1",
  ].join("\n");
}
