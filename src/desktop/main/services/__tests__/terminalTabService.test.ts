/**
 * @vitest-environment node
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";

const mockFindOneBy = vi.fn();
vi.mock("@/lib/database", () => ({
  getTaskRepository: async () => ({ findOneBy: mockFindOneBy }),
}));

const mockExecGit = vi.fn();
vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

const mockListLocalTerminalTabs = vi.fn((..._args: unknown[]) => [] as unknown[]);
const mockCreateLocalTerminalTab = vi.fn((..._args: unknown[]) => undefined);
const mockCloseLocalTerminalTab = vi.fn((..._args: unknown[]) => 2);
vi.mock("@/lib/terminal", () => ({
  listLocalTerminalTabs: (...args: unknown[]) => mockListLocalTerminalTabs(...args),
  createLocalTerminalTab: (...args: unknown[]) => mockCreateLocalTerminalTab(...args),
  closeLocalTerminalTab: (...args: unknown[]) => mockCloseLocalTerminalTab(...args),
  renameLocalTerminalTab: vi.fn(),
  selectLocalTerminalTab: vi.fn(),
  moveLocalTerminalTab: vi.fn(),
}));

function stubTask(overrides: Partial<KanbanTask>): void {
  mockFindOneBy.mockResolvedValue({
    id: "task-1",
    sessionType: SessionType.TMUX,
    sessionName: "proj-main",
    sshHost: null,
    worktreePath: "/work/tree",
    ...overrides,
  });
}

/** execGit 호출 중 특정 조각이 든 명령을 찾는다 */
function findExecutedCommand(fragment: string): string | undefined {
  return mockExecGit.mock.calls
    .map((call) => call[0] as string)
    .find((command) => command.includes(fragment));
}

async function importService() {
  return import("@/desktop/main/services/terminalTabService");
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.resetModules();
  mockExecGit.mockResolvedValue("");
});

describe("탭 목록 조회", () => {
  it("tmux window 출력을 탭 목록으로 준다", async () => {
    stubTask({});
    mockExecGit.mockResolvedValue("@1\t0\t1\tzsh\n@2\t1\t0\tvim");
    const { listTerminalTabs } = await importService();

    const result = await listTerminalTabs("task-1");

    expect(result.ok).toBe(true);
    expect(result.tabs).toEqual([
      { id: "@1", index: 0, name: "zsh", isActive: true },
      { id: "@2", index: 1, name: "vim", isActive: false },
    ]);
  });

  it("멀티플렉서 명령이 실패하면 예외 대신 실패 결과를 준다", async () => {
    stubTask({});
    mockExecGit.mockRejectedValue(new Error("tmux: command not found"));
    const { listTerminalTabs } = await importService();

    const result = await listTerminalTabs("task-1");

    expect(result).toEqual({ ok: false, tabs: [], error: "tmux: command not found" });
  });

  it("세션이 없는 태스크는 실패 결과를 준다", async () => {
    mockFindOneBy.mockResolvedValue({ id: "task-1", sessionType: null, sessionName: null });
    const { listTerminalTabs } = await importService();

    const result = await listTerminalTabs("task-1");

    expect(result.ok).toBe(false);
    expect(mockExecGit).not.toHaveBeenCalled();
  });

  it("terminal 세션은 멀티플렉서를 부르지 않고 내부 레지스트리를 읽는다", async () => {
    stubTask({ sessionType: SessionType.TERMINAL });
    mockListLocalTerminalTabs.mockReturnValue([
      { id: "task-1-1", index: 0, name: "zsh", isActive: true },
    ] as never);
    const { listTerminalTabs } = await importService();

    const result = await listTerminalTabs("task-1");

    expect(result.tabs).toHaveLength(1);
    expect(mockExecGit).not.toHaveBeenCalled();
  });
});

describe("탭 생성", () => {
  it("tmux 새 window에 worktree 작업 디렉터리를 넘긴다", async () => {
    stubTask({});
    const { createTerminalTab } = await importService();

    await createTerminalTab("task-1");

    expect(findExecutedCommand("new-window")).toContain("-c '/work/tree'");
  });

  it("zellij 새 탭에도 같은 작업 디렉터리를 넘긴다", async () => {
    stubTask({ sessionType: SessionType.ZELLIJ });
    const { createTerminalTab } = await importService();

    await createTerminalTab("task-1");

    expect(findExecutedCommand("new-tab")).toContain("--cwd '/work/tree'");
  });

  it("terminal 세션은 레지스트리에만 탭을 추가한다", async () => {
    stubTask({ sessionType: SessionType.TERMINAL });
    const { createTerminalTab } = await importService();

    await createTerminalTab("task-1");

    expect(mockCreateLocalTerminalTab).toHaveBeenCalledWith("task-1");
    expect(mockExecGit).not.toHaveBeenCalled();
  });
});

