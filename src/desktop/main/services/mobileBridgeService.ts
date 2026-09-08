import { spawn, type ChildProcess } from "child_process";
import { getAppSetting, setAppSetting } from "@/desktop/main/services/appSettingsService";
import { getTasksByStatus } from "@/desktop/main/services/kanbanService";
import { getAllProjects } from "@/desktop/main/services/projectService";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import { getTaskRepository } from "@/lib/database";
import { execGit } from "@/lib/gitOperations";
import {
  findPairedDevice,
  issuePairingCode,
  mintDeviceId,
  mintDeviceToken,
  parseBearerToken,
  parsePairedDevices,
  readPendingPairingCode,
  redeemPairingCode,
  revokePairingCode,
  serializePairedDevices,
  type PairedDevice,
} from "@/lib/mobilePairing";
import {
  buildTmuxCancelPipePaneCommand,
  buildTmuxCapturePaneCommand,
  buildTmuxListMirrorPanesCommand,
  buildTmuxPaneStreamCommand,
  buildTmuxSendKeysCommand,
  buildZellijDumpPaneCommand,
  buildZellijListMirrorPanesCommand,
  buildZellijWritePaneCommand,
  parseTmuxMirrorPaneList,
  parseZellijMirrorPaneList,
  type MirrorPane,
} from "@/lib/paneMirror";
import {
  buildZellijVersionCommand,
  supportsZellijTabIdCommands,
} from "@/lib/terminalTabs";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig } from "@/lib/sshConfig";
import { quoteForPosixShell } from "@/lib/worktree";

/**
 * 모바일 클라이언트가 데스크탑을 읽고 터미널 pane을 비추는 경로.
 *
 * 이 서비스는 데스크탑 화면을 바꾸지 않는다. pane을 고르거나 크기를 바꾸는 명령은 하나도 부르지 않고,
 * 스냅샷과 출력 스트림만 가져다 나눠 보낸다. 근거는 `paneMirror.ts` 머리말에 적어 두었다.
 *
 * 부르는 곳이 둘이다. 페어링 코드를 띄우고 기기를 끊는 것은 설정 화면이 `serviceRegistry`를 거쳐 부르고,
 * 보드와 pane은 hook 서버의 `/api/mobile/*` 경로가 부른다.
 * 지켜야 하는 경계는 IPC가 아니라 네트워크다. 렌더러는 이미 터미널을 직접 열 수 있으므로 IPC로 더 열리는 것이 없고,
 * 네트워크에서 들어오는 요청만 [authorizeMobileRequest]를 반드시 지나야 한다.
 */

/** 조회 명령은 짧게 끝나야 모바일 화면이 멈춘 것처럼 보이지 않는다 */
const MIRROR_COMMAND_TIMEOUT_MS = 5_000;

/**
 * zellij에는 tmux `pipe-pane`에 해당하는 출력 스트림이 없어 화면을 주기적으로 다시 뜬다.
 * 간격은 탭 폴링이 이미 쓰는 값을 그대로 따른다. 같은 이유로 도는 폴링이 서로 다른 주기를 갖는 편이 더 나쁘다.
 */
const LOCAL_DUMP_INTERVAL_MS = 1_000;
const REMOTE_DUMP_INTERVAL_MS = 3_000;

const PAIRED_DEVICES_KEY = "mobile_paired_devices";

/** 태스크 하나의 세션 좌표. `terminalTabService`도 같은 값을 쓰지만 그쪽 조회 함수는 내보내지 않는다 */
interface MirrorSessionTarget {
  sessionType: SessionType;
  sessionName: string;
  sshHost: string | null;
}

/**
 * 모바일이 보는 탭 하나. tmux window 또는 zellij tab에 대응한다.
 * 태블릿은 이 단위를 통째로 조립하고, 폰은 `panes`를 펼쳐 하나씩 보여 준다.
 */
export interface MirrorTab {
  id: string;
  name: string;
  panes: MirrorPane[];
}

export interface TaskSurfaces {
  taskId: string;
  sessionType: SessionType;
  tabs: MirrorTab[];
}

/** 세션을 비출 수 없는 이유. 화면이 빈 터미널 대신 이 사유를 보여 준다 */
export type SurfaceUnavailableReason = "no-session" | "session-not-running" | "zellij-too-old";

export class SurfaceUnavailableError extends Error {
  constructor(readonly reason: SurfaceUnavailableReason) {
    super(reason);
    this.name = "SurfaceUnavailableError";
  }
}

