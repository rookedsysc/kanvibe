/** @vitest-environment node */
import { createServer } from "net";
import { mkdtemp, rm, stat } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { once, EventEmitter } from "events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PaneLayoutType } from "@/entities/PaneLayoutConfig";
import { buildHerdrCommand, buildHerdrSessionCleanupCommand, sanitizeHerdrSessionName } from "@/lib/worktree";
import { ensureHerdrRemoteMachine, ensureHerdrSession, listHerdrMirrorPanes, listHerdrRunningAgentPanes, moveHerdrTab, readHerdrTabs, readHerdrClientTabBarPosition } from "@/lib/herdr";

const { execGit, spawn, readTextFile, updateTextFile } = vi.hoisted(() => ({ execGit: vi.fn(), spawn: vi.fn(), readTextFile: vi.fn(), updateTextFile: vi.fn() }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/hostFileAccess")>(),
  readTextFile,
  updateTextFile,
}));
vi.mock("child_process", async (importOriginal) => ({ ...await importOriginal<typeof import("child_process")>(), spawn }));
vi.mock("@/lib/sshConfig", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/sshConfig")>(),
  parseSSHConfig: async () => [{ host: "remote", hostname: "remote.test", port: 2202, username: "user", privateKeyPath: "/key" }],
}));
vi.mock("@/lib/gitOperations", () => ({ execGit, isSSHTransportError: (error: Error) => error.message.includes("SSH transport") }));

const result = (value: unknown) => JSON.stringify({ id: "test", result: value });
const commandFor = (args: string[]) => buildHerdrCommand("feature", args);
let directory: string | undefined;
let server: ReturnType<typeof createServer> | undefined;

beforeEach(() => { vi.clearAllMocks(); });
afterEach(async () => {
  vi.unstubAllEnvs();
  if (server) { await new Promise<void>((resolve) => server!.close(() => resolve())); server = undefined; }
  if (directory) { await rm(directory, { recursive: true, force: true }); directory = undefined; }
});

describe("Herdr 세션 이름", () => {
  it.each(["normal-branch", "a".repeat(40)])("유효한 이름은 유지한다: %s", (name) => {
    expect(sanitizeHerdrSessionName(name)).toBe(name);
  });
  it.each(["", ".", "..", "한글/브랜치", "a".repeat(65)])("제한 밖 이름은 결정적 ASCII 이름으로 바꾼다: %s", (name) => {
    const sanitized = sanitizeHerdrSessionName(name);
    expect(sanitized).toMatch(/^[a-zA-Z0-9._-]{1,40}$/);
    expect(sanitizeHerdrSessionName(name)).toBe(sanitized);
  });
  it("동일한 절단 접두사를 가진 이름도 구분한다", () => {
    expect(sanitizeHerdrSessionName("a".repeat(80) + "x")).not.toBe(sanitizeHerdrSessionName("a".repeat(80) + "y"));
  });
});

