import { beforeEach, describe, expect, it, vi } from "vitest";

const entityMocks = vi.hoisted(() => ({
  TaskStatus: {
    TODO: "todo",
    PROGRESS: "progress",
    PENDING: "pending",
    REVIEW: "review",
    DONE: "done",
  },
  SessionType: {
    TMUX: "tmux",
    ZELLIJ: "zellij",
  },
}));

const mocks = vi.hoisted(() => ({
  projectRepo: {
    findOneBy: vi.fn(),
  },
  taskRepo: {
    findOne: vi.fn(),
    findOneBy: vi.fn(),
    save: vi.fn(),
    create: vi.fn(),
  },
  broadcastBoardUpdate: vi.fn(),
  broadcastHookStatusTargetMissing: vi.fn(),
  broadcastTaskStatusChanged: vi.fn(),
  writeTextFile: vi.fn(),
  readTextFile: vi.fn(),
  execGit: vi.fn(),
  recordPaneChatSessionBinding: vi.fn(),
}));

vi.mock("@/entities/KanbanTask", () => entityMocks);

/** 상태 hook은 task 토큰으로 인증한다. 이 파일은 인증 뒤의 동작을 다루므로 "valid-token"만 통과시킨다 */
const VALID_HOOK_AUTHORIZATION = "Bearer valid-token";
vi.mock("@/desktop/main/services/taskCliTokenService", () => ({
  isValidTaskCliToken: async (_taskId: string, token: string) => token === "valid-token",
}));

const { TaskStatus } = entityMocks;

vi.mock("@/lib/database", () => ({
  getProjectRepository: vi.fn(async () => mocks.projectRepo),
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
  broadcastHookStatusTargetMissing: mocks.broadcastHookStatusTargetMissing,
  broadcastTaskStatusChanged: mocks.broadcastTaskStatusChanged,
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: mocks.execGit,
}));

vi.mock("@/lib/aiSessions/paneChatSessionBindings", () => ({
  recordPaneChatSessionBinding: mocks.recordPaneChatSessionBinding,
}));

vi.mock("@/lib/hostFileAccess", () => ({
  readTextFile: mocks.readTextFile,
  writeTextFile: mocks.writeTextFile,
  quoteShellArgument: (value: string) => `'${value.replaceAll("'", `'\''`)}'`,
}));

async function flushMicrotasks(count = 8): Promise<void> {
  for (let index = 0; index < count; index += 1) {
    await Promise.resolve();
  }
}

