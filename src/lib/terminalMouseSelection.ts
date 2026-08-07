import type { ITerminalOptions } from "@xterm/xterm";

interface XTermSelectionService {
  shouldForceSelection(event: MouseEvent): boolean;
  handleMouseDown(event: MouseEvent): void;
}

interface XTermWithCore {
  _core?: {
    _selectionService?: XTermSelectionService;
  };
}

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

export function installMacShiftSelectionPatch(terminal: unknown, isMacPlatform = detectMacPlatform()): () => void {
  if (!isMacPlatform) {
    return () => undefined;
  }

  const selectionService = (terminal as XTermWithCore)._core?._selectionService;
  if (!selectionService) {
    return () => undefined;
  }

  const defaultShouldForceSelection = selectionService.shouldForceSelection.bind(selectionService);
  const defaultHandleMouseDown = selectionService.handleMouseDown.bind(selectionService);

  selectionService.shouldForceSelection = (event: MouseEvent) => {
    return shouldPromoteMacShiftSelection(event, true) || defaultShouldForceSelection(event);
  };

  selectionService.handleMouseDown = (event: MouseEvent) => {
    defaultHandleMouseDown(createMacForceSelectionEvent(event, true) ?? event);
  };

  return () => {
    selectionService.shouldForceSelection = defaultShouldForceSelection;
    selectionService.handleMouseDown = defaultHandleMouseDown;
  };
}

function shouldPromoteMacShiftSelection(event: MouseEvent, isMacPlatform = detectMacPlatform()): boolean {
  return isMacPlatform && event.button === 0 && event.shiftKey && !event.altKey;
}

function createMacForceSelectionEvent(event: MouseEvent, isMacPlatform = detectMacPlatform()): MouseEvent | null {
  if (!shouldPromoteMacShiftSelection(event, isMacPlatform)) {
    return null;
  }

  return {
    altKey: true,
    button: event.button,
    buttons: event.buttons,
    clientX: event.clientX,
    clientY: event.clientY,
    ctrlKey: event.ctrlKey,
    detail: event.detail,
    metaKey: event.metaKey,
    preventDefault: event.preventDefault.bind(event),
    screenX: event.screenX,
    screenY: event.screenY,
    shiftKey: false,
    stopImmediatePropagation: event.stopImmediatePropagation.bind(event),
    stopPropagation: event.stopPropagation.bind(event),
    target: event.target,
    timeStamp: event.timeStamp,
    type: event.type,
  } as MouseEvent;
}

function detectMacPlatform(): boolean {
  if (typeof navigator === "undefined") {
    return false;
  }

  const userAgentData = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
  const platform = userAgentData?.platform ?? navigator.platform ?? "";
  return /mac/i.test(platform);
}