describe("Herdr 원격 machine 등록", () => {
  beforeEach(() => { execGit.mockReset(); });

  it.each(["kanvibe:task-a", "feature"])("%s의 실제 서버 세션을 로컬 machine에 등록한다", async (sessionName) => {
    execGit.mockResolvedValueOnce("[]").mockResolvedValueOnce("Saved SSH machine profile. Remote server is ready.");

    await ensureHerdrRemoteMachine(sessionName, "remote");

    const serverName = sessionName === "feature" ? "feature" : "kanvibe";
    expect(execGit.mock.calls).toEqual([
      [buildHerdrCommand(serverName, ["machine", "list", "--json"]), null, { timeoutMs: 5_000 }],
      [`${buildHerdrCommand(serverName, ["machine", "add", "remote", "--remote-session", serverName])} </dev/null`, null, { timeoutMs: 30_000 }],
    ]);
  });

  it.each([true, false])("기존 프로필의 이름과 활성화 상태(%s)를 보존한다", async (enabled) => {
    execGit.mockResolvedValueOnce(JSON.stringify([{ id: "opaque-id", label: "사용자 이름", target: "remote", session: "kanvibe", enabled }]));

    await ensureHerdrRemoteMachine("kanvibe:task-a", "remote");

    expect(execGit).toHaveBeenCalledOnce();
  });

  it.each([
    { target: "remote", session: "default" },
    { target: "other-remote", session: "kanvibe" },
  ])("다른 호스트 또는 세션의 프로필과 구분한다: %j", async (existing) => {
    execGit.mockResolvedValueOnce(JSON.stringify([existing])).mockResolvedValueOnce("");

    await ensureHerdrRemoteMachine("kanvibe:task-a", "remote");

    expect(execGit.mock.calls[1][0]).toContain("'machine' 'add' 'remote' '--remote-session' 'kanvibe'");
  });

  it("동일 호스트의 동시 접속과 재접속은 중복 등록하지 않는다", async () => {
    const machines: { target: string; session: string }[] = [];
    execGit.mockImplementation(async (command: string) => {
      if (command.includes("'machine' 'list'")) return JSON.stringify(machines);
      machines.push({ target: "remote", session: "kanvibe" });
      return "";
    });

    await Promise.all([
      ensureHerdrRemoteMachine("kanvibe:task-a", "remote"),
      ensureHerdrRemoteMachine("kanvibe:task-b", "remote"),
    ]);
    await ensureHerdrRemoteMachine("kanvibe:task-a", "remote");

    expect(machines).toEqual([{ target: "remote", session: "kanvibe" }]);
    expect(execGit.mock.calls.filter(([command]) => command.includes("'machine' 'add'"))).toHaveLength(1);
  });

  it("다른 호스트의 catalog 저장도 직렬화한다", async () => {
    let releaseList!: (value: string) => void;
    execGit.mockImplementationOnce(() => new Promise<string>((resolve) => { releaseList = resolve; }))
      .mockResolvedValueOnce("")
      .mockResolvedValueOnce(JSON.stringify([{ target: "remote-a", session: "kanvibe" }]))
      .mockResolvedValueOnce("");

    const first = ensureHerdrRemoteMachine("kanvibe:task-a", "remote-a");
    const second = ensureHerdrRemoteMachine("kanvibe:task-b", "remote-b");
    await vi.waitFor(() => expect(execGit).toHaveBeenCalledOnce());
    releaseList("[]");
    await Promise.all([first, second]);

    expect(execGit.mock.calls.map(([command]) => command)).toEqual([
      buildHerdrCommand("kanvibe", ["machine", "list", "--json"]),
      `${buildHerdrCommand("kanvibe", ["machine", "add", "remote-a", "--remote-session", "kanvibe"])} </dev/null`,
      buildHerdrCommand("kanvibe", ["machine", "list", "--json"]),
      `${buildHerdrCommand("kanvibe", ["machine", "add", "remote-b", "--remote-session", "kanvibe"])} </dev/null`,
    ]);
  });

  it.each(["lookup", "add"])("%s 실패 뒤 다음 요청은 재시도한다", async (failure) => {
    if (failure === "add") execGit.mockResolvedValueOnce("[]");
    execGit.mockRejectedValueOnce(new Error("machine failed"));
    await expect(ensureHerdrRemoteMachine("kanvibe:task-a", "remote")).rejects.toThrow("machine failed");
    execGit.mockResolvedValueOnce("[]").mockResolvedValueOnce("");

    await expect(ensureHerdrRemoteMachine("kanvibe:task-a", "remote")).resolves.toBeUndefined();
    expect(execGit.mock.calls.at(-1)?.[0]).toContain("'machine' 'add' 'remote'");
  });

  it.each(["invalid json", "{}"])("잘못된 목록 응답(%s)에는 프로필을 쓰지 않는다", async (output) => {
    execGit.mockResolvedValueOnce(output);

    await expect(ensureHerdrRemoteMachine("kanvibe:task-a", "remote")).rejects.toThrow();

    expect(execGit).toHaveBeenCalledOnce();
  });

  it("사용자가 삭제한 프로필은 다음 접속에서 다시 등록한다", async () => {
    execGit.mockResolvedValueOnce(JSON.stringify([{ target: "remote", session: "kanvibe" }]));
    await ensureHerdrRemoteMachine("kanvibe:task-a", "remote");
    execGit.mockResolvedValueOnce("[]").mockResolvedValueOnce("");

    await ensureHerdrRemoteMachine("kanvibe:task-a", "remote");

    expect(execGit.mock.calls.at(-1)?.[0]).toContain("'machine' 'add' 'remote'");
  });
});

