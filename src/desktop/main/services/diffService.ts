import { constants } from "fs";
import { open } from "fs/promises";
import path from "path";
import { getTaskDiffStatsRepository, getTaskRepository } from "@/lib/database";
import { In } from "typeorm";
import { execGit, runWithBackgroundSSHCommands } from "@/lib/gitOperations";
import { quoteShellArgument, readTextFile, writeTextFile } from "@/lib/hostFileAccess";
import { summarizeDiffFiles, type TaskDiffStats } from "@/desktop/shared/taskDiffStats";

export interface DiffFile {
  path: string;
  originalPath?: string;
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
    `printf '%s\\0' ${quoteShellArgument(DIFF_NAME_STATUS_MARKER)}`,
    `${buildGitCommand(worktreePath, ["diff", range, "--name-status", "-z"])}`,
    `printf '%s\\0' ${quoteShellArgument(DIFF_NUMSTAT_MARKER)}`,
    `${buildGitCommand(worktreePath, ["diff", range, "--numstat", "-z"])}`,
    `printf '%s\\0' ${quoteShellArgument(DIFF_WORKING_TREE_MARKER)}`,
    buildGitCommand(worktreePath, ["status", "--porcelain", "--untracked-files=all", "-z"]),
  ].join(" && ");
}

function parseGitDiffFilesCommandOutput(output: string) {
  const sections: Record<"nameStatus" | "numstat" | "workingTree", string[]> = {
    nameStatus: [], numstat: [], workingTree: [],
  };
  let currentSection: keyof typeof sections | null = null;
  const records = output.split("\0");
  for (let index = 0; index < records.length;) {
    const record = records[index];
    if (record === DIFF_NAME_STATUS_MARKER || record === DIFF_NUMSTAT_MARKER || record === DIFF_WORKING_TREE_MARKER) {
      currentSection = record === DIFF_NAME_STATUS_MARKER ? "nameStatus" : record === DIFF_NUMSTAT_MARKER ? "numstat" : "workingTree";
      index += 1;
      continue;
    }
    if (!currentSection || !record) {
      index += 1;
      continue;
    }
    // rename의 경로 필드는 구간 마커와 같은 이름이어도 경로로 소비해야 한다.
    let width = 1;
    if (currentSection === "nameStatus") width = /^[RC]/.test(record) ? 3 : 2;
    if (currentSection === "numstat" && record.endsWith("\t")) width = 3;
    if (currentSection === "workingTree" && /[RC]/.test(record.slice(0, 2))) width = 2;
    sections[currentSection].push(...records.slice(index, index + width));
    index += width;
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
  if (relativePath === ".." || relativePath.startsWith(`..${pathModule.sep}`) || pathModule.isAbsolute(relativePath)) {
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
 * 변경 파일 목록을 조회하되 실패를 `null`로 구분해 돌려준다.
 * 조회에 실패한 것과 정말 아무것도 바뀌지 않은 것을 같은 빈 배열로 뭉치면 캐시를 0으로 덮어쓰게 된다.
 */
async function readGitDiffFiles(taskId: string): Promise<DiffFile[] | null> {
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
    for (let index = 0; index < diffSections.numstat.length; index += 1) {
      const [added, deleted, ...pathParts] = diffSections.numstat[index].split("\t");
      let filePath = pathParts.join("\t");
      if (!filePath) {
        filePath = diffSections.numstat[index + 2];
        index += 2;
      }
      statsByPath.set(filePath, {
        additions: added === "-" ? 0 : parseInt(added, 10),
        deletions: deleted === "-" ? 0 : parseInt(deleted, 10),
      });
    }

    /** 커밋된 브랜치 diff에서 파일 목록을 구성한다 */
    const fileMap = new Map<string, DiffFile>();
    for (let index = 0; index < diffSections.nameStatus.length;) {
      const statusLetter = diffSections.nameStatus[index++];
      const originalPath = diffSections.nameStatus[index++];
      const filePath = /^[RC]/.test(statusLetter) ? diffSections.nameStatus[index++] : originalPath;
      const stats = statsByPath.get(filePath) ?? { additions: 0, deletions: 0 };
      fileMap.set(filePath, {
        path: filePath,
        ...(/^[RC]/.test(statusLetter) ? { originalPath } : {}),
        status: parseGitStatus(statusLetter),
        additions: stats.additions,
        deletions: stats.deletions,
      });
    }

    /** -z 상태 출력은 rename 목적지를 먼저 주고 이전 경로를 별도 필드로 준다 */
    for (let index = 0; index < diffSections.workingTree.length; index += 1) {
      const record = diffSections.workingTree[index];
      const xy = record.slice(0, 2);
      const filePath = record.slice(3);
      const originalPath = /[RC]/.test(xy) ? diffSections.workingTree[++index] : undefined;
      if (fileMap.has(filePath)) continue;
      const status = parseWorkingTreeStatus(xy);
      if (!status) continue;
      const previousFile = originalPath ? fileMap.get(originalPath) : undefined;
      if (originalPath) fileMap.delete(originalPath);
      fileMap.set(filePath, {
        path: filePath,
        ...(originalPath ? { originalPath: previousFile?.originalPath ?? originalPath } : {}),
        status,
        additions: previousFile?.additions ?? 0,
        deletions: previousFile?.deletions ?? 0,
      });
    }

    return Array.from(fileMap.values());
  } catch (error) {
    console.error("git diff 파일 목록 조회 실패:", error);
    return null;
  }
}

/**
 * baseBranch와 현재 브랜치 사이에서 변경된 파일 목록을 조회한다.
 * 커밋된 브랜치 차이뿐 아니라 working directory의
 * untracked/unstaged/staged 파일도 포함한다.
 *
 * 조회한 김에 집계 캐시도 갱신한다. 보드는 진행 중이 아닌 태스크의 배지를 이 캐시로 그리므로,
 * 사용자가 상세를 열어 본 태스크는 그 시점 값으로 최신화된다.
 */
export async function getGitDiffFiles(
  taskId: string
): Promise<DiffFile[]> {
  return (await refreshTaskDiffFiles(taskId)) ?? [];
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

export type TaskWorkspaceFileFailureReason = "outside-worktree" | "not-found" | "too-large" | "unreadable";

export interface TaskWorkspaceFile {
  /** worktree 기준 상대 경로 */
  path: string;
  content: string;
}

/**
 * 미리보기가 파일을 열지 못한 이유.
 * 받는 쪽은 `name`과 `reason`으로 가려내므로 클래스는 내보내지 않는다 — `serviceRegistry`가 이 모듈을 통째로
 * IPC에 얹어서, 내보내면 화면이 부를 일이 없는 값이 그 표면에 하나 더 얹힌다.
 * 메시지를 사유 그대로 두는 것은 IPC를 건너면 메시지 글만 남기 때문이다.
 */
class TaskWorkspaceFileError extends Error {
  constructor(readonly reason: TaskWorkspaceFileFailureReason) {
    super(reason);
    this.name = "TaskWorkspaceFileError";
  }
}

/** 미리보기 한 번에 싣는 파일 크기의 상한. 넘는 파일은 내용을 읽기 전에 거절한다 */
const MAX_TASK_WORKSPACE_FILE_BYTES = 1024 * 1024;

async function readLocalWorkspaceFile(resolvedPath: string): Promise<string> {
  // 이름 붙은 파이프를 열다 멈추지 않도록 막힘 없이 연다.
  const file = await open(resolvedPath, constants.O_RDONLY | constants.O_NONBLOCK).catch((error: NodeJS.ErrnoException) => {
    throw new TaskWorkspaceFileError(error.code === "ENOENT" || error.code === "ENOTDIR" ? "not-found" : "unreadable");
  });

  try {
    const stats = await file.stat();
    if (!stats.isFile()) throw new TaskWorkspaceFileError("not-found");
    if (stats.size > MAX_TASK_WORKSPACE_FILE_BYTES) throw new TaskWorkspaceFileError("too-large");

    const buffer = await file.readFile();
    // 크기를 잰 뒤에 파일이 커졌을 수 있다.
    if (buffer.byteLength > MAX_TASK_WORKSPACE_FILE_BYTES) throw new TaskWorkspaceFileError("too-large");
    return buffer.toString("utf-8");
  } finally {
    await file.close();
  }
}

const REMOTE_FILE_MISSING_MARKER = "__KANVIBE_FILE_MISSING__";
const REMOTE_FILE_TOO_LARGE_MARKER = "__KANVIBE_FILE_TOO_LARGE__";
const REMOTE_FILE_CONTENT_MARKER = "__KANVIBE_FILE_CONTENT__";

/** 원격에서는 크기를 먼저 재고 상한 안일 때만 내용을 보내게 해서, 큰 파일이 SSH를 타고 넘어오지 않게 한다 */
async function readRemoteWorkspaceFile(resolvedPath: string, sshHost: string): Promise<string> {
  const quotedPath = quoteShellArgument(resolvedPath);
  const command = [
    `if ! test -f ${quotedPath}; then printf '%s' ${REMOTE_FILE_MISSING_MARKER};`,
    `elif test "$(wc -c < ${quotedPath})" -gt ${MAX_TASK_WORKSPACE_FILE_BYTES}; then printf '%s' ${REMOTE_FILE_TOO_LARGE_MARKER};`,
    `else printf '%s\\n' ${REMOTE_FILE_CONTENT_MARKER} && cat ${quotedPath}; fi`,
  ].join(" ");
  const output = await execGit(command, sshHost, { maxBufferBytes: MAX_TASK_WORKSPACE_FILE_BYTES + 4096 });

  if (output === REMOTE_FILE_MISSING_MARKER) throw new TaskWorkspaceFileError("not-found");
  if (output === REMOTE_FILE_TOO_LARGE_MARKER) throw new TaskWorkspaceFileError("too-large");
  if (output === REMOTE_FILE_CONTENT_MARKER) return "";
  if (!output.startsWith(`${REMOTE_FILE_CONTENT_MARKER}\n`)) throw new TaskWorkspaceFileError("unreadable");
  return output.slice(REMOTE_FILE_CONTENT_MARKER.length + 1);
}

/** 절대 경로로 온 요청을 worktree 기준 상대 경로로 바꾼다. AI 기록은 파일을 대개 절대 경로로 남긴다 */
function toWorktreeRelativePath(worktreePath: string, filePath: string, sshHost: string | null): string {
  const pathModule = sshHost ? path.posix : path;
  return pathModule.isAbsolute(filePath) ? pathModule.relative(worktreePath, filePath) : filePath;
}

/**
 * 미리보기에 쓸 worktree 안 파일 하나를 읽는다.
 * `getFileContent`와 달리 실패를 빈 문자열로 삼키지 않고, 화면이 사유별 안내를 낼 수 있게
 * `TaskWorkspaceFileError`로 알린다. 태스크나 worktree가 없는 경우도 파일을 찾지 못한 것으로 본다.
 */
export async function readTaskWorkspaceFile(taskId: string, filePath: string): Promise<TaskWorkspaceFile> {
  const { worktreePath, sshHost } = await getTaskWorktreeInfo(taskId).catch(() => {
    throw new TaskWorkspaceFileError("not-found");
  });
  const pathModule = sshHost ? path.posix : path;

  let resolvedPath: string;
  try {
    resolvedPath = validateFilePath(worktreePath, toWorktreeRelativePath(worktreePath, filePath, sshHost), sshHost);
  } catch {
    throw new TaskWorkspaceFileError("outside-worktree");
  }

  try {
    const content = sshHost
      ? await readRemoteWorkspaceFile(resolvedPath, sshHost)
      : await readLocalWorkspaceFile(resolvedPath);
    return { path: pathModule.relative(worktreePath, resolvedPath), content };
  } catch (error) {
    if (error instanceof TaskWorkspaceFileError) throw error;
    console.error("미리보기 파일 읽기 실패:", error);
    throw new TaskWorkspaceFileError("unreadable");
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

/** 집계 저장 실패는 기록하고 파일 목록 조회는 유지한다 */
async function saveTaskDiffStats(taskId: string, stats: TaskDiffStats): Promise<void> {
  try {
    const repo = await getTaskDiffStatsRepository();
    await repo.save({ taskId, ...stats });
  } catch (error) {
    console.error("변경 집계 저장 실패:", error);
  }
}

/** 보드가 갱신을 기다리지 않고 표시할 마지막 집계 */
async function loadCachedTaskDiffStats(): Promise<Record<string, TaskDiffStats>> {
  try {
    const repo = await getTaskDiffStatsRepository();
    const records = await repo.find();

    return Object.fromEntries(records.map((record) => [
      record.taskId,
      { fileCount: record.fileCount, additions: record.additions, deletions: record.deletions },
    ]));
  } catch (error) {
    console.error("변경 집계 캐시 조회 실패:", error);
    return {};
  }
}

const pendingDiffReads = new Map<string, Promise<DiffFile[] | null>>();

function refreshTaskDiffFiles(taskId: string): Promise<DiffFile[] | null> {
  const pending = pendingDiffReads.get(taskId);
  if (pending) return pending;

  const refresh = (async () => {
    const files = await readGitDiffFiles(taskId);
    if (files) await saveTaskDiffStats(taskId, summarizeDiffFiles(files));
    return files;
  })().finally(() => { pendingDiffReads.delete(taskId); });
  // 저장까지 공유해 늦은 동일 task 응답이 새 집계를 덮어쓰지 않게 한다.
  pendingDiffReads.set(taskId, refresh);
  return refresh;
}

/** 진행 중이 아닌 task의 갱신을 시작한다. 같은 task의 조회와 저장은 refreshTaskDiffFiles가 공유한다 */
async function scheduleTaskDiffRefreshes(taskIds: string[]): Promise<void> {
  const unscheduledTaskIds = taskIds.filter((taskId) => (
    !pendingDiffReads.has(taskId)
  ));
  if (unscheduledTaskIds.length === 0) return;

  try {
    const repo = await getTaskRepository();
    const tasks = await repo.find({ where: { id: In(unscheduledTaskIds) }, select: { id: true } });
    for (const task of tasks) {
      // 보드 배지 갱신은 기다리는 사용자가 없으므로 원격 명령의 background 우선순위만 지정한다.
      void runWithBackgroundSSHCommands(() => refreshTaskDiffFiles(task.id));
    }
  } catch (error) {
    console.error("변경 집계 갱신 예약 실패:", error);
  }
}

/** 저장된 집계를 먼저 반환한다. 완료된 갱신은 다음 조회부터 항목별로 표시한다. */
export async function getTaskDiffStats(
  taskIdsToRefresh: string[]
): Promise<Record<string, TaskDiffStats>> {
  const [cachedStats] = await Promise.all([
    loadCachedTaskDiffStats(),
    scheduleTaskDiffRefreshes(taskIdsToRefresh),
  ]);
  return cachedStats;
}
