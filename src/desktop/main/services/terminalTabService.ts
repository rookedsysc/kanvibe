import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import { getTaskRepository } from "@/lib/database";
import { execGit } from "@/lib/gitOperations";
import {
  closeLocalTerminalTab,
  createLocalTerminalTab,
  listLocalTerminalTabs,
  moveLocalTerminalTab,
  renameLocalTerminalTab,
  selectLocalTerminalTab,
} from "@/lib/terminal";
import {
  buildTmuxKillWindowCommand,
  buildTmuxListWindowsCommand,
  buildTmuxMoveWindowCommands,
  buildTmuxNewWindowCommand,
  buildTmuxRenameWindowCommand,
  buildTmuxSelectWindowCommand,
  buildZellijCloseFocusedTabCommands,
  buildZellijCloseTabByIdCommand,
  buildZellijDumpLayoutCommand,
  buildZellijGoToTabByIdCommand,
  buildZellijGoToTabByNameCommand,
  buildZellijListTabsCommand,
  buildZellijMoveTabByIdCommands,
  buildZellijMoveTabCommands,
  buildZellijNewTabCommand,
  buildZellijQueryTabNamesCommand,
  buildZellijRenameFocusedTabCommands,
  buildZellijRenameTabByIdCommand,
  buildZellijVersionCommand,
  parseTmuxWindowList,
  parseZellijTabList,
  parseZellijTabNamesWithFocus,
  supportsZellijTabIdCommands,
} from "@/lib/terminalTabs";
import type {
  TerminalTab,
  TerminalTabListResult,
  TerminalTabMutationResult,
} from "@/desktop/shared/terminalTabs";

/**
 * KanVibe 탭 바가 부르는 진입점.
 * tmux와 zellij는 멀티플렉서 CLI를 실행해 탭을 읽고 바꾸며,
 * terminal 세션은 `src/lib/terminal.ts`의 탭 레지스트리에 그대로 위임한다.
 */

/** 탭 명령은 짧게 끝나야 폴링이 밀리지 않는다 */
const TAB_COMMAND_TIMEOUT_MS = 5_000;

/** zellij 버전은 세션이 사는 동안 바뀌지 않으므로 호스트별로 한 번만 확인한다 */
const zellijTabIdSupportByHost = new Map<string, boolean>();

interface TerminalSessionTarget {
  sessionType: SessionType;
  sessionName: string;
  sshHost: string | null;
  worktreePath: string | null;
}

async function findTerminalSessionTarget(taskId: string): Promise<TerminalSessionTarget | null> {
  const taskRepo = await getTaskRepository();
  const task: KanbanTask | null = await taskRepo.findOneBy({ id: taskId });

  if (!task?.sessionType || !task.sessionName) {
    return null;
  }

  return {
    sessionType: task.sessionType,
    sessionName: task.sessionName,
    sshHost: task.sshHost,
    worktreePath: task.worktreePath,
  };
}

function runTabCommand(command: string, sshHost: string | null): Promise<string> {
  return execGit(command, sshHost, { timeoutMs: TAB_COMMAND_TIMEOUT_MS });
}

async function runTabCommands(commands: string[], sshHost: string | null): Promise<void> {
  for (const command of commands) {
    await runTabCommand(command, sshHost);
  }
}

async function hasZellijTabIdSupport(sshHost: string | null): Promise<boolean> {
  const hostKey = sshHost ?? "local";
  const cachedSupport = zellijTabIdSupportByHost.get(hostKey);
  if (cachedSupport !== undefined) {
    return cachedSupport;
  }

  let isSupported = false;
  try {
    isSupported = supportsZellijTabIdCommands(await runTabCommand(buildZellijVersionCommand(), sshHost));
  } catch {
    /** 버전을 못 읽으면 기능이 적은 구버전 경로로 간다 */
  }

  zellijTabIdSupportByHost.set(hostKey, isSupported);
  return isSupported;
}

async function readZellijTabs(target: TerminalSessionTarget): Promise<TerminalTab[]> {
  if (await hasZellijTabIdSupport(target.sshHost)) {
    return parseZellijTabList(
      await runTabCommand(buildZellijListTabsCommand(target.sessionName), target.sshHost),
    );
  }

  const [tabNamesOutput, layoutOutput] = await Promise.all([
    runTabCommand(buildZellijQueryTabNamesCommand(target.sessionName), target.sshHost),
    runTabCommand(buildZellijDumpLayoutCommand(target.sessionName), target.sshHost).catch(() => ""),
  ]);

  return parseZellijTabNamesWithFocus(tabNamesOutput, layoutOutput);
}

