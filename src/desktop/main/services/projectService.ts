import { getProjectRepository, getTaskRepository } from "@/lib/database";
import { Project } from "@/entities/Project";
import { validateGitRepo, getDefaultBranch, listBranches, scanGitRepos, listWorktrees, execGit } from "@/lib/gitOperations";
import { TaskStatus, SessionType } from "@/entities/KanbanTask";
import { IsNull } from "typeorm";
import { isSessionAlive, formatSessionName, createSessionWithoutWorktree } from "@/lib/worktree";
import { setupClaudeHooks, getClaudeHooksStatus, type ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import { setupGeminiHooks, getGeminiHooksStatus, type GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import { setupCodexHooks, getCodexHooksStatus, type CodexHooksStatus } from "@/lib/codexHooksSetup";
import { setupOpenCodeHooks, getOpenCodeHooksStatus, type OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";
import { aggregateAiSessions, getAiSessionDetail } from "@/lib/aiSessions/aggregateAiSessions";
import type { AggregatedAiSessionDetail, AggregatedAiSessionsResult, AiMessageRole, AiSessionProvider } from "@/lib/aiSessions/types";
import { homedir } from "os";
import path from "path";
import { computeProjectColor } from "@/lib/projectColor";
import { broadcastBoardUpdate } from "@/lib/boardNotifier";
import { getAvailableHosts as readAvailableHosts } from "@/lib/sshConfig";

/** TypeORM 엔티티를 직렬화 가능한 plain object로 변환한다 */
function serialize<T>(data: T): T {
  return JSON.parse(JSON.stringify(data));
}

/** 프로젝트의 메인 브랜치 태스크를 생성하고 tmux 세션을 자동 연결한다 */
async function createDefaultBranchTask(project: Project): Promise<void> {
  const taskRepo = await getTaskRepository();

  /** 해당 프로젝트에 이미 기본 브랜치 태스크가 있으면 생성하지 않는다 */
  const existing = await taskRepo.findOneBy({ branchName: project.defaultBranch, projectId: project.id });
  if (existing) return;

  /** orphan 태스크(projectId 없음)가 있으면 현재 프로젝트에 연결한다 */
  const orphan = await taskRepo.findOneBy({ branchName: project.defaultBranch, projectId: IsNull() });
  if (orphan) {
    orphan.projectId = project.id;
    orphan.baseBranch = project.defaultBranch;
    await taskRepo.save(orphan);
    return;
  }

  const task = taskRepo.create({
    title: project.defaultBranch,
    branchName: project.defaultBranch,
    status: TaskStatus.TODO,
    projectId: project.id,
    baseBranch: project.defaultBranch,
  });

  try {
    const session = await createSessionWithoutWorktree(
      project.repoPath,
      project.defaultBranch,
      SessionType.TMUX,
      project.sshHost,
      project.repoPath,
    );
    task.sessionType = SessionType.TMUX;
    task.sessionName = session.sessionName;
    task.worktreePath = project.repoPath;
    task.sshHost = project.sshHost;
    task.status = TaskStatus.PROGRESS;
  } catch (error) {
    console.error("메인 브랜치 tmux 세션 생성 실패:", error);
  }

  await taskRepo.save(task);
}

/** 등록된 모든 프로젝트를 반환한다 */
export async function getAllProjects(): Promise<Project[]> {
  const repo = await getProjectRepository();
  const projects = await repo.find({ order: { createdAt: "ASC" } });
  return serialize(projects);
}

export async function getAvailableHosts(): Promise<string[]> {
  return readAvailableHosts();
}

/** 단일 프로젝트를 ID로 조회한다 */
export async function getProjectById(projectId: string): Promise<Project | null> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  return project ? serialize(project) : null;
}

/**
 * 새 프로젝트를 등록한다.
 * git 저장소 유효성을 검증하고 기본 브랜치를 자동 감지한다.
 */
export async function registerProject(
  name: string,
  repoPath: string,
  sshHost?: string
): Promise<{ success: boolean; error?: string; project?: Project }> {
  if (!name || !repoPath) {
    return { success: false, error: "이름과 경로는 필수입니다." };
  }

  const isValid = await validateGitRepo(repoPath, sshHost || null);
  if (!isValid) {
    return { success: false, error: "유효한 git 저장소가 아닙니다." };
  }

  const repo = await getProjectRepository();

  const existing = await repo.findOneBy({ name });
  if (existing) {
    return { success: false, error: "이미 같은 이름의 프로젝트가 있습니다." };
  }

  const defaultBranch = await getDefaultBranch(repoPath, sshHost || null);

  const project = repo.create({
    name,
    repoPath,
    defaultBranch,
    sshHost: sshHost || null,
    color: computeProjectColor(name),
  });

  const saved = await repo.save(project);
  await createDefaultBranchTask(saved);
  broadcastBoardUpdate();
  return { success: true, project: serialize(saved) };
}

/** 프로젝트를 삭제한다. 연결된 작업의 projectId는 FK cascade로 null이 된다 */
export async function deleteProject(projectId: string): Promise<boolean> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return false;

  await repo.remove(project);
  broadcastBoardUpdate();
  return true;
}

export interface ScanResult {
  registered: string[];
  skipped: string[];
  errors: string[];
  worktreeTasks: string[];
  hooksSetup: string[];
}

/**
 * 지정 디렉토리 하위의 git 저장소를 스캔하여 미등록 프로젝트를 일괄 등록한다.
 * 이미 동일 경로로 등록된 프로젝트는 건너뛰고, 이름 중복 시 상위 디렉토리를 포함하여 구분한다.
 */
export async function scanAndRegisterProjects(
  rootPath: string,
  sshHost?: string
): Promise<ScanResult> {
  const result: ScanResult = { registered: [], skipped: [], errors: [], worktreeTasks: [], hooksSetup: [] };

  const repoPaths = await scanGitRepos(rootPath, sshHost || null);
  if (repoPaths.length === 0) {
    return result;
  }

  const repo = await getProjectRepository();
  const existing = await repo.find();
  const existingPaths = new Set(
    existing.map((p) => `${p.sshHost || ""}:${p.repoPath}`)
  );
  const existingNames = new Set(existing.map((p) => p.name));

  /** 폴더명 기반 프로젝트 이름을 생성한다. 중복 시 상위 디렉토리를 포함한다 */
  function resolveProjectName(repoPath: string): string {
    const baseName = path.basename(repoPath);
    if (!existingNames.has(baseName)) {
      existingNames.add(baseName);
      return baseName;
    }

    const parentName = path.basename(path.dirname(repoPath));
    const combinedName = `${parentName}/${baseName}`;
    if (!existingNames.has(combinedName)) {
      existingNames.add(combinedName);
      return combinedName;
    }

    let counter = 2;
    while (existingNames.has(`${baseName}-${counter}`)) {
      counter++;
    }
    const numberedName = `${baseName}-${counter}`;
    existingNames.add(numberedName);
    return numberedName;
  }

  for (const repoPath of repoPaths) {
    const pathKey = `${sshHost || ""}:${repoPath}`;
    if (existingPaths.has(pathKey)) {
      result.skipped.push(repoPath);
      continue;
    }

    try {
      const defaultBranch = await getDefaultBranch(repoPath, sshHost || null);
      const projectName = resolveProjectName(repoPath);

      const project = repo.create({
        name: projectName,
        repoPath,
        defaultBranch,
        sshHost: sshHost || null,
        color: computeProjectColor(projectName),
      });

      const saved = await repo.save(project);
      existingPaths.add(pathKey);
      result.registered.push(projectName);

      /** 기본 브랜치 태스크 생성 실패는 프로젝트 등록 결과에 영향을 주지 않는다 */
      try {
        await createDefaultBranchTask(saved);
      } catch (taskError) {
        console.error(`${projectName} 기본 브랜치 태스크 생성 실패:`, taskError);
      }

      /** 로컬 repo에 Claude Code / Gemini CLI / Codex CLI hooks를 자동 설정한다 */
      if (!sshHost) {
        try {
          const kanvibeUrl = "http://localhost:9736";
          await setupClaudeHooks(repoPath, project.id, kanvibeUrl);
          await setupGeminiHooks(repoPath, project.id, kanvibeUrl);
          await setupCodexHooks(repoPath, project.id, kanvibeUrl);
          await setupOpenCodeHooks(repoPath, project.id, kanvibeUrl);
          result.hooksSetup.push(projectName);
        } catch (hookError) {
          result.errors.push(
            `${projectName} hooks 설정 실패: ${hookError instanceof Error ? hookError.message : "알 수 없는 오류"}`
          );
        }
      }
    } catch (error) {
      result.errors.push(
        `${repoPath}: ${error instanceof Error ? error.message : "등록 실패"}`
      );
    }
  }

  /** 등록된 모든 프로젝트의 worktree를 스캔하여 미등록 브랜치를 TODO task로 생성한다 */
  const allProjects = await repo.find();
  const taskRepo = await getTaskRepository();

  for (const project of allProjects) {
    try {
      const worktrees = await listWorktrees(project.repoPath, project.sshHost);

      for (const wt of worktrees) {
        if (wt.isBare || !wt.branch) continue;

        /** 메인 작업 디렉토리(프로젝트 루트)는 기본 브랜치 태스크와 중복되므로 건너뛴다 */
        if (wt.path === project.repoPath) continue;

        /** 해당 프로젝트에 이미 동일 브랜치 태스크가 있으면 건너뛴다 */
        const existingTask = await taskRepo.findOneBy({ branchName: wt.branch, projectId: project.id });
        if (existingTask) continue;

        /** orphan 태스크(projectId 없음)가 있으면 현재 프로젝트에 연결한다 */
        const orphanTask = await taskRepo.findOneBy({ branchName: wt.branch, projectId: IsNull() });
        if (orphanTask) {
          orphanTask.projectId = project.id;
          orphanTask.worktreePath = wt.path;
          orphanTask.baseBranch = orphanTask.baseBranch || project.defaultBranch;
          await taskRepo.save(orphanTask);
          result.worktreeTasks.push(wt.branch);
          continue;
        }

        /** 브랜치명 기반 독립 세션이 존재하면 연결 정보를 설정한다 */
        const sessionName = formatSessionName(path.basename(project.repoPath), wt.branch);
        const hasSession = await isSessionAlive(
          SessionType.TMUX,
          sessionName,
          project.sshHost
        );

        const task = taskRepo.create({
          title: wt.branch,
          branchName: wt.branch,
          worktreePath: wt.path,
          projectId: project.id,
          baseBranch: project.defaultBranch,
          status: hasSession ? TaskStatus.PROGRESS : TaskStatus.TODO,
          ...(hasSession && {
            sessionType: SessionType.TMUX,
            sessionName,
            sshHost: project.sshHost,
          }),
        });
        await taskRepo.save(task);
        result.worktreeTasks.push(wt.branch);
      }
    } catch (error) {
      result.errors.push(
        `${project.name} worktree 스캔 실패: ${error instanceof Error ? error.message : "알 수 없는 오류"}`
      );
    }

    /** 메인 브랜치 태스크에 활성 tmux 세션이 있으면 자동 연결한다 */
    try {
      const mainBranchTask = await taskRepo.findOneBy({
        projectId: project.id,
        baseBranch: project.defaultBranch,
        branchName: project.defaultBranch,
      });

      if (mainBranchTask && !mainBranchTask.sessionType) {
        const sessionName = formatSessionName(path.basename(project.repoPath), project.defaultBranch);
        const hasSession = await isSessionAlive(
          SessionType.TMUX,
          sessionName,
          project.sshHost,
        );

        if (hasSession) {
          mainBranchTask.sessionType = SessionType.TMUX;
          mainBranchTask.sessionName = sessionName;
          mainBranchTask.worktreePath = project.repoPath;
          mainBranchTask.sshHost = project.sshHost;
          mainBranchTask.status = TaskStatus.PROGRESS;
          await taskRepo.save(mainBranchTask);
        }
      }
    } catch (error) {
      console.error(`${project.name} 메인 브랜치 세션 감지 실패:`, error);
    }
  }

  broadcastBoardUpdate();
  return result;
}

/** 지정 디렉토리의 직속 하위 디렉토리 이름 목록을 반환한다 (fzf drill-down 탐색용) */
export async function listSubdirectories(
  parentPath: string,
  sshHost?: string
): Promise<string[]> {
  const resolvedPath = parentPath.startsWith("~")
    ? parentPath.replace(/^~/, homedir())
    : parentPath;

  try {
    const output = await execGit(
      `find "${resolvedPath}" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort`,
      sshHost || null
    );

    if (!output) return [];

    return output
      .split("\n")
      .filter(Boolean)
      .map((dir) => path.basename(dir))
      .filter((name) => !name.startsWith("."));
  } catch {
    return [];
  }
}

/** 프로젝트의 브랜치 목록을 반환한다 */
export async function getProjectBranches(projectId: string): Promise<string[]> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return [];

  return listBranches(project.repoPath, project.sshHost);
}

