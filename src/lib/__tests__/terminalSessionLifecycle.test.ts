/**
 * @vitest-environment node
 */
import { spawnSync } from "child_process";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

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
  /** node-pty가 등록받은 종료 콜백. 셸이 스스로 끝나는 상황을 재현할 때 부른다 */
  triggerExit: () => void;

}

const spawnedPtys: SpawnedPtyStub[] = [];
const spawnCalls: { shell: string; args: string[]; cwd: string }[] = [];

vi.mock("node-pty", () => ({
  spawn: vi.fn((shell: string, args: string[], options: { cwd: string }) => {
    let exitHandler: ((exitStatus: { exitCode: number; signal?: number }) => void) | null = null;
    const ptyStub: SpawnedPtyStub = {
      kill: vi.fn(),
      write: vi.fn(),
      resize: vi.fn(),
      onData: vi.fn(),
      onExit: vi.fn((handler: (exitStatus: { exitCode: number; signal?: number }) => void) => {
        exitHandler = handler;
      }),
      pid: 1000 + spawnedPtys.length,
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
    expect(tabs[0]).toMatchObject({ index: 0, isActive: true });
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
      { id: secondTab.id, index: 0, name: secondTab.name, isActive: true },
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

    expect(listLocalTerminalTabs("task-1").map((tab) => ({ id: tab.id, index: tab.index, name: tab.name }))).toEqual([
      { id: secondTab.id, index: 0, name: secondTab.name },
      { id: firstTab.id, index: 1, name: "빌드" },
    ]);
  });

  it("목록 밖 인덱스로는 순서를 바꾸지 않는다", async () => {
    const { listLocalTerminalTabs, moveLocalTerminalTab } = await importTerminalModule();

    const [firstTab] = listLocalTerminalTabs("task-1");
    moveLocalTerminalTab("task-1", firstTab.id, 5);

    expect(listLocalTerminalTabs("task-1").map((tab) => tab.id)).toEqual([firstTab.id]);
  });
});
