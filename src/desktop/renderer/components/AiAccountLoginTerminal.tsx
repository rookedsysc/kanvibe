import { useCallback, useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import { createTerminalOptions } from "@/lib/terminalMouseSelection";
import { installOsc52ClipboardHandler } from "@/lib/terminalClipboard";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

interface AiAccountLoginTerminalProps {
  provider: AiUsageProvider;
  /** 로그인 세션은 태스크가 아니라 계정 루트로 식별한다 */
  accountRoot: string;
  onExit: (exitCode: number) => void;
}

const LOGIN_TERMINAL_FONT_FAMILY = "monospace";

/**
 * provider CLI의 로그인 화면을 앱 안에서 그대로 보여준다.
 *
 * 브라우저로 넘어가는 provider는 안내만 지나가지만, Gemini처럼 첫 실행 화면에서 인증 방식을
 * 골라야 하는 CLI는 사용자가 여기서 직접 고를 수 있어야 터미널로 나가지 않는다.
 */
export default function AiAccountLoginTerminal({
  provider,
  accountRoot,
  onExit,
}: AiAccountLoginTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const onExitRef = useRef(onExit);

  onExitRef.current = onExit;

  const connect = useCallback(async () => {
    if (!containerRef.current) {
      return undefined;
    }

    const [{ Terminal: XTerm }, { FitAddon }] = await Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit"),
    ]);

    const terminal = new XTerm(createTerminalOptions(LOGIN_TERMINAL_FONT_FAMILY));
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(containerRef.current);
    const osc52ClipboardHandler = installOsc52ClipboardHandler(terminal);

    const isThisSession = (event: { accountRoot: string }) => event.accountRoot === accountRoot;

    const unsubscribeData = window.kanvibeDesktop?.onAiAccountLoginData?.((event: {
      accountRoot: string;
      data: string;
    }) => {
      if (isThisSession(event)) {
        terminal.write(event.data);
      }
    });

    const unsubscribeExit = window.kanvibeDesktop?.onAiAccountLoginExit?.((event: {
      accountRoot: string;
      exitCode: number;
    }) => {
      if (isThisSession(event)) {
        onExitRef.current(event.exitCode);
      }
    });

    await new Promise<void>((resolve) => {
      requestAnimationFrame(() => {
        fitAddon.fit();
        resolve();
      });
    });

    const loginReady = await window.kanvibeDesktop?.openAiAccountLogin?.(
      provider,
      accountRoot,
      terminal.cols,
      terminal.rows,
    );

    if (!loginReady?.ok) {
      terminal.writeln(`\r\n\x1b[31m${loginReady?.error || "로그인 명령을 실행하지 못했습니다."}\x1b[0m`);
      return () => {
        osc52ClipboardHandler.dispose();
        unsubscribeData?.();
        unsubscribeExit?.();
        terminal.dispose();
      };
    }

    terminal.onData((data) => {
      window.kanvibeDesktop?.writeAiAccountLogin?.(accountRoot, data);
    });

    terminal.focus();

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      window.kanvibeDesktop?.resizeAiAccountLogin?.(accountRoot, terminal.cols, terminal.rows);
    });
    resizeObserver.observe(containerRef.current);

    return () => {
      resizeObserver.disconnect();
      osc52ClipboardHandler.dispose();
      unsubscribeData?.();
      unsubscribeExit?.();
      window.kanvibeDesktop?.closeAiAccountLogin?.(accountRoot);
      terminal.dispose();
    };
  }, [accountRoot, provider]);

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
        console.error("AI 계정 로그인 터미널 초기화 실패:", error);
      });

    return () => {
      isDisposed = true;
      cleanup?.();
    };
  }, [connect]);

  return (
    <div
      ref={containerRef}
      className="h-64 w-full overflow-hidden rounded-md bg-terminal-bg p-2"
      data-testid="ai-account-login-terminal"
    />
  );
}
