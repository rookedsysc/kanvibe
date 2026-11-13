import { spawn, type ChildProcess } from "child_process";
import { access, stat } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { getAppSetting, getDefaultSessionType, getLanPlaintextAllowed, getNotificationSettings, setAppSetting, setLanPlaintextAllowed, setNotificationEnabled, setNotificationStatuses } from "@/desktop/main/services/appSettingsService";
import { getMobileTlsPort } from "@/lib/hookEndpoint";
import {
  changeTaskSessionType,
  connectTerminalSession,
  createTask,
  deleteTaskAcrossTargets,
  getTaskById,
  getTasksByStatus,
  isTaskSessionAlive,
  removeTaskRegistration,
  updateTaskStatus,
} from "@/desktop/main/services/kanbanService";
import { subscribeToBoardEvents } from "@/lib/boardNotifier";
import { getAllProjects, getProjectBranches, getTaskAiSessions, getTaskAiSessionDetail, getTaskAiSessionAttachment, listSubdirectories, scanAndRegisterProjects } from "@/desktop/main/services/projectService";
import { isAiSessionProvider } from "@/lib/aiSessions/providers";
import type { AiMessageRole, AiSessionProvider } from "@/lib/aiSessions/types";
import { attachTaskTerminal, CallbackTerminalClient } from "@/desktop/main/terminalBridge";
import { createTerminalTab } from "@/desktop/main/services/terminalTabService";
import { propagateTaskRegistrationToPeers } from "@/desktop/main/services/taskTargetPropagationService";
import { listNotifications, markNotificationRead, markAllNotificationsRead, subscribeToNotificationChanges } from "@/desktop/main/notificationStore";
import type { AppNotification } from "@/desktop/shared/notifications";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import { getProjectRepository, getTaskRepository } from "@/lib/database";
import { execGit } from "@/lib/gitOperations";
import { isSafeGitRef } from "@/lib/gitRef";
import {
  findPairedDevice,
  issuePairingCode,
  isMobilePushSubscription,
  mintDeviceId,
  mintDeviceToken,
  parseBearerToken,
  parsePairedDevices,
  readPendingPairingCode,
  redeemPairingCode,
  revokePairingCode,
  serializePairedDevices,
  type PairedDevice,
  type MobilePushSubscription,
} from "@/lib/mobilePairing";
import {
  buildHerdrClosePaneCommand,
  buildHerdrControlPaneCommand,
  buildHerdrObservePaneCommand,
  buildHerdrPaneShortcutCommand,
  buildHerdrRecentScreenCommand,
  buildHerdrWritePaneCommand,
  buildScreenRedraw,
  buildTmuxClosePaneCommand,
  buildTmuxPaneShortcutCommand,
  buildZellijClosePaneCommand,
  buildZellijPaneShortcutCommand,
  buildTmuxCancelPipePaneCommand,
  buildTmuxCapturePaneCommand,
  buildTmuxPaneStateCommand,
  buildTmuxListMirrorPanesCommand,
  buildTmuxPaneStreamCommand,
  buildTmuxReadPaneGeometryCommand,
  buildTmuxReadViewportCommand,
  buildTmuxResizeViewportCommand,
  buildTmuxRestoreViewportCommand,
  buildTmuxSendKeysCommand,
  buildZellijDumpPaneCommand,
  buildZellijListMirrorPanesCommand,
  buildZellijReadViewportCommand,
  buildZellijSelectTabCommand,
  buildZellijResizeFloatingPaneCommand,
  buildZellijWritePaneCommand,
  parseHerdrObserveRecord,
  parseTmuxPaneState,
  parseTmuxMirrorPaneList,
  parseTmuxViewportState,
  parseZellijMirrorPaneList,
  parseZellijViewportState,
  type MirrorPane,
} from "@/lib/paneMirror";
import { resolveZellijIdTargetingSupport } from "@/lib/terminalTabs";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig } from "@/lib/sshConfig";
import {
  attachClientToTerminalTab,
  closeLocalTerminalTab,
  getLocalTerminalTabProcess,
  getTerminalTabSize,
  getTaskTerminalSize,
  listLocalTerminalTabs,
  releaseTerminalTabViewport,
  resizeTerminalTab,
  resizeTaskTerminalViewport,
  releaseTaskTerminalViewport,
  writeTerminalTabInput,
} from "@/lib/terminal";
import { listHerdrMirrorPanes, readHerdrPaneExtent } from "@/lib/herdr";
import { getSessionDependencyStatus } from "@/lib/remoteSessionDependency";
import { isSessionAlive, quoteForPosixShell } from "@/lib/worktree";

/**
 * 모바일 클라이언트가 데스크탑을 읽고, 터미널 pane을 비추고, 태스크를 만들거나 옮기는 경로.
 *
 * 스냅샷과 출력 스트림 조회는 포커스를 바꾸지 않는다. 크기 조정을 지원하는 세션은
 * [claimPaneViewport]가 모바일 연결 수명 동안 크기를 소유하고 마지막 연결이 나가면 복원한다.
 *
 * 태스크와 터미널을 바꾸는 것은 [createMobileTask]와 [updateMobileTaskStatus], [changeMobileTaskSessionType], [deleteMobileTask], [createMobileTerminalTab], [closeMobilePane], 그리고 세션이 아직 없는 태스크를 열 때
 * 세션을 띄우는 일이다. 태스크 변경은 데스크탑 화면과 같은 `kanbanService`, 탭 생성은 `terminalTabService`를 거치고, 세션은 데스크탑이 태스크를
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
const MAX_CHAT_OBSERVE_FRAME_BYTES = 1024 * 1024;
const VIEWPORT_SETTLE_POLL_INTERVAL_MS = 50;

/**
 * zellij에는 tmux `pipe-pane`에 해당하는 출력 스트림이 없어 화면을 주기적으로 다시 뜬다.
 * 간격은 탭 폴링이 이미 쓰는 값을 그대로 따른다. 같은 이유로 도는 폴링이 서로 다른 주기를 갖는 편이 더 나쁘다.
 */
const LOCAL_DUMP_INTERVAL_MS = 1_000;
const REMOTE_DUMP_INTERVAL_MS = 3_000;

/**
 * 세션이나 탭을 띄우거나 닫은 뒤 pane이 나타나거나 사라질 때까지 기다리는 상한과 확인 간격.
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
  canResize: boolean;
  tabs: MirrorTab[];
}

/** 세션을 비출 수 없는 이유. 화면이 빈 터미널 대신 이 사유를 보여 준다 */
export type SurfaceUnavailableReason = "no-session" | "session-not-running" | "zellij-too-old" | "herdr-missing" | "remote-unreachable";

export class SurfaceUnavailableError extends Error {
  constructor(readonly reason: SurfaceUnavailableReason) {
    super(reason);
    this.name = "SurfaceUnavailableError";
  }
}

const pendingSessionConnections = new Map<string, Promise<KanbanTask | null>>();