describe("hookService.updateHookTaskStatus", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.readTextFile.mockResolvedValue("");
    mocks.execGit.mockResolvedValue(".git");
  });

  it.each([
    ["토큰 없음", undefined],
    ["다른 task의 토큰", "Bearer other-token"],
    ["Bearer 형식이 아님", "valid-token"],
  ])("%s이면 task를 조회하지 않고 401로 거절해 상태를 바꾸지 않는다", async (_case, authorizationHeader) => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");

    const result = await updateHookTaskStatus({ taskId: "task-1", status: TaskStatus.REVIEW }, authorizationHeader);

    expect(result).toMatchObject({ success: false, status: 401 });
    expect(mocks.taskRepo.findOne).not.toHaveBeenCalled();
    expect(mocks.taskRepo.save).not.toHaveBeenCalled();
    expect(mocks.broadcastTaskStatusChanged).not.toHaveBeenCalled();
  });

  it("저널 재전달도 보관된 토큰이 없으면 401로 거절한다", async () => {
    const { updateJournalHookTaskStatus } = await import("@/desktop/main/services/hookService");

    const result = await updateJournalHookTaskStatus({ taskId: "task-1", status: TaskStatus.REVIEW }, undefined, new Set(["host-a"]));

    expect(result).toMatchObject({ success: false, status: 401 });
    expect(mocks.taskRepo.findOne).not.toHaveBeenCalled();
  });

  it("터널로 들어온 요청은 다른 호스트의 작업을 바꾸지 못한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    mocks.taskRepo.findOne.mockResolvedValue({
      id: "task-1",
      status: TaskStatus.PROGRESS,
      sshHost: "host-b",
    });

    const result = await updateHookTaskStatus({ taskId: "task-1", status: TaskStatus.REVIEW }, VALID_HOOK_AUTHORIZATION, new Set(["host-a"]));

    expect(result).toMatchObject({ success: false, status: 404 });
    expect(mocks.taskRepo.save).not.toHaveBeenCalled();
    expect(mocks.broadcastTaskStatusChanged).not.toHaveBeenCalled();
  });

  it("taskId로 작업 상태를 변경한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const project = { id: "project-1", name: "kanvibe" };
    const task = {
      id: "task-1",
      title: "Fix notification",
      description: "debug electron hook",
      branchName: "fix-electron-notification",
      projectId: project.id,
      project,
      status: TaskStatus.PROGRESS,
      sessionType: null,
      sessionName: null,
      worktreePath: null,
      sshHost: null,
    };
    mocks.taskRepo.findOne.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    const result = await updateHookTaskStatus({
      taskId: task.id,
      status: TaskStatus.REVIEW,
    }, VALID_HOOK_AUTHORIZATION);

    expect(mocks.taskRepo.findOne).toHaveBeenCalledWith({
      where: { id: task.id },
      relations: ["project"],
    });
    expect(mocks.broadcastTaskStatusChanged).toHaveBeenCalledWith({
      projectName: project.name,
      branchName: task.branchName,
      taskTitle: task.title,
      description: task.description,
      newStatus: TaskStatus.REVIEW,
      taskId: task.id,
    });
    expect(result).toEqual({
      success: true,
      data: {
        id: task.id,
        status: TaskStatus.REVIEW,
        branchName: task.branchName,
        projectName: project.name,
      },
    });
  });

  it("hook Done 상태 변경은 세션/worktree 정보를 보존하고 Done cleanup을 예약하지 않는다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const project = { id: "project-1", name: "kanvibe", repoPath: "/workspace/api", sshHost: "remote-host" };
    const task = {
      id: "task-done-hook",
      title: "Preserve hook resources",
      description: null,
      branchName: "feature-hook-done",
      projectId: project.id,
      project,
      status: TaskStatus.REVIEW,
      sessionType: "tmux" as never,
      sessionName: "api-feature-hook-done",
      worktreePath: "/workspace/api__worktrees/feature-hook-done",
      sshHost: "remote-host",
    };
    mocks.taskRepo.findOne.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    const result = await updateHookTaskStatus({
      taskId: task.id,
      status: TaskStatus.DONE,
    }, VALID_HOOK_AUTHORIZATION);

    expect(mocks.execGit).toHaveBeenCalledWith(
      "git -C '/workspace/api__worktrees/feature-hook-done' rev-parse --path-format=absolute --git-common-dir",
      "remote-host",
      { strictHostKeyChecking: true },
    );
    expect(mocks.taskRepo.save).toHaveBeenCalledWith(expect.objectContaining({
      id: task.id,
      status: TaskStatus.DONE,
      sessionType: "tmux",
      sessionName: "api-feature-hook-done",
      worktreePath: "/workspace/api__worktrees/feature-hook-done",
      sshHost: "remote-host",
    }));
    expect(result).toEqual({
      success: true,
      data: {
        id: task.id,
        status: TaskStatus.DONE,
        branchName: task.branchName,
        projectName: project.name,
      },
    });
  });

  it("hook 상태 변경은 worktree의 .kanvibe task 상태 파일에도 저장한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const task = {
      id: "task-1",
      title: "Fix sync",
      description: null,
      branchName: "feature-sync",
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: TaskStatus.PROGRESS,
      sessionType: null,
      sessionName: null,
      worktreePath: "/workspace/api__worktrees/feature-sync",
      sshHost: null,
    };
    mocks.taskRepo.findOne.mockResolvedValue(task);
    mocks.taskRepo.save.mockImplementation(async (value) => value);

    await updateHookTaskStatus({ taskId: task.id, status: TaskStatus.REVIEW }, VALID_HOOK_AUTHORIZATION);

    expect(mocks.writeTextFile).toHaveBeenCalledWith(
      "/workspace/api__worktrees/feature-sync/.kanvibe/status.json",
      expect.stringContaining('"status": "review"'),
      null,
    );
  });

  it(".kanvibe task 상태 파일 저장이 실패해도 hook 상태 변경 응답과 broadcast는 계속 진행한다", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    try {
      const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
      const task = {
        id: "task-1",
        title: "Fix sync",
        description: null,
        branchName: "feature-sync",
        projectId: "project-1",
        project: { id: "project-1", name: "kanvibe" },
        status: TaskStatus.PROGRESS,
        sessionType: null,
        sessionName: null,
        worktreePath: "/workspace/api__worktrees/feature-sync",
        sshHost: null,
      };
      mocks.taskRepo.findOne.mockResolvedValue(task);
      mocks.taskRepo.save.mockImplementation(async (value) => value);
      mocks.writeTextFile
        .mockResolvedValueOnce(undefined)
        .mockRejectedValueOnce(new Error("state write failed"));

      const result = await updateHookTaskStatus({ taskId: task.id, status: TaskStatus.REVIEW }, VALID_HOOK_AUTHORIZATION);

      expect(result).toEqual({
        success: true,
        data: {
          id: task.id,
          status: TaskStatus.REVIEW,
          branchName: task.branchName,
          projectName: "kanvibe",
        },
      });
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
      expect(mocks.broadcastTaskStatusChanged).toHaveBeenCalledWith(expect.objectContaining({
        taskId: task.id,
        newStatus: TaskStatus.REVIEW,
      }));
      expect(consoleError).toHaveBeenCalledWith(".kanvibe task 상태 저장 실패:", expect.objectContaining({
        repoPath: "/workspace/api__worktrees/feature-sync",
        taskId: task.id,
        status: TaskStatus.REVIEW,
      }));
    } finally {
      consoleError.mockRestore();
    }
  });

  it("task 식별자가 없으면 400을 반환한다", async () => {
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    const result = await updateHookTaskStatus({
      taskId: "",
      status: TaskStatus.REVIEW,
    }, VALID_HOOK_AUTHORIZATION);

    expect(result).toEqual({
      success: false,
      error: "taskId, status는 필수입니다.",
      status: 400,
    });
  });
});

