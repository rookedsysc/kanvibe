import { useCallback, useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import { createTerminalOptions, installMacShiftSelectionPatch } from "@/lib/terminalMouseSelection";
import { installOsc52ClipboardHandler } from "@/lib/terminalClipboard";
import { REQUEST_ACTIVE_TERMINAL_FOCUS_EVENT, hasTerminalFocusBlocker } from "@/desktop/renderer/utils/terminalFocus";

interface TerminalProps {
  taskId: string;
  /** terminal 세션은 탭마다 PTY가 따로라 탭 식별자가 필요하다. tmux·zellij 세션은 null이다 */
  tabId?: string | null;
  /**
   * 비활성 탭은 화면에서 숨겨진다.
   * 숨겨진 컨테이너는 높이·너비가 0이라 fit이 터미널을 2×1로 줄여 버리므로 크기 동기화를 멈춘다.
   */
  isHidden?: boolean;
}

const NERD_FONT_FAMILY = "JetBrainsMono Nerd Font Mono";
const NERD_FONT_CDN_BASE = "https://cdn.jsdelivr.net/gh/mshaugh/nerdfont-webfonts@v3.3.0/build/fonts";
const FALLBACK_FONT_FAMILY = "monospace";
const TERMINAL_FONT_FAMILY = `'${NERD_FONT_FAMILY}', ${FALLBACK_FONT_FAMILY}`;

type TerminalModules = [
  typeof import("@xterm/xterm"),
  typeof import("@xterm/addon-fit"),
  typeof import("@xterm/addon-web-links"),
];

let terminalModulesPromise: Promise<TerminalModules> | null = null;
let nerdFontLoadPromise: Promise<string | null> | null = null;

function loadTerminalModules() {
  if (!terminalModulesPromise) {
    terminalModulesPromise = Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit"),
      import("@xterm/addon-web-links"),
    ]);
  }

  return terminalModulesPromise;
}

function loadNerdFontFamily(): Promise<string | null> {
  if (
    typeof document === "undefined" ||
    typeof FontFace === "undefined" ||
    !document.fonts
  ) {
    return Promise.resolve(null);
  }

  if (!nerdFontLoadPromise) {
    const regular = new FontFace(
      NERD_FONT_FAMILY,
      `url(${NERD_FONT_CDN_BASE}/JetBrainsMonoNerdFontMono-Regular.woff2)`,
      { weight: "400" },
    );
    const bold = new FontFace(
      NERD_FONT_FAMILY,
      `url(${NERD_FONT_CDN_BASE}/JetBrainsMonoNerdFontMono-Bold.woff2)`,
      { weight: "700" },
    );

    document.fonts.add(regular);
    document.fonts.add(bold);

    nerdFontLoadPromise = Promise.allSettled([regular.load(), bold.load()])
      .then(() => document.fonts.ready)
      .then(() => TERMINAL_FONT_FAMILY)
      .catch(() => null);
  }

  return nerdFontLoadPromise;
}

