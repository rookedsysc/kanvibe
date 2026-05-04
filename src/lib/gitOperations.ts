import { exec, execFile } from "child_process";
import { promisify } from "util";
import { homedir } from "os";
import {
  buildSSHArgs,
  ensureKanvibeSSHControlDirectory,
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

const REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS = 60_000;
const REMOTE_SSH_HOST_MAX_CONCURRENCY = 4;
const remoteSSHTransportFailures = new Map<string, { until: number; error: Error }>();
const remoteSSHHostLimiters = new Map<string, { active: number; queue: Array<() => void> }>();

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

/** 로컬에서 셸 명령을 실행하고 stdout을 반환한다 */
async function execLocal(command: string): Promise<string> {
  const { stdout } = await execAsync(command, {
    env: createLocalShellEnvironment(),
  });
  return stdout.trim();
}

/** SSH를 통해 원격에서 명령을 실행하고 stdout을 반환한다 */
async function execRemote(sshHost: string, command: string): Promise<string> {
  return runLimitedRemoteCommand(sshHost, async () => {
    throwIfSSHTransportCooldownActive(sshHost);

    const configs = await parseSSHConfig();
    const hostConfig = configs.find((c) => c.host === sshHost);

    if (!hostConfig) {
      throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
    }

    const sshArgs = [
      ...await buildExecSSHArgs(hostConfig),
      buildRemoteShellCommand(command),
    ];

    try {
      const { stdout } = await execFileAsync("ssh", sshArgs, {
        env: createLocalShellEnvironment(),
        maxBuffer: 10 * 1024 * 1024,
      });
      return stdout.trim();
    } catch (error) {
      const stderr = error && typeof error === "object" && "stderr" in error
        ? String((error as { stderr?: string }).stderr || "")
        : "";

      if (shouldLogRemoteCommandFailure(command, stderr)) {
        console.error("[remote-ssh] command failed", {
          sshHost,
          command,
          sshArgs,
          error: error instanceof Error ? error.message : String(error),
        });
      }

      const normalizedError = normalizeSSHExecError(error, sshHost);
      rememberSSHTransportFailure(sshHost, normalizedError);
      throw normalizedError;
    }
  });
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

    if (limiter.active < REMOTE_SSH_HOST_MAX_CONCURRENCY) {
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
    until: Date.now() + REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS,
    error,
  });
}

async function buildExecSSHArgs(
  hostConfig: SSHHostConfig,
): Promise<string[]> {
  if (process.platform === "win32") {
    return buildSSHArgs(hostConfig, { disableTty: true });
  }

  await ensureKanvibeSSHControlDirectory();
  return buildSSHArgs(hostConfig, {
    disableTty: true,
    connectionReuse: getKanvibeSSHConnectionReuseOptions(),
  });
}

function buildRemoteShellCommand(command: string): string {
  return `sh -lc ${quoteForPosixShell(command)}`;
}

function quoteForPosixShell(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function normalizeSSHExecError(error: unknown, sshHost: string): Error {
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
export async function execGit(command: string, sshHost?: string | null): Promise<string> {
  if (sshHost) {
    return execRemote(sshHost, command);
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
