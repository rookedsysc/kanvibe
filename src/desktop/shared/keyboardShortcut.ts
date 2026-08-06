import type { TerminalTabShortcutCommand } from "@/desktop/shared/terminalTabs";

export type ShortcutPlatform = "mac" | "linux";
export type ShortcutPlatformInput = ShortcutPlatform | boolean;
export type ShortcutDefinition = string | Record<ShortcutPlatform, string>;

type ShortcutInput = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

export interface ElectronShortcutInput {
  type?: string;
  isAutoRepeat?: boolean;
  key: string;
  meta?: boolean;
  control?: boolean;
  alt?: boolean;
  shift?: boolean;
}

export interface NavigatorShortcutSource {
  userAgent?: string;
  platform?: string;
  userAgentData?: {
    platform?: string;
  };
}

const MODIFIER_ORDER = ["Mod", "Meta", "Ctrl", "Alt", "Shift"] as const;
const MODIFIER_KEYS = new Set(["Meta", "Control", "Ctrl", "Alt", "Shift"]);

export const SHORTCUTS = {
  taskSearchDefault: "Mod+Shift+O",
  boardNotification: "Mod+Shift+I",
  boardProjectFilter: "Mod+Shift+P",
  createTask: "Mod+N",
  newWindow: "Mod+Shift+N",
  pageBack: "Mod+[",
  pageForward: "Mod+]",
  boardPageFind: "Mod+F",
  terminalTabNew: "Mod+T",
  terminalTabClose: "Mod+W",
  terminalWindowClose: "Mod+Shift+W",
  terminalTabPrevious: "Mod+Shift+[",
  terminalTabNext: "Mod+Shift+]",
} as const;

export const DESKTOP_SHORTCUTS = {
  notificationCenter: SHORTCUTS.boardNotification,
  createTask: SHORTCUTS.createTask,
  newWindow: SHORTCUTS.newWindow,
} as const;

export const BLOCKED_DESKTOP_SHORTCUTS = {
  reload: "Mod+R",
} as const;

export const DEFAULT_TASK_SEARCH_SHORTCUT = SHORTCUTS.taskSearchDefault;
export const TASK_DETAIL_DOCK_SHORTCUT_INDEXES = [1, 2, 3, 4, 5, 6, 7, 8, 9] as const;
export const TERMINAL_TAB_SHORTCUT_INDEXES = [1, 2, 3, 4, 5] as const;

export type TaskDetailDockShortcutIndex = typeof TASK_DETAIL_DOCK_SHORTCUT_INDEXES[number];
export type TerminalTabShortcutIndex = typeof TERMINAL_TAB_SHORTCUT_INDEXES[number];

function normalizeShortcutPlatform(platform: ShortcutPlatformInput): ShortcutPlatform {
  if (typeof platform === "boolean") {
    return platform ? "mac" : "linux";
  }

  return platform === "mac" ? "mac" : "linux";
}

function resolveShortcutForPlatform(
  shortcut: ShortcutDefinition,
  platform: ShortcutPlatformInput,
): string {
  if (typeof shortcut === "string") {
    return shortcut;
  }

  return shortcut[normalizeShortcutPlatform(platform)];
}

export function getShortcutPlatformFromNavigator(
  navigatorSource: NavigatorShortcutSource | null | undefined,
): ShortcutPlatform {
  const platformText = [
    navigatorSource?.userAgentData?.platform,
    navigatorSource?.platform,
    navigatorSource?.userAgent,
  ].filter(Boolean).join(" ");

  return /mac/i.test(platformText) ? "mac" : "linux";
}

export function getCurrentShortcutPlatform(): ShortcutPlatform {
  if (typeof navigator === "undefined") {
    return "linux";
  }

  return getShortcutPlatformFromNavigator(navigator as NavigatorShortcutSource);
}

export function getShortcutPlatformFromProcessPlatform(processPlatform: string): ShortcutPlatform {
  return processPlatform === "darwin" ? "mac" : "linux";
}

