import { beforeEach, describe, expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({
  task: { id: "task-1", projectId: "project-1", branchName: null, worktreePath: null, sessionName: null, sshHost: null, prUrl: null } as Record<string, any>,
  getTaskById: vi.fn(), getProjectById: vi.fn(), updateTask: vi.fn(), updateProjectColor: vi.fn(), getSearchableTasks: vi.fn(), getMoreDoneTasks: vi.fn(), getTaskIdByProjectAndBranch: vi.fn(), branchFromTask: vi.fn(), fetchAndSavePrUrl: vi.fn(), deleteProject: vi.fn(), getAvailableHosts: vi.fn(), listSubdirectories: vi.fn(), scanAndRegisterProjects: vi.fn(), getSessionDependencyStatus: vi.fn(),
  getGitDiffFiles: vi.fn(), getOriginalFileContent: vi.fn(), getFileContent: vi.fn(), saveFileContent: vi.fn(), getTaskDiffStats: vi.fn(), findSymbolicLinkPath: vi.fn(), getTasksByStatus: vi.fn(),
  listTerminalTabs: vi.fn(), renameTerminalTab: vi.fn(), moveTerminalTab: vi.fn(), getAllPaneLayouts: vi.fn(), savePaneLayout: vi.fn(), deletePaneLayout: vi.fn(), pasteImageToRemoteTerminal: vi.fn(), getTaskSurfaces: vi.fn(), writeToPane: vi.fn(),
  getDefaultSessionType: vi.fn(), setDefaultSessionType: vi.fn(), getDoneAlertDismissed: vi.fn(), dismissDoneAlert: vi.fn(), getBoardSortPreference: vi.fn(), setBoardSortPreference: vi.fn(), getBackgroundSyncEnabled: vi.fn(), setBackgroundSyncEnabled: vi.fn(), getBackgroundSyncIntervalMs: vi.fn(), setBackgroundSyncIntervalMs: vi.fn(), getSessionTypeChangeTargets: vi.fn(), changeAllTaskSessionTypes: vi.fn(), deleteTasks: vi.fn(), runBackgroundTaskSyncNow: vi.fn(), getNotificationById: vi.fn(), markTaskNotificationsRead: vi.fn(),
}));
vi.mock("../kanbanService", () => ({ ...mocks }));
vi.mock("../projectService", () => ({ ...mocks }));
vi.mock("../diffService", () => ({ ...mocks }));
vi.mock("../terminalTabService", () => ({ ...mocks }));
vi.mock("../paneLayoutService", () => ({ ...mocks }));
vi.mock("../appSettingsService", () => ({ ...mocks }));
vi.mock("../backgroundTaskSyncService", () => ({ ...mocks }));
vi.mock("../mobileBridgeService", () => ({ ...mocks }));
vi.mock("@/desktop/main/terminalBridge", () => ({ ...mocks }));
vi.mock("@/desktop/main/notificationStore", () => ({ ...mocks }));
vi.mock("@/lib/remoteSessionDependency", () => ({ ...mocks }));
vi.mock("@/lib/hostFileAccess", () => ({ findSymbolicLinkPath: mocks.findSymbolicLinkPath, quoteShellArgument: (value: string) => `'${value.replaceAll("'", `'\\''`)}'` }));
import * as feature from "../mobileFeatureService";

beforeEach(() => {
  vi.clearAllMocks();
  mocks.task = { id: "task-1", projectId: "project-1", branchName: null, worktreePath: null, sessionName: null, sshHost: null, prUrl: null };
  mocks.getTaskById.mockImplementation(async () => ({ ...mocks.task }));
  mocks.getProjectById.mockResolvedValue({ id: "project-1", isWorktree: false, sshHost: null });
  mocks.getAvailableHosts.mockResolvedValue(["known-host"]);
  mocks.getSessionDependencyStatus.mockResolvedValue({ available: true });
  mocks.getGitDiffFiles.mockResolvedValue([{ path: "src/file.ts", status: "modified" }]);
  mocks.findSymbolicLinkPath.mockResolvedValue(null);
  mocks.listTerminalTabs.mockResolvedValue({ ok: true, tabs: [{ id: "@1", name: "one" }, { id: "@2", name: "two" }] });
  mocks.renameTerminalTab.mockResolvedValue({ ok: true });
  mocks.moveTerminalTab.mockResolvedValue({ ok: true });
  mocks.getDefaultSessionType.mockResolvedValue("tmux");
  mocks.getDoneAlertDismissed.mockResolvedValue(false);
  mocks.getBoardSortPreference.mockResolvedValue({ keys: [] });
  mocks.getBackgroundSyncEnabled.mockResolvedValue(false);
  mocks.getBackgroundSyncIntervalMs.mockResolvedValue(600000);
  mocks.getSessionTypeChangeTargets.mockResolvedValue({ taskCount: 2, liveSessionCount: 1, sshHosts: [null] });
});

describe("task and project feature guards", () => {
  it.each([{ priority: "urgent" }, { title: "hidden write" }, { description: 42 }, {}, null])("rejects invalid details %j before mutation", async (body) => {
    await expect(feature.updateMobileTaskDetails("task-1", body)).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.updateTask).not.toHaveBeenCalled();
  });
  it("permits explicit description/priority clears and passes only the approved fields", async () => {
    mocks.updateTask.mockResolvedValue({ id: "task-1", description: null, priority: null });
    await feature.updateMobileTaskDetails("task-1", { description: null, priority: null });
    expect(mocks.updateTask).toHaveBeenCalledWith("task-1", { description: null, priority: null });
  });
  it("does not mutate a missing task", async () => {
    mocks.getTaskById.mockResolvedValue(null);
    await expect(feature.updateMobileTaskDetails("missing", { priority: null })).rejects.toMatchObject({ statusCode: 404 });
    expect(mocks.updateTask).not.toHaveBeenCalled();
  });
  it.each([[-1, 20], [0, 101], [0.5, 20]])("rejects invalid pagination (%s,%s)", async (offset, limit) => {
    await expect(feature.getMobileDoneTasks(offset, limit)).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.getMoreDoneTasks).not.toHaveBeenCalled();
  });
  it("rejects unconfigured SSH aliases and never invokes scan", async () => {
    await expect(feature.scanMobileRemoteProjects("~/repo", "arbitrary-host")).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.scanAndRegisterProjects).not.toHaveBeenCalled();
  });
  it("passes a configured remote host to the existing directory and scan services", async () => {
    await feature.getMobileRemoteDirectories("~/repo", "known-host");
    await feature.scanMobileRemoteProjects("~/repo", "known-host");
    expect(mocks.listSubdirectories).toHaveBeenCalledWith("~/repo", "known-host");
    expect(mocks.scanAndRegisterProjects).toHaveBeenCalledWith("~/repo", "known-host");
  });
  it("validates color before saving", async () => {
    await expect(feature.updateMobileProjectColor("project-1", { color: "red" })).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.updateProjectColor).not.toHaveBeenCalled();
    await feature.updateMobileProjectColor("project-1", { color: "#0064FF" });
    expect(mocks.updateProjectColor).toHaveBeenCalledWith("project-1", "#0064FF");
  });
  it("branches the existing ID and does not duplicate an already connected task", async () => {
    const draft = { projectId: "project-1", baseBranch: "main", branchName: "feat/mobile", sessionType: "tmux" };
    mocks.branchFromTask.mockResolvedValue({ id: "task-1", status: "progress" });
    expect(await feature.branchMobileTask("task-1", draft)).toMatchObject({ id: "task-1", status: "progress" });
    expect(mocks.branchFromTask).toHaveBeenCalledWith("task-1", "project-1", "main", "feat/mobile", "tmux");
    mocks.task.branchName = "already-attached";
    await expect(feature.branchMobileTask("task-1", draft)).rejects.toMatchObject({ statusCode: 409 });
    expect(mocks.branchFromTask).toHaveBeenCalledTimes(1);
  });
});

describe("diff file boundary", () => {
  beforeEach(() => Object.assign(mocks.task, { branchName: "feature", worktreePath: "/fixture/repo" }));
  it.each(["../outside", "/etc/passwd", "a/../../outside", ".git/config", "src\\file", "src//file"])("rejects unsafe path %s before file access", async (filePath) => {
    await expect(feature.getMobileDiffFile("task-1", filePath)).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.getFileContent).not.toHaveBeenCalled();
  });
  it("rejects a changed file reached through symlinks", async () => {
    mocks.findSymbolicLinkPath.mockResolvedValue("/fixture/repo/src");
    await expect(feature.saveMobileDiffFile("task-1", "src/file.ts", { content: "text" })).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.saveFileContent).not.toHaveBeenCalled();
  });
  it("rejects non-diff paths and oversized edits without writing", async () => {
    await expect(feature.saveMobileDiffFile("task-1", "unknown.ts", { content: "text" })).rejects.toMatchObject({ statusCode: 404 });
    await expect(feature.saveMobileDiffFile("task-1", "src/file.ts", { content: "x".repeat(1024 * 1024 + 1) })).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.saveFileContent).not.toHaveBeenCalled();
  });
  it("delegates accepted file edits and reports write failures", async () => {
    mocks.saveFileContent.mockResolvedValue({ success: true });
    await feature.saveMobileDiffFile("task-1", "src/file.ts", { content: "한글" });
    expect(mocks.saveFileContent).toHaveBeenCalledWith("task-1", "src/file.ts", "한글");
    mocks.saveFileContent.mockResolvedValue({ success: false, error: "secret path" });
    await expect(feature.saveMobileDiffFile("task-1", "src/file.ts", { content: "next" })).rejects.toMatchObject({ statusCode: 409, message: "파일을 저장하지 못했습니다" });
  });
});

describe("workspace and settings boundaries", () => {
  it("checks tab identity before invoking rename or move", async () => {
    await expect(feature.renameMobileTab("task-1", "other-session-tab", { name: "renamed" })).rejects.toMatchObject({ statusCode: 404 });
    await expect(feature.moveMobileTab("task-1", "@1", { index: 2 })).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.renameTerminalTab).not.toHaveBeenCalled();
    expect(mocks.moveTerminalTab).not.toHaveBeenCalled();
    await feature.renameMobileTab("task-1", "@1", { name: "새 탭" });
    await feature.moveMobileTab("task-1", "@1", { index: 1 });
    expect(mocks.renameTerminalTab).toHaveBeenCalledWith("task-1", "@1", "새 탭");
    expect(mocks.moveTerminalTab).toHaveBeenCalledWith("task-1", "@1", 1);
  });
  it("checks pane layout size and scope before persistence", async () => {
    await expect(feature.saveMobilePaneLayout({ layoutType: "quad", isGlobal: true, panes: [{ position: 0, command: "" }] })).rejects.toMatchObject({ statusCode: 400 });
    await expect(feature.saveMobilePaneLayout({ layoutType: "single", isGlobal: true, projectId: "project-1", panes: [{ position: 0, command: "" }] })).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.savePaneLayout).not.toHaveBeenCalled();
    await feature.saveMobilePaneLayout({ layoutType: "single", isGlobal: true, panes: [{ position: 0, command: "echo ready" }] });
    expect(mocks.savePaneLayout).toHaveBeenCalledWith({ layoutType: "single", isGlobal: true, projectId: undefined, panes: [{ position: 0, command: "echo ready" }] });
  });
  it("validates all settings before applying any write", async () => {
    await expect(feature.updateMobileSettings({ defaultSessionType: "tmux", backgroundSync: { isEnabled: true, intervalMs: 1 } })).rejects.toMatchObject({ statusCode: 400 });
    expect(mocks.setDefaultSessionType).not.toHaveBeenCalled();
    await expect(feature.updateMobileSettings({ arbitrarySetting: "unsafe" })).rejects.toMatchObject({ statusCode: 400 });
    await feature.updateMobileSettings({ defaultSessionType: "zellij", sortPreference: { keys: [{ field: "priority", direction: "desc" }] } });
    expect(mocks.setDefaultSessionType).toHaveBeenCalledWith("zellij");
    expect(mocks.setBoardSortPreference).toHaveBeenCalledWith({ keys: [{ field: "priority", direction: "desc" }] });
  });
  it("rejects duplicate sort criteria and dependencies missing on any bulk target", async () => {
    await expect(feature.updateMobileSettings({ sortPreference: { keys: [{ field: "title", direction: "asc" }, { field: "title", direction: "desc" }] } })).rejects.toMatchObject({ statusCode: 400 });
    mocks.getSessionDependencyStatus.mockResolvedValue({ available: false });
    await expect(feature.changeAllMobileTaskSessions({ sessionType: "herdr", shouldCloseExistingSessions: false })).rejects.toMatchObject({ statusCode: 409 });
    expect(mocks.changeAllTaskSessionTypes).not.toHaveBeenCalled();
  });
  it("limits sync-review deletes to notification-owned unchanged tasks", async () => {
    mocks.getNotificationById.mockResolvedValue({ action: { type: "background-sync-review", payload: { mergedPullRequests: [{ taskId: "task-1", branchName: "feature", prUrl: "https://github.com/p/r/pull/1" }] } } });
    await expect(feature.deleteMobileSyncReviewTasks("notification", { taskIds: ["other"] })).rejects.toMatchObject({ statusCode: 400 });
    await expect(feature.deleteMobileSyncReviewTasks("notification", { taskIds: ["task-1"] })).rejects.toMatchObject({ statusCode: 409 });
    expect(mocks.deleteTasks).not.toHaveBeenCalled();
    mocks.task.branchName = "feature";
    mocks.deleteTasks.mockResolvedValue(["task-1"]);
    expect(await feature.deleteMobileSyncReviewTasks("notification", { taskIds: ["task-1", "task-1"] })).toEqual(["task-1"]);
    expect(mocks.deleteTasks).toHaveBeenCalledWith(["task-1"]);
  });
  it("marks only the requested existing task notifications read", async () => {
    mocks.markTaskNotificationsRead.mockResolvedValue(3);
    expect(await feature.markMobileTaskNotificationsRead("task-1")).toBe(3);
    expect(mocks.markTaskNotificationsRead).toHaveBeenCalledWith("task-1");
  });
  it("rejects malformed images and local targets before uploads", async () => {
    await expect(feature.pasteMobileRemoteImage("task-1", "%1", { mimeType: "image/png", base64: "not-base64" })).rejects.toMatchObject({ statusCode: 400 });
    await expect(feature.pasteMobileRemoteImage("task-1", "%1", { mimeType: "image/png", base64: "iVBORw0KGgo=" })).rejects.toMatchObject({ statusCode: 409 });
    expect(mocks.pasteImageToRemoteTerminal).not.toHaveBeenCalled();
  });
  it("uploads images only to an owned remote pane and inserts a quoted path", async () => {
    mocks.task.sshHost = "known-host";
    mocks.getTaskSurfaces.mockResolvedValue({ tabs: [{ panes: [{ id: "%1" }] }] });
    mocks.pasteImageToRemoteTerminal.mockResolvedValue({ ok: true, remotePath: "/tmp/kanvibe-paste.png" });
    expect(await feature.pasteMobileRemoteImage("task-1", "%1", { mimeType: "image/png", base64: "iVBORw0KGgo=" })).toBe("/tmp/kanvibe-paste.png");
    expect(mocks.writeToPane).toHaveBeenCalledWith("task-1", "%1", "'/tmp/kanvibe-paste.png'");
  });
});
