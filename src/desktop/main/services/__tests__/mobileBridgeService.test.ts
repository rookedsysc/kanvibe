/**
 * @vitest-environment node
 */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const appSettingsStore = new Map<string, string>();
const execGitMock = vi.fn<(command: string, sshHost?: string | null) => Promise<string>>();
const findOneByMock = vi.fn();
const spawnMock = vi.fn();

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

vi.mock("child_process", async (importOriginal) => ({
  ...(await importOriginal<typeof import("child_process")>()),
  spawn: (...args: unknown[]) => spawnMock(...args),
}));

const {
  authorizeMobileRequest,
  getTaskSurfaces,
  listPairedMobileDevices,
  onMobileDeviceUnpaired,
  pairMobileDevice,
  readMobilePairingCode,
  startMobilePairing,
  stopMobilePairing,
  stopAllPaneMirrors,
  subscribeToPane,
  unpairMobileDevice,
  unpairMobileDeviceByToken,
  writeToPane,
} = await import("@/desktop/main/services/mobileBridgeService");

const TMUX_TASK = { sessionType: SessionType.TMUX, sessionName: "kanvibe-task", sshHost: null };

/** 이 세션이 가진 pane은 `%19` 하나뿐이다. `%3`은 같은 tmux 서버의 다른 세션에 있는 pane을 가리킨다 */
const MIRROR_PANE_LINE = "%19\t@10\tbash\t0\t0\t80\t24\tfirst";

beforeEach(() => {
  appSettingsStore.clear();
  execGitMock.mockReset();
  findOneByMock.mockReset();
  spawnMock.mockReset();
  stopMobilePairing();
});

/** 세션 pane 목록과 스냅샷만 답하는 tmux 태스크를 세운다 */
function stubTmuxMirror(): void {
  findOneByMock.mockResolvedValue(TMUX_TASK);
  execGitMock.mockImplementation(async (command) =>
    command.includes("list-panes") ? MIRROR_PANE_LINE : "snapshot",
  );
}

function executedCommands(): string[] {
  return execGitMock.mock.calls.map(([command]) => command);
}

/**
 * tmux 스트림을 띄우는 자식 프로세스 대신, 테스트가 직접 출력을 흘려보내고 죽일 수 있는 가짜를 세운다.
 * `pid`를 비워 두면 `process.kill`을 부르지 않아 테스트가 실제 프로세스 그룹을 건드리지 않는다.
 */
function stubPaneProcess(): {
  emit: (chunk: string) => void;
  die: (event: "error" | "exit") => void;
  hasStderrListener: () => boolean;
} {
  const dataListeners = new Set<(chunk: string) => void>();
  const processListeners = new Map<string, Set<(payload?: unknown) => void>>();
  let isStderrDrained = false;

  spawnMock.mockReturnValue({
    pid: undefined,
    stdout: {
      setEncoding: () => {},
      on: (event: string, listener: (chunk: string) => void) => {
        if (event === "data") {
          dataListeners.add(listener);
        }
      },
    },
    stderr: {
      resume: () => {
        isStderrDrained = true;
      },
    },
    on: (event: string, listener: (payload?: unknown) => void) => {
      const listeners = processListeners.get(event) ?? new Set();
      listeners.add(listener);
      processListeners.set(event, listeners);
    },
    kill: () => {},
  });

  return {
    emit: (chunk) => dataListeners.forEach((listener) => listener(chunk)),
    die: (event) => processListeners.get(event)?.forEach((listener) => listener(new Error("boom"))),
    hasStderrListener: () => isStderrDrained,
  };
}

/**
 * 실패한 페어링은 코드 훑기를 늦추려고 300ms를 기다린다.
 * 실제로 재우면 실패를 다루는 테스트마다 그만큼 느려지므로 시계를 대신 민다.
 */
async function failedPairing(submittedCode: string): Promise<string | null> {
  vi.useFakeTimers();
  try {
    const pairing = pairMobileDevice(submittedCode, "iPhone");
    await vi.advanceTimersByTimeAsync(300);
    return await pairing;
  } finally {
    vi.useRealTimers();
  }
}

/** 인증은 통과 여부와 함께 기기 식별자까지 돌려준다. 통과했는지만 보는 곳은 이 helper를 지난다 */
async function isAuthorized(authorizationHeader: string | undefined): Promise<boolean> {
  return (await authorizeMobileRequest(authorizationHeader)) !== null;
}

/**
 * 해제 알림 리스너는 모듈 지역에 쌓이므로, 테스트가 끝나면 반드시 등록을 풀어 다음 테스트로 새지 않게 한다.
 * 등록 해제가 실제로 도는지도 이 helper가 함께 지킨다.
 */
