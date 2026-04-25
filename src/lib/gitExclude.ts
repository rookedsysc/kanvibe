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

/**
 * AI 코딩 도구의 hooks 설정 파일을 git common dir의 info/exclude에 추가하여
 * 모든 worktree에서 공통으로 git tracking에서 제외한다.
 * 마커 블록을 사용해 멱등성을 보장하며, 원격 저장소도 지원한다.
 * @param repoPath - worktree 또는 저장소 경로
 */
export async function addAiToolPatternsToGitExclude(
  repoPath: string,
  sshHost?: string | null,
): Promise<void> {
  const gitCommonDir = (await execGit(
    `git -C ${quoteShellArgument(repoPath)} rev-parse --path-format=absolute --git-common-dir`,
    sshHost,
  )).trim();

  if (!gitCommonDir) {
    throw new Error(`git common dir를 확인할 수 없습니다: ${repoPath}`);
  }

  const excludePath = sshHost
    ? path.posix.join(gitCommonDir, "info", "exclude")
    : path.join(gitCommonDir, "info", "exclude");

  const content = await readTextFile(excludePath, sshHost);
  if (content.includes(MARKER)) {
    return;
  }

  const markerBlock = [MARKER, ...EXCLUDE_PATTERNS].join("\n");
  const nextContent = content.trimEnd().length > 0
    ? `${content.trimEnd()}\n\n${markerBlock}\n`
    : `${markerBlock}\n`;

  await writeTextFile(excludePath, nextContent, sshHost);
}
