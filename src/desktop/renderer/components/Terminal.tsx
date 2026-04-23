import { useCallback, useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import { createTerminalOptions, installMacShiftSelectionPatch } from "@/lib/terminalMouseSelection";

interface TerminalProps {
  taskId: string;
}

export default function Terminal({ taskId }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  const connect = useCallback(async () => {
    if (!containerRef.current) {
      return undefined;
    }

    const nerdFontFamily = "JetBrainsMono Nerd Font Mono";
    const fontFamily = `'${nerdFontFamily}', monospace`;
    const cdnBase = "https://cdn.jsdelivr.net/gh/mshaugh/nerdfont-webfonts@v3.3.0/build/fonts";
    const regular = new FontFace(nerdFontFamily, `url(${cdnBase}/JetBrainsMonoNerdFontMono-Regular.woff2)`, { weight: "400" });
    const bold = new FontFace(nerdFontFamily, `url(${cdnBase}/JetBrainsMonoNerdFontMono-Bold.woff2)`, { weight: "700" });
    document.fonts.add(regular);
    document.fonts.add(bold);
    await document.fonts.ready;
    await Promise.all([regular.load(), bold.load()]);

    const [{ Terminal: XTerm }, { FitAddon }, { WebLinksAddon }] = await Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit"),
      import("@xterm/addon-web-links"),
    ]);

    const terminal = new XTerm(createTerminalOptions(fontFamily));

    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(new WebLinksAddon());
    terminal.open(containerRef.current);
    const disposeMacShiftSelectionPatch = installMacShiftSelectionPatch(terminal);
    terminal.options.fontFamily = "monospace";
    terminal.options.fontFamily = fontFamily;

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
        disposeMacShiftSelectionPatch();
        terminal.dispose();
      };
    }

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
      fitAddon.fit();
      window.kanvibeDesktop!.resizeTerminal(taskId, terminal.cols, terminal.rows);
    });
    resizeObserver.observe(containerRef.current);

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        focusCurrentTerminal();
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
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
