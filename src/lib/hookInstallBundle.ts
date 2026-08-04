import { randomBytes } from "node:crypto";
import path from "node:path";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import type { ShellHookProviderFile } from "@/lib/shellHookProvider";

/** 설치 스크립트를 내려주는 Hook 서버 경로 */
export const HOOK_INSTALL_SCRIPT_PATH = "/api/install/hooks.sh";

/** 설치 스크립트가 모든 파일을 기록한 뒤 마지막 줄에 남기는 표시. 원격 설치 성공 판정에 쓴다 */
export const HOOK_INSTALL_SUCCESS_MARKER = "__KANVIBE_HOOK_INSTALL_OK__";

/**
 * 발급한 설치 토큰의 유효 시간.
 * 설치 실패 시 재시도가 같은 토큰을 다시 쓸 수 있을 만큼 길고, LAN에 오래 남지 않을 만큼 짧게 잡는다.
 */
const HOOK_INSTALL_TOKEN_TTL_MS = 5 * 60_000;

interface PendingHookInstall {
  script: string;
  expiresAt: number;
}

const pendingHookInstalls = new Map<string, PendingHookInstall>();

/**
 * 원격이 내려받아 실행할 설치 스크립트를 등록하고 조회 토큰을 돌려준다.
 * Hook 서버는 LAN에 열려 있으므로 토큰 없이는 설치 산출물을 내주지 않는다.
 */
export function issueHookInstallScript(files: ShellHookProviderFile[]): string {
  purgeExpiredHookInstalls();

  const token = randomBytes(24).toString("hex");
  pendingHookInstalls.set(token, {
    script: buildHookInstallScript(files),
    expiresAt: Date.now() + HOOK_INSTALL_TOKEN_TTL_MS,
  });

  return token;
}

/** 토큰에 해당하는 설치 스크립트를 반환한다. 없거나 만료됐으면 null */
export function readHookInstallScript(token: string | null | undefined): string | null {
  if (!token) {
    return null;
  }

  purgeExpiredHookInstalls();
  return pendingHookInstalls.get(token)?.script ?? null;
}

/** 테스트와 앱 종료 경로에서 대기 중인 설치 토큰을 모두 지운다 */
export function clearHookInstallScripts(): void {
  pendingHookInstalls.clear();
}

function purgeExpiredHookInstalls(): void {
  const now = Date.now();

  for (const [token, pendingInstall] of pendingHookInstalls) {
    if (pendingInstall.expiresAt <= now) {
      pendingHookInstalls.delete(token);
    }
  }
}

/**
 * hook 산출물을 원격에서 그대로 실행할 수 있는 POSIX sh 스크립트로 만든다.
 * 파일 본문은 base64로 실어 개행이나 따옴표가 셸 해석에 걸리지 않게 한다.
 */
export function buildHookInstallScript(files: ShellHookProviderFile[]): string {
  const fileCommands = files.map(({ filePath, content, mode }) => {
    const encodedContent = Buffer.from(content, "utf-8").toString("base64");
    const quotedFilePath = quoteShellArgument(filePath);
    const commands = [
      `mkdir -p ${quoteShellArgument(path.posix.dirname(filePath))}`,
      `printf '%s' ${quoteShellArgument(encodedContent)} | (base64 -d 2>/dev/null || base64 -D) > ${quotedFilePath}`,
    ];

    if (mode) {
      commands.push(`chmod ${mode.toString(8)} ${quotedFilePath}`);
    }

    return commands.join(" && ");
  });

  return [
    "set -e",
    ...fileCommands,
    `printf '%s\\n' ${quoteShellArgument(HOOK_INSTALL_SUCCESS_MARKER)}`,
    "",
  ].join("\n");
}

/**
 * 원격이 설치 스크립트를 내려받아 실행하는 한 줄짜리 명령을 만든다.
 * 스크립트 본문은 HTTP로 오가므로 SSH에는 짧은 명령만 실린다.
 */
export function buildHookInstallBootstrapCommand(hookServerUrl: string, token: string): string {
  const scriptUrl = quoteShellArgument(
    `${hookServerUrl.replace(/\/+$/, "")}${HOOK_INSTALL_SCRIPT_PATH}?token=${token}`,
  );

  /** 내려받기가 중간에 끊긴 스크립트를 실행하지 않도록, 전부 받은 뒤에 sh로 넘긴다 */
  return [
    `__kanvibe_install_script=$(curl -fsSL ${scriptUrl} 2>/dev/null || wget -qO- ${scriptUrl} 2>/dev/null)`,
    `printf '%s\\n' "$__kanvibe_install_script" | sh`,
  ].join(" && ");
}