async function findMirrorSessionTarget(taskId: string): Promise<MirrorSessionTarget> {
  const taskRepo = await getTaskRepository();
  const task: KanbanTask | null = await taskRepo.findOneBy({ id: taskId });

  if (!task?.sessionType || !task.sessionName) {
    throw new SurfaceUnavailableError("no-session");
  }

  return { sessionType: task.sessionType, sessionName: task.sessionName, sshHost: task.sshHost };
}

function runMirrorCommand(command: string, sshHost: string | null): Promise<string> {
  return execGit(command, sshHost, { timeoutMs: MIRROR_COMMAND_TIMEOUT_MS });
}

/** zellij 버전은 세션이 사는 동안 바뀌지 않으므로 호스트별로 한 번만 확인한다 */
const zellijPaneIdSupportByHost = new Map<string, boolean>();

async function hasZellijPaneIdSupport(sshHost: string | null): Promise<boolean> {
  const hostKey = sshHost ?? "local";
  const cachedSupport = zellijPaneIdSupportByHost.get(hostKey);
  if (cachedSupport !== undefined) {
    return cachedSupport;
  }

  let isSupported = false;
  try {
    isSupported = supportsZellijTabIdCommands(await runMirrorCommand(buildZellijVersionCommand(), sshHost));
  } catch {
    /** 버전을 못 읽으면 pane 지정 명령이 없는 구버전으로 보고 비추지 않는다 */
  }

  zellijPaneIdSupportByHost.set(hostKey, isSupported);
  return isSupported;
}

/**
 * 태스크의 pane을 탭 단위로 묶어 돌려준다.
 * 폰은 이 결과의 pane을 전부 펼쳐 탭으로 쓰고, 태블릿은 탭 하나의 pane을 좌표대로 조립한다.
 */
export async function getTaskSurfaces(taskId: string): Promise<TaskSurfaces> {
  const target = await findMirrorSessionTarget(taskId);
  const panes = await listMirrorPanes(target);

  if (panes.length === 0) {
    throw new SurfaceUnavailableError("session-not-running");
  }

  return { taskId, sessionType: target.sessionType, tabs: groupPanesIntoTabs(panes) };
}

async function listMirrorPanes(target: MirrorSessionTarget): Promise<MirrorPane[]> {
  if (target.sessionType === SessionType.ZELLIJ) {
    if (!(await hasZellijPaneIdSupport(target.sshHost))) {
      throw new SurfaceUnavailableError("zellij-too-old");
    }
    const output = await runMirrorCommand(
      buildZellijListMirrorPanesCommand(target.sessionName),
      target.sshHost,
    );
    return parseZellijMirrorPaneList(output);
  }

  const output = await runMirrorCommand(
    buildTmuxListMirrorPanesCommand(target.sessionName),
    target.sshHost,
  );
  return parseTmuxMirrorPaneList(output);
}

/** pane 목록을 처음 나온 탭 순서대로 묶는다. 멀티플렉서가 이미 정렬해 주므로 다시 정렬하지 않는다 */
function groupPanesIntoTabs(panes: MirrorPane[]): MirrorTab[] {
  const tabsById = new Map<string, MirrorTab>();

  for (const pane of panes) {
    const existingTab = tabsById.get(pane.tabId);
    if (existingTab) {
      existingTab.panes.push(pane);
      continue;
    }
    tabsById.set(pane.tabId, { id: pane.tabId, name: pane.tabName, panes: [pane] });
  }

  return [...tabsById.values()];
}

/** 모바일 보드가 한 번에 받는 것. 데스크탑 보드와 같은 조회를 쓰므로 정렬과 done 페이지 크기가 같다 */
export async function getMobileBoard() {
  const [board, projects] = await Promise.all([getTasksByStatus(), getAllProjects()]);
  return { ...board, projects };
}

/** 화면에 띄울 페어링 코드를 발급한다 */
export function startMobilePairing(): { code: string; expiresAt: number } {
  return issuePairingCode();
}

/** 페어링 화면을 닫는다 */
export function stopMobilePairing(): void {
  revokePairingCode();
}

/** 화면에 떠 있는 코드. 데스크탑 UI가 다시 그릴 때 쓴다 */
export function readMobilePairingCode(): string | null {
  return readPendingPairingCode();
}

async function readPairedDevices(): Promise<PairedDevice[]> {
  return parsePairedDevices(await getAppSetting(PAIRED_DEVICES_KEY));
}

