import { exec, spawn } from "child_process";
import { promisify } from "util";
import { homedir } from "os";
import { AsyncLocalStorage } from "node:async_hooks";
import {
  buildSSHArgs,
  ensureKanvibeSSHControlDirectory,
  getKanvibeSSHConnectionHealthOptions,
  getKanvibeSSHConnectionReuseOptions,
  parseSSHConfig,
  type SSHHostConfig,
} from "@/lib/sshConfig";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";

const execAsync = promisify(exec);

const SSH_TRANSPORT_ERROR_PATTERNS = [
  /kex_exchange_identification/i,
  /connection reset by peer/i,
  /connection reset by /i,
  /connection closed by /i,
  /connection refused/i,
  /connection timed out/i,
  /operation timed out/i,
  /no route to host/i,
  /network is unreachable/i,
  /could not resolve hostname/i,
  /name or service not known/i,
  /temporary failure in name resolution/i,
  /session open refused by peer/i,
  /ssh: connect to host/i,
];

const SSH_PERMANENT_CONNECTION_ERROR_PATTERNS = [
  /host key verification failed/i,
  /remote host identification has changed/i,
  /permission denied/i,
  /too many authentication failures/i,
  /bad owner or permissions/i,
];

const DEFAULT_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS = 5_000;
const MAX_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS = 60_000;
// Leave two channels below OpenSSH's default MaxSessions=10 for nested lock callbacks.
const REMOTE_SSH_HOST_MAX_CHANNELS = 8;
// Revalidate before the 10m ControlPersist idle window can expire.
const REMOTE_SSH_MASTER_READY_TTL_MS = 9 * 60_000;
const DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS = 45_000;
const REMOTE_SSH_COMMAND_MAX_ATTEMPTS = 3;
const REMOTE_COMMAND_EXIT_MARKER = "__KANVIBE_REMOTE_COMMAND_EXIT_7b3f6e5d__";
const REMOTE_COMMAND_MAX_BUFFER_BYTES = 10 * 1024 * 1024;
const remoteSSHTransportFailures = new Map<string, { until: number; error: Error }>();
// Command slots and authenticated master readiness have separate lifetimes.
const remoteSSHConnectionPools = new Map<string, RemoteSSHConnectionPool>();
const remoteSSHResponderContext = new AsyncLocalStorage<{ sshHost: string; active: boolean }>();
const remoteSSHMasters = new Map<string, RemoteSSHMaster>();
const remoteSSHAuthenticationQueues = new Map<string, Promise<void>>();
let nextSSHMasterGeneration = 0;

interface RemoteSSHMaster {
  generation: number;
  readyAt: number;
  failed: boolean;
  preparing: Promise<void>;
}

interface RemoteSSHConnectionPool {
  activeConnections: number;
  queue: Array<() => void>;
}

interface RemoteSSHConnectionLease {
  release: () => void;
}

export interface ExecGitOptions {
  timeoutMs?: number;
  maxBufferBytes?: number;
  strictHostKeyChecking?: boolean;
  /** Only opt in when repeating this command cannot duplicate a remote side effect. */
  retrySafe?: boolean;
  /** 원격 명령의 표준 입력으로 보낼 내용. 자격 증명처럼 프로세스 인자에 남으면 안 되는 값을 싣는다. */
  remoteStdin?: string;
  /** false면 원격 명령 실패를 오류 로그로 남기지 않는다. 세션이 꺼져 있으면 실패하는 탭 목록 조회처럼 실패가 예상된 폴링에 쓴다 */
  shouldLogFailure?: boolean;
  /** 원격 셸이 marker와 tab으로 시작하는 요청 한 줄을 보내면 한 번 응답한다. callback은 재실행하지 않는다. */
  remoteStdinResponder?: {
    marker: string;
    respond: (payload: string) => Promise<string>;
  };
}

function collectErrorOutput(error: unknown): string {
  const outputs: string[] = [];

  if (error instanceof Error) {
    outputs.push(error.message);
  }

  if (error && typeof error === "object") {
    if ("stderr" in error) {
      outputs.push(String((error as { stderr?: string }).stderr || ""));
    }

    if ("stdout" in error) {
      outputs.push(String((error as { stdout?: string }).stdout || ""));
    }
  }

  return outputs.filter(Boolean).join("\n");
}

export function isSSHTransportError(error: unknown): boolean {
  const errorOutput = collectErrorOutput(error);
  if (hasRemoteCommandCompleted(error) || SSH_PERMANENT_CONNECTION_ERROR_PATTERNS.some((pattern) => pattern.test(errorOutput))) return false;
  return SSH_TRANSPORT_ERROR_PATTERNS.some((pattern) => pattern.test(errorOutput));
}

