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
const mockGetLocalTerminalTabProcess = vi.fn();
const mockFocusHerdrTaskTab = vi.fn(async (..._args: unknown[]) => {});
const mockGetHerdrTaskActiveTabId = vi.fn((): string | null => null);
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/hostFileAccess")>(),
  readTextFile: async () => JSON.stringify({ "task-a": "w1" }),
}));
const mockCloseLocalTerminalTab = vi.fn((..._args: unknown[]) => 2);
vi.mock("@/lib/terminal", () => ({
  focusHerdrTaskTab: (...args: unknown[]) => mockFocusHerdrTaskTab(...args),
  getHerdrTaskActiveTabId: () => mockGetHerdrTaskActiveTabId(),
  getLocalTerminalTabProcess: (...args: unknown[]) => mockGetLocalTerminalTabProcess(...args),
  listLocalTerminalTabs: (...args: unknown[]) => mockListLocalTerminalTabs(...args),
  listRemoteTerminalTabs: async (...args: unknown[]) => mockListLocalTerminalTabs(args[0]),
  createLocalTerminalTab: (...args: unknown[]) => mockCreateLocalTerminalTab(...args),
  closeLocalTerminalTab: (...args: unknown[]) => mockCloseLocalTerminalTab(...args),
  closeRemoteTerminalTab: async (...args: unknown[]) => mockCloseLocalTerminalTab(args[0], args[1]),
  renameLocalTerminalTab: vi.fn(),
  selectLocalTerminalTab: vi.fn(),
  moveLocalTerminalTab: vi.fn(),
}));

const mockMoveHerdrTab = vi.fn();
vi.mock("@/lib/herdr", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/herdr")>(),
  moveHerdrTab: (...args: unknown[]) => mockMoveHerdrTab(...args),
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
  mockGetHerdrTaskActiveTabId.mockReturnValue(null);
});

