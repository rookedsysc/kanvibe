const MODIFIER_ORDER = ["Mod", "Meta", "Ctrl", "Alt", "Shift"] as const;
const MODIFIER_KEYS = new Set(["Meta", "Control", "Ctrl", "Alt", "Shift"]);

export const DEFAULT_TASK_SEARCH_SHORTCUT = "Mod+Shift+O";

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

  if (trimmedToken === " ") {
    return "Space";
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

export function formatShortcutForDisplay(shortcut: string, isMacLike: boolean): string {
  const { modifiers, key } = normalizeShortcutParts(shortcut);
  const displayParts: string[] = modifiers.map((modifier) => {
    if (modifier === "Mod") {
      return isMacLike ? "Cmd" : "Ctrl";
    }

    if (modifier === "Meta") {
      return isMacLike ? "Cmd" : "Meta";
    }

    return modifier;
  });

  if (key) {
    displayParts.push(key);
  }

  return displayParts.join("+");
}

export function matchShortcutEvent(
  event: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">,
  shortcut: string,
  isMacLike: boolean,
): boolean {
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

  const expectedMeta = requiresMeta || (requiresMod && isMacLike);
  const expectedCtrl = requiresCtrl || (requiresMod && !isMacLike);

  if (event.metaKey !== expectedMeta) {
    return false;
  }

  if (event.ctrlKey !== expectedCtrl) {
    return false;
  }

  if (event.altKey !== requiresAlt) {
    return false;
  }

  if (event.shiftKey !== requiresShift) {
    return false;
  }

  return true;
}

export function captureShortcutFromEvent(
  event: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">,
): string | null {
  if (MODIFIER_KEYS.has(event.key)) {
    return null;
  }

  const modifiers: string[] = [];

  if (event.metaKey) {
    modifiers.push("Meta");
  }

  if (event.ctrlKey) {
    modifiers.push("Ctrl");
  }

  if (event.altKey) {
    modifiers.push("Alt");
  }

  if (event.shiftKey) {
    modifiers.push("Shift");
  }

  if (modifiers.length === 0) {
    return null;
  }

  const key = normalizeEventKey(event.key);
  if (!key) {
    return null;
  }

  return [...modifiers, key].join("+");
}