export function isSSHConnectionError(error: unknown): boolean {
  return !hasRemoteCommandCompleted(error) && (isSSHCommandTimeoutError(error)
    || /ssh 명령이 .*완료되지 않았/i.test(collectErrorOutput(error)) || isSSHTransportError(error)
    || SSH_PERMANENT_CONNECTION_ERROR_PATTERNS.some((pattern) => pattern.test(collectErrorOutput(error))));
}

function hasRemoteCommandCompleted(error: unknown): boolean {
  return Boolean(error && typeof error === "object" && "remoteCommandCompleted" in error && error.remoteCommandCompleted);
}

function summarizeCommandFailure(errorOutput: string): string {
  const lines = errorOutput
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  return lines.at(-1) ?? "원격 명령 실행에 실패했습니다.";
}

function shouldLogRemoteCommandFailure(command: string, stderr: string): boolean {
  const trimmedCommand = command.trim();
  const isQuietProbe = command.includes("2>/dev/null")
    || command.includes(">/dev/null 2>&1")
    || (/^test -[ef] /.test(trimmedCommand) && trimmedCommand.includes("|| true"));

  if (isQuietProbe && !stderr.trim()) {
    return false;
  }

  if (isQuietProbe) {
    return false;
  }

  return true;
}

function readPositiveInteger(value: string | undefined, fallback: number): number {
  const parsed = Number.parseInt(value ?? "", 10);
  if (!Number.isFinite(parsed) || parsed < 1) {
    return fallback;
  }

  return parsed;
}

function getRemoteSSHHostMaxConcurrency(): number {
  return Math.min(
    readPositiveInteger(process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY, REMOTE_SSH_HOST_MAX_CHANNELS),
    REMOTE_SSH_HOST_MAX_CHANNELS,
  );
}

function getRemoteSSHCommandTimeoutMs(options?: ExecGitOptions): number {
  return options?.timeoutMs
    ?? readPositiveInteger(
      process.env.KANVIBE_REMOTE_SSH_COMMAND_TIMEOUT_MS,
      DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS,
    );
}

function getRemoteSSHTransportFailureCooldownMs(): number {
  return Math.min(
    readPositiveInteger(
      process.env.KANVIBE_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS,
      DEFAULT_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS,
    ),
    MAX_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS,
  );
}

function isSSHCommandTimeoutError(error: unknown): boolean {
  if (!error || typeof error !== "object") {
    return false;
  }

  const maybeTimeoutError = error as {
    code?: string;
    killed?: boolean;
    signal?: string | null;
  };

  return maybeTimeoutError.killed === true
    || maybeTimeoutError.signal === "SIGTERM"
    || maybeTimeoutError.code === "ETIMEDOUT";
}

/** 로컬에서 셸 명령을 실행하고 stdout을 반환한다 */
async function execLocal(command: string, options?: ExecGitOptions): Promise<string> {
  const { stdout } = await execAsync(command, {
    env: createLocalShellEnvironment(),
    ...(options?.timeoutMs ? { timeout: options.timeoutMs } : {}),
  });
  return stdout.trim();
}

