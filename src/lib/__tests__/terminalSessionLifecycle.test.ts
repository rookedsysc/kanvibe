/**
 * @vitest-environment node
 */
import { spawnSync } from "child_process";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const { createHerdrClientProxy, readHerdrClientTabBarPosition, readHerdrMapping } = vi.hoisted(() => ({
  createHerdrClientProxy: vi.fn(),
  readHerdrClientTabBarPosition: vi.fn(async () => "top"),
  readHerdrMapping: vi.fn(async () => JSON.stringify({ "task-a": "w1", "task-b": "w2" })),
}));
vi.mock("@/lib/herdrClientProxy", () => ({ createHerdrClientProxy }));
vi.mock("@/lib/herdr", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/herdr")>(), readHerdrClientTabBarPosition }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/hostFileAccess")>(),
  readTextFile: readHerdrMapping,
  updateTextFile: async (_path: string, update: (content: string) => string | Promise<string>) => {
    await update(JSON.stringify({ "task-a": "w1", "task-b": "w2" }));
  },
}));

// --- Mocks ---

vi.mock("child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("child_process")>();
  return { ...actual, execSync: vi.fn(() => "") };
});

vi.mock("@/lib/gitOperations", () => ({ execGit: vi.fn(async () => "") }));

interface SpawnedPtyStub {
  kill: ReturnType<typeof vi.fn>;
  write: ReturnType<typeof vi.fn>;
  resize: ReturnType<typeof vi.fn>;
  onData: ReturnType<typeof vi.fn>;
  onExit: ReturnType<typeof vi.fn>;
  pid: number;
  process: string;
  cols: number;
  rows: number;
  /** node-pty가 등록받은 종료 콜백. 셸이 스스로 끝나는 상황을 재현할 때 부른다 */
  triggerExit: () => void;

}

const spawnedPtys: SpawnedPtyStub[] = [];
const spawnCalls: { shell: string; args: string[]; cwd: string }[] = [];

vi.mock("node-pty", () => ({
  spawn: vi.fn((shell: string, args: string[], options: { cwd: string; cols: number; rows: number }) => {
    let exitHandler: ((exitStatus: { exitCode: number; signal?: number }) => void) | null = null;
    const ptyStub: SpawnedPtyStub = {
      kill: vi.fn(),
      write: vi.fn(),
      resize: vi.fn((cols: number, rows: number) => {
        ptyStub.cols = cols;
        ptyStub.rows = rows;
      }),
      onData: vi.fn(),
      onExit: vi.fn((handler: (exitStatus: { exitCode: number; signal?: number }) => void) => {
        exitHandler = handler;
      }),
      pid: 1000 + spawnedPtys.length,
      process: "zsh",
      cols: options.cols,
      rows: options.rows,
      triggerExit: () => exitHandler?.({ exitCode: 0, signal: undefined }),
    };

    spawnedPtys.push(ptyStub);
    spawnCalls.push({ shell, args, cwd: options.cwd });
    return ptyStub;
  }),
}));

interface MockWebSocket {
  readyState: number;
  OPEN: number;
  send: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
  on: ReturnType<typeof vi.fn>;
  /** 창이 닫히는 상황을 재현한다 */
  triggerClose: () => void;
}

function createMockWs(): MockWebSocket {
  const handlers = new Map<string, (payload?: unknown) => void>();

  return {
    readyState: 1,
    OPEN: 1,
    send: vi.fn(),
    close: vi.fn(),
    on: vi.fn((event: string, handler: (payload?: unknown) => void) => {
      handlers.set(event, handler);
    }),
    triggerClose: () => handlers.get("close")?.(),
  };
}

/** 원격 명령은 `sh -lc '<payload>'`로 감싸 전달되므로 실제 셸로 풀어 원문을 본다 */
function extractRemoteShellPayload(remoteShellCommand: string): string {
  const result = spawnSync("sh", ["-c", `set -- ${remoteShellCommand}; printf '%s' "$3"`], {
    encoding: "utf-8",
  });

  expect(result.status, result.stderr).toBe(0);
  return result.stdout;
}

async function importTerminalModule() {
  return import("@/lib/terminal");
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.resetModules();
  spawnedPtys.length = 0;
  spawnCalls.length = 0;
  readHerdrClientTabBarPosition.mockResolvedValue("top");
});

