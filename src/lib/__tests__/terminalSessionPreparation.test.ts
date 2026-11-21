/**
 * @vitest-environment node
 */
import { spawnSync } from "child_process";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const mockExecSync = vi.fn();
vi.mock("child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("child_process")>();
  return { ...actual, execSync: (...args: unknown[]) => mockExecSync(...args) };
});

const mockExecGit = vi.fn(async (..._args: unknown[]): Promise<string> => "");
vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

const mockExistsSync = vi.fn(() => false);
vi.mock("fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("fs")>();
  return { ...actual, existsSync: (...args: unknown[]) => mockExistsSync(...(args as [])) };
});

const mockEnsureHerdrSession = vi.fn();
const mockListHerdrTaskPaneTabIds = vi.fn();
vi.mock("@/lib/herdr", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/herdr")>();
  return {
    ...actual,
    ensureHerdrSession: (...args: unknown[]) => mockEnsureHerdrSession(...args),
    listHerdrTaskPaneTabIds: (...args: unknown[]) => mockListHerdrTaskPaneTabIds(...args),
  };
});

function baseTarget(overrides: Partial<import("@/lib/terminal").TerminalSessionPreparationTarget> = {}) {
  return {
    taskId: "task-1",
    sessionType: SessionType.TMUX,
    sessionName: "repo-feat-new",
    worktreePath: "/repo__worktrees/feat-new",
    sshHost: null,
    tmuxPaneLayout: null,
    ...overrides,
  };
}

/** tmux 세션 생성 명령(new-session)이 실행된 횟수. 존재 확인·설정 확인 명령은 세지 않는다 */
function countTmuxBootstraps(): number {
  return mockExecSync.mock.calls.filter(([command]) => String(command).includes("new-session")).length;
}

function expectValidPosixShellSyntax(command: string): void {
  const result = spawnSync("sh", ["-n", "-c", command], { encoding: "utf-8" });
  expect(result.status, result.stderr).toBe(0);
}

describe("prepareTerminalSession", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
    mockExistsSync.mockReturnValue(false);
  });

  it("로컬 tmux 세션이 없으면 화면 attach와 같은 부트스트랩으로 attach 없이 한 번 만든다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");
    mockExecSync.mockImplementation((command: string) => {
      if (command.startsWith("tmux has-session")) throw new Error("no session");
      return "";
    });

    await expect(prepareTerminalSession(baseTarget())).resolves.toEqual({ isPrepared: true });

    expect(countTmuxBootstraps()).toBe(1);
    const bootstrapCommand = mockExecSync.mock.calls.map(([command]) => String(command)).find((command) => command.includes("new-session"))!;
    expect(bootstrapCommand).toContain("/repo__worktrees/feat-new");
    expect(bootstrapCommand).not.toContain("attach-session");
  });

  it("같은 세션의 준비가 동시에 두 번 들어오면 부트스트랩을 한 번만 실행한다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");
    mockExecSync.mockImplementation((command: string) => {
      if (command.startsWith("tmux has-session")) throw new Error("no session");
      return "";
    });

    const [first, second] = await Promise.all([prepareTerminalSession(baseTarget()), prepareTerminalSession(baseTarget())]);

    expect([first, second]).toEqual([{ isPrepared: true }, { isPrepared: true }]);
    expect(countTmuxBootstraps()).toBe(1);
  });

  it("이미 살아 있는 tmux 세션은 다시 만들지 않는다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");
    mockExecSync.mockReturnValue("");

    await expect(prepareTerminalSession(baseTarget())).resolves.toEqual({ isPrepared: true });

    expect(countTmuxBootstraps()).toBe(0);
  });

  it("terminal 타입은 미리 만들 세션이 없다고 답하고 아무 명령도 실행하지 않는다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");

    const prepared = await prepareTerminalSession(baseTarget({ sessionType: SessionType.TERMINAL }));

    expect(prepared.isPrepared).toBe(false);
    expect(prepared.skippedReason).toContain("미리 만들 세션이 없습니다");
    expect(mockExecSync).not.toHaveBeenCalled();
    expect(mockExecGit).not.toHaveBeenCalled();
  });

  it("로컬 zellij 세션이 없으면 worktree에서 레이아웃 파일을 적용해 background로 만든다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");
    mockExistsSync.mockReturnValue(true);
    mockExecSync.mockImplementation((command: string) => (command === "zellij list-sessions" ? "" : ""));

    await prepareTerminalSession(baseTarget({ sessionType: SessionType.ZELLIJ }));

    const createCall = mockExecSync.mock.calls.find(([command]) => String(command).includes("--create-background"));
    expect(createCall?.[0]).toBe("zellij --layout '/repo__worktrees/feat-new/.zellij-layout.kdl' attach --create-background 'repo-feat-new'");
    expect(createCall?.[1]).toMatchObject({ cwd: "/repo__worktrees/feat-new" });
  });

  it("원격 tmux는 attach 없이 세션만 만드는 명령을 로그인 셸로 한 번 실행한다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");

    await prepareTerminalSession(baseTarget({ sshHost: "devbox" }));

    expect(mockExecGit).toHaveBeenCalledTimes(1);
    const [remoteCommand, sshHost] = mockExecGit.mock.calls[0] as [string, string];
    expect(sshHost).toBe("devbox");
    expect(remoteCommand.startsWith("sh -lc ")).toBe(true);
    expect(remoteCommand).toContain("has-session");
    expect(remoteCommand).toContain("new-session");
    expect(remoteCommand).not.toContain("attach-session");
    expectValidPosixShellSyntax(remoteCommand);
  });

  it("원격 zellij는 살아 있지 않을 때만 background로 만든다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");

    await prepareTerminalSession(baseTarget({ sessionType: SessionType.ZELLIJ, sshHost: "devbox" }));

    const [remoteCommand] = mockExecGit.mock.calls[0] as [string];
    expect(remoteCommand).toContain("attach --create-background");
    expect(remoteCommand).toContain(".zellij-layout.kdl");
    expectValidPosixShellSyntax(remoteCommand);
  });

  it("herdr는 기존 Space 준비 함수로 만들고 그 Space의 pane을 돌려준다", async () => {
    const { prepareTerminalSession } = await import("@/lib/terminal");
    mockEnsureHerdrSession.mockResolvedValue("w7");
    mockListHerdrTaskPaneTabIds.mockResolvedValue([{ paneId: "w7:p1", tabId: "w7:t1", agentSession: { provider: "claude", sessionId: "s" } }]);

    const prepared = await prepareTerminalSession(baseTarget({ sessionType: SessionType.HERDR, sessionName: "kanvibe:repo-feat-new", sshHost: "devbox" }));

    expect(mockEnsureHerdrSession).toHaveBeenCalledWith("kanvibe:repo-feat-new", "/repo__worktrees/feat-new", null, "devbox");
    expect(prepared).toEqual({ isPrepared: true, herdrWorkspaceId: "w7", herdrPanes: [{ paneId: "w7:p1", tabId: "w7:t1" }] });
  });
});