/** 화면 조회에서만 누락된 세션 정보를 복구하고, 입력 경로에서는 기존 세션을 확인한다. */
async function findMirrorTask(taskId: string, connectIfMissing = false): Promise<KanbanTask> {
  const taskRepo = await getTaskRepository();
  let task: KanbanTask | null = await taskRepo.findOneBy({ id: taskId });

  if (connectIfMissing && task && (!task.sessionType || !task.sessionName) &&
      task.projectId && (task.branchName || task.baseBranch)) {
    let pending = pendingSessionConnections.get(taskId);
    if (!pending) {
      const sessionType = task.sessionType;
      pending = (async () => connectTerminalSession(taskId, sessionType ?? await getDefaultSessionType()))()
        .finally(() => pendingSessionConnections.delete(taskId));
      pendingSessionConnections.set(taskId, pending);
    }
    task = await pending;
    if (!task) throw new SurfaceUnavailableError("session-not-running");
  }

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
  const task = await findMirrorTask(taskId, true);
  const target = toMirrorSessionTarget(task);

  if (target.sshHost) {
    await ensureRemoteHostReachable(target.sshHost);
  }

  if (target.sessionType === SessionType.TERMINAL) {
    return { taskId, sessionType: target.sessionType, canResize: true, tabs: listTerminalMirrorTabs(taskId) };
  }

  await startMultiplexerSessionIfMissing(task, target);
  const panes = await listMirrorPanes(target);

  if (panes.length === 0) {
    throw new SurfaceUnavailableError("session-not-running");
  }

  return { taskId, sessionType: target.sessionType, canResize: true, tabs: groupPanesIntoTabs(panes) };
}

export async function createMobileTerminalTab(taskId: string): Promise<{ surfaces: TaskSurfaces; paneId: string }> {
  const previous = await getTaskSurfaces(taskId);
  const previousTabIds = new Set(previous.tabs.map((tab) => tab.id));
  const result = await createTerminalTab(taskId);
  if (!result.ok) {
    throw new Error(result.error ?? "터미널 탭 생성 실패");
  }

  const deadline = Date.now() + SESSION_START_TIMEOUT_MS;
  while (true) {
    const surfaces = await getTaskSurfaces(taskId);
    const created = surfaces.tabs.find((tab) => !previousTabIds.has(tab.id) && tab.panes.length > 0);
    if (created) {
      return { surfaces, paneId: created.panes[0].id };
    }
    if (Date.now() >= deadline) {
      throw new Error("추가한 터미널 탭을 찾을 수 없습니다.");
    }
    await new Promise((resolve) => setTimeout(resolve, SESSION_START_POLL_INTERVAL_MS));
  }
}

/** 이 태스크에 없거나 이미 닫힌 pane. 라우팅이 이름을 보고 `pane-not-found`로 옮기므로 내보내지 않는다 */
class PaneNotFoundError extends Error {
  constructor() {
    super("이미 닫혔거나 이 태스크에 없는 탭입니다");
    this.name = "PaneNotFoundError";
  }
}

/** 세션의 마지막 pane. 이것을 닫으면 세션이 통째로 사라지므로 태스크 삭제와 구분해 막는다 */
class LastPaneError extends Error {
  constructor() {
    super("마지막 탭은 닫을 수 없습니다");
    this.name = "LastPaneError";
  }
}

/** 현재 명령이 이 이름 중 하나면 입력을 기다리는 셸일 뿐이라 닫아도 잃는 작업이 없다 */
const IDLE_SHELL_COMMANDS = new Set(["sh", "bash", "zsh", "fish", "dash"]);

/**
 * 태스크별로 pane 종료를 한 줄로 세운다. 두 기기가 마지막 두 pane을 동시에 닫을 때 둘 다 남은 pane이 있다고 보면 세션이 통째로 사라진다.
 */
const pendingPaneCloses = new Map<string, Promise<unknown>>();

function listSurfacePanes(surfaces: TaskSurfaces): MirrorPane[] {
  return surfaces.tabs.flatMap((tab) => tab.panes);
}

/** 모바일 탭 바의 탭 하나는 pane 하나다. 같은 창의 다른 pane은 그대로 남긴다 */
export async function closeMobilePane(taskId: string, paneId: string): Promise<{ surfaces: TaskSurfaces }> {
  const previousClose = (pendingPaneCloses.get(taskId) ?? Promise.resolve()).catch(() => undefined);
  const closeOperation = previousClose.then(() => closeSessionPane(taskId, paneId));
  pendingPaneCloses.set(taskId, closeOperation);
  try {
    return await closeOperation;
  } finally {
    if (pendingPaneCloses.get(taskId) === closeOperation) {
      pendingPaneCloses.delete(taskId);
    }
  }
}

async function closeSessionPane(taskId: string, paneId: string): Promise<{ surfaces: TaskSurfaces }> {
  const surfaces = await getTaskSurfaces(taskId);
  const panes = listSurfacePanes(surfaces);
  const pane = panes.find((candidate) => candidate.id === paneId);
  if (!pane) throw new PaneNotFoundError();
  if (panes.length === 1) throw new LastPaneError();

  const target = await findMirrorSessionTarget(taskId);
  if (target.sessionType === SessionType.TERMINAL) {
    closeLocalTerminalTab(taskId, pane.tabId);
  } else {
    await runMirrorCommand(buildClosePaneCommand(target, paneId), target.sshHost);
  }

  return { surfaces: await waitUntilPaneDisappears(taskId, paneId) };
}

function buildClosePaneCommand(target: MirrorSessionTarget, paneId: string): string {
  if (target.sessionType === SessionType.HERDR) return buildHerdrClosePaneCommand(target.sessionName, paneId);
  if (target.sessionType === SessionType.ZELLIJ) return buildZellijClosePaneCommand(target.sessionName, paneId);
  return buildTmuxClosePaneCommand(paneId);
}

async function waitUntilPaneDisappears(taskId: string, paneId: string): Promise<TaskSurfaces> {
  const deadline = Date.now() + SESSION_START_TIMEOUT_MS;
  while (true) {
    const surfaces = await getTaskSurfaces(taskId);
    if (!listSurfacePanes(surfaces).some((pane) => pane.id === paneId)) {
      return surfaces;
    }
    if (Date.now() >= deadline) {
      throw new Error("탭이 닫히지 않았습니다.");
    }
    await new Promise((resolve) => setTimeout(resolve, SESSION_START_POLL_INTERVAL_MS));
  }
}

/**
 * 닫으면 잃을 수 있는 실행 중인 프로세스 이름. 빈 배열이면 확인 없이 닫아도 된다.
 * 조회에 실패하면 없다고 단정하지 않고 오류를 올려, 호출한 쪽이 확인을 거치게 한다.
 */
export async function getMobilePaneCloseInfo(taskId: string, paneId: string): Promise<{ processNames: string[] }> {
  const surfaces = await getTaskSurfaces(taskId);
  const pane = listSurfacePanes(surfaces).find((candidate) => candidate.id === paneId);
  if (!pane) throw new PaneNotFoundError();

  const currentCommand = surfaces.sessionType === SessionType.TERMINAL
    ? getLocalTerminalTabProcess(taskId, pane.tabId)
    : pane.command;
  const isRunningProgram = Boolean(currentCommand) && !IDLE_SHELL_COMMANDS.has(currentCommand as string);
  return { processNames: isRunningProgram ? [currentCommand as string] : [] };
}

