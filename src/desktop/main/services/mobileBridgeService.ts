import { spawn, type ChildProcess } from "child_process";
import { getAppSetting, getDefaultSessionType, setAppSetting } from "@/desktop/main/services/appSettingsService";
import { createTask, getTasksByStatus, updateTaskStatus } from "@/desktop/main/services/kanbanService";
import { getAllProjects, getProjectBranches } from "@/desktop/main/services/projectService";
import { attachTaskTerminal, CallbackTerminalClient } from "@/desktop/main/terminalBridge";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
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
import { resolveZellijIdTargetingSupport } from "@/lib/terminalTabs";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig } from "@/lib/sshConfig";
import {
  attachClientToTerminalTab,
  getTerminalTabSize,
  listLocalTerminalTabs,
  writeTerminalTabInput,
} from "@/lib/terminal";
import { isSessionAlive, quoteForPosixShell } from "@/lib/worktree";

/**
 * 모바일 클라이언트가 데스크탑을 읽고, 터미널 pane을 비추고, 태스크를 만들거나 옮기는 경로.
 *
 * 비추는 쪽은 데스크탑 화면을 바꾸지 않는다. pane을 고르거나 크기를 바꾸는 명령은 하나도 부르지 않고,
 * 스냅샷과 출력 스트림만 가져다 나눠 보낸다. 근거는 `paneMirror.ts` 머리말에 적어 두었다.
 *
 * 데스크탑을 바꾸는 것은 [createMobileTask]와 [updateMobileTaskStatus], 그리고 세션이 아직 없는 태스크를 열 때
 * 세션을 띄우는 일이다. 앞의 둘은 데스크탑 화면이 누르는 것과 같은 `kanbanService` 경로를, 세션은 데스크탑이 태스크를
 * 열 때와 같은 [attachTaskTerminal]을 지난다. 페어링한 기기가 이 머신에 셸 세션을 열 수 있다는 뜻이라
 * 이것들은 [authorizeMobileRequest] 뒤에만 놓이고, 화면에 없는 칸은 받지 않는다.
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

/**
 * 세션을 띄운 뒤 살아날 때까지 기다리는 상한과 확인 간격.
 * zellij와 원격 tmux는 PTY 안에서 비동기로 만들어져 붙는 것만으로는 다 됐는지 알 수 없다.
 */
const SESSION_START_TIMEOUT_MS = 15_000;
const SESSION_START_POLL_INTERVAL_MS = 500;

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
export type SurfaceUnavailableReason = "no-session" | "session-not-running" | "zellij-too-old" | "remote-unreachable";

export class SurfaceUnavailableError extends Error {
  constructor(readonly reason: SurfaceUnavailableReason) {
    super(reason);
    this.name = "SurfaceUnavailableError";
  }
}

/** 세션이 지정된 태스크. 세션이 없으면 비출 것도 띄울 것도 없다 */
async function findMirrorTask(taskId: string): Promise<KanbanTask> {
  const taskRepo = await getTaskRepository();
  const task: KanbanTask | null = await taskRepo.findOneBy({ id: taskId });

  if (!task?.sessionType || !task.sessionName) {
    throw new SurfaceUnavailableError("no-session");
  }

  return task;
}

function toMirrorSessionTarget(task: KanbanTask): MirrorSessionTarget {
  return { sessionType: task.sessionType as SessionType, sessionName: task.sessionName as string, sshHost: task.sshHost };
}

async function findMirrorSessionTarget(taskId: string): Promise<MirrorSessionTarget> {
  return toMirrorSessionTarget(await findMirrorTask(taskId));
}

function runMirrorCommand(command: string, sshHost: string | null): Promise<string> {
  return execGit(command, sshHost, { timeoutMs: MIRROR_COMMAND_TIMEOUT_MS });
}

function hasZellijPaneIdSupport(sshHost: string | null): Promise<boolean> {
  return resolveZellijIdTargetingSupport(sshHost, (command) => runMirrorCommand(command, sshHost));
}

/**
 * 태스크의 pane을 탭 단위로 묶어 돌려준다.
 * 폰은 이 결과의 pane을 전부 펼쳐 탭으로 쓰고, 태블릿은 탭 하나의 pane을 좌표대로 조립한다.
 */
