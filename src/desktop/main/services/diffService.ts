import path from "path";
import { getTaskRepository } from "@/lib/database";
import { execGit } from "@/lib/gitOperations";
import { quoteShellArgument, readTextFile, writeTextFile } from "@/lib/hostFileAccess";

export interface DiffFile {
  path: string;
  status: "added" | "modified" | "deleted" | "renamed";
  additions: number;
  deletions: number;
}

/** git status 문자를 사람이 읽기 쉬운 상태 문자열로 변환한다 */
function parseGitStatus(statusLetter: string): DiffFile["status"] {
  switch (statusLetter) {
    case "A":
      return "added";
    case "D":
      return "deleted";
    case "M":
      return "modified";
    default:
      if (statusLetter.startsWith("R")) return "renamed";
      return "modified";
  }
}

function buildGitCommand(worktreePath: string, args: string[]) {
  return `git -C ${quoteShellArgument(worktreePath)} ${args.join(" ")}`;
}

const DIFF_NAME_STATUS_MARKER = "__KANVIBE_DIFF_NAME_STATUS__";
const DIFF_NUMSTAT_MARKER = "__KANVIBE_DIFF_NUMSTAT__";
const DIFF_WORKING_TREE_MARKER = "__KANVIBE_DIFF_WORKING_TREE__";

function buildGitDiffFilesCommand(worktreePath: string, baseBranch: string, branchName: string): string {
  const range = quoteShellArgument(`${baseBranch}...${branchName}`);
  return [
    `printf '%s\\n' ${quoteShellArgument(DIFF_NAME_STATUS_MARKER)}`,
    `${buildGitCommand(worktreePath, ["diff", range, "--name-status"])} || true`,
    `printf '%s\\n' ${quoteShellArgument(DIFF_NUMSTAT_MARKER)}`,
    `${buildGitCommand(worktreePath, ["diff", range, "--numstat"])} || true`,
    `printf '%s\\n' ${quoteShellArgument(DIFF_WORKING_TREE_MARKER)}`,
    buildGitCommand(worktreePath, ["status", "--porcelain", "--untracked-files=all"]),
  ].join("; ");
}

function parseGitDiffFilesCommandOutput(output: string) {
  const sections = {
    nameStatus: "",
    numstat: "",
    workingTree: "",
  };
  let currentSection: keyof typeof sections | null = null;

  for (const line of output.split("\n")) {
    if (line === DIFF_NAME_STATUS_MARKER) {
      currentSection = "nameStatus";
      continue;
    }

    if (line === DIFF_NUMSTAT_MARKER) {
      currentSection = "numstat";
      continue;
    }

    if (line === DIFF_WORKING_TREE_MARKER) {
      currentSection = "workingTree";
      continue;
    }

    if (currentSection) {
      sections[currentSection] += `${line}\n`;
    }
  }

  return sections;
}

/**
 * 파일 경로가 worktree 디렉토리 내부에 있는지 검증한다.
 * 경로 탐색 공격(path traversal)을 방지하기 위해 ".." 포함 여부와
 * resolve된 절대 경로가 worktreePath로 시작하는지 확인한다.
 */
function validateFilePath(worktreePath: string, filePath: string, sshHost?: string | null): string {
  const pathModule = sshHost ? path.posix : path;

  if (pathModule.isAbsolute(filePath)) {
    throw new Error("잘못된 파일 경로: 절대 경로는 허용되지 않습니다");
  }

  const resolvedPath = pathModule.resolve(worktreePath, filePath);
  const relativePath = pathModule.relative(worktreePath, resolvedPath);
  if (relativePath.startsWith("..") || pathModule.isAbsolute(relativePath)) {
    throw new Error("잘못된 파일 경로: worktree 외부 접근이 감지되었습니다");
  }

  return resolvedPath;
}

/** 태스크 ID로 worktree 관련 정보(worktreePath, branchName, baseBranch)를 조회한다 */
async function getTaskWorktreeInfo(taskId: string) {
  const repo = await getTaskRepository();
  const task = await repo.findOne({ where: { id: taskId } });

  if (!task) {
    throw new Error("태스크를 찾을 수 없습니다");
  }

  if (!task.worktreePath) {
    throw new Error("worktree 경로가 설정되지 않은 태스크입니다");
  }

  if (!task.branchName) {
    throw new Error("브랜치가 설정되지 않은 태스크입니다");
  }

  return {
    worktreePath: task.worktreePath,
    branchName: task.branchName,
    baseBranch: task.baseBranch ?? "main",
    sshHost: task.sshHost ?? null,
  };
}

/** git status의 porcelain 포맷 문자를 DiffFile 상태로 변환한다 */
function parseWorkingTreeStatus(
  xy: string
): DiffFile["status"] | null {
  const index = xy[0];
  const worktree = xy[1];

  if (xy === "??") return "added";
  if (index === "A" || worktree === "A") return "added";
  if (index === "D" || worktree === "D") return "deleted";
  if (index === "M" || worktree === "M") return "modified";
  if (index === "R" || worktree === "R") return "renamed";
  return null;
}