function normalizeModifierToken(token: string): string | null {
  const normalizedToken = token.trim().toLowerCase();

  switch (normalizedToken) {
    case "mod":
      return "Mod";
    case "meta":
    case "cmd":
    case "command":
      return "Meta";
    case "ctrl":
    case "control":
      return "Ctrl";
    case "alt":
    case "option":
      return "Alt";
    case "shift":
      return "Shift";
    default:
      return null;
  }
}

function normalizeKeyToken(token: string): string {
  const trimmedToken = token.trim();
  if (!trimmedToken) {
    return "";
  }

  if (trimmedToken.length === 1) {
    return trimmedToken.toUpperCase();
  }

  const lowerToken = trimmedToken.toLowerCase();

  switch (lowerToken) {
    case "esc":
      return "Escape";
    case "space":
      return "Space";
    default:
      return trimmedToken[0].toUpperCase() + trimmedToken.slice(1);
  }
}

function normalizeShortcutParts(shortcut: string) {
  const tokens = shortcut.split("+").map((token) => token.trim()).filter(Boolean);
  const modifiers = new Set<string>();
  let key = "";

  for (const token of tokens) {
    const normalizedModifier = normalizeModifierToken(token);
    if (normalizedModifier) {
      modifiers.add(normalizedModifier);
      continue;
    }

    key = normalizeKeyToken(token);
  }

  return {
    modifiers: MODIFIER_ORDER.filter((modifier) => modifiers.has(modifier)),
    key,
  };
}

/**
 * Shift를 누르면 브라우저가 대괄호를 중괄호로 보고한다.
 * 이 별칭이 없으면 `Mod+Shift+[`는 실제 키 입력에서 영영 매칭되지 않고,
 * `Mod+[`와 `Mod+Shift+[`를 서로 다른 명령에 배정할 수 없다.
 */
const SHIFTED_KEY_ALIASES: Record<string, string> = {
  "{": "[",
  "}": "]",
};

function normalizeEventKey(key: string): string {
  if (key === " ") {
    return "Space";
  }

  if (key.length === 1) {
    return (SHIFTED_KEY_ALIASES[key] ?? key).toUpperCase();
  }

  if (key === "Control") {
    return "Ctrl";
  }

  return normalizeKeyToken(key);
}

function getNormalizedModifierState(event: ShortcutInput) {
  return {
    metaKey: Boolean(event.metaKey),
    ctrlKey: Boolean(event.ctrlKey),
    altKey: Boolean(event.altKey),
    shiftKey: Boolean(event.shiftKey),
  };
}

export function formatShortcutForDisplay(shortcut: ShortcutDefinition, platform: ShortcutPlatformInput): string {
  const shortcutPlatform = normalizeShortcutPlatform(platform);
  const resolvedShortcut = resolveShortcutForPlatform(shortcut, shortcutPlatform);
  const { modifiers, key } = normalizeShortcutParts(resolvedShortcut);
  const displayParts: string[] = modifiers.map((modifier) => {
    if (modifier === "Mod") {
      return shortcutPlatform === "mac" ? "Cmd" : "Ctrl";
    }

    if (modifier === "Meta") {
      return shortcutPlatform === "mac" ? "Cmd" : "Meta";
    }

    return modifier;
  });

  if (key) {
    displayParts.push(key);
  }

  return displayParts.join("+");
}

export function matchShortcutEvent(
  event: ShortcutInput,
  shortcut: ShortcutDefinition,
  platform: ShortcutPlatformInput,
): boolean {
  const shortcutPlatform = normalizeShortcutPlatform(platform);
  const resolvedShortcut = resolveShortcutForPlatform(shortcut, shortcutPlatform);
  const { modifiers, key } = normalizeShortcutParts(resolvedShortcut);
  const normalizedKey = normalizeEventKey(event.key);

  if (!key || key !== normalizedKey) {
    return false;
  }

  const requiresMod = modifiers.includes("Mod");
  const requiresMeta = modifiers.includes("Meta");
  const requiresCtrl = modifiers.includes("Ctrl");
  const requiresAlt = modifiers.includes("Alt");
  const requiresShift = modifiers.includes("Shift");
  const expectedMeta = requiresMeta || (requiresMod && shortcutPlatform === "mac");
  const expectedCtrl = requiresCtrl || (requiresMod && shortcutPlatform === "linux");
  const modifierState = getNormalizedModifierState(event);

  return modifierState.metaKey === expectedMeta
    && modifierState.ctrlKey === expectedCtrl
    && modifierState.altKey === requiresAlt
    && modifierState.shiftKey === requiresShift;
}

