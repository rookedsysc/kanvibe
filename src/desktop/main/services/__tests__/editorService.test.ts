import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: { findOneBy: vi.fn() },
  projectRepo: { findOneBy: vi.fn() },
  execFile: vi.fn(),
  createLocalShellEnvironment: vi.fn(() => ({ PATH: "/usr/local/bin" })),
}));

vi.mock("child_process", () => ({
  execFile: mocks.execFile,
  default: { execFile: mocks.execFile },
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
  getProjectRepository: vi.fn(async () => mocks.projectRepo),
}));

vi.mock("@/lib/shellEnvironment", () => ({
  createLocalShellEnvironment: mocks.createLocalShellEnvironment,
}));

function resolveExecFileSuccessfully() {
  mocks.execFile.mockImplementation((_command, _args, _options, callback) => {
    callback(null, "", "");
    return {} as never;
  });
}

describe("editorService", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.createLocalShellEnvironment.mockReturnValue({ PATH: "/usr/local/bin" });
    resolveExecFileSuccessfully();
  });

  it("로컬 작업은 worktree 경로만 넘겨 VS Code를 연다", async () => {
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-1",
      projectId: "project-1",
      worktreePath: "/workspace/repo__worktrees/feature",
      sshHost: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue({ id: "project-1", repoPath: "/workspace/repo", sshHost: null });

    const { openTaskInVsCode } = await import("@/desktop/main/services/editorService");

    await expect(openTaskInVsCode("task-1")).resolves.toEqual({ ok: true });
    expect(mocks.execFile).toHaveBeenCalledWith(
      "code",
      ["/workspace/repo__worktrees/feature"],
      expect.objectContaining({ env: { PATH: "/usr/local/bin" } }),
      expect.any(Function),
    );
  });

  /**
   * 원격 셸에서 `code`를 부르면 그 머신에서 창을 띄우려 하므로 사용자 화면에는 아무것도 안 뜬다.
   * 로컬 VS Code에 붙을 원격 호스트를 알려 주는 방향이어야 한다.
   */
  it("원격 작업은 로컬 VS Code를 Remote-SSH로 붙여서 연다", async () => {
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-2",
      projectId: "project-remote",
      worktreePath: "/remote/repo__worktrees/feature",
      sshHost: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-remote",
      repoPath: "/remote/repo",
      sshHost: "build-box",
    });

    const { openTaskInVsCode } = await import("@/desktop/main/services/editorService");

    await expect(openTaskInVsCode("task-2")).resolves.toEqual({ ok: true });
    expect(mocks.execFile).toHaveBeenCalledWith(
      "code",
      ["--remote", "ssh-remote+build-box", "/remote/repo__worktrees/feature"],
      expect.anything(),
      expect.any(Function),
    );
  });

  it("worktree가 없으면 프로젝트 저장소 경로를 연다", async () => {
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-3",
      projectId: "project-1",
      worktreePath: null,
      sshHost: null,
    });
    mocks.projectRepo.findOneBy.mockResolvedValue({ id: "project-1", repoPath: "/workspace/repo", sshHost: null });

    const { openTaskInVsCode } = await import("@/desktop/main/services/editorService");

    await openTaskInVsCode("task-3");

    expect(mocks.execFile).toHaveBeenCalledWith("code", ["/workspace/repo"], expect.anything(), expect.any(Function));
  });

  it("열 경로를 찾지 못하면 VS Code를 실행하지 않는다", async () => {
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-4",
      projectId: null,
      worktreePath: null,
      sshHost: null,
    });

    const { openTaskInVsCode } = await import("@/desktop/main/services/editorService");

    await expect(openTaskInVsCode("task-4")).resolves.toEqual({ ok: false, error: "no-target-path" });
    expect(mocks.execFile).not.toHaveBeenCalled();
  });

  it("code 명령을 찾지 못하면 실패 사유를 돌려준다", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.taskRepo.findOneBy.mockResolvedValue({
      id: "task-5",
      projectId: null,
      worktreePath: "/workspace/repo__worktrees/feature",
      sshHost: null,
    });
    mocks.execFile.mockImplementation((_command, _args, _options, callback) => {
      callback(Object.assign(new Error("spawn code ENOENT"), { code: "ENOENT" }), "", "");
      return {} as never;
    });

    const { openTaskInVsCode } = await import("@/desktop/main/services/editorService");

    await expect(openTaskInVsCode("task-5")).resolves.toEqual({ ok: false, error: "spawn code ENOENT" });
    consoleErrorSpy.mockRestore();
  });
});
