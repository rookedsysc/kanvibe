import {
  SHORTCUTS,
  TASK_DETAIL_DOCK_SHORTCUT_INDEXES,
  TERMINAL_TAB_SHORTCUT_INDEXES,
  canonicalizeShortcutForPlatform,
  createTaskDetailDockShortcut,
  createTerminalTabShortcut,
  matchElectronShortcutInput,
  matchShortcutEvent,
  type ElectronShortcutInput,
  type ShortcutPlatformInput,
  type TaskDetailDockShortcutIndex,
  type TerminalTabShortcutIndex,
} from "@/desktop/shared/keyboardShortcut";
import { TASK_DETAIL_DOCK_ITEM_IDS, type TaskDetailDockItemId } from "@/desktop/shared/taskDetailDock";
import type { TerminalTabShortcutCommand } from "@/desktop/shared/terminalTabs";

/**
 * 사용자가 다시 배정할 수 있는 단축키 명령의 식별자.
 * dock과 터미널 탭은 번호로만 구분되므로 번호 배열에서 식별자를 파생시킨다.
 */
export type TaskDetailDockCommandId = `taskDetailDock${TaskDetailDockShortcutIndex}`;
export type TerminalTabCommandId = `terminalTab${TerminalTabShortcutIndex}`;

export type ShortcutCommandId =
  | TaskDetailDockCommandId
  | TerminalTabCommandId
  | "taskDetailUsage"
  | "taskSearch"
  | "boardNotification"
  | "boardProjectFilter"
  | "commandPalette"
  | "boardPageFind"
  | "createTask"
  | "newWindow"
  | "pageBack"
  | "pageForward"
  | "terminalTabNew"
  | "terminalTabClose"
  | "terminalWindowClose"
  | "terminalTabPrevious"
  | "terminalTabNext";

/** 설정 화면에서 명령을 묶어 보여 주는 단위 */
export type ShortcutCommandGroup = "taskDetailDock" | "taskDetail" | "board" | "terminal";

export interface ShortcutCommandDefinition {
  id: ShortcutCommandId;
  group: ShortcutCommandGroup;
  defaultShortcut: string;
  /** messages의 `settings.shortcuts.commands` 아래 라벨 키 */
  labelKey: string;
  /** 번호로만 구분되는 명령이 라벨에 끼워 넣을 숫자 */
  labelIndex?: number;
  /** 도크 명령이 가리키는 항목. 설정 화면이 번호 대신 항목 이름을 보여 주는 근거다 */
  dockItemId?: TaskDetailDockItemId;
}

export function createTaskDetailDockCommandId(index: TaskDetailDockShortcutIndex): TaskDetailDockCommandId {
  return `taskDetailDock${index}`;
}

export function createTerminalTabCommandId(index: TerminalTabShortcutIndex): TerminalTabCommandId {
  return `terminalTab${index}`;
}

const TASK_DETAIL_DOCK_COMMAND_PATTERN = /^taskDetailDock([1-9])$/;
const TERMINAL_TAB_COMMAND_PATTERN = /^terminalTab([1-9])$/;

/** dock 명령이면 그 dock 자리 번호를, 아니면 null을 준다 */
export function getTaskDetailDockIndexForCommand(commandId: ShortcutCommandId | null): number | null {
  const matchedCommand = commandId ? TASK_DETAIL_DOCK_COMMAND_PATTERN.exec(commandId) : null;
  return matchedCommand ? Number(matchedCommand[1]) : null;
}

/** 터미널 탭 이동 명령이면 그 탭 자리 번호를, 아니면 null을 준다 */
export function getTerminalTabPositionForCommand(commandId: ShortcutCommandId | null): number | null {
  const matchedCommand = commandId ? TERMINAL_TAB_COMMAND_PATTERN.exec(commandId) : null;
  return matchedCommand ? Number(matchedCommand[1]) : null;
}