/** SSH를 통해 원격에서 명령을 실행하고 stdout을 반환한다 */
async function execRemote(
  sshHost: string,
  command: string,
  options?: ExecGitOptions,
): Promise<string> {
  const execute = async () => {
    throwIfSSHTransportCooldownActive(sshHost);
    const timeoutMs = getRemoteSSHCommandTimeoutMs(options);
    const deadline = Date.now() + timeoutMs;
    const configs = await parseSSHConfig();
    const hostConfig = configs.find((c) => c.host === sshHost);

    if (!hostConfig) {
      throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
    }
    const independent = process.platform === "win32" || options?.strictHostKeyChecking;
    const masterKey = JSON.stringify(hostConfig);
    for (let attempt = 1; ; attempt += 1) {
      let master: RemoteSSHMaster | undefined;
      let sshArgs: string[] = [];
      let waitingForMaster = false;
      const independentGeneration = remoteSSHMasters.get(masterKey);
      const run = async (onAuthenticated?: () => void) => {
        throwIfSSHTransportCooldownActive(sshHost);
        if (!independent) {
          master = ensureRemoteSSHMaster(hostConfig, masterKey);
          waitingForMaster = true;
          await waitForSSHPreparation(master.preparing, deadline);
          waitingForMaster = false;
          master.readyAt = Date.now();
        }
        sshArgs = [
          ...buildExecSSHArgs(hostConfig, master, options),
          buildRemoteShellCommand(command),
        ];
        const context = { sshHost, active: true };
        return spawnSSHCommand(
          sshArgs,
          remainingSSHTimeout(deadline),
          options?.maxBufferBytes ?? REMOTE_COMMAND_MAX_BUFFER_BYTES,
          options?.remoteStdin,
          options?.remoteStdinResponder && {
            ...options.remoteStdinResponder,
            respond: (payload) => {
              onAuthenticated?.();
              return remoteSSHResponderContext.run(context, async () => {
                try {
                  return await options.remoteStdinResponder!.respond(payload);
                } finally {
                  context.active = false;
                }
              });
            },
          },
        ).finally(() => { context.active = false; });
      };
      try {
        return await (independent
          ? runSerializedSSHAuthentication(hostConfig, run, deadline)
          : run());
      } catch (error) {
        const timeout = isSSHCommandTimeoutError(error);
        const normalizedError = normalizeSSHExecError(error, sshHost, timeoutMs, timeout);
        const transient = isSSHTransportError(normalizedError);
        // Old command failures cannot invalidate a replacement master prepared by a newer request.
        const currentMaster = remoteSSHMasters.get(masterKey);
        const ownsCurrentGeneration = independent ? currentMaster === independentGeneration : currentMaster === master;
        if (master && ownsCurrentGeneration && !(waitingForMaster && error && typeof error === "object" && "preparationWaitTimeout" in error) && (transient || master.readyAt === 0)) remoteSSHMasters.delete(masterKey);
        const shouldRetry = options?.retrySafe && !options.remoteStdinResponder
          && !hasRemoteCommandCompleted(error) && (transient || timeout)
          && attempt < REMOTE_SSH_COMMAND_MAX_ATTEMPTS && Date.now() < deadline;
        if (shouldRetry) continue;
        if (options?.shouldLogFailure !== false) {
          logRemoteCommandFailure(command, sshHost, sshArgs, error);
        }
        if (transient && ownsCurrentGeneration) rememberSSHTransportFailure(sshHost, normalizedError);
        throw normalizedError;
      }
    }
  };
  const responderContext = remoteSSHResponderContext.getStore();
  if (responderContext?.active && responderContext.sshHost === sshHost) {
    return execute();
  }
  return runLimitedRemoteCommand(sshHost, execute);
}

function logRemoteCommandFailure(
  command: string,
  sshHost: string,
  sshArgs: string[],
  error: unknown,
): void {
  const stderr = error && typeof error === "object" && "stderr" in error
    ? String((error as { stderr?: string }).stderr || "")
    : "";

  if (!shouldLogRemoteCommandFailure(command, stderr)) {
    return;
  }

  console.error("[remote-ssh] command failed", {
    sshHost,
    command,
    sshArgs,
    error: error instanceof Error ? error.message : String(error),
  });
}


function spawnSSHProcess(sshArgs: string[], remoteStdin?: string, interactive = false) {
  const env = createLocalShellEnvironment();
  if (remoteStdin === undefined && !interactive) {
    return spawn("ssh", sshArgs, { env, stdio: ["ignore", "pipe", "pipe"] });
  }

  const child = spawn("ssh", sshArgs, { env, stdio: ["pipe", "pipe", "pipe"] });
  /** 원격이 입력을 다 읽기 전에 끝나면 EPIPE가 나지만, 실패 판정은 종료 코드가 맡는다 */
  child.stdin.on("error", () => undefined);
  if (!interactive) child.stdin.end(remoteStdin);
  return child;
}

