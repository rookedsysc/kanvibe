"use client";

import { useEffect, useRef, useCallback } from "react";
import "@xterm/xterm/css/xterm.css";

interface TerminalProps {
  taskId: string;
}

/**
 * xterm.js 터미널 컴포넌트.
 * WebSocket으로 서버의 tmux/zellij 세션에 연결한다.
 */
export default function Terminal({ taskId }: TerminalProps) {
  const terminalRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const xtermRef = useRef<import("@xterm/xterm").Terminal | null>(null);

  const connect = useCallback(async () => {
    if (!terminalRef.current) return;

    /** Nerd Font 웹폰트를 document.fonts에 등록하고 완전히 로드될 때까지 대기. xterm.js는 글리프 폭을 동기적으로 측정하므로 폰트가 완전히 준비된 후에 터미널을 open 해야 한다 */
    const nerdFontFamily = "GeistMono Nerd Font Mono";
    const fontFamily = `'${nerdFontFamily}', monospace`;
    const cdnBase =
      "https://cdn.jsdelivr.net/gh/mshaugh/nerdfont-webfonts@v3.3.0/build/fonts";
    const regular = new FontFace(
      nerdFontFamily,
      `url(${cdnBase}/GeistMonoNerdFontMono-Regular.woff2)`,
      { weight: "400" }
    );
    const bold = new FontFace(
      nerdFontFamily,
      `url(${cdnBase}/GeistMonoNerdFontMono-Bold.woff2)`,
      { weight: "700" }
    );
    document.fonts.add(regular);
    document.fonts.add(bold);
    await document.fonts.ready;
    await Promise.all([regular.load(), bold.load()]);

    const { Terminal } = await import("@xterm/xterm");
    const { FitAddon } = await import("@xterm/addon-fit");
    const { WebLinksAddon } = await import("@xterm/addon-web-links");

    const term = new Terminal({
      allowProposedApi: true,
      cursorBlink: true,
      fontSize: 14,
      fontFamily,
      theme: {
        background: "#0a0a0a",
        foreground: "#e4e4e7",
        cursor: "#e4e4e7",
        selectionBackground: "#3b82f680",
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon());
    term.open(terminalRef.current);

    /** 웹폰트 로드 완료 후 fontFamily를 재설정하여 xterm.js 글리프 캐시를 강제 갱신 */
    term.options.fontFamily = "monospace";
    term.options.fontFamily = fontFamily;

    requestAnimationFrame(() => fitAddon.fit());

    xtermRef.current = term;

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsHost = window.location.hostname;
    const wsPort = parseInt(window.location.port || "4885", 10) + 10000;
    const wsUrl = `${protocol}//${wsHost}:${wsPort}/api/terminal/${taskId}`;
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      term.writeln("\x1b[32m터미널에 연결되었습니다.\x1b[0m\r\n");

      /** 초기 크기를 서버에 전달 */
      const resizeMsg = JSON.stringify({
        type: "resize",
        cols: term.cols,
        rows: term.rows,
      });
      ws.send("\x01" + resizeMsg);
    };

    ws.onmessage = (event) => {
      term.write(event.data);
    };

    ws.onclose = () => {
      term.writeln("\r\n\x1b[31m연결이 종료되었습니다.\x1b[0m");
    };

    ws.onerror = () => {
      term.writeln("\r\n\x1b[31m연결 오류가 발생했습니다.\x1b[0m");
    };

    term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(data);
      }
    });

    term.onResize(({ cols, rows }) => {
      if (ws.readyState === WebSocket.OPEN) {
        const resizeMsg = JSON.stringify({ type: "resize", cols, rows });
        ws.send("\x01" + resizeMsg);
      }
    });

    /** 컨테이너 크기 변경을 직접 감지하여 터미널 재적용 */
    const resizeObserver = new ResizeObserver(() => fitAddon.fit());
    resizeObserver.observe(terminalRef.current);

    return () => {
      resizeObserver.disconnect();
      ws.close();
      term.dispose();
    };
  }, [taskId]);

  useEffect(() => {
    let cleanup: (() => void) | undefined;
    connect().then((fn) => {
      cleanup = fn;
    });
    return () => cleanup?.();
  }, [connect]);

  return (
    <div
      ref={terminalRef}
      className="w-full h-full overflow-hidden bg-terminal-bg"
    />
  );
}
