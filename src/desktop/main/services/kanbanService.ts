import { In, Not, Like } from "typeorm";
import { getTaskRepository } from "@/lib/database";
import { KanbanTask, TaskStatus, SessionType } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { TaskPriority } from "@/entities/TaskPriority";
import { createWorktreeWithSession, removeWorktreeAndBranch, createSessionWithoutWorktree, removeSessionOnly, formatProjectBranchSessionName } from "@/lib/worktree";
import { getProjectRepository } from "@/lib/database";
import {
  broadcastBoardUpdate,
  broadcastTaskHookInstallFailed,
  broadcastTaskPrMergedDetectedBatch,
  type BackgroundSyncFailurePayload,
  type TaskPrMergedDetectedPayload,
} from "@/lib/boardNotifier";
import {
  installKanvibeHooks,
  installKanvibeHookFiles,
  scheduleKanvibeHooksVerification,
} from "@/lib/kanvibeHooksInstaller";
import { execGit, pullCurrentBranch, remoteBranchExists, type ExecGitOptions } from "@/lib/gitOperations";
import { detachSession } from "@/lib/terminal";
import {
  persistTaskMetadataForTask as persistTaskMetadata,
  persistTaskStateForTask as persistTaskState,
} from "@/desktop/main/services/kanvibeTaskStateService";
import { persistProjectColorToKanvibeState } from "@/desktop/main/services/kanvibeProjectColorService";

export type TasksByStatus = Record<TaskStatus, KanbanTask[]>;

export interface TasksByStatusWithMeta {
  tasks: TasksByStatus;
  doneTotal: number;
  doneLimit: number;
}

export interface LoadMoreDoneResponse {
  tasks: KanbanTask[];
  doneTotal: number;
}

export interface SearchableTask {
  id: string;
  title: string;
  branchName: string | null;
  projectId: string | null;
  projectName: string | null;
  sshHost: string | null;
  status: TaskStatus;
  updatedAt: Date;
}

const DONE_PAGE_SIZE = 20;
const TERMINAL_HOOK_INSTALL_PRE_ATTACH_WAIT_MS = 1_500;
const ACTIVE_PULL_TASK_STATUSES = [
  TaskStatus.TODO,
  TaskStatus.PROGRESS,
  TaskStatus.PENDING,
  TaskStatus.REVIEW,
];
const ACTIVE_TASK_PULL_GIT_TIMEOUT_MS = 10_000;
const ACTIVE_TASK_PR_GITHUB_CLI_TIMEOUT_MS = 10_000;
const notifiedPullFailureKeys = new Set<string>();

interface CleanupTaskResourcesOptions {
  throwOnError?: boolean;
}

const TASK_RESOURCE_DELETE_CLEANUP_OPTIONS = { throwOnError: true } as const;

interface GitHubPullRequestInfo {
  url: string | null;
  state: string | null;
  mergedAt: string | null;
  updatedAt: string | null;
}

export interface ActiveTaskPullRequestSyncResult {
  updatedTaskIds: string[];
  mergeEventKeys: string[];
  mergedPullRequests: TaskPrMergedDetectedPayload[];
  failures?: BackgroundSyncFailurePayload[];
}

export interface TaskPullSyncPayload {
  taskId: string;
  taskTitle: string;
  branchName: string;
  worktreePath: string;
  sshHost: string | null;
  status: "updated" | "failed";
  summary: string;
}

export interface ActiveTaskPullSyncResult {
  pulledTasks: TaskPullSyncPayload[];
}

/** TypeORM 엔티티를 직렬화 가능한 plain object로 변환한다 */
function serialize<T>(data: T): T {
  return JSON.parse(JSON.stringify(data));
}