async function listMirrorPanes(target: MirrorSessionTarget): Promise<MirrorPane[]> {
  if (target.sessionType === SessionType.HERDR) {
    return listHerdrMirrorPanes(target.sessionName, target.sshHost);
  }

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

  /** 설치에는 사용자 동의가 필요해 모바일이 대신 설치하지 않고, 세션 문제와 구분해 안내한다 */
  if (target.sessionType === SessionType.HERDR && !(await getSessionDependencyStatus(target.sessionType, target.sshHost)).available) {
    throw new SurfaceUnavailableError("herdr-missing");
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

async function resolveMobileProjectDirectory(submittedPath: string): Promise<string> {
  if (typeof submittedPath !== "string" || !submittedPath || /[\x00-\x1f\x7f]/.test(submittedPath)) {
    throw new MobileTaskInputError("올바른 폴더 경로를 입력해 주세요");
  }
  const expandedPath = submittedPath === "~" || submittedPath.startsWith("~/")
    ? path.join(homedir(), submittedPath.slice(1))
    : submittedPath;
  if (!path.isAbsolute(expandedPath)) {
    throw new MobileTaskInputError("절대 경로나 ~/로 시작하는 폴더를 선택해 주세요");
  }
  try {
    if (!(await stat(expandedPath)).isDirectory()) {
      throw new MobileTaskInputError("폴더를 선택해 주세요");
    }
    // 기존 데스크톱 조회는 find 오류를 빈 목록으로 바꾸므로, 모바일은 접근 가능 여부를 먼저 확인한다.
    await access(expandedPath, fsConstants.R_OK | fsConstants.X_OK);
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "ENOENT" || code === "ENOTDIR") {
      throw new MobileTaskInputError("폴더를 찾을 수 없습니다");
    }
    if (code === "EACCES" || code === "EPERM") {
      throw new MobileTaskInputError("이 폴더에 접근할 권한이 없습니다");
    }
    throw error;
  }
  return expandedPath;
}

export async function getMobileDirectories(parentPath: string): Promise<string[]> {
  return listSubdirectories(await resolveMobileProjectDirectory(parentPath));
}

export async function scanMobileProjects(rootPath: string) {
  return scanAndRegisterProjects(await resolveMobileProjectDirectory(rootPath));
}

/** 완료 태스크의 보드 페이지와 관계없이 알림이 가리키는 태스크를 직접 조회한다 */
export async function getMobileTask(taskId: string) {
  const task = await getTaskById(taskId);
  return task ? { ...task, projectName: task.project?.name ?? null } : null;
}

export const isMobileAiSessionProvider = isAiSessionProvider;

const MOBILE_AI_SOURCE_CACHE_LIMIT = 500;
const mobileAiSourceCache = new Map<string, string | null>();

function mobileAiSessionKey(taskId: string, provider: AiSessionProvider, sessionId: string) {
  return JSON.stringify([taskId, provider, sessionId]);
}

function rememberMobileAiSources(taskId: string, sessions: Awaited<ReturnType<typeof getTaskAiSessions>>["sessions"]) {
  for (const session of sessions) {
    const key = mobileAiSessionKey(taskId, session.provider, session.id);
    mobileAiSourceCache.delete(key);
    mobileAiSourceCache.set(key, session.sourceRef ?? null);
    if (mobileAiSourceCache.size > MOBILE_AI_SOURCE_CACHE_LIMIT) {
      mobileAiSourceCache.delete(mobileAiSourceCache.keys().next().value!);
    }
  }
}

async function refreshMobileAiSource(taskId: string, provider: AiSessionProvider, sessionId: string) {
  const history = await getTaskAiSessions(taskId);
  const session = history.sessions.find((entry) => entry.provider === provider && entry.id === sessionId);
  if (session) rememberMobileAiSources(taskId, [session]);
}

interface MobileAiHistoryOptions {
  query?: string;
  cursor?: string;
  limit: number;
  roles?: AiMessageRole[];
}

export async function getMobileAiSessions(taskId: string, options: MobileAiHistoryOptions) {
  const history = await getTaskAiSessions(taskId, options.query, options.cursor, options.limit);
  rememberMobileAiSources(taskId, history.sessions);
  return { ...history, sessions: history.sessions.map(({ sourceRef: _sourceRef, ...session }) => session) };
}

export async function getMobileAiSessionDetail(
  taskId: string,
  provider: AiSessionProvider,
  sessionId: string,
  options: MobileAiHistoryOptions,
) {
  const key = mobileAiSessionKey(taskId, provider, sessionId);
  const hadCachedSource = mobileAiSourceCache.has(key);
  if (!hadCachedSource) {
    await refreshMobileAiSource(taskId, provider, sessionId);
  }
  if (!mobileAiSourceCache.has(key)) return null;
  // 캐시는 탐색 힌트다. 매 상세 조회에서 리더가 현재 태스크 경로와 세션 ID를 다시 검증한다.
  let detail: Awaited<ReturnType<typeof getTaskAiSessionDetail>>;
  try {
    detail = await getTaskAiSessionDetail(taskId, provider, sessionId, mobileAiSourceCache.get(key), options.cursor, options.limit, options.query, options.roles);
  } catch (error) {
    if (!(error instanceof Error) || !["ENOENT", "ENOTDIR"].includes((error as NodeJS.ErrnoException).code ?? "")) throw error;
    detail = null;
  }
  if (!detail) {
    mobileAiSourceCache.delete(key);
    if (!hadCachedSource) return null;
    await refreshMobileAiSource(taskId, provider, sessionId);
    if (!mobileAiSourceCache.has(key)) return null;
    detail = await getTaskAiSessionDetail(taskId, provider, sessionId, mobileAiSourceCache.get(key), options.cursor, options.limit, options.query, options.roles);
  }
  if (!detail) {
    mobileAiSourceCache.delete(key);
    return null;
  }
  const { sourceRef: _sourceRef, ...safeDetail } = detail;
  return safeDetail;
}

export async function getMobileAiSessionAttachment(taskId: string, provider: AiSessionProvider, sessionId: string, attachmentId: string, sourceRef?: string | null) {
  const key = mobileAiSessionKey(taskId, provider, sessionId);
  if (sourceRef == null && !mobileAiSourceCache.has(key)) await refreshMobileAiSource(taskId, provider, sessionId);
  if (sourceRef == null && !mobileAiSourceCache.has(key)) return null;
  try {
    return await getTaskAiSessionAttachment(taskId, provider, sessionId, attachmentId, sourceRef ?? mobileAiSourceCache.get(key));
  } catch {
    throw new MobileTaskInputError("첨부 파일을 불러올 수 없습니다. 파일 형식·크기와 원본 파일을 확인해 주세요.");
  }
}

function toMobileNotification(notification: AppNotification) {
  const { id, title, body, taskId, isRead, createdAt } = notification;
  return { id, title, body, taskId, isRead, createdAt };
}

export async function getMobileNotifications() {
  const notifications = await listNotifications();
  return notifications.map(toMobileNotification);
}

export async function getMobileNotificationSettings() {
  return getNotificationSettings();
}

export async function updateMobileNotificationSettings(input: unknown) {
  const settings = input as { isEnabled?: unknown; enabledStatuses?: unknown } | null;
  if (!settings || typeof settings.isEnabled !== "boolean" || !Array.isArray(settings.enabledStatuses) ||
      settings.enabledStatuses.some((status) => !Object.values(TaskStatus).includes(status))) {
    throw new MobileTaskInputError("알림 상태 설정이 올바르지 않습니다.");
  }
  await setNotificationStatuses([...new Set(settings.enabledStatuses)]);
  await setNotificationEnabled(settings.isEnabled);
  return getNotificationSettings();
}

export async function markMobileNotificationRead(notificationId: string) {
  const notification = await markNotificationRead(notificationId);
  return notification ? toMobileNotification(notification) : null;
}

export async function markAllMobileNotificationsRead(): Promise<void> {
  await markAllNotificationsRead();
}

export function onMobileNotificationsChanged(listener: () => void): () => void {
  return subscribeToNotificationChanges(listener);
}

/**
 * 모바일이 보낸 값이 데스크탑 열거형에 없을 때.
 *
 * 서버 오류가 아니라 요청이 잘못된 것이라 라우팅이 이름을 보고 400으로 옮긴다.
 * 이름으로만 가려내므로 클래스 자체는 이 파일 밖으로 내보내지 않는다 — `serviceRegistry`가 이 모듈을 통째로
 * IPC에 얹기 때문에, 내보내면 화면이 부를 일이 없는 값이 그 표면에 하나 더 얹힌다.
 * 세션 타입 변경에서 모르는 값을 조용히 버리면 기기에는 바뀐 것으로 보이고 데스크탑 세션은 그대로 남는다.
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

function readGitRef(submitted: string, fieldLabel: string): string {
  if (!isSafeGitRef(submitted)) {
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
}

/**
 * 모바일에서 태스크를 만든다.
 *
 * 제목을 따로 받지 않고 브랜치 이름을 그대로 쓰는 것은 데스크탑 생성 화면과 같다.
 * 두 화면이 다른 규칙으로 제목을 정하면 같은 보드에서 어느 쪽에서 만들었는지에 따라 제목 모양이 갈린다.
 */
export async function createMobileTask(draft: MobileTaskDraft): Promise<KanbanTask> {
  const createdTask = await createTask({
    title: draft.branchName,
    branchName: readGitRef(draft.branchName, "브랜치 이름"),
    projectId: draft.projectId,
    baseBranch: draft.baseBranch ? readGitRef(draft.baseBranch, "베이스 브랜치") : undefined,
    description: draft.description,
    priority: readEnumValue(Object.values(TaskPriority), draft.priority, "우선순위"),
    /** 세션 종류는 데스크탑이 정한다. 이전 앱이 보낸 값이 있어도 따르지 않는다 */
    sessionType: await getDefaultSessionType(),
  });
  await registerMobileTaskOnPeerClients(createdTask);
  return createdTask;
}

/**
 * 새 세션 도구가 없어 바꿀 수 없다. 기기에서는 설치를 진행할 수 없으니 데스크탑에서 설치하라고 안내해야 해서
 * 입력 오류(400)와 나눠 라우팅이 409로 옮긴다. 이름으로만 가려내는 것은 [MobileTaskInputError]와 같다.
 */
class MobileSessionDependencyMissingError extends Error {
  constructor(toolName: string) {
    super(`${toolName}이(가) 설치되어 있지 않습니다. 데스크탑에서 설치한 뒤 다시 시도해 주세요.`);
    this.name = "MobileSessionDependencyMissingError";
  }
}

/** 기기에서 세션 타입을 바꾼다. 실제 전환은 데스크탑 경로가 하므로 데스크탑과 같은 세션 정보가 남는다 */
export async function changeMobileTaskSessionType(
  taskId: string,
  submittedSessionType: string,
  shouldCloseExistingSession: boolean,
): Promise<KanbanTask | null> {
  const sessionType = readEnumValue(Object.values(SessionType), submittedSessionType, "세션 타입");
  if (!sessionType) {
    throw new MobileTaskInputError("세션 타입 값이 올바르지 않습니다");
  }

  const task = await getTaskById(taskId);
  if (!task) {
    return null;
  }

  const dependencyStatus = await getSessionDependencyStatus(sessionType, task.sshHost);
  if (!dependencyStatus.available && dependencyStatus.toolName) {
    throw new MobileSessionDependencyMissingError(dependencyStatus.toolName);
  }

  const changedTask = await changeTaskSessionType(taskId, sessionType, shouldCloseExistingSession);
  if (!changedTask) {
    throw new Error(`세션 타입 변경 실패: ${taskId}`);
  }

  return changedTask;
}

/** 기기가 세션을 닫을지 묻기 전에 기존 세션이 켜져 있는지 확인한다 */
export function isMobileTaskSessionAlive(taskId: string): Promise<boolean> {
  return isTaskSessionAlive(taskId);
}

/** 태스크 세션이 바뀌면 알린다. 이전 세션을 비추던 기기 스트림을 끊어 새 세션으로 다시 붙게 하는 데 쓴다 */
export function watchTaskSessionChange(taskId: string, onChanged: () => void): () => void {
  return subscribeToBoardEvents((event) => {
    if (event.type === "task-session-changed" && event.taskId === taskId) {
      onChanged();
    }
  });
}

/**
 * worktree는 연결된 이 데스크탑만 만들고, 같은 프로젝트를 다루는 다른 데스크탑에는 task 등록만 전파한다.
 * 이미 만든 task를 되돌리지 않도록 전파 실패는 기기에 오류로 돌려주지 않고 로그로만 남긴다.
 */
async function registerMobileTaskOnPeerClients(createdTask: KanbanTask): Promise<void> {
  if (!createdTask.projectId || !createdTask.branchName || !createdTask.worktreePath) {
    return;
  }

  const project = await (await getProjectRepository()).findOneBy({ id: createdTask.projectId });
  if (!project) {
    return;
  }

  const peerResults = await propagateTaskRegistrationToPeers(project.repoPath, createdTask.branchName, project.sshHost);
  const failedPeers = peerResults.filter((result) => !result.isSuccess);
  if (failedPeers.length > 0) {
    console.warn("[mobile] 다른 데스크탑에 task를 등록하지 못했습니다", { taskId: createdTask.id, failedPeers });
  }
}

/**
 * 모바일에서 태스크를 지운다. 연결된 이 데스크탑이 지우고 같은 task를 가진 다른 데스크탑으로 전파한다.
 * 커밋하지 않은 변경이 있으면 `DirtyWorktreeError`로 멈추고, 기기가 강제 삭제를 고르면 그래도 지운다.
 * 꺼져 있어 전파받지 못한 데스크탑 수를 돌려 기기가 경고하게 하고, 없는 태스크면 null이 나와 라우팅이 404로 옮긴다.
 */
export async function deleteMobileTask(taskId: string, isForced: boolean): Promise<{ failedPeerCount: number } | null> {
  const deletion = await deleteTaskAcrossTargets(taskId, {
    shouldRejectDirtyWorktree: !isForced,
    shouldPropagateToPeers: true,
  });
  if (!deletion.isDeleted) {
    return null;
  }
  return { failedPeerCount: deletion.peerResults.filter((result) => !result.isSuccess).length };
}

export async function removeMobileTaskRegistration(taskId: string): Promise<boolean> {
  return removeTaskRegistration(taskId);
}

/** 모바일에서 태스크를 다른 상태로 옮긴다. 없는 태스크면 null이 나와 라우팅이 404로 옮긴다 */
export async function updateMobileTaskStatus(taskId: string, submittedStatus: string): Promise<KanbanTask | null> {
  const status = readEnumValue(Object.values(TaskStatus), submittedStatus, "상태");
  if (!status) {
    throw new MobileTaskInputError("상태 값이 올바르지 않습니다");
  }

  return updateTaskStatus(taskId, status);
}

export interface MobileConnectionSettings {
  mobileTlsPort: number;
  isLanPlaintextAllowed: boolean;
}

/** 설정 화면이 보여 줄 모바일 포트와 LAN 평문 허용 여부 */
export async function readMobileConnectionSettings(): Promise<MobileConnectionSettings> {
  return { mobileTlsPort: getMobileTlsPort(), isLanPlaintextAllowed: await getLanPlaintextAllowed() };
}

/** LAN 평문 포트를 임시로 계속 열어 둘지 저장한다. 포트는 앱 시작 때 열리므로 다음 실행부터 적용된다 */
export async function updateLanPlaintextAllowed(isAllowed: boolean): Promise<void> {
  await setLanPlaintextAllowed(isAllowed);
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
 * 페어링과 푸시 구독을 바꾸는 곳이 모두 이 줄을 지나므로 해제된 기기가 늦은 구독 등록으로 되살아나지 않는다.
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

/** 요청이 증명한 기기 하나의 푸시 구독을 등록하거나 회전된 토큰으로 바꾼다 */
export async function updateMobileNotificationSubscription(
  authorizationHeader: string | undefined,
  subscription: MobilePushSubscription,
): Promise<boolean> {
  if (!isMobilePushSubscription(subscription)) {
    throw new MobileTaskInputError("알림 구독 값이 올바르지 않습니다");
  }
  return withDeviceList(async (devices) => {
    const device = findPairedDevice(devices, parseBearerToken(authorizationHeader));
    if (!device) return false;
    device.notification = { pushToken: subscription.pushToken, platform: subscription.platform, connectionId: subscription.connectionId };
    await setAppSetting(PAIRED_DEVICES_KEY, serializePairedDevices(devices));
    return true;
  });
}

/** 알림만 끄는 동작은 페어링과 터미널 인증을 유지한다 */
export async function deleteMobileNotificationSubscription(authorizationHeader: string | undefined): Promise<boolean> {
  return withDeviceList(async (devices) => {
    const device = findPairedDevice(devices, parseBearerToken(authorizationHeader));
    if (!device) return false;
    delete device.notification;
    await setAppSetting(PAIRED_DEVICES_KEY, serializePairedDevices(devices));
    return true;
  });
}

/** 푸시 발송 경계에는 페어링 인증 토큰을 넘기지 않는다 */
export async function listMobilePushSubscriptions(): Promise<Array<MobilePushSubscription & { deviceId: string }>> {
  const devices = await readPairedDevices();
  return devices.flatMap(({ deviceId, notification }) => isMobilePushSubscription(notification)
    ? [{ deviceId, pushToken: notification.pushToken, platform: notification.platform, connectionId: notification.connectionId }]
    : []);
}

/** 늦은 FCM 만료 응답이 새로 등록된 토큰을 지우지 않도록 기기와 발송했던 토큰을 함께 대조한다 */
export async function removeUnregisteredMobilePushSubscription(deviceId: string, pushToken: string): Promise<void> {
  await withDeviceList(async (devices) => {
    const device = devices.find((candidate) => candidate.deviceId === deviceId);
    if (device?.notification?.pushToken !== pushToken) return;
    delete device.notification;
    await setAppSetting(PAIRED_DEVICES_KEY, serializePairedDevices(devices));
  });
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
  /** 스트림이 첫 화면을 스스로 보내는 경우에만 있다. 뒤늦게 붙은 구독자가 화면을 받도록 다시 보낸다 */
  resendScreen?: () => void;
  setViewport?: (size: { cols: number; rows: number } | null) => Promise<void>;
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

interface PaneViewportDriver {
  resize: (paneId: string, cols: number, rows: number) => Promise<void>;
  restore: () => Promise<void>;
}

interface PaneViewportRequest {
  paneId: string;
  size: { cols: number; rows: number } | null;
}

interface PaneViewportGroup {
  driver: Promise<PaneViewportDriver>;
  pending: Promise<void>;
  clients: Map<symbol, PaneViewportRequest>;
}

const paneViewports = new Map<string, PaneViewportGroup>();
let paneViewportShutdown = false;

function queuePaneViewport(group: PaneViewportGroup, action: (driver: PaneViewportDriver) => Promise<void>): Promise<void> {
  const operation = group.pending.then(() => group.driver).then(action);
  group.pending = operation.catch(() => {});
  return operation;
}

async function createPaneViewportDriver(target: MirrorSessionTarget, taskId: string, pane: MirrorPane): Promise<PaneViewportDriver> {
  if (target.sessionType === SessionType.TERMINAL) {
    const original = getTerminalTabSize(taskId, pane.id);
    return {
      resize: async (paneId, cols, rows) => resizeTerminalTab(taskId, paneId, cols, rows),
      restore: async () => releaseTerminalTabViewport(taskId, pane.id, original.cols, original.rows),
    };
  }

  if (target.sessionType === SessionType.HERDR) {
    return createHerdrViewportDriver(target, taskId, pane.id);
  }

  if (target.sessionType === SessionType.ZELLIJ) {
    return createZellijViewportDriver(target, taskId, pane.id);
  }

  const state = parseTmuxViewportState(await runMirrorCommand(buildTmuxReadViewportCommand(pane.tabId), target.sshHost));
  return {
    resize: async (paneId, cols, rows) => {
      const geometry = await runMirrorCommand(
        buildTmuxReadPaneGeometryCommand(pane.tabId, paneId),
        target.sshHost,
      );
      await runMirrorCommand(buildTmuxResizeViewportCommand(paneId, geometry, cols, rows), target.sshHost);
    },
    restore: async () => { await runMirrorCommand(buildTmuxRestoreViewportCommand(pane.tabId, state), target.sshHost); },
  };
}

async function createZellijViewportDriver(target: MirrorSessionTarget, taskId: string, paneId: string): Promise<PaneViewportDriver> {
  const readGeometry = async (id: string) => {
    const output = await runMirrorCommand(buildZellijReadViewportCommand(target.sessionName), target.sshHost);
    const geometry = parseZellijViewportState(output, id);
    if (!geometry) throw new Error("Zellij pane 크기를 확인하지 못했습니다.");
    return geometry;
  };
  const floatingOriginals = new Map<string, { left: number; top: number; cols: number; rows: number }>();
  const initial = await readGeometry(paneId);
  const original = getTaskTerminalSize(taskId) ?? { cols: initial.displayCols, rows: initial.displayRows };
  const client = new CallbackTerminalClient(() => {});
  try {
    await attachTaskTerminal(await findMirrorTask(taskId), null, client, original.cols, original.rows);
    if (!getTaskTerminalSize(taskId)) throw new Error("Zellij 터미널 연결이 종료됐습니다.");
  } catch (error) {
    client.close();
    throw error;
  }
  return {
    resize: async (id, cols, rows) => {
      const selected = await readGeometry(id);
      await runMirrorCommand(buildZellijSelectTabCommand(target.sessionName, selected.tabId), target.sshHost);
      const deadline = Date.now() + MIRROR_COMMAND_TIMEOUT_MS;
      do {
        const geometry = await readGeometry(id);
        if (geometry.cols === cols && geometry.rows === rows) {
          const current = getTaskTerminalSize(taskId);
          if (!current) throw new Error("Zellij 터미널 연결이 종료됐습니다.");
          resizeTaskTerminalViewport(taskId, current.cols, current.rows);
          return;
        }
        const outerCols = cols + geometry.paneCols - geometry.cols;
        const outerRows = rows + geometry.paneRows - geometry.rows;
        if (geometry.floating && !floatingOriginals.has(id)) {
          floatingOriginals.set(id, { ...geometry.floating, cols: geometry.paneCols, rows: geometry.paneRows });
        }
        const floating = geometry.floating ? floatingOriginals.get(id)! : null;
        const nextCols = floating
          ? Math.max(original.cols, floating.left + outerCols + geometry.displayCols - geometry.viewportCols)
          : Math.round(geometry.viewportCols * outerCols / geometry.paneCols) + geometry.displayCols - geometry.viewportCols;
        const nextRows = floating
          ? Math.max(original.rows, floating.top + outerRows + geometry.displayRows - geometry.viewportRows)
          : Math.round(geometry.viewportRows * outerRows / geometry.paneRows) + geometry.displayRows - geometry.viewportRows;
        if (![nextCols, nextRows].every((size) => size > 0 && size <= 65535)) {
          throw new Error("Zellij 분할 배치에서 요청한 크기를 적용할 수 없습니다.");
        }
        resizeTaskTerminalViewport(taskId, nextCols, nextRows);
        if (floating) {
          await runMirrorCommand(buildZellijResizeFloatingPaneCommand(target.sessionName, id, {
            left: floating.left, top: floating.top, cols: outerCols, rows: outerRows,
          }), target.sshHost);
        }
        // Zellij가 PTY resize 신호로 배치를 다시 계산한 뒤 실제 content 격자를 확인한다.
        await new Promise((resolve) => setTimeout(resolve, VIEWPORT_SETTLE_POLL_INTERVAL_MS));
      } while (Date.now() < deadline);
      throw new Error("다른 Zellij 클라이언트나 탭 배치가 모바일 크기를 제한하고 있습니다.");
    },
    restore: async () => {
      try {
        releaseTaskTerminalViewport(taskId, original.cols, original.rows);
        if (floatingOriginals.size > 0) {
          await new Promise((resolve) => setTimeout(resolve, VIEWPORT_SETTLE_POLL_INTERVAL_MS));
          await Promise.all([...floatingOriginals].map(([id, geometry]) =>
            runMirrorCommand(buildZellijResizeFloatingPaneCommand(target.sessionName, id, geometry), target.sshHost),
          ));
        }
      } finally {
        client.close();
      }
    },
  };
}

function createHerdrViewportDriver(target: MirrorSessionTarget, taskId: string, paneId: string): PaneViewportDriver {
  const key = buildSubscriptionKey(taskId, paneId);
  let controller: ChildProcess | null = null;
  let failure: Error | null = null;
  let pendingFrame: { cols: number; rows: number; resolve: () => void; reject: (error: Error) => void } | null = null;

  const failController = (child: ChildProcess, error: Error) => {
    if (controller !== child) return;
    controller = null;
    failure = error;
    pendingFrame?.reject(error);
    stopMirrorProcess(child);
    const claim = paneSubscriptions.get(key);
    if (!claim) return;
    void claim.subscription.then((subscription) => {
      if (!forgetPaneClaim(key, claim)) return;
      subscription.stop();
      const listeners = [...subscription.endListeners];
      subscription.listeners.clear();
      subscription.endListeners.clear();
      listeners.forEach((listener) => listener());
    }).catch(() => {});
  };

  const awaitFrame = (child: ChildProcess, cols: number, rows: number, send: () => void) => new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => failController(child, new Error("Herdr 터미널 크기 응답이 없습니다.")), MIRROR_COMMAND_TIMEOUT_MS);
    const finish = (error?: Error) => {
      clearTimeout(timer);
      pendingFrame = null;
      if (error) reject(error);
      else resolve();
    };
    pendingFrame = { cols, rows, resolve: () => finish(), reject: (error) => finish(error) };
    try {
      send();
    } catch (error) {
      failController(child, error instanceof Error ? error : new Error(String(error)));
    }
  });

  const openController = async (cols: number, rows: number) => {
    const child = await spawnMirrorProcess(buildHerdrControlPaneCommand(target.sessionName, paneId, cols, rows), target.sshHost);
    controller = child;
    let partialLine = "";
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      if (controller !== child) return;
      const lines = `${partialLine}${chunk}`.split("\n");
      partialLine = lines.pop() ?? "";
      for (const line of lines) {
        const record = parseHerdrObserveRecord(line);
        if (record?.type === "closed") {
          failController(child, new Error("Herdr 터미널 제어 연결이 종료됐습니다."));
          break;
        }
        if (record?.type === "frame" && record.isFullScreen &&
            record.width === pendingFrame?.cols && record.height === pendingFrame?.rows) {
          pendingFrame?.resolve();
        }
      }
    });
    child.stderr?.resume();
    child.on("error", (error) => failController(child, error));
    child.on("exit", () => failController(child, new Error("Herdr 터미널 제어 프로세스가 종료됐습니다.")));
    child.stdin?.on("error", (error) => failController(child, error));
    await awaitFrame(child, cols, rows, () => {});
  };

  return {
    resize: async (_paneId, cols, rows) => {
      if (failure) throw failure;
      const subscription = await paneSubscriptions.get(key)?.subscription;
      await subscription?.setViewport?.({ cols, rows });
      if (!controller) {
        await openController(cols, rows);
        return;
      }
      const child = controller;
      await awaitFrame(child, cols, rows, () => {
        if (!child.stdin) throw new Error("Herdr 터미널 제어 입력이 닫혔습니다.");
        child.stdin.write(`${JSON.stringify({ type: "terminal.resize", cols, rows })}\n`, (error) => {
          if (error) failController(child, error);
        });
      });
    },
    restore: async () => {
      const child = controller;
      if (!child) return;
      controller = null;
      await new Promise<void>((resolve, reject) => {
        const finish = (error?: Error) => {
          clearTimeout(timer);
          child.off("exit", onExit);
          child.off("error", onError);
          child.stdin?.off("error", onError);
          if (error) {
            stopMirrorProcess(child);
            reject(error);
          } else resolve();
        };
        const onExit = () => finish();
        const onError = (error: Error) => finish(error);
        const timer = setTimeout(() => finish(new Error("Herdr 터미널 제어 반납 응답이 없습니다.")), MIRROR_COMMAND_TIMEOUT_MS);
        child.once("exit", onExit);
        child.once("error", onError);
        child.stdin?.once("error", onError);
        try {
          if (!child.stdin) onError(new Error("Herdr 터미널 제어 입력이 닫혔습니다."));
          else child.stdin.end(`${JSON.stringify({ type: "terminal.release" })}\n`);
        } catch (error) {
          onError(error instanceof Error ? error : new Error(String(error)));
        }
      });
      const subscription = await paneSubscriptions.get(key)?.subscription;
      await subscription?.setViewport?.(null);
    },
  };
}

