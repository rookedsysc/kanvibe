/**
 * Electron IPC 클라이언트 레이어.
 * 렌더러 프로세스에서 main 프로세스의 IPC 핸들러를 호출하기 위한 타입 안전한 래퍼 함수들.
 * 기존 Server Action 함수와 동일한 인터페이스를 제공하여 컴포넌트 전환을 최소화한다.
 */
import type { KanbanTask, TaskStatus, SessionType } from "@/entities/KanbanTask";
import type { TaskPriority } from "@/entities/TaskPriority";
import type { Project } from "@/entities/Project";
import type { PaneLayoutConfig, PaneLayoutType, PaneCommand } from "@/entities/PaneLayoutConfig";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import type { CodexHooksStatus } from "@/lib/codexHooksSetup";
import type { OpenCodeHooksStatus } from "@/lib/openCodeHooksSetup";

/** window.ipc가 준비될 때까지 대기한다 (preload 로드 타이밍 이슈 방어) */
function waitForIpc(): Promise<void> {
  if (typeof window !== "undefined" && window.ipc) return Promise.resolve();
  if (typeof window === "undefined") return Promise.reject(new Error("SSR 환경"));

  return new Promise((resolve, reject) => {
    let elapsed = 0;
    const interval = setInterval(() => {
      elapsed += 50;
      if (window.ipc) {
        clearInterval(interval);
        resolve();
      } else if (elapsed >= 3000) {
        clearInterval(interval);
        reject(new Error("window.ipc 초기화 타임아웃 (3초)"));
      }
    }, 50);
  });
}

/** 타입 안전한 IPC invoke 래퍼. preload 로드 전이면 대기 후 재시도한다 */
async function invoke<T>(channel: string, ...args: unknown[]): Promise<T> {
  await waitForIpc();
  return window.ipc.invoke(channel, ...args) as Promise<T>;
}

// ─── 공유 타입 ──────────────────────────────────────────────────────

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

export interface DiffFile {
  path: string;
  status: "added" | "modified" | "deleted" | "renamed";
  additions: number;
  deletions: number;
}

export interface SavePaneLayoutInput {
  layoutType: PaneLayoutType;
  panes: PaneCommand[];
  projectId?: string | null;
  isGlobal?: boolean;
}

export interface ScanResult {
  registered: string[];
  skipped: string[];
  errors: string[];
  worktreeTasks: string[];
  hooksSetup: string[];
}

// ─── Kanban IPC ─────────────────────────────────────────────────────

export const ipcKanban = {
  getTasksByStatus: () => invoke<TasksByStatusWithMeta>("kanban:getTasksByStatus"),

  getMoreDoneTasks: (offset: number, limit?: number) =>
    invoke<LoadMoreDoneResponse>("kanban:getMoreDoneTasks", offset, limit),

  getTaskById: (taskId: string) => invoke<KanbanTask | null>("kanban:getTaskById", taskId),

  getTaskIdByProjectAndBranch: (projectId: string, branchName: string) =>
    invoke<string | null>("kanban:getTaskIdByProjectAndBranch", projectId, branchName),

  createTask: (input: CreateTaskInput) => invoke<KanbanTask>("kanban:createTask", input),

  updateTaskStatus: (taskId: string, newStatus: TaskStatus) =>
    invoke<KanbanTask>("kanban:updateTaskStatus", taskId, newStatus),

  updateTask: (taskId: string, updates: Partial<Pick<KanbanTask, "title" | "description" | "priority">>) =>
    invoke<KanbanTask>("kanban:updateTask", taskId, updates),

  updateProjectColor: (projectId: string, color: string) =>
    invoke<void>("kanban:updateProjectColor", projectId, color),

  deleteTask: (taskId: string) => invoke<boolean>("kanban:deleteTask", taskId),

  branchFromTask: (
    taskId: string,
    projectId: string,
    baseBranch: string,
    branchName: string,
    sessionType: SessionType,
  ) =>
    invoke<KanbanTask>("kanban:branchFromTask", taskId, projectId, baseBranch, branchName, sessionType),

  connectTerminalSession: (taskId: string, sessionType: SessionType) =>
    invoke<KanbanTask>("kanban:connectTerminalSession", taskId, sessionType),

  reorderTasks: (status: TaskStatus, orderedIds: string[]) =>
    invoke<void>("kanban:reorderTasks", status, orderedIds),

  moveTaskToColumn: (taskId: string, newStatus: TaskStatus, destOrderedIds: string[]) =>
    invoke<void>("kanban:moveTaskToColumn", taskId, newStatus, destOrderedIds),

  fetchAndSavePrUrl: (taskId: string) => invoke<string | null>("kanban:fetchAndSavePrUrl", taskId),
};

// ─── Project IPC ────────────────────────────────────────────────────