describe("Herdr 부트스트랩", () => {
  it("로컬 서버를 앱의 프로세스 그룹과 분리하고 셸 환경을 정리한다", async () => {
    vi.stubEnv("PORT", "4884");
    vi.stubEnv("NODE_ENV", "production");
    vi.stubEnv("KANVIBE_HOOK_PORT", "4885");
    const server = Object.assign(new EventEmitter(), { unref: vi.fn() });
    spawn.mockImplementationOnce(() => {
      queueMicrotask(() => server.emit("spawn"));
      return server;
    });
    execGit.mockRejectedValueOnce(new Error("server not running"))
      .mockResolvedValue(result({ workspaces: [{ workspace_id: "w1" }] }));

    await ensureHerdrSession("feature", "/work");

    expect(spawn).toHaveBeenCalledWith("/bin/sh", ["-c", `exec ${commandFor(["server"])}`], {
      detached: true, stdio: "ignore", env: expect.any(Object),
    });
    const environment = spawn.mock.calls[0][2].env;
    expect(environment).not.toHaveProperty("PORT");
    expect(environment).not.toHaveProperty("NODE_ENV");
    expect(environment).not.toHaveProperty("KANVIBE_HOOK_PORT");
    expect(server.unref).toHaveBeenCalledOnce();
    expect(execGit.mock.calls.map(([command]) => command)).toEqual([
      commandFor(["workspace", "list"]), commandFor(["workspace", "list"]),
    ]);
  });
  it("로컬 서버 시작 실패를 연결 요청에 전달한다", async () => {
    const server = new EventEmitter();
    spawn.mockImplementationOnce(() => {
      queueMicrotask(() => server.emit("error", new Error("spawn failed")));
      return server;
    });
    execGit.mockRejectedValueOnce(new Error("server not running"));
    await expect(ensureHerdrSession("feature", "/work")).rejects.toThrow("spawn failed");
    expect(execGit).toHaveBeenCalledOnce();
  });
  it("SSH 서버는 원격 호스트에서 시작하고 준비를 확인한다", async () => {
    execGit.mockRejectedValueOnce(new Error("server not running"))
      .mockResolvedValueOnce("")
      .mockResolvedValueOnce(result({ workspaces: [{ workspace_id: "w1" }] }));
    await ensureHerdrSession("feature", "/work", null, "remote");
    expect(spawn).not.toHaveBeenCalled();
    expect(execGit).toHaveBeenCalledWith(`nohup ${commandFor(["server"])} </dev/null >/dev/null 2>&1 &`, "remote");
  });
  it("기존 세션에 다시 연결하면 레이아웃과 명령을 재실행하지 않는다", async () => {
    execGit.mockResolvedValue(result({ workspaces: [{ workspace_id: "w1" }] }));
    await ensureHerdrSession("feature", "/work", { layoutType: PaneLayoutType.QUAD, panes: [{ position: 0, command: "do-not-repeat" }] });
    expect(execGit.mock.calls.map(([command]) => command)).toEqual([commandFor(["workspace", "list"])]);
  });
  it.each([
    [PaneLayoutType.SINGLE, [], ["w1:p1"]],
    [PaneLayoutType.HORIZONTAL_2, [["w1:p1", "down"]], ["w1:p1", "w1:p2"]],
    [PaneLayoutType.VERTICAL_2, [["w1:p1", "right"]], ["w1:p1", "w1:p2"]],
    [PaneLayoutType.LEFT_RIGHT_TB, [["w1:p1", "right"], ["w1:p2", "down"]], ["w1:p1", "w1:p2", "w1:p3"]],
    [PaneLayoutType.LEFT_TB_RIGHT, [["w1:p1", "right"], ["w1:p1", "down"]], ["w1:p1", "w1:p3", "w1:p2"]],
    [PaneLayoutType.QUAD, [["w1:p1", "right"], ["w1:p1", "down"], ["w1:p2", "down"]], ["w1:p1", "w1:p2", "w1:p3", "w1:p4"]],
  ] as const)("%s 레이아웃의 위치별 명령을 올바른 페인에 보낸다", async (layoutType, splits, ids) => {
    let nextPane = 2;
    execGit.mockImplementation(async (command: string) => {
      if (command.includes("'workspace' 'list'")) return result({ workspaces: [] });
      if (command.includes("'workspace' 'create'")) return result({ root_pane: { pane_id: "w1:p1" } });
      if (command.includes("'pane' 'split'")) return result({ pane: { pane_id: `w1:p${nextPane++}` } });
      return "";
    });
    await ensureHerdrSession("feature", "/work tree", { layoutType, panes: ids.map((_id, position) => ({ position, command: `echo pane-${position}` })) }, "remote");
    const commands = execGit.mock.calls.map(([command]) => command);
    expect(commands).toContain(commandFor(["workspace", "create", "--cwd", "/work tree", "--focus"]));
    expect(commands.filter((command) => command.includes("'pane' 'split'"))).toEqual(splits.map(([id, direction]) => commandFor(["pane", "split", id, "--direction", direction, "--cwd", "/work tree"])));
    expect(commands.filter((command) => command.includes("'pane' 'send-text'"))).toEqual(ids.map((id, position) => commandFor(["pane", "send-text", id, `echo pane-${position}\r`])));
    expect(execGit.mock.calls.every((call) => call[1] === "remote")).toBe(true);
  });
  it("동시 연결 요청이 레이아웃을 중복 생성하지 않는다", async () => {
    execGit.mockResolvedValue(result({ workspaces: [{ workspace_id: "w1" }] }));
    await Promise.all([ensureHerdrSession("feature", "/work"), ensureHerdrSession("feature", "/work")]);
    expect(execGit).toHaveBeenCalledTimes(1);
  });
  it("SSH 전송 실패 시 원격 서버를 새로 시작하지 않는다", async () => {
    execGit.mockRejectedValue(new Error("SSH transport"));
    await expect(ensureHerdrSession("feature", "/work", null, "remote")).rejects.toThrow("SSH transport");
    expect(execGit).toHaveBeenCalledTimes(1);
  });
  it("초기 레이아웃 실패 시 해당 신규 세션을 정리한다", async () => {
    execGit.mockResolvedValueOnce(result({ workspaces: [] })).mockResolvedValueOnce(result({ root_pane: { pane_id: "w1:p1" } }))
      .mockRejectedValueOnce(new Error("split failed")).mockResolvedValueOnce("");
    await expect(ensureHerdrSession("feature", "/work", { layoutType: PaneLayoutType.QUAD, panes: [] })).rejects.toThrow("split failed");
    expect(execGit.mock.calls.at(-1)?.[0]).toBe(buildHerdrSessionCleanupCommand("feature"));
  });
});

