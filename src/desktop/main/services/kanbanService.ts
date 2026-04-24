import { execFile } from "child_process";
import { Not, Like } from "typeorm";
import { getTaskRepository } from "@/lib/database";
import { KanbanTask, TaskStatus, SessionType } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import { createWorktreeWithSession, removeWorktreeAndBranch, createSessionWithoutWorktree, removeSessionOnly, buildManagedWorktreePath } from "@/lib/worktree";
import { getProjectRepository } from "@/lib/database";
import { broadcastBoardUpdate, broadcastTaskHookInstallFailed } from "@/lib/boardNotifier";
import { installKanvibeHooks } from "@/lib/kanvibeHooksInstaller";
import { execGit } from "@/lib/gitOperations";

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

const DONE_PAGE_SIZE = 20;

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

function quoteForShell(value: string): string {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}

function scheduleTaskHookInstall(
  targetPath: string,
  task: Pick<KanbanTask, "id" | "title" | "sshHost">,
) {
  setTimeout(() => {
    void installKanvibeHooks(targetPath, task.id, task.sshHost).catch((error) => {
      const errorMessage = getErrorMessage(error);

      console.error("새 태스크 hooks 백그라운드 설치 실패:", {
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
    });
  }, 0);
}

async function getPrUrlFromGitHubCli(branchName: string, cwd: string, sshHost?: string | null): Promise<string | null> {
  if (sshHost) {
    try {
      const output = await execGit(
        `cd ${quoteForShell(cwd)} && gh pr list --head ${quoteForShell(branchName)} --json url -q '.[0].url'`,
        sshHost,
      );
      return output.trim() || null;
    } catch (error) {
      if (isMissingGitHubCli(error)) {
        return null;
      }

      throw error;
    }
  }

  return new Promise((resolve, reject) => {
    execFile(
      "gh",
      ["pr", "list", "--head", branchName, "--json", "url", "-q", ".[0].url"],
      { cwd },
      (error, stdout) => {
        if (error) {
          if (isMissingGitHubCli(error)) {
            resolve(null);
            return;
          }

          reject(error);
          return;
        }

        resolve(stdout.trim() || null);
      },
    );
  });
}

/** 모든 작업을 상태별로 그룹핑하여 반환한다. Done 컬럼은 첫 페이지만 로드한다 */
export async function getTasksByStatus(): Promise<TasksByStatusWithMeta> {
  const repo = await getTaskRepository();

  const nonDoneTasks = await repo.find({
    where: { status: Not(TaskStatus.DONE) },
    order: { displayOrder: "ASC", createdAt: "ASC" },
  });

  const [doneTasks, doneTotal] = await repo.findAndCount({
    where: { status: TaskStatus.DONE },
    order: { displayOrder: "ASC", createdAt: "ASC" },
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
    order: { displayOrder: "ASC", createdAt: "ASC" },
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
  const maxDisplayOrderPromise = repo
    .createQueryBuilder("t")
    .select("MAX(t.displayOrder)", "max")
    .where("t.status = :status", { status: TaskStatus.TODO })
    .getRawOne();

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

  const maxResult = await maxDisplayOrderPromise;
  task.displayOrder = (maxResult?.max ?? -1) + 1;

  const saved = await repo.save(task);

  broadcastBoardUpdate();

  if (shouldInstallHooks && hookTargetPath) {
    scheduleTaskHookInstall(hookTargetPath, {
      id: saved.id,
      title: saved.title,
      sshHost: saved.sshHost,
    });
  }

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

  if (newStatus === TaskStatus.DONE) {
    await cleanupTaskResources(task);
    task.sessionType = null;
    task.sessionName = null;
    task.worktreePath = null;
  }

  task.status = newStatus;
  const saved = await repo.save(task);
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
  return serialize(saved);
}

/** 프로젝트의 color(hex)를 변경하고, 같은 repo의 worktree 프로젝트에도 동일하게 반영한다 */
export async function updateProjectColor(
  projectId: string,
  color: string
): Promise<void> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return;

  project.color = color;
  await repo.save(project);

  // worktree 프로젝트들의 color도 함께 업데이트한다
  const mainRepoPath = project.repoPath.includes("__worktrees")
    ? project.repoPath.split("__worktrees")[0]
    : project.repoPath;

  const relatedProjects = await repo.find({
    where: { repoPath: Like(`${mainRepoPath}%`) },
  });

  for (const related of relatedProjects) {
    if (related.id === projectId) continue;
    related.color = color;
    await repo.save(related);
  }

}

/** 작업에 연결된 worktree, 세션, 브랜치를 정리한다. task 레코드는 삭제하지 않는다 */
export async function cleanupTaskResources(task: KanbanTask): Promise<void> {
  let project = null;
  if (task.projectId) {
    const projectRepo = await getProjectRepository();
    project = await projectRepo.findOneBy({ id: task.projectId });
  }

  const sshHost = task.sshHost || project?.sshHost || null;

  // 프로젝트 없이 브랜치/worktree만 남은 태스크는 stale 상태이므로 정리를 시도하면 잘못된 원격 세션에 접근할 수 있다.
  if (!project && task.branchName && task.worktreePath) {
    return;
  }

  const isProjectRoot = project && task.worktreePath === project.repoPath;
  const expectedWorktreePath = project?.repoPath && task.branchName
    ? buildManagedWorktreePath(project.repoPath, task.branchName)
    : null;

  /** 브랜치별 독립 세션 정리 */
  if (task.sessionType && task.sessionName) {
    try {
      await removeSessionOnly(
        task.sessionType,
        task.sessionName,
        sshHost
      );
    } catch (error) {
      console.error("세션 정리 실패:", error);
    }
  }

  /** worktree + 브랜치 정리 (프로젝트 루트 브랜치 제외) */
  if (task.branchName && !isProjectRoot) {
    const canCleanupBranch = Boolean(project?.repoPath)
      && Boolean(task.worktreePath)
      && task.worktreePath === expectedWorktreePath;

    if (!canCleanupBranch) {
      console.warn("worktree/브랜치 정리 건너뜀: task 경로와 project 경로가 일치하지 않습니다.", {
        taskId: task.id,
        branchName: task.branchName,
        worktreePath: task.worktreePath,
        projectRepoPath: project?.repoPath ?? null,
        sshHost,
      });
      return;
    }

    try {
      await removeWorktreeAndBranch(
        project?.repoPath || process.cwd(),
        task.branchName,
        sshHost
      );
    } catch (error) {
      console.error("worktree/브랜치 정리 실패:", error);
    }
  }
}

/** 작업을 삭제한다. worktree와 세션이 있으면 함께 정리한다 */
export async function deleteTask(taskId: string): Promise<boolean> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task) return false;

  await cleanupTaskResources(task);

  await repo.remove(task);
  broadcastBoardUpdate();
  return true;
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
    try {
      await installKanvibeHooks(session.worktreePath, saved.id, project.sshHost);
    } catch (error) {
      console.error("Hooks 설정 실패:", error);
    }
  }

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
    const session = await createSessionWithoutWorktree(
      project.repoPath,
      branchForSession,
      sessionType,
      project.sshHost,
      workingDir,
    );

    task.sessionType = sessionType;
    task.sessionName = session.sessionName;
    task.worktreePath = workingDir;
    task.sshHost = project.sshHost;
    task.status = TaskStatus.PROGRESS;

    const saved = await repo.save(task);
    broadcastBoardUpdate();
    return serialize(saved);
  } catch (error) {
    console.error("터미널 세션 생성 실패:", error);
    return null;
  }
}