export async function getTaskSurfaces(taskId: string): Promise<TaskSurfaces> {
  const task = await findMirrorTask(taskId);
  const target = toMirrorSessionTarget(task);

  if (target.sshHost) {
    await ensureRemoteHostReachable(target.sshHost);
  }

  if (target.sessionType === SessionType.TERMINAL) {
    return { taskId, sessionType: target.sessionType, tabs: listTerminalMirrorTabs(taskId) };
  }

  await startMultiplexerSessionIfMissing(task, target);
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

/**
 * 원격 호스트에 닿는지 먼저 본다.
 *
 * 세션 확인(`isSessionAlive`)은 연결 실패도 "세션 없음"으로 접는다. 그대로 두면 닿지도 않는 호스트에 세션을 띄우려다
 * 실패하고, 화면은 세션이 꺼져 있다고 안내한다. 사용자가 할 일은 세션이 아니라 네트워크나 SSH 설정을 보는 것이라
 * 사유를 따로 둔다. `true`는 연결되면 실패할 수 없는 명령이라, 실패는 곧 연결 실패다.
 */
async function ensureRemoteHostReachable(sshHost: string): Promise<void> {
  try {
    await runMirrorCommand("true", sshHost);
  } catch (error) {
    console.error("[kanvibe] Mobile remote host unreachable:", error);
    throw new SurfaceUnavailableError("remote-unreachable");
  }
}

/**
 * 세션이 아직 없으면 데스크탑이 태스크를 열 때와 같은 길로 띄운다.
 *
 * tmux와 zellij 세션은 데스크탑이 터미널을 붙일 때 생기므로, 모바일에서 만들었거나 데스크탑에서 한 번도 열지 않은
 * 태스크에는 비출 세션이 없다. 화면 없는 클라이언트로 붙어 세션을 만들고 살아난 것을 확인한 뒤 떨어진다.
 * 멀티플렉서 세션은 클라이언트가 떨어져도 남고, 데스크탑 화면은 붙어 있지 않았으니 바뀌지 않는다.
 */
async function startMultiplexerSessionIfMissing(task: KanbanTask, target: MirrorSessionTarget): Promise<void> {
  if (await isSessionAlive(target.sessionType, target.sessionName, target.sshHost)) {
    return;
  }

  const startupClient = new CallbackTerminalClient(() => {});
  try {
    await attachTaskTerminal(task, null, startupClient);
    await waitUntilSessionAlive(target);
  } catch (error) {
    console.error("[kanvibe] Mobile session start failed:", error);
    throw new SurfaceUnavailableError("session-not-running");
  } finally {
    startupClient.close();
  }
}

async function waitUntilSessionAlive(target: MirrorSessionTarget): Promise<void> {
  const deadline = Date.now() + SESSION_START_TIMEOUT_MS;

  while (!(await isSessionAlive(target.sessionType, target.sessionName, target.sshHost))) {
    if (Date.now() >= deadline) {
      throw new Error(`세션이 ${SESSION_START_TIMEOUT_MS}ms 안에 살아나지 않았습니다: ${target.sessionName}`);
    }
    await new Promise((resolve) => setTimeout(resolve, SESSION_START_POLL_INTERVAL_MS));
  }
}

/**
 * terminal 세션의 탭 목록. 멀티플렉서가 없어 탭 하나가 PTY 하나, 즉 pane 하나다.
 * pane 식별자로 탭 식별자를 그대로 쓴다.
 */
function listTerminalMirrorTabs(taskId: string): MirrorTab[] {
  return listLocalTerminalTabs(taskId).map((tab) => {
    const { cols, rows } = getTerminalTabSize(taskId, tab.id);
    const pane: MirrorPane = {
      id: tab.id,
      tabId: tab.id,
      tabName: tab.name,
      command: "",
      left: 0,
      top: 0,
      width: cols,
      height: rows,
    };
    return { id: tab.id, name: tab.name, panes: [pane] };
  });
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
  const [board, projects, defaultSessionType] = await Promise.all([
    getTasksByStatus(),
    getAllProjects(),
    getDefaultSessionType(),
  ]);
  return { ...board, projects, defaultSessionType };
}

/**
 * 모바일이 보낸 값이 데스크탑 열거형에 없을 때.
 *
 * 서버 오류가 아니라 요청이 잘못된 것이라 라우팅이 이름을 보고 400으로 옮긴다.
 * 이름으로만 가려내므로 클래스 자체는 이 파일 밖으로 내보내지 않는다 — `serviceRegistry`가 이 모듈을 통째로
 * IPC에 얹기 때문에, 내보내면 화면이 부를 일이 없는 값이 그 표면에 하나 더 얹힌다.
 * 모르는 세션 타입을 조용히 버리면 [createTask]가 worktree 없이 이름만 있는 태스크를 만들어,
 * 기기에는 성공으로 보이고 데스크탑에는 열 수 없는 항목이 남는다.
 */
class MobileTaskInputError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "MobileTaskInputError";
  }
}

