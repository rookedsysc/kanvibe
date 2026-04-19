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

export function promoteMacShiftClickSelection(event: MouseEvent, isMacPlatform = detectMacPlatform()): void {
  if (!isMacPlatform || event.button !== 0 || !event.shiftKey || event.altKey) {
    return;
  }

  try {
    Object.defineProperty(event, "altKey", {
      configurable: true,
      value: true,
    });
  } catch {
    /* 읽기 전용 이벤트 프로퍼티면 그대로 둔다 */
  }
}

export function registerTerminalMouseSelectionBridge(container: HTMLElement, isMacPlatform = detectMacPlatform()): () => void {
  if (!isMacPlatform) {
    return () => undefined;
  }

  const handleMouseDown = (event: MouseEvent) => {
    promoteMacShiftClickSelection(event, true);
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