/**
 * 코드를 확인하고 기기 토큰을 발급한다.
 * 토큰은 만료시키지 않는다. 한 번 연결한 기기가 계속 쓸 수 있어야 한다는 것이 이 기능의 요구사항이다.
 */
export async function pairMobileDevice(submittedCode: string, deviceName: string): Promise<string | null> {
  if (!redeemPairingCode(submittedCode)) {
    return null;
  }

  const token = mintDeviceToken();
  const devices = await readPairedDevices();
  devices.push({ deviceId: mintDeviceId(), token, deviceName, pairedAt: new Date().toISOString() });
  await setAppSetting(PAIRED_DEVICES_KEY, serializePairedDevices(devices));

  return token;
}

/** 연결된 기기 목록. 토큰은 대조용이라 화면에 내보내지 않는다 */
export async function listPairedMobileDevices(): Promise<Omit<PairedDevice, "token">[]> {
  const devices = await readPairedDevices();
  return devices.map(({ deviceId, deviceName, pairedAt }) => ({ deviceId, deviceName, pairedAt }));
}

/** 기기 하나의 연결을 끊는다 */
export async function unpairMobileDevice(deviceId: string): Promise<void> {
  const devices = await readPairedDevices();
  await setAppSetting(
    PAIRED_DEVICES_KEY,
    serializePairedDevices(devices.filter((device) => device.deviceId !== deviceId)),
  );
}

/**
 * 요청이 연결된 기기에서 온 것인지 확인한다.
 * `/api/hooks/*`는 인증이 없지만 터미널을 읽고 쓰는 경로는 반드시 이 함수를 지나야 한다.
 */
export async function authorizeMobileRequest(authorizationHeader: string | undefined): Promise<boolean> {
  const devices = await readPairedDevices();
  return findPairedDevice(devices, parseBearerToken(authorizationHeader)) !== null;
}

/** pane 하나를 보고 있는 구독자들 */
interface PaneSubscription {
  listeners: Set<(chunk: string) => void>;
  stop: () => void;
}

/**
 * pane별 구독. tmux는 pane 하나에 파이프를 하나만 유지하므로 여기서 하나만 걸고 나눠 보낸다.
 * 두 번째 구독자가 파이프를 다시 걸면 `-o` 토글이 첫 번째 파이프를 꺼 버려 둘 다 조용해진다.
 *
 * 완성된 구독이 아니라 만들고 있는 약속을 담는다. 두 기기가 같은 pane을 동시에 열면
 * 완성값을 담을 경우 둘 다 "아직 없다"를 보고 파이프를 두 번 걸게 된다.
 */
const paneSubscriptions = new Map<string, Promise<PaneSubscription>>();

function buildSubscriptionKey(taskId: string, paneId: string): string {
  return `${taskId}#${paneId}`;
}

/**
 * 스트림은 오래 살기 때문에 `execGit`의 원격 경로를 쓰지 않는다.
 * 그쪽은 동시 실행 수를 제한하고 명령이 끝날 때까지 슬롯을 잡으므로, 끝나지 않는 명령을 태우면 슬롯이 영영 반납되지 않는다.
 */
async function spawnMirrorProcess(command: string, sshHost: string | null): Promise<ChildProcess> {
  /** 프로세스 그룹으로 띄워야 종료할 때 `tail`까지 함께 정리된다 */
  if (!sshHost) {
    return spawn("sh", ["-lc", command], { detached: true });
  }

  const hostConfig = (await parseSSHConfig()).find((config) => config.host === sshHost);
  if (!hostConfig) {
    throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
  }

  const args = [
    ...buildSSHArgs(hostConfig, { connectionHealth: getKanvibeSSHConnectionHealthOptions() }),
    `sh -lc ${quoteForPosixShell(command)}`,
  ];
  return spawn("ssh", args, { detached: true });
}

/**
 * pane 하나를 구독한다. 첫 화면을 한 번 보내고 그 뒤로는 바뀌는 부분만 흘려보낸다.
 * 반환값을 부르면 구독이 끝나고, 마지막 구독자가 나가면 서버 쪽 파이프와 폴링도 함께 멈춘다.
 */
