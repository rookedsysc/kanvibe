// @vitest-environment node
import { EventEmitter } from "node:events";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  spawn: vi.fn(),
  execGit: vi.fn(),
  find: vi.fn(),
  getRemoteHookServerUrl: vi.fn(),
  parseSSHConfig: vi.fn(),
  buildSSHArgs: vi.fn(),
}));

vi.mock("node:child_process", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:child_process")>()),
  spawn: mocks.spawn,
}));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execGit }));
vi.mock("@/lib/database", () => ({ getTaskRepository: async () => ({ find: mocks.find }) }));
vi.mock("@/lib/hookEndpoint", () => ({ getRemoteHookServerUrl: mocks.getRemoteHookServerUrl }));
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

const REMOTE_SOCKET = "/tmp/kv-1000/10.0.0.5_9736.sock";

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
    mocks.getRemoteHookServerUrl.mockResolvedValue("http://10.0.0.5:9736");
    mocks.parseSSHConfig.mockResolvedValue([{ host: "host-a" }, { host: "host-b" }]);
    mocks.buildSSHArgs.mockReturnValue(["-p", "22", "host-a"]);
    mocks.find.mockResolvedValue([{ sshHost: "host-a" }]);
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