/**
 * baseBranch와 현재 브랜치 사이에서 변경된 파일 목록을 조회한다.
 * 커밋된 브랜치 차이뿐 아니라 working directory의
 * untracked/unstaged/staged 파일도 포함한다.
 */
export async function getGitDiffFiles(
  taskId: string
): Promise<DiffFile[]> {
  try {
    const { worktreePath, branchName, baseBranch, sshHost } =
      await getTaskWorktreeInfo(taskId);

    const commandOutput = await execGit(
      buildGitDiffFilesCommand(worktreePath, baseBranch, branchName),
      sshHost,
    );
    const diffSections = parseGitDiffFilesCommandOutput(commandOutput);

    /** numstat 결과를 파일 경로 기준으로 맵핑한다 */
    const statsByPath = new Map<
      string,
      { additions: number; deletions: number }
    >();
    if (diffSections.numstat.trim()) {
      for (const line of diffSections.numstat.trim().split("\n")) {
        const [added, deleted, ...pathParts] = line.split("\t");
        const filePath = pathParts.join("\t");
        statsByPath.set(filePath, {
          additions: added === "-" ? 0 : parseInt(added, 10),
          deletions: deleted === "-" ? 0 : parseInt(deleted, 10),
        });
      }
    }

    /** 커밋된 브랜치 diff에서 파일 목록을 구성한다 */
    const fileMap = new Map<string, DiffFile>();

    if (diffSections.nameStatus.trim()) {
      for (const line of diffSections.nameStatus.trim().split("\n")) {
        const parts = line.split("\t");
        const statusLetter = parts[0];
        /** rename의 경우 "R100\told\tnew" 형태이므로 마지막 경로를 사용한다 */
        const filePath = parts.length >= 3 ? parts[2] : parts[1];
        const stats = statsByPath.get(filePath) ?? {
          additions: 0,
          deletions: 0,
        };
        fileMap.set(filePath, {
          path: filePath,
          status: parseGitStatus(statusLetter),
          additions: stats.additions,
          deletions: stats.deletions,
        });
      }
    }

    /** working directory의 변경/untracked 파일을 추가한다 (커밋된 diff에 없는 파일만) */
    const workingTreeLines = diffSections.workingTree.replace(/\n$/, "");
    if (workingTreeLines) {
      for (const line of workingTreeLines.split("\n")) {
        const xy = line.substring(0, 2);
        const filePath = line.substring(3);
        if (fileMap.has(filePath)) continue;

        const status = parseWorkingTreeStatus(xy);
        if (!status) continue;

        fileMap.set(filePath, {
          path: filePath,
          status,
          additions: 0,
          deletions: 0,
        });
      }
    }

    return Array.from(fileMap.values());
  } catch (error) {
    console.error("git diff 파일 목록 조회 실패:", error);
    return [];
  }
}

/** baseBranch 기준의 원본 파일 내용을 조회한다. 파일이 존재하지 않으면 빈 문자열을 반환한다 */
export async function getOriginalFileContent(
  taskId: string,
  filePath: string
): Promise<string> {
  try {
    const { worktreePath, baseBranch, sshHost } =
      await getTaskWorktreeInfo(taskId);

    validateFilePath(worktreePath, filePath, sshHost);

    return await execGit(
      buildGitCommand(worktreePath, [
        "show",
        quoteShellArgument(`${baseBranch}:${filePath}`),
      ]),
      sshHost,
    );
  } catch (error) {
    /** 파일이 baseBranch에 존재하지 않는 경우(신규 파일) 빈 문자열을 반환한다 */
    return "";
  }
}

/** worktree에서 현재 파일 내용을 읽어 반환한다 */
export async function getFileContent(
  taskId: string,
  filePath: string
): Promise<string> {
  try {
    const { worktreePath, sshHost } = await getTaskWorktreeInfo(taskId);
    const resolvedPath = validateFilePath(worktreePath, filePath, sshHost);

    return await readTextFile(resolvedPath, sshHost);
  } catch (error) {
    console.error("파일 내용 읽기 실패:", error);
    return "";
  }
}

/** worktree 내 파일에 내용을 저장한다 */
export async function saveFileContent(
  taskId: string,
  filePath: string,
  content: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const { worktreePath, sshHost } = await getTaskWorktreeInfo(taskId);
    const resolvedPath = validateFilePath(worktreePath, filePath, sshHost);

    await writeTextFile(resolvedPath, content, sshHost);
    return { success: true };
  } catch (error) {
    const message =
      error instanceof Error ? error.message : "파일 저장 중 알 수 없는 오류가 발생했습니다";
    console.error("파일 저장 실패:", error);
    return { success: false, error: message };
  }
}