/** 창 배치를 공유하는 pane은 같은 줄에서 크기를 바꿔야 한 기기의 퇴장이 다른 기기의 화면을 되돌리지 않는다. */
export async function claimPaneViewport(taskId: string, paneId: string): Promise<{
  resize: (cols: number, rows: number) => Promise<string | null>;
  release: () => Promise<void>;
}> {
  if (paneViewportShutdown) throw new Error("데스크탑이 종료 중입니다.");
  const task = await findMirrorTask(taskId);
  const target = toMirrorSessionTarget(task);
  const panes = target.sessionType === SessionType.TERMINAL
    ? listTerminalMirrorTabs(taskId).flatMap((tab) => tab.panes)
    : await listMirrorPanes(target);
  const pane = panes.find((candidate) => candidate.id === paneId);
  if (paneViewportShutdown) throw new Error("데스크탑이 종료 중입니다.");
  if (!pane) {
    throw new SurfaceUnavailableError("session-not-running");
  }

  const key = buildSubscriptionKey(taskId, target.sessionType === SessionType.ZELLIJ ? "zellij" : target.sessionType === SessionType.TMUX ? pane.tabId : paneId);
  let group = paneViewports.get(key);
  if (!group) {
    group = { driver: createPaneViewportDriver(target, taskId, pane), pending: Promise.resolve(), clients: new Map() };
    paneViewports.set(key, group);
  }
  const claimed = group;
  const id = Symbol();
  const request: PaneViewportRequest = { paneId, size: null };
  claimed.clients.set(id, request);
  let released: Promise<void> | null = null;

  return {
    resize: async (cols, rows) => {
      if (released || !claimed.clients.has(id)) return null;
      if (![cols, rows].every((value) => Number.isInteger(value) && value >= 1 && value <= 500)) {
        return Promise.reject(new Error("터미널 크기가 유효하지 않습니다."));
      }
      request.size = { cols, rows };
      claimed.clients.delete(id);
      claimed.clients.set(id, request);
      let snapshot: string | null = null;
      await queuePaneViewport(claimed, async (driver) => {
        if (!claimed.clients.has(id)) return;
        await driver.resize(paneId, cols, rows);
        if (target.sessionType === SessionType.TMUX || target.sessionType === SessionType.ZELLIJ) {
          snapshot = await readPaneSnapshot(target, paneId);
        }
      });
      return snapshot;
    },
    release: () => {
      if (released) return released;
      claimed.clients.delete(id);
      const restoration = queuePaneViewport(claimed, async (driver) => {
        const remaining = [...claimed.clients.values()].reverse().find((client) => client.size);
        if (remaining?.size) {
          await driver.resize(remaining.paneId, remaining.size.cols, remaining.size.rows);
        } else {
          await driver.restore();
        }
      });
      const queued = claimed.pending;
      released = restoration.finally(() => {
        if (claimed.pending === queued && claimed.clients.size === 0 && paneViewports.get(key) === claimed) paneViewports.delete(key);
      });
      return released;
    },
  };
}

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
  const isJoiningLiveStream = subscription.listeners.size > 0;
  subscription.listeners.add(onChunk);
  subscription.endListeners.add(onEnd);
  if (isJoiningLiveStream) {
    subscription.resendScreen?.();
  }

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
 * 크기 변경은 출력 구독과 분리해 [claimPaneViewport]가 소유한다.
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
 * 모바일이 소유하던 임시 크기도 함께 복원한다.
 */