async function readTabs(target: TerminalSessionTarget): Promise<TerminalTab[]> {
  if (target.sessionType === SessionType.TMUX) {
    return parseTmuxWindowList(
      await runTabCommand(buildTmuxListWindowsCommand(target.sessionName), target.sshHost),
    );
  }

  return readZellijTabs(target);
}

function findTabName(tabs: TerminalTab[], tabId: string): string | null {
  return tabs.find((tab) => tab.id === tabId)?.name ?? null;
}

function toFailure(error: unknown): TerminalTabMutationResult {
  return { ok: false, error: error instanceof Error ? error.message : "터미널 탭 명령 실패" };
}

export async function listTerminalTabs(taskId: string): Promise<TerminalTabListResult> {
  const target = await findTerminalSessionTarget(taskId);
  if (!target) {
    return { ok: false, tabs: [], error: "작업에 연결된 세션이 없습니다." };
  }

  if (target.sessionType === SessionType.TERMINAL) {
    return { ok: true, tabs: listLocalTerminalTabs(taskId) };
  }

  try {
    return { ok: true, tabs: await readTabs(target) };
  } catch (error) {
    return {
      ok: false,
      tabs: [],
      error: error instanceof Error ? error.message : "터미널 탭 목록을 읽지 못했습니다.",
    };
  }
}

export async function createTerminalTab(taskId: string): Promise<TerminalTabMutationResult> {
  const target = await findTerminalSessionTarget(taskId);
  if (!target) {
    return { ok: false, error: "작업에 연결된 세션이 없습니다." };
  }

  if (target.sessionType === SessionType.TERMINAL) {
    createLocalTerminalTab(taskId);
    return { ok: true };
  }

  try {
    const createCommand = target.sessionType === SessionType.TMUX
      ? buildTmuxNewWindowCommand(target.sessionName, target.worktreePath)
      : buildZellijNewTabCommand(target.sessionName, target.worktreePath);

    await runTabCommand(createCommand, target.sshHost);
    return { ok: true };
  } catch (error) {
    return toFailure(error);
  }
}

export async function selectTerminalTab(
  taskId: string,
  tabId: string,
): Promise<TerminalTabMutationResult> {
  const target = await findTerminalSessionTarget(taskId);
  if (!target) {
    return { ok: false, error: "작업에 연결된 세션이 없습니다." };
  }

  if (target.sessionType === SessionType.TERMINAL) {
    selectLocalTerminalTab(taskId, tabId);
    return { ok: true };
  }

  try {
    if (target.sessionType === SessionType.TMUX) {
      await runTabCommand(buildTmuxSelectWindowCommand(tabId), target.sshHost);
      return { ok: true };
    }

    await runTabCommand(await buildZellijSelectCommand(target, tabId), target.sshHost);
    return { ok: true };
  } catch (error) {
    return toFailure(error);
  }
}

async function buildZellijSelectCommand(
  target: TerminalSessionTarget,
  tabId: string,
): Promise<string> {
  if (await hasZellijTabIdSupport(target.sshHost)) {
    return buildZellijGoToTabByIdCommand(target.sessionName, tabId);
  }

  const tabName = findTabName(await readZellijTabs(target), tabId);
  if (!tabName) {
    throw new Error("탭을 찾을 수 없습니다.");
  }

  return buildZellijGoToTabByNameCommand(target.sessionName, tabName);
}

export async function closeTerminalTab(
  taskId: string,
  tabId: string,
): Promise<TerminalTabMutationResult> {
  const target = await findTerminalSessionTarget(taskId);
  if (!target) {
    return { ok: false, error: "작업에 연결된 세션이 없습니다." };
  }

  if (target.sessionType === SessionType.TERMINAL) {
    return { ok: true, remainingCount: closeLocalTerminalTab(taskId, tabId) };
  }

  try {
    if (target.sessionType === SessionType.TMUX) {
      await runTabCommand(buildTmuxKillWindowCommand(tabId), target.sshHost);
    } else if (await hasZellijTabIdSupport(target.sshHost)) {
      await runTabCommand(buildZellijCloseTabByIdCommand(target.sessionName, tabId), target.sshHost);
    } else {
      const tabName = findTabName(await readZellijTabs(target), tabId);
      if (!tabName) {
        return { ok: false, error: "탭을 찾을 수 없습니다." };
      }
      await runTabCommands(
        buildZellijCloseFocusedTabCommands(target.sessionName, tabName),
        target.sshHost,
      );
    }

    /**
     * 마지막 탭을 닫으면 멀티플렉서 세션 자체가 사라져 목록 조회도 실패한다.
     * 그 실패를 "남은 탭 0개"로 읽어 호출자가 창을 닫게 한다.
     */
    const remainingTabs = await readTabs(target).catch(() => []);
    return { ok: true, remainingCount: remainingTabs.length };
  } catch (error) {
    return toFailure(error);
  }
}

