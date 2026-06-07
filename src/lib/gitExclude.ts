import path from "node:path";
import { execGit } from "@/lib/gitOperations";
import { quoteShellArgument, readTextFile, writeTextFile } from "@/lib/hostFileAccess";

export const KANVIBE_GIT_EXCLUDE_MARKER = "# KanVibe AI hooks (auto-generated)";
export const KANVIBE_STATE_DIR_EXCLUDE_PATTERN = ".kanvibe/";

const EXCLUDE_PATTERNS = [
  ".claude/hooks/",
  ".claude/settings.json",
  ".gemini/hooks/",
  ".gemini/settings.json",
  ".codex/hooks/",
  ".codex/hooks.json",
  ".codex/config.toml",
  ".opencode/plugins/",
  KANVIBE_STATE_DIR_EXCLUDE_PATTERN,
];

const LEGACY_EXCLUDE_PATTERNS = [
  ".kanvibe/hooks-targets.json",
  ".kanvibe/task-state.json",
  ".kanvibe/status.md",
  ".kanvibe/status.json",
];

function buildExcludeContent(currentContent: string): string {
  const lines = currentContent.replace(/\r\n/g, "\n").split("\n");
  const preservedLines: string[] = [];

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line !== KANVIBE_GIT_EXCLUDE_MARKER) {
      preservedLines.push(line);
      continue;
    }

    index += 1;
    while (index < lines.length && (
      EXCLUDE_PATTERNS.includes(lines[index])
      || LEGACY_EXCLUDE_PATTERNS.includes(lines[index])
      || lines[index] === ""
    )) {
      index += 1;
    }
    index -= 1;
  }

  const preservedContent = preservedLines.join("\n").trimEnd();
  const markerBlock = [KANVIBE_GIT_EXCLUDE_MARKER, ...EXCLUDE_PATTERNS].join("\n");

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

async function getCommonExcludePath(
  repoPath: string,
  sshHost?: string | null,
): Promise<string> {
  const pathModule = sshHost ? path.posix : path;
  const gitCommonDirOutput = await execGit(
    `git -C ${quoteShellArgument(repoPath)} rev-parse --path-format=absolute --git-common-dir`,
    sshHost,
  );

  const gitCommonDir = resolveAbsoluteGitPath(repoPath, gitCommonDirOutput, pathModule);

  return pathModule.join(gitCommonDir, "info", "exclude");
}

/**
 * AI 코딩 도구의 hooks 설정 파일을 git metadata의 info/exclude에 추가하여
 * main worktree가 공유하는 git common dir의 info/exclude에서 git tracking에서 제외한다.
 * 마커 블록을 사용해 멱등성을 보장하며, 원격 저장소도 지원한다.
 * @param repoPath - worktree 또는 저장소 경로
 */
export async function addAiToolPatternsToGitExclude(
  repoPath: string,
  sshHost?: string | null,
): Promise<void> {
  const excludePath = await getCommonExcludePath(repoPath, sshHost);
  const content = await readTextFile(excludePath, sshHost);
  const nextContent = buildExcludeContent(content);
  if (content === nextContent) {
    return;
  }

  await writeTextFile(excludePath, nextContent, sshHost);
}
