import { useCallback, useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import { createTerminalOptions, installMacShiftSelectionPatch } from "@/lib/terminalMouseSelection";

interface TerminalProps {
  taskId: string;
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

export default function Terminal({ taskId }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);

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
    terminal.options.fontFamily = FALLBACK_FONT_FAMILY;

    const unsubscribeData = window.kanvibeDesktop!.onTerminalData((event: { taskId: string; data: string }) => {
      if (event.taskId === taskId) {
        terminal.write(event.data);
      }
    });

    const unsubscribeClose = window.kanvibeDesktop!.onTerminalClose((event: { taskId: string; reason: string | null }) => {
      if (event.taskId === taskId) {
        terminal.writeln(`\r\n\x1b[31m${event.reason || "연결이 종료되었습니다."}\x1b[0m`);
      }
    });

    const syncTerminalSize = () => {
      fitAddon.fit();
      window.kanvibeDesktop!.resizeTerminal(taskId, terminal.cols, terminal.rows);
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
        fitAddon.fit();
        resolve();
      });
    });

    const terminalReady = await window.kanvibeDesktop!.openTerminal(taskId, terminal.cols, terminal.rows);
    if (!terminalReady.ok) {
      terminal.writeln(`\r\n\x1b[31m${terminalReady.error || "터미널 연결 실패"}\x1b[0m`);
      return () => {
        isTerminalDisposed = true;
        disposeMacShiftSelectionPatch();
        unsubscribeData();
        unsubscribeClose();
        terminal.dispose();
      };
    }

    terminal.onData((data) => {
      window.kanvibeDesktop!.writeTerminal(taskId, data);
    });

    terminal.onResize(({ cols, rows }) => {
      window.kanvibeDesktop!.resizeTerminal(taskId, cols, rows);
    });

    const focusCurrentTerminal = () => {
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

    document.addEventListener("visibilitychange", handleVisibilityChange);
    window.addEventListener("focus", handleWindowFocus);

    return () => {
      isTerminalDisposed = true;
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", handleWindowFocus);
      disposeMacShiftSelectionPatch();
      resizeObserver.disconnect();
      unsubscribeData();
      unsubscribeClose();
      window.kanvibeDesktop!.closeTerminal(taskId);
      terminal.dispose();
    };
  }, [taskId]);

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