function spawnSSHCommand(
  sshArgs: string[],
  timeoutMs: number,
  maxBufferBytes: number,
  remoteStdin?: string,
  responder?: ExecGitOptions["remoteStdinResponder"],
): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawnSSHProcess(sshArgs, remoteStdin, Boolean(responder));

    let stdoutChunks: string[] = [];
    const stderrChunks: string[] = [];
    let stdoutBytes = 0;
    let stderrBytes = 0;
    let stdoutLength = 0;
    let stdoutLastCharacter = "";
    let stderrLastCharacter = "";
    let readCompletion = createRemoteCommandCompletionParser();
    const responsePrefix = responder ? `${responder.marker}\t` : "";
    let responseSearchTail = "";
    let responseStart = -1;
    const readStdout = () => stdoutChunks.join("");
    const readStderr = () => stderrChunks.join("");
    let settled = false;
    let responded = false;
    let responseComplete = false;
    let responderError: Error | undefined;
    const timeout = setTimeout(() => {
      finishWithError(createSSHTimeoutError(timeoutMs, readStdout(), readStderr()));
    }, timeoutMs);

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");

    child.stdout.on("data", (chunk: string) => {
      if (settled) return;
      const previousLength = stdoutLength;
      stdoutChunks.push(chunk);
      stdoutBytes += getUTF8ChunkByteLength(chunk, stdoutLastCharacter);
      if (chunk) stdoutLastCharacter = chunk.slice(-1);
      stdoutLength += chunk.length;
      rejectIfOutputIsTooLarge();
      if (settled) return;
      let parsed: ReturnType<typeof readCompletion> = null;
      if (responder && !responded) {
        const search = responseSearchTail + chunk;
        const searchOffset = previousLength - responseSearchTail.length;
        if (responseStart < 0) {
          const found = search.indexOf(`\n${responsePrefix}`);
          if (searchOffset === 0 && search.startsWith(responsePrefix)) responseStart = 0;
          else if (found >= 0) responseStart = searchOffset + found + 1;
        }
        responseSearchTail = search.slice(-responsePrefix.length);
        const end = responseStart < 0 ? -1 : responsePrefix.includes("\n")
          ? readStdout().indexOf("\n", responseStart)
          : chunk.indexOf("\n", Math.max(0, responseStart - previousLength));
        if (responseStart >= 0 && end >= 0) {
          const stdout = readStdout();
          const responseEnd = responsePrefix.includes("\n") ? end : previousLength + end;
          responded = true;
          const payload = stdout.slice(responseStart + responsePrefix.length, responseEnd);
          const remaining = stdout.slice(0, responseStart) + stdout.slice(responseEnd + 1);
          stdoutChunks = [remaining];
          stdoutLength = remaining.length;
          stdoutBytes = Buffer.byteLength(remaining);
          stdoutLastCharacter = remaining.slice(-1);
          readCompletion = createRemoteCommandCompletionParser();
          parsed = readCompletion(remaining);
          void Promise.resolve().then(async () => {
            if (settled) return;
            const reply = await responder.respond(payload);
            if (!settled) {
              responseComplete = true;
              child.stdin?.end(reply);
            }
          }).catch((error: unknown) => {
            if (settled) return;
            responderError = error instanceof Error ? error : new Error(String(error));
            child.stdin?.end();
          });
        } else {
          parsed = readCompletion(chunk);
        }
      } else {
        parsed = readCompletion(chunk);
      }
      if (parsed) {
        finishWithRemoteCompletion(parsed.exitCode, readStdout().slice(0, parsed.markerIndex), readStderr());
      }
    });

    child.stderr.on("data", (chunk: string) => {
      if (settled) return;
      stderrChunks.push(chunk);
      stderrBytes += getUTF8ChunkByteLength(chunk, stderrLastCharacter);
      if (chunk) stderrLastCharacter = chunk.slice(-1);
      rejectIfOutputIsTooLarge();
    });

    child.on("error", (error) => {
      if (settled) return;
      finishWithError(attachCommandOutput(error, readStdout(), readStderr()));
    });

    child.on("close", (code, signal) => {
      if (settled) {
        return;
      }

      if (code === 0) {
        finishWithSuccess(readStdout().trim());
        return;
      }

      finishWithError(createSSHExitError(code, signal, readStdout(), readStderr()));
    });

    function rejectIfOutputIsTooLarge(): void {
      if (stdoutBytes + stderrBytes <= maxBufferBytes) {
        return;
      }

      finishWithError(attachCommandOutput(
        new Error("SSH command output exceeded maxBuffer"),
        readStdout(),
        readStderr(),
      ));
    }

    function finishWithRemoteCompletion(exitCode: number, completedStdout: string, completedStderr: string): void {
      if (responderError) {
        finishWithError(responderError);
        return;
      }
      if (exitCode === 0) {
        finishWithSuccess(completedStdout.trim());
        return;
      }

      const error = Object.assign(createSSHExitError(exitCode, null, completedStdout, completedStderr), { remoteCommandCompleted: true });
      finishWithError(error);
    }

    function finishWithSuccess(output: string): void {
      if (settled) {
        return;
      }
      if (responderError || (responded && !responseComplete)) {
        finishWithError(responderError ?? new Error("SSH transaction ended before its input response"));
        return;
      }

      settled = true;
      clearTimeout(timeout);
      if (responder && !responded) child.stdin?.end();
      if (child.exitCode === null && !child.killed) {
        child.kill("SIGTERM");
      }
      resolve(output);
    }

    function finishWithError(error: Error): void {
      if (settled) {
        return;
      }

      settled = true;
      clearTimeout(timeout);
      if (responder) child.stdin?.end();
      if (child.exitCode === null && !child.killed) {
        child.kill("SIGTERM");
      }
      reject(responderError ?? error);
    }
  });
}

function getUTF8ChunkByteLength(chunk: string, previousCharacter: string): number {
  const joinsSurrogatePair = /[\uD800-\uDBFF]/.test(previousCharacter)
    && /^[\uDC00-\uDFFF]/.test(chunk);
  return Buffer.byteLength(chunk) - (joinsSurrogatePair ? 2 : 0);
}