async function withUnpairedDeviceIds(run: (unpairedDeviceIds: string[]) => Promise<void>): Promise<void> {
  const unpairedDeviceIds: string[] = [];
  const unsubscribe = onMobileDeviceUnpaired((deviceId) => unpairedDeviceIds.push(deviceId));
  try {
    await run(unpairedDeviceIds);
  } finally {
    unsubscribe();
  }
}

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

    expect(await failedPairing("000000")).toBeNull();
  });

  it("코드를 발급하지 않았으면 연결할 수 없다", async () => {
    expect(await failedPairing(readMobilePairingCode() ?? "123456")).toBeNull();
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

  /** 목록은 `app_settings`의 칸 하나라, 읽고-고쳐-쓰기가 겹치면 나중 쓰기가 앞선 토큰을 덮어 지운다 */
  it("두 기기가 겹쳐 연결해도 토큰이 둘 다 남는다", async () => {
    const firstPairing = pairMobileDevice(startMobilePairing().code, "iPhone");
    const secondPairing = pairMobileDevice(startMobilePairing().code, "iPad");

    const [firstToken, secondToken] = await Promise.all([firstPairing, secondPairing]);

    expect(await isAuthorized(`Bearer ${firstToken}`)).toBe(true);
    expect(await isAuthorized(`Bearer ${secondToken}`)).toBe(true);
    expect(await listPairedMobileDevices()).toHaveLength(2);
  });

  it("한쪽을 끊는 사이에 연결한 기기는 살아남고 끊은 기기는 되살아나지 않는다", async () => {
    const removedToken = await pairOneDevice("iPhone");
    const [removedDevice] = await listPairedMobileDevices();

    const pairing = pairMobileDevice(startMobilePairing().code, "iPad");
    const unpairing = unpairMobileDevice(removedDevice.deviceId);
    const [addedToken] = await Promise.all([pairing, unpairing]);

    expect(await isAuthorized(`Bearer ${addedToken}`)).toBe(true);
    expect(await isAuthorized(`Bearer ${removedToken}`)).toBe(false);
  });

  it("연결을 끊으면 그 기기의 토큰은 더 이상 통하지 않는다", async () => {
    const token = await pairOneDevice();
    const [device] = await listPairedMobileDevices();

    await unpairMobileDevice(device.deviceId);

    expect(await isAuthorized(`Bearer ${token}`)).toBe(false);
  });
});

describe("요청 인증", () => {
  it("연결된 기기의 토큰은 통과한다", async () => {
    const token = await pairOneDevice();

    expect(await isAuthorized(`Bearer ${token}`)).toBe(true);
  });

  it("토큰이 없으면 막는다", async () => {
    await pairOneDevice();

    expect(await isAuthorized(undefined)).toBe(false);
  });

  it("모르는 토큰은 막는다", async () => {
    await pairOneDevice();

    expect(await isAuthorized(`Bearer ${"f".repeat(64)}`)).toBe(false);
  });

  it("연결된 기기가 하나도 없으면 무엇도 통과하지 못한다", async () => {
    expect(await isAuthorized(`Bearer ${"a".repeat(64)}`)).toBe(false);
  });

  /** 통과 여부만 돌려주면 스트림을 연 소켓이 누구 것인지 알 수 없어, 기기 하나를 끊어도 그 소켓만 닫지 못한다 */
  it("통과한 요청은 그 토큰을 가진 기기의 식별자를 돌려준다", async () => {
    const token = await pairOneDevice();
    const [device] = await listPairedMobileDevices();

    expect(await authorizeMobileRequest(`Bearer ${token}`)).toBe(device.deviceId);
  });

  it("같은 순간에 연결된 두 기기도 따로 끊긴다", async () => {
    const keptToken = await pairOneDevice("iPhone");
    await pairOneDevice("iPad");
    const devices = await listPairedMobileDevices();

    await unpairMobileDevice(devices[1].deviceId);

    expect(await isAuthorized(`Bearer ${keptToken}`)).toBe(true);
  });
});

/**
 * 기기가 자기 저장소만 비우면 데스크탑 항목이 남아 만료되지 않는 토큰이 계속 통과한다.
 * 그래서 기기가 스스로 지울 수 있어야 하되, 지우는 범위는 요청이 증명한 기기 하나로 묶여 있어야 한다.
 */