export async function stopAllPaneMirrors(shutdown = false): Promise<void> {
  if (shutdown) paneViewportShutdown = true;
  const restorations: Promise<void>[] = [];
  for (const [key, group] of paneViewports) {
    group.clients.clear();
    const restoration = queuePaneViewport(group, (driver) => driver.restore());
    const queued = group.pending;
    restorations.push(restoration
      .catch((error) => console.error("[kanvibe] Mobile viewport restore failed:", error))
      .finally(() => { if (group.pending === queued && paneViewports.get(key) === group) paneViewports.delete(key); }));
  }
  for (const [key, claim] of [...paneSubscriptions]) {
    forgetPaneClaim(key, claim);
    void claim.subscription.then((subscription) => subscription.stop()).catch(() => {});
  }
  await Promise.all(restorations);
}

/** pane의 지금 화면을 모바일 터미널이 처음부터 다시 그리는 출력으로 떠 온다 */
async function readPaneSnapshot(target: MirrorSessionTarget, paneId: string): Promise<string> {
  if (target.sessionType === SessionType.ZELLIJ) {
    const dump = await runMirrorCommand(buildZellijDumpPaneCommand(target.sessionName, paneId), target.sshHost);
    return buildScreenRedraw(dump, null);
  }

  if (target.sessionType === SessionType.HERDR) {
    const recentScreen = await runMirrorCommand(buildHerdrRecentScreenCommand(target.sessionName, paneId), target.sshHost);
    return buildScreenRedraw(recentScreen, null);
  }

  /** 화면과 커서를 한 번에 읽는다. 원격이면 왕복이 하나 줄고, 둘 사이에 화면이 바뀌는 틈도 좁아진다 */
  const output = await runMirrorCommand(
    `${buildTmuxCapturePaneCommand(paneId)} && ${buildTmuxPaneStateCommand(paneId)}`,
    target.sshHost,
  );
  const cursorLineStart = output.lastIndexOf("\n");
  const state = cursorLineStart === -1 ? null : parseTmuxPaneState(output.slice(cursorLineStart + 1));
  const screen = state ? output.slice(0, cursorLineStart) : output;
  return `${state?.restoreSequences ?? ""}${buildScreenRedraw(screen, state?.cursor ?? null)}`;
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

  if (target.sessionType === SessionType.HERDR) {
    const stream = await startHerdrFrameStream(target, paneId, broadcast, handleStreamDeath);
    return { listeners, endListeners, ...stream };
  }

  const stop = target.sessionType === SessionType.ZELLIJ
    ? startZellijDumpPolling(target, paneId, broadcast)
    : await startTmuxPaneStream(target, paneId, broadcast, handleStreamDeath);

  return { listeners, endListeners, stop };
}