function isMissingGitHubCli(error: unknown): boolean {
  if (error && typeof error === "object" && "code" in error && (error as { code?: string }).code === "ENOENT") {
    return true;
  }

  const message = error instanceof Error ? error.message : String(error ?? "");
  return /(?:^|\s)gh:.*not found/i.test(message) || /command not found.*\bgh\b/i.test(message);
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function buildPullRequestSyncFailure(
  task: Pick<KanbanTask, "id" | "title" | "branchName" | "sshHost">,
  error: unknown,
): BackgroundSyncFailurePayload {
  const branchSuffix = task.branchName ? ` (${task.branchName})` : "";

  return {
    operation: "pull-request-sync",
    target: `${task.title}${branchSuffix}`,
    reason: getErrorMessage(error),
    taskId: task.id,
    ...(task.branchName ? { branchName: task.branchName } : {}),
    ...(task.sshHost ? { sshHost: task.sshHost } : {}),
  };
}

function quoteForShell(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function scheduleTaskHookVerification(
  targetPath: string,
  task: Pick<KanbanTask, "id" | "title" | "sshHost">,
) {
  scheduleKanvibeHooksVerification(targetPath, task.id, task.sshHost, {
    onSuccess: () => {
      broadcastBoardUpdate();
    },
    onFailure: (error) => {
      reportTaskHookInstallFailure(targetPath, task, error, "hooks 백그라운드 검증 실패");
    },
  });
}

export async function installTaskHooksImmediately(
  targetPath: string,
  task: Pick<KanbanTask, "id" | "title" | "sshHost">,
  failureLogMessage: string,
) {
  try {
    await installKanvibeHooks(targetPath, task.id, task.sshHost);
  } catch (error) {
    reportTaskHookInstallFailure(targetPath, task, error, failureLogMessage);
    throw error;
  }
}

export async function installTaskHookFilesImmediately(
  targetPath: string,
  task: Pick<KanbanTask, "id" | "title" | "sshHost">,
  failureLogMessage: string,
) {
  try {
    await installKanvibeHookFiles(targetPath, task.id, task.sshHost);
    scheduleTaskHookVerification(targetPath, task);
  } catch (error) {
    reportTaskHookInstallFailure(targetPath, task, error, failureLogMessage);
  }
}

async function installTaskHookFilesBeforeTerminalAttach(
  targetPath: string,
  task: Pick<KanbanTask, "id" | "title" | "sshHost">,
) {
  const installJob = installTaskHookFilesImmediately(
    targetPath,
    task,
    "터미널 연결 전 hooks 동기 설치 실패",
  );

  const installResult = await waitForTerminalHookInstallPreAttach(installJob);
  if (installResult === "timeout") {
    void installJob;
  }
}

async function waitForTerminalHookInstallPreAttach(
  installJob: Promise<void>,
): Promise<"completed" | "timeout"> {
  let timeoutId: ReturnType<typeof setTimeout> | null = null;

  try {
    return await Promise.race([
      installJob.then(() => "completed" as const),
      new Promise<"timeout">((resolve) => {
        timeoutId = setTimeout(() => resolve("timeout"), TERMINAL_HOOK_INSTALL_PRE_ATTACH_WAIT_MS);
      }),
    ]);
  } finally {
    if (timeoutId) {
      clearTimeout(timeoutId);
    }
  }
}

function reportTaskHookInstallFailure(
  targetPath: string,
  task: Pick<KanbanTask, "id" | "title" | "sshHost">,
  error: unknown,
  logMessage: string,
) {
  const errorMessage = getErrorMessage(error);

  console.error(`${logMessage}:`, {
    taskId: task.id,
    taskTitle: task.title,
    targetPath,
    sshHost: task.sshHost ?? null,
    error: errorMessage,
  });

  broadcastTaskHookInstallFailed({
    taskId: task.id,
    taskTitle: task.title,
    error: errorMessage,
  });
}

/**
 * 저장소 안에서 gh를 실행할 셸 명령을 만든다.
 *
 * 등록된 경로가 저장소를 감싼 상위 폴더일 수 있어, `.git`이 없으면 바로 아래에서 저장소를 한 번 더 찾는다.
 * direnv가 깔려 있으면 그 경로의 `.envrc`를 태워서 gh를 부른다. GitHub 인증을 `.envrc`에 심어 둔 경우
 * 셸을 거치지 않고 gh를 부르면 인증이 빠진 채로 돌아 권한 없음으로 실패한다.
 */
function buildGitHubCliCommand(repoPath: string, ghArguments: string): string {
  return [
    `repo=${quoteForShell(repoPath)}`,
    'if [ ! -e "$repo/.git" ]; then for candidate in "$repo"/*/; do if [ -e "$candidate.git" ]; then repo="${candidate%/}"; break; fi; done; fi',
    'cd "$repo" || exit 1',
    `if command -v direnv >/dev/null 2>&1; then direnv exec "$repo" gh ${ghArguments}; else gh ${ghArguments}; fi`,
  ].join("; ");
}

/** 로컬만 명령 자체에 제한 시간을 걸고, 원격은 SSH 계층이 쓰는 제한 시간을 그대로 따른다 */
function getGitHubCliExecOptions(sshHost?: string | null): ExecGitOptions | undefined {
  return sshHost ? undefined : { timeoutMs: ACTIVE_TASK_PR_GITHUB_CLI_TIMEOUT_MS };
}

async function getPrUrlFromGitHubCli(branchName: string, cwd: string, sshHost?: string | null): Promise<string | null> {
  try {
    const output = await execGit(
      buildGitHubCliCommand(cwd, `pr list --head ${quoteForShell(branchName)} --json url -q '.[0].url'`),
      sshHost,
      getGitHubCliExecOptions(sshHost),
    );
    return output.trim() || null;
  } catch (error) {
    if (isMissingGitHubCli(error)) {
      return null;
    }

    throw error;
  }
}

function parseGitHubPullRequestInfo(output: string): GitHubPullRequestInfo | null {
  const parsed = JSON.parse(output) as Array<{
    url?: string | null;
    state?: string | null;
    mergedAt?: string | null;
    updatedAt?: string | null;
  }>;

  if (!Array.isArray(parsed) || parsed.length === 0) {
    return null;
  }

  const latest = [...parsed].sort((left, right) => {
    const leftTime = left.updatedAt ? Date.parse(left.updatedAt) : 0;
    const rightTime = right.updatedAt ? Date.parse(right.updatedAt) : 0;
    return rightTime - leftTime;
  })[0];

  return {
    url: latest?.url ?? null,
    state: latest?.state ?? null,
    mergedAt: latest?.mergedAt ?? null,
    updatedAt: latest?.updatedAt ?? null,
  };
}

async function getPrInfoFromGitHubCli(
  branchName: string,
  cwd: string,
  sshHost?: string | null,
): Promise<GitHubPullRequestInfo | null> {
  try {
    const output = await execGit(
      buildGitHubCliCommand(cwd, `pr list --head ${quoteForShell(branchName)} --state all --json url,state,mergedAt,updatedAt`),
      sshHost,
      getGitHubCliExecOptions(sshHost),
    );
    if (!output.trim()) {
      return null;
    }

    return parseGitHubPullRequestInfo(output);
  } catch (error) {
    if (isMissingGitHubCli(error)) {
      return null;
    }

    throw error;
  }
}

function isDefaultBranchTask(
  task: Pick<KanbanTask, "branchName">,
  project: { defaultBranch: string } | null,
): boolean {
  return Boolean(project && task.branchName && task.branchName === project.defaultBranch);
}

async function resolveTaskGitContext(
  task: Pick<KanbanTask, "projectId" | "worktreePath" | "sshHost">,
  project?: { repoPath: string; sshHost: string | null } | null,
): Promise<{
  cwd: string;
  sshHost: string | null;
}> {
  let repoPath: string | null = null;
  let sshHost: string | null = task.sshHost || null;

  if (project) {
    repoPath = project.repoPath;
    sshHost = sshHost ?? project.sshHost ?? null;
  } else if (task.projectId) {
    const projectRepo = await getProjectRepository();
    const resolvedProject = await projectRepo.findOneBy({ id: task.projectId });
    if (resolvedProject) {
      repoPath = resolvedProject.repoPath;
      sshHost = sshHost ?? resolvedProject.sshHost ?? null;
    }
  }

  return {
    cwd: repoPath ?? task.worktreePath ?? process.cwd(),
    sshHost,
  };
}

function buildMergedPullRequestEventKey(
  taskId: string,
  prUrl: string,
  mergedAt: string,
): string {
  return `${taskId}:${prUrl}:${mergedAt}`;
}

function summarizePullOutput(output: string): string {
  const line = output
    .split(/\r?\n/)
    .map((item) => item.trim())
    .find(Boolean);

  return line ?? "Pull completed";
}

function isPullNoop(output: string): boolean {
  return /already up[- ]to[- ]date/i.test(output)
    || /current branch .* is up to date/i.test(output);
}

function buildPullFailureKey(taskId: string, branchName: string, worktreePath: string, sshHost: string | null): string {
  return [taskId, branchName, worktreePath, sshHost ?? ""].join("::");
}

function isMissingRemoteBranchPullError(error: unknown): boolean {
  const message = getErrorMessage(error);
  return /no such ref was fetched/i.test(message)
    || /could(?: not|n't) find remote ref/i.test(message)
    || /requested upstream branch .* does not exist/i.test(message);
}

/** 모든 작업을 상태별로 그룹핑하여 반환한다. Done 컬럼은 첫 페이지만 로드한다 */
export async function getTasksByStatus(): Promise<TasksByStatusWithMeta> {
  const repo = await getTaskRepository();

  const nonDoneTasks = await repo.find({
    where: { status: Not(TaskStatus.DONE) },
    order: { updatedAt: "DESC" },
  });

  const [doneTasks, doneTotal] = await repo.findAndCount({
    where: { status: TaskStatus.DONE },
    order: { updatedAt: "DESC" },
    take: DONE_PAGE_SIZE,
  });

  const grouped: TasksByStatus = {
    [TaskStatus.TODO]: [],
    [TaskStatus.PROGRESS]: [],
    [TaskStatus.PENDING]: [],
    [TaskStatus.REVIEW]: [],
    [TaskStatus.DONE]: doneTasks,
  };

  for (const task of nonDoneTasks) {
    grouped[task.status].push(task);
  }

  return serialize({ tasks: grouped, doneTotal, doneLimit: DONE_PAGE_SIZE });
}

/** Done 태스크를 추가 로드한다 */
export async function getMoreDoneTasks(
  offset: number,
  limit: number = DONE_PAGE_SIZE
): Promise<LoadMoreDoneResponse> {
  const repo = await getTaskRepository();

  const [tasks, doneTotal] = await repo.findAndCount({
    where: { status: TaskStatus.DONE },
    order: { updatedAt: "DESC" },
    skip: offset,
    take: limit,
  });

  return serialize({ tasks, doneTotal });
}

/** 단일 작업을 ID로 조회한다 */
export async function getTaskById(taskId: string): Promise<KanbanTask | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOne({ where: { id: taskId }, relations: ["project"] });
  return task ? serialize(task) : null;
}

/** 빠른 검색 오버레이에서 사용할 태스크 목록을 반환한다 */
export async function getSearchableTasks(): Promise<SearchableTask[]> {
  const repo = await getTaskRepository();
  const tasks = await repo.find({
    relations: ["project"],
    order: { updatedAt: "DESC", createdAt: "DESC" },
  });

  return serialize(tasks.map((task) => ({
    id: task.id,
    title: task.title,
    branchName: task.branchName,
    projectId: task.projectId,
    projectName: task.project?.name ?? null,
    sshHost: task.sshHost ?? task.project?.sshHost ?? null,
    status: task.status,
    updatedAt: task.updatedAt,
  })));
}

/** 같은 프로젝트 내에서 branchName이 일치하는 태스크 ID를 조회 */
export async function getTaskIdByProjectAndBranch(
  projectId: string,
  branchName: string,
): Promise<string | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOne({
    where: { projectId, branchName },
    select: ["id"],
  });
  return task?.id ?? null;
}

export interface CreateTaskInput {
  title: string;
  description?: string;
  branchName?: string;
  baseBranch?: string;
  sessionType?: SessionType;
  sshHost?: string;
  projectId?: string;
  priority?: TaskPriority;
}

/** 새 작업을 생성한다. branchName + projectId가 있으면 worktree와 세션도 함께 생성한다 */
export async function createTask(input: CreateTaskInput): Promise<KanbanTask> {
  const repo = await getTaskRepository();

  const task = repo.create({
    title: input.title || input.branchName || "Untitled",
    description: input.description || null,
    branchName: input.branchName || null,
    baseBranch: input.baseBranch || null,
    sessionType: input.sessionType || null,
    sshHost: input.sshHost || null,
    projectId: input.projectId || null,
    priority: input.priority || null,
    status: TaskStatus.TODO,
  });

  let hookTargetPath: string | null = null;
  let shouldInstallHooks = false;

  if (input.branchName && input.sessionType && input.projectId) {
    try {
      const projectRepo = await getProjectRepository();
      const project = await projectRepo.findOneBy({ id: input.projectId });

      if (project) {
        const baseBranch = input.baseBranch || project.defaultBranch;
        const session = await createWorktreeWithSession(
          project.repoPath,
          input.branchName,
          baseBranch,
          input.sessionType,
          project.sshHost,
          input.projectId
        );
        task.worktreePath = session.worktreePath;
        task.sessionName = session.sessionName;
        task.sshHost = project.sshHost;
        hookTargetPath = session.worktreePath;
        shouldInstallHooks = Boolean(session.worktreePath);
      }
    } catch (error) {
      console.error("Worktree/세션 생성 실패:", error);
      if (input.sshHost || input.projectId) {
        throw error;
      }
    }
  }

  const saved = await repo.save(task);

  if (shouldInstallHooks && hookTargetPath) {
    await installTaskHookFilesImmediately(
      hookTargetPath,
      saved,
      "새 태스크 hooks 동기 설치 실패",
    );
  }

  await persistTaskState(saved);

  if (saved.description || saved.priority) {
    await persistTaskMetadata(saved);
  }

  broadcastBoardUpdate();

  return serialize(saved);
}

/** 작업의 상태를 변경한다 */
export async function updateTaskStatus(
  taskId: string,
  newStatus: TaskStatus
): Promise<KanbanTask | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task) return null;

  task.status = newStatus;
  const saved = await repo.save(task);
  await persistTaskState(saved);
  broadcastBoardUpdate();
  return serialize(saved);
}

