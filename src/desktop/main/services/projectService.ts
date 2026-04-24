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
import { getDefaultSessionType } from "@/desktop/main/services/appSettingsService";
import { installKanvibeHooks } from "@/lib/kanvibeHooksInstaller";
import { getHookServerToken, getHookServerUrl } from "@/lib/hookEndpoint";

function matchesTaskLocation(task: { worktreePath?: string | null; sshHost?: string | null }, expectedPath: string, sshHost?: string | null): boolean {
  return task.worktreePath === expectedPath && (task.sshHost || null) === (sshHost || null);
}

function resolveDirectorySearchPath(targetPath: string, sshHost?: string): string {
  if (!targetPath.startsWith("~")) {
    return `"${targetPath}"`;
  }

  if (sshHost) {
    const suffix = targetPath.slice(1);
    return `"$HOME${suffix}"`;
  }

  return `"${targetPath.replace(/^~/, homedir())}"`;
}

/** TypeORM 엔티티를 직렬화 가능한 plain object로 변환한다 */
function serialize<T>(data: T): T {
  return JSON.parse(JSON.stringify(data));
}

function buildProjectPathKey(repoPath: string, sshHost?: string | null): string {
  return `${sshHost || ""}:${repoPath}`;
}

async function resolveCommonRepoPath(repoPath: string, sshHost?: string | null): Promise<string> {
  try {
    const pathModule = sshHost ? path.posix : path;
    const gitCommonDir = await execGit(
      `git -C "${repoPath}" rev-parse --path-format=absolute --git-common-dir`,
      sshHost,
    );
    const normalizedCommonDir = gitCommonDir.trim();
    if (!normalizedCommonDir) {
      return repoPath;
    }

    return pathModule.dirname(normalizedCommonDir);
  } catch {
    return repoPath;
  }
}

async function resolveProjectDefaultBranchWorktreePath(project: Pick<Project, "repoPath" | "defaultBranch" | "sshHost">): Promise<string | null> {
  try {
    const worktrees = await listWorktrees(project.repoPath, project.sshHost);
    if (!Array.isArray(worktrees)) {
      return null;
    }

    return worktrees.find((worktree) => !worktree.isBare && worktree.branch === project.defaultBranch)?.path ?? null;
  } catch {
    return null;
  }
}

function isRemoteConnectionError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error || "");
  return /원격 명령 실패:.*(Connection (?:reset|closed)|kex_exchange_identification|operation timed out|no route to host|connection refused|could not resolve hostname|broken pipe)/i.test(message);
}

const projectRootHookRepairJobs = new Map<string, Promise<void>>();
const projectRootHookRepairScheduled = new Set<string>();

function scheduleProjectRootHookRepair(project: Project) {
  const projectPathKey = buildProjectPathKey(project.repoPath, project.sshHost);
  if (projectRootHookRepairScheduled.has(projectPathKey)) {
    return;
  }

  projectRootHookRepairScheduled.add(projectPathKey);

  setTimeout(() => {
    const repairJob = (async () => {
      try {
        const { repaired } = await ensureProjectRootTask(project, {
          repairHooks: true,
          throwOnHookRepairFailure: false,
          suppressRemoteConnectionErrorLogging: true,
        });

        if (repaired) {
          broadcastBoardUpdate();
        }
      } catch (error) {
        if (!isRemoteConnectionError(error)) {
          console.error(`${project.name} 기본 브랜치 hooks 백그라운드 복구 실패:`, error);
        }
      } finally {
        projectRootHookRepairJobs.delete(projectPathKey);
        projectRootHookRepairScheduled.delete(projectPathKey);
      }
    })();

    projectRootHookRepairJobs.set(projectPathKey, repairJob);
  }, 0);
}

/** 프로젝트 표시 이름은 전역적으로 유일해야 하므로 충돌 시 경로 기반 후보명으로 보정한다 */
function resolveUniqueProjectName(
  preferredName: string,
  repoPath: string,
  existingNames: Set<string>,
): string {
  const baseName = path.basename(repoPath);
  const parentName = path.basename(path.dirname(repoPath));
  const combinedName = `${parentName}/${baseName}`;
  const candidates = [preferredName];

  if (baseName && baseName !== preferredName) {
    candidates.push(baseName);
  }

  if (combinedName && combinedName !== preferredName) {
    candidates.push(combinedName);
  }

  for (const candidate of candidates) {
    if (existingNames.has(candidate)) {
      continue;
    }

    existingNames.add(candidate);
    return candidate;
  }

  let counter = 2;
  while (existingNames.has(`${preferredName}-${counter}`)) {
    counter++;
  }

  const numberedName = `${preferredName}-${counter}`;
  existingNames.add(numberedName);
  return numberedName;
}