/**
 * Herdr pane의 화면을 `terminal session observe` 프레임으로 따라간다.
 *
 * 변경 프레임은 화면 좌표에 글자를 덮어쓰기만 해서, 위로 밀려난 줄이 모바일 스크롤에 쌓이지 않는다.
 * 그래서 출력이 잠잠해졌을 때 지난 출력 줄 수가 늘었으면 이력을 다시 보내고 observe를 새로 띄운다.
 * 새 observe의 첫 프레임은 항상 전체 화면이라, 이력을 받은 화면과 커서를 끊김 없이 맞출 수 있다.
 * Herdr의 스크롤백이 상한에 닿으면 줄 수가 더 늘지 않아, 그 뒤의 이력은 화면을 다시 열 때 갱신된다.
 */
async function startHerdrFrameStream(
  target: MirrorSessionTarget,
  paneId: string,
  broadcast: (chunk: string) => void,
  onDead: () => void,
): Promise<Pick<PaneSubscription, "stop" | "resendScreen" | "setViewport">> {
  const quietPeriodMs = target.sshHost ? REMOTE_DUMP_INTERVAL_MS : LOCAL_DUMP_INTERVAL_MS;
  let observer: ChildProcess | null = null;
  let quietTimer: ReturnType<typeof setTimeout> | null = null;
  let isStopped = false;
  let isRefreshing = false;
  let observerGeneration = 0;
  let viewportSize: { cols: number; rows: number } | null = null;
  let extent = await readHerdrPaneExtent(target.sessionName, paneId, target.sshHost);

  const stopObserver = () => {
    const stoppingObserver = observer;
    /** 먼저 비워야 곧이어 오는 종료 이벤트가 스트림의 죽음으로 읽히지 않는다 */
    observer = null;
    if (stoppingObserver) {
      stopMirrorProcess(stoppingObserver);
    }
  };

  const stopStream = () => {
    isStopped = true;
    observerGeneration += 1;
    if (quietTimer) {
      clearTimeout(quietTimer);
    }
    stopObserver();
  };

  /** 끝을 알리면 구독 자리가 먼저 지워져 해제 함수가 `stop`을 부르지 않으므로, 알리기 전에 스스로 멈춘다 */
  const endStream = () => {
    stopStream();
    onDead();
  };

  const handleRecord =(child: ChildProcess, line: string) => {
    const record = parseHerdrObserveRecord(line);
    if (record?.type !== "frame" || child !== observer) {
      return;
    }
    broadcast(record.output);
    /** 전체 프레임은 observe가 시작하며 보내는 것이라 새 출력이 아니다. 여기서도 예약하면 재시작이 끝없이 반복된다 */
    if (!record.isFullScreen) {
      scheduleQuietCheck();
    }
  };

  const startObserver = async () => {
    const generation = ++observerGeneration;
    const pane = (await listHerdrMirrorPanes(target.sessionName, target.sshHost)).find((candidate) => candidate.id === paneId);
    if (isStopped || generation !== observerGeneration) return;
    if (!pane) {
      throw new SurfaceUnavailableError("session-not-running");
    }
    const child = await spawnMirrorProcess(
      buildHerdrObservePaneCommand(target.sessionName, paneId, viewportSize?.cols ?? pane.width, viewportSize?.rows ?? pane.height),
      target.sshHost,
    );
    if (isStopped || generation !== observerGeneration) {
      stopMirrorProcess(child);
      return;
    }
    /** 재시작이 겹치면 먼저 뜬 observe가 주인 없이 남으므로 항상 하나만 남긴다 */
    stopObserver();
    observer = child;

    let partialLine = "";
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      const lines = `${partialLine}${chunk}`.split("\n");
      partialLine = lines.pop() ?? "";
      lines.forEach((line) => handleRecord(child, line));
    });
    child.stderr?.resume();

    const handleObserverEnd = () => {
      if (child === observer) {
        endStream();
      }
    };
    child.on("error", (error) => {
      console.error("[kanvibe] Herdr observe failed:", error);
      handleObserverEnd();
    });
    child.on("exit", handleObserverEnd);
  };

  const restartObserver = async () => {
    stopObserver();
    await startObserver();
  };

  const failOnRestartError = (error: unknown) => {
    console.error("[kanvibe] Herdr observe restart failed:", error);
    if (!isStopped) {
      endStream();
    }
  };

  const refreshHistoryWhenGrown = async () => {
    if (isRefreshing) {
      return;
    }
    isRefreshing = true;
    try {
      const nextExtent = await readHerdrPaneExtent(target.sessionName, paneId, target.sshHost);
      if (isStopped || (nextExtent.historyRows === extent.historyRows && nextExtent.viewportRows === extent.viewportRows)) {
        return;
      }
      extent = nextExtent;
      stopObserver();
      broadcast(await readPaneSnapshot(target, paneId));
      if (!isStopped) {
        await startObserver();
      }
    } catch (error) {
      failOnRestartError(error);
    } finally {
      isRefreshing = false;
    }
  };

  function scheduleQuietCheck() {
    if (quietTimer) {
      clearTimeout(quietTimer);
    }
    quietTimer = setTimeout(() => void refreshHistoryWhenGrown(), quietPeriodMs);
  }

  await startObserver();

  return {
    stop: stopStream,
    setViewport: async (size) => {
      viewportSize = size;
      if (!isStopped) await restartObserver();
    },
    /** 뒤늦게 붙은 구독자에게 전체 화면을 주려면 observe를 새로 띄우는 수밖에 없다. 이미 보던 기기에는 같은 화면이 한 번 더 그려질 뿐이다 */
    resendScreen: () => {
      restartObserver().catch(failOnRestartError);
    },
  };
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