/** 작업의 정보를 부분 업데이트한다 */
export async function updateTask(
  taskId: string,
  updates: Partial<Pick<KanbanTask, "title" | "description" | "priority">>
): Promise<KanbanTask | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task) return null;

  if (updates.title !== undefined) task.title = updates.title;
  if (updates.description !== undefined) task.description = updates.description;
  if (updates.priority !== undefined) task.priority = updates.priority;

  const saved = await repo.save(task);

  if (updates.description !== undefined || updates.priority !== undefined) {
    await persistTaskMetadata(saved);
  }

  broadcastBoardUpdate();
  return serialize(saved);
}

/**
 * 프로젝트의 color(hex)를 변경하고, 같은 repo의 worktree 프로젝트에도 동일하게 반영한다.
 * 변경된 색상은 `.kanvibe/project.json`에도 기록되어 같은 저장소를 보는 다른 client와 동기화된다.
 *
 * 색상의 권위는 프로젝트 루트의 `.kanvibe/project.json`이므로 DB보다 먼저 기록한다. 순서가 뒤집히면
 * 그 사이 도는 background sync가 아직 이전 색상인 공유 파일을 읽어 방금 고른 색을 DB에서 되돌린다.
 */
export async function updateProjectColor(
  projectId: string,
  color: string
): Promise<void> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return;

  // worktree 프로젝트들의 color도 함께 업데이트한다
  const mainRepoPath = project.repoPath.includes("__worktrees")
    ? project.repoPath.split("__worktrees")[0]
    : project.repoPath;

  const relatedProjects = await repo.find({
    where: { repoPath: Like(`${mainRepoPath}%`) },
  });

  const recoloredProjects = orderProjectRepoRootFirst(
    [project, ...relatedProjects.filter((related) => related.id !== projectId)],
    mainRepoPath,
  );

  for (const recolored of recoloredProjects) {
    recolored.color = color;
    await persistProjectColorToKanvibeState(recolored);
  }

  await repo.update({ id: In(recoloredProjects.map((recolored) => recolored.id)) }, { color });

  broadcastBoardUpdate();
}