describe("Zellij 모바일 viewport 소유권", () => {
  it("모바일 크기를 유지하고 마지막에 최신 PC 크기를 복원한다", async () => {
    const { attachLocalSession, getTaskTerminalSize, resizeTaskTerminalViewport, releaseTaskTerminalViewport } = await importTerminalModule();
    const desktop = createMockWs();
    await attachLocalSession("zellij-task", null, SessionType.ZELLIJ, "proj-main", desktop as never, "/work", 120, 30);
    expect(getTaskTerminalSize("zellij-task")).toEqual({ cols: 120, rows: 30 });

    resizeTaskTerminalViewport("zellij-task", 88, 73);
    const onMessage = desktop.on.mock.calls.find(([event]) => event === "message")?.[1];
    onMessage(Buffer.from(`\x01${JSON.stringify({ type: "resize", cols: 100, rows: 40 })}`));

    expect(getTaskTerminalSize("zellij-task")).toEqual({ cols: 88, rows: 73 });
    expect(spawnedPtys[0].resize.mock.calls).toEqual([[88, 73]]);
    releaseTaskTerminalViewport("zellij-task", 120, 30);
    expect(getTaskTerminalSize("zellij-task")).toEqual({ cols: 100, rows: 40 });

    onMessage(Buffer.from(`\x01${JSON.stringify({ type: "resize", cols: 110, rows: 45 })}`));
    expect(getTaskTerminalSize("zellij-task")).toEqual({ cols: 110, rows: 45 });
  });

  it("접속 중 PC 변경이 없으면 원래 크기를 복원한다", async () => {
    const { attachLocalSession, getTaskTerminalSize, resizeTaskTerminalViewport, releaseTaskTerminalViewport } = await importTerminalModule();
    await attachLocalSession("zellij-task", null, SessionType.ZELLIJ, "proj-main", createMockWs() as never, "/work", 120, 30);

    resizeTaskTerminalViewport("zellij-task", 88, 73);
    releaseTaskTerminalViewport("zellij-task", 120, 30);

    expect(getTaskTerminalSize("zellij-task")).toEqual({ cols: 120, rows: 30 });
  });

  it("다른 세션이나 존재하지 않는 PTY는 변경하지 않는다", async () => {
    const { attachLocalSession, getTaskTerminalSize, resizeTaskTerminalViewport, releaseTaskTerminalViewport } = await importTerminalModule();
    await attachLocalSession("tmux-task", null, SessionType.TMUX, "proj-main", createMockWs() as never, "/work");

    for (const taskId of ["tmux-task", "missing-task"]) {
      expect(getTaskTerminalSize(taskId)).toBeNull();
      expect(() => resizeTaskTerminalViewport(taskId, 88, 73)).toThrow();
      expect(() => releaseTaskTerminalViewport(taskId, 120, 30)).not.toThrow();
    }
    expect(spawnedPtys[0].resize).not.toHaveBeenCalled();
  });

  it("닫힌 Zellij PTY는 viewport 복원으로 다시 만들지 않는다", async () => {
    const { attachLocalSession, getTaskTerminalSize, resizeTaskTerminalViewport, releaseTaskTerminalViewport } = await importTerminalModule();
    const mobile = createMockWs();
    await attachLocalSession("zellij-task", null, SessionType.ZELLIJ, "proj-main", mobile as never, "/work", 120, 30);
    resizeTaskTerminalViewport("zellij-task", 88, 73);
    mobile.triggerClose();

    releaseTaskTerminalViewport("zellij-task", 120, 30);

    expect(getTaskTerminalSize("zellij-task")).toBeNull();
    expect(spawnedPtys).toHaveLength(1);
    expect(spawnedPtys[0].resize.mock.calls).toEqual([[88, 73]]);
  });
});

describe("세션 타입별 수명", () => {
  it("terminal 세션은 마지막 클라이언트가 떠나도 PTY를 살려 둔다", async () => {
    const { attachLocalSession, getActiveTerminalCount } = await importTerminalModule();
    const ws = createMockWs();

    await attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj-main", ws as never, "/work");
    ws.triggerClose();

    expect(spawnedPtys[0].kill).not.toHaveBeenCalled();
    expect(getActiveTerminalCount()).toBe(1);
  });

  it("tmux 세션은 마지막 클라이언트가 떠나면 PTY를 끊는다", async () => {
    const { attachLocalSession, getActiveTerminalCount } = await importTerminalModule();
    const ws = createMockWs();

    await attachLocalSession("task-1", null, SessionType.TMUX, "proj-main", ws as never, "/work");
    ws.triggerClose();

    expect(spawnedPtys[0].kill).toHaveBeenCalled();
    expect(getActiveTerminalCount()).toBe(0);
  });

  it("클라이언트가 남아 있으면 tmux 세션도 끊지 않는다", async () => {
    const { attachLocalSession } = await importTerminalModule();
    const firstWs = createMockWs();
    const secondWs = createMockWs();

    await attachLocalSession("task-1", null, SessionType.TMUX, "proj-main", firstWs as never, "/work");
    await attachLocalSession("task-1", null, SessionType.TMUX, "proj-main", secondWs as never, "/work");
    firstWs.triggerClose();

    expect(spawnedPtys).toHaveLength(1);
    expect(spawnedPtys[0].kill).not.toHaveBeenCalled();
  });

  it("앱 종료 경로는 살아 있는 terminal PTY를 모두 끊는다", async () => {
    const { attachLocalSession, killAllTerminalSessions, getActiveTerminalCount } = await importTerminalModule();

    await attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", createMockWs() as never, "/work");
    await attachLocalSession("task-1", "task-1-2", SessionType.TERMINAL, "proj", createMockWs() as never, "/work");

    killAllTerminalSessions();

    expect(spawnedPtys).toHaveLength(2);
    expect(spawnedPtys.every((pty) => pty.kill.mock.calls.length > 0)).toBe(true);
    expect(getActiveTerminalCount()).toBe(0);
  });

  it("태스크 정리는 그 태스크의 탭 PTY를 전부 끊고 다른 태스크는 건드리지 않는다", async () => {
    const { attachLocalSession, detachSession, getActiveTerminalCount } = await importTerminalModule();

    await attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", createMockWs() as never, "/work");
    await attachLocalSession("task-1", "task-1-2", SessionType.TERMINAL, "proj", createMockWs() as never, "/work");
    await attachLocalSession("task-2", "task-2-1", SessionType.TERMINAL, "proj2", createMockWs() as never, "/work2");

    detachSession("task-1", "cleanup-task-resources");

    expect(spawnedPtys[0].kill).toHaveBeenCalled();
    expect(spawnedPtys[1].kill).toHaveBeenCalled();
    expect(spawnedPtys[2].kill).not.toHaveBeenCalled();
    expect(getActiveTerminalCount()).toBe(1);
  });
});