function createRemoteCommandCompletionParser(): (chunk: string) => { markerIndex: number; exitCode: number } | null {
  const prefix = `\n${REMOTE_COMMAND_EXIT_MARKER}:`;
  let tail = "";
  let length = 0;
  let markerIndex = -1;
  let digits: string[] = [];
  let digitCount = 0;
  let awaitingNewline = false;
  let invalid = false;

  return (chunk) => {
    const search = tail + chunk;
    const found = search.lastIndexOf(prefix);
    let suffix = chunk;
    if (found >= 0) {
      markerIndex = length - tail.length + found;
      digits = [];
      digitCount = 0;
      awaitingNewline = false;
      invalid = false;
      suffix = search.slice(found + prefix.length);
    }
    length += chunk.length;
    tail = search.slice(-(prefix.length - 1));
    if (markerIndex < 0 || invalid || !suffix) return null;
    if (!awaitingNewline) {
      const nextDigits = suffix.match(/^\d*/)?.[0] ?? "";
      if (nextDigits) {
        digits.push(nextDigits);
        digitCount += nextDigits.length;
      }
      suffix = suffix.slice(nextDigits.length);
      if (!suffix) return null;
      if (suffix[0] === "\r") {
        awaitingNewline = true;
        suffix = suffix.slice(1);
        if (!suffix) return null;
      }
    }
    if (digitCount && suffix[0] === "\n") {
      return { markerIndex, exitCode: Number.parseInt(digits.join(""), 10) };
    }
    invalid = true;
    return null;
  };
}

function createSSHTimeoutError(timeoutMs: number, stdout: string, stderr: string): Error {
  const error = new Error(`Command timed out after ${timeoutMs}ms`) as Error & {
    killed?: boolean;
    signal?: string;
  };
  error.killed = true;
  error.signal = "SIGTERM";
  return attachCommandOutput(error, stdout, stderr);
}

function createSSHExitError(
  code: number | null,
  signal: NodeJS.Signals | null,
  stdout: string,
  stderr: string,
): Error {
  const error = new Error(`Command failed: ssh exited with ${signal ?? code ?? "unknown"}`) as Error & {
    code?: number | null;
    signal?: NodeJS.Signals | null;
  };
  error.code = code;
  error.signal = signal;
  return attachCommandOutput(error, stdout, stderr);
}

function attachCommandOutput<T extends Error>(error: T, stdout: string, stderr: string): T {
  return Object.assign(error, { stdout, stderr });
}

async function runLimitedRemoteCommand<T>(
  sshHost: string,
  operation: () => Promise<T>,
): Promise<T> {
  const lease = await acquireRemoteSSHConnection(sshHost);

  try {
    return await operation();
  } finally {
    lease.release();
  }
}

function acquireRemoteSSHConnection(sshHost: string): Promise<RemoteSSHConnectionLease> {
  const connectionPool = getOrCreateRemoteSSHConnectionPool(sshHost);

  return new Promise((resolve) => {
    const start = () => {
      connectionPool.activeConnections += 1;
      resolve({
        release: () => releaseRemoteSSHConnection(sshHost, connectionPool),
      });
    };

    if (connectionPool.activeConnections < getRemoteSSHHostMaxConcurrency()) {
      start();
      return;
    }

    connectionPool.queue.push(start);
  });
}

function getOrCreateRemoteSSHConnectionPool(sshHost: string): RemoteSSHConnectionPool {
  let connectionPool = remoteSSHConnectionPools.get(sshHost);
  if (!connectionPool) {
    connectionPool = {
      activeConnections: 0,
      queue: [],
    };
    remoteSSHConnectionPools.set(sshHost, connectionPool);
  }

  return connectionPool;
}

function releaseRemoteSSHConnection(
  sshHost: string,
  connectionPool: RemoteSSHConnectionPool,
): void {
  connectionPool.activeConnections -= 1;

  const next = connectionPool.queue.shift();
  if (next) {
    next();
    return;
  }

  if (connectionPool.activeConnections === 0) {
    remoteSSHConnectionPools.delete(sshHost);
  }
}

function throwIfSSHTransportCooldownActive(sshHost: string): void {
  const failure = remoteSSHTransportFailures.get(sshHost);
  if (!failure) {
    return;
  }

  if (Date.now() >= failure.until) {
    remoteSSHTransportFailures.delete(sshHost);
    return;
  }

  throw Object.assign(new Error(
    `${sshHost} 원격 명령 실패: 최근 SSH transport 실패로 원격 명령을 잠시 건너뜁니다. 마지막 오류: ${failure.error.message}`,
  ), { retryAfterMs: Math.max(0, failure.until - Date.now()) });
}