/**
 * 저장소 루트를 보는 프로젝트를 맨 앞으로 옮긴다.
 * 색상의 권위 파일은 루트에 있으므로 사용자가 worktree 프로젝트에서 색을 바꿨더라도 루트부터 확정해야 한다.
 */
function orderProjectRepoRootFirst(projects: Project[], mainRepoPath: string): Project[] {
  const rootIndex = projects.findIndex((project) => project.repoPath === mainRepoPath);
  if (rootIndex <= 0) {
    return projects;
  }

  return [
    projects[rootIndex],
    ...projects.filter((_, index) => index !== rootIndex),
  ];
}

/** 작업에 연결된 worktree, 세션, 브랜치를 정리한다. task 레코드는 삭제하지 않는다 */
async function cleanupTaskResources(
  task: KanbanTask,
  options: CleanupTaskResourcesOptions = {},
): Promise<void> {
  let project = null;
  if (task.projectId) {
    const projectRepo = await getProjectRepository();
    project = await projectRepo.findOneBy({ id: task.projectId });
  }

  const sshHost = task.sshHost || project?.sshHost || null;
  const fallbackTmuxSessionName = !task.sessionName
    && (task.sessionType === SessionType.TMUX || !task.sessionType)
    && sshHost
    && task.branchName
    && project?.repoPath
      ? formatProjectBranchSessionName(project.repoPath, task.branchName)
      : null;
  const cleanupSessionType = task.sessionName
    ? task.sessionType
    : (fallbackTmuxSessionName ? SessionType.TMUX : null);
  const cleanupSessionName = task.sessionName ?? fallbackTmuxSessionName;
  const sessionCleanupOptions = options.throwOnError
    ? { throwOnError: true }
    : undefined;
  const worktreeCleanupOptions = {
    ...(options.throwOnError ? { throwOnError: true } : {}),
    worktreePath: task.worktreePath,
  };

  /** 브랜치별 독립 세션 정리 */
  if (cleanupSessionType && cleanupSessionName) {
    try {
      detachSession(task.id, "cleanup-task-resources");

      if (sessionCleanupOptions) {
        await removeSessionOnly(
          cleanupSessionType,
          cleanupSessionName,
          sshHost,
          sessionCleanupOptions,
        );
      } else {
        await removeSessionOnly(
          cleanupSessionType,
          cleanupSessionName,
          sshHost,
        );
      }
    } catch (error) {
      if (options.throwOnError) {
        throw error;
      }
      console.error("세션 정리 실패:", error);
    }
  }

  // 프로젝트 없이 브랜치/worktree만 남은 태스크는 stale 상태이므로 worktree/브랜치 정리는 건너뛴다.
  if (!project && task.branchName && task.worktreePath) {
    if (options.throwOnError) {
      throw new Error("연결된 프로젝트를 찾을 수 없어 worktree/브랜치 정리를 완료할 수 없습니다.");
    }
    return;
  }

  const isProjectRoot = project && task.worktreePath === project.repoPath;

  /** worktree + 브랜치 정리 (프로젝트 루트 브랜치 제외) */
  if (task.branchName && !isProjectRoot) {
    if (!project?.repoPath) {
      const warningPayload = {
        taskId: task.id,
        branchName: task.branchName,
        worktreePath: task.worktreePath,
        projectRepoPath: project?.repoPath ?? null,
        sshHost,
      };
      console.warn("worktree/브랜치 정리 건너뜀: 정리할 프로젝트 정보가 부족합니다.", warningPayload);
      if (options.throwOnError) {
        throw new Error("정리할 프로젝트 정보가 부족해 worktree/브랜치 정리를 건너뛰었습니다.");
      }
      return;
    }

    try {
      await removeWorktreeAndBranch(
        project.repoPath,
        task.branchName,
        sshHost,
        worktreeCleanupOptions,
      );
    } catch (error) {
      if (options.throwOnError) {
        throw error;
      }
      console.error("worktree/브랜치 정리 실패:", error);
    }
  }
}

