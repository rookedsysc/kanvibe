import { exec, execFile, spawn } from "child_process";
import { promisify } from "util";
import { homedir } from "os";
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
const execFileAsync = promisify(execFile);

const SSH_TRANSPORT_ERROR_PATTERNS = [
  /kex_exchange_identification/i,
  /connection reset by peer/i,
  /connection reset by /i,
  /connection closed by /i,
  /connection refused/i,
  /connection timed out/i,
  /ssh command timed out/i,
  /ssh 명령이 .*완료되지 않았/i,
  /operation timed out/i,
  /no route to host/i,
  /network is unreachable/i,
  /could not resolve hostname/i,
  /name or service not known/i,
  /temporary failure in name resolution/i,
  /host key verification failed/i,
  /remote host identification has changed/i,
  /permission denied \(publickey/i,
  /too many authentication failures/i,
  /bad owner or permissions/i,
  /session open refused by peer/i,
  /ssh: connect to host/i,
];

const DEFAULT_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS = 5_000;
const MAX_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS = 60_000;
const DEFAULT_REMOTE_SSH_HOST_MAX_CONCURRENCY = 4;
const MAX_REMOTE_SSH_HOST_MAX_CONCURRENCY = 16;
const DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS = 45_000;
const REMOTE_SSH_COMMAND_MAX_ATTEMPTS = 3;
const REMOTE_SSH_CONTROLMASTER_SHUTDOWN_TIMEOUT_MS = 3_000;
const REMOTE_COMMAND_EXIT_MARKER = "__KANVIBE_REMOTE_COMMAND_EXIT_7b3f6e5d__";
const REMOTE_COMMAND_MAX_BUFFER_BYTES = 10 * 1024 * 1024;
const remoteSSHTransportFailures = new Map<string, { until: number; error: Error }>();
const remoteSSHHostLimiters = new Map<string, { active: number; queue: Array<() => void> }>();

export interface ExecGitOptions {
  timeoutMs?: number;
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
  return SSH_TRANSPORT_ERROR_PATTERNS.some((pattern) => pattern.test(errorOutput));
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
    readPositiveInteger(
      process.env.KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY,
      DEFAULT_REMOTE_SSH_HOST_MAX_CONCURRENCY,
    ),
    MAX_REMOTE_SSH_HOST_MAX_CONCURRENCY,
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
async function execLocal(command: string): Promise<string> {
  const { stdout } = await execAsync(command, {
    env: createLocalShellEnvironment(),
  });
  return stdout.trim();
}

/** SSH를 통해 원격에서 명령을 실행하고 stdout을 반환한다 */
async function execRemote(
  sshHost: string,
  command: string,
  options?: ExecGitOptions,
): Promise<string> {
  return runLimitedRemoteCommand(sshHost, async () => {
    throwIfSSHTransportCooldownActive(sshHost);

    const timeoutMs = getRemoteSSHCommandTimeoutMs(options);
    const configs = await parseSSHConfig();
    const hostConfig = configs.find((c) => c.host === sshHost);

    if (!hostConfig) {
      throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
    }

    const sshArgs = [
      ...await buildExecSSHArgs(hostConfig),
      buildRemoteShellCommand(command),
    ];

    let lastError: Error | null = null;
    for (let attempt = 1; attempt <= REMOTE_SSH_COMMAND_MAX_ATTEMPTS; attempt += 1) {
      try {
        return await spawnSSHCommand(sshArgs, timeoutMs);
      } catch (error) {
        const isCommandTimeout = isSSHCommandTimeoutError(error);
        const normalizedError = normalizeSSHExecError(error, sshHost, timeoutMs, isCommandTimeout);
        const shouldRetry = isRetriableSSHExecError(normalizedError, isCommandTimeout)
          && attempt < REMOTE_SSH_COMMAND_MAX_ATTEMPTS;
        lastError = normalizedError;

        if (isCommandTimeout || isSSHTransportError(normalizedError)) {
          await closeRemoteSSHControlMaster(hostConfig).catch(() => undefined);
        }

        if (!shouldRetry) {
          logRemoteCommandFailure(command, sshHost, sshArgs, error);
          rememberSSHTransportFailure(sshHost, normalizedError);
          throw normalizedError;
        }
      }
    }

    throw lastError ?? new Error(`${sshHost} 원격 명령 실패`);
  });
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

function isRetriableSSHExecError(error: Error, isCommandTimeout: boolean): boolean {
  return isCommandTimeout || isSSHTransportError(error);
}

function spawnSSHCommand(sshArgs: string[], timeoutMs: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn("ssh", sshArgs, {
      env: createLocalShellEnvironment(),
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";
    let settled = false;
    const timeout = setTimeout(() => {
      finishWithError(createSSHTimeoutError(timeoutMs, stdout, stderr));
    }, timeoutMs);

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");

    child.stdout.on("data", (chunk: string) => {
      stdout += chunk;
      rejectIfOutputIsTooLarge();

      const parsed = parseRemoteCommandCompletion(stdout);
      if (parsed) {
        finishWithRemoteCompletion(parsed.exitCode, parsed.stdout, stderr);
      }
    });

    child.stderr.on("data", (chunk: string) => {
      stderr += chunk;
      rejectIfOutputIsTooLarge();
    });

    child.on("error", (error) => {
      finishWithError(attachCommandOutput(error, stdout, stderr));
    });

    child.on("close", (code, signal) => {
      if (settled) {
        return;
      }

      const parsed = parseRemoteCommandCompletion(stdout);
      if (parsed) {
        finishWithRemoteCompletion(parsed.exitCode, parsed.stdout, stderr);
        return;
      }

      if (code === 0) {
        finishWithSuccess(stdout.trim());
        return;
      }

      finishWithError(createSSHExitError(code, signal, stdout, stderr));
    });

    function rejectIfOutputIsTooLarge(): void {
      if (Buffer.byteLength(stdout) + Buffer.byteLength(stderr) <= REMOTE_COMMAND_MAX_BUFFER_BYTES) {
        return;
      }

      finishWithError(attachCommandOutput(
        new Error("SSH command output exceeded maxBuffer"),
        stdout,
        stderr,
      ));
    }

    function finishWithRemoteCompletion(exitCode: number, completedStdout: string, completedStderr: string): void {
      if (exitCode === 0) {
        finishWithSuccess(completedStdout.trim());
        return;
      }

      const error = createSSHExitError(exitCode, null, completedStdout, completedStderr);
      finishWithError(error);
    }

    function finishWithSuccess(output: string): void {
      if (settled) {
        return;
      }

      settled = true;
      clearTimeout(timeout);
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
      if (child.exitCode === null && !child.killed) {
        child.kill("SIGTERM");
      }
      reject(error);
    }
  });
}

function parseRemoteCommandCompletion(stdout: string): { stdout: string; exitCode: number } | null {
  const markerPrefix = `\n${REMOTE_COMMAND_EXIT_MARKER}:`;
  const markerIndex = stdout.lastIndexOf(markerPrefix);
  if (markerIndex < 0) {
    return null;
  }

  const exitCodeMatch = stdout
    .slice(markerIndex + markerPrefix.length)
    .match(/^(\d+)\r?\n/);
  if (!exitCodeMatch) {
    return null;
  }

  return {
    stdout: stdout.slice(0, markerIndex),
    exitCode: Number.parseInt(exitCodeMatch[1], 10),
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
  const release = await acquireRemoteSSHSlot(sshHost);

  try {
    return await operation();
  } finally {
    release();
  }
}

function acquireRemoteSSHSlot(sshHost: string): Promise<() => void> {
  let limiter = remoteSSHHostLimiters.get(sshHost);
  if (!limiter) {
    limiter = { active: 0, queue: [] };
    remoteSSHHostLimiters.set(sshHost, limiter);
  }

  return new Promise((resolve) => {
    const start = () => {
      limiter.active += 1;
      resolve(() => releaseRemoteSSHSlot(sshHost, limiter));
    };

    if (limiter.active < getRemoteSSHHostMaxConcurrency()) {
      start();
      return;
    }

    limiter.queue.push(start);
  });
}

function releaseRemoteSSHSlot(
  sshHost: string,
  limiter: { active: number; queue: Array<() => void> },
): void {
  limiter.active -= 1;

  const next = limiter.queue.shift();
  if (next) {
    next();
    return;
  }

  if (limiter.active === 0) {
    remoteSSHHostLimiters.delete(sshHost);
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

  throw new Error(
    `${sshHost} 원격 명령 실패: 최근 SSH transport 실패로 원격 명령을 잠시 건너뜁니다. 마지막 오류: ${failure.error.message}`,
  );
}

function rememberSSHTransportFailure(sshHost: string, error: Error): void {
  if (!isSSHTransportError(error)) {
    return;
  }

  remoteSSHTransportFailures.set(sshHost, {
    until: Date.now() + getRemoteSSHTransportFailureCooldownMs(),
    error,
  });
}

async function buildExecSSHArgs(
  hostConfig: SSHHostConfig,
): Promise<string[]> {
  if (process.platform === "win32") {
    return buildSSHArgs(hostConfig, {
      disableTty: true,
      connectionHealth: getKanvibeSSHConnectionHealthOptions(),
    });
  }

  await ensureKanvibeSSHControlDirectory();
  return buildSSHArgs(hostConfig, {
    disableTty: true,
    connectionReuse: getKanvibeSSHConnectionReuseOptions(),
    connectionHealth: getKanvibeSSHConnectionHealthOptions(),
  });
}

async function closeRemoteSSHControlMaster(hostConfig: SSHHostConfig): Promise<void> {
  if (process.platform === "win32") {
    return;
  }

  const reuseOptions = getKanvibeSSHConnectionReuseOptions();
  const baseArgs = buildSSHArgs(hostConfig, {
    disableTty: true,
    connectionHealth: {
      connectTimeoutSeconds: 3,
      serverAliveIntervalSeconds: 1,
      serverAliveCountMax: 1,
    },
  });
  const destination = baseArgs.at(-1);
  if (!destination) {
    return;
  }

  const shutdownArgs = [
    ...baseArgs.slice(0, -1),
    "-O",
    "exit",
    "-S",
    reuseOptions.controlPath,
    destination,
  ];

  await execFileAsync("ssh", shutdownArgs, {
    env: createLocalShellEnvironment(),
    timeout: REMOTE_SSH_CONTROLMASTER_SHUTDOWN_TIMEOUT_MS,
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
    return new Error(`${sshHost} 원격 명령 실패: ${message}`);
  }

  return error instanceof Error ? error : new Error(`${sshHost} 원격 명령 실패`);
}

export function resolvePathForShell(targetPath: string, sshHost?: string | null): string {
  if (!targetPath.startsWith("~")) {
    return `"${targetPath}"`;
  }

  if (sshHost) {
    const suffix = targetPath.slice(1);
    return `"$HOME${suffix}"`;
  }

  return `"${targetPath.replace(/^~/, homedir())}"`;
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
  return execLocal(command);
}

/** remote origin의 최신 브랜치 정보를 가져온다. 네트워크 실패 시 silent fail */
export async function fetchOrigin(
  repoPath: string,
  sshHost?: string | null
): Promise<void> {
  try {
    await execGit(`git -C "${repoPath}" fetch --prune`, sshHost);
  } catch {
    /* 네트워크 실패 시에도 로컬 브랜치 목록은 정상 제공해야 하므로 무시한다 */
  }
}

/** 현재 checkout된 브랜치를 fast-forward 가능한 경우에만 pull한다 */
export async function pullCurrentBranch(
  repoPath: string,
  sshHost?: string | null
): Promise<string> {
  return execGit(`git -C "${repoPath}" pull --ff-only`, sshHost);
}

/** origin에 현재 task 브랜치가 존재하는지 확인한다 */
export async function remoteBranchExists(
  repoPath: string,
  branchName: string,
  sshHost?: string | null,
): Promise<boolean> {
  const resolvedRepoPath = resolvePathForShell(repoPath, sshHost);
  const remoteRef = quoteForPosixShell(`refs/heads/${branchName}`);
  const command = [
    `git -C ${resolvedRepoPath} ls-remote --exit-code --heads origin ${remoteRef} >/dev/null`,
    `status=$?`,
    `if [ "$status" -eq 0 ]; then printf exists; elif [ "$status" -eq 2 ]; then printf missing; else exit "$status"; fi`,
  ].join("; ");

  const output = await execGit(command, sshHost);
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
    `git -C "${repoPath}" branch -a --format='%(refname:short)'`,
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
      `git -C "${repoPath}" rev-parse --is-inside-work-tree`,
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
      `git -C "${repoPath}" symbolic-ref --short HEAD`,
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
      `git -C "${repoPath}" worktree list --porcelain`,
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
