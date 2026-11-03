/** @vitest-environment node */
import { createServer } from "net";
import { mkdtemp, rm, stat } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { once, EventEmitter } from "events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PaneLayoutType } from "@/entities/PaneLayoutConfig";
import { buildHerdrCommand, buildHerdrSessionCleanupCommand, sanitizeHerdrSessionName } from "@/lib/worktree";
import { ensureHerdrSession, listHerdrRunningAgentPanes, moveHerdrTab, readHerdrTabs } from "@/lib/herdr";

const { execGit, spawn } = vi.hoisted(() => ({ execGit: vi.fn(), spawn: vi.fn() }));
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

describe("Herdr 부트스트랩", () => {
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