export async function readHerdrPaneFrameForChat(taskId: string, paneId: string): Promise<{ output: string; width: number; height: number }> {
  const target = await findMirrorSessionTarget(taskId);
  if (target.sessionType !== SessionType.HERDR) throw new Error("Herdr pane이 아닙니다.");
  const pane = (await listMirrorPanes(target)).find((entry) => entry.id === paneId);
  if (!pane) throw new PaneNotFoundError();
  const child = await spawnMirrorProcess(buildHerdrObservePaneCommand(target.sessionName, paneId, pane.width, pane.height), target.sshHost);
  return new Promise((resolve, reject) => {
    let settled = false;
    let buffered = "";
    const finish = (error?: Error, value?: { output: string; width: number; height: number }) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      stopMirrorProcess(child);
      if (error) reject(error); else resolve(value!);
    };
    const timer = setTimeout(() => finish(new Error("Herdr 화면 응답 시간 초과")), MIRROR_COMMAND_TIMEOUT_MS);
    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      buffered += chunk;
      if (Buffer.byteLength(buffered, "utf8") > MAX_CHAT_OBSERVE_FRAME_BYTES) { finish(new Error("Herdr 화면 크기 초과")); return; }
      const lines = buffered.split("\n");
      buffered = lines.pop() ?? "";
      for (const line of lines) {
        const record = parseHerdrObserveRecord(line);
        if (record?.type === "frame" && record.isFullScreen) {
          finish(undefined, { output: record.output, width: record.width ?? pane.width, height: record.height ?? pane.height });
          break;
        }
      }
    });
    child.stderr?.resume();
    child.once("error", (error) => finish(error));
    child.once("exit", () => finish(new Error("Herdr 화면 연결이 종료됐습니다.")));
  });
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

  let command: string;
  if (target.sessionType === SessionType.HERDR) {
    command = buildHerdrWritePaneCommand(target.sessionName, paneId, input);
  } else if (target.sessionType === SessionType.ZELLIJ) {
    command = buildZellijWritePaneCommand(target.sessionName, paneId, input);
  } else {
    command = buildTmuxSendKeysCommand(paneId, input);
  }

  await runMirrorCommand(command, target.sshHost);
}


