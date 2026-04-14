import { exec } from "child_process";
import { promisify } from "util";
import { readFile } from "fs/promises";
import { homedir } from "os";
import path from "path";

const execAsync = promisify(exec);

/** 로컬에서 셸 명령을 실행하고 stdout을 반환한다 */
async function execLocal(command: string): Promise<string> {
  const { stdout } = await execAsync(command);
  return stdout.trim();
}

/** SSH를 통해 원격에서 명령을 실행하고 stdout을 반환한다 */
async function execRemote(sshHost: string, command: string): Promise<string> {
  const { Client } = await import("ssh2");
  const { parseSSHConfig } = await import("@/lib/sshConfig");

  const configs = await parseSSHConfig();
  const hostConfig = configs.find((c) => c.host === sshHost);

  if (!hostConfig) {
    throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
  }

  return new Promise((resolve, reject) => {
    const conn = new Client();

    conn.on("ready", () => {
      conn.exec(command, (err, stream) => {
        if (err) {
          conn.end();
          return reject(err);
        }

        let output = "";
        let errorOutput = "";

        stream.on("data", (data: Buffer) => {
          output += data.toString();
        });

        stream.stderr.on("data", (data: Buffer) => {
          errorOutput += data.toString();
        });

        stream.on("close", (code: number) => {
          conn.end();
          if (code !== 0) {
            reject(new Error(`SSH 명령 실패 (exit ${code}): ${errorOutput}`));
          } else {
            resolve(output.trim());
          }
        });
      });
    });

    conn.on("error", reject);

    let privateKey: Buffer;
    try {
      privateKey = require("fs").readFileSync(hostConfig.privateKeyPath);
    } catch {
      return reject(new Error(`SSH 키를 읽을 수 없습니다: ${hostConfig.privateKeyPath}`));
    }

    conn.connect({
      host: hostConfig.hostname,
      port: hostConfig.port,
      username: hostConfig.username,
      privateKey,
    });
  });
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

/**
 * git 저장소의 브랜치 목록을 반환한다.
 * remote origin을 먼저 fetch하여 최신 상태를 반영하며,
 * 로컬에 없는 remote-only 브랜치는 `origin/` prefix를 유지한다.
 */
export async function listBranches(
  repoPath: string,
  sshHost?: string | null
): Promise<string[]> {
  await fetchOrigin(repoPath, sshHost);

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
 * .git 디렉토리를 maxdepth 4까지 탐색하여 상위 경로를 추출한다.
 */
export async function scanGitRepos(
  rootPath: string,
  sshHost?: string | null
): Promise<string[]> {
  const resolvedPath = rootPath.startsWith("~")
    ? rootPath.replace(/^~/, homedir())
    : rootPath;

  try {
    const output = await execGit(
      `find "${resolvedPath}" -maxdepth 4 -name ".git" -type d 2>/dev/null`,
      sshHost
    );

    if (!output) return [];

    return output
      .split("\n")
      .filter(Boolean)
      .map((gitDir) => gitDir.replace(/\/\.git$/, ""));
  } catch {
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