describe("같은 터미널을 동시에 열 때", () => {
  /** 경쟁이 되살아나 실제 ssh가 뜨더라도 곧바로 거절되도록 닫힌 로컬 포트를 가리킨다 */
  const SSH_CONFIG = { host: "remote-host", hostname: "127.0.0.1", port: 1, username: "dev", privateKeyPath: "/key" };

  /** node-pty가 등록받은 출력 콜백. PTY가 무언가를 내보내는 상황을 재현할 때 부른다 */
  function emitPtyOutput(pty: SpawnedPtyStub, chunk: string): void {
    for (const [handler] of pty.onData.mock.calls) {
      handler(chunk);
    }
  }

  /**
   * 동시에 일어난 두 번째 `import("node-pty")`는 vitest가 mock이 아닌 실제 모듈을 돌려줄 때가 있다.
   * 그래서 spawn 횟수가 아니라, 두 클라이언트가 같은 PTY의 출력을 받는지로 판정한다.
   */
  it("로컬은 PTY를 하나만 띄우고 두 클라이언트가 모두 출력을 받는다", async () => {
    const { attachLocalSession, getActiveTerminalCount } = await importTerminalModule();
    const firstWs = createMockWs();
    const secondWs = createMockWs();

    await Promise.all([
      attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", firstWs as never, "/work"),
      attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", secondWs as never, "/work"),
    ]);
    emitPtyOutput(spawnedPtys[0], "hello");

    expect(getActiveTerminalCount()).toBe(1);
    expect(firstWs.send).toHaveBeenCalledWith("hello");
    expect(secondWs.send).toHaveBeenCalledWith("hello");
  });

  it("원격도 ssh PTY 하나를 두 클라이언트가 함께 쓴다", async () => {
    const { attachRemoteSession, getActiveTerminalCount } = await importTerminalModule();
    const firstWs = createMockWs();
    const secondWs = createMockWs();

    await Promise.all([
      attachRemoteSession("task-1", null, "remote-host", SessionType.TMUX, "proj-main", firstWs as never, SSH_CONFIG),
      attachRemoteSession("task-1", null, "remote-host", SessionType.TMUX, "proj-main", secondWs as never, SSH_CONFIG),
    ]);
    emitPtyOutput(spawnedPtys[0], "hello");

    expect(getActiveTerminalCount()).toBe(1);
    expect(firstWs.send).toHaveBeenCalledWith("hello");
    expect(secondWs.send).toHaveBeenCalledWith("hello");
  });

  it("먼저 온 쪽이 PTY를 띄우지 못하면 기다리던 쪽도 닫고 거절한다", async () => {
    const nodePty = await import("node-pty");
    vi.mocked(nodePty.spawn).mockImplementationOnce(() => {
      throw new Error("spawn failed");
    });
    const { attachLocalSession, getActiveTerminalCount } = await importTerminalModule();
    const firstWs = createMockWs();
    const secondWs = createMockWs();

    const results = await Promise.allSettled([
      attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", firstWs as never, "/work"),
      attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", secondWs as never, "/work"),
    ]);

    expect(results.map((result) => result.status)).toEqual(["rejected", "rejected"]);
    expect(firstWs.close).toHaveBeenCalledWith(1011, "터미널 프로세스 생성 실패");
    expect(secondWs.close).toHaveBeenCalledWith(1011, "터미널 연결 실패");
    expect(getActiveTerminalCount()).toBe(0);
  });

  it("실패한 뒤 다시 열면 새로 띄운다", async () => {
    const nodePty = await import("node-pty");
    vi.mocked(nodePty.spawn).mockImplementationOnce(() => {
      throw new Error("spawn failed");
    });
    const { attachLocalSession } = await importTerminalModule();

    await expect(
      attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", createMockWs() as never, "/work"),
    ).rejects.toThrow();
    await attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", createMockWs() as never, "/work");

    expect(spawnedPtys).toHaveLength(1);
  });
});

