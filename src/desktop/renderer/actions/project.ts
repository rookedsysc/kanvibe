import { invokeDesktop } from "@/desktop/renderer/ipc";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import type { Project } from "@/entities/Project";
import type {
  AggregatedAiSessionDetail,
  AggregatedAiSessionsResult,
  AiMessageRole,
  AiSessionProvider,
} from "@/lib/aiSessions/types";
import type {
  ClaudeHooksStatus,
} from "@/lib/claudeHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import type { CodexHooksStatus } from "@/lib/codexHooksSetup";
import type { OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";
import type { ScanResult } from "@/desktop/main/services/projectService";

async function invokeAndRefresh<T>(
  method: string,
  args: unknown[] = [],
  options?: { refresh?: boolean },
): Promise<T> {
  const result = await invokeDesktop<T>("project", method, ...args);
  if (options?.refresh !== false) {
    triggerDesktopRefresh();
  }
  return result;
}

export type { ScanResult };

export function getAllProjects(): Promise<Project[]> {
  return invokeDesktop("project", "getAllProjects");
}

export function getAvailableHosts(): Promise<string[]> {
  return invokeDesktop("project", "getAvailableHosts");
}

export function getProjectById(projectId: string): Promise<Project | null> {
  return invokeDesktop("project", "getProjectById", projectId);
}

export function registerProject(name: string, repoPath: string, sshHost?: string) {
  return invokeAndRefresh<{ success: boolean; error?: string; project?: Project }>("registerProject", [name, repoPath, sshHost]);
}

export function deleteProject(projectId: string): Promise<boolean> {
  return invokeAndRefresh("deleteProject", [projectId]);
}

export function scanAndRegisterProjects(rootPath: string, sshHost?: string): Promise<ScanResult> {
  return invokeAndRefresh("scanAndRegisterProjects", [rootPath, sshHost]);
}

export function listSubdirectories(parentPath: string, sshHost?: string): Promise<string[]> {
  return invokeDesktop("project", "listSubdirectories", parentPath, sshHost);
}

export function getProjectBranches(projectId: string): Promise<string[]> {
  return invokeDesktop("project", "getProjectBranches", projectId);
}

export function getProjectHooksStatus(projectId: string): Promise<ClaudeHooksStatus | null> {
  return invokeDesktop("project", "getProjectHooksStatus", projectId);
}

export function installProjectHooks(projectId: string): Promise<{ success: boolean; error?: string; status?: ClaudeHooksStatus | null }> {
  return invokeAndRefresh("installProjectHooks", [projectId], { refresh: false });
}

export function getTaskHooksStatus(taskId: string): Promise<ClaudeHooksStatus | null> {
  return invokeDesktop("project", "getTaskHooksStatus", taskId);
}

export function installTaskHooks(taskId: string): Promise<{ success: boolean; error?: string; status?: ClaudeHooksStatus | null }> {
  return invokeAndRefresh("installTaskHooks", [taskId], { refresh: false });
}

export function getProjectGeminiHooksStatus(projectId: string): Promise<GeminiHooksStatus | null> {
  return invokeDesktop("project", "getProjectGeminiHooksStatus", projectId);
}

export function installProjectGeminiHooks(projectId: string): Promise<{ success: boolean; error?: string; status?: GeminiHooksStatus | null }> {
  return invokeAndRefresh("installProjectGeminiHooks", [projectId], { refresh: false });
}

export function getTaskGeminiHooksStatus(taskId: string): Promise<GeminiHooksStatus | null> {
  return invokeDesktop("project", "getTaskGeminiHooksStatus", taskId);
}

export function installTaskGeminiHooks(taskId: string): Promise<{ success: boolean; error?: string; status?: GeminiHooksStatus | null }> {
  return invokeAndRefresh("installTaskGeminiHooks", [taskId], { refresh: false });
}

export function getProjectCodexHooksStatus(projectId: string): Promise<CodexHooksStatus | null> {
  return invokeDesktop("project", "getProjectCodexHooksStatus", projectId);
}

export function installProjectCodexHooks(projectId: string): Promise<{ success: boolean; error?: string; status?: CodexHooksStatus | null }> {
  return invokeAndRefresh("installProjectCodexHooks", [projectId], { refresh: false });
}

export function getTaskCodexHooksStatus(taskId: string): Promise<CodexHooksStatus | null> {
  return invokeDesktop("project", "getTaskCodexHooksStatus", taskId);
}

export function installTaskCodexHooks(taskId: string): Promise<{ success: boolean; error?: string; status?: CodexHooksStatus | null }> {
  return invokeAndRefresh("installTaskCodexHooks", [taskId], { refresh: false });
}

export function installProjectOpenCodeHooks(projectId: string): Promise<{ success: boolean; error?: string; status?: OpenCodeHooksStatus | null }> {
  return invokeAndRefresh("installProjectOpenCodeHooks", [projectId], { refresh: false });
}

export function installTaskOpenCodeHooks(taskId: string): Promise<{ success: boolean; error?: string; status?: OpenCodeHooksStatus | null }> {
  return invokeAndRefresh("installTaskOpenCodeHooks", [taskId], { refresh: false });
}

export function getTaskOpenCodeHooksStatus(taskId: string): Promise<OpenCodeHooksStatus | null> {
  return invokeDesktop("project", "getTaskOpenCodeHooksStatus", taskId);
}

export function getTaskAiSessions(taskId: string, includeRepoSessions = false, query?: string): Promise<AggregatedAiSessionsResult> {
  return invokeDesktop("project", "getTaskAiSessions", taskId, includeRepoSessions, query);
}

export function getTaskAiSessionDetail(
  taskId: string,
  provider: AiSessionProvider,
  sessionId: string,
  sourceRef?: string | null,
  cursor?: string | null,
  limit = 20,
  includeRepoSessions = false,
  query?: string,
  roles?: AiMessageRole[],
): Promise<AggregatedAiSessionDetail | null> {
  return invokeDesktop(
    "project",
    "getTaskAiSessionDetail",
    taskId,
    provider,
    sessionId,
    sourceRef,
    cursor,
    limit,
    includeRepoSessions,
    query,
    roles,
  );
}