describe("Herdr API", () => {
  it("CLI의 API 오류를 비어 있는 성공 목록으로 오인하지 않는다", async () => {
    execGit.mockResolvedValue(JSON.stringify({ error: { code: "not_found", message: "missing session" } }));
    await expect(readHerdrTabs("feature")).rejects.toThrow("missing session");
  });
  it("소켓 응답이 여러 조각으로 와도 정확한 tab.move 요청을 한 번 전송한다", async () => {
    directory = await mkdtemp(path.join(tmpdir(), "kvht-"));
    const socketPath = path.join(directory, "api");
    let request: unknown;
    server = createServer((socket) => socket.once("data", (data) => {
      request = JSON.parse(data.toString());
      socket.write('{"id":"kanvibe","res');
      setImmediate(() => socket.end('ult":{"type":"ok"}}\n'));
    }));
    server.listen(socketPath);
    await once(server, "listening");
    execGit.mockResolvedValue(JSON.stringify({ sessions: [{ name: "feature", running: true, socket_path: socketPath }] }));
    await moveHerdrTab("feature", "w1:t1", 3);
    expect(request).toEqual({ id: "kanvibe", method: "tab.move", params: { tab_id: "w1:t1", insert_index: 3 } });
  });
  it("소켓의 오류 응답을 호출자에게 전달한다", async () => {
    directory = await mkdtemp(path.join(tmpdir(), "kvht-"));
    const socketPath = path.join(directory, "api");
    server = createServer((socket) => socket.once("data", () => socket.end('{"id":"kanvibe","error":{"code":"not_found","message":"tab missing"}}\n')));
    server.listen(socketPath);
    await once(server, "listening");
    execGit.mockResolvedValue(JSON.stringify({ sessions: [{ name: "feature", running: true, socket_path: socketPath }] }));
    await expect(moveHerdrTab("feature", "w1:t1", 3)).rejects.toThrow("tab missing");
  });
  it("종료된 세션에 소켓 요청을 보내지 않는다", async () => {
    execGit.mockResolvedValue(JSON.stringify({ sessions: [{ name: "feature", running: false, socket_path: "/missing" }] }));
    await expect(moveHerdrTab("feature", "w1:t1", 0)).rejects.toThrow("세션을 찾을 수 없습니다");
  });
  it("다른 세션 실패에도 실행 중 Herdr 에이전트를 수집한다", async () => {
    execGit.mockImplementation(async (command: string) => {
      if (command.includes("'session' 'list'")) return JSON.stringify({ sessions: [{ name: "feature", running: true }, { name: "broken", running: true }] });
      if (command.includes("--session 'broken'")) throw new Error("offline");
      if (command.includes("'agent' 'list'")) return result({ agents: [{ agent: "codex", tab_id: "w1:t1", cwd: "/old", foreground_cwd: "/work" }, { agent: null, cwd: "/work" }] });
      return result({ tabs: [{ tab_id: "w1:t1", label: "coding", focused: true }] });
    });
    await expect(listHerdrRunningAgentPanes()).resolves.toEqual([{ provider: "codex", sessionName: "feature", windowId: "w1:t1", windowName: "coding", worktreePath: "/work" }]);
  });
});


describe("Herdr SSH 소켓 수명", () => {
  it("명시 SSH 설정과 단독 터널로 요청한 뒤 프로세스와 소켓 디렉터리를 정리한다", async () => {
    vi.stubEnv("PORT", "4999");
    vi.stubEnv("HOST", "internal-host");
    vi.stubEnv("NODE_ENV", "test");
    vi.stubEnv("KANVIBE_INTERNAL_SECRET", "not-for-shell");
    execGit.mockResolvedValue(JSON.stringify({ sessions: [{ name: "feature", running: true, socket_path: "/remote/config/herdr.sock" }] }));
    const tunnel = Object.assign(new EventEmitter(), { stderr: new EventEmitter(), kill: vi.fn() });
    let localSocket = "";
    spawn.mockImplementation((_command: string, args: string[]) => {
      localSocket = args[args.indexOf("-L") + 1].split(":")[0];
      server = createServer((socket) => socket.once("data", () => socket.end('{"id":"kanvibe","result":{"type":"ok"}}\n')));
      server.listen(localSocket);
      tunnel.kill.mockImplementation(() => { server!.close(() => tunnel.emit("close", 0)); return true; });
      return tunnel;
    });
    await moveHerdrTab("feature", "w1:t2", 0, "remote");
    expect(spawn.mock.calls[0][0]).toBe("ssh");
    const args = spawn.mock.calls[0][1] as string[];
    expect(args).toEqual(expect.arrayContaining(["-i", "/key", "-p", "2202", "BatchMode=yes", "ControlMaster=no", "ControlPath=none", "ExitOnForwardFailure=yes"]));
    expect(args.at(-1)).toBe("remote");
    expect(spawn.mock.calls[0][2].env).not.toHaveProperty("PORT");
    expect(spawn.mock.calls[0][2].env).not.toHaveProperty("HOST");
    expect(spawn.mock.calls[0][2].env).not.toHaveProperty("NODE_ENV");
    expect(spawn.mock.calls[0][2].env).not.toHaveProperty("KANVIBE_INTERNAL_SECRET");
    expect(tunnel.kill).toHaveBeenCalledOnce();
    expect(await stat(path.dirname(localSocket)).catch(() => null)).toBeNull();
  });
  it("SSH 실행 오류를 전달하고 생성한 임시 소켓 경로를 제거한다", async () => {
    execGit.mockResolvedValue(JSON.stringify({ sessions: [{ name: "feature", running: true, socket_path: "/remote/herdr.sock" }] }));
    const tunnel = Object.assign(new EventEmitter(), { stderr: new EventEmitter(), kill: vi.fn(() => true) });
    let localSocket = "";
    spawn.mockImplementation((_command: string, args: string[]) => {
      localSocket = args[args.indexOf("-L") + 1].split(":")[0];
      queueMicrotask(() => { tunnel.emit("error", new Error("ssh unavailable")); tunnel.emit("close", 1); });
      return tunnel;
    });
    await expect(moveHerdrTab("feature", "w1:t1", 0, "remote")).rejects.toThrow("ssh unavailable");
    expect(await stat(path.dirname(localSocket)).catch(() => null)).toBeNull();
  });
});

