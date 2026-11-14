import { createProviderCliEnvironment, getProviderLoginCommand } from "@/lib/aiUsage/providerCli";
import { AI_PROVIDER_CONFIG_DIR_SPECS } from "@/lib/aiUsage/providerConfigDir";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

// OAuth 화면을 남겨 둔 기기가 CLI를 계속 점유하지 않도록 로그인 한 번의 수명을 제한한다.
const LOGIN_TIMEOUT_MS = 10 * 60 * 1000;
const MAX_LOGINS_PER_DEVICE = 4;
const MAX_TOTAL_LOGINS = 12;
const MIN_TERMINAL_COLUMNS = 2;
const MAX_TERMINAL_COLUMNS = 500;
const MIN_TERMINAL_ROWS = 1;
const MAX_TERMINAL_ROWS = 250;
const MAX_INPUT_BYTES = 64 * 1024;

export type MobileAiLoginEvent =
  | { type: "output"; data: string }
  | { type: "exit"; exitCode: number }
  | { type: "error"; error: string };

export interface MobileAiLoginCallbacks {
  isOpen: () => boolean;
  onEvent: (event: MobileAiLoginEvent) => void;
}

interface LoginSession extends MobileAiLoginCallbacks {
  deviceId: string;
  socketId: string;
  accountRoot: string;
  pty?: import("node-pty").IPty;
  timer?: ReturnType<typeof setTimeout>;
  subscriptions: { dispose: () => void }[];
}

const loginSessions = new Map<string, LoginSession>();

class MobileAiLoginError extends Error {
  constructor(readonly statusCode: number, message: string) {
    super(message);
    this.name = "MobileAiLoginError";
  }
}

function sessionKey(deviceId: string, socketId: string): string {
  return JSON.stringify([deviceId, socketId]);
}

function validateTerminalSize(cols: number, rows: number): void {
  if (!Number.isInteger(cols) || cols < MIN_TERMINAL_COLUMNS || cols > MAX_TERMINAL_COLUMNS
    || !Number.isInteger(rows) || rows < MIN_TERMINAL_ROWS || rows > MAX_TERMINAL_ROWS) {
    throw new MobileAiLoginError(400, "터미널 크기가 올바르지 않습니다");
  }
}

function finishSession(key: string, session: LoginSession, shouldKill: boolean): void {
  if (loginSessions.get(key) !== session) return;
  loginSessions.delete(key);
  clearTimeout(session.timer);
  for (const subscription of session.subscriptions) subscription.dispose();
  if (shouldKill) {
    try { session.pty?.kill(); } catch {
      // 이미 종료된 PTY여도 다른 기기의 정리가 계속되어야 한다.
    }
  }
}

export async function openMobileAiLoginSession(options: MobileAiLoginCallbacks & {
  deviceId: string;
  socketId: string;
  provider: AiUsageProvider;
  accountRoot: string;
  cols: number;
  rows: number;
}): Promise<void> {
  validateTerminalSize(options.cols, options.rows);
  const key = sessionKey(options.deviceId, options.socketId);
  const sessions = [...loginSessions.values()];
  if (loginSessions.has(key) || sessions.some((session) => session.accountRoot === options.accountRoot)) {
    throw new MobileAiLoginError(409, "이 계정의 로그인 터미널이 이미 열려 있습니다");
  }
  if (sessions.length >= MAX_TOTAL_LOGINS
    || sessions.filter((session) => session.deviceId === options.deviceId).length >= MAX_LOGINS_PER_DEVICE) {
    throw new MobileAiLoginError(429, "열려 있는 로그인 터미널을 먼저 닫아 주세요");
  }
  if (!options.isOpen()) return;
  const session: LoginSession = { ...options, subscriptions: [] };
  loginSessions.set(key, session);
  session.timer = setTimeout(() => {
    if (loginSessions.get(key) !== session) return;
    finishSession(key, session, true);
    if (session.isOpen()) session.onEvent({ type: "error", error: "로그인 시간이 만료되었습니다. 다시 열어 주세요" });
  }, LOGIN_TIMEOUT_MS);
  session.timer.unref();

  try {
    const { command, args } = getProviderLoginCommand(options.provider);
    // 휴대폰 브라우저는 PC의 loopback OAuth callback에 도달하지 못하므로 Codex는 공식 기기 인증을 쓴다.
    // https://learn.chatgpt.com/docs/auth?surface=cli#login-on-headless-devices
    const loginArgs = options.provider === "codex" ? [...args, "--device-auth"] : args;
    const environment = createProviderCliEnvironment(AI_PROVIDER_CONFIG_DIR_SPECS[options.provider], options.accountRoot);
    const pty = await import("node-pty");
    if (loginSessions.get(key) !== session || !session.isOpen()) {
      finishSession(key, session, false);
      return;
    }
    session.pty = pty.spawn(command, loginArgs, { name: "xterm-color", cols: options.cols, rows: options.rows, cwd: environment.HOME, env: environment });
    session.subscriptions.push(session.pty.onData((data) => {
      if (loginSessions.get(key) === session && session.isOpen()) session.onEvent({ type: "output", data });
    }));
    session.subscriptions.push(session.pty.onExit(({ exitCode }) => {
      if (loginSessions.get(key) !== session) return;
      finishSession(key, session, false);
      if (session.isOpen()) session.onEvent({ type: "exit", exitCode });
    }));
  } catch {
    finishSession(key, session, true);
    throw new MobileAiLoginError(500, "로그인 터미널을 열지 못했습니다. PC의 CLI 설치를 확인해 주세요");
  }
}

export function writeMobileAiLogin(deviceId: string, socketId: string, data: string): void {
  if (typeof data !== "string" || Buffer.byteLength(data) > MAX_INPUT_BYTES) {
    throw new MobileAiLoginError(400, "터미널 입력이 올바르지 않습니다");
  }
  loginSessions.get(sessionKey(deviceId, socketId))?.pty?.write(data);
}

export function resizeMobileAiLogin(deviceId: string, socketId: string, cols: number, rows: number): void {
  validateTerminalSize(cols, rows);
  loginSessions.get(sessionKey(deviceId, socketId))?.pty?.resize(cols, rows);
}

export function closeMobileAiLogin(deviceId: string, socketId: string): void {
  const key = sessionKey(deviceId, socketId);
  const session = loginSessions.get(key);
  if (session) finishSession(key, session, true);
}

export function closeMobileAiLoginsForDevice(deviceId: string): void {
  for (const [key, session] of loginSessions) {
    if (session.deviceId === deviceId) finishSession(key, session, true);
  }
}

export function closeMobileAiLoginsForAccount(accountRoot: string): void {
  for (const [key, session] of loginSessions) {
    if (session.accountRoot === accountRoot) {
      finishSession(key, session, true);
      if (session.isOpen()) session.onEvent({ type: "error", error: "계정 삭제를 위해 로그인 터미널을 닫았습니다" });
    }
  }
}

export function closeAllMobileAiLogins(): void {
  for (const [key, session] of loginSessions) finishSession(key, session, true);
}
