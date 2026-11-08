import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  class DirtyWorktreeError extends Error {
    constructor(readonly worktreePath: string) {
      super(`커밋하지 않은 변경이 있어 삭제하지 않았습니다: ${worktreePath}`);
      this.name = "DirtyWorktreeError";
    }
  }

  return {
    DirtyWorktreeError,
    taskRepo: { findOneBy: vi.fn() },
    projectRepo: { findOneBy: vi.fn() },
    createTask: vi.fn(),
    deleteTaskAcrossTargets: vi.fn(),
    getTaskIdByProjectAndBranch: vi.fn(),
    syncRegisteredProjectWorktrees: vi.fn(),
    isValidTaskCliToken: vi.fn(),
    propagateTaskRegistrationToPeers: vi.fn(),
    broadcastBoardUpdate: vi.fn(),
  };
});

vi.mock("@/entities/KanbanTask", () => ({
  SessionType: { TMUX: "tmux", ZELLIJ: "zellij", HERDR: "herdr", TERMINAL: "terminal" },
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
  getProjectRepository: vi.fn(async () => mocks.projectRepo),
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
}));

vi.mock("@/desktop/main/services/kanbanService", () => ({
  createTask: mocks.createTask,
  deleteTaskAcrossTargets: mocks.deleteTaskAcrossTargets,
  DirtyWorktreeError: mocks.DirtyWorktreeError,
  getTaskIdByProjectAndBranch: mocks.getTaskIdByProjectAndBranch,
}));

vi.mock("@/desktop/main/services/projectService", () => ({
  syncRegisteredProjectWorktrees: mocks.syncRegisteredProjectWorktrees,
}));

vi.mock("@/desktop/main/services/taskCliTokenService", () => ({
  isValidTaskCliToken: mocks.isValidTaskCliToken,
}));

vi.mock("@/desktop/main/services/taskTargetPropagationService", () => ({
  propagateTaskRegistrationToPeers: mocks.propagateTaskRegistrationToPeers,
}));

const { createTaskFromCli, deleteTaskFromCli, registerTaskFromPeer } = await import(
  "@/desktop/main/services/taskCliService"
);

const AUTHORIZATION = "Bearer valid-token";
const PROJECT = { id: "project-1", repoPath: "/remote/repo", sshHost: "devbox", defaultBranch: "main" };
const CONTEXT_TASK = {
  id: "context-task",
  projectId: "project-1",
  branchName: "feat/current",
  sessionType: "tmux",
  worktreePath: "/remote/repo__worktrees/feat-current",
};
const PEER_RESULTS = [{ url: "http://10.0.0.7:9736", isSuccess: true, httpStatus: "200", message: "ok" }];

/** id나 브랜치, 프로젝트가 모두 맞는 task만 돌려줘 실제 저장소 조회처럼 프로젝트 경계를 지킨다 */
function givenTasks(tasks: Array<Record<string, unknown>>): void {
  mocks.taskRepo.findOneBy.mockImplementation(async (where: Record<string, unknown>) => (
    tasks.find((task) => Object.entries(where).every(([key, value]) => task[key] === value)) ?? null
  ));
}

async function expectCliError(request: Promise<unknown>, statusCode: number, message?: string): Promise<void> {
  await expect(request).rejects.toMatchObject({
    name: "TaskCliRequestError",
    statusCode,
    ...(message ? { message: expect.stringContaining(message) } : {}),
  });
}