/**
 * 사용자가 직접 원격 연결을 요청하면 백그라운드 실패로 걸린 쿨다운을 풀어 바로 다시 시도하게 한다.
 * 쿨다운은 백그라운드 명령이 끊긴 연결로 몰리는 것을 막으려는 장치라서, 사용자 요청까지 막으면 터미널이 열리지 않는다.
 * @param sshHost 쿨다운을 풀 원격 PC의 SSH 호스트 이름
 */
export function clearRemoteSSHTransportCooldown(sshHost: string): void {
  remoteSSHTransportFailures.delete(sshHost);
}

function rememberSSHTransportFailure(sshHost: string, error: Error): void {
  if (!isSSHTransportError(error)) {
    return;
  }

  const retryAfterMs = getRemoteSSHTransportFailureCooldownMs();
  Object.assign(error, { retryAfterMs });
  remoteSSHTransportFailures.set(sshHost, {
    until: Date.now() + retryAfterMs,
    error,
  });
}

function buildExecSSHArgs(
  hostConfig: SSHHostConfig,
  master: RemoteSSHMaster | undefined,
  options?: ExecGitOptions,
): string[] {
  const args = buildSSHArgs(hostConfig, {
    disableTty: true,
    connectionHealth: getKanvibeSSHConnectionHealthOptions(),
    strictHostKeyChecking: options?.strictHostKeyChecking,
    ...(master ? { connectionReuse: getKanvibeSSHConnectionReuseOptions(master.generation, hostConfig) } : {}),
  });
  if (master) {
    args[args.indexOf("ControlMaster=auto")] = "ControlMaster=no";
    // A dead mux socket must fail rather than starting unbounded fallback authentications.
    args.unshift("-o", "ProxyCommand=false");
  }
  return args;
}

function ensureRemoteSSHMaster(hostConfig: SSHHostConfig, key: string): RemoteSSHMaster {
  const existing = remoteSSHMasters.get(key);
  if (existing && !existing.failed && (existing.readyAt === 0 || Date.now() - existing.readyAt < REMOTE_SSH_MASTER_READY_TTL_MS)) {
    return existing;
  }
  const master: RemoteSSHMaster = { generation: nextSSHMasterGeneration++, readyAt: 0, failed: false, preparing: Promise.resolve() };
  remoteSSHMasters.set(key, master);
  master.preparing = runSerializedSSHAuthentication(hostConfig, async () => {
    await ensureKanvibeSSHControlDirectory();
    const args = buildSSHArgs(hostConfig, {
      disableTty: true,
      connectionReuse: getKanvibeSSHConnectionReuseOptions(master.generation, hostConfig),
      connectionHealth: getKanvibeSSHConnectionHealthOptions(),
    });
    args.unshift("-N", "-f");
    await spawnSSHMaster(args);
    master.readyAt = Date.now();
  }).catch((error: unknown) => {
    master.failed = true;
    throw error;
  });
  return master;
}

async function runSerializedSSHAuthentication<T>(hostConfig: SSHHostConfig, operation: (onAuthenticated: () => void) => Promise<T>, deadline?: number): Promise<T> {
  const key = JSON.stringify([hostConfig.hostname, hostConfig.port]);
  const previous = remoteSSHAuthenticationQueues.get(key) ?? Promise.resolve();
  let release!: () => void;
  const finished = new Promise<void>((resolve) => { release = resolve; });
  // Even a timed-out waiter stays chained behind its predecessor so later callers cannot overtake.
  const current = previous.then(() => finished);
  remoteSSHAuthenticationQueues.set(key, current);
  try {
    if (deadline === undefined) await previous;
    else await waitForSSHPreparation(previous, deadline);
    // The responder marker is the earliest reliable authentication acknowledgement available
    // from the CLI. Release before its callback can join a master queued behind this connection.
    return await operation(release);
  } finally {
    release();
    void current.then(() => {
      if (remoteSSHAuthenticationQueues.get(key) === current) remoteSSHAuthenticationQueues.delete(key);
    });
  }
}

function remainingSSHTimeout(deadline: number): number {
  const remaining = deadline - Date.now();
  if (remaining <= 0) throw createSSHTimeoutError(0, "", "");
  return remaining;
}

function waitForSSHPreparation(preparing: Promise<void>, deadline: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const remaining = Math.max(0, deadline - Date.now());
    const timeout = setTimeout(() => reject(Object.assign(createSSHTimeoutError(remaining, "", ""), { preparationWaitTimeout: true })), remaining);
    preparing.then(resolve, reject).finally(() => clearTimeout(timeout));
  });
}