describe("Herdr 소켓 시간 제한", () => {
  it("응답하지 않는 서버는 시간 초과로 실패하고 변경 요청을 재전송하지 않는다", async () => {
    directory = await mkdtemp(path.join(tmpdir(), "kvht-"));
    const socketPath = path.join(directory, "api");
    let requestCount = 0;
    server = createServer((socket) => socket.on("data", () => { requestCount += 1; }));
    server.listen(socketPath);
    await once(server, "listening");
    execGit.mockResolvedValue(JSON.stringify({ sessions: [{ name: "feature", running: true, socket_path: socketPath }] }));
    await expect(moveHerdrTab("feature", "w1:t1", 0)).rejects.toThrow("시간 초과");
    expect(requestCount).toBe(1);
  }, 8_000);

  it("SSH 터널이 준비되지 않으면 시간 초과로 종료하고 경로를 정리한다", async () => {
    execGit.mockResolvedValue(JSON.stringify({ sessions: [{ name: "feature", running: true, socket_path: "/remote/herdr.sock" }] }));
    const tunnel = Object.assign(new EventEmitter(), { stderr: new EventEmitter(), kill: vi.fn() });
    let localSocket = "";
    spawn.mockImplementation((_command: string, args: string[]) => {
      localSocket = args[args.indexOf("-L") + 1].split(":")[0];
      tunnel.kill.mockImplementation(() => { tunnel.emit("close", 0); return true; });
      return tunnel;
    });
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      const moving = moveHerdrTab("feature", "w1:t1", 0, "remote");
      const rejected = expect(moving).rejects.toThrow("시간 초과");
      await vi.waitFor(() => expect(spawn).toHaveBeenCalled());
      vi.setSystemTime(Date.now() + 31_000);
      await rejected;
      expect(tunnel.kill).toHaveBeenCalledOnce();
      expect(await stat(path.dirname(localSocket)).catch(() => null)).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("listHerdrMirrorPanes", () => {
  const scroll = (rows: number) => ({ max_offset_from_bottom: 0, offset_from_bottom: 0, viewport_rows: rows });
  const rect = (x: number, width: number) => ({ x, y: 0, width, height: 40 });
  const panes = [
    { pane_id: "w1:p2", tab_id: "w1:t2", scroll: scroll(38) },
    { pane_id: "w1:p1", tab_id: "w1:t1", scroll: scroll(38) },
    { pane_id: "w1:p3", tab_id: "w1:t1", scroll: scroll(38) },
  ];
  const tabs = [
    { tab_id: "w1:t1", number: 1, label: "work", focused: true },
    { tab_id: "w1:t2", number: 2, label: "logs", focused: false },
  ];
  const stubSession = (zoomed: boolean, processInfo: (paneId: string) => string) => execGit.mockImplementation(async (command: string) => {
    if (command === commandFor(["pane", "list"])) return result({ panes });
    if (command === commandFor(["tab", "list"])) return result({ tabs });
    if (command.includes("'pane' 'layout'")) {
      return result({ layout: {
        area: { x: 0, y: 0, width: 120, height: 40 },
        focused_pane_id: "w1:p1",
        zoomed,
        panes: [{ pane_id: "w1:p1", rect: rect(0, 60) }, { pane_id: "w1:p3", rect: rect(60, 60) }, { pane_id: "w1:p2", rect: rect(0, 120) }],
      } });
    }
    const paneId = /'process-info' '--pane' '([^']+)'/.exec(command)?.[1] ?? "";
    return processInfo(paneId);
  });

  it("탭 순서대로 좌표와 실행 명령을 합친다", async () => {
    stubSession(false, (paneId) => result({ process_info: { foreground_processes: [{ name: paneId === "w1:p3" ? "vim" : "zsh" }] } }));
    expect(await listHerdrMirrorPanes("feature", "remote")).toEqual([
      { id: "w1:p1", tabId: "w1:t1", tabName: "work", command: "zsh", left: 0, top: 0, width: 60, height: 38 },
      { id: "w1:p3", tabId: "w1:t1", tabName: "work", command: "vim", left: 60, top: 0, width: 60, height: 38 },
      { id: "w1:p2", tabId: "w1:t2", tabName: "logs", command: "zsh", left: 0, top: 0, width: 120, height: 38 },
    ]);
    expect(execGit.mock.calls.every((call) => call[1] === "remote")).toBe(true);
  });

  it("확대된 pane은 탭 전체 너비를 쓰고 다른 pane은 그대로 둔다", async () => {
    stubSession(true, () => result({ process_info: { foreground_processes: [] } }));
    const mirrorPanes = await listHerdrMirrorPanes("feature");
    expect(mirrorPanes.find((pane) => pane.id === "w1:p1")?.width).toBe(120);
    expect(mirrorPanes.find((pane) => pane.id === "w1:p3")?.width).toBe(60);
  });

  it("실행 명령을 읽지 못해도 pane 목록은 돌려준다", async () => {
    stubSession(false, () => { throw new Error("process lookup failed"); });
    expect((await listHerdrMirrorPanes("feature")).map((pane) => pane.command)).toEqual(["", "", ""]);
  });

  it("작업 경로가 사라져 복원하지 못한 pane은 비출 터미널이 없어 뺀다", async () => {
    stubSession(false, () => result({ process_info: { foreground_processes: [] } }));
    const listPanes = execGit.getMockImplementation()!;
    execGit.mockImplementation(async (command: string) => {
      if (command !== commandFor(["pane", "list"])) return listPanes(command);
      return result({ panes: [
        { pane_id: "w1:p1", tab_id: "w1:t1", restore_error: "Saved directory is unavailable." },
        ...panes.filter((pane) => pane.pane_id !== "w1:p1"),
      ] });
    });
    expect((await listHerdrMirrorPanes("feature")).map((pane) => pane.id)).toEqual(["w1:p3", "w1:p2"]);
  });
});


describe("공유 Herdr Spaces", () => {
  let mapping: Record<string, string>;
  let spaces: { workspace_id: string; label: string }[];
  beforeEach(() => {
    mapping = {};
    spaces = [];
    readTextFile.mockImplementation(async () => JSON.stringify(mapping));
    updateTextFile.mockImplementation(async (_path, update) => { mapping = JSON.parse(await update(JSON.stringify(mapping))); });
    execGit.mockImplementation(async (command: string) => {
      if (command.includes("'session' 'list'")) return JSON.stringify({ sessions: [{ name: "kanvibe", running: true, session_dir: "/config/kanvibe", socket_path: "/config/kanvibe/herdr.sock" }] });
      if (command.includes("'workspace' 'list'")) return result({ workspaces: spaces });
      if (command.includes("'workspace' 'create'")) {
        const id = `w${spaces.length + 1}`;
        spaces.push({ workspace_id: id, label: id });
        return result({ workspace: { workspace_id: id }, root_pane: { pane_id: `${id}:p1` } });
      }
      if (command.includes("'tab' 'list'")) return result({ tabs: [{ tab_id: "w1:t1", number: 1, label: "coding", focused: true }] });
      return result({});
    });
  });
  it("서로 다른 task를 같은 서버의 Spaces로 만들고 동시 재연결은 중복 생성하지 않는다", async () => {
    await Promise.all([ensureHerdrSession("kanvibe:task-a", "/work/a"), ensureHerdrSession("kanvibe:task-b", "/work/b"), ensureHerdrSession("kanvibe:task-a", "/work/a")]);
    expect(spaces).toHaveLength(2);
    expect(mapping).toEqual({ "task-a": "w1", "task-b": "w2" });
    expect(execGit.mock.calls.filter(([c]) => c.includes("'workspace' 'create'"))).toHaveLength(2);
    expect(execGit.mock.calls.every(([c]) => !c.includes("--session 'kanvibe:"))).toBe(true);
  });
  it("Space 이름을 바꾼 뒤에도 저장한 ID로 연결하고 시작 명령을 반복하지 않는다", async () => {
    mapping = { "task-a": "w1" };
    spaces = [{ workspace_id: "w1", label: "renamed" }];
    await ensureHerdrSession("kanvibe:task-a", "/work/a", { layoutType: PaneLayoutType.QUAD, panes: [{ position: 0, command: "once" }] });
    expect(execGit.mock.calls.some(([c]) => c.includes("'workspace' 'create'") || c.includes("'pane' 'send-text'"))).toBe(false);
    await readHerdrTabs("kanvibe:task-a");
    expect(execGit.mock.calls.at(-1)?.[0]).toBe(buildHerdrCommand("kanvibe", ["tab", "list", "--workspace", "w1"]));
  });
  it("한 task의 탭 mutation으로 다른 task를 선택할 수 없다", async () => {
    mapping = { "task-a": "w1" };
    const { runHerdrCommand } = await import("@/lib/herdr");
    await expect(runHerdrCommand("kanvibe:task-a", ["tab", "close", "w2:t1"])).rejects.toThrow("다른 Space");
    expect(execGit.mock.calls.some(([c]) => c.includes("'tab' 'close'"))).toBe(false);
  });
  it("문자가 포함된 Space ID가 있어도 기존 숫자 ID의 task를 재연결한다", async () => {
    mapping = { "task-a": "w1", "task-b": "wA" };
    spaces = [{ workspace_id: "w1", label: "a" }, { workspace_id: "wA", label: "b" }];

    await ensureHerdrSession("kanvibe:task-a", "/work/a");
    await readHerdrTabs("kanvibe:task-a");

    expect(execGit.mock.calls.at(-1)?.[0]).toBe(buildHerdrCommand("kanvibe", ["tab", "list", "--workspace", "w1"]));
    expect(execGit.mock.calls.some(([command]) => command.includes("'workspace' 'create'"))).toBe(false);
    expect(mapping).toEqual({ "task-a": "w1", "task-b": "wA" });
  });
  it("새로 발급된 wA Space를 저장한 뒤 다시 연결하고 해당 Space만 정리한다", async () => {
    mapping = { "task-a": "w1" };
    spaces = [{ workspace_id: "w1", label: "a" }];
    const original = execGit.getMockImplementation()!;
    execGit.mockImplementation(async (...args) => {
      if (!args[0].includes("'workspace' 'create'")) return original(...args);
      spaces.push({ workspace_id: "wA", label: "b" });
      return result({ root_pane: { pane_id: "wA:p1" } });
    });
    const { closeHerdrTaskWorkspace, getHerdrWorkspaceId } = await import("@/lib/herdr");

    await ensureHerdrSession("kanvibe:task-b", "/work/b");
    await expect(getHerdrWorkspaceId("kanvibe:task-b")).resolves.toBe("wA");
    await ensureHerdrSession("kanvibe:task-b", "/work/b");
    await readHerdrTabs("kanvibe:task-b");
    expect(execGit.mock.calls.at(-1)?.[0]).toBe(buildHerdrCommand("kanvibe", ["tab", "list", "--workspace", "wA"]));
    expect(execGit.mock.calls.filter(([command]) => command.includes("'workspace' 'create'"))).toHaveLength(1);

    await closeHerdrTaskWorkspace("kanvibe:task-b");
    expect(mapping).toEqual({ "task-a": "w1" });
    expect(execGit.mock.calls.some(([command]) => command.includes("'workspace' 'close' 'wA'"))).toBe(true);
    expect(execGit.mock.calls.some(([command]) => command.includes("'workspace' 'close' 'w1'"))).toBe(false);
  });
  it.each([
    ["tab", "close", "wA:t1"],
    ["tab", "close", "w2:tA"],
    ["pane", "send-text", "wA:p1", "hello"],
    ["pane", "send-text", "w2:pA", "hello"],
    ["pane", "split", "--pane", "wA:pA"],
  ])("문자 ID에도 다른 Space 접근을 차단한다: %j", async (...args) => {
    mapping = { "task-a": "w1" };
    const { runHerdrCommand } = await import("@/lib/herdr");

    await expect(runHerdrCommand("kanvibe:task-a", args)).rejects.toThrow("다른 Space");
    expect(execGit).not.toHaveBeenCalled();
  });
  it("초기 레이아웃 실패는 새 Space만 닫고 공유 서버와 다른 Space를 남긴다", async () => {
    spaces = [{ workspace_id: "w1", label: "other" }];
    const original = execGit.getMockImplementation()!;
    execGit.mockImplementation(async (...args) => args[0].includes("'pane' 'split'") ? Promise.reject(new Error("split failed")) : original(...args));
    await expect(ensureHerdrSession("kanvibe:task-b", "/work/b", { layoutType: PaneLayoutType.QUAD, panes: [] })).rejects.toThrow("split failed");
    expect(execGit.mock.calls.some(([c]) => c.includes("'workspace' 'close' 'w2'"))).toBe(true);
    expect(execGit.mock.calls.some(([c]) => c.includes("'session' 'stop'") || c.includes("'session' 'delete'"))).toBe(false);
    expect(mapping).toEqual({});
  });
  it("task 하나를 닫아도 다른 Space와 공유 서버를 유지한다", async () => {
    mapping = { "task-a": "w1", "task-b": "w2" };
    spaces = [{ workspace_id: "w1", label: "a" }, { workspace_id: "w2", label: "b" }];
    const { closeHerdrTaskWorkspace, isHerdrTaskAlive } = await import("@/lib/herdr");
    await closeHerdrTaskWorkspace("kanvibe:task-a");
    expect(mapping).toEqual({ "task-b": "w2" });
    expect(execGit.mock.calls.some(([c]) => c.includes("'workspace' 'close' 'w1'"))).toBe(true);
    expect(execGit.mock.calls.some(([c]) => c.includes("'session' 'stop'") || c.includes("'session' 'delete'"))).toBe(false);
    await expect(isHerdrTaskAlive("kanvibe:task-a")).resolves.toBe(false);
    await expect(isHerdrTaskAlive("kanvibe:task-b")).resolves.toBe(true);
  });
  it("공유 서버가 살아 있어도 해당 task Space가 없으면 세션 없음으로 반환한다", async () => {
    mapping = { "task-a": "w9" };
    const { isHerdrTaskAlive } = await import("@/lib/herdr");
    await expect(isHerdrTaskAlive("kanvibe:task-a")).resolves.toBe(false);
  });
  it("네이티브에서 닫힌 Space는 새로 생성하고 연결 ID를 갱신한다", async () => {
    mapping = { "task-a": "w9" };
    await ensureHerdrSession("kanvibe:task-a", "/work/a");
    expect(mapping).toEqual({ "task-a": "w1" });
    expect(spaces).toHaveLength(1);
  });
  it("공유 서버의 모바일 미러는 해당 task Space의 pane만 반환한다", async () => {
    mapping = { "task-a": "w1", "task-b": "w2" };
    const original = execGit.getMockImplementation()!;
    execGit.mockImplementation(async (...args) => {
      const command = args[0];
      if (command.includes("'pane' 'list'")) return result({ panes: [
        { pane_id: "w1:p1", tab_id: "w1:t1", scroll: { viewport_rows: 30 } },
        { pane_id: "w2:p1", tab_id: "w2:t1", scroll: { viewport_rows: 30 } },
      ] });
      if (command.includes("'pane' 'layout'")) return result({ layout: { area: { width: 80 }, panes: [], focused_pane_id: "w1:p1" } });
      if (command.includes("'pane' 'process-info'")) return result({ process_info: { foreground_processes: [{ name: "zsh" }] } });
      return original(...args);
    });
    expect((await listHerdrMirrorPanes("kanvibe:task-a")).map((pane) => pane.id)).toEqual(["w1:p1"]);
    expect(execGit.mock.calls.some(([command]) => command.includes("'process-info' '--pane' 'w2:p1'"))).toBe(false);
  });
  it("공유 서버의 에이전트는 workspace ID를 task 세션 참조로 돌려준다", async () => {
    mapping = { "task-a": "w1", "task-b": "w2" };
    const original = execGit.getMockImplementation()!;
    execGit.mockImplementation(async (...args) => args[0].includes("'agent' 'list'") ? result({ agents: [
      { agent: "codex", pane_id: "w1:p1", tab_id: "w1:t1", cwd: "/work/a", agent_status: "working" },
      { agent: "claude", pane_id: "w2:p1", tab_id: "w2:t1", cwd: "/work/b", agent_status: "working" },
    ] }) : original(...args));
    expect((await listHerdrRunningAgentPanes()).map((agent) => agent.sessionName)).toEqual(["kanvibe:task-a", "kanvibe:task-b"]);
  });
  it("입력 문자열이나 탭 이름을 다른 Space 식별자로 오인하지 않는다", async () => {
    mapping = { "task-a": "w1" };
    const { runHerdrCommand } = await import("@/lib/herdr");
    await expect(runHerdrCommand("kanvibe:task-a", ["pane", "send-text", "w1:p1", "w2:p1"])).resolves.toBeDefined();
    await expect(runHerdrCommand("kanvibe:task-a", ["tab", "rename", "w1:t1", "w2:t1"])).resolves.toBeDefined();
  });
  it("손상된 mapping을 덮어쓰지 않고 오류를 반환한다", async () => {
    readTextFile.mockResolvedValue("{broken");
    await expect(ensureHerdrSession("kanvibe:task-a", "/work/a")).rejects.toThrow();
    expect(updateTextFile).not.toHaveBeenCalled();
    expect(execGit.mock.calls.some(([c]) => c.includes("'workspace' 'create'"))).toBe(false);
  });
});


describe("Herdr client tab bar projection config", () => {
  beforeEach(() => { execGit.mockReset(); });
  it.each([
    ["", "top"],
    ['[ui]\ntab_bar_position = "bottom" # client setting', "bottom"],
    ["[ui]\ntab_bar_position = 'top'", "top"],
    ['ui.tab_bar_position = "bottom"', "bottom"],
    ['["ui"]\n"tab_bar_position" = "bottom"', "bottom"],
    ['[keys]\ntab_bar_position = "bottom"', "top"],
    ['[ui]\ntab_bar_position = "left"', null],
    ['ui = { tab_bar_position = "bottom" }', null],
    ['[ui]\ntab_bar_position = "top"\ntab_bar_position = "bottom"', null],
  ])("config %s resolves to %s", async (content, expected) => {
    execGit.mockResolvedValueOnce("/isolated/config/herdr.toml").mockResolvedValueOnce(content);
    expect(await readHerdrClientTabBarPosition("remote")).toBe(expected);
    expect(execGit.mock.calls[0][0]).toContain("HERDR_CONFIG_PATH");
    expect(execGit.mock.calls[0][0]).toContain("XDG_CONFIG_HOME");
    expect(execGit.mock.calls[1][0]).toContain("/isolated/config/herdr.toml");
    expect(execGit.mock.calls.every((call) => call[1] === "remote")).toBe(true);
  });
  it("fails closed when config cannot be read or its path is relative to an unknown client cwd", async () => {
    execGit.mockRejectedValueOnce(new Error("unreachable"));
    expect(await readHerdrClientTabBarPosition("remote")).toBeNull();
    execGit.mockResolvedValueOnce("relative/config.toml");
    expect(await readHerdrClientTabBarPosition()).toBeNull();
  });
});