/** 프로젝트의 Claude Code hooks 설치 상태를 조회한다 */
export async function getProjectHooksStatus(
  projectId: string
): Promise<ClaudeHooksStatus | null> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project || project.sshHost) return null;

  return getClaudeHooksStatus(project.repoPath);
}

/** 프로젝트에 Claude Code hooks를 설치한다 */
export async function installProjectHooks(
  projectId: string
): Promise<{ success: boolean; error?: string }> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };
  if (project.sshHost) return { success: false, error: "SSH 원격 프로젝트는 지원하지 않습니다." };

  try {
    const kanvibeUrl = "http://localhost:9736";
    await setupClaudeHooks(project.repoPath, project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "hooks 설정 실패",
    };
  }
}

/** 태스크의 worktree 또는 프로젝트 경로에서 Claude Code hooks 상태를 조회한다 */
export async function getTaskHooksStatus(
  taskId: string
): Promise<ClaudeHooksStatus | null> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project || task.project.sshHost) return null;

  const targetPath = task.worktreePath || task.project.repoPath;
  return getClaudeHooksStatus(targetPath);
}

/** 태스크의 worktree 또는 프로젝트 경로에 Claude Code hooks를 설치한다 */
export async function installTaskHooks(
  taskId: string
): Promise<{ success: boolean; error?: string }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };
  if (task.project.sshHost) return { success: false, error: "SSH 원격 프로젝트는 지원하지 않습니다." };

  try {
    const kanvibeUrl = "http://localhost:9736";
    const targetPath = task.worktreePath || task.project.repoPath;
    await setupClaudeHooks(targetPath, task.project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "hooks 설정 실패",
    };
  }
}