/** 컬럼 내 작업 순서를 변경한다 */
export async function reorderTasks(
  status: TaskStatus,
  orderedIds: string[]
): Promise<void> {
  const repo = await getTaskRepository();

  const updates = orderedIds.map((id, index) =>
    repo.update(id, { displayOrder: index })
  );

  await Promise.all(updates);
}

/** 드래그로 태스크를 다른 컬럼으로 이동할 때 사용한다. revalidation 없이 DB만 갱신한다 */
export async function moveTaskToColumn(
  taskId: string,
  newStatus: TaskStatus,
  destOrderedIds: string[]
): Promise<void> {
  const repo = await getTaskRepository();

  if (newStatus === TaskStatus.DONE) {
    const task = await repo.findOneBy({ id: taskId });
    if (task) {
        await cleanupTaskResources(task);
        await repo.update(taskId, {
          status: newStatus,
          sessionType: null,
          sessionName: null,
          worktreePath: null,
        });
    }
  } else {
    await repo.update(taskId, { status: newStatus });
  }

  const reorderUpdates = destOrderedIds.map((id, index) =>
    repo.update(id, { displayOrder: index })
  );

  await Promise.all(reorderUpdates);
}

/**
 * 작업의 브랜치에 연결된 PR URL을 조회하여 DB에 저장한다.
 * `gh pr list` CLI를 사용하며, PR이 없으면 null을 유지한다.
 */
export async function fetchAndSavePrUrl(taskId: string): Promise<string | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task?.branchName) return null;

  let repoPath: string | null = task.worktreePath || null;
  let sshHost: string | null = task.sshHost || null;
  if (task.projectId) {
    const projectRepo = await getProjectRepository();
    const project = await projectRepo.findOneBy({ id: task.projectId });
    if (project) {
      repoPath = repoPath ?? project.repoPath;
      sshHost = project.sshHost;
    }
  }

  try {
    const cwd = repoPath ?? process.cwd();
    const prUrl = await getPrUrlFromGitHubCli(task.branchName, cwd, sshHost);

    if (prUrl) {
      task.prUrl = prUrl;
      await repo.save(task);
      return prUrl;
    }
  } catch (error) {
    console.error("PR URL 조회 실패:", error);
  }

  return null;
}
