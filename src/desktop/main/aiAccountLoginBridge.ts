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
  pty?: import("node-pty").IPty;
  ready: Promise<AiAccountLoginOpenResult>;
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
  const existing = loginSessions.get(sessionKey);
  if (existing) {
    return existing.ready;
  }

  const session: AiAccountLoginSession = { ready: Promise.resolve({ ok: false }) };
  loginSessions.set(sessionKey, session);
  session.ready = (async () => {
    try {
      const { command, args } = getProviderLoginCommand(provider);
      const environment = createProviderCliEnvironment(
        AI_PROVIDER_CONFIG_DIR_SPECS[provider],
        accountRoot,
      );
      const pty = await import("node-pty");
      if (loginSessions.get(sessionKey) !== session || webContents.isDestroyed()) {
        if (loginSessions.get(sessionKey) === session) {
          loginSessions.delete(sessionKey);
        }
        return { ok: false };
      }
      const ptyProcess = pty.spawn(command, args, {
        name: "xterm-color",
        cols,
        rows,
        cwd: environment.HOME,
        env: environment,
      });
      session.pty = ptyProcess;

      ptyProcess.onData((data) => {
        if (loginSessions.get(sessionKey) === session && !webContents.isDestroyed()) {
          webContents.send("kanvibe:ai-login-data", { accountRoot, data });
        }
      });
      ptyProcess.onExit(({ exitCode }) => {
        if (loginSessions.get(sessionKey) !== session) {
          return;
        }
        loginSessions.delete(sessionKey);
        if (!webContents.isDestroyed()) {
          webContents.send("kanvibe:ai-login-exit", { accountRoot, exitCode });
        }
      });
      return { ok: true };
    } catch (error) {
      if (loginSessions.get(sessionKey) === session) {
        loginSessions.delete(sessionKey);
      }
      try {
        session.pty?.kill();
      } catch (cleanupError) {
        console.warn("[ai-login] PTY cleanup failed:", cleanupError);
      }
      return {
        ok: false,
        error: error instanceof Error ? error.message : String(error),
      };
    }
  })();
  return session.ready;
}

export function writeAiAccountLogin(
  webContentsId: number,
  accountRoot: string,
  data: string,
): void {
  try {
    loginSessions.get(buildSessionKey(webContentsId, accountRoot))?.pty?.write(data);
  } catch (error) {
    console.warn("[ai-login] PTY write failed:", error);
  }
}

export function resizeAiAccountLogin(
  webContentsId: number,
  accountRoot: string,
  cols: number,
  rows: number,
): void {
  // The native fd can close before node-pty delivers onExit; queued IPC must not terminate main.
  try {
    loginSessions.get(buildSessionKey(webContentsId, accountRoot))?.pty?.resize(cols, rows);
  } catch (error) {
    console.warn("[ai-login] PTY resize failed:", error);
  }
}

export function closeAiAccountLogin(webContentsId: number, accountRoot: string): void {
  const sessionKey = buildSessionKey(webContentsId, accountRoot);
  const session = loginSessions.get(sessionKey);
  if (!session) {
    return;
  }

  loginSessions.delete(sessionKey);
  try {
    session.pty?.kill();
  } catch (error) {
    console.warn("[ai-login] PTY close failed:", error);
  }
}

/** 창이 사라지면 그 창이 띄운 로그인 프로세스도 남겨 두지 않는다 */
export function closeWindowAiAccountLogins(webContentsId: number): void {
  for (const sessionKey of [...loginSessions.keys()]) {
    if (sessionKey.startsWith(`${webContentsId}:`)) {
      closeAiAccountLogin(webContentsId, sessionKey.slice(`${webContentsId}:`.length));
    }
  }
}