describe("taskCliService", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.isValidTaskCliToken.mockImplementation(async (taskId: string, token: string) => (
      taskId === CONTEXT_TASK.id && token === "valid-token"
    ));
    givenTasks([CONTEXT_TASK]);
    mocks.projectRepo.findOneBy.mockResolvedValue(PROJECT);
    mocks.getTaskIdByProjectAndBranch.mockResolvedValue(null);
    mocks.propagateTaskRegistrationToPeers.mockResolvedValue(PEER_RESULTS);
    mocks.createTask.mockImplementation(async (input: Record<string, unknown>) => ({
      id: "created-task",
      status: "todo",
      worktreePath: "/remote/repo__worktrees/feat-new",
      ...input,
    }));
  });

  describe("인증", () => {
    it("토큰이 없거나 다른 task의 토큰이면 task를 조회하지 않고 401로 거절한다", async () => {
      await expectCliError(createTaskFromCli(undefined, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", baseBranch: "main" }), 401);
      await expectCliError(createTaskFromCli("Bearer other", { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", baseBranch: "main" }), 401);

      expect(mocks.taskRepo.findOneBy).not.toHaveBeenCalled();
      expect(mocks.createTask).not.toHaveBeenCalled();
    });

    it("토큰을 확인하다 내부 오류가 나면 인증 전이므로 원인을 감춘 500으로 답한다", async () => {
      vi.spyOn(console, "error").mockImplementation(() => {});
      mocks.isValidTaskCliToken.mockRejectedValue(new Error("SQLITE_BUSY: /Users/secret/kanvibe.db"));

      const request = deleteTaskFromCli("Bearer anything", { contextTaskId: CONTEXT_TASK.id, confirmed: true });

      await expect(request).rejects.toMatchObject({ name: "TaskCliRequestError", statusCode: 500 });
      await expect(request).rejects.not.toMatchObject({ message: expect.stringContaining("secret") });
      expect(mocks.taskRepo.findOneBy).not.toHaveBeenCalled();
    });

    it("다른 호스트의 터널로 들어온 요청은 토큰이 맞아도 401로 거절해 새어 나간 토큰으로 이 호스트의 task를 지우지 못하게 한다", async () => {
      const body = { contextTaskId: CONTEXT_TASK.id, confirmed: true };

      await expectCliError(deleteTaskFromCli(AUTHORIZATION, body, new Set(["other-host"])), 401);
      await expectCliError(
        createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", baseBranch: "main" }, new Set(["other-host"])),
        401,
      );
      await expectCliError(registerTaskFromPeer(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new" }, new Set(["other-host"])), 401);
      expect(mocks.deleteTaskAcrossTargets).not.toHaveBeenCalled();
      expect(mocks.createTask).not.toHaveBeenCalled();
    });

    it("task의 호스트가 비어 있으면 프로젝트 호스트의 터널에서 온 요청을 받는다", async () => {
      mocks.deleteTaskAcrossTargets.mockResolvedValue({ isDeleted: true, peerResults: [] });

      await expect(
        deleteTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, confirmed: true }, new Set([PROJECT.sshHost])),
      ).resolves.toMatchObject({ id: CONTEXT_TASK.id, deleted: true });
    });

    it("프로젝트에 연결되지 않은 task에서 온 요청은 409로 거절한다", async () => {
      mocks.projectRepo.findOneBy.mockResolvedValue(null);

      await expectCliError(deleteTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, confirmed: true }), 409);
      expect(mocks.deleteTaskAcrossTargets).not.toHaveBeenCalled();
    });
  });

  describe("생성", () => {
    it("기본 생성은 CLI가 보낸 현재 브랜치에서 만들고 요청한 task의 세션 타입을 이어받은 뒤 다른 client에 등록을 전파한다", async () => {
      const created = await createTaskFromCli(AUTHORIZATION, {
        contextTaskId: CONTEXT_TASK.id,
        branchName: "feat/new",
        baseBranch: "feat/current",
        title: "로그인 \"수정\"",
      });

      expect(mocks.createTask).toHaveBeenCalledWith({
        title: "로그인 \"수정\"",
        branchName: "feat/new",
        baseBranch: "feat/current",
        sessionType: "tmux",
        projectId: "project-1",
      });
      expect(mocks.propagateTaskRegistrationToPeers).toHaveBeenCalledWith("/remote/repo", "feat/new", "devbox");
      expect(created).toMatchObject({ id: "created-task", baseBranch: "feat/current", peers: PEER_RESULTS });
    });

    it("--from-task로 지정한 같은 프로젝트 task의 브랜치를 기준으로 쓰고 그 task의 세션 타입을 이어받는다", async () => {
      givenTasks([CONTEXT_TASK, { id: "base-task", projectId: "project-1", branchName: "feat/base", sessionType: "zellij" }]);

      await createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", fromTaskId: "base-task" });

      expect(mocks.createTask).toHaveBeenCalledWith(expect.objectContaining({ baseBranch: "feat/base", sessionType: "zellij" }));
    });

    it("명시한 세션 타입이 이어받는 값보다 앞서고, 둘 다 없으면 terminal을 쓴다", async () => {
      givenTasks([{ ...CONTEXT_TASK, sessionType: null }]);

      await createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/a", baseBranch: "main" });
      await createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/b", baseBranch: "main", sessionType: "herdr" });

      expect(mocks.createTask.mock.calls.map(([input]) => input.sessionType)).toEqual(["terminal", "herdr"]);
    });

    it("다른 프로젝트의 task나 브랜치 없는 task는 기준으로 쓰지 않는다", async () => {
      givenTasks([
        CONTEXT_TASK,
        { id: "other-project-task", projectId: "project-2", branchName: "feat/other" },
        { id: "branchless-task", projectId: "project-1", branchName: null },
      ]);

      await expectCliError(createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", fromTaskId: "other-project-task" }), 404);
      await expectCliError(createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", fromTaskId: "branchless-task" }), 409);
      expect(mocks.createTask).not.toHaveBeenCalled();
    });

    it.each([
      [{ baseBranch: "main" }, "branchName는 필수"],
      [{ branchName: "feat/$(rm -rf ~)", baseBranch: "main" }, "branchName에 쓸 수 없는 문자"],
      [{ branchName: "feat/new" }, "baseBranch는 필수"],
      [{ branchName: "feat/new", baseBranch: "main", sessionType: "screen" }, "sessionType 값이 올바르지 않습니다"],
    ])("잘못된 입력 %o은 400으로 거절하고 아무것도 만들지 않는다", async (fields, message) => {
      await expectCliError(createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, ...fields }), 400, message);
      expect(mocks.createTask).not.toHaveBeenCalled();
    });

    it("같은 프로젝트에 같은 브랜치 task가 있으면 409로 거절한다", async () => {
      mocks.getTaskIdByProjectAndBranch.mockResolvedValue("existing-task");

      await expectCliError(createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", baseBranch: "main" }), 409);
      expect(mocks.createTask).not.toHaveBeenCalled();
    });

    it("worktree 생성이 실패하면 다른 client에 등록을 전파하지 않는다", async () => {
      mocks.createTask.mockRejectedValue(new Error("fatal: a branch named 'feat/new' already exists"));

      await expect(createTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new", baseBranch: "main" }))
        .rejects.toThrow("already exists");
      expect(mocks.propagateTaskRegistrationToPeers).not.toHaveBeenCalled();
    });
  });

  describe("삭제", () => {
    beforeEach(() => {
      mocks.deleteTaskAcrossTargets.mockResolvedValue({ isDeleted: true, peerResults: PEER_RESULTS });
    });

    it("확인 없이는 지우지 않는다", async () => {
      await expectCliError(deleteTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id }), 400);
      expect(mocks.deleteTaskAcrossTargets).not.toHaveBeenCalled();
    });

    it("브랜치를 주지 않으면 요청한 task를, 변경이 있으면 거부하는 정책으로 지우고 다른 client에 전파한다", async () => {
      const deleted = await deleteTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, confirmed: true });

      expect(mocks.deleteTaskAcrossTargets).toHaveBeenCalledWith("context-task", {
        shouldRejectDirtyWorktree: true,
        shouldPropagateToPeers: true,
      });
      expect(deleted).toEqual({ id: "context-task", branchName: "feat/current", deleted: true, peers: PEER_RESULTS });
    });

    it("같은 프로젝트의 다른 task는 브랜치로 가리키고, 다른 client가 보낸 전파 요청은 다시 전파하지 않는다", async () => {
      givenTasks([CONTEXT_TASK, { id: "sibling-task", projectId: "project-1", branchName: "feat/sibling" }]);

      await deleteTaskFromCli(AUTHORIZATION, {
        contextTaskId: CONTEXT_TASK.id,
        branchName: "feat/sibling",
        confirmed: true,
        force: true,
        propagateToPeers: false,
      });

      expect(mocks.deleteTaskAcrossTargets).toHaveBeenCalledWith("sibling-task", {
        shouldRejectDirtyWorktree: false,
        shouldPropagateToPeers: false,
      });
    });

    it("다른 프로젝트의 브랜치는 찾지 못한 것으로 거절한다", async () => {
      givenTasks([CONTEXT_TASK, { id: "foreign-task", projectId: "project-2", branchName: "feat/foreign" }]);

      await expectCliError(deleteTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/foreign", confirmed: true }), 404);
      expect(mocks.deleteTaskAcrossTargets).not.toHaveBeenCalled();
    });

    it("커밋하지 않은 변경 때문에 멈추면 --force 안내와 함께 409로 알린다", async () => {
      mocks.deleteTaskAcrossTargets.mockRejectedValue(new mocks.DirtyWorktreeError(CONTEXT_TASK.worktreePath));

      await expectCliError(deleteTaskFromCli(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, confirmed: true }), 409, "--force");
    });
  });

  describe("다른 client의 등록 요청", () => {
    it("이미 등록된 브랜치면 worktree를 다시 훑지 않고 그 task를 돌려준다", async () => {
      mocks.getTaskIdByProjectAndBranch.mockResolvedValue("known-task");

      await expect(registerTaskFromPeer(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new" }))
        .resolves.toEqual({ id: "known-task", branchName: "feat/new" });
      expect(mocks.syncRegisteredProjectWorktrees).not.toHaveBeenCalled();
      expect(mocks.createTask).not.toHaveBeenCalled();
    });

    it("새 브랜치면 worktree를 만들지 않고 그 프로젝트의 worktree 동기화로 task를 등록한다", async () => {
      mocks.getTaskIdByProjectAndBranch.mockResolvedValueOnce(null).mockResolvedValueOnce("registered-task");
      mocks.syncRegisteredProjectWorktrees.mockResolvedValue({ changed: true });

      await expect(registerTaskFromPeer(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/new" }))
        .resolves.toEqual({ id: "registered-task", branchName: "feat/new" });
      expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledWith([PROJECT]);
      expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);
      expect(mocks.createTask).not.toHaveBeenCalled();
    });

    it("동기화 뒤에도 worktree가 없으면 404로 알린다", async () => {
      mocks.syncRegisteredProjectWorktrees.mockResolvedValue({ changed: false });

      await expectCliError(registerTaskFromPeer(AUTHORIZATION, { contextTaskId: CONTEXT_TASK.id, branchName: "feat/missing" }), 404);
    });
  });
});
