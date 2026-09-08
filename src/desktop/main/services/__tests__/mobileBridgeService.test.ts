/**
 * @vitest-environment node
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const appSettingsStore = new Map<string, string>();
const execGitMock = vi.fn<(command: string, sshHost?: string | null) => Promise<string>>();
const findOneByMock = vi.fn();

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getAppSetting: async (key: string) => appSettingsStore.get(key) ?? null,
  setAppSetting: async (key: string, value: string) => {
    appSettingsStore.set(key, value);
  },
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: (command: string, sshHost?: string | null) => execGitMock(command, sshHost),
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: async () => ({ findOneBy: findOneByMock }),
}));

vi.mock("@/desktop/main/services/kanbanService", () => ({ getTasksByStatus: async () => ({ tasks: {} }) }));
vi.mock("@/desktop/main/services/projectService", () => ({ getAllProjects: async () => [] }));

const {
  authorizeMobileRequest,
  getTaskSurfaces,
  listPairedMobileDevices,
  pairMobileDevice,
  readMobilePairingCode,
  startMobilePairing,
  stopMobilePairing,
  unpairMobileDevice,
} = await import("@/desktop/main/services/mobileBridgeService");

const TMUX_TASK = { sessionType: SessionType.TMUX, sessionName: "kanvibe-task", sshHost: null };

beforeEach(() => {
  appSettingsStore.clear();
  execGitMock.mockReset();
  findOneByMock.mockReset();
  stopMobilePairing();
});

async function pairOneDevice(deviceName = "iPhone"): Promise<string> {
  const { code } = startMobilePairing();
  const token = await pairMobileDevice(code, deviceName);
  if (!token) {
    throw new Error("페어링이 실패했다");
  }
  return token;
}

describe("기기 페어링", () => {
  it("화면의 코드로 연결하면 토큰을 받는다", async () => {
    const { code } = startMobilePairing();

    expect(await pairMobileDevice(code, "iPhone")).toMatch(/^[0-9a-f]{64}$/);
  });

  it("틀린 코드로는 토큰을 받지 못한다", async () => {
    startMobilePairing();

    expect(await pairMobileDevice("000000", "iPhone")).toBeNull();
  });

  it("코드를 발급하지 않았으면 연결할 수 없다", async () => {
    expect(await pairMobileDevice(readMobilePairingCode() ?? "123456", "iPhone")).toBeNull();
  });

  it("연결한 기기는 저장되어 목록에 남는다", async () => {
    await pairOneDevice("iPad");

    expect(await listPairedMobileDevices()).toEqual([
      { deviceId: expect.any(String), deviceName: "iPad", pairedAt: expect.any(String) },
    ]);
  });

  it("기기 목록에 토큰은 들어가지 않는다", async () => {
    await pairOneDevice();

    expect(JSON.stringify(await listPairedMobileDevices())).not.toContain("token");
  });

  it("연결을 끊으면 그 기기의 토큰은 더 이상 통하지 않는다", async () => {
    const token = await pairOneDevice();
    const [device] = await listPairedMobileDevices();

    await unpairMobileDevice(device.deviceId);

    expect(await authorizeMobileRequest(`Bearer ${token}`)).toBe(false);
  });
});

describe("요청 인증", () => {
  it("연결된 기기의 토큰은 통과한다", async () => {
    const token = await pairOneDevice();

    expect(await authorizeMobileRequest(`Bearer ${token}`)).toBe(true);
  });

  it("토큰이 없으면 막는다", async () => {
    await pairOneDevice();

    expect(await authorizeMobileRequest(undefined)).toBe(false);
  });

  it("모르는 토큰은 막는다", async () => {
    await pairOneDevice();

    expect(await authorizeMobileRequest(`Bearer ${"f".repeat(64)}`)).toBe(false);
  });

  it("연결된 기기가 하나도 없으면 무엇도 통과하지 못한다", async () => {
    expect(await authorizeMobileRequest(`Bearer ${"a".repeat(64)}`)).toBe(false);
  });

  it("같은 순간에 연결된 두 기기도 따로 끊긴다", async () => {
    const keptToken = await pairOneDevice("iPhone");
    await pairOneDevice("iPad");
    const devices = await listPairedMobileDevices();

    await unpairMobileDevice(devices[1].deviceId);

    expect(await authorizeMobileRequest(`Bearer ${keptToken}`)).toBe(true);
  });
});

describe("태스크 pane 조회", () => {
  it("pane을 탭 단위로 묶어 돌려준다", async () => {
    findOneByMock.mockResolvedValue(TMUX_TASK);
    execGitMock.mockResolvedValue(
      [
        "%19\t@10\tbash\t0\t0\t50\t30\tfirst",
        "%20\t@10\tclaude\t51\t0\t49\t30\tfirst",
        "%21\t@11\tbash\t0\t0\t100\t30\tsecond",
      ].join("\n"),
    );

    const surfaces = await getTaskSurfaces("task-1");

    expect(surfaces.tabs.map((tab) => ({ id: tab.id, panes: tab.panes.length }))).toEqual([
      { id: "@10", panes: 2 },
      { id: "@11", panes: 1 },
    ]);
  });

  it("좌표를 그대로 실어 보내 태블릿이 배치를 재현할 수 있다", async () => {
    findOneByMock.mockResolvedValue(TMUX_TASK);
    execGitMock.mockResolvedValue("%20\t@10\tclaude\t51\t0\t49\t30\tfirst");

    const [pane] = (await getTaskSurfaces("task-1")).tabs[0].panes;

    expect(pane).toEqual({
      id: "%20",
      tabId: "@10",
      tabName: "first",
      command: "claude",
      left: 51,
      top: 0,
      width: 49,
      height: 30,
    });
  });

  it("세션이 지정되지 않은 태스크는 사유를 알린다", async () => {
    findOneByMock.mockResolvedValue({ sessionType: null, sessionName: null, sshHost: null });

    await expect(getTaskSurfaces("task-1")).rejects.toMatchObject({ reason: "no-session" });
  });

  it("세션이 꺼져 있으면 빈 화면 대신 사유를 알린다", async () => {
    findOneByMock.mockResolvedValue(TMUX_TASK);
    execGitMock.mockResolvedValue("");

    await expect(getTaskSurfaces("task-1")).rejects.toMatchObject({ reason: "session-not-running" });
  });

  it("없는 태스크도 사유로 처리한다", async () => {
    findOneByMock.mockResolvedValue(null);

    await expect(getTaskSurfaces("task-1")).rejects.toMatchObject({ reason: "no-session" });
  });

  it("zellij 태스크는 JSON 조회를 쓴다", async () => {
    findOneByMock.mockResolvedValue({
      sessionType: SessionType.ZELLIJ,
      sessionName: "kanvibe-zj",
      sshHost: null,
    });
    execGitMock.mockImplementation(async (command) => {
      if (command.includes("--version")) {
        return "zellij 0.44.3";
      }
      return JSON.stringify([
        {
          id: 1,
          is_plugin: false,
          tab_id: 0,
          tab_name: "Tab #1",
          title: "Pane #2",
          terminal_command: "claude",
          pane_x: 40,
          pane_y: 0,
          pane_rows: 24,
          pane_columns: 40,
        },
      ]);
    });

    const surfaces = await getTaskSurfaces("task-zj");

    expect(surfaces.tabs[0].panes[0].id).toBe("terminal_1");
    expect(execGitMock.mock.calls.some(([command]) => command.includes("list-panes --json"))).toBe(true);
  });
});