describe("탭 목록 조회", () => {
  it("tmux window 출력을 탭 목록으로 준다", async () => {
    stubTask({});
    mockExecGit.mockResolvedValue("@1\t0\t1\tzsh\n@2\t1\t0\tvim");
    const { listTerminalTabs } = await importService();

    const result = await listTerminalTabs("task-1");

    expect(result.ok).toBe(true);
    expect(result.tabs).toEqual([
      { id: "@1", nativeIndex: 0, name: "zsh", isActive: true },
      { id: "@2", nativeIndex: 1, name: "vim", isActive: false },
    ]);
  });

  it("멀티플렉서 명령이 실패하면 예외 대신 실패 결과를 준다", async () => {
    stubTask({});
    mockExecGit.mockRejectedValue(new Error("tmux: command not found"));
    const { listTerminalTabs } = await importService();

    const result = await listTerminalTabs("task-1");

    expect(result).toEqual({ ok: false, tabs: [], error: "tmux: command not found" });
  });

  it("세션이 꺼져 있으면 실패하는 tmux window 목록 조회는 실패를 오류 로그로 남기지 않게 요청한다", async () => {
    stubTask({ sshHost: "remote-mac" });
    mockExecGit.mockResolvedValue("@1\t0\t1\tzsh");
    const { listTerminalTabs } = await importService();

    await listTerminalTabs("task-1");

    const listWindowsCall = mockExecGit.mock.calls.find(([command]) => String(command).includes("list-windows"));
    expect(listWindowsCall?.[1]).toBe("remote-mac");
    expect(listWindowsCall?.[2]).toMatchObject({ shouldLogFailure: false });
  });

  it("탭 닫기 같은 사용자 조작 명령의 실패는 계속 오류 로그로 남긴다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => (
      command.includes("list-windows") ? "@1\t0\t1\tzsh\n@2\t1\t0\tvim" : ""
    ));
    const { closeTerminalTab } = await importService();

    await closeTerminalTab("task-1", "@1");

    const killWindowCall = mockExecGit.mock.calls.find(([command]) => String(command).includes("kill-window"));
    expect(killWindowCall?.[2]).toMatchObject({ shouldLogFailure: true });
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
      { id: "task-1-1", nativeIndex: 0, name: "zsh", isActive: true },
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
  it("탭이 남아 있으면 tmux window를 닫고 창은 유지한다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => (
      command.includes("list-windows") ? "@1\t0\t1\tzsh\n@2\t1\t0\tvim" : ""
    ));
    const { closeTerminalTab } = await importService();

    const result = await closeTerminalTab("task-1", "@1");

    expect(findExecutedCommand("kill-window")).toBe("tmux kill-window -t '@1'");
    expect(result).toEqual({ ok: true, shouldCloseWindow: false });
  });

  /** 마지막 window를 죽이면 tmux 세션까지 사라진다. 세션 정리는 태스크 삭제 경로에만 있어야 한다 */
  it("tmux window가 하나뿐이면 닫지 않고 창만 닫는다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => (
      command.includes("list-windows") ? "@1\t0\t1\tzsh" : ""
    ));
    const { closeTerminalTab } = await importService();

    const result = await closeTerminalTab("task-1", "@1");

    expect(findExecutedCommand("kill-window")).toBeUndefined();
    expect(result).toEqual({ ok: true, shouldCloseWindow: true });
  });

  it.each([SessionType.TMUX, SessionType.ZELLIJ])("%s의 두 탭을 동시에 닫아도 마지막 탭은 남긴다", async (sessionType) => {
    stubTask({ sessionType });
    let tabs = ["1", "2"];
    mockExecGit.mockImplementation(async (command: string) => {
      if (command === "zellij --version") return "zellij 0.44.0";
      if (command.includes("list-windows")) return tabs.map((id, index) => `@${id}\t${index}\t1\tshell`).join("\n");
      if (command.includes("list-tabs")) return JSON.stringify(tabs.map((id, position) => ({ tab_id: Number(id), position, name: "shell", active: true })));
      const closedId = command.match(/(?:kill-window -t '@|close-tab-by-id ')(\d+)'/)?.[1];
      if (closedId) tabs = tabs.filter((id) => id !== closedId);
      return "";
    });
    const { closeTerminalTab } = await importService();

    const results = await Promise.all([
      closeTerminalTab("task-1", sessionType === SessionType.TMUX ? "@1" : "1"),
      closeTerminalTab("task-1", sessionType === SessionType.TMUX ? "@2" : "2"),
    ]);

    expect(tabs).toEqual(["2"]);
    expect(results).toEqual([{ ok: true, shouldCloseWindow: false }, { ok: true, shouldCloseWindow: true }]);
  });

  it.each(["host", "session"])("다른 %s의 탭 종료는 앞 세션을 기다리지 않는다", async (boundary) => {
    let releaseFirst!: () => void;
    let firstEntered!: () => void;
    const firstStarted = new Promise<void>((resolve) => { firstEntered = resolve; });
    const firstGate = new Promise<void>((resolve) => { releaseFirst = resolve; });
    mockFindOneBy.mockImplementation(async ({ id }: { id: string }) => ({
      sessionType: SessionType.TMUX,
      sessionName: boundary === "session" ? id : "same-session",
      sshHost: boundary === "host" ? id : "same-host",
      worktreePath: "/work/tree",
    }));
    mockExecGit.mockImplementation(async (command: string, host: string) => {
      if (command.includes("list-windows")) {
        if (host === "first" || command.includes("'first'")) {
          firstEntered();
          await firstGate;
        }
        return "@1\t0\t1\tzsh\n@2\t1\t0\tvim";
      }
      return "";
    });
    const { closeTerminalTab } = await importService();
    const first = closeTerminalTab("first", "@1");
    await firstStarted;
    try {
      expect(await closeTerminalTab("second", "@1")).toEqual({ ok: true, shouldCloseWindow: false });
    } finally {
      releaseFirst();
    }
    expect(await first).toEqual({ ok: true, shouldCloseWindow: false });
  });

  it("앞선 탭 종료가 실패해도 같은 세션의 다음 종료를 처리한다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("list-windows")) return "@1\t0\t1\tzsh\n@2\t1\t0\tvim";
      if (command.includes("'@1'")) throw new Error("close failed");
      return "";
    });
    const { closeTerminalTab } = await importService();

    const results = await Promise.all([closeTerminalTab("task-1", "@1"), closeTerminalTab("task-1", "@2")]);

    expect(results).toEqual([{ ok: false, error: "close failed" }, { ok: true, shouldCloseWindow: false }]);
    expect(findExecutedCommand("kill-window -t '@2'")).toBeDefined();
  });

  it("zellij tab이 하나뿐이면 닫지 않고 창만 닫는다", async () => {
    stubTask({ sessionType: SessionType.ZELLIJ });
    mockExecGit.mockImplementation(async (command: string) => {
      if (command === "zellij --version") return "zellij 0.44.0";
      if (command.includes("list-tabs")) {
        return JSON.stringify([{ tab_id: 0, position: 0, name: "Tab #1", active: true }]);
      }
      return "";
    });
    const { closeTerminalTab } = await importService();

    const result = await closeTerminalTab("task-1", "0");

    expect(findExecutedCommand("close-tab")).toBeUndefined();
    expect(result).toEqual({ ok: true, shouldCloseWindow: true });
  });

  it("세션 목록을 읽지 못하면 아무것도 지우지 않고 창만 닫는다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("list-windows")) {
        throw new Error("no server running");
      }
      return "";
    });
    const { closeTerminalTab } = await importService();

    const result = await closeTerminalTab("task-1", "@1");

    expect(findExecutedCommand("kill-window")).toBeUndefined();
    expect(result).toEqual({ ok: true, shouldCloseWindow: true });
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