export function matchElectronShortcutInput(
  input: ElectronShortcutInput,
  shortcut: ShortcutDefinition,
  platform: ShortcutPlatformInput,
): boolean {
  if (input.type !== "keyDown" || input.isAutoRepeat) {
    return false;
  }

  return matchShortcutEvent({
    key: input.key,
    metaKey: Boolean(input.meta),
    ctrlKey: Boolean(input.control),
    altKey: Boolean(input.alt),
    shiftKey: Boolean(input.shift),
  }, shortcut, platform);
}

export function createTaskDetailDockShortcut(index: TaskDetailDockShortcutIndex): ShortcutDefinition {
  return {
    mac: `Meta+${index}`,
    linux: `Alt+${index}`,
  };
}

export function matchTaskDetailDockShortcutEvent(
  event: ShortcutInput,
  platform: ShortcutPlatformInput,
): TaskDetailDockShortcutIndex | null {
  for (const shortcutIndex of TASK_DETAIL_DOCK_SHORTCUT_INDEXES) {
    if (matchShortcutEvent(event, createTaskDetailDockShortcut(shortcutIndex), platform)) {
      return shortcutIndex;
    }
  }

  return null;
}

export function matchTaskDetailDockShortcutInput(
  input: ElectronShortcutInput,
  platform: ShortcutPlatformInput,
): TaskDetailDockShortcutIndex | null {
  for (const shortcutIndex of TASK_DETAIL_DOCK_SHORTCUT_INDEXES) {
    if (matchElectronShortcutInput(input, createTaskDetailDockShortcut(shortcutIndex), platform)) {
      return shortcutIndex;
    }
  }

  return null;
}

/** 터미널 탭 n번으로 바로 이동하는 단축키. dock의 `Meta+숫자`/`Alt+숫자`와 Shift 유무로 구분된다 */
export function createTerminalTabShortcut(index: TerminalTabShortcutIndex): ShortcutDefinition {
  return `Mod+Shift+${index}`;
}

export function matchTerminalTabShortcutEvent(
  event: ShortcutInput,
  platform: ShortcutPlatformInput,
): TerminalTabShortcutIndex | null {
  for (const shortcutIndex of TERMINAL_TAB_SHORTCUT_INDEXES) {
    if (matchShortcutEvent(event, createTerminalTabShortcut(shortcutIndex), platform)) {
      return shortcutIndex;
    }
  }

  return null;
}

export function matchTerminalTabShortcutInput(
  input: ElectronShortcutInput,
  platform: ShortcutPlatformInput,
): TerminalTabShortcutIndex | null {
  for (const shortcutIndex of TERMINAL_TAB_SHORTCUT_INDEXES) {
    if (matchElectronShortcutInput(input, createTerminalTabShortcut(shortcutIndex), platform)) {
      return shortcutIndex;
    }
  }

  return null;
}

/**
 * 키 입력을 터미널 탭 명령으로 바꾼다. 해당 없으면 null.
 * 창 닫기는 어느 화면에서나 동작하고, 나머지는 터미널이 있는 태스크 상세에서만 의미가 있다.
 * 탭 닫기는 터미널 밖에서는 일반 앱처럼 창 닫기로 동작한다.
 *
 * 렌더러 이벤트와 Electron 입력이 같은 판정을 쓰도록 매칭 방법만 인자로 받는다.
 */