/** 프로젝트의 Gemini CLI hooks 설치 상태를 조회한다 */
export async function getProjectGeminiHooksStatus(
  projectId: string
): Promise<GeminiHooksStatus | null> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project || project.sshHost) return null;

  return getGeminiHooksStatus(project.repoPath);
}

/** 프로젝트에 Gemini CLI hooks를 설치한다 */
export async function installProjectGeminiHooks(
  projectId: string
): Promise<{ success: boolean; error?: string }> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };
  if (project.sshHost) return { success: false, error: "SSH 원격 프로젝트는 지원하지 않습니다." };

  try {
    const kanvibeUrl = "http://localhost:9736";
    await setupGeminiHooks(project.repoPath, project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "hooks 설정 실패",
    };
  }
}

/** 태스크의 worktree 또는 프로젝트 경로에서 Gemini CLI hooks 상태를 조회한다 */
export async function getTaskGeminiHooksStatus(
  taskId: string
): Promise<GeminiHooksStatus | null> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project || task.project.sshHost) return null;

  const targetPath = task.worktreePath || task.project.repoPath;
  return getGeminiHooksStatus(targetPath);
}

/** 태스크의 worktree 또는 프로젝트 경로에 Gemini CLI hooks를 설치한다 */
export async function installTaskGeminiHooks(
  taskId: string
): Promise<{ success: boolean; error?: string }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };
  if (task.project.sshHost) return { success: false, error: "SSH 원격 프로젝트는 지원하지 않습니다." };

  try {
    const kanvibeUrl = "http://localhost:9736";
    const targetPath = task.worktreePath || task.project.repoPath;
    await setupGeminiHooks(targetPath, task.project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "hooks 설정 실패",
    };
  }
}