export async function subscribeToPane(
  taskId: string,
  paneId: string,
  onChunk: (chunk: string) => void,
): Promise<() => void> {
  const target = await findMirrorSessionTarget(taskId);
  const key = buildSubscriptionKey(taskId, paneId);

  const snapshot = await readPaneSnapshot(target, paneId);
  onChunk(snapshot);

  let pendingSubscription = paneSubscriptions.get(key);
  if (!pendingSubscription) {
    pendingSubscription = startPaneSubscription(target, paneId);
    paneSubscriptions.set(key, pendingSubscription);
  }

  let subscription: PaneSubscription;
  try {
    subscription = await pendingSubscription;
  } catch (error) {
    /** 시작에 실패한 약속을 남겨 두면 다음 구독자도 같은 실패를 물려받는다 */
    paneSubscriptions.delete(key);
    throw error;
  }
  subscription.listeners.add(onChunk);

  return () => {
    subscription.listeners.delete(onChunk);
    if (subscription.listeners.size === 0) {
      subscription.stop();
      paneSubscriptions.delete(key);
    }
  };
}

async function readPaneSnapshot(target: MirrorSessionTarget, paneId: string): Promise<string> {
  const command = target.sessionType === SessionType.ZELLIJ
    ? buildZellijDumpPaneCommand(target.sessionName, paneId)
    : buildTmuxCapturePaneCommand(paneId);

  return runMirrorCommand(command, target.sshHost);
}

async function startPaneSubscription(
  target: MirrorSessionTarget,
  paneId: string,
): Promise<PaneSubscription> {
  const listeners = new Set<(chunk: string) => void>();
  const broadcast = (chunk: string) => listeners.forEach((listener) => listener(chunk));

  const stop = target.sessionType === SessionType.ZELLIJ
    ? startZellijDumpPolling(target, paneId, broadcast)
    : await startTmuxPaneStream(target, paneId, broadcast);

  return { listeners, stop };
}

async function startTmuxPaneStream(
  target: MirrorSessionTarget,
  paneId: string,
  broadcast: (chunk: string) => void,
): Promise<() => void> {
  const child = await spawnMirrorProcess(buildTmuxPaneStreamCommand(paneId), target.sshHost);
  child.stdout?.setEncoding("utf8");
  child.stdout?.on("data", broadcast);

  return () => {
    stopMirrorProcess(child);
    /**
     * 프로세스가 트랩을 실행하지 못하고 죽는 경우가 있어 파이프를 한 번 더 끊는다.
     * 파이프가 남으면 pane이 계속 임시 파일에 쓰고, 다음 구독자가 `-o` 토글에 걸려 아무것도 못 받는다.
     */
    void runMirrorCommand(buildTmuxCancelPipePaneCommand(paneId), target.sshHost).catch(() => {});
  };
}

function startZellijDumpPolling(
  target: MirrorSessionTarget,
  paneId: string,
  broadcast: (chunk: string) => void,
): () => void {
  const intervalMs = target.sshHost ? REMOTE_DUMP_INTERVAL_MS : LOCAL_DUMP_INTERVAL_MS;
  let lastDump: string | null = null;
  let isReading = false;

  const readOnce = async () => {
    /** 앞선 조회가 아직 안 끝났으면 건너뛴다. 느린 호스트에서 요청이 밀려 쌓이는 것을 막는다 */
    if (isReading) {
      return;
    }
    isReading = true;
    try {
      const dump = await readPaneSnapshot(target, paneId);
      if (dump !== lastDump) {
        lastDump = dump;
        broadcast(dump);
      }
    } catch {
      /** 세션이 사라졌을 수 있다. 소켓을 끊는 판단은 화면이 하도록 두고 여기서는 다음 주기를 기다린다 */
    } finally {
      isReading = false;
    }
  };

  const timer = setInterval(readOnce, intervalMs);
  return () => clearInterval(timer);
}

function stopMirrorProcess(child: ChildProcess): void {
  if (child.pid === undefined) {
    return;
  }

  try {
    /** 음수 pid는 프로세스 그룹 전체를 뜻한다. `sh`만 죽이면 `tail`이 남는다 */
    process.kill(-child.pid, "SIGTERM");
  } catch {
    child.kill("SIGTERM");
  }
}

/** 모바일에서 누른 키를 pane에 그대로 넣는다 */
export async function writeToPane(taskId: string, paneId: string, input: string): Promise<void> {
  const target = await findMirrorSessionTarget(taskId);

  const command = target.sessionType === SessionType.ZELLIJ
    ? buildZellijWritePaneCommand(target.sessionName, paneId, input)
    : buildTmuxSendKeysCommand(paneId, input);

  await runMirrorCommand(command, target.sshHost);
}