describe("Herdr 탭", () => {
  it("네이티브 ID로 동일 이름 탭을 구분하고 활성 상태를 유지한다", async () => {
    stubTask({ sessionType: "herdr" as SessionType });
    mockExecGit.mockResolvedValue(JSON.stringify({ result: { tabs: [
      { tab_id: "w1:t1", number: 1, label: "shell", focused: true },
      { tab_id: "w1:t2", number: 2, label: "shell", focused: false },
    ] } }));
    const { listTerminalTabs } = await importService();
    await expect(listTerminalTabs("task-1")).resolves.toEqual({ ok: true, tabs: [
      { id: "w1:t1", nativeIndex: 0, name: "shell", isActive: true },
      { id: "w1:t2", nativeIndex: 1, name: "shell", isActive: false },
    ] });
    expect(findExecutedCommand("--session 'proj-main' 'tab' 'list'")).toBeDefined();
  });
});


describe("Herdr 탭 조작", () => {
  const tabs = [
    { tab_id: "w1:t1", label: "same", focused: false },
    { tab_id: "w1:t2", label: "same", focused: false },
    { tab_id: "w2:t1", label: "same", focused: false },
    { tab_id: "w2:t2", label: "same", focused: true },
  ];
  beforeEach(() => {
    stubTask({ sessionType: SessionType.HERDR, sshHost: "remote-herdr" });
    mockExecGit.mockResolvedValue(JSON.stringify({ result: { tabs } }));
  });
  it("새 탭은 활성 워크스페이스에 생성한다", async () => {
    const service = await importService();
    await expect(service.createTerminalTab("task-1")).resolves.toEqual({ ok: true });
    expect(findExecutedCommand("'tab' 'create' '--workspace' 'w2' '--cwd' '/work/tree' '--focus'")).toBeDefined();
  });
  it.each([["w2:t2", 2, 0], ["w2:t1", 3, 2]] as const)("%s 이동은 워크스페이스 내부 삽입 경계 %s로 변환한다", async (id, destination, insertIndex) => {
    const service = await importService();
    await expect(service.moveTerminalTab("task-1", id, destination)).resolves.toEqual({ ok: true });
    expect(mockMoveHerdrTab).toHaveBeenCalledWith("proj-main", id, insertIndex, "remote-herdr");
  });
  it("워크스페이스를 가로지르는 이동은 다른 탭을 훼손하지 않고 거절한다", async () => {
    const service = await importService();
    await expect(service.moveTerminalTab("task-1", "w2:t2", 0)).resolves.toMatchObject({ ok: false });
    expect(mockMoveHerdrTab).not.toHaveBeenCalled();
  });
  it("마지막 탭은 지우지 않고 창을 닫는다", async () => {
    mockExecGit.mockResolvedValue(JSON.stringify({ result: { tabs: [tabs[0]] } }));
    const service = await importService();
    await expect(service.closeTerminalTab("task-1", "w1:t1")).resolves.toEqual({ ok: true, shouldCloseWindow: true });
    expect(findExecutedCommand("'tab' 'close'")).toBeUndefined();
  });
  it("중복 이름이어도 ID로 정확한 탭을 선택·변경·닫는다", async () => {
    const service = await importService();
    await expect(service.selectTerminalTab("task-1", "w2:t2")).resolves.toEqual({ ok: true });
    await expect(service.renameTerminalTab("task-1", "w2:t2", "new name")).resolves.toEqual({ ok: true });
    await expect(service.closeTerminalTab("task-1", "w2:t2")).resolves.toEqual({ ok: true, shouldCloseWindow: false });
    expect(findExecutedCommand("'tab' 'focus' 'w2:t2'")).toBeDefined();
    expect(findExecutedCommand("'tab' 'rename' 'w2:t2' 'new name'")).toBeDefined();
    expect(findExecutedCommand("'tab' 'close' 'w2:t2'")).toBeDefined();
  });
});