describe("terminal 세션 spawn", () => {
  it("로컬은 로그인 셸을 worktree에서 띄운다", async () => {
    process.env.SHELL = "/bin/zsh";
    const { attachLocalSession } = await importTerminalModule();

    await attachLocalSession("task-1", "task-1-1", SessionType.TERMINAL, "proj", createMockWs() as never, "/work/tree");

    expect(spawnCalls[0]).toEqual({ shell: "/bin/zsh", args: ["-l"], cwd: "/work/tree" });
  });

  it("원격은 worktree로 이동한 뒤 로그인 셸을 exec한다", async () => {
    const { attachRemoteSession } = await importTerminalModule();

    await attachRemoteSession(
      "task-1",
      "task-1-1",
      "remote-host",
      SessionType.TERMINAL,
      "proj",
      createMockWs() as never,
      { host: "remote-host", hostname: "10.0.0.1", port: 22, username: "dev", privateKeyPath: "/key" },
      120,
      30,
      "/remote/worktree",
    );

    const remoteShellCommand = extractRemoteShellPayload(spawnCalls[0].args.at(-1) ?? "");

    expect(spawnCalls[0].shell).toBe("ssh");
    expect(remoteShellCommand).toContain("cd '/remote/worktree'");
    expect(remoteShellCommand).toContain('exec "${SHELL:-/bin/sh}" -l');
    expect(remoteShellCommand).not.toContain("tmux");
    expect(remoteShellCommand).not.toContain("zellij");
  });
});

describe("terminal 세션 탭 레지스트리", () => {
  it("처음 조회하면 활성 탭 하나를 만들어 준다", async () => {
    const { listLocalTerminalTabs } = await importTerminalModule();

    const tabs = listLocalTerminalTabs("task-1");

    expect(tabs).toHaveLength(1);
    expect(tabs[0]).toMatchObject({ nativeIndex: 0, isActive: true });
  });

  it("새 탭은 목록 끝에 붙고 활성이 된다", async () => {
    const { createLocalTerminalTab, listLocalTerminalTabs } = await importTerminalModule();

    const createdTab = createLocalTerminalTab("task-1");
    const tabs = listLocalTerminalTabs("task-1");

    expect(tabs).toHaveLength(2);
    expect(tabs[1].id).toBe(createdTab.id);
    expect(tabs.filter((tab) => tab.isActive).map((tab) => tab.id)).toEqual([createdTab.id]);
  });

  it("탭을 닫으면 그 탭의 PTY만 끊고 남은 탭 수를 알려 준다", async () => {
    const {
      attachLocalSession,
      closeLocalTerminalTab,
      createLocalTerminalTab,
      listLocalTerminalTabs,
    } = await importTerminalModule();

    const [firstTab] = listLocalTerminalTabs("task-1");
    const secondTab = createLocalTerminalTab("task-1");
    await attachLocalSession("task-1", firstTab.id, SessionType.TERMINAL, "proj", createMockWs() as never, "/work");
    await attachLocalSession("task-1", secondTab.id, SessionType.TERMINAL, "proj", createMockWs() as never, "/work");

    const remainingCount = closeLocalTerminalTab("task-1", firstTab.id);

    expect(remainingCount).toBe(1);
    expect(spawnedPtys[0].kill).toHaveBeenCalled();
    expect(spawnedPtys[1].kill).not.toHaveBeenCalled();
  });

  it("활성 탭을 닫으면 그 자리를 이어받는 탭이 활성이 된다", async () => {
    const { closeLocalTerminalTab, createLocalTerminalTab, listLocalTerminalTabs, selectLocalTerminalTab } = await importTerminalModule();

    const [firstTab] = listLocalTerminalTabs("task-1");
    const secondTab = createLocalTerminalTab("task-1");
    selectLocalTerminalTab("task-1", firstTab.id);

    closeLocalTerminalTab("task-1", firstTab.id);

    expect(listLocalTerminalTabs("task-1")).toEqual([
      { id: secondTab.id, nativeIndex: 0, name: secondTab.name, isActive: true },
    ]);
  });

  it("셸이 스스로 끝나면 그 탭은 목록에서 사라진다", async () => {
    const { attachLocalSession, createLocalTerminalTab, listLocalTerminalTabs } = await importTerminalModule();

    const [firstTab] = listLocalTerminalTabs("task-1");
    const secondTab = createLocalTerminalTab("task-1");
    await attachLocalSession("task-1", firstTab.id, SessionType.TERMINAL, "proj", createMockWs() as never, "/work");
    await attachLocalSession("task-1", secondTab.id, SessionType.TERMINAL, "proj", createMockWs() as never, "/work");

    spawnedPtys[0].triggerExit();

    expect(listLocalTerminalTabs("task-1").map((tab) => tab.id)).toEqual([secondTab.id]);
  });

  it("이름 변경과 순서 변경이 목록에 반영된다", async () => {
    const {
      createLocalTerminalTab,
      listLocalTerminalTabs,
      moveLocalTerminalTab,
      renameLocalTerminalTab,
    } = await importTerminalModule();

    const [firstTab] = listLocalTerminalTabs("task-1");
    const secondTab = createLocalTerminalTab("task-1");
    renameLocalTerminalTab("task-1", firstTab.id, "빌드");
    moveLocalTerminalTab("task-1", firstTab.id, 1);

    expect(listLocalTerminalTabs("task-1").map((tab) => ({ id: tab.id, nativeIndex: tab.nativeIndex, name: tab.name }))).toEqual([
      { id: secondTab.id, nativeIndex: 0, name: secondTab.name },
      { id: firstTab.id, nativeIndex: 1, name: "빌드" },
    ]);
  });

  it("목록 밖 인덱스로는 순서를 바꾸지 않는다", async () => {
    const { listLocalTerminalTabs, moveLocalTerminalTab } = await importTerminalModule();

    const [firstTab] = listLocalTerminalTabs("task-1");
    moveLocalTerminalTab("task-1", firstTab.id, 5);

    expect(listLocalTerminalTabs("task-1").map((tab) => tab.id)).toEqual([firstTab.id]);
  });
});