/** task 삭제 액션에서 사용하는 리소스 삭제 정책 */
async function deleteTaskResources(task: KanbanTask): Promise<void> {
  await cleanupTaskResources(task, TASK_RESOURCE_DELETE_CLEANUP_OPTIONS);
}

/** 작업을 삭제한다. worktree와 세션이 있으면 함께 정리한다 */
export async function deleteTask(taskId: string): Promise<boolean> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task) return false;

  await deleteTaskResources(task);

  await repo.remove(task);
  broadcastBoardUpdate();
  return true;
}

/** 여러 작업을 삭제한다. worktree와 세션이 있으면 함께 정리하고 board update는 한 번만 보낸다 */
export async function deleteTasks(taskIds: string[]): Promise<string[]> {
  const uniqueTaskIds = [...new Set(taskIds)];
  if (uniqueTaskIds.length === 0) {
    return [];
  }

  const repo = await getTaskRepository();
  const tasks = await repo.find({
    where: { id: In(uniqueTaskIds) },
  });
  const taskById = new Map(tasks.map((task) => [task.id, task]));
  const deletedTaskIds: string[] = [];

  try {
    for (const taskId of uniqueTaskIds) {
      const task = taskById.get(taskId);
      if (!task) {
        continue;
      }

      await deleteTaskResources(task);
      await repo.remove(task);
      deletedTaskIds.push(task.id);
    }
  } finally {
    if (deletedTaskIds.length > 0) {
      broadcastBoardUpdate();
    }
  }

  return deletedTaskIds;
}