describe("탭 닫기 전 확인 정보", () => {
  it("마지막 멀티플렉서 탭은 프로세스를 건드리지 않고 창만 닫도록 안내한다", async () => {
    stubTask({});
    mockExecGit.mockResolvedValue("@1\t0\t1\tzsh");
    const service = await importService();
    expect(await service.getTerminalTabCloseInfo("task-1", "@1")).toEqual({
      ok: true, shouldCloseWindow: true, processNames: null,
    });
    expect(findExecutedCommand("kill-window")).toBeUndefined();
    expect(findExecutedCommand("list-panes")).toBeUndefined();
  });

  it("현재 탭의 모든 분할 화면에서 프로세스 이름을 모으고 다른 탭은 제외한다", async () => {
    stubTask({ sshHost: "dev-host" });
    mockExecGit.mockImplementation(async (command: string) => command.includes("list-windows")
      ? "@1\t0\t1\twork\n@2\t1\t0\tother"
      : "%1\t@1\tclaude\t0\t0\t80\t24\twork\n%2\t@1\tnode\t80\t0\t80\t24\twork\n%3\t@2\tvim\t0\t0\t80\t24\tother");
    const service = await importService();
    expect(await service.getTerminalTabCloseInfo("task-1", "@1")).toEqual({
      ok: true, shouldCloseWindow: false, processNames: ["claude", "node"],
    });
    expect(mockExecGit.mock.calls.every((call) => call[1] === "dev-host")).toBe(true);
    expect(findExecutedCommand("kill-window")).toBeUndefined();
  });

  it("Herdr 탭을 닫기 전에 그 탭의 프로세스 이름만 모은다", async () => {
    stubTask({ sessionType: SessionType.HERDR, sessionName: "herdr-session" });
    const herdrResult = (value: unknown) => JSON.stringify({ id: "test", result: value });
    const scroll = { max_offset_from_bottom: 0, viewport_rows: 38 };
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("'tab' 'list'")) {
        return herdrResult({ tabs: [
          { tab_id: "w1:t1", number: 1, label: "work", focused: true },
          { tab_id: "w1:t2", number: 2, label: "other", focused: false },
        ] });
      }
      if (command.includes("'pane' 'list'")) {
        return herdrResult({ panes: [
          { pane_id: "w1:p1", tab_id: "w1:t1", scroll },
          { pane_id: "w1:p2", tab_id: "w1:t2", scroll },
        ] });
      }
      if (command.includes("'pane' 'layout'")) {
        return herdrResult({ layout: { area: { x: 0, y: 0, width: 80, height: 40 }, focused_pane_id: "w1:p1", zoomed: false, panes: [] } });
      }
      return herdrResult({ process_info: { foreground_processes: [{ name: command.includes("w1:p1") ? "claude" : "vim" }] } });
    });
    const service = await importService();
    expect(await service.getTerminalTabCloseInfo("task-1", "w1:t1")).toEqual({
      ok: true, shouldCloseWindow: false, processNames: ["claude"],
    });
  });

  it("프로세스 조회 실패는 실행 중인 작업이 없다는 뜻으로 바꾸지 않는다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("list-windows")) return "@1\t0\t1\twork\n@2\t1\t0\tother";
      throw new Error("process lookup failed");
    });
    const service = await importService();
    expect(await service.getTerminalTabCloseInfo("task-1", "@1")).toEqual({
      ok: true, shouldCloseWindow: false, processNames: null,
    });
  });

  it("탭 목록 조회 실패는 닫기 허가로 바꾸지 않는다", async () => {
    stubTask({});
    mockExecGit.mockRejectedValue(new Error("offline"));
    const service = await importService();
    expect(await service.getTerminalTabCloseInfo("task-1", "@1")).toEqual({
      ok: false, shouldCloseWindow: false, processNames: null, error: "offline",
    });
  });

  it("프로세스가 없는 조회 결과도 안전하다고 단정하지 않는다", async () => {
    stubTask({});
    mockExecGit.mockImplementation(async (command: string) => command.includes("list-windows")
      ? "@1\t0\t1\twork\n@2\t1\t0\tother" : "");
    const service = await importService();
    expect((await service.getTerminalTabCloseInfo("task-1", "@1")).processNames).toBeNull();
  });

  it("원격 일반 터미널에서 로컬 SSH 프로세스를 원격 작업 이름으로 표시하지 않는다", async () => {
    stubTask({ sessionType: SessionType.TERMINAL, sshHost: "dev-host" });
    mockListLocalTerminalTabs.mockReturnValue([{ id: "tab-1" }]);
    mockGetLocalTerminalTabProcess.mockReturnValue("ssh");
    const service = await importService();
    expect((await service.getTerminalTabCloseInfo("task-1", "tab-1")).processNames).toBeNull();
    expect(mockGetLocalTerminalTabProcess).not.toHaveBeenCalled();
  });

  it("존재하지 않는 탭은 닫기 대상으로 인정하지 않는다", async () => {
    stubTask({});
    mockExecGit.mockResolvedValue("@1\t0\t1\tzsh");
    const service = await importService();
    expect((await service.getTerminalTabCloseInfo("task-1", "@999")).ok).toBe(false);
  });

  it("일반 터미널은 마지막 탭도 삭제되므로 현재 PTY 프로세스를 경고한다", async () => {
    stubTask({ sessionType: SessionType.TERMINAL });
    mockListLocalTerminalTabs.mockReturnValue([{ id: "tab-1" }]);
    mockGetLocalTerminalTabProcess.mockReturnValue("node");
    const service = await importService();
    expect(await service.getTerminalTabCloseInfo("task-1", "tab-1")).toEqual({
      ok: true, shouldCloseWindow: false, processNames: ["node"],
    });
    expect(mockCloseLocalTerminalTab).not.toHaveBeenCalled();
  });
});