/** 모바일이 보낸 문자열을 데스크탑 열거값으로 옮긴다. 비어 있으면 고르지 않은 것이고, 모르는 값이면 잘못된 요청이다 */
function readEnumValue<T extends string>(
  allowedValues: readonly T[],
  submitted: string | undefined,
  fieldLabel: string
): T | undefined {
  if (!submitted) {
    return undefined;
  }

  const matched = allowedValues.find((value) => value === submitted);
  if (!matched) {
    throw new MobileTaskInputError(`${fieldLabel} 값이 올바르지 않습니다`);
  }
  return matched;
}

/**
 * 브랜치 이름으로 받아들일 모양.
 *
 * 이 값은 `createWorktreeWithSession`에서 `git ... -b "<이름>"` 꼴로 셸 명령 문자열에 끼워지고,
 * worktree 경로에도 그대로 들어간다. 큰따옴표 안이라도 셸은 `$(...)`와 백틱을 펼치므로,
 * 데스크탑 화면에서만 오던 값과 달리 네트워크에서 오는 값은 여기서 막지 않으면
 * 페어링한 기기가 이 머신에서 임의의 명령을 돌리고 프로젝트 밖 경로를 짚을 수 있다.
 * git이 받아 주는 이름 중 셸과 경로에 뜻이 없는 문자만 통과시킨다.
 */
const SAFE_GIT_REF_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._/-]*$/;

function readGitRef(submitted: string, fieldLabel: string): string {
  if (!SAFE_GIT_REF_PATTERN.test(submitted) || submitted.includes("..")) {
    throw new MobileTaskInputError(`${fieldLabel}에 쓸 수 없는 문자가 있습니다`);
  }
  return submitted;
}

/** 태스크 생성 화면의 베이스 브랜치 칸을 채운다 */
export async function getMobileProjectBranches(projectId: string): Promise<string[]> {
  return getProjectBranches(projectId);
}

/**
 * 모바일 생성 화면이 보내는 태스크 초안.
 *
 * 데스크탑 `CreateTaskInput`을 그대로 열지 않는다. 그쪽에는 모바일 화면에 없는 `sshHost`가 있어,
 * 열어 두면 화면에 없는 경로로 임의의 호스트에 세션을 여는 요청을 만들 수 있다. 화면에 있는 칸만 받는다.
 */
export interface MobileTaskDraft {
  projectId: string;
  branchName: string;
  baseBranch?: string;
  description?: string;
  priority?: string;
  sessionType?: string;
}

/**
 * 모바일에서 태스크를 만든다.
 *
 * 제목을 따로 받지 않고 브랜치 이름을 그대로 쓰는 것은 데스크탑 생성 화면과 같다.
 * 두 화면이 다른 규칙으로 제목을 정하면 같은 보드에서 어느 쪽에서 만들었는지에 따라 제목 모양이 갈린다.
 */
export async function createMobileTask(draft: MobileTaskDraft): Promise<KanbanTask> {
  return createTask({
    title: draft.branchName,
    branchName: readGitRef(draft.branchName, "브랜치 이름"),
    projectId: draft.projectId,
    baseBranch: draft.baseBranch ? readGitRef(draft.baseBranch, "베이스 브랜치") : undefined,
    description: draft.description,
    priority: readEnumValue(Object.values(TaskPriority), draft.priority, "우선순위"),
    sessionType: readEnumValue(Object.values(SessionType), draft.sessionType, "세션 타입"),
  });
}