export async function getProjectCodexHooksStatus(
  projectId: string
): Promise<CodexHooksStatus | null> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project || project.sshHost) return null;

  return getCodexHooksStatus(project.repoPath);
}

export async function installProjectCodexHooks(
  projectId: string
): Promise<{ success: boolean; error?: string }> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };
  if (project.sshHost) return { success: false, error: "SSH 원격 프로젝트는 지원하지 않습니다." };

  try {
    const kanvibeUrl = "http://localhost:9736";
    await setupCodexHooks(project.repoPath, project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "hooks 설정 실패",
    };
  }
}

export async function getTaskCodexHooksStatus(
  taskId: string
): Promise<CodexHooksStatus | null> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project || task.project.sshHost) return null;

  const targetPath = task.worktreePath || task.project.repoPath;
  return getCodexHooksStatus(targetPath);
}

export async function installTaskCodexHooks(
  taskId: string
): Promise<{ success: boolean; error?: string }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };
  if (task.project.sshHost) return { success: false, error: "SSH 원격 프로젝트는 지원하지 않습니다." };

  try {
    const kanvibeUrl = "http://localhost:9736";
    const targetPath = task.worktreePath || task.project.repoPath;
    await setupCodexHooks(targetPath, task.project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "hooks 설정 실패",
    };
  }
}

