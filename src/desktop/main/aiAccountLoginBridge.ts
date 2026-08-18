import type { WebContents } from "electron";
import {
  createProviderCliEnvironment,
  getProviderLoginCommand,
} from "@/lib/aiUsage/providerCli";
import { AI_PROVIDER_CONFIG_DIR_SPECS } from "@/lib/aiUsage/providerConfigDir";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

/**
 * 계정 로그인은 태스크 터미널이 아니다.
 *
 * 태스크 세션은 작업 행에 묶여 수명주기가 다르므로, 로그인 세션은 자기 레지스트리를 따로 가진다.
 * 이렇게 두면 가짜 태스크를 만들지 않고도 CLI 로그인 화면을 앱 안에서 그대로 보여줄 수 있다.
 */
interface AiAccountLoginSession {
  pty: import("node-pty").IPty;
}

const loginSessions = new Map<string, AiAccountLoginSession>();

function buildSessionKey(webContentsId: number, accountRoot: string): string {
  return `${webContentsId}:${accountRoot}`;
}

export interface AiAccountLoginOpenResult {
  ok: boolean;
  error?: string;
}

export async function openAiAccountLogin(
  webContents: WebContents,
  provider: AiUsageProvider,
  accountRoot: string,
  cols: number,
  rows: number,
): Promise<AiAccountLoginOpenResult> {
  const sessionKey = buildSessionKey(webContents.id, accountRoot);
  if (loginSessions.has(sessionKey)) {
    return { ok: true };
  }

  const { command, args } = getProviderLoginCommand(provider);
  const environment = createProviderCliEnvironment(
    AI_PROVIDER_CONFIG_DIR_SPECS[provider],
    accountRoot,
  );

  let ptyProcess: import("node-pty").IPty;
  try {
    const pty = await import("node-pty");
    ptyProcess = pty.spawn(command, args, {
      name: "xterm-color",
      cols,
      rows,
      cwd: environment.HOME,
      env: environment,
    });
  } catch (error) {
    return {
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    };
  }

  loginSessions.set(sessionKey, { pty: ptyProcess });

  ptyProcess.onData((data) => {
    if (!webContents.isDestroyed()) {
      webContents.send("kanvibe:ai-login-data", { accountRoot, data });
    }
  });

  ptyProcess.onExit(({ exitCode }) => {
    loginSessions.delete(sessionKey);
    if (!webContents.isDestroyed()) {
      webContents.send("kanvibe:ai-login-exit", { accountRoot, exitCode });
    }
  });

  return { ok: true };
}

export function writeAiAccountLogin(
  webContentsId: number,
  accountRoot: string,
  data: string,
): void {
  loginSessions.get(buildSessionKey(webContentsId, accountRoot))?.pty.write(data);
}

export function resizeAiAccountLogin(
  webContentsId: number,
  accountRoot: string,
  cols: number,
  rows: number,
): void {
  loginSessions.get(buildSessionKey(webContentsId, accountRoot))?.pty.resize(cols, rows);
}

export function closeAiAccountLogin(webContentsId: number, accountRoot: string): void {
  const sessionKey = buildSessionKey(webContentsId, accountRoot);
  const session = loginSessions.get(sessionKey);
  if (!session) {
    return;
  }

  loginSessions.delete(sessionKey);
  session.pty.kill();
}

/** 창이 사라지면 그 창이 띄운 로그인 프로세스도 남겨 두지 않는다 */
export function closeWindowAiAccountLogins(webContentsId: number): void {
  for (const sessionKey of [...loginSessions.keys()]) {
    if (sessionKey.startsWith(`${webContentsId}:`)) {
      loginSessions.get(sessionKey)?.pty.kill();
      loginSessions.delete(sessionKey);
    }
  }
}