function spawnSSHMaster(args: string[]): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn("ssh", args, { env: createLocalShellEnvironment(), stdio: ["ignore", "ignore", "pipe"] });
    let stderr = "";
    let settled = false;
    const timeout = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGTERM");
      reject(createSSHTimeoutError(DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS, "", stderr));
    }, DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS);
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => { if (!settled) stderr += chunk; });
    child.on("error", (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      reject(attachCommandOutput(error, "", stderr));
    });
    child.on("exit", (code) => {
      if (settled || code !== 0) return;
      settled = true;
      clearTimeout(timeout);
      // -f forks an idle master which retains stderr; exit confirms readiness without waiting
      // for that pipe's close. Keep draining it without extending the app process lifetime.
      (child.stderr as typeof child.stderr & { unref?: () => void }).unref?.();
      resolve();
    });
    child.on("close", (code, signal) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      reject(createSSHExitError(code, signal, "", stderr));
    });
  });
}

function buildRemoteShellCommand(command: string): string {
  const wrappedCommand = [
    `sh -lc ${quoteForPosixShell(command)}`,
    "__kanvibe_status=$?",
    `printf '\\n${REMOTE_COMMAND_EXIT_MARKER}:%s\\n' "$__kanvibe_status"`,
    'exit "$__kanvibe_status"',
  ].join("; ");

  return `sh -lc ${quoteForPosixShell(wrappedCommand)}`;
}

