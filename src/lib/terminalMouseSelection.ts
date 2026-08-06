import type { ITerminalOptions, ITheme } from "@xterm/xterm";
import { OPAQUE_TERMINAL_OPACITY, clampTerminalOpacity, isTerminalTransparent } from "@/lib/terminalOpacity";

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

/** 불투명도를 xterm 색상 문자열이 받는 두 자리 alpha hex로 바꾼다 */
function toAlphaHex(opacity: number): string {
  return Math.round(opacity * 255).toString(16).padStart(2, "0");
}

/** 요청된 불투명도를 배경색 alpha로 반영한 xterm 테마를 만든다 */
export function createTerminalTheme(terminalOpacity: number): ITheme {
  if (!isTerminalTransparent(terminalOpacity)) {
    return TERMINAL_THEME;
  }

  return {
    ...TERMINAL_THEME,
    background: `${TERMINAL_THEME.background}${toAlphaHex(clampTerminalOpacity(terminalOpacity))}`,
  };
}

export function createTerminalOptions(
  fontFamily: string,
  terminalOpacity: number = OPAQUE_TERMINAL_OPACITY,
): ITerminalOptions {
  return {
    allowProposedApi: true,
    /** 반투명 배경은 open() 전에 켜야 하므로 터미널 생성 시점의 설정으로 결정한다 */
    allowTransparency: isTerminalTransparent(terminalOpacity),
    cursorBlink: true,
    fontSize: 14,
    fontFamily,
    rescaleOverlappingGlyphs: true,
    macOptionClickForcesSelection: true,
    theme: createTerminalTheme(terminalOpacity),
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
