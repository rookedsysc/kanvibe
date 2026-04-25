import path from "node:path";
import { execGit } from "@/lib/gitOperations";
import { quoteShellArgument, readTextFile, writeTextFile } from "@/lib/hostFileAccess";

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
  const markerBlock = [MARKER, ...EXCLUDE_PATTERNS].join("\n");

  if (!preservedContent) {
    return `${markerBlock}\n`;
  }

  return `${preservedContent}\n\n${markerBlock}\n`;
}

function resolveAbsoluteGitPath(
  repoPath: string,
  gitPath: string,
  pathModule: typeof path | typeof path.posix,
): string {
  const trimmed = gitPath.trim();
  if (!trimmed) {
    return repoPath;
  }

  if (pathModule.isAbsolute(trimmed)) {
    return trimmed;
  }

  return pathModule.join(repoPath, trimmed);
}

async function getExcludePaths(
  repoPath: string,
  sshHost?: string | null,
): Promise<string[]> {
  const pathModule = sshHost ? path.posix : path;
  const [gitDirOutput, gitCommonDirOutput] = await Promise.all([
    execGit(
      `git -C ${quoteShellArgument(repoPath)} rev-parse --path-format=absolute --git-dir`,
      sshHost,
    ),
    execGit(
      `git -C ${quoteShellArgument(repoPath)} rev-parse --path-format=absolute --git-common-dir`,
      sshHost,
    ),
  ]);

  const gitDir = resolveAbsoluteGitPath(repoPath, gitDirOutput, pathModule);
  const gitCommonDir = resolveAbsoluteGitPath(repoPath, gitCommonDirOutput, pathModule);

  return [...new Set([
    pathModule.join(gitDir, "info", "exclude"),
    pathModule.join(gitCommonDir, "info", "exclude"),
  ])];
}

/**
 * AI 코딩 도구의 hooks 설정 파일을 git metadata의 info/exclude에 추가하여
 * 현재 worktree와 공용 git common dir 모두에서 git tracking에서 제외한다.
 * 마커 블록을 사용해 멱등성을 보장하며, 원격 저장소도 지원한다.
 * @param repoPath - worktree 또는 저장소 경로
 */
export async function addAiToolPatternsToGitExclude(
  repoPath: string,
  sshHost?: string | null,
): Promise<void> {
  const excludePaths = await getExcludePaths(repoPath, sshHost);

  for (const excludePath of excludePaths) {
    const content = await readTextFile(excludePath, sshHost);
    const nextContent = buildExcludeContent(content);
    if (content === nextContent) {
      continue;
    }

    await writeTextFile(excludePath, nextContent, sshHost);
  }
}