/** 프로젝트의 메인 브랜치 태스크를 생성하고 tmux 세션을 자동 연결한다 */
async function createDefaultBranchTask(project: Project) {
  const taskRepo = await getTaskRepository();
  const defaultSessionType = await getDefaultSessionType();
  const defaultBranchWorktreePath = await resolveProjectDefaultBranchWorktreePath(project);

  const existing = await taskRepo.findOneBy({ branchName: project.defaultBranch, projectId: project.id });
  if (existing) return existing;

  const orphan = await taskRepo.findOneBy({ branchName: project.defaultBranch, projectId: IsNull() });
  if (orphan && matchesTaskLocation(orphan, project.repoPath, project.sshHost)) {
    orphan.projectId = project.id;
    orphan.title = project.defaultBranch;
    orphan.worktreePath = project.repoPath;
    orphan.sshHost = project.sshHost;
    orphan.baseBranch = project.defaultBranch;
    return taskRepo.save(orphan);
  }

  const task = taskRepo.create({
    title: project.defaultBranch,
    branchName: project.defaultBranch,
    worktreePath: defaultBranchWorktreePath,
    sshHost: project.sshHost,
    status: TaskStatus.TODO,
    projectId: project.id,
    baseBranch: project.defaultBranch,
  });

  if (!project.sshHost && defaultBranchWorktreePath) {
    try {
      const session = await createSessionWithoutWorktree(
        project.repoPath,
        project.defaultBranch,
        defaultSessionType,
        project.sshHost,
        defaultBranchWorktreePath,
      );
      task.sessionType = defaultSessionType;
      task.sessionName = session.sessionName;
      task.worktreePath = defaultBranchWorktreePath;
      task.sshHost = project.sshHost;
      task.status = TaskStatus.PROGRESS;
    } catch (error) {
      console.error("메인 브랜치 tmux 세션 생성 실패:", error);
    }
  }

  return taskRepo.save(task);
}

async function getProjectRootTask(projectId: string, defaultBranch: string) {
  const taskRepo = await getTaskRepository();
  return taskRepo.findOneBy({ projectId, branchName: defaultBranch });
}

async function areProjectRootHooksInstalled(project: Project, taskId: string): Promise<boolean> {
  const [claudeStatus, geminiStatus, codexStatus, openCodeStatus] = await Promise.all([
    getClaudeHooksStatus(project.repoPath, taskId, project.sshHost),
    getGeminiHooksStatus(project.repoPath, taskId, project.sshHost),
    getCodexHooksStatus(project.repoPath, taskId, project.sshHost),
    getOpenCodeHooksStatus(project.repoPath, taskId, project.sshHost),
  ]);

  return claudeStatus.installed
    && geminiStatus.installed
    && codexStatus.installed
    && openCodeStatus.installed;
}

async function ensureProjectRootTask(
  project: Project,
  options?: {
    repairHooks?: boolean;
    throwOnHookRepairFailure?: boolean;
    suppressRemoteConnectionErrorLogging?: boolean;
  },
): Promise<{ task: Awaited<ReturnType<typeof getProjectRootTask>>; repaired: boolean }> {
  const taskRepo = await getTaskRepository();
  const repairHooks = options?.repairHooks === true;
  const throwOnHookRepairFailure = options?.throwOnHookRepairFailure !== false;
  const suppressRemoteConnectionErrorLogging = options?.suppressRemoteConnectionErrorLogging === true;
  let task = await getProjectRootTask(project.id, project.defaultBranch);
  let repaired = false;

  if (!task) {
    task = await createDefaultBranchTask(project);
    repaired = true;
  }

  if (!task) {
    return { task: null, repaired };
  }

  const expectedDefaultBranchWorktreePath = await resolveProjectDefaultBranchWorktreePath(project);

  let shouldSaveTask = false;
  if (task.baseBranch !== project.defaultBranch) {
    task.baseBranch = project.defaultBranch;
    shouldSaveTask = true;
  }

  if ((task.worktreePath ?? null) !== expectedDefaultBranchWorktreePath) {
    task.worktreePath = expectedDefaultBranchWorktreePath;
    shouldSaveTask = true;
  }

  if (task.sshHost !== project.sshHost) {
    task.sshHost = project.sshHost;
    shouldSaveTask = true;
  }

  if (shouldSaveTask) {
    task = await taskRepo.save(task);
    repaired = true;
  }

  if (!repairHooks) {
    return { task, repaired };
  }

  const hasInstalledHooks = await areProjectRootHooksInstalled(project, task.id);

  if (!hasInstalledHooks) {
    try {
      await installKanvibeHooks(project.repoPath, task.id, project.sshHost);
      repaired = true;
    } catch (error) {
      if (throwOnHookRepairFailure) {
        throw error;
      }

      if (!suppressRemoteConnectionErrorLogging || !isRemoteConnectionError(error)) {
        console.error(`${project.name} 기본 브랜치 hooks 복구 실패:`, error);
      }
    }
  }

  return { task, repaired };
}