/** 프로젝트 repo에 OpenCode hooks를 설치한다 */
export async function installProjectOpenCodeHooks(
  projectId: string
): Promise<{ success: boolean; error?: string }> {
  const projectRepo = await getProjectRepository();
  const project = await projectRepo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "Project not found" };

  try {
    const kanvibeUrl = "http://localhost:9736";
    await setupOpenCodeHooks(project.repoPath, project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "Unknown error",
    };
  }
}

/** 태스크의 worktree 또는 프로젝트 경로에 OpenCode hooks를 설치한다 */
export async function installTaskOpenCodeHooks(
  taskId: string
): Promise<{ success: boolean; error?: string }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({
    where: { id: taskId },
    relations: ["project"],
  });
  if (!task?.project) return { success: false, error: "Task or project not found" };

  try {
    const kanvibeUrl = "http://localhost:9736";
    const targetPath = task.worktreePath || task.project.repoPath;
    await setupOpenCodeHooks(targetPath, task.project.id, kanvibeUrl);
    return { success: true };
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : "Unknown error",
    };
  }
}

/** 태스크의 OpenCode hooks 설치 상태를 조회한다 */
export async function getTaskOpenCodeHooksStatus(
  taskId: string
): Promise<OpenCodeHooksStatus | null> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({
    where: { id: taskId },
    relations: ["project"],
  });
  if (!task?.project) return null;

  const targetPath = task.worktreePath || task.project.repoPath;
  return getOpenCodeHooksStatus(targetPath);
}

/** 태스크와 연결된 로컬 AI 세션들을 집계한다 */
export async function getTaskAiSessions(
  taskId: string,
  includeRepoSessions = false,
  query?: string
): Promise<AggregatedAiSessionsResult> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({
    where: { id: taskId },
    relations: ["project"],
  });

  if (!task?.project) {
    return {
      isRemote: false,
      targetPath: null,
      repoPath: null,
      sessions: [],
      sources: [],
    };
  }

  const targetPath = task.worktreePath || task.project.repoPath;
  if (task.project.sshHost) {
    return {
      isRemote: true,
      targetPath,
      repoPath: task.project.repoPath,
      sessions: [],
      sources: [],
    };
  }

  return aggregateAiSessions({
    worktreePath: targetPath,
    repoPath: task.project.repoPath,
    includeRepoSessions,
    query,
  });
}

export async function getTaskAiSessionDetail(
  taskId: string,
  provider: AiSessionProvider,
  sessionId: string,
  sourceRef?: string | null,
  cursor?: string | null,
  limit = 20,
  includeRepoSessions = false,
  query?: string,
  roles?: AiMessageRole[]
): Promise<AggregatedAiSessionDetail | null> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({
    where: { id: taskId },
    relations: ["project"],
  });

  if (!task?.project || task.project.sshHost) {
    return null;
  }

  const targetPath = task.worktreePath || task.project.repoPath;
  return getAiSessionDetail(
    {
      worktreePath: targetPath,
      repoPath: task.project.repoPath,
      includeRepoSessions,
      query,
      roles,
    },
    provider,
    sessionId,
    sourceRef,
    cursor,
    limit
  );
}
