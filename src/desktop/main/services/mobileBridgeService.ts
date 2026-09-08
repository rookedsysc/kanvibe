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
import { resolveZellijPaneIdSupport } from "@/lib/terminalTabs";
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

function hasZellijPaneIdSupport(sshHost: string | null): Promise<boolean> {
  return resolveZellijPaneIdSupport(sshHost, (command) => runMirrorCommand(command, sshHost));
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
 * 기기 목록을 바꾸는 호출을 한 줄로 세운다.
 *
 * 목록은 `app_settings`의 문자열 칸 하나라 읽고-고쳐-쓰는 사이에 다른 호출이 끼면 한쪽 변경이 통째로 사라진다.
 * 두 기기가 거의 같은 순간에 연결하면 토큰 하나가 없어져 나중에 401이 나고,
 * 한쪽을 끊는 동안 다른 쪽이 연결하면 끊은 기기가 되살아난다. 셀이 하나뿐이라 DB는 이것을 막아 주지 않는다.
 * 목록을 바꾸는 곳은 [pairMobileDevice], [unpairMobileDevice], [unpairMobileDeviceByToken]뿐이므로 경계도 이 파일이면 충분하다.
 */
let devicesWriteQueue: Promise<unknown> = Promise.resolve();

function withDeviceList<T>(mutate: (devices: PairedDevice[]) => Promise<T>): Promise<T> {
  const next = devicesWriteQueue.then(async () => mutate(await readPairedDevices()));
  /** 실패를 삼킨 약속을 줄에 남겨야 한 번의 오류가 뒤따르는 호출까지 막지 않는다 */
  devicesWriteQueue = next.catch(() => {});
  return next;
}

/**
 * 코드를 확인하고 기기 토큰을 발급한다.
 * 토큰은 만료시키지 않는다. 한 번 연결한 기기가 계속 쓸 수 있어야 한다는 것이 이 기능의 요구사항이다.
 */
export async function pairMobileDevice(submittedCode: string, deviceName: string): Promise<string | null> {
  if (!(await redeemPairingCode(submittedCode))) {
    return null;
  }

  const token = mintDeviceToken();
  await withDeviceList(async (devices) => {
    devices.push({ deviceId: mintDeviceId(), token, deviceName, pairedAt: new Date().toISOString() });
    await setAppSetting(PAIRED_DEVICES_KEY, serializePairedDevices(devices));
  });

  return token;
}

/** 연결된 기기 목록. 토큰은 대조용이라 화면에 내보내지 않는다 */
export async function listPairedMobileDevices(): Promise<Omit<PairedDevice, "token">[]> {
  const devices = await readPairedDevices();
  return devices.map(({ deviceId, deviceName, pairedAt }) => ({ deviceId, deviceName, pairedAt }));
}

/** 기기 하나의 연결을 끊는다 */
export async function unpairMobileDevice(deviceId: string): Promise<void> {
  await withDeviceList(async (devices) => {
    await setAppSetting(
      PAIRED_DEVICES_KEY,
      serializePairedDevices(devices.filter((device) => device.deviceId !== deviceId)),
    );
  });
}

/**
 * 요청에 실린 토큰의 주인만 목록에서 지운다. 모바일의 "연결 끊기"가 부르는 경로다.
 *
 * 기기가 자기 저장소만 비우면 데스크탑 항목은 그대로 남아 토큰이 계속 통과한다. 토큰은 만료되지 않으므로
 * 서버 쪽에도 지울 길이 없으면 그 기기는 영원히 인증된다. 그것을 닫는 것이 이 함수의 전부다.
 *
 * 지우는 대상은 요청이 증명한 기기 하나뿐이다. 토큰 하나로 남의 기기까지 끊을 수 있으면 없던 권한이 새로 생긴다.
 * 지운 기기가 있었는지를 돌려주어, 인증과 삭제 사이에 이미 끊긴 경우를 부르는 쪽이 구분할 수 있게 한다.
 */
export async function unpairMobileDeviceByToken(
  authorizationHeader: string | undefined,
): Promise<boolean> {
  const token = parseBearerToken(authorizationHeader);

  return withDeviceList(async (devices) => {
    const device = findPairedDevice(devices, token);
    if (!device) {
      return false;
    }

    await setAppSetting(
      PAIRED_DEVICES_KEY,
      serializePairedDevices(devices.filter((candidate) => candidate.deviceId !== device.deviceId)),
    );
    return true;
  });
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

/**
 * 구독자 수. 약속이 풀리기 전에 세는 것이 이 지도의 존재 이유다.
 *
 * `await`은 이미 풀린 약속이라도 마이크로태스크 경계라, 뒤에 온 구독자가 지도에서 약속을 집어 든 뒤
 * 콜백을 얹기 전까지 틈이 생긴다. 그 틈에 앞선 구독자가 나가면, 완성된 구독의 listener만 세는 정리 쪽에서는
 * 아직 아무도 없는 것으로 보여 스트림을 끊는다. 뒤에 온 구독자는 이미 멈춘 구독에 콜백을 얹고 영영 조용해진다.
 * 그래서 자리를 잡는 순간 동기적으로 세고, 그 수가 0이 될 때만 멈춘다.
 */
const paneSubscriberCounts = new Map<string, number>();

/**
 * pane이 이 태스크의 세션에 속한다고 확인된 키.
 *
 * tmux pane id는 서버 전역이라 `-t %17`이 세션 경계를 넘는다. 토큰 하나를 가진 기기가 다른 태스크의 pane은 물론
 * KanVibe가 만들지 않은 pane까지 읽고 쓸 수 있다는 뜻이다. zellij 쪽은 명령이 `--session`을 함께 받아 원래 막혀 있다.
 * 키 입력마다 pane 목록을 다시 조회하면 비싸므로, 구독이 붙어 있는 동안만 확인 결과를 기억한다.
 */
const verifiedPaneKeys = new Set<string>();

function buildSubscriptionKey(taskId: string, paneId: string): string {
  return `${taskId}#${paneId}`;
}

/** 이 pane이 태스크의 세션 안에 있는지 확인한다. 조회 명령은 `getTaskSurfaces`가 쓰는 것과 같다 */
async function ensurePaneBelongsToSession(
  target: MirrorSessionTarget,
  paneId: string,
): Promise<void> {
  const panes = await listMirrorPanes(target);
  if (!panes.some((pane) => pane.id === paneId)) {
    throw new SurfaceUnavailableError("session-not-running");
  }
}

/**
 * 구독자 하나를 놓아준다. 마지막 구독자였고 지도가 아직 이 호출이 집어 든 약속을 들고 있을 때만 멈춰야 한다.
 * 그 사이 새 구독자가 새 약속을 걸었다면 정리는 그쪽 몫이다.
 */
function releasePaneSubscriber(key: string, claimed: Promise<PaneSubscription>): boolean {
  const remaining = (paneSubscriberCounts.get(key) ?? 1) - 1;
  if (remaining > 0) {
    paneSubscriberCounts.set(key, remaining);
    return false;
  }

  paneSubscriberCounts.delete(key);
  verifiedPaneKeys.delete(key);
  if (paneSubscriptions.get(key) !== claimed) {
    return false;
  }

  paneSubscriptions.delete(key);
  return true;
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

  await ensurePaneBelongsToSession(target, paneId);

  const snapshot = await readPaneSnapshot(target, paneId);
  onChunk(snapshot);

  let pendingSubscription = paneSubscriptions.get(key);
  if (!pendingSubscription) {
    pendingSubscription = startPaneSubscription(target, paneId);
    paneSubscriptions.set(key, pendingSubscription);
  }
  /** 자리를 잡는 것과 수를 세는 것은 같은 동기 구간 안에서 끝나야 한다 */
  const claimedSubscription = pendingSubscription;
  paneSubscriberCounts.set(key, (paneSubscriberCounts.get(key) ?? 0) + 1);
  verifiedPaneKeys.add(key);

  let subscription: PaneSubscription;
  try {
    subscription = await claimedSubscription;
  } catch (error) {
    /** 시작에 실패한 약속을 남겨 두면 다음 구독자도 같은 실패를 물려받는다 */
    if (paneSubscriptions.get(key) === claimedSubscription) {
      paneSubscriptions.delete(key);
    }
    releasePaneSubscriber(key, claimedSubscription);
    throw error;
  }
  subscription.listeners.add(onChunk);

  let isReleased = false;
  return () => {
    /** 소켓이 닫히는 경로와 구독 실패 경로가 모두 이 함수를 부를 수 있어, 두 번 불려도 남의 몫을 깎지 않게 한다 */
    if (isReleased) {
      return;
    }
    isReleased = true;

    subscription.listeners.delete(onChunk);
    if (releasePaneSubscriber(key, claimedSubscription)) {
      subscription.stop();
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

  /**
   * 구독이 붙기 전에도 소켓은 메시지를 받을 수 있어(`mobileRoutes`가 `message`를 먼저 건다)
   * 기억해 둔 확인이 없으면 여기서 직접 확인한다.
   */
  if (!verifiedPaneKeys.has(buildSubscriptionKey(taskId, paneId))) {
    await ensurePaneBelongsToSession(target, paneId);
  }

  const command = target.sessionType === SessionType.ZELLIJ
    ? buildZellijWritePaneCommand(target.sessionName, paneId, input)
    : buildTmuxSendKeysCommand(paneId, input);

  await runMirrorCommand(command, target.sshHost);
}
