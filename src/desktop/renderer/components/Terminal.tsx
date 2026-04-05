import { useCallback, useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";

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

    const terminal = new XTerm({
      allowProposedApi: true,
      cursorBlink: true,
      fontSize: 14,
      fontFamily,
      rescaleOverlappingGlyphs: true,
      theme: {
        background: "#0a0a0a",
        foreground: "#e4e4e7",
        cursor: "#e4e4e7",
        selectionBackground: "#3b82f680",
      },
    });

    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(new WebLinksAddon());
    terminal.open(containerRef.current);
    terminal.options.fontFamily = "monospace";
    terminal.options.fontFamily = fontFamily;

    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => {
        fitAddon.fit();
        resolve();
      });
    });

    const terminalReady = await window.kanvibeDesktop.openTerminal(taskId, terminal.cols, terminal.rows);
    if (!terminalReady.ok) {
      terminal.writeln(`\r\n\x1b[31m${terminalReady.error || "터미널 연결 실패"}\x1b[0m`);
    }

    const unsubscribeData = window.kanvibeDesktop.onTerminalData((event) => {
      if (event.taskId === taskId) {
        terminal.write(event.data);
      }
    });

    const unsubscribeClose = window.kanvibeDesktop.onTerminalClose((event) => {
      if (event.taskId === taskId) {
        terminal.writeln(`\r\n\x1b[31m${event.reason || "연결이 종료되었습니다."}\x1b[0m`);
      }
    });

    terminal.onData((data) => {
      window.kanvibeDesktop.writeTerminal(taskId, data);
    });

    terminal.onResize(({ cols, rows }) => {
      window.kanvibeDesktop.resizeTerminal(taskId, cols, rows);
    });

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      window.kanvibeDesktop.resizeTerminal(taskId, terminal.cols, terminal.rows);
    });
    resizeObserver.observe(containerRef.current);

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        window.kanvibeDesktop.focusTerminal(taskId);
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      resizeObserver.disconnect();
      unsubscribeData();
      unsubscribeClose();
      window.kanvibeDesktop.closeTerminal(taskId);
      terminal.dispose();
    };
  }, [taskId]);

  useEffect(() => {
    let cleanup: (() => void) | undefined;
    connect().then((dispose) => {
      cleanup = dispose;
    });
    return () => cleanup?.();
  }, [connect]);

  return <div ref={containerRef} className="w-full h-full overflow-hidden bg-terminal-bg" />;
}