describe("hook 상태 저장 위치 소유권", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.readTextFile.mockResolvedValue("");
    mocks.execGit.mockResolvedValue(".git");
    mocks.writeTextFile.mockResolvedValue(undefined);
    mocks.taskRepo.save.mockImplementation(async (value) => value);
  });

  it.each(["feature", "main"])("worktree 없는 %s 브랜치는 공통 위치 판정을 따른다", async (branchName) => {
    const project = { id: "project", name: "repo", repoPath: "/fixture/root", defaultBranch: "main", sshHost: "remote" };
    mocks.projectRepo.findOneBy.mockResolvedValue(project);
    mocks.taskRepo.findOne.mockResolvedValue({ id: "task", title: "task", projectId: "project", project, branchName, worktreePath: null, sshHost: null });
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    expect((await updateHookTaskStatus({ taskId: "task", status: "done" }, VALID_HOOK_AUTHORIZATION)).success).toBe(true);
    if (branchName === "main") {
      expect(mocks.writeTextFile).toHaveBeenCalledWith("/fixture/root/.kanvibe/status.json", expect.stringContaining('"status": "done"'), "remote");
    } else {
      expect(mocks.writeTextFile).not.toHaveBeenCalled();
      expect(mocks.execGit).not.toHaveBeenCalled();
    }
  });

  it("로컬 hook 메타데이터는 저장소에서 읽은 task 범위로만 바인딩한다", async () => {
    const project = { id: "project", name: "repo", repoPath: "/fixture/root", defaultBranch: "main" };
    mocks.taskRepo.findOne.mockResolvedValue({ id: "task", title: "task", projectId: "project", project, worktreePath: "/fixture/feature", sshHost: null });
    const metadata = { provider: "claude", hookPid: 123, payloadBase64: "e30=" };
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    expect((await updateHookTaskStatus({ taskId: "task", status: "progress", agentSession: metadata }, VALID_HOOK_AUTHORIZATION)).success).toBe(true);
    expect(mocks.recordPaneChatSessionBinding).toHaveBeenCalledWith({ taskId: "task", worktreePath: "/fixture/feature", repoPath: "/fixture/root", sshHost: null }, metadata);
  });

  it("원격 task의 메타데이터는 해당 호스트 터널로 들어왔을 때만 바인딩한다", async () => {
    const project = { id: "project", name: "repo", repoPath: "/fixture/root", defaultBranch: "main", sshHost: "remote" };
    mocks.taskRepo.findOne.mockResolvedValue({ id: "task", title: "task", status: "progress", projectId: "project", project, worktreePath: "/fixture/feature", sshHost: "remote" });
    const metadata = { provider: "codex", hookPid: 123, payloadBase64: "e30=" };
    const { updateHookTaskStatus } = await import("@/desktop/main/services/hookService");
    await updateHookTaskStatus({ taskId: "task", status: "progress", agentSession: metadata }, VALID_HOOK_AUTHORIZATION);
    expect(mocks.recordPaneChatSessionBinding).not.toHaveBeenCalled();
    await updateHookTaskStatus({ taskId: "task", status: "progress", agentSession: metadata }, VALID_HOOK_AUTHORIZATION, new Set(["remote"]));
    expect(mocks.recordPaneChatSessionBinding).toHaveBeenCalledWith(expect.objectContaining({ sshHost: "remote" }), metadata);
  });
});
