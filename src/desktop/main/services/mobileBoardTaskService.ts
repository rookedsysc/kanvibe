import { SessionType } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import { isSafeGitRef } from "@/lib/gitRef";
import { getSessionDependencyStatus } from "@/lib/remoteSessionDependency";
import { getMoreDoneTasks, getSearchableTasks, updateTask, updateProjectColor, getTaskIdByProjectAndBranch, getTaskById, branchFromTask, fetchAndSavePrUrl } from "./kanbanService";
import { deleteProject, getAvailableHosts, listSubdirectories, scanAndRegisterProjects } from "./projectService";
import { MobileFeatureError, readMobileEnum, readMobileObject, readMobileText, requireMobileProject, requireMobileTask } from "./mobileFeatureValidation";

const branchingTaskIds = new Set<string>();

export async function getMobileSearchableTasks() {
  return getSearchableTasks();
}

export async function getMobileDoneTasks(offset: number, limit: number) {
  if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
    throw new MobileFeatureError(400, "페이지 값이 올바르지 않습니다");
  }
  return getMoreDoneTasks(offset, limit);
}

export async function updateMobileTaskDetails(taskId: string, input: unknown) {
  const body = readMobileObject(input);
  const updates: Parameters<typeof updateTask>[1] = {};
  if ("description" in body) {
    if (body.description !== null && (typeof body.description !== "string" || body.description.length > 16000 || body.description.includes("\0"))) {
      throw new MobileFeatureError(400, "설명 값이 올바르지 않습니다");
    }
    updates.description = body.description as string | null;
  }
  if ("priority" in body) updates.priority = body.priority === null ? null : readMobileEnum(Object.values(TaskPriority), body.priority, "우선순위");
  if (Object.keys(updates).length === 0 || Object.keys(body).some((key) => !["description", "priority"].includes(key))) {
    throw new MobileFeatureError(400, "설명 또는 우선순위만 변경할 수 있습니다");
  }
  await requireMobileTask(taskId);
  const task = await updateTask(taskId, updates);
  if (!task) throw new MobileFeatureError(404, "없는 태스크입니다");
  return task;
}

export async function updateMobileProjectColor(projectId: string, input: unknown) {
  const body = readMobileObject(input);
  if (typeof body.color !== "string" || !/^#[0-9a-fA-F]{6}$/.test(body.color)) throw new MobileFeatureError(400, "색상 값이 올바르지 않습니다");
  await requireMobileProject(projectId);
  await updateProjectColor(projectId, body.color);
  return body.color;
}

export async function getMobileProjectTasks(projectId: string) {
  await requireMobileProject(projectId);
  return (await getSearchableTasks()).filter((task) => task.projectId === projectId);
}

export async function getMobileProjectBranchTask(projectId: string, submittedBranch: unknown) {
  const branch = readMobileText(submittedBranch, "브랜치");
  await requireMobileProject(projectId);
  const taskId = await getTaskIdByProjectAndBranch(projectId, branch);
  return taskId ? getTaskById(taskId) : null;
}

export async function branchMobileTask(taskId: string, input: unknown) {
  const body = readMobileObject(input);
  const projectId = readMobileText(body.projectId, "프로젝트", 128);
  const branchName = readMobileText(body.branchName, "브랜치");
  const baseBranch = readMobileText(body.baseBranch, "베이스 브랜치");
  const sessionType = readMobileEnum(Object.values(SessionType), body.sessionType, "세션 종류");
  if (!isSafeGitRef(branchName) || !isSafeGitRef(baseBranch)) throw new MobileFeatureError(400, "브랜치 값이 올바르지 않습니다");
  if (branchingTaskIds.has(taskId)) throw new MobileFeatureError(409, "이미 브랜치를 연결하고 있습니다");
  branchingTaskIds.add(taskId);
  try {
    const task = await requireMobileTask(taskId);
    if (task.branchName) throw new MobileFeatureError(409, "이미 브랜치가 연결된 태스크입니다");
    const project = await requireMobileProject(projectId);
    if (project.isWorktree) throw new MobileFeatureError(400, "원본 프로젝트를 선택해 주세요");
    const dependency = await getSessionDependencyStatus(sessionType, project.sshHost);
    if (!dependency.available) throw new MobileFeatureError(409, "데스크탑에서 세션 도구를 설치해 주세요");
    const result = await branchFromTask(taskId, projectId, baseBranch, branchName, sessionType);
    if (!result) throw new MobileFeatureError(404, "없는 태스크입니다");
    return result;
  } finally {
    branchingTaskIds.delete(taskId);
  }
}

export async function refreshMobileTaskPr(taskId: string) {
  const task = await requireMobileTask(taskId);
  if (!task.branchName) throw new MobileFeatureError(409, "브랜치가 없는 태스크입니다");
  return fetchAndSavePrUrl(taskId);
}

export async function getMobileHosts() {
  return getAvailableHosts();
}

export async function readMobileSshHost(input: unknown): Promise<string> {
  const sshHost = readMobileText(input, "SSH 호스트", 256);
  if (!(await getAvailableHosts()).includes(sshHost)) throw new MobileFeatureError(400, "등록된 SSH 호스트를 선택해 주세요");
  return sshHost;
}

export async function getMobileRemoteDirectories(parentPath: unknown, host: unknown) {
  const sshHost = await readMobileSshHost(host);
  return listSubdirectories(readMobileText(parentPath, "폴더 경로"), sshHost);
}

export async function scanMobileRemoteProjects(rootPath: unknown, host: unknown) {
  const sshHost = await readMobileSshHost(host);
  return scanAndRegisterProjects(readMobileText(rootPath, "폴더 경로"), sshHost);
}

export async function deleteMobileProject(projectId: string) {
  await requireMobileProject(projectId);
  const isDeleted = await deleteProject(projectId);
  if (!isDeleted) throw new MobileFeatureError(404, "없는 프로젝트입니다");
}