/**
 * 기존 작업에서 브랜치를 분기한다.
 * worktree + 세션을 생성하고 상태를 progress로 변경한다.
 */
export async function branchFromTask(
  taskId: string,
  projectId: string,
  baseBranch: string,
  branchName: string,
  sessionType: SessionType
): Promise<KanbanTask | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task) return null;

  const projectRepo = await getProjectRepository();
  const project = await projectRepo.findOneBy({ id: projectId });
  if (!project) return null;

  const session = await createWorktreeWithSession(
    project.repoPath,
    branchName,
    baseBranch,
    sessionType,
    project.sshHost,
    projectId
  );

  task.projectId = projectId;
  task.branchName = branchName;
  task.baseBranch = baseBranch;
  task.sessionType = sessionType;
  task.sessionName = session.sessionName;
  task.worktreePath = session.worktreePath;
  task.sshHost = project.sshHost;
  task.status = TaskStatus.PROGRESS;

  const saved = await repo.save(task);

  if (session.worktreePath) {
    await installTaskHookFilesImmediately(
      session.worktreePath,
      saved,
      "Hooks 설정 실패",
    );
  }

  await persistTaskState(saved);
  broadcastBoardUpdate();
  return serialize(saved);
}

/**
 * 세션이 없는 태스크에 터미널 세션을 연결한다.
 * worktree를 생성하지 않고 기존 디렉토리(프로젝트 루트 또는 worktree 경로)에 window/tab을 생성한다.
 */