describe("탭 닫기", () => {
  it("tmux 탭을 닫은 뒤 남은 탭 수를 알려 준다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => (
      command.includes("list-windows") ? "@2\t0\t1\tvim" : ""
    ));
    const { closeTerminalTab } = await importService();

    const result = await closeTerminalTab("task-1", "@1");

    expect(findExecutedCommand("kill-window")).toBe("tmux kill-window -t '@1'");
    expect(result).toEqual({ ok: true, remainingCount: 1 });
  });

  it("마지막 탭을 닫아 세션이 사라지면 남은 탭 0으로 본다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("list-windows")) {
        throw new Error("no server running");
      }
      return "";
    });
    const { closeTerminalTab } = await importService();

    expect(await closeTerminalTab("task-1", "@1")).toEqual({ ok: true, remainingCount: 0 });
  });
});

describe("zellij 버전 분기", () => {
  it("0.44 이상이면 탭 id 명령을 쓴다", async () => {
    stubTask({ sessionType: SessionType.ZELLIJ });
    mockExecGit.mockImplementation(async (command: string) => {
      if (command === "zellij --version") return "zellij 0.44.0";
      return "[]";
    });
    const { selectTerminalTab } = await importService();

    await selectTerminalTab("task-1", "2");

    expect(findExecutedCommand("go-to-tab-by-id")).toBe(
      "zellij --session 'proj-main' action go-to-tab-by-id '2'",
    );
  });

  it("0.44 미만이면 이름으로 이동하는 폴백을 쓴다", async () => {
    stubTask({ sessionType: SessionType.ZELLIJ });
    mockExecGit.mockImplementation(async (command: string) => {
      if (command === "zellij --version") return "zellij 0.43.1";
      if (command.includes("query-tab-names")) return "shell\nlogs";
      return "";
    });
    const { selectTerminalTab } = await importService();

    await selectTerminalTab("task-1", "1");

    expect(findExecutedCommand("go-to-tab-name")).toBe(
      "zellij --session 'proj-main' action go-to-tab-name 'logs'",
    );
    expect(findExecutedCommand("go-to-tab-by-id")).toBeUndefined();
  });

  it("버전 확인은 호스트당 한 번만 한다", async () => {
    stubTask({ sessionType: SessionType.ZELLIJ });
    mockExecGit.mockImplementation(async (command: string) => (
      command === "zellij --version" ? "zellij 0.44.0" : "[]"
    ));
    const { listTerminalTabs } = await importService();

    await listTerminalTabs("task-1");
    await listTerminalTabs("task-1");

    const versionCalls = mockExecGit.mock.calls.filter((call) => call[0] === "zellij --version");
    expect(versionCalls).toHaveLength(1);
  });
});

describe("탭 이름 변경", () => {
  it("빈 이름은 거부하고 명령을 실행하지 않는다", async () => {
    stubTask({});
    const { renameTerminalTab } = await importService();

    const result = await renameTerminalTab("task-1", "@1", "   ");

    expect(result.ok).toBe(false);
    expect(mockExecGit).not.toHaveBeenCalled();
  });

  it("tmux는 -- 뒤에 이름을 둬 대시로 시작하는 이름도 받는다", async () => {
    stubTask({});
    const { renameTerminalTab } = await importService();

    await renameTerminalTab("task-1", "@1", "-build");

    expect(findExecutedCommand("rename-window")).toBe("tmux rename-window -t '@1' -- '-build'");
  });
});

describe("탭 순서 변경", () => {
  it("tmux는 목표 window 앞에 끼운 뒤 번호를 다시 채운다", async () => {
    stubTask({});
    mockExecGit.mockResolvedValue("@1\t0\t0\tzsh\n@2\t1\t0\tvim\n@3\t2\t1\ttop");
    const { moveTerminalTab } = await importService();

    await moveTerminalTab("task-1", "@3", 0);

    expect(mockExecGit.mock.calls.map((call) => call[0]).slice(1)).toEqual([
      "tmux move-window -b -s '@3' -t 'proj-main':0",
      "tmux move-window -r -t 'proj-main'",
    ]);
  });

  /** 탭 바는 배열 위치를 넘기므로, base-index가 1이면 tmux가 매긴 번호와 어긋난다 */
  it("tmux 목표 인덱스는 배열 위치가 아니라 그 자리 window의 번호로 옮긴다", async () => {
    stubTask({});
    mockExecGit.mockResolvedValue("@1\t1\t0\tzsh\n@2\t2\t0\tvim\n@3\t3\t1\ttop");
    const { moveTerminalTab } = await importService();

    await moveTerminalTab("task-1", "@3", 0);

    expect(findExecutedCommand("move-window -b")).toBe("tmux move-window -b -s '@3' -t 'proj-main':1");
  });

  it("목록에 없는 탭은 옮기지 않는다", async () => {
    stubTask({});
    mockExecGit.mockResolvedValue("@1\t0\t1\tzsh");
    const { moveTerminalTab } = await importService();

    const result = await moveTerminalTab("task-1", "@9", 0);

    expect(result.ok).toBe(false);
    expect(findExecutedCommand("move-window")).toBeUndefined();
  });

  it("음수 인덱스는 거부한다", async () => {
    stubTask({});
    const { moveTerminalTab } = await importService();

    const result = await moveTerminalTab("task-1", "@3", -1);

    expect(result.ok).toBe(false);
    expect(mockExecGit).not.toHaveBeenCalled();
  });
});
