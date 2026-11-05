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

  const connect = useCallback(async (signal: AbortSignal, registerCleanup: (cleanup: () => void) => void) => {
    if (!containerRef.current) {
      return undefined;
    }

    const container = containerRef.current;
    const [{ Terminal: XTerm }, { FitAddon }] = await Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit"),
    ]);

    if (signal.aborted) {
      return;
    }

    const terminal = new XTerm(createTerminalOptions(LOGIN_TERMINAL_FONT_FAMILY));
    let disposed = false;
    const cleanups: (() => void)[] = [() => terminal.dispose()];
    const dispose = () => {
      if (disposed) {
        return;
      }
      disposed = true;
      for (const cleanup of cleanups.reverse()) {
        cleanup();
      }
    };
    registerCleanup(dispose);
    try {
      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      terminal.open(container);
      const osc52ClipboardHandler = installOsc52ClipboardHandler(terminal);

      cleanups.push(() => osc52ClipboardHandler.dispose());

      const isThisSession = (event: { accountRoot: string }) => event.accountRoot === accountRoot;

      const unsubscribeData = window.kanvibeDesktop?.onAiAccountLoginData?.((event: {
        accountRoot: string;
        data: string;
      }) => {
        if (isThisSession(event)) {
          terminal.write(event.data);
        }
      });

      cleanups.push(() => unsubscribeData?.());

      const unsubscribeExit = window.kanvibeDesktop?.onAiAccountLoginExit?.((event: {
        accountRoot: string;
        exitCode: number;
      }) => {
        if (isThisSession(event)) {
          onExitRef.current(event.exitCode);
        }
      });

      cleanups.push(() => unsubscribeExit?.());

      await new Promise<void>((resolve, reject) => {
        requestAnimationFrame(() => {
          try {
            if (!signal.aborted) {
              fitAddon.fit();
            }
            resolve();
          } catch (error) {
            reject(error);
          }
        });
      });

      if (signal.aborted) {
        return;
      }
      cleanups.push(() => window.kanvibeDesktop?.closeAiAccountLogin?.(accountRoot));
      const loginReady = await window.kanvibeDesktop?.openAiAccountLogin?.(
        provider,
        accountRoot,
        terminal.cols,
        terminal.rows,
      );

      if (signal.aborted) {
        return;
      }
      if (!loginReady?.ok) {
        terminal.writeln(`\r\n\x1b[31m${loginReady?.error || "로그인 명령을 실행하지 못했습니다."}\x1b[0m`);
        return;
      }

      terminal.onData((data) => {
        window.kanvibeDesktop?.writeAiAccountLogin?.(accountRoot, data);
      });

      terminal.focus();

      const resizeObserver = new ResizeObserver(() => {
        fitAddon.fit();
        window.kanvibeDesktop?.resizeAiAccountLogin?.(accountRoot, terminal.cols, terminal.rows);
      });
      cleanups.push(() => resizeObserver.disconnect());
      resizeObserver.observe(container);

    } catch (error) {
      dispose();
      throw error;
    }
  }, [accountRoot, provider]);

  useEffect(() => {
    const controller = new AbortController();
    let cleanup: (() => void) | undefined;

    void connect(controller.signal, (dispose) => { cleanup = dispose; })
      .catch((error) => {
        console.error("AI 계정 로그인 터미널 초기화 실패:", error);
      });

    return () => {
      controller.abort();
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