function resolveTerminalTabCommand(
  matchesShortcut: (shortcut: ShortcutDefinition) => boolean,
  matchTabPosition: () => TerminalTabShortcutIndex | null,
  isTaskDetailRoute: boolean,
): TerminalTabShortcutCommand | null {
  if (matchesShortcut(SHORTCUTS.terminalWindowClose)) {
    return { type: "close-window" };
  }

  if (matchesShortcut(SHORTCUTS.terminalTabClose)) {
    return isTaskDetailRoute ? { type: "close-tab" } : { type: "close-window" };
  }

  if (!isTaskDetailRoute) {
    return null;
  }

  if (matchesShortcut(SHORTCUTS.terminalTabNew)) {
    return { type: "new-tab" };
  }

  if (matchesShortcut(SHORTCUTS.terminalTabPrevious)) {
    return { type: "previous-tab" };
  }

  if (matchesShortcut(SHORTCUTS.terminalTabNext)) {
    return { type: "next-tab" };
  }

  const tabPosition = matchTabPosition();
  return tabPosition === null ? null : { type: "go-to-tab", position: tabPosition };
}

/** Electron `before-input-event` 입력을 터미널 탭 명령으로 바꾼다. 터미널이 입력을 먹기 전에 가로채는 경로다 */
export function resolveTerminalTabShortcutCommand(
  input: ElectronShortcutInput,
  platform: ShortcutPlatformInput,
  isTaskDetailRoute: boolean,
): TerminalTabShortcutCommand | null {
  return resolveTerminalTabCommand(
    (shortcut) => matchElectronShortcutInput(input, shortcut, platform),
    () => matchTerminalTabShortcutInput(input, platform),
    isTaskDetailRoute,
  );
}

/**
 * 렌더러 keydown을 터미널 탭 명령으로 바꾼다.
 * main이 가로채지 못한 입력을 렌더러가 받아 처리하는 두 번째 경로이며,
 * 다른 단축키들과 같은 이중 경로 구조를 따른다.
 */
export function resolveTerminalTabShortcutEvent(
  event: ShortcutInput,
  platform: ShortcutPlatformInput,
  isTaskDetailRoute: boolean,
): TerminalTabShortcutCommand | null {
  return resolveTerminalTabCommand(
    (shortcut) => matchShortcutEvent(event, shortcut, platform),
    () => matchTerminalTabShortcutEvent(event, platform),
    isTaskDetailRoute,
  );
}

export function isBlockedShortcutEvent(
  event: ShortcutInput,
  platform: ShortcutPlatformInput,
): boolean {
  return Object.values(BLOCKED_DESKTOP_SHORTCUTS).some((shortcut) => (
    matchShortcutEvent(event, shortcut, platform)
  ));
}

export function isBlockedElectronShortcutInput(
  input: ElectronShortcutInput,
  platform: ShortcutPlatformInput,
): boolean {
  return Object.values(BLOCKED_DESKTOP_SHORTCUTS).some((shortcut) => (
    matchElectronShortcutInput(input, shortcut, platform)
  ));
}

export function captureShortcutFromEvent(
  event: ShortcutInput,
  platform?: ShortcutPlatformInput,
): string | null {
  if (MODIFIER_KEYS.has(event.key)) {
    return null;
  }

  const modifiers = new Set<string>();

  if (platform === undefined) {
    if (event.metaKey) {
      modifiers.add("Meta");
    }

    if (event.ctrlKey) {
      modifiers.add("Ctrl");
    }
  } else {
    const shortcutPlatform = normalizeShortcutPlatform(platform);
    if (shortcutPlatform === "mac" && event.metaKey) {
      modifiers.add("Mod");
    } else if (event.metaKey) {
      modifiers.add("Meta");
    }

    if (shortcutPlatform === "linux" && event.ctrlKey) {
      modifiers.add("Mod");
    } else if (event.ctrlKey) {
      modifiers.add("Ctrl");
    }
  }

  if (event.altKey) {
    modifiers.add("Alt");
  }

  if (event.shiftKey) {
    modifiers.add("Shift");
  }

  if (modifiers.size === 0) {
    return null;
  }

  const key = normalizeEventKey(event.key);
  if (!key) {
    return null;
  }

  const shortcut = [...MODIFIER_ORDER.filter((modifier) => modifiers.has(modifier)), key].join("+");
  if (platform !== undefined && Object.values(BLOCKED_DESKTOP_SHORTCUTS).some((blockedShortcut) => (
    normalizeShortcutParts(blockedShortcut).key === key
    && matchShortcutEvent(event, blockedShortcut, platform)
  ))) {
    return null;
  }

  return shortcut;
}
