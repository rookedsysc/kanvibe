import type { ITerminalOptions } from "@xterm/xterm";

const TERMINAL_THEME = {
  background: "#0a0a0a",
  foreground: "#e4e4e7",
  cursor: "#e4e4e7",
  selectionBackground: "#3b82f680",
};

export function createTerminalOptions(fontFamily: string): ITerminalOptions {
  return {
    allowProposedApi: true,
    cursorBlink: true,
    fontSize: 14,
    fontFamily,
    rescaleOverlappingGlyphs: true,
    macOptionClickForcesSelection: true,
    theme: TERMINAL_THEME,
  };
}

export function promoteMacShiftClickSelection(
  event: MouseEvent,
  isMacPlatform = detectMacPlatform(),
): MouseEvent | null {
  if (!isMacPlatform || event.button !== 0 || !event.shiftKey || event.altKey) {
    return null;
  }

  return new MouseEvent(event.type, {
    bubbles: event.bubbles,
    cancelable: event.cancelable,
    composed: event.composed,
    detail: event.detail,
    button: event.button,
    buttons: event.buttons,
    clientX: event.clientX,
    clientY: event.clientY,
    screenX: event.screenX,
    screenY: event.screenY,
    ctrlKey: event.ctrlKey,
    metaKey: event.metaKey,
    altKey: true,
    shiftKey: false,
  });
}

export function registerTerminalMouseSelectionBridge(container: HTMLElement, isMacPlatform = detectMacPlatform()): () => void {
  if (!isMacPlatform) {
    return () => undefined;
  }

  const handleMouseDown = (event: MouseEvent) => {
    const promotedEvent = promoteMacShiftClickSelection(event, true);
    if (!promotedEvent) {
      return;
    }

    event.preventDefault();
    event.stopImmediatePropagation();
    (event.target ?? container).dispatchEvent(promotedEvent);
  };

  container.addEventListener("mousedown", handleMouseDown, true);
  return () => {
    container.removeEventListener("mousedown", handleMouseDown, true);
  };
}

function detectMacPlatform(): boolean {
  if (typeof navigator === "undefined") {
    return false;
  }

  const userAgentData = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  const platform = userAgentData?.platform ?? navigator.platform ?? "";
  return /mac/i.test(platform);
}