async function resolveTaskHookTarget(task: {
  id: string;
  worktreePath: string | null;
  project: Project | null;
}) {
  if (!task.project) {
    return null;
  }

  const targetPath = task.worktreePath || task.project.repoPath;
  if (targetPath !== task.project.repoPath) {
    return {
      targetPath,
      taskId: task.id,
      sshHost: task.project.sshHost,
    };
  }

  const { task: projectRootTask } = await ensureProjectRootTask(task.project);
  return {
    targetPath,
    taskId: projectRootTask?.id ?? task.id,
    sshHost: task.project.sshHost,
  };
}

async function getHookInstallConfig(sshHost?: string | null) {
  return {
    kanvibeUrl: await getHookServerUrl(sshHost),
    authToken: getHookServerToken() || undefined,
  };
}

/** 등록된 모든 프로젝트를 반환한다 */
export async function getAllProjects(): Promise<Project[]> {
  const repo = await getProjectRepository();
  const projects = await repo.find({ order: { createdAt: "ASC" } });

  let repairedAnyProject = false;
  for (const project of projects) {
    try {
      const { repaired } = await ensureProjectRootTask(project);
      repairedAnyProject = repairedAnyProject || repaired;
      scheduleProjectRootHookRepair(project);
    } catch (error) {
      console.error(`${project.name} 기본 브랜치 task 복구 실패:`, error);
    }
  }

  if (repairedAnyProject) {
    broadcastBoardUpdate();
  }

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

  const normalizedRepoPath = await resolveCommonRepoPath(repoPath, sshHost || null);

  const repo = await getProjectRepository();

  const existingProjects = await repo.find();
  const pathKey = buildProjectPathKey(normalizedRepoPath, sshHost || null);
  const alreadyRegistered = existingProjects.some(
    (project) => buildProjectPathKey(project.repoPath, project.sshHost) === pathKey,
  );
  if (alreadyRegistered) {
    return { success: false, error: "이미 등록된 프로젝트입니다." };
  }

  const defaultBranch = await getDefaultBranch(normalizedRepoPath, sshHost || null);
  const projectName = resolveUniqueProjectName(
    name,
    normalizedRepoPath,
    new Set(existingProjects.map((project) => project.name)),
  );

  const project = repo.create({
    name: projectName,
    repoPath: normalizedRepoPath,
    defaultBranch,
    sshHost: sshHost || null,
    color: computeProjectColor(projectName),
  });

  const saved = await repo.save(project);
  try {
    await ensureProjectRootTask(saved, {
      repairHooks: true,
      throwOnHookRepairFailure: false,
    });
  } catch (error) {
    await repo.remove(saved);
    return {
      success: false,
      error: error instanceof Error ? error.message : "기본 태스크 생성 실패",
    };
  }

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

  const discoveredRepoPaths = await scanGitRepos(rootPath, sshHost || null);
  const repoPaths = Array.from(
    new Set(
      await Promise.all(discoveredRepoPaths.map((repoPath) => resolveCommonRepoPath(repoPath, sshHost || null))),
    ),
  );
  if (repoPaths.length === 0) {
    return result;
  }

  const scannedProjectPathKeys = new Set(
    repoPaths.map((repoPath) => buildProjectPathKey(repoPath, sshHost || null))
  );

  const repo = await getProjectRepository();
  const existing = await repo.find();
  const existingPaths = new Set(
    existing.map((project) => buildProjectPathKey(project.repoPath, project.sshHost))
  );
  const existingNames = new Set(existing.map((p) => p.name));

  for (const repoPath of repoPaths) {
    const pathKey = buildProjectPathKey(repoPath, sshHost || null);
    if (existingPaths.has(pathKey)) {
      result.skipped.push(repoPath);
      continue;
    }

    try {
      const defaultBranch = await getDefaultBranch(repoPath, sshHost || null);
      const projectName = resolveUniqueProjectName(path.basename(repoPath), repoPath, existingNames);

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

      let defaultTask = null;

      /** 기본 브랜치 태스크 생성 실패는 프로젝트 등록 결과에 영향을 주지 않는다 */
      try {
        const ensured = await ensureProjectRootTask(saved, {
          repairHooks: true,
          throwOnHookRepairFailure: false,
        });
        defaultTask = ensured.task;
      } catch (taskError) {
        await repo.remove(saved);
        existingPaths.delete(pathKey);
        result.registered = result.registered.filter((name) => name !== projectName);
        console.error(`${projectName} 기본 브랜치 태스크 생성 실패:`, taskError);
        result.errors.push(
          `${projectName} 기본 브랜치 태스크 생성 실패: ${taskError instanceof Error ? taskError.message : "알 수 없는 오류"}`,
        );
        continue;
      }

      /** 기본 브랜치 작업이 준비되면 hooks가 보장된다 */
      if (defaultTask) {
        result.hooksSetup.push(projectName);
      }
    } catch (error) {
      result.errors.push(
        `${repoPath}: ${error instanceof Error ? error.message : "등록 실패"}`
      );
    }
  }

  /** 등록된 모든 프로젝트의 worktree를 스캔하여 미등록 브랜치를 TODO task로 생성한다 */
  const scannedProjects = (await repo.find()).filter((project) => (
    scannedProjectPathKeys.has(buildProjectPathKey(project.repoPath, project.sshHost))
  ));
  const taskRepo = await getTaskRepository();

  for (const project of scannedProjects) {
    try {
      const { repaired } = await ensureProjectRootTask(project, {
        repairHooks: true,
        throwOnHookRepairFailure: false,
      });
      if (repaired && !result.hooksSetup.includes(project.name)) {
        result.hooksSetup.push(project.name);
      }

      const worktrees = await listWorktrees(project.repoPath, project.sshHost);

      for (const wt of worktrees) {
        if (wt.isBare || !wt.branch) continue;

        /** 메인 작업 디렉토리(프로젝트 루트)는 기본 브랜치 태스크와 중복되므로 건너뛴다 */
        if (wt.path === project.repoPath) continue;

        /** 해당 프로젝트에 이미 동일 브랜치 태스크가 있으면 건너뛴다 */
        const existingTask = await taskRepo.findOneBy({ branchName: wt.branch, projectId: project.id });
        if (existingTask) {
          const shouldRepairTask = !matchesTaskLocation(existingTask, wt.path, project.sshHost)
            || existingTask.baseBranch !== project.defaultBranch;
          if (shouldRepairTask) {
            existingTask.worktreePath = wt.path;
            existingTask.sshHost = project.sshHost;
            existingTask.baseBranch = project.defaultBranch;
            const savedTask = await taskRepo.save(existingTask);
            try {
              await installKanvibeHooks(wt.path, savedTask.id, project.sshHost);
            } catch (hookError) {
              result.errors.push(
                `${project.name}/${wt.branch} hooks 설정 실패: ${hookError instanceof Error ? hookError.message : "알 수 없는 오류"}`
              );
            }
          }
          continue;
        }

        /** orphan 태스크(projectId 없음)가 있으면 현재 프로젝트에 연결한다 */
        const orphanTask = await taskRepo.findOneBy({ branchName: wt.branch, projectId: IsNull() });
        if (orphanTask && matchesTaskLocation(orphanTask, wt.path, project.sshHost)) {
          orphanTask.projectId = project.id;
          orphanTask.title = wt.branch;
          orphanTask.worktreePath = wt.path;
          orphanTask.sshHost = project.sshHost;
          orphanTask.baseBranch = orphanTask.baseBranch || project.defaultBranch;
          const savedTask = await taskRepo.save(orphanTask);
          try {
            await installKanvibeHooks(wt.path, savedTask.id, project.sshHost);
          } catch (hookError) {
            result.errors.push(
              `${project.name}/${wt.branch} hooks 설정 실패: ${hookError instanceof Error ? hookError.message : "알 수 없는 오류"}`
            );
          }
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
        const savedTask = await taskRepo.save(task);
        try {
          await installKanvibeHooks(wt.path, savedTask.id, project.sshHost);
        } catch (hookError) {
          result.errors.push(
            `${project.name}/${wt.branch} hooks 설정 실패: ${hookError instanceof Error ? hookError.message : "알 수 없는 오류"}`
          );
        }
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
          const defaultBranchWorktreePath = await resolveProjectDefaultBranchWorktreePath(project);
          mainBranchTask.sessionType = SessionType.TMUX;
          mainBranchTask.sessionName = sessionName;
          mainBranchTask.worktreePath = defaultBranchWorktreePath;
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
  const resolvedPath = resolveDirectorySearchPath(parentPath, sshHost);
  const command = `find ${resolvedPath} -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort`;

  try {
    const output = await execGit(command, sshHost || null);

    if (!output) return [];

    return output
      .split("\n")
      .filter(Boolean)
      .map((dir) => path.basename(dir))
      .filter((name) => !name.startsWith("."));
  } catch (error) {
    console.error("[remote-scan] subdirectory scan failed", {
      sshHost: sshHost || null,
      parentPath,
      resolvedPath,
      command,
      error: error instanceof Error ? error.message : String(error),
    });
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
  if (!project) return null;

  const { task } = await ensureProjectRootTask(project);
  return getClaudeHooksStatus(project.repoPath, task?.id, project.sshHost);
}

/** 프로젝트에 Claude Code hooks를 설치한다 */
export async function installProjectHooks(
  projectId: string
): Promise<{ success: boolean; error?: string; status?: ClaudeHooksStatus | null }> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };

  const { task } = await ensureProjectRootTask(project);
  if (!task) return { success: false, error: "기본 브랜치 태스크를 찾을 수 없습니다." };

  try {
    if (project.sshHost) {
      await installKanvibeHooks(project.repoPath, task.id, project.sshHost);
      return { success: true, status: await getClaudeHooksStatus(project.repoPath, task.id, project.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(project.sshHost);
    await setupClaudeHooks(project.repoPath, task.id, kanvibeUrl, authToken);
    return { success: true, status: await getClaudeHooksStatus(project.repoPath, task.id, project.sshHost) };
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
  if (!task?.project) return null;

  const hookTarget = await resolveTaskHookTarget(task);
  if (!hookTarget) return null;

  return getClaudeHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
}

/** 태스크의 worktree 또는 프로젝트 경로에 Claude Code hooks를 설치한다 */
export async function installTaskHooks(
  taskId: string
): Promise<{ success: boolean; error?: string; status?: ClaudeHooksStatus | null }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };

  try {
    const hookTarget = await resolveTaskHookTarget(task);
    if (!hookTarget) {
      return { success: false, error: "프로젝트를 찾을 수 없습니다." };
    }

    if (hookTarget.sshHost) {
      await installKanvibeHooks(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
      return { success: true, status: await getClaudeHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(task.project.sshHost);
    await setupClaudeHooks(hookTarget.targetPath, hookTarget.taskId, kanvibeUrl, authToken);
    return { success: true, status: await getClaudeHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
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
  if (!project) return null;

  const { task } = await ensureProjectRootTask(project);
  return getGeminiHooksStatus(project.repoPath, task?.id, project.sshHost);
}

/** 프로젝트에 Gemini CLI hooks를 설치한다 */
export async function installProjectGeminiHooks(
  projectId: string
): Promise<{ success: boolean; error?: string; status?: GeminiHooksStatus | null }> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };

  const { task } = await ensureProjectRootTask(project);
  if (!task) return { success: false, error: "기본 브랜치 태스크를 찾을 수 없습니다." };

  try {
    if (project.sshHost) {
      await installKanvibeHooks(project.repoPath, task.id, project.sshHost);
      return { success: true, status: await getGeminiHooksStatus(project.repoPath, task.id, project.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(project.sshHost);
    await setupGeminiHooks(project.repoPath, task.id, kanvibeUrl, authToken);
    return { success: true, status: await getGeminiHooksStatus(project.repoPath, task.id, project.sshHost) };
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
  if (!task?.project) return null;

  const hookTarget = await resolveTaskHookTarget(task);
  if (!hookTarget) return null;

  return getGeminiHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
}

/** 태스크의 worktree 또는 프로젝트 경로에 Gemini CLI hooks를 설치한다 */
export async function installTaskGeminiHooks(
  taskId: string
): Promise<{ success: boolean; error?: string; status?: GeminiHooksStatus | null }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };

  try {
    const hookTarget = await resolveTaskHookTarget(task);
    if (!hookTarget) {
      return { success: false, error: "프로젝트를 찾을 수 없습니다." };
    }

    if (hookTarget.sshHost) {
      await installKanvibeHooks(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
      return { success: true, status: await getGeminiHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(task.project.sshHost);
    await setupGeminiHooks(hookTarget.targetPath, hookTarget.taskId, kanvibeUrl, authToken);
    return { success: true, status: await getGeminiHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
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
  if (!project) return null;

  const { task } = await ensureProjectRootTask(project);
  return getCodexHooksStatus(project.repoPath, task?.id, project.sshHost);
}

export async function installProjectCodexHooks(
  projectId: string
): Promise<{ success: boolean; error?: string; status?: CodexHooksStatus | null }> {
  const repo = await getProjectRepository();
  const project = await repo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };

  const { task } = await ensureProjectRootTask(project);
  if (!task) return { success: false, error: "기본 브랜치 태스크를 찾을 수 없습니다." };

  try {
    if (project.sshHost) {
      await installKanvibeHooks(project.repoPath, task.id, project.sshHost);
      return { success: true, status: await getCodexHooksStatus(project.repoPath, task.id, project.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(project.sshHost);
    await setupCodexHooks(project.repoPath, task.id, kanvibeUrl, authToken);
    return { success: true, status: await getCodexHooksStatus(project.repoPath, task.id, project.sshHost) };
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
  if (!task?.project) return null;

  const hookTarget = await resolveTaskHookTarget(task);
  if (!hookTarget) return null;

  return getCodexHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
}

export async function installTaskCodexHooks(
  taskId: string
): Promise<{ success: boolean; error?: string; status?: CodexHooksStatus | null }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({ where: { id: taskId }, relations: ["project"] });
  if (!task?.project) return { success: false, error: "프로젝트를 찾을 수 없습니다." };

  try {
    const hookTarget = await resolveTaskHookTarget(task);
    if (!hookTarget) {
      return { success: false, error: "프로젝트를 찾을 수 없습니다." };
    }

    if (hookTarget.sshHost) {
      await installKanvibeHooks(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
      return { success: true, status: await getCodexHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(task.project.sshHost);
    await setupCodexHooks(hookTarget.targetPath, hookTarget.taskId, kanvibeUrl, authToken);
    return { success: true, status: await getCodexHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
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
): Promise<{ success: boolean; error?: string; status?: OpenCodeHooksStatus | null }> {
  const projectRepo = await getProjectRepository();
  const project = await projectRepo.findOneBy({ id: projectId });
  if (!project) return { success: false, error: "Project not found" };

  const { task } = await ensureProjectRootTask(project);
  if (!task) return { success: false, error: "Default branch task not found" };

  try {
    if (project.sshHost) {
      await installKanvibeHooks(project.repoPath, task.id, project.sshHost);
      return { success: true, status: await getOpenCodeHooksStatus(project.repoPath, task.id, project.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(project.sshHost);
    await setupOpenCodeHooks(project.repoPath, task.id, kanvibeUrl, authToken);
    return { success: true, status: await getOpenCodeHooksStatus(project.repoPath, task.id, project.sshHost) };
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
): Promise<{ success: boolean; error?: string; status?: OpenCodeHooksStatus | null }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOne({
    where: { id: taskId },
    relations: ["project"],
  });
  if (!task?.project) return { success: false, error: "Task or project not found" };

  try {
    const hookTarget = await resolveTaskHookTarget(task);
    if (!hookTarget) {
      return { success: false, error: "Task or project not found" };
    }

    if (hookTarget.sshHost) {
      await installKanvibeHooks(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
      return { success: true, status: await getOpenCodeHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
    }

    const { kanvibeUrl, authToken } = await getHookInstallConfig(task.project.sshHost);
    await setupOpenCodeHooks(hookTarget.targetPath, hookTarget.taskId, kanvibeUrl, authToken);
    return { success: true, status: await getOpenCodeHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost) };
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

  const hookTarget = await resolveTaskHookTarget(task);
  if (!hookTarget) return null;

  return getOpenCodeHooksStatus(hookTarget.targetPath, hookTarget.taskId, hookTarget.sshHost);
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

  return aggregateAiSessions({
    worktreePath: task.worktreePath || task.project.repoPath,
    repoPath: task.project.repoPath,
    includeRepoSessions,
    query,
    sshHost: task.project.sshHost,
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

  if (!task?.project) {
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
      sshHost: task.project.sshHost,
    },
    provider,
    sessionId,
    sourceRef,
    cursor,
    limit
  );
}
