// @vitest-environment node
import { EventEmitter } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  spawn: vi.fn(),
  execGit: vi.fn(),
  find: vi.fn(),
  findProjects: vi.fn(),
  parseSSHConfig: vi.fn(),
  buildSSHArgs: vi.fn(),
}));

vi.mock("node:child_process", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:child_process")>()),
  spawn: mocks.spawn,
}));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execGit }));
vi.mock("@/lib/database", () => ({
  getTaskRepository: async () => ({ find: mocks.find }),
  getProjectRepository: async () => ({ find: mocks.findProjects }),
}));
vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getOrCreateHookClientId: async () => "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed",
}));
vi.mock("@/lib/sshConfig", () => ({ parseSSHConfig: mocks.parseSSHConfig, buildSSHArgs: mocks.buildSSHArgs }));
vi.mock("@/lib/shellEnvironment", () => ({ createLocalShellEnvironment: () => ({}) }));

class FakeSSHProcess extends EventEmitter {
  stdin = Object.assign(new EventEmitter(), { end: vi.fn() });
  stderr = new EventEmitter();
  kill = vi.fn(() => {
    this.emit("close", null);
    return true;
  });
}

const REMOTE_SOCKET = "/tmp/kv-1000/1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed.sock";

describe("hookTunnelService", () => {
  let service: typeof import("@/desktop/main/services/hookTunnelService");
  let sshProcesses: FakeSSHProcess[];

  const flushAsync = async () => {
    await vi.advanceTimersByTimeAsync(50);
    await new Promise((resolve) => setImmediate(resolve));
    await vi.advanceTimersByTimeAsync(50);
  };

  const waitForSpawnCount = async (count: number) => {
    await vi.waitFor(() => expect(sshProcesses.length).toBe(count));
  };

  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
    sshProcesses = [];
    mocks.spawn.mockImplementation(() => {
      const process = new FakeSSHProcess();
      sshProcesses.push(process);
      return process;
    });
    mocks.execGit.mockResolvedValue(REMOTE_SOCKET);
    mocks.parseSSHConfig.mockResolvedValue([
      { host: "host-a", hostname: "10.0.0.7", port: 22, username: "dev" },
      { host: "host-a-alias", hostname: "10.0.0.7", port: 22, username: "dev" },
      { host: "host-b", hostname: "10.0.0.8", port: 22, username: "dev" },
    ]);
    mocks.buildSSHArgs.mockReturnValue(["-p", "22", "host-a"]);
    mocks.find.mockResolvedValue([{ sshHost: "host-a" }]);
    mocks.findProjects.mockResolvedValue([]);
    service = await import("@/desktop/main/services/hookTunnelService");
  });

  afterEach(() => {
    service.stopHookTunnels();
    vi.useRealTimers();
  });

  it("터널 인자는 전달 실패를 종료로 드러내고 앱이 죽으면 함께 끝나도록 만든다", () => {
    expect(service.buildHookTunnelSSHArgs(["-p", "22", "host-a"], "/remote.sock", "/local.sock")).toEqual([
      "-p", "22",
      "-o", "ExitOnForwardFailure=yes",
      "-o", "StreamLocalBindUnlink=yes",
      "-o", "ControlMaster=no",
      "-o", "ControlPath=none",
      "-o", "StrictHostKeyChecking=yes",
      "-R", "/remote.sock:/local.sock",
      "host-a",
      "cat >/dev/null",
    ]);
  });

  it("시작 전에는 터널을 열지 않는다", async () => {
    service.ensureHookTunnel("host-a");
    await flushAsync();
    expect(mocks.spawn).not.toHaveBeenCalled();
  });

  it("같은 원격 계정을 가리키는 별칭들은 터널 하나를 함께 쓰고, 그 터널은 두 별칭의 요청을 모두 받는다", async () => {
    // Given
    const listenerHosts: ReadonlySet<string>[] = [];
    mocks.find.mockResolvedValue([{ sshHost: "host-a" }, { sshHost: "host-a-alias" }]);

    // When
    service.startHookTunnels((sshHosts) => {
      listenerHosts.push(sshHosts);
      return () => undefined;
    });
    await waitForSpawnCount(1);
    await flushAsync();

    // Then
    expect(sshProcesses.length).toBe(1);
    expect([...listenerHosts[0]].sort()).toEqual(["host-a", "host-a-alias"]);
  });

  it("다른 원격 계정은 터널을 따로 연다", async () => {
    mocks.find.mockResolvedValue([{ sshHost: "host-a" }, { sshHost: "host-b" }]);
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(2);
  });

  it("완료한 task만 남은 호스트도 worktree의 hook이 쓰도록 터널을 유지한다", async () => {
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(1);
    expect(mocks.find.mock.calls[0][0].where).not.toHaveProperty("status");
  });

  it("task가 없어도 원격 프로젝트가 있는 호스트의 터널은 연다", async () => {
    mocks.find.mockResolvedValue([]);
    mocks.findProjects.mockResolvedValue([{ sshHost: "host-b" }]);
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(1);
  });

  it("진행 중인 원격 task가 있는 호스트의 터널을 열고, 없어지면 닫는다", async () => {
    // Given
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(1);
    expect(mocks.spawn.mock.calls[0][1]).toEqual(expect.arrayContaining(["-R"]));

    // When
    mocks.find.mockResolvedValue([]);
    await vi.advanceTimersByTimeAsync(5 * 60_000);

    // Then
    await vi.waitFor(() => expect(sshProcesses[0].kill).toHaveBeenCalled());
    await vi.advanceTimersByTimeAsync(60_000);
    expect(sshProcesses.length).toBe(1);
  });

  it("같은 호스트를 다시 요청해도 터널을 하나만 연다", async () => {
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(1);
    service.ensureHookTunnel("host-a");
    await flushAsync();
    expect(sshProcesses.length).toBe(1);
  });

  it("터널이 끊기면 다시 연결하고, 금세 또 끊기면 대기를 늘린다", async () => {
    // Given
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(1);

    // When
    sshProcesses[0].emit("close", 255);
    await vi.advanceTimersByTimeAsync(4_999);
    expect(sshProcesses.length).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    await waitForSpawnCount(2);

    sshProcesses[1].emit("close", 255);
    await vi.advanceTimersByTimeAsync(9_999);
    expect(sshProcesses.length).toBe(2);
    await vi.advanceTimersByTimeAsync(1);

    // Then
    await waitForSpawnCount(3);
  });

  it("준비 명령이 실패해도 앱은 멈추지 않고 다시 시도한다", async () => {
    // Given
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    mocks.execGit.mockRejectedValueOnce(new Error("다른 사용자의 디렉터리"));
    service.startHookTunnels(() => () => undefined);
    await vi.waitFor(() => expect(mocks.execGit).toHaveBeenCalledTimes(1));
    expect(sshProcesses.length).toBe(0);

    // When
    await vi.advanceTimersByTimeAsync(5_000);

    // Then
    await waitForSpawnCount(1);
  });

  it("다른 client의 소켓 경로를 준비 결과로 받으면 터널을 열지 않는다", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    mocks.execGit.mockResolvedValue("/tmp/kv-1000/another-client.sock");

    service.startHookTunnels(() => () => undefined);
    await flushAsync();

    expect(sshProcesses).toEqual([]);
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining("Hook tunnel"),
      expect.stringContaining("소켓 경로"),
    );
    warn.mockRestore();
  });

  it("잠자기에서 깨면 살아 있어 보이는 연결을 끊고 곧바로 다시 잇는다", async () => {
    // Given
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(1);

    // When
    service.reconnectHookTunnels();

    // Then
    expect(sshProcesses[0].kill).toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(5_000);
    await waitForSpawnCount(2);
  });

  it("중지하면 터널을 닫고 다시 연결하지 않는다", async () => {
    // Given
    service.startHookTunnels(() => () => undefined);
    await waitForSpawnCount(1);

    // When
    service.stopHookTunnels();
    await vi.advanceTimersByTimeAsync(60_000);

    // Then
    expect(sshProcesses[0].kill).toHaveBeenCalled();
    expect(sshProcesses.length).toBe(1);
  });
});

describe("isTaskReachableFromTunnel", () => {
  it.each([
    ["평문 포트의 요청은 호스트를 묻지 않는다", { sshHost: "host-b" }, null, undefined, true],
    ["터널이 맡은 호스트의 task", { sshHost: "host-a" }, null, new Set(["host-a"]), true],
    ["다른 호스트의 task", { sshHost: "host-b" }, null, new Set(["host-a"]), false],
    ["sshHost가 비어 있는 옛 task는 프로젝트 호스트로 판정한다", { sshHost: null }, { sshHost: "host-a" }, new Set(["host-a"]), true],
    ["호스트를 알 수 없는 task는 터널로 다루지 못한다", { sshHost: null }, null, new Set(["host-a"]), false],
  ])("%s", async (_name, task, project, sourceSshHosts, expected) => {
    const { isTaskReachableFromTunnel } = await import("@/desktop/main/services/hookTunnelService");
    expect(isTaskReachableFromTunnel(task, project, sourceSshHosts)).toBe(expected);
  });
});
