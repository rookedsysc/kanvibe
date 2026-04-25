import { readFile, writeFile, mkdir } from "fs/promises";
import path from "path";
import { exec } from "child_process";
import { promisify } from "util";
import { execGit } from "@/lib/gitOperations";

const execAsync = promisify(exec);

const MARKER = "# KanVibe AI hooks (auto-generated)";

const EXCLUDE_PATTERNS = [
  ".claude/hooks/",
  ".claude/settings.json",
  ".gemini/hooks/",
  ".gemini/settings.json",
  ".codex/hooks/",
  ".codex/hooks.json",
  ".codex/config.toml",
  ".opencode/plugins/",
];

function resolveAbsoluteGitPath(repoPath: string, gitPath: string, pathModule: typeof path | typeof path.posix): string {
  const trimmed = gitPath.trim();
  if (!trimmed) {
    return repoPath;
  }

  if (pathModule.isAbsolute(trimmed)) {
    return trimmed;
  }

  return pathModule.join(repoPath, trimmed);
}

function buildExcludeContent(currentContent: string): string {
  const lines = currentContent.replace(/\r\n/g, "\n").split("\n");
  const preservedLines: string[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line !== MARKER) {
      preservedLines.push(line);
      continue;
    }

    index += 1;
    while (index < lines.length && (EXCLUDE_PATTERNS.includes(lines[index]) || lines[index] === "")) {
      index += 1;
    }
    index -= 1;
  }

  const preservedContent = preservedLines.join("\n").trimEnd();
  const block = [MARKER, ...EXCLUDE_PATTERNS].join("\n");

  if (!preservedContent) {
    return `${block}\n`;
  }

  return `${preservedContent}\n\n${block}\n`;
}

async function readOptionalFile(filePath: string): Promise<string> {
  try {
    return await readFile(filePath, "utf-8");
  } catch {
    return "";
  }
}

async function writeExcludeFile(filePath: string): Promise<void> {
  await mkdir(path.dirname(filePath), { recursive: true });

  const currentContent = await readOptionalFile(filePath);
  const nextContent = buildExcludeContent(currentContent);
  if (currentContent === nextContent) {
    return;
  }

  await writeFile(filePath, nextContent, "utf-8");
}

async function getLocalExcludePaths(repoPath: string): Promise<string[]> {
  const [{ stdout: gitDirOutput }, { stdout: commonDirOutput }] = await Promise.all([
    execAsync("git rev-parse --git-dir", { cwd: repoPath }),
    execAsync("git rev-parse --git-common-dir", { cwd: repoPath }),
  ]);

  const gitDir = resolveAbsoluteGitPath(repoPath, gitDirOutput, path);
  const commonDir = resolveAbsoluteGitPath(repoPath, commonDirOutput, path);

  return [...new Set([
    path.join(gitDir, "info", "exclude"),
    path.join(commonDir, "info", "exclude"),
  ])];
}

async function readRemoteTextFile(filePath: string, sshHost: string): Promise<string> {
  return execGit(`test -f "${filePath}" && cat "${filePath}" || true`, sshHost);
}

async function writeRemoteTextFile(filePath: string, content: string, sshHost: string): Promise<void> {
  const encodedContent = Buffer.from(content, "utf-8").toString("base64");
  await execGit(
    `mkdir -p "${path.posix.dirname(filePath)}" && printf '%s' '${encodedContent}' | (base64 -d 2>/dev/null || base64 -D) > "${filePath}"`,
    sshHost,
  );
}

async function getRemoteExcludePaths(repoPath: string, sshHost: string): Promise<string[]> {
  const [gitDirOutput, commonDirOutput] = await Promise.all([
    execGit(`git -C "${repoPath}" rev-parse --path-format=absolute --git-dir`, sshHost),
    execGit(`git -C "${repoPath}" rev-parse --path-format=absolute --git-common-dir`, sshHost),
  ]);

  const gitDir = resolveAbsoluteGitPath(repoPath, gitDirOutput, path.posix);
  const commonDir = resolveAbsoluteGitPath(repoPath, commonDirOutput, path.posix);

  return [...new Set([
    path.posix.join(gitDir, "info", "exclude"),
    path.posix.join(commonDir, "info", "exclude"),
  ])];
}

/**
 * AI 코딩 도구의 hooks 설정 파일을 .git/info/exclude에 추가하여 git tracking에서 제외한다.
 * worktree 저장소에서는 현재 gitdir과 공용 git common dir 둘 다 갱신한다.
 * @param repoPath - worktree 또는 저장소 경로
 */
export async function addAiToolPatternsToGitExclude(repoPath: string): Promise<void> {
  const excludePaths = await getLocalExcludePaths(repoPath);
  await Promise.all(excludePaths.map((excludePath) => writeExcludeFile(excludePath)));
}

/**
 * 원격 저장소의 현재 gitdir과 공용 git common dir exclude 파일을 갱신한다.
 * linked worktree와 main worktree 모두에서 동일한 ignore 규칙이 적용되도록 한다.
 */
export async function addAiToolPatternsToGitExcludeRemote(repoPath: string, sshHost: string): Promise<void> {
  const excludePaths = await getRemoteExcludePaths(repoPath, sshHost);

  for (const excludePath of excludePaths) {
    const currentContent = await readRemoteTextFile(excludePath, sshHost);
    const nextContent = buildExcludeContent(currentContent);
    if (currentContent === nextContent) {
      continue;
    }

    await writeRemoteTextFile(excludePath, nextContent, sshHost);
  }
}