export async function executeMobileTerminalShortcut(
  taskId: string,
  paneId: string,
  action: string,
): Promise<{ surfaces: TaskSurfaces; screen?: string }> {
  if (!buildTmuxPaneShortcutCommand(paneId, action) && !buildZellijPaneShortcutCommand("", paneId, "", action)) {
    throw new MobileTaskInputError("지원하지 않는 단축키입니다");
  }
  const target = await findMirrorSessionTarget(taskId);
  if (target.sessionType === SessionType.TERMINAL) {
    throw new MobileTaskInputError("일반 터미널에서는 세션 제어 단축키를 사용할 수 없습니다");
  }
  const panes = await listMirrorPanes(target);
  const pane = panes.find((candidate) => candidate.id === paneId);
  if (!pane) throw new SurfaceUnavailableError("session-not-running");
  let command: string | null;
  if (target.sessionType === SessionType.HERDR) {
    command = buildHerdrPaneShortcutCommand(target.sessionName, paneId, action);
  } else if (target.sessionType === SessionType.ZELLIJ) {
    command = buildZellijPaneShortcutCommand(target.sessionName, paneId, pane.tabId, action);
  } else {
    command = buildTmuxPaneShortcutCommand(paneId, action);
  }
  if (!command) throw new MobileTaskInputError("이 세션에서 지원하지 않는 단축키입니다");
  await runMirrorCommand(command, target.sshHost);

  let screen: string | undefined;
  if (target.sessionType === SessionType.TMUX && (action === "copy-mode" || action.startsWith("scroll-"))) {
    // pipe-pane은 프로그램 출력만 전하므로 복사 모드의 화면은 별도로 떠서 모바일에 돌려준다.
    const capture = action === "scroll-bottom"
      ? buildTmuxCapturePaneCommand(paneId)
      : `tmux capture-pane -peM -t ${quoteForPosixShell(paneId)}`;
    screen = buildScreenRedraw(await runMirrorCommand(capture, target.sshHost), null);
  }
  let surfaces = await getTaskSurfaces(taskId);
  if (action === "split-right" || action === "split-down") {
    const previousPaneIds = new Set(panes.map((previous) => previous.id));
    const deadline = Date.now() + SESSION_START_TIMEOUT_MS;
    while (!surfaces.tabs.some((tab) => tab.id === pane.tabId && tab.panes.some((created) => !previousPaneIds.has(created.id)))) {
      if (Date.now() >= deadline) throw new Error("추가한 pane을 찾을 수 없습니다.");
      await new Promise((resolve) => setTimeout(resolve, SESSION_START_POLL_INTERVAL_MS));
      surfaces = await getTaskSurfaces(taskId);
    }
  }
  return { surfaces, ...(screen === undefined ? {} : { screen }) };
}