export const ipcProject = {
  getAll: () => invoke<Project[]>("project:getAll"),

  getById: (projectId: string) => invoke<Project | null>("project:getById", projectId),

  register: (name: string, repoPath: string, sshHost?: string) =>
    invoke<Project>("project:register", name, repoPath, sshHost),

  delete: (projectId: string) => invoke<boolean>("project:delete", projectId),

  scanAndRegister: (rootPath: string, sshHost?: string) =>
    invoke<ScanResult>("project:scanAndRegister", rootPath, sshHost),

  listSubdirectories: (parentPath: string, sshHost?: string) =>
    invoke<string[]>("project:listSubdirectories", parentPath, sshHost),

  getBranches: (projectId: string) => invoke<string[]>("project:getBranches", projectId),

  getHooksStatus: (projectId: string) =>
    invoke<ClaudeHooksStatus | null>("project:getHooksStatus", projectId),
  installHooks: (projectId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installHooks", projectId),

  getGeminiHooksStatus: (projectId: string) =>
    invoke<GeminiHooksStatus | null>("project:getGeminiHooksStatus", projectId),
  installGeminiHooks: (projectId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installGeminiHooks", projectId),

  getCodexHooksStatus: (projectId: string) =>
    invoke<CodexHooksStatus | null>("project:getCodexHooksStatus", projectId),
  installCodexHooks: (projectId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installCodexHooks", projectId),

  installOpenCodeHooks: (projectId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installOpenCodeHooks", projectId),

  getTaskHooksStatus: (taskId: string) =>
    invoke<ClaudeHooksStatus | null>("project:getTaskHooksStatus", taskId),
  installTaskHooks: (taskId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installTaskHooks", taskId),

  getTaskGeminiHooksStatus: (taskId: string) =>
    invoke<GeminiHooksStatus | null>("project:getTaskGeminiHooksStatus", taskId),
  installTaskGeminiHooks: (taskId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installTaskGeminiHooks", taskId),

  getTaskCodexHooksStatus: (taskId: string) =>
    invoke<CodexHooksStatus | null>("project:getTaskCodexHooksStatus", taskId),
  installTaskCodexHooks: (taskId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installTaskCodexHooks", taskId),

  getTaskOpenCodeHooksStatus: (taskId: string) =>
    invoke<OpenCodeHooksStatus | null>("project:getTaskOpenCodeHooksStatus", taskId),
  installTaskOpenCodeHooks: (taskId: string) =>
    invoke<{ success: boolean; error?: string }>("project:installTaskOpenCodeHooks", taskId),
};

// ─── Settings IPC ───────────────────────────────────────────────────

export const ipcSettings = {
  getAppSetting: (key: string) => invoke<string | null>("settings:getAppSetting", key),
  setAppSetting: (key: string, value: string) =>
    invoke<void>("settings:setAppSetting", key, value),

  getSidebarDefaultCollapsed: () => invoke<boolean>("settings:getSidebarDefaultCollapsed"),
  setSidebarDefaultCollapsed: (collapsed: boolean) =>
    invoke<void>("settings:setSidebarDefaultCollapsed", collapsed),

  getSidebarHintDismissed: () => invoke<boolean>("settings:getSidebarHintDismissed"),
  dismissSidebarHint: () => invoke<void>("settings:dismissSidebarHint"),

  getDoneAlertDismissed: () => invoke<boolean>("settings:getDoneAlertDismissed"),
  dismissDoneAlert: () => invoke<void>("settings:dismissDoneAlert"),

  getNotificationSettings: () =>
    invoke<{ isEnabled: boolean; enabledStatuses: string[] }>("settings:getNotificationSettings"),
  setNotificationEnabled: (enabled: boolean) =>
    invoke<void>("settings:setNotificationEnabled", enabled),
  setNotificationStatuses: (statuses: string[]) =>
    invoke<void>("settings:setNotificationStatuses", statuses),

  getDefaultSessionType: () => invoke<SessionType>("settings:getDefaultSessionType"),
  setDefaultSessionType: (sessionType: SessionType) =>
    invoke<void>("settings:setDefaultSessionType", sessionType),
};

// ─── PaneLayout IPC ─────────────────────────────────────────────────

export const ipcPaneLayout = {
  getGlobal: () => invoke<PaneLayoutConfig | null>("paneLayout:getGlobal"),
  getProject: (projectId: string) =>
    invoke<PaneLayoutConfig | null>("paneLayout:getProject", projectId),
  getEffective: (projectId?: string) =>
    invoke<PaneLayoutConfig | null>("paneLayout:getEffective", projectId),
  getAll: () => invoke<PaneLayoutConfig[]>("paneLayout:getAll"),
  save: (input: SavePaneLayoutInput) => invoke<PaneLayoutConfig>("paneLayout:save", input),
  delete: (id: string) => invoke<boolean>("paneLayout:delete", id),
};

// ─── Diff IPC ───────────────────────────────────────────────────────

export const ipcDiff = {
  getGitDiffFiles: (taskId: string) => invoke<DiffFile[]>("diff:getGitDiffFiles", taskId),

  getOriginalFileContent: (taskId: string, filePath: string) =>
    invoke<string>("diff:getOriginalFileContent", taskId, filePath),

  getFileContent: (taskId: string, filePath: string) =>
    invoke<string>("diff:getFileContent", taskId, filePath),

  saveFileContent: (taskId: string, filePath: string, content: string) =>
    invoke<{ success: boolean; error?: string }>("diff:saveFileContent", taskId, filePath, content),
};

// ─── App IPC ────────────────────────────────────────────────────────

export const ipcApp = {
  getWsPort: () => invoke<number>("app:getWsPort"),
  getHooksPort: () => invoke<number>("app:getHooksPort"),
  getAvailableHosts: () => invoke<string[]>("app:getAvailableHosts"),
};

// ─── 이벤트 리스너 ─────────────────────────────────────────────────

/** 보드 새로고침 이벤트를 수신한다. 구독 해제 함수를 반환한다 */
export function onBoardRefresh(callback: () => void): () => void {
  return window.ipc.on("board:refresh", callback);
}

/** 자동 업데이트 이벤트를 수신한다 */
export function onUpdaterEvent(
  event: string,
  callback: (...args: unknown[]) => void,
): () => void {
  return window.ipc.on(`updater:${event}`, callback);
}