describe("기기 스스로 연결 끊기", () => {
  it("토큰의 주인만 지우고 다른 기기는 그대로 둔다", async () => {
    const removedToken = await pairOneDevice("iPhone");
    const keptToken = await pairOneDevice("iPad");

    expect(await unpairMobileDeviceByToken(`Bearer ${removedToken}`)).toBe(true);

    expect(await isAuthorized(`Bearer ${removedToken}`)).toBe(false);
    expect(await isAuthorized(`Bearer ${keptToken}`)).toBe(true);
    expect(await listPairedMobileDevices()).toEqual([
      { deviceId: expect.any(String), deviceName: "iPad", pairedAt: expect.any(String) },
    ]);
  });

  it("모르는 토큰으로는 아무것도 지우지 않는다", async () => {
    const token = await pairOneDevice();

    expect(await unpairMobileDeviceByToken(`Bearer ${"f".repeat(64)}`)).toBe(false);
    expect(await isAuthorized(`Bearer ${token}`)).toBe(true);
    expect(await listPairedMobileDevices()).toHaveLength(1);
  });

  it("헤더가 없으면 아무것도 지우지 않는다", async () => {
    await pairOneDevice();

    expect(await unpairMobileDeviceByToken(undefined)).toBe(false);
    expect(await listPairedMobileDevices()).toHaveLength(1);
  });

  /** 이 경로도 [withDeviceList] 줄에 서야, 겹친 연결이 지운 기기를 되살리거나 새 토큰을 함께 지우지 않는다 */
  it("연결과 겹쳐 끊어도 새 기기는 남고 끊은 기기는 되살아나지 않는다", async () => {
    const removedToken = await pairOneDevice("iPhone");

    const pairing = pairMobileDevice(startMobilePairing().code, "iPad");
    const unpairing = unpairMobileDeviceByToken(`Bearer ${removedToken}`);
    const [addedToken, wasRemoved] = await Promise.all([pairing, unpairing]);

    expect(wasRemoved).toBe(true);
    expect(await isAuthorized(`Bearer ${addedToken}`)).toBe(true);
    expect(await isAuthorized(`Bearer ${removedToken}`)).toBe(false);
    expect(await listPairedMobileDevices()).toHaveLength(1);
  });
});

/**
 * 목록에서 지운 기기라도 이미 열어 둔 pane 스트림은 살아 있어 터미널을 계속 비춘다.
 * 소켓은 `mobileRoutes`가 들고 있으므로, 이 서비스가 할 일은 지운 기기를 정확히 알리는 것까지다.
 */