function quoteForPosixShell(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function normalizeSSHExecError(
  error: unknown,
  sshHost: string,
  timeoutMs: number,
  isCommandTimeout: boolean,
): Error {
  if (isCommandTimeout) {
    return new Error(`${sshHost} 원격 명령 실패: SSH 명령이 ${Math.ceil(timeoutMs / 1000)}초 안에 완료되지 않았습니다.`);
  }

  if (error && typeof error === "object" && "stderr" in error) {
    const stderr = String((error as { stderr?: string }).stderr || "").trim();
    const message = stderr ? summarizeCommandFailure(stderr) : (error instanceof Error ? error.message : "SSH 명령 실패");
    return Object.assign(new Error(`${sshHost} 원격 명령 실패: ${message}`), { remoteCommandCompleted: hasRemoteCommandCompleted(error) });
  }

  return error instanceof Error ? error : new Error(`${sshHost} 원격 명령 실패`);
}

export function resolvePathForShell(targetPath: string, sshHost?: string | null): string {
  if (!targetPath.startsWith("~")) {
    return quoteForPosixShell(targetPath);
  }

  if (sshHost) {
    const suffix = targetPath.slice(1);
    return `"$HOME"${quoteForPosixShell(suffix)}`;
  }

  return quoteForPosixShell(targetPath.replace(/^~/, homedir()));
}

/** 로컬 또는 SSH에서 명령을 실행한다. sshHost가 null이면 로컬 실행 */
export async function execGit(
  command: string,
  sshHost?: string | null,
  options?: ExecGitOptions,
): Promise<string> {
  if (sshHost) {
    return execRemote(sshHost, command, options);
  }
  return execLocal(command, options);
}

/** remote origin의 최신 브랜치 정보를 가져온다. 네트워크 실패 시 silent fail */
export async function fetchOrigin(
  repoPath: string,
  sshHost?: string | null
): Promise<void> {
  try {
    await execGit(`git -C ${resolvePathForShell(repoPath, sshHost)} fetch --prune`, sshHost);
  } catch {
    /* 네트워크 실패 시에도 로컬 브랜치 목록은 정상 제공해야 하므로 무시한다 */
  }
}

/** 현재 checkout된 브랜치를 fast-forward 가능한 경우에만 pull한다 */
export async function pullCurrentBranch(
  repoPath: string,
  sshHost?: string | null,
  options?: ExecGitOptions,
): Promise<string> {
  return execGit(`git -C ${resolvePathForShell(repoPath, sshHost)} pull --ff-only`, sshHost, options);
}

/** origin에 현재 task 브랜치가 존재하는지 확인한다 */
export async function remoteBranchExists(
  repoPath: string,
  branchName: string,
  sshHost?: string | null,
  options?: ExecGitOptions,
): Promise<boolean> {
  const resolvedRepoPath = resolvePathForShell(repoPath, sshHost);
  const remoteRef = quoteForPosixShell(`refs/heads/${branchName}`);
  const command = [
    `git -C ${resolvedRepoPath} ls-remote --exit-code --heads origin ${remoteRef} >/dev/null`,
    `status=$?`,
    `if [ "$status" -eq 0 ]; then printf exists; elif [ "$status" -eq 2 ]; then printf missing; else exit "$status"; fi`,
  ].join("; ");

  const output = await execGit(command, sshHost, options);
  return output.trim() === "exists";
}

/**
 * git 저장소의 브랜치 목록을 반환한다.
 * remote origin을 먼저 fetch하여 최신 상태를 반영하며,
 * 로컬에 없는 remote-only 브랜치는 `origin/` prefix를 유지한다.
 */
interface ListBranchesOptions {
  refresh?: boolean;
}

export async function listBranches(
  repoPath: string,
  sshHost?: string | null,
  options: ListBranchesOptions = {},
): Promise<string[]> {
  if (options.refresh ?? true) {
    await fetchOrigin(repoPath, sshHost);
  }

  const output = await execGit(
    `git -C ${resolvePathForShell(repoPath, sshHost)} branch -a --format='%(refname:short)'`,
    sshHost
  );

  const allBranches = output.split("\n").filter(Boolean);

  const localBranches = new Set(
    allBranches.filter((b) => !b.startsWith("origin/"))
  );

  const remoteBranches = allBranches
    .filter((b) => b.startsWith("origin/"))
    .map((b) => b.replace(/^origin\//, ""))
    .filter((b) => b !== "HEAD");

  /** remote-only 브랜치는 origin/ prefix를 유지하여 git ref resolution이 정확히 동작하도록 한다 */
  const remoteOnlyBranches = remoteBranches
    .filter((b) => !localBranches.has(b))
    .map((b) => `origin/${b}`);

  return [...localBranches, ...remoteOnlyBranches].filter(
    (value, index, self) => self.indexOf(value) === index
  );
}

/** 경로가 유효한 git 저장소인지 확인한다 */
export async function validateGitRepo(
  repoPath: string,
  sshHost?: string | null
): Promise<boolean> {
  try {
    await execGit(
      `git -C ${resolvePathForShell(repoPath, sshHost)} rev-parse --is-inside-work-tree`,
      sshHost
    );
    return true;
  } catch {
    return false;
  }
}

/** 저장소의 기본 브랜치(HEAD가 가리키는 브랜치)를 감지한다 */
export async function getDefaultBranch(
  repoPath: string,
  sshHost?: string | null
): Promise<string> {
  try {
    const output = await execGit(
      `git -C ${resolvePathForShell(repoPath, sshHost)} symbolic-ref --short HEAD`,
      sshHost
    );
    return output || "main";
  } catch {
    return "main";
  }
}

/**
 * 지정 디렉토리 하위의 git 저장소 경로 목록을 반환한다.
 * 일반 저장소의 `.git` 디렉토리와 worktree의 `.git` 파일을 모두 탐색하여 상위 경로를 추출한다.
 */
export async function scanGitRepos(
  rootPath: string,
  sshHost?: string | null
): Promise<string[]> {
  const resolvedPath = resolvePathForShell(rootPath, sshHost);
  const command = `find ${resolvedPath} -maxdepth 4 -name ".git" \\( -type d -o -type f \\) 2>/dev/null`;

  try {
    const output = await execGit(command, sshHost);

    if (!output) return [];

    return output
      .split("\n")
      .filter(Boolean)
      .map((gitDir) => gitDir.replace(/\/\.git$/, ""))
      .filter((value, index, self) => self.indexOf(value) === index);
  } catch (error) {
    console.error("[remote-scan] git repository scan failed", {
      sshHost: sshHost || null,
      rootPath,
      resolvedPath,
      command,
      error: error instanceof Error ? error.message : String(error),
    });
    return [];
  }
}

export interface WorktreeInfo {
  path: string;
  branch: string;
  isBare: boolean;
}

/** 프로젝트의 git worktree 목록을 조회한다 (bare worktree 포함) */
export async function listWorktrees(
  repoPath: string,
  sshHost?: string | null
): Promise<WorktreeInfo[]> {
  try {
    const output = await execGit(
      `git -C ${resolvePathForShell(repoPath, sshHost)} worktree list --porcelain`,
      sshHost
    );

    if (!output) return [];

    const worktrees: WorktreeInfo[] = [];
    const blocks = output.split("\n\n").filter(Boolean);

    for (const block of blocks) {
      const lines = block.split("\n");
      let worktreePath = "";
      let branch = "";
      let isBare = false;

      for (const line of lines) {
        if (line.startsWith("worktree ")) {
          worktreePath = line.replace("worktree ", "");
        } else if (line.startsWith("branch ")) {
          branch = line.replace("branch refs/heads/", "");
        } else if (line === "bare") {
          isBare = true;
        }
      }

      if (worktreePath) {
        worktrees.push({ path: worktreePath, branch, isBare });
      }
    }

    return worktrees;
  } catch {
    return [];
  }
}
