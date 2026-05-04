export type ShortcutPlatform = "mac" | "linux";
export type ShortcutPlatformInput = ShortcutPlatform | boolean;

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
} as const;

export const DESKTOP_SHORTCUTS = {
  notificationCenter: SHORTCUTS.boardNotification,
  createTask: SHORTCUTS.createTask,
  newWindow: SHORTCUTS.newWindow,
} as const;

export const DEFAULT_TASK_SEARCH_SHORTCUT = SHORTCUTS.taskSearchDefault;

function normalizeShortcutPlatform(platform: ShortcutPlatformInput): ShortcutPlatform {
  if (typeof platform === "boolean") {
    return platform ? "mac" : "linux";
  }

  return platform === "mac" ? "mac" : "linux";
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

function normalizeEventKey(key: string): string {
  if (key === " ") {
    return "Space";
  }

  if (key.length === 1) {
    return key.toUpperCase();
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

export function formatShortcutForDisplay(shortcut: string, platform: ShortcutPlatformInput): string {
  const shortcutPlatform = normalizeShortcutPlatform(platform);
  const { modifiers, key } = normalizeShortcutParts(shortcut);
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
  shortcut: string,
  platform: ShortcutPlatformInput,
): boolean {
  const shortcutPlatform = normalizeShortcutPlatform(platform);
  const { modifiers, key } = normalizeShortcutParts(shortcut);
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
  shortcut: string,
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

  return [...MODIFIER_ORDER.filter((modifier) => modifiers.has(modifier)), key].join("+");
}