describe("연결 해제 알림", () => {
  it("설정 화면이 끊은 기기의 식별자를 알린다", async () => {
    await pairOneDevice();
    const [device] = await listPairedMobileDevices();

    await withUnpairedDeviceIds(async (unpairedDeviceIds) => {
      await unpairMobileDevice(device.deviceId);

      expect(unpairedDeviceIds).toEqual([device.deviceId]);
    });
  });

  it("기기가 자기 토큰으로 끊어도 같은 신호가 나간다", async () => {
    const token = await pairOneDevice();
    const [device] = await listPairedMobileDevices();

    await withUnpairedDeviceIds(async (unpairedDeviceIds) => {
      expect(await unpairMobileDeviceByToken(`Bearer ${token}`)).toBe(true);

      expect(unpairedDeviceIds).toEqual([device.deviceId]);
    });
  });

  /** 지운 것이 없는데 알리면 듣는 쪽이 아직 연결된 기기의 스트림을 닫을 근거로 삼는다 */
  it("지운 기기가 없으면 아무것도 알리지 않는다", async () => {
    const token = await pairOneDevice();
    const [device] = await listPairedMobileDevices();

    await withUnpairedDeviceIds(async (unpairedDeviceIds) => {
      await unpairMobileDevice(`${device.deviceId}-없는-기기`);
      expect(await unpairMobileDeviceByToken(`Bearer ${"f".repeat(64)}`)).toBe(false);

      expect(unpairedDeviceIds).toEqual([]);
      expect(await isAuthorized(`Bearer ${token}`)).toBe(true);
    });
  });

  it("등록을 풀면 더는 알림을 받지 않는다", async () => {
    await pairOneDevice();
    const [device] = await listPairedMobileDevices();

    const unpairedDeviceIds: string[] = [];
    onMobileDeviceUnpaired((deviceId) => unpairedDeviceIds.push(deviceId))();
    await unpairMobileDevice(device.deviceId);

    expect(unpairedDeviceIds).toEqual([]);
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

/**
 * tmux pane id는 서버 전역이라 `-t %3`이 세션 경계를 넘는다.
 * 토큰 하나를 가진 기기가 다른 태스크의 pane까지 읽고 쓰는 것을 막아야 한다.
 */
describe("pane 접근 범위", () => {
  it("세션에 없는 pane은 구독할 수 없다", async () => {
    stubTmuxMirror();

    await expect(subscribeToPane("task-scope-read", "%3", () => {}, () => {})).rejects.toMatchObject({
      reason: "session-not-running",
    });
    expect(executedCommands().some((command) => command.includes("capture-pane"))).toBe(false);
  });

  it("세션에 없는 pane에는 키를 넣을 수 없다", async () => {
    stubTmuxMirror();

    await expect(writeToPane("task-scope-write", "%3", "ls")).rejects.toMatchObject({
      reason: "session-not-running",
    });
    expect(executedCommands().some((command) => command.includes("send-keys"))).toBe(false);
  });

  it("세션 안의 pane에는 키가 그대로 들어간다", async () => {
    stubTmuxMirror();

    await writeToPane("task-scope-write", "%19", "ls");

    expect(executedCommands().some((command) => command.includes("send-keys -t '%19'"))).toBe(true);
  });
});


/**
 * 미러 자식은 `detached`로 떠 있어 부모가 죽어도 함께 죽지 않고, spawn 실패의 `error`는
 * 리스너가 없으면 그대로 던져져 Electron main을 끈다. 두 가지 모두 pane 하나를 여는 것만으로 도달한다.
 */
describe("미러 자식 프로세스의 수명", () => {
  it("자식의 stderr를 흘려보내 파이프가 차서 스트림이 서지 않게 한다", async () => {
    stubTmuxMirror();
    const paneProcess = stubPaneProcess();

    const unsubscribe = await subscribeToPane("task-stderr", "%19", () => {}, () => {});

    expect(paneProcess.hasStderrListener()).toBe(true);
    unsubscribe();
  });

  it.each(["error", "exit"] as const)("자식이 %s로 죽으면 구독자에게 알린다", async (deathEvent) => {
    stubTmuxMirror();
    const paneProcess = stubPaneProcess();

    let hasEnded = false;
    const unsubscribe = await subscribeToPane("task-death", "%19", () => {}, () => {
      hasEnded = true;
    });

    paneProcess.die(deathEvent);

    expect(hasEnded).toBe(true);
    unsubscribe();
  });

  it("죽은 구독은 자리를 비워 다음 구독자가 새 스트림을 받는다", async () => {
    stubTmuxMirror();
    const firstProcess = stubPaneProcess();

    const unsubscribeFirst = await subscribeToPane("task-revive", "%19", () => {}, () => {});
    firstProcess.die("exit");

    const secondProcess = stubPaneProcess();
    const receivedChunks: string[] = [];
    const unsubscribeSecond = await subscribeToPane(
      "task-revive",
      "%19",
      (chunk) => receivedChunks.push(chunk),
      () => {},
    );

    secondProcess.emit("살아 있다");

    expect(receivedChunks).toEqual(["snapshot", "살아 있다"]);
    unsubscribeFirst();
    unsubscribeSecond();
  });

  it("앱을 끌 때 열려 있는 미러의 파이프를 거둔다", async () => {
    stubTmuxMirror();
    stubPaneProcess();

    await subscribeToPane("task-quit", "%19", () => {}, () => {});
    expect(executedCommands()).not.toContain("tmux pipe-pane -t '%19'");

    stopAllPaneMirrors();
    await Promise.resolve();

    expect(executedCommands()).toContain("tmux pipe-pane -t '%19'");
  });
});

describe("pane 구독 정리", () => {
  /**
   * 두 번째 구독자가 자리를 잡은 뒤 콜백을 얹기 전에 첫 구독자가 나가는 순간을 만든다.
   * 완성된 구독의 listener만 세면 이 틈에서 스트림이 끊기고 두 번째 구독자는 영영 조용해진다.
   */
  it("자리를 잡은 구독자가 있으면 앞선 구독자가 나가도 스트림을 끊지 않는다", async () => {
    stubTmuxMirror();
    const paneProcess = stubPaneProcess();

    const unsubscribeFirst = await subscribeToPane("task-race", "%19", () => {}, () => {});

    const receivedChunks: string[] = [];
    const unsubscribeSecond = await subscribeToPane(
      "task-race",
      "%19",
      (chunk) => {
        if (receivedChunks.length === 0) {
          queueMicrotask(unsubscribeFirst);
        }
        receivedChunks.push(chunk);
      },
      () => {},
    );

    paneProcess.emit("stream");

    expect(receivedChunks).toEqual(["snapshot", "stream"]);
    expect(executedCommands()).not.toContain("tmux pipe-pane -t '%19'");

    unsubscribeSecond();
    expect(executedCommands()).toContain("tmux pipe-pane -t '%19'");
  });

  it("같은 구독 해제를 두 번 불러도 남은 구독자의 몫을 깎지 않는다", async () => {
    stubTmuxMirror();
    const paneProcess = stubPaneProcess();

    const unsubscribeFirst = await subscribeToPane("task-idempotent", "%19", () => {}, () => {});
    const receivedChunks: string[] = [];
    const unsubscribeSecond = await subscribeToPane(
      "task-idempotent",
      "%19",
      (chunk) => receivedChunks.push(chunk),
      () => {},
    );

    unsubscribeFirst();
    unsubscribeFirst();
    paneProcess.emit("stream");

    expect(receivedChunks).toEqual(["snapshot", "stream"]);
    expect(executedCommands()).not.toContain("tmux pipe-pane -t '%19'");

    unsubscribeSecond();
  });
});