export const SHORTCUT_COMMAND_DEFINITIONS: readonly ShortcutCommandDefinition[] = [
  ...TASK_DETAIL_DOCK_SHORTCUT_INDEXES.map<ShortcutCommandDefinition>((dockIndex) => ({
    id: createTaskDetailDockCommandId(dockIndex),
    group: "taskDetailDock",
    defaultShortcut: createTaskDetailDockShortcut(dockIndex),
    labelKey: "taskDetailDock",
    labelIndex: dockIndex,
    dockItemId: TASK_DETAIL_DOCK_ITEM_IDS[dockIndex - 1],
  })),
  { id: "taskDetailUsage", group: "taskDetail", defaultShortcut: SHORTCUTS.taskDetailUsage, labelKey: "taskDetailUsage" },
  { id: "taskSearch", group: "board", defaultShortcut: SHORTCUTS.taskSearchDefault, labelKey: "taskSearch" },
  { id: "boardNotification", group: "board", defaultShortcut: SHORTCUTS.boardNotification, labelKey: "boardNotification" },
  { id: "boardProjectFilter", group: "board", defaultShortcut: SHORTCUTS.boardProjectFilter, labelKey: "boardProjectFilter" },
  { id: "commandPalette", group: "board", defaultShortcut: SHORTCUTS.commandPalette, labelKey: "commandPalette" },
  { id: "boardPageFind", group: "board", defaultShortcut: SHORTCUTS.boardPageFind, labelKey: "boardPageFind" },
  { id: "createTask", group: "board", defaultShortcut: SHORTCUTS.createTask, labelKey: "createTask" },
  { id: "newWindow", group: "board", defaultShortcut: SHORTCUTS.newWindow, labelKey: "newWindow" },
  { id: "pageBack", group: "board", defaultShortcut: SHORTCUTS.pageBack, labelKey: "pageBack" },
  { id: "pageForward", group: "board", defaultShortcut: SHORTCUTS.pageForward, labelKey: "pageForward" },
  { id: "terminalTabNew", group: "terminal", defaultShortcut: SHORTCUTS.terminalTabNew, labelKey: "terminalTabNew" },
  { id: "terminalTabClose", group: "terminal", defaultShortcut: SHORTCUTS.terminalTabClose, labelKey: "terminalTabClose" },
  { id: "terminalWindowClose", group: "terminal", defaultShortcut: SHORTCUTS.terminalWindowClose, labelKey: "terminalWindowClose" },
  { id: "terminalTabPrevious", group: "terminal", defaultShortcut: SHORTCUTS.terminalTabPrevious, labelKey: "terminalTabPrevious" },
  { id: "terminalTabNext", group: "terminal", defaultShortcut: SHORTCUTS.terminalTabNext, labelKey: "terminalTabNext" },
  ...TERMINAL_TAB_SHORTCUT_INDEXES.map<ShortcutCommandDefinition>((tabIndex) => ({
    id: createTerminalTabCommandId(tabIndex),
    group: "terminal",
    defaultShortcut: createTerminalTabShortcut(tabIndex),
    labelKey: "terminalTab",
    labelIndex: tabIndex,
  })),
];

export type ShortcutBindings = Record<ShortcutCommandId, string>;

const SHORTCUT_COMMAND_IDS = new Set<string>(SHORTCUT_COMMAND_DEFINITIONS.map((definition) => definition.id));

export const DEFAULT_SHORTCUT_BINDINGS: ShortcutBindings = Object.freeze(
  Object.fromEntries(SHORTCUT_COMMAND_DEFINITIONS.map((definition) => [definition.id, definition.defaultShortcut])),
) as ShortcutBindings;

function isShortcutCommandId(candidateId: string): candidateId is ShortcutCommandId {
  return SHORTCUT_COMMAND_IDS.has(candidateId);
}

/**
 * 저장된 재배정 값을 읽는다. 모르는 명령과 빈 값은 버려서 기본값이 그대로 남게 한다.
 * 저장 형식이 깨져 있어도 앱이 단축키를 통째로 잃지 않아야 하므로 파싱 실패는 빈 재배정으로 본다.
 * 옛 버전이 남긴 `Meta+…`/`Ctrl+…` 표기는 읽는 순간 canonical 형태로 접어, 이후 비교가 전부 한 기준을 쓰게 한다.
 */
export function parseShortcutOverrides(
  rawValue: string | null | undefined,
  platform: ShortcutPlatformInput,
): Partial<ShortcutBindings> {
  if (!rawValue) {
    return {};
  }

  let parsedValue: unknown;
  try {
    parsedValue = JSON.parse(rawValue);
  } catch {
    return {};
  }

  if (!parsedValue || typeof parsedValue !== "object" || Array.isArray(parsedValue)) {
    return {};
  }

  const overrides: Partial<ShortcutBindings> = {};
  for (const [commandId, shortcut] of Object.entries(parsedValue as Record<string, unknown>)) {
    if (typeof shortcut !== "string" || !isShortcutCommandId(commandId)) {
      continue;
    }

    const canonicalShortcut = canonicalizeShortcutForPlatform(shortcut, platform);
    if (canonicalShortcut) {
      overrides[commandId] = canonicalShortcut;
    }
  }

  return overrides;
}