describe("Herdr 원격 machine 연결", () => {
  const config = { host: "host", hostname: "host", port: 22, username: "user", privateKeyPath: "/key" };

  it("원격 서버 준비 후 로컬 machine 등록을 시작하면서 SSH PTY를 연결한다", async () => {
    const git = await import("@/lib/gitOperations");
    let finishRegistration!: (output: string) => void;
    vi.mocked(git.execGit).mockImplementation(async (command, sshHost) => {
      if (command.includes("'machine' 'list'")) return "[]";
      if (command.includes("'machine' 'add'")) return new Promise<string>((resolve) => { finishRegistration = resolve; });
      expect(sshHost).toBe("host");
      return JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } });
    });
    const terminal = await importTerminalModule();

    await terminal.attachRemoteSession("machine-task", null, "host", SessionType.HERDR, "project-task", createMockWs() as never, config, 120, 30, "/work");
    await vi.waitFor(() => expect(finishRegistration).toBeTypeOf("function"));
    try {
      expect(spawnCalls).toHaveLength(1);
      expect(spawnCalls[0].shell).toBe("ssh");
      expect(spawnCalls[0].args.at(-1)).toContain("--session");
      const machineCalls = vi.mocked(git.execGit).mock.calls.filter(([command]) => command.includes("'machine'"));
      expect(machineCalls).toHaveLength(2);
      expect(machineCalls.every(([, sshHost]) => sshHost === null)).toBe(true);
      expect(machineCalls[1][0]).toContain("'add' 'host' '--remote-session' 'project-task'");
      expect(vi.mocked(git.execGit).mock.calls[0][0]).toContain("'workspace' 'list'");
    } finally {
      finishRegistration("");
    }
  });

  it.each(["list", "add"])("machine %s 실패에도 원격 PTY를 유지하고 경고한다", async (failedCommand) => {
    const git = await import("@/lib/gitOperations");
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.mocked(git.execGit).mockImplementation(async (command) => {
      if (command.includes(`'machine' '${failedCommand}'`)) throw new Error("machine unavailable");
      if (command.includes("'machine' 'list'")) return "[]";
      return JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } });
    });
    const terminal = await importTerminalModule();
    const ws = createMockWs();
    try {
      await terminal.attachRemoteSession("machine-task", null, "host", SessionType.HERDR, "project-task", ws as never, config, 120, 30, "/work");

      await vi.waitFor(() => expect(warning).toHaveBeenCalledWith(expect.stringContaining("host"), expect.objectContaining({ message: "machine unavailable" })));
      expect(spawnCalls[0].shell).toBe("ssh");
      expect(terminal.getActiveTerminalCount()).toBe(1);
      expect(ws.close).not.toHaveBeenCalled();
    } finally {
      warning.mockRestore();
    }
  });

  it("로컬 Herdr 접속에서는 원격 machine을 조회하지 않는다", async () => {
    const git = await import("@/lib/gitOperations");
    vi.mocked(git.execGit).mockResolvedValue(JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } }));
    const terminal = await importTerminalModule();

    await terminal.attachLocalSession("local-machine-task", null, SessionType.HERDR, "project-task", createMockWs() as never, "/work");

    expect(spawnCalls).toHaveLength(1);
    expect(vi.mocked(git.execGit).mock.calls.some(([command]) => command.includes("'machine'"))).toBe(false);
  });
});

