import { TASK_DETAIL_DOCK_ITEM_IDS } from "@/desktop/shared/taskDetailDock";

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
  boardProjectFilter: "Mod+Shift+F",
  commandPalette: "Mod+Shift+P",
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
  taskDetailUsage: "Mod+0",
} as const;

export const BLOCKED_DESKTOP_SHORTCUTS = {
  reload: "Mod+R",
} as const;

export const DEFAULT_TASK_SEARCH_SHORTCUT = SHORTCUTS.taskSearchDefault;
/**
 * 도크에 실제로 있는 자리만큼만 번호를 준다. 비어 있는 번호를 노출하면 설정 화면이
 * 아무것도 실행하지 않는 단축키를 보여 준다. `satisfies`가 도크 항목 수와의 어긋남을 컴파일에서 잡는다.
 */
export const TASK_DETAIL_DOCK_SHORTCUT_INDEXES = [1, 2, 3, 4, 5, 6] as const satisfies {
  length: typeof TASK_DETAIL_DOCK_ITEM_IDS["length"];
};
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

/**
 * 플랫폼에서 Mod가 가리키는 수식키(mac은 Meta, linux는 Ctrl)를 Mod 표기로 접는다.
 * matchShortcutEvent는 두 표기를 같은 입력으로 보므로 저장과 중복 판정도 같은 기준을 써야 한다.
 * 그러지 않으면 실제로 겹치는 조합이 서로 다른 명령에 조용히 배정되고, 정의 순서상 뒤인 명령은 도달할 방법이 없어진다.
 * mac의 Ctrl과 linux의 Meta는 Mod와 별개 키이므로 접지 않는다.
 */
export function canonicalizeShortcutForPlatform(
  shortcut: string,
  platform: ShortcutPlatformInput,
): string {
  const shortcutPlatform = normalizeShortcutPlatform(platform);
  const { modifiers, key } = normalizeShortcutParts(shortcut);
  if (!key) {
    return "";
  }

  const platformModifier = shortcutPlatform === "mac" ? "Meta" : "Ctrl";
  const canonicalModifiers = new Set(
    modifiers.map((modifier) => (modifier === platformModifier ? "Mod" : modifier)),
  );

  return [...MODIFIER_ORDER.filter((modifier) => canonicalModifiers.has(modifier)), key].join("+");
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

    /** macOS 키캡에는 Alt가 아니라 Option이 새겨져 있다 */
    if (modifier === "Alt") {
      return shortcutPlatform === "mac" ? "Option" : "Alt";
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

/** dock 항목 n번을 여는 기본 단축키 */
export function createTaskDetailDockShortcut(index: TaskDetailDockShortcutIndex): string {
  return `Mod+${index}`;
}

/**
 * 터미널 탭 n번으로 바로 이동하는 기본 단축키. `Mod+숫자`는 dock 항목이 쓰므로 Alt를 더해 비켜 간다.
 * macOS의 `Cmd+Shift+3~5`는 OS 스크린샷이 먼저 가져가 앱에 도달하지 않으므로 Shift로는 비켜 갈 수 없다.
 */
export function createTerminalTabShortcut(index: TerminalTabShortcutIndex): string {
  return `Mod+Alt+${index}`;
}

/**
 * 태스크 상세 화면인지 URL로 판정한다.
 * 같은 키가 화면에 따라 다른 명령이 되므로 main과 렌더러가 이 판정을 공유해야 한다.
 */
export function isTaskDetailRouteUrl(url: string | null | undefined): boolean {
  return /#\/[^/]+\/task\/[^/?#]+(?:[?#]|$)/.test(url || "");
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

/**
 * 수식키 단독 입력은 아직 조합이 완성되지 않은 상태다.
 * captureShortcutFromEvent가 이 경우와 "수식키가 하나도 없는 조합"에 똑같이 null을 돌려주므로,
 * 녹화 화면이 둘을 구분하려면 이 판별을 밖에서 쓸 수 있어야 한다.
 */
export function isShortcutModifierKey(key: string): boolean {
  return MODIFIER_KEYS.has(key);
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
  /**
   * `+`는 토큰 구분자라 키 자리에 담기지 못한다. 그대로 돌려주면 다시 파싱할 때 키가 통째로 사라져
   * 저장·매칭·표시가 전부 조용히 실패하고, 녹화 화면에는 배정된 것처럼 보인다.
   */
  if (normalizeShortcutParts(shortcut).key !== key) {
    return null;
  }

  if (platform !== undefined && Object.values(BLOCKED_DESKTOP_SHORTCUTS).some((blockedShortcut) => (
    normalizeShortcutParts(blockedShortcut).key === key
    && matchShortcutEvent(event, blockedShortcut, platform)
  ))) {
    return null;
  }

  return shortcut;
}