export async function connectTerminalSession(
  taskId: string,
  sessionType: SessionType
): Promise<KanbanTask | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task || !task.projectId) return null;

  const branchForSession = task.branchName || task.baseBranch;
  if (!branchForSession) return null;

  const projectRepo = await getProjectRepository();
  const project = await projectRepo.findOneBy({ id: task.projectId });
  if (!project) return null;

  const workingDir = task.worktreePath || project.repoPath;

  try {
    await installTaskHookFilesBeforeTerminalAttach(workingDir, {
      id: task.id,
      title: task.title,
      sshHost: project.sshHost,
    });

    const session = await createSessionWithoutWorktree(
      project.repoPath,
      branchForSession,
      sessionType,
      project.sshHost,
      workingDir,
      task.projectId,
    );

    task.sessionType = sessionType;
    task.sessionName = session.sessionName;
    task.worktreePath = workingDir;
    task.sshHost = project.sshHost;

    const saved = await repo.save(task);
    await persistTaskState(saved);
    broadcastBoardUpdate();
    return serialize(saved);
  } catch (error) {
    console.error("터미널 세션 생성 실패:", error);
    return null;
  }
}

/**
 * 태스크를 다른 컬럼으로 이동할 때 사용한다. revalidation 없이 DB만 갱신한다.
 *
 * 목적지 컬럼에서의 자리는 저장하지 않는다. 보드 순서는 사용자가 고른 정렬 기준으로만 정해지고,
 * 기준이 없으면 최근 수정순이므로 방금 옮긴 카드가 목적지 맨 위에 온다.
 */
export async function moveTaskToColumn(
  taskId: string,
  newStatus: TaskStatus
): Promise<void> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });

  try {
    if (newStatus === TaskStatus.DONE) {
      if (task) {
        await repo.update(taskId, {
          status: newStatus,
          sessionType: task.sessionType,
          sessionName: task.sessionName,
          worktreePath: task.worktreePath,
        });
      }
    } else {
      await repo.update(taskId, { status: newStatus });
    }

    if (task) {
      await persistTaskState({ ...task, status: newStatus });
    }

    broadcastBoardUpdate();
  } catch (error) {
    if (newStatus === TaskStatus.DONE && task) {
      await repo.update(taskId, {
        status: task.status,
        sessionType: task.sessionType,
        sessionName: task.sessionName,
        worktreePath: task.worktreePath,
        sshHost: task.sshHost,
      });
      await persistTaskState(task);
      broadcastBoardUpdate();
    }
    throw error;
  }
}

/**
 * 작업의 브랜치에 연결된 PR URL을 조회하여 DB에 저장한다.
 * `gh pr list` CLI를 사용하며, PR이 없으면 null을 유지한다.
 */
export async function fetchAndSavePrUrl(taskId: string): Promise<string | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task?.branchName) return null;

  try {
    const { cwd, sshHost } = await resolveTaskGitContext(task);
    const prUrl = await getPrUrlFromGitHubCli(task.branchName, cwd, sshHost);

    if (prUrl) {
      task.prUrl = prUrl;
      await repo.save(task);
      broadcastBoardUpdate();
      return prUrl;
    }
  } catch (error) {
    console.error("PR URL 조회 실패:", error);
  }

  return null;
}