describe("Herdr PTY 수명", () => {
  it.each([false, true])("준비 중 닫힌 %s 원격 클라이언트에는 PTY를 만들지 않는다", async (remote) => {
    const git = await import("@/lib/gitOperations");
    let finishBootstrap!: (output: string) => void;
    vi.mocked(git.execGit).mockResolvedValue(JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } })).mockImplementationOnce(() => new Promise<string>((resolve) => {
      finishBootstrap = resolve;
    }));
    const terminal = await importTerminalModule();
    const ws = createMockWs();
    const attaching = remote
      ? terminal.attachRemoteSession("herdr-task", null, "host", SessionType.HERDR, "project-task", ws as never, { host: "host", hostname: "host", port: 22, username: "user", privateKeyPath: "/key" }, 120, 30, "/work")
      : terminal.attachLocalSession("herdr-task", null, SessionType.HERDR, "project-task", ws as never, "/work");

    ws.readyState = 3;
    ws.triggerClose();
    finishBootstrap(JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } }));
    await attaching;

    expect(spawnedPtys).toHaveLength(0);
    expect(terminal.getActiveTerminalCount()).toBe(0);
    expect(ws.on).not.toHaveBeenCalled();
    expect(vi.mocked(git.execGit).mock.calls.some(([command]) => command.includes("'session' 'stop'"))).toBe(false);
  });

  it.each([
    [false, false], [false, true], [true, false], [true, true],
  ])("동시 연결 준비 중 취소된 클라이언트는 남기지 않는다 (remote=%s, cancelledFirst=%s)", async (remote, cancelledFirst) => {
    const git = await import("@/lib/gitOperations");
    let finishBootstrap!: (output: string) => void;
    vi.mocked(git.execGit).mockResolvedValue(JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } })).mockImplementationOnce(() => new Promise<string>((resolve) => {
      finishBootstrap = resolve;
    }));
    const terminal = await importTerminalModule();
    const cancelled = createMockWs();
    const live = createMockWs();
    const attach = (ws: MockWebSocket) => remote
      ? terminal.attachRemoteSession("herdr-task", null, "host", SessionType.HERDR, "project-task", ws as never, { host: "host", hostname: "host", port: 22, username: "user", privateKeyPath: "/key" }, 120, 30, "/work")
      : terminal.attachLocalSession("herdr-task", null, SessionType.HERDR, "project-task", ws as never, "/work");
    const clients = cancelledFirst ? [cancelled, live] : [live, cancelled];
    const attaching = clients.map(attach);
    cancelled.readyState = 3;
    cancelled.triggerClose();
    finishBootstrap(JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } }));
    await Promise.all(attaching);

    expect(spawnedPtys).toHaveLength(1);
    expect(terminal.getActiveTerminalCount()).toBe(1);
    expect(cancelled.on).not.toHaveBeenCalled();
    expect(spawnedPtys[0].kill).not.toHaveBeenCalled();
    live.triggerClose();
    expect(spawnedPtys[0].kill).toHaveBeenCalledOnce();
    expect(terminal.getActiveTerminalCount()).toBe(0);
    expect(vi.mocked(git.execGit).mock.calls.some(([command]) => command.includes("'session' 'stop'"))).toBe(false);
  });

  it.each([false, true])("동시 %s 원격 연결에서도 PTY 하나를 공유하고 마지막 창만 분리한다", async (remote) => {
    const git = await import("@/lib/gitOperations");
    vi.mocked(git.execGit).mockResolvedValue(JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }] } }));
    const terminal = await importTerminalModule();
    const first = createMockWs();
    const second = createMockWs();
    const attach = (ws: MockWebSocket) => remote
      ? terminal.attachRemoteSession("herdr-task", null, "host", SessionType.HERDR, "project-task", ws as never, { host: "host", hostname: "host", port: 22, username: "user", privateKeyPath: "/key" }, 120, 30, "/work")
      : terminal.attachLocalSession("herdr-task", null, SessionType.HERDR, "project-task", ws as never, "/work");
    await Promise.all([attach(first), attach(second)]);
    expect(spawnedPtys).toHaveLength(1);
    const launchedCommand = spawnCalls[0].args.join(" ");
    expect(launchedCommand).toContain("--session");
    expect(launchedCommand).not.toContain("'session' 'attach'");
    expect(terminal.getActiveTerminalCount()).toBe(1);
    first.triggerClose();
    expect(spawnedPtys[0].kill).not.toHaveBeenCalled();
    second.triggerClose();
    expect(spawnedPtys[0].kill).toHaveBeenCalledOnce();
    expect(terminal.getActiveTerminalCount()).toBe(0);
    expect(vi.mocked(git.execGit).mock.calls.some(([command]) => command.includes("'session' 'stop'"))).toBe(false);
  });
});