/** 기본값 위에 재배정을 얹은 최종 단축키 표 */
export function resolveShortcutBindings(overrides: Partial<ShortcutBindings>): ShortcutBindings {
  return { ...DEFAULT_SHORTCUT_BINDINGS, ...overrides };
}

/** 기본값과 같아진 재배정은 저장하지 않는다. 나중에 기본값을 바꿔도 옛 값이 발목을 잡지 않게 한다 */
export function collectShortcutOverrides(
  bindings: ShortcutBindings,
  platform: ShortcutPlatformInput,
): Partial<ShortcutBindings> {
  const overrides: Partial<ShortcutBindings> = {};
  for (const definition of SHORTCUT_COMMAND_DEFINITIONS) {
    const canonicalShortcut = canonicalizeShortcutForPlatform(bindings[definition.id] ?? "", platform);
    const canonicalDefault = canonicalizeShortcutForPlatform(definition.defaultShortcut, platform);
    if (canonicalShortcut && canonicalShortcut !== canonicalDefault) {
      overrides[definition.id] = canonicalShortcut;
    }
  }

  return overrides;
}

/** 이미 같은 조합을 쓰고 있는 다른 명령. 없으면 null */
export function findShortcutCommandConflict(
  bindings: ShortcutBindings,
  commandId: ShortcutCommandId,
  shortcut: string,
  platform: ShortcutPlatformInput,
): ShortcutCommandId | null {
  const canonicalShortcut = canonicalizeShortcutForPlatform(shortcut, platform);

  for (const definition of SHORTCUT_COMMAND_DEFINITIONS) {
    if (definition.id === commandId) {
      continue;
    }

    if (canonicalizeShortcutForPlatform(bindings[definition.id] ?? "", platform) === canonicalShortcut) {
      return definition.id;
    }
  }

  return null;
}

function findShortcutCommand(
  bindings: ShortcutBindings,
  matchesShortcut: (shortcut: string) => boolean,
): ShortcutCommandId | null {
  for (const definition of SHORTCUT_COMMAND_DEFINITIONS) {
    if (matchesShortcut(bindings[definition.id])) {
      return definition.id;
    }
  }

  return null;
}

/** 렌더러 keydown이 어떤 명령인지 판정한다 */
export function findShortcutCommandForEvent(
  bindings: ShortcutBindings,
  event: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">,
  platform: ShortcutPlatformInput,
): ShortcutCommandId | null {
  return findShortcutCommand(bindings, (shortcut) => matchShortcutEvent(event, shortcut, platform));
}

/** Electron `before-input-event` 입력이 어떤 명령인지 판정한다 */
export function findShortcutCommandForElectronInput(
  bindings: ShortcutBindings,
  input: ElectronShortcutInput,
  platform: ShortcutPlatformInput,
): ShortcutCommandId | null {
  return findShortcutCommand(bindings, (shortcut) => matchElectronShortcutInput(input, shortcut, platform));
}

/**
 * 단축키 명령을 터미널 탭 명령으로 바꾼다. 해당 없으면 null.
 * 창 닫기는 어느 화면에서나 동작하고, 나머지는 터미널이 있는 태스크 상세에서만 의미가 있다.
 * 탭 닫기는 터미널 밖에서는 일반 앱처럼 창 닫기로 동작한다.
 */
export function resolveTerminalTabCommand(
  commandId: ShortcutCommandId | null,
  isTaskDetailRoute: boolean,
): TerminalTabShortcutCommand | null {
  if (commandId === "terminalWindowClose") {
    return { type: "close-window" };
  }

  if (commandId === "terminalTabClose") {
    return isTaskDetailRoute ? { type: "close-tab" } : { type: "close-window" };
  }

  if (!isTaskDetailRoute) {
    return null;
  }

  if (commandId === "terminalTabNew") {
    return { type: "new-tab" };
  }

  if (commandId === "terminalTabPrevious") {
    return { type: "previous-tab" };
  }

  if (commandId === "terminalTabNext") {
    return { type: "next-tab" };
  }

  const tabPosition = getTerminalTabPositionForCommand(commandId);
  return tabPosition === null ? null : { type: "go-to-tab", position: tabPosition };
}
