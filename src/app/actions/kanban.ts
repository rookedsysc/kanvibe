"use server";

import { revalidatePath } from "next/cache";
import { exec } from "child_process";
import { promisify } from "util";
import { Not } from "typeorm";
import { getTaskRepository } from "@/lib/database";
import { KanbanTask, TaskStatus, SessionType } from "@/entities/KanbanTask";
import { createWorktreeWithSession, removeWorktreeAndSession, removeWorktreeAndBranch, createSessionWithoutWorktree, removeSessionOnly } from "@/lib/worktree";
import { getProjectRepository } from "@/lib/database";
import { setupClaudeHooks } from "@/lib/claudeHooksSetup";

const execAsync = promisify(exec);

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
  const task = await repo.findOneBy({ id: taskId });
  return task ? serialize(task) : null;
}

export interface CreateTaskInput {
  title: string;
  description?: string;
  branchName?: string;
  baseBranch?: string;
  sessionType?: SessionType;
  sshHost?: string;
  projectId?: string;
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
    status: TaskStatus.TODO,
  });

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
        task.status = TaskStatus.PROGRESS;

        /** 로컬 worktree에 Claude Code hooks를 자동 설정한다 */
        if (!project.sshHost && session.worktreePath) {
          const kanvibeUrl = `http://localhost:${process.env.PORT || 4885}`;
          await setupClaudeHooks(session.worktreePath, project.name, kanvibeUrl);
        }
      }
    } catch (error) {
      console.error("Worktree/세션 생성 실패:", error);
    }
  }

  const saved = await repo.save(task);
  revalidatePath("/[locale]", "page");
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
    task.sshHost = null;
  }

  task.status = newStatus;
  const saved = await repo.save(task);
  revalidatePath("/[locale]", "page");
  return serialize(saved);
}

/** 작업의 정보를 부분 업데이트한다 */
export async function updateTask(
  taskId: string,
  updates: Partial<Pick<KanbanTask, "title" | "description">>
): Promise<KanbanTask | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task) return null;

  if (updates.title !== undefined) task.title = updates.title;
  if (updates.description !== undefined) task.description = updates.description;

  const saved = await repo.save(task);
  revalidatePath("/[locale]", "page");
  revalidatePath("/[locale]/task/[id]", "page");
  return serialize(saved);
}

/** 작업에 연결된 worktree, 세션, 브랜치를 정리한다. task 레코드는 삭제하지 않는다 */
export async function cleanupTaskResources(task: KanbanTask): Promise<void> {
  let project = null;
  if (task.projectId) {
    const projectRepo = await getProjectRepository();
    project = await projectRepo.findOneBy({ id: task.projectId });
  }

  const isProjectRoot = project && task.worktreePath === project.repoPath;
  const derivedBranch = task.branchName || task.baseBranch;

  /** 세션(window/tab) 정리 */
  if (task.sessionType && task.sessionName) {
    try {
      if (derivedBranch) {
        await removeSessionOnly(
          task.sessionType,
          task.sessionName,
          derivedBranch,
          task.sshHost
        );
      }
    } catch (error) {
      console.error("세션 정리 실패:", error);
    }
  }

  /** worktree + 브랜치 정리 (프로젝트 루트 브랜치 제외) */
  if (task.branchName && !isProjectRoot) {
    try {
      await removeWorktreeAndBranch(
        project?.repoPath || process.cwd(),
        task.branchName,
        task.sshHost
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
  revalidatePath("/[locale]", "page");
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

  /** 로컬 worktree에 Claude Code hooks를 자동 설정한다 */
  if (!project.sshHost && session.worktreePath) {
    try {
      const kanvibeUrl = `http://localhost:${process.env.PORT || 4885}`;
      await setupClaudeHooks(session.worktreePath, project.name, kanvibeUrl);
    } catch (error) {
      console.error("Claude hooks 설정 실패:", error);
    }
  }

  const saved = await repo.save(task);
  revalidatePath("/[locale]", "page");
  revalidatePath("/[locale]/task/[id]", "page");
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

  const branchForWindow = task.branchName || task.baseBranch;
  if (!branchForWindow) return null;

  const projectRepo = await getProjectRepository();
  const project = await projectRepo.findOneBy({ id: task.projectId });
  if (!project) return null;

  const workingDir = task.worktreePath || project.repoPath;

  try {
    const session = await createSessionWithoutWorktree(
      project.repoPath,
      branchForWindow,
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
    revalidatePath("/[locale]", "page");
    revalidatePath("/[locale]/task/[id]", "page");
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
  revalidatePath("/[locale]", "page");
}

/**
 * 작업의 브랜치에 연결된 PR URL을 조회하여 DB에 저장한다.
 * `gh pr list` CLI를 사용하며, PR이 없으면 null을 유지한다.
 */
export async function fetchAndSavePrUrl(taskId: string): Promise<string | null> {
  const repo = await getTaskRepository();
  const task = await repo.findOneBy({ id: taskId });
  if (!task?.branchName) return null;

  let repoPath: string | null = null;
  if (task.projectId) {
    const projectRepo = await getProjectRepository();
    const project = await projectRepo.findOneBy({ id: task.projectId });
    if (project) repoPath = project.repoPath;
  }

  try {
    const cwd = repoPath ?? process.cwd();
    const { stdout } = await execAsync(
      `gh pr list --head "${task.branchName}" --json url -q ".[0].url"`,
      { cwd }
    );
    const prUrl = stdout.trim();

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