describe("닫기 경고용 터미널 프로세스 조회", () => {
  it("해당 탭 PTY의 현재 프로세스를 읽고 사라진 탭은 unknown으로 반환한다", async () => {
    const terminal = await importTerminalModule();
    const [first] = terminal.listLocalTerminalTabs("task-1");
    const second = terminal.createLocalTerminalTab("task-1");
    await terminal.attachLocalSession("task-1", first.id, SessionType.TERMINAL, "proj", createMockWs() as never, "/work");
    await terminal.attachLocalSession("task-1", second.id, SessionType.TERMINAL, "proj", createMockWs() as never, "/work");
    spawnedPtys[0].process = "claude";
    spawnedPtys[1].process = "node";
    expect(terminal.getLocalTerminalTabProcess("task-1", first.id)).toBe("claude");
    expect(terminal.getLocalTerminalTabProcess("task-1", second.id)).toBe("node");
    terminal.closeLocalTerminalTab("task-1", first.id);
    expect(terminal.getLocalTerminalTabProcess("task-1", first.id)).toBeNull();
  });
});

describe("attach ownership races", () => {
  it.each([false, true])("shares one PTY between simultaneous attaches (remote=%s)", async (remote) => {
    const terminal = await importTerminalModule();
    const sockets = [createMockWs(), createMockWs()];
    await Promise.all(sockets.map((ws) => remote
      ? terminal.attachRemoteSession("race", null, "host", SessionType.TMUX, "session", ws as never,
        { host: "host", hostname: "host", port: 22, username: "user", privateKeyPath: "" })
      : terminal.attachLocalSession("race", null, SessionType.TMUX, "session", ws as never)));
    expect(spawnedPtys).toHaveLength(1);
    terminal.killAllTerminalSessions();
    expect(spawnedPtys[0].kill).toHaveBeenCalledTimes(1);
  });

  it("does not spawn for a client closed during initialization", async () => {
    const terminal = await importTerminalModule();
    const ws = createMockWs();
    const pending = terminal.attachLocalSession("cancel", null, SessionType.TMUX, "session", ws as never);
    ws.readyState = 3;
    await pending;
    expect(spawnedPtys).toHaveLength(0);
  });

  it("ignores old PTY exit and client close after replacement", async () => {
    const terminal = await importTerminalModule();
    const oldWs = createMockWs();
    await terminal.attachLocalSession("replace", null, SessionType.TMUX, "session", oldWs as never);
    oldWs.triggerClose();
    const newWs = createMockWs();
    await terminal.attachLocalSession("replace", null, SessionType.TMUX, "session", newWs as never);
    spawnedPtys[0].triggerExit();
    oldWs.triggerClose();
    expect(spawnedPtys[1].kill).not.toHaveBeenCalled();
    expect(terminal.getActiveTerminalCount()).toBe(1);
    terminal.killAllTerminalSessions();
  });
});