/** 모바일에서 태스크를 다른 상태로 옮긴다. 없는 태스크면 null이 나와 라우팅이 404로 옮긴다 */
export async function updateMobileTaskStatus(taskId: string, submittedStatus: string): Promise<KanbanTask | null> {
  const status = readEnumValue(Object.values(TaskStatus), submittedStatus, "상태");
  if (!status) {
    throw new MobileTaskInputError("상태 값이 올바르지 않습니다");
  }

  return updateTaskStatus(taskId, status);
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

/** 기기 하나의 연결을 끊는다. 설정 화면이 부르는 경로다 */
export async function unpairMobileDevice(deviceId: string): Promise<void> {
  const wasRemoved = await withDeviceList(async (devices) => {
    const remaining = devices.filter((device) => device.deviceId !== deviceId);
    await setAppSetting(PAIRED_DEVICES_KEY, serializePairedDevices(remaining));
    return remaining.length !== devices.length;
  });

  /** 이미 없던 기기까지 알리면 듣는 쪽이 남의 소켓을 닫을 근거로 삼을 수 있어, 정말 지운 경우에만 알린다 */
  if (wasRemoved) {
    notifyMobileDeviceUnpaired(deviceId);
  }
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

  const removedDeviceId = await withDeviceList(async (devices) => {
    const device = findPairedDevice(devices, token);
    if (!device) {
      return null;
    }

    await setAppSetting(
      PAIRED_DEVICES_KEY,
      serializePairedDevices(devices.filter((candidate) => candidate.deviceId !== device.deviceId)),
    );
    return device.deviceId;
  });

  if (removedDeviceId !== null) {
    notifyMobileDeviceUnpaired(removedDeviceId);
  }

  return removedDeviceId !== null;
}

type MobileDeviceUnpairedListener = (deviceId: string) => void;

const mobileDeviceUnpairedListeners = new Set<MobileDeviceUnpairedListener>();

/**
 * 기기 연결이 끊겼다는 것을 알려 달라고 등록한다. 돌려주는 함수를 부르면 등록이 풀린다.
 *
 * 목록에서 지워도 그 기기가 이미 열어 둔 pane 스트림은 살아 있어 터미널을 계속 비춘다.
 * 토큰을 지우는 것만으로는 다음 요청부터 막힐 뿐이라, 연결을 끊었다는 말이 열려 있는 화면 앞에서 거짓이 된다.
 * 소켓은 `mobileRoutes`가 들고 있고 이 서비스는 네트워크 계층을 알지 않기로 되어 있으므로,
 * 여기서는 지운 기기만 알리고 닫는 일은 소켓을 가진 쪽에 맡긴다.
 */
export function onMobileDeviceUnpaired(listener: MobileDeviceUnpairedListener): () => void {
  mobileDeviceUnpairedListeners.add(listener);
  return () => {
    mobileDeviceUnpairedListeners.delete(listener);
  };
}

/** 알림은 [withDeviceList] 줄 밖에서 돈다. 듣는 쪽이 오래 걸려도 다음 목록 변경이 그만큼 밀리면 안 된다 */
function notifyMobileDeviceUnpaired(deviceId: string): void {
  mobileDeviceUnpairedListeners.forEach((listener) => listener(deviceId));
}

/**
 * 요청이 연결된 기기에서 온 것인지 확인하고, 통과했으면 그 기기의 식별자를 돌려준다. 아니면 null.
 * `/api/hooks/*`는 인증이 없지만 터미널을 읽고 쓰는 경로는 반드시 이 함수를 지나야 한다.
 *
 * 통과 여부만 돌려주면 스트림을 연 소켓이 누구 것인지 알 길이 없어, 기기 하나를 끊어도 그 소켓만 골라 닫지 못한다.
 * 토큰의 주인을 찾는 일은 이미 여기서 하고 있으므로 그 결과를 버리지 않고 그대로 내보낸다.
 */
export async function authorizeMobileRequest(
  authorizationHeader: string | undefined,
): Promise<string | null> {
  const devices = await readPairedDevices();
  return findPairedDevice(devices, parseBearerToken(authorizationHeader))?.deviceId ?? null;
}

/**
 * pane 하나를 보고 있는 구독자들.
 *
 * 출력과 별개로 종료를 따로 알린다. 서버 쪽 스트림이 스스로 죽으면 화면은 아무것도 받지 못한 채
 * 멈춘 터미널을 계속 들고 있어, 사용자에게는 조용한 pane과 살아 있는 pane이 똑같이 보인다.
 */
interface PaneSubscription {
  listeners: Set<(chunk: string) => void>;
  endListeners: Set<() => void>;
  stop: () => void;
}

/**
 * pane 하나에 걸린 구독과 그것을 보고 있는 구독자 수.
 *
 * 수를 따로 둔 지도가 아니라 이 객체에 담는 이유는, 스트림이 죽어 구독이 갈리는 순간에 있다.
 * 키로 세면 앞선 구독자의 퇴장이 그 뒤에 새로 생긴 구독의 수를 깎아 아무도 안 보는 것으로 만든다.
 *
 * 완성된 구독이 아니라 만들고 있는 약속을 담는다. 두 기기가 같은 pane을 동시에 열면
 * 완성값을 담을 경우 둘 다 "아직 없다"를 보고 파이프를 두 번 걸게 된다.
 */
interface PaneClaim {
  subscription: Promise<PaneSubscription>;
  subscriberCount: number;
}

/**
 * pane별 구독. tmux는 pane 하나에 파이프를 하나만 유지하므로 여기서 하나만 걸고 나눠 보낸다.
 * 두 번째 구독자가 파이프를 다시 걸면 `-o` 토글이 첫 번째 파이프를 꺼 버려 둘 다 조용해진다.
 *
 * 이 지도에 자리가 있다는 것은 그 pane이 태스크의 세션에 속한다고 이미 확인됐다는 뜻이기도 하다.
 * tmux pane id는 서버 전역이라 `-t %17`이 세션 경계를 넘어, 토큰 하나를 가진 기기가 다른 태스크의 pane은 물론
 * KanVibe가 만들지 않은 pane까지 읽고 쓸 수 있다. zellij 쪽은 명령이 `--session`을 함께 받아 원래 막혀 있다.
 * 키 입력마다 pane 목록을 다시 조회하면 비싸므로 [writeToPane]은 이 자리의 유무로 확인을 대신한다.
 */
const paneSubscriptions = new Map<string, PaneClaim>();

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
 * pane 구독 자리를 잡고 구독자 수를 하나 올린다. [paneSubscriptions]를 건드리는 곳은 여기와 [forgetPaneClaim]뿐이다.
 *
 * 자리를 잡는 것과 수를 세는 것이 같은 동기 구간 안에서 끝나야 한다. `await`은 이미 풀린 약속이라도
 * 마이크로태스크 경계라, 뒤에 온 구독자가 약속을 집어 든 뒤 콜백을 얹기 전까지 틈이 생긴다.
 * 그 틈에 앞선 구독자가 나가면 정리 쪽에서는 아직 아무도 없는 것으로 보여 스트림을 끊고,
 * 뒤에 온 구독자는 이미 멈춘 구독에 콜백을 얹어 영영 조용해진다.
 *
 * 돌려주는 `release`는 마지막 구독자였고 지도가 아직 이 호출이 집어 든 자리를 들고 있을 때만 참이다.
 * 그 사이 새 구독자가 새 자리를 잡았다면 정리는 그쪽 몫이다.
 */
function claimPaneSubscription(
  key: string,
  create: (forgetSelf: () => void) => Promise<PaneSubscription>,
): { claimed: Promise<PaneSubscription>; release: () => boolean } {
  let claim = paneSubscriptions.get(key);
  if (!claim) {
    /** 시작하는 도중에 스트림이 죽어도 자기 자리를 정확히 거둘 수 있도록, 만드는 쪽에 그 길을 함께 넘긴다 */
    const created: PaneClaim = {
      subscription: create(() => forgetPaneClaim(key, created)),
      subscriberCount: 0,
    };
    /** 시작에 실패한 약속을 남겨 두면 다음 구독자도 같은 실패를 물려받는다 */
    void created.subscription.catch(() => forgetPaneClaim(key, created));
    paneSubscriptions.set(key, created);
    claim = created;
  }

  const claimed = claim;
  claimed.subscriberCount += 1;

  return {
    claimed: claimed.subscription,
    release: () => {
      claimed.subscriberCount -= 1;
      if (claimed.subscriberCount > 0) {
        return false;
      }
      return forgetPaneClaim(key, claimed);
    },
  };
}

/** 이 자리가 아직 지도에 있을 때만 지운다. 이미 다른 자리로 갈렸다면 그쪽을 건드리면 안 된다 */
function forgetPaneClaim(key: string, claim: PaneClaim): boolean {
  if (paneSubscriptions.get(key) !== claim) {
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
 *
 * [onEnd]는 서버 쪽 스트림이 스스로 죽었을 때만 돈다. 구독자가 [subscribeToPane]을 부른 뒤 나가는 경우가 아니라,
 * `ssh`가 끊기거나 `tmux pipe-pane`이 실패해 더 이상 아무것도 오지 않게 된 경우를 뜻한다.
 */
export async function subscribeToPane(
  taskId: string,
  paneId: string,
  onChunk: (chunk: string) => void,
  onEnd: () => void,
): Promise<() => void> {
  const task = await findMirrorTask(taskId);
  if (task.sessionType === SessionType.TERMINAL) {
    return subscribeToTerminalTab(task, paneId, onChunk, onEnd);
  }

  const target = toMirrorSessionTarget(task);
  const key = buildSubscriptionKey(taskId, paneId);

  await ensurePaneBelongsToSession(target, paneId);

  const snapshot = await readPaneSnapshot(target, paneId);
  onChunk(snapshot);

  const { claimed, release } = claimPaneSubscription(key, (forgetSelf) =>
    startPaneSubscription(target, paneId, forgetSelf),
  );

  let subscription: PaneSubscription;
  try {
    subscription = await claimed;
  } catch (error) {
    release();
    throw error;
  }
  subscription.listeners.add(onChunk);
  subscription.endListeners.add(onEnd);

  let isReleased = false;
  return () => {
    /** 소켓이 닫히는 경로와 구독 실패 경로가 모두 이 함수를 부를 수 있어, 두 번 불려도 남의 몫을 깎지 않게 한다 */
    if (isReleased) {
      return;
    }
    isReleased = true;

    subscription.listeners.delete(onChunk);
    subscription.endListeners.delete(onEnd);
    if (release()) {
      subscription.stop();
    }
  };
}

/**
 * terminal 세션 탭 하나를 구독한다. 멀티플렉서가 없으니 KanVibe가 들고 있는 PTY에 직접 붙는다.
 *
 * PTY가 이미 있으면 지금 화면을 먼저 받고, 없으면 데스크탑이 탭을 열 때와 같은 길로 만든다.
 * 이 클라이언트는 크기 변경을 보내지 않으므로 데스크탑 화면의 크기는 바뀌지 않는다.
 * [onEnd]는 PTY가 스스로 끝났을 때만 돈다.
 */
async function subscribeToTerminalTab(
  task: KanbanTask,
  tabId: string,
  onChunk: (chunk: string) => void,
  onEnd: () => void,
): Promise<() => void> {
  if (!listLocalTerminalTabs(task.id).some((tab) => tab.id === tabId)) {
    throw new SurfaceUnavailableError("session-not-running");
  }

  let isReleased = false;
  /** 붙기 전에 닫힌 것은 끝이 아니라 실패다. 실패는 던지는 쪽이 알리므로 끝까지 함께 알리면 사유가 덮인다 */
  let isAttached = false;
  const client = new CallbackTerminalClient(onChunk);
  client.once("close", () => {
    if (isAttached && !isReleased) {
      onEnd();
    }
  });
  const release = () => {
    if (isReleased) {
      return;
    }
    isReleased = true;
    client.close();
  };

  try {
    client.holdOutput();
    const screenUntilNow = attachClientToTerminalTab(task.id, tabId, client as never);
    if (screenUntilNow) {
      client.releaseHeldOutput(await screenUntilNow);
    } else {
      client.releaseHeldOutput("");
      await attachTaskTerminal(task, tabId, client);
    }

    /** 화면을 기다리는 사이 셸이 끝났으면 붙을 PTY가 없다 */
    if (client.readyState !== client.OPEN) {
      throw new SurfaceUnavailableError("session-not-running");
    }
    isAttached = true;
    return release;
  } catch (error) {
    release();
    throw error;
  }
}

/**
 * 앱을 끄기 전에 열려 있는 미러를 전부 멈춘다.
 *
 * [spawnMirrorProcess]는 자식을 `detached`로 띄우므로 부모가 죽어도 함께 죽지 않는다. 아무도 멈추지 않으면
 * 고아가 된 `sh`/`ssh`와 `tail`이 남고, 사용자의 pane에는 `pipe-pane`이 걸린 채로 남아 임시 파일이 계속 자란다.
 * 이 서비스가 데스크탑 화면을 바꾸지 않는다는 약속은 앱이 사라진 뒤에도 지켜져야 한다.
 */
export function stopAllPaneMirrors(): void {
  for (const [key, claim] of [...paneSubscriptions]) {
    forgetPaneClaim(key, claim);
    void claim.subscription.then((subscription) => subscription.stop()).catch(() => {});
  }
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
  forgetSelf: () => void,
): Promise<PaneSubscription> {
  const listeners = new Set<(chunk: string) => void>();
  const endListeners = new Set<() => void>();
  const broadcast = (chunk: string) => listeners.forEach((listener) => listener(chunk));

  /**
   * 서버 쪽 스트림이 스스로 죽었다. 자리를 거둬야 다음 구독자가 죽은 구독을 물려받지 않고,
   * 지금 보고 있는 기기에는 끝났다고 알려야 화면이 멈춘 터미널을 살아 있는 것으로 착각하지 않는다.
   */
  const handleStreamDeath = () => {
    forgetSelf();

    const ending = [...endListeners];
    listeners.clear();
    endListeners.clear();
    ending.forEach((listener) => listener());
  };

  const stop = target.sessionType === SessionType.ZELLIJ
    ? startZellijDumpPolling(target, paneId, broadcast)
    : await startTmuxPaneStream(target, paneId, broadcast, handleStreamDeath);

  return { listeners, endListeners, stop };
}

async function startTmuxPaneStream(
  target: MirrorSessionTarget,
  paneId: string,
  broadcast: (chunk: string) => void,
  onDead: () => void,
): Promise<() => void> {
  const child = await spawnMirrorProcess(buildTmuxPaneStreamCommand(paneId), target.sshHost);
  child.stdout?.setEncoding("utf8");
  child.stdout?.on("data", broadcast);
  /** 읽지 않는 파이프는 64KB에서 차고, 그때부터 자식이 쓰기에서 멈춰 스트림째 선다 */
  child.stderr?.resume();

  /**
   * `ChildProcess`는 EventEmitter라 리스너 없는 `error`가 그대로 던져지고, Electron main에는 그것을 받을 곳이 없다.
   * `ssh` 바이너리가 없거나 프로세스 수 한도에 걸리는 것만으로 모바일에서 pane 하나를 여는 것이 앱 전체를 끈다.
   * 종료도 함께 받아야 하는데, 자식이 죽어도 자리가 남으면 다음 구독자가 죽은 구독을 집어 들고 영영 조용해지기 때문이다.
   */
  child.on("error", (error) => {
    console.error("[kanvibe] Mirror stream failed:", error);
    onDead();
  });
  child.on("exit", onDead);

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

  if (target.sessionType === SessionType.TERMINAL) {
    if (!writeTerminalTabInput(taskId, paneId, input)) {
      throw new SurfaceUnavailableError("session-not-running");
    }
    return;
  }

  /**
   * 구독이 붙기 전에도 소켓은 메시지를 받을 수 있어(`mobileRoutes`가 `message`를 먼저 건다)
   * 기억해 둔 확인이 없으면 여기서 직접 확인한다.
   */
  if (!paneSubscriptions.has(buildSubscriptionKey(taskId, paneId))) {
    await ensurePaneBelongsToSession(target, paneId);
  }

  const command = target.sessionType === SessionType.ZELLIJ
    ? buildZellijWritePaneCommand(target.sessionName, paneId, input)
    : buildTmuxSendKeysCommand(paneId, input);

  await runMirrorCommand(command, target.sshHost);
}