export default function Terminal({ taskId, tabId = null, isHidden = false }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  /** connect는 taskId·tabId에만 묶여 있어야 하므로, 표시 여부는 ref로 읽는다 */
  const isHiddenRef = useRef(isHidden);

  isHiddenRef.current = isHidden;

  const connect = useCallback(async () => {
    if (!containerRef.current) {
      return undefined;
    }

    const [{ Terminal: XTerm }, { FitAddon }, { WebLinksAddon }] = await loadTerminalModules();
    let isTerminalDisposed = false;

    const terminal = new XTerm(createTerminalOptions(FALLBACK_FONT_FAMILY));

    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(new WebLinksAddon());
    terminal.open(containerRef.current);
    const disposeMacShiftSelectionPatch = installMacShiftSelectionPatch(terminal);
    const osc52ClipboardHandler = installOsc52ClipboardHandler(terminal);
    terminal.options.fontFamily = FALLBACK_FONT_FAMILY;

    const isThisTerminal = (event: { taskId: string; tabId: string | null }) => (
      event.taskId === taskId && (event.tabId ?? null) === tabId
    );

    const unsubscribeData = window.kanvibeDesktop!.onTerminalData((event: { taskId: string; tabId: string | null; data: string }) => {
      if (isThisTerminal(event)) {
        terminal.write(event.data);
      }
    });

    const unsubscribeClose = window.kanvibeDesktop!.onTerminalClose((event: { taskId: string; tabId: string | null; reason: string | null }) => {
      if (isThisTerminal(event)) {
        terminal.writeln(`\r\n\x1b[31m${event.reason || "연결이 종료되었습니다."}\x1b[0m`);
      }
    });

    const syncTerminalSize = () => {
      if (isHiddenRef.current) {
        return;
      }

      fitAddon.fit();
      window.kanvibeDesktop!.resizeTerminal(taskId, tabId, terminal.cols, terminal.rows);
    };

    const scheduleTerminalSync = () => {
      requestAnimationFrame(() => {
        syncTerminalSize();
      });
    };

    void loadNerdFontFamily().then((fontFamily) => {
      if (!fontFamily || isTerminalDisposed) {
        return;
      }

      terminal.options.fontFamily = fontFamily;
      scheduleTerminalSync();
    });

    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => {
        if (!isHiddenRef.current) {
          fitAddon.fit();
        }
        resolve();
      });
    });

    const terminalReady = await window.kanvibeDesktop!.openTerminal(taskId, tabId, terminal.cols, terminal.rows);
    if (!terminalReady.ok) {
      terminal.writeln(`\r\n\x1b[31m${terminalReady.error || "터미널 연결 실패"}\x1b[0m`);
      return () => {
        isTerminalDisposed = true;
        disposeMacShiftSelectionPatch();
        osc52ClipboardHandler.dispose();
        unsubscribeData();
        unsubscribeClose();
        terminal.dispose();
      };
    }

    terminal.onData((data) => {
      window.kanvibeDesktop!.writeTerminal(taskId, tabId, data);
    });

    terminal.onResize(({ cols, rows }) => {
      window.kanvibeDesktop!.resizeTerminal(taskId, tabId, cols, rows);
    });

    const focusCurrentTerminal = () => {
      if (hasTerminalFocusBlocker()) {
        return;
      }

      terminal.focus();
    };

    focusCurrentTerminal();

    const resizeObserver = new ResizeObserver(() => {
      syncTerminalSize();
    });
    resizeObserver.observe(containerRef.current);

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        focusCurrentTerminal();
        scheduleTerminalSync();
      }
    };

    const handleWindowFocus = () => {
      focusCurrentTerminal();
      scheduleTerminalSync();
    };

    const handleRequestTerminalFocus = () => {
      focusCurrentTerminal();
      scheduleTerminalSync();
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", handleWindowFocus);
    window.addEventListener(REQUEST_ACTIVE_TERMINAL_FOCUS_EVENT, handleRequestTerminalFocus);

    return () => {
      isTerminalDisposed = true;
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", handleWindowFocus);
      window.removeEventListener(REQUEST_ACTIVE_TERMINAL_FOCUS_EVENT, handleRequestTerminalFocus);
      disposeMacShiftSelectionPatch();
      osc52ClipboardHandler.dispose();
      resizeObserver.disconnect();
      unsubscribeData();
      unsubscribeClose();
      window.kanvibeDesktop!.closeTerminal(taskId, tabId);
      terminal.dispose();
    };
  }, [tabId, taskId]);

  useEffect(() => {
    let isDisposed = false;
    let cleanup: (() => void) | undefined;

    void connect()
      .then((dispose) => {
        if (isDisposed) {
          dispose?.();
          return;
        }

        cleanup = dispose;
      })
      .catch((error) => {
        console.error("데스크톱 터미널 초기화 실패:", error);
      });

    return () => {
      isDisposed = true;
      cleanup?.();
    };
  }, [connect]);

  return <div ref={containerRef} className="w-full h-full overflow-hidden bg-terminal-bg" />;
}