describe("공유 Herdr Space 클라이언트", () => {
  const config = { host: "host", hostname: "host", port: 22, username: "user", privateKeyPath: "/key" };
  beforeEach(async () => {
    const git = await import("@/lib/gitOperations");
    vi.mocked(git.execGit).mockImplementation(async (command) => command.includes("'session' 'list'")
      ? JSON.stringify({ sessions: [{ name: "kanvibe", running: true, session_dir: "/config/herdr/sessions/kanvibe", socket_path: "/config/herdr/sessions/kanvibe/herdr.sock" }] })
      : JSON.stringify({ result: { workspaces: [{ workspace_id: "w1" }, { workspace_id: "w2" }] } }));
  });
  it.each([false, true])("공유 Space는 클라이언트 중계를 통해 연결하고 PTY 종료 때 중계도 닫는다 (remote=%s)", async (remote) => {
    const proxy = { socketPath: "/tmp/proxy/client", request: vi.fn(), getActiveTabId: () => "w1:t1", close: vi.fn(async () => {}) };
    createHerdrClientProxy.mockResolvedValueOnce(proxy);
    const terminal = await importTerminalModule();
    const ws = createMockWs();
    await (remote
      ? terminal.attachRemoteSession("task-a", null, "host", SessionType.HERDR, "kanvibe:task-a", ws as never, config, 120, 30, "/work")
      : terminal.attachLocalSession("task-a", null, SessionType.HERDR, "kanvibe:task-a", ws as never, "/work"));
    expect(createHerdrClientProxy).toHaveBeenCalledWith(expect.any(String), "w1");
    expect(readHerdrMapping).toHaveBeenCalledOnce();
    expect(spawnCalls[0].args.join(" ")).toContain("HERDR_CLIENT_SOCKET_PATH=");
    expect(spawnCalls[0].args.join(" ")).not.toContain("--session 'kanvibe'");
    if (remote) {
      expect(spawnCalls[0].args).toContain("-L");
      expect(spawnCalls[0].args).toContain("-R");
      expect(spawnCalls[0].args).toContain("ExitOnForwardFailure=yes");
    }
    terminal.killAllTerminalSessions();
    expect(proxy.close).toHaveBeenCalledOnce();
  });
  it("중계 준비 중 창이 닫히면 소켓을 정리하고 PTY를 만들지 않는다", async () => {
    const proxy = { socketPath: "/tmp/proxy/client", request: vi.fn(), getActiveTabId: () => null, close: vi.fn(async () => {}) };
    let ready!: (value: typeof proxy) => void;
    const preparing = new Promise<void>((resolve) => {
      createHerdrClientProxy.mockImplementationOnce(() => new Promise((finish) => { ready = finish; resolve(); }));
    });
    const terminal = await importTerminalModule();
    const ws = createMockWs();
    const attaching = terminal.attachLocalSession("task-a", null, SessionType.HERDR, "kanvibe:task-a", ws as never, "/work");
    await preparing;
    ws.readyState = 3;
    ready(proxy);
    await attaching;
    expect(spawnedPtys).toHaveLength(0);
    expect(proxy.close).toHaveBeenCalledOnce();
  });
  it("task A 탭 선택은 A 클라이언트에만 전송하고 B는 그대로 둔다", async () => {
    const proxyA = { socketPath: "/tmp/a/client", request: vi.fn(async () => {}), getActiveTabId: () => "w1:t1", close: vi.fn(async () => {}) };
    const proxyB = { ...proxyA, socketPath: "/tmp/b/client", request: vi.fn(async () => {}) };
    createHerdrClientProxy.mockResolvedValueOnce(proxyA).mockResolvedValueOnce(proxyB);
    const terminal = await importTerminalModule();
    await terminal.attachLocalSession("task-a", null, SessionType.HERDR, "kanvibe:task-a", createMockWs() as never, "/work/a");
    await terminal.attachLocalSession("task-b", null, SessionType.HERDR, "kanvibe:task-b", createMockWs() as never, "/work/b");
    await terminal.focusHerdrTaskTab("task-a", "w1:t2");
    expect(proxyA.request).toHaveBeenCalledWith("tab.focus", { tab_id: "w1:t2" });
    expect(proxyB.request).not.toHaveBeenCalled();
    await expect(terminal.focusHerdrTaskTab("task-a", "w2:t1")).rejects.toThrow("다른 Space");
    terminal.killAllTerminalSessions();
  });
  it("채팅에서 돌아갈 pane은 task 소유 Herdr 클라이언트만 선택하고 다른 Space와 끊긴 연결은 거부한다", async () => {
    const proxyA = { socketPath: "/tmp/a/client", request: vi.fn(async () => {}), getActiveTabId: () => "w1:t1", close: vi.fn(async () => {}) };
    const proxyB = { ...proxyA, socketPath: "/tmp/b/client", request: vi.fn(async () => {}) };
    createHerdrClientProxy.mockResolvedValueOnce(proxyA).mockResolvedValueOnce(proxyB);
    const terminal = await importTerminalModule();
    await expect(terminal.focusHerdrTaskPane("task-a", "w1:p2")).rejects.toThrow("연결");
    await terminal.attachLocalSession("task-a", null, SessionType.HERDR, "kanvibe:task-a", createMockWs() as never, "/work/a");
    await terminal.attachLocalSession("task-b", null, SessionType.HERDR, "kanvibe:task-b", createMockWs() as never, "/work/b");
    await terminal.focusHerdrTaskPane("task-a", "w1:p2");
    expect(proxyA.request.mock.calls).toEqual([["pane.focus", { pane_id: "w1:p2" }]]);
    expect(proxyB.request).not.toHaveBeenCalled();
    await expect(terminal.focusHerdrTaskPane("task-a", "w2:p1")).rejects.toThrow("다른 Space");
    expect(proxyA.request).toHaveBeenCalledTimes(1);
    terminal.killAllTerminalSessions();
  });
  it.each(["top", "bottom"] as const)("Herdr pane은 클라이언트 surface 크기와 %s 탭 위치로 실제 화면에 투영한다", async (position) => {
    readHerdrClientTabBarPosition.mockResolvedValue(position);
    let surfaceCols = 134;
    const proxy = { socketPath: "/tmp/a/client", request: vi.fn(async () => {}), getActiveTabId: () => "w1:t1", getSurfaceSize: () => ({ cols: surfaceCols, rows: 47 }), close: vi.fn(async () => {}) };
    createHerdrClientProxy.mockResolvedValueOnce(proxy);
    const terminal = await importTerminalModule();
    await terminal.attachLocalSession("task-a", null, SessionType.HERDR, "kanvibe:task-a", createMockWs() as never, "/work/a", 160, 48);
    expect(await terminal.getHerdrTaskSurfaceBounds("task-a")).toEqual({ left: 26, top: position === "top" ? 1 : 0, width: 134, height: 47 });
    surfaceCols = 156;
    expect(await terminal.getHerdrTaskSurfaceBounds("task-a")).toEqual({ left: 4, top: position === "top" ? 1 : 0, width: 156, height: 47 });
    readHerdrClientTabBarPosition.mockResolvedValue(position === "top" ? "bottom" : "top");
    expect(await terminal.getHerdrTaskSurfaceBounds("task-a")).toBeNull();
    terminal.killAllTerminalSessions();
  });
});