export async function renameTerminalTab(
  taskId: string,
  tabId: string,
  name: string,
): Promise<TerminalTabMutationResult> {
  const trimmedName = name.trim();
  if (!trimmedName) {
    return { ok: false, error: "탭 이름은 비워 둘 수 없습니다." };
  }

  const target = await findTerminalSessionTarget(taskId);
  if (!target) {
    return { ok: false, error: "작업에 연결된 세션이 없습니다." };
  }

  if (target.sessionType === SessionType.TERMINAL) {
    renameLocalTerminalTab(taskId, tabId, trimmedName);
    return { ok: true };
  }

  try {
    if (target.sessionType === SessionType.TMUX) {
      await runTabCommand(buildTmuxRenameWindowCommand(tabId, trimmedName), target.sshHost);
      return { ok: true };
    }

    if (await hasZellijTabIdSupport(target.sshHost)) {
      await runTabCommand(
        buildZellijRenameTabByIdCommand(target.sessionName, tabId, trimmedName),
        target.sshHost,
      );
      return { ok: true };
    }

    const tabName = findTabName(await readZellijTabs(target), tabId);
    if (!tabName) {
      return { ok: false, error: "탭을 찾을 수 없습니다." };
    }

    await runTabCommands(
      buildZellijRenameFocusedTabCommands(target.sessionName, tabName, trimmedName),
      target.sshHost,
    );
    return { ok: true };
  } catch (error) {
    return toFailure(error);
  }
}

export async function moveTerminalTab(
  taskId: string,
  tabId: string,
  targetIndex: number,
): Promise<TerminalTabMutationResult> {
  if (!Number.isInteger(targetIndex) || targetIndex < 0) {
    return { ok: false, error: "이동할 위치가 올바르지 않습니다." };
  }

  const target = await findTerminalSessionTarget(taskId);
  if (!target) {
    return { ok: false, error: "작업에 연결된 세션이 없습니다." };
  }

  if (target.sessionType === SessionType.TERMINAL) {
    moveLocalTerminalTab(taskId, tabId, targetIndex);
    return { ok: true };
  }

  try {
    /**
     * 두 멀티플렉서 모두 "몇 번째로 옮겨라"를 그대로 받지 못한다.
     * tmux는 목표 window의 앞뒤 중 어디에 끼울지, zellij는 어느 방향으로 몇 칸 밀지 정해야 해서
     * 현재 목록을 먼저 읽는다. 탭 바가 넘기는 값은 배열 위치이므로 실제 대상도 여기서 고른다.
     */
    const currentTabs = await readTabs(target);
    const currentTab = currentTabs.find((tab) => tab.id === tabId);
    if (!currentTab) {
      return { ok: false, error: "탭을 찾을 수 없습니다." };
    }

    const boundedTargetIndex = Math.min(targetIndex, currentTabs.length - 1);
    const destinationTab = currentTabs[boundedTargetIndex];

    if (target.sessionType === SessionType.TMUX) {
      await runTabCommands(
        buildTmuxMoveWindowCommands(
          target.sessionName,
          tabId,
          currentTab.index,
          destinationTab.index,
        ),
        target.sshHost,
      );
      return { ok: true };
    }

    const moveCommands = await hasZellijTabIdSupport(target.sshHost)
      ? buildZellijMoveTabByIdCommands(target.sessionName, tabId, currentTab.index, boundedTargetIndex)
      : buildZellijMoveTabCommands(target.sessionName, currentTab.name, currentTab.index, boundedTargetIndex);

    await runTabCommands(moveCommands, target.sshHost);
    return { ok: true };
  } catch (error) {
    return toFailure(error);
  }
}