describe("공유 Space 탭 서비스", () => {
  beforeEach(() => {
    stubTask({ sessionType: SessionType.HERDR, sessionName: "kanvibe:task-a" });
    mockExecGit.mockImplementation(async (command: string) => {
      if (command.includes("'session' 'list'")) return JSON.stringify({ sessions: [{ name: "kanvibe", running: true, session_dir: "/config/kanvibe", socket_path: "/config/kanvibe/herdr.sock" }] });
      if (command.includes("'tab' 'create'")) return JSON.stringify({ result: { tab: { tab_id: "w1:t3" } } });
      return JSON.stringify({ result: { tabs: [
        { tab_id: "w1:t1", label: "one", focused: true }, { tab_id: "w1:t2", label: "two", focused: false },
      ] } });
    });
  });
  it("활성 탭은 전역 focus 대신 해당 task 클라이언트의 선택을 표시한다", async () => {
    mockGetHerdrTaskActiveTabId.mockReturnValue("w1:t2");
    const service = await importService();
    const result = await service.listTerminalTabs("task-1");
    expect(result.tabs.map((tab) => tab.isActive)).toEqual([false, true]);
    expect(findExecutedCommand("'tab' 'list' '--workspace' 'w1'")).toBeDefined();
  });
  it("탭 생성과 선택은 전역 focus 없이 task 클라이언트로 전달한다", async () => {
    const service = await importService();
    await expect(service.createTerminalTab("task-1")).resolves.toEqual({ ok: true });
    expect(findExecutedCommand("'--no-focus'")).toBeDefined();
    expect(mockFocusHerdrTaskTab).toHaveBeenCalledWith("task-1", "w1:t3");
    await expect(service.selectTerminalTab("task-1", "w1:t2")).resolves.toEqual({ ok: true });
    expect(mockFocusHerdrTaskTab).toHaveBeenCalledWith("task-1", "w1:t2");
    expect(findExecutedCommand("'tab' 'focus'")).toBeUndefined();
  });
  it("다른 Space의 탭 선택은 클라이언트 명령을 보내지 않고 거절한다", async () => {
    const service = await importService();
    await expect(service.selectTerminalTab("task-1", "w2:t1")).resolves.toMatchObject({ ok: false });
    expect(mockFocusHerdrTaskTab).not.toHaveBeenCalled();
  });
});
