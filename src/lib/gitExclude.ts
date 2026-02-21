import { exec } from "child_process";
import { promisify } from "util";
import { readFile, writeFile, mkdir } from "fs/promises";
import path from "path";

const execAsync = promisify(exec);

const MARKER = "# KanVibe AI hooks (auto-generated)";

const EXCLUDE_PATTERNS = [
  ".claude/hooks/",
  ".claude/settings.json",
  ".gemini/hooks/",
  ".gemini/settings.json",
  ".codex/hooks/",
  ".codex/config.toml",
  ".opencode/plugins/",
];

/**
 * AI 코딩 도구의 hooks 설정 파일을 .git/info/excluded에 추가하여 git tracking에서 제외한다.
 * 마커 블록을 사용해 멱등성을 보장하며, 실패해도 예외를 던지지 않는다.
 * @param repoPath - worktree 또는 저장소 경로
 */
export async function addAiToolPatternsToGitExclude(
  repoPath: string
): Promise<void> {
  const { stdout } = await execAsync("git rev-parse --git-dir", {
    cwd: repoPath,
  });
  const gitDir = stdout.trim();
  const absoluteGitDir = path.isAbsolute(gitDir)
    ? gitDir
    : path.join(repoPath, gitDir);

  const infoDir = path.join(absoluteGitDir, "info");
  await mkdir(infoDir, { recursive: true });

  const excludedPath = path.join(infoDir, "exclude");

  let content = "";
  try {
    content = await readFile(excludedPath, "utf-8");
  } catch {
    /* 파일이 없으면 새로 생성 */
  }

  if (content.includes(MARKER)) return;

  const block = [
    "",
    MARKER,
    ...EXCLUDE_PATTERNS,
  ].join("\n") + "\n";

  await writeFile(excludedPath, content.trimEnd() + "\n" + block, "utf-8");
}