export async function syncActiveTaskPullRequests(
  emittedMergeEventKeys: Set<string>,
): Promise<ActiveTaskPullRequestSyncResult> {
  const repo = await getTaskRepository();
  const projectRepo = await getProjectRepository();
  const tasks = await repo.find({
    where: { status: Not(TaskStatus.DONE) },
    order: { updatedAt: "ASC" },
  });
  const result: ActiveTaskPullRequestSyncResult = {
    updatedTaskIds: [],
    mergeEventKeys: [],
    mergedPullRequests: [],
  };
  const failures: BackgroundSyncFailurePayload[] = [];

  for (const task of tasks) {
    if (!task.branchName) {
      continue;
    }

    try {
      const project = task.projectId
        ? await projectRepo.findOneBy({ id: task.projectId })
        : null;

      if (isDefaultBranchTask(task, project)) {
        continue;
      }

      const { cwd, sshHost } = await resolveTaskGitContext(task, project);
      const prInfo = await getPrInfoFromGitHubCli(task.branchName, cwd, sshHost);

      if (!prInfo?.url) {
        continue;
      }

      const updatedTaskIds: string[] = [];
      const mergeEventKeys: string[] = [];
      const mergedPullRequests: TaskPrMergedDetectedPayload[] = [];

      if (task.prUrl !== prInfo.url) {
        task.prUrl = prInfo.url;
        await repo.save(task);
        updatedTaskIds.push(task.id);
      }

      if (prInfo.state === "MERGED" && prInfo.mergedAt) {
        const mergeEventKey = buildMergedPullRequestEventKey(task.id, prInfo.url, prInfo.mergedAt);
        if (!emittedMergeEventKeys.has(mergeEventKey)) {
          emittedMergeEventKeys.add(mergeEventKey);
          mergeEventKeys.push(mergeEventKey);
          mergedPullRequests.push({
            taskId: task.id,
            taskTitle: task.title,
            branchName: task.branchName,
            prUrl: prInfo.url,
            mergedAt: prInfo.mergedAt,
          });
        }
      }

      result.updatedTaskIds.push(...updatedTaskIds);
      result.mergeEventKeys.push(...mergeEventKeys);
      result.mergedPullRequests.push(...mergedPullRequests);
    } catch (error) {
      failures.push(buildPullRequestSyncFailure(task, error));
      console.error("PR 상태 동기화 실패:", {
        taskId: task.id,
        branchName: task.branchName,
        error: getErrorMessage(error),
      });
    }
  }

  if (result.mergedPullRequests.length > 0) {
    broadcastTaskPrMergedDetectedBatch({
      mergedPullRequests: result.mergedPullRequests,
    });
  }

  if (failures.length > 0) {
    result.failures = failures;
  }

  return result;
}

export async function syncActiveTaskPulls(): Promise<ActiveTaskPullSyncResult> {
  const repo = await getTaskRepository();
  const projectRepo = await getProjectRepository();
  const tasks = await repo.find({
    where: { status: In(ACTIVE_PULL_TASK_STATUSES) },
    order: { updatedAt: "ASC" },
  });

  const pulledTasks: TaskPullSyncPayload[] = [];

  for (const task of tasks) {
    if (task.status === TaskStatus.DONE) {
      continue;
    }

    if (!task.branchName || !task.worktreePath) {
      continue;
    }

    const project = task.projectId
      ? await projectRepo.findOneBy({ id: task.projectId })
      : null;

    if (isDefaultBranchTask(task, project)) {
      continue;
    }

    const sshHost = task.sshHost || project?.sshHost || null;
    const pullFailureKey = buildPullFailureKey(task.id, task.branchName, task.worktreePath, sshHost);

    try {
      const hasRemoteBranch = sshHost
        ? await remoteBranchExists(task.worktreePath, task.branchName, sshHost)
        : await remoteBranchExists(
          task.worktreePath,
          task.branchName,
          sshHost,
          { timeoutMs: ACTIVE_TASK_PULL_GIT_TIMEOUT_MS },
        );
      if (!hasRemoteBranch) {
        notifiedPullFailureKeys.delete(pullFailureKey);
        continue;
      }

      const output = sshHost
        ? await pullCurrentBranch(task.worktreePath, sshHost)
        : await pullCurrentBranch(task.worktreePath, sshHost, { timeoutMs: ACTIVE_TASK_PULL_GIT_TIMEOUT_MS });
      notifiedPullFailureKeys.delete(pullFailureKey);
      if (isPullNoop(output)) {
        continue;
      }

      pulledTasks.push({
        taskId: task.id,
        taskTitle: task.title,
        branchName: task.branchName,
        worktreePath: task.worktreePath,
        sshHost,
        status: "updated",
        summary: summarizePullOutput(output),
      });
    } catch (error) {
      if (isMissingRemoteBranchPullError(error)) {
        notifiedPullFailureKeys.delete(pullFailureKey);
        continue;
      }

      if (notifiedPullFailureKeys.has(pullFailureKey)) {
        continue;
      }

      notifiedPullFailureKeys.add(pullFailureKey);
      pulledTasks.push({
        taskId: task.id,
        taskTitle: task.title,
        branchName: task.branchName,
        worktreePath: task.worktreePath,
        sshHost,
        status: "failed",
        summary: getErrorMessage(error),
      });
    }
  }

  return {
    pulledTasks,
  };
}
