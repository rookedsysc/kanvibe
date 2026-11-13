import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import "@xterm/xterm/css/xterm.css";
import { createTerminalOptions, installMacShiftSelectionPatch } from "@/lib/terminalMouseSelection";
import { installOsc52ClipboardHandler } from "@/lib/terminalClipboard";
import { REQUEST_ACTIVE_TERMINAL_FOCUS_EVENT, hasTerminalFocusBlocker } from "@/desktop/renderer/utils/terminalFocus";
import { TerminalPaneChatLayer, type TerminalChatGrid } from "./TerminalPaneChatLayer";

interface TerminalProps {
  taskId: string;
  /** terminal 세션은 탭마다 PTY가 따로라 탭 식별자가 필요하다. tmux·zellij 세션은 null이다 */
  tabId?: string | null;
  /**
   * 비활성 탭은 화면에서 숨겨진다.
   * 숨겨진 컨테이너는 높이·너비가 0이라 fit이 터미널을 2×1로 줄여 버리므로 크기 동기화를 멈춘다.
   */
  isHidden?: boolean;
  /** 원격(SSH) 세션에서만 클립보드 이미지 붙여넣기를 가로챈다. 로컬 세션은 기존 텍스트 붙여넣기만 유지한다 */
  isRemote?: boolean;
  taskTitle?: string;
  onHistory?: () => void;
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

export default function Terminal({ taskId, tabId = null, isHidden = false, isRemote = false, taskTitle, onHistory }: TerminalProps) {
  const t = useTranslations("taskDetail");
  const translationsRef = useRef(t);
  translationsRef.current = t;
  const containerRef = useRef<HTMLDivElement>(null);
  const chatFocusedRef = useRef(false);
  const [chatGrid, setChatGrid] = useState<TerminalChatGrid | null>(null);
  const onChatFocusChange = useCallback((focused: boolean) => { chatFocusedRef.current = focused; }, []);
  const cancelRetryRef = useRef<(() => void) | null>(null);
  const [connectionStatus, setConnectionStatus] = useState<"connecting" | "reconnecting" | "ready" | "error">("connecting");
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [connectionAttempt, setConnectionAttempt] = useState(0);
  /** connect는 taskId·tabId에만 묶여 있어야 하므로, 표시 여부·원격 여부는 ref로 읽는다 */
  const isHiddenRef = useRef(isHidden);
  const isRemoteRef = useRef(isRemote);

  isHiddenRef.current = isHidden;
  isRemoteRef.current = isRemote;

  const connect = useCallback(async (signal: AbortSignal, registerCleanup: (cleanup: () => void) => void) => {
    if (!containerRef.current) {
      return undefined;
    }

    setConnectionStatus("connecting");
    setConnectionError(null);
    const pasteEventTarget = containerRef.current;
    const [{ Terminal: XTerm }, { FitAddon }, { WebLinksAddon }] = await loadTerminalModules();
    if (signal.aborted) {
      return;
    }
    let isTerminalDisposed = false;
    let isTerminalOpen = false;

    const terminal = new XTerm(createTerminalOptions(FALLBACK_FONT_FAMILY));
    const cleanups: (() => void)[] = [() => terminal.dispose()];
    const dispose = () => {
      if (isTerminalDisposed) {
        return;
      }
      isTerminalDisposed = true;
      for (const cleanup of cleanups.reverse()) {
        cleanup();
      }
    };
    registerCleanup(dispose);

    try {

      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      terminal.loadAddon(new WebLinksAddon());
      terminal.open(pasteEventTarget);
      const disposeMacShiftSelectionPatch = installMacShiftSelectionPatch(terminal);
      cleanups.push(disposeMacShiftSelectionPatch);
      const osc52ClipboardHandler = installOsc52ClipboardHandler(terminal);
      cleanups.push(() => osc52ClipboardHandler.dispose());
      terminal.options.fontFamily = FALLBACK_FONT_FAMILY;

      /**
       * 원격 세션의 CLI(예: Claude Code)는 헤드리스 서버라 로컬 클립보드 이미지를 직접 읽을 수 없다.
       * 그래서 로컬 클립보드 이미지를 감지하면 scp로 원격에 옮기고, 그 경로만 텍스트로 붙여넣는다.
       */
      const sendImageToRemoteTerminal = (imageDataUrl: string) => {
        void window.kanvibeDesktop!.pasteImageToRemoteTerminal(taskId, imageDataUrl).then((result: { ok: boolean; remotePath?: string; error?: string }) => {
          if (result.ok && result.remotePath) {
            window.kanvibeDesktop!.writeTerminal(taskId, tabId, result.remotePath);
          } else {
            terminal.writeln(`\r\n\x1b[31m${result.error || "이미지 전송 실패"}\x1b[0m`);
          }
        });
      };

      const handleImagePaste = (file: File) => {
        const reader = new FileReader();
        reader.onload = () => {
          const imageDataUrl = reader.result;
          if (typeof imageDataUrl === "string") {
            sendImageToRemoteTerminal(imageDataUrl);
          }
        };
        reader.readAsDataURL(file);
      };

      /** capture 단계로 등록해야 xterm이 자신의 textarea(target)에 붙인 붙여넣기 처리보다 먼저 가로챌 수 있다 */
      const handlePaste = (event: ClipboardEvent) => {
        if (chatFocusedRef.current) {
          event.preventDefault();
          event.stopImmediatePropagation();
          return;
        }
        if (!isRemoteRef.current) {
          return;
        }

        const items = event.clipboardData?.items;
        if (!items) {
          return;
        }

        const imageItem = Array.from(items).find(
          (item) => item.kind === "file" && item.type.startsWith("image/"),
        );
        if (!imageItem) {
          return;
        }

        event.preventDefault();
        event.stopImmediatePropagation();

        const file = imageItem.getAsFile();
        if (file) {
          handleImagePaste(file);
        }
      };

      pasteEventTarget.addEventListener("paste", handlePaste, true);
      cleanups.push(() => pasteEventTarget.removeEventListener("paste", handlePaste, true));

      /**
       * macOS의 Chromium 편집 동작은 Cmd+V만 붙여넣기로 매핑하므로, Ctrl+V·Ctrl+Shift+V·Shift+Insert는
       * 네이티브 paste 이벤트를 만들지 않고 xterm이 제어 문자를 그대로 pty에 흘려보낸다.
       * Claude Code·Codex 같은 CLI는 이 조합도 붙여넣기 시도로 해석하는 경우가 많아 같은 기능을 태운다.
       * 클립보드에 이미지가 없으면 xterm 기본 처리를 그대로 둬야 vim 비주얼 블록 모드 같은 기존 용법이 깨지지 않는다.
       */
      const tryHandleImagePasteShortcut = (): boolean => {
        if (!isRemoteRef.current) {
          return false;
        }

        const imageDataUrl = window.kanvibeDesktop!.readClipboardImage();
        if (!imageDataUrl) {
          return false;
        }

        sendImageToRemoteTerminal(imageDataUrl);
        return true;
      };

      terminal.attachCustomKeyEventHandler((event) => {
        if (chatFocusedRef.current) return false;
        if (event.type !== "keydown") {
          return true;
        }

        const isCtrlV = event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey && event.key.toLowerCase() === "v";
        const isCtrlShiftV = event.ctrlKey && !event.metaKey && event.shiftKey && !event.altKey && event.key.toLowerCase() === "v";
        const isShiftInsert = event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey && event.key === "Insert";

        if (!isCtrlV && !isCtrlShiftV && !isShiftInsert) {
          return true;
        }

        return !tryHandleImagePasteShortcut();
      });

      /** X11 중간 클릭 붙여넣기 관례를 macOS에서도 받아 주기 위한 트리거. 이 저장소에 기존 중간 클릭 처리는 없다 */
      const handleAuxClick = (event: MouseEvent) => {
        if (event.button !== 1) {
          return;
        }

        if (tryHandleImagePasteShortcut()) {
          event.preventDefault();
        }
      };

      pasteEventTarget.addEventListener("auxclick", handleAuxClick);
      cleanups.push(() => pasteEventTarget.removeEventListener("auxclick", handleAuxClick));

      const isThisTerminal = (event: { taskId: string; tabId: string | null }) => (
        event.taskId === taskId && (event.tabId ?? null) === tabId
      );

      const unsubscribeData = window.kanvibeDesktop!.onTerminalData((event: { taskId: string; tabId: string | null; data: string }) => {
        if (isThisTerminal(event)) {
          terminal.write(event.data);
        }
      });

      cleanups.push(unsubscribeData);

      const unsubscribeClose = window.kanvibeDesktop!.onTerminalClose((event: { taskId: string; tabId: string | null; reason: string | null }) => {
        // 실패한 open도 close를 보낼 수 있으므로 연결이 끝나기 전에는 최종 오류를 그리지 않는다.
        if (isThisTerminal(event) && isTerminalOpen) {
          terminal.writeln(`\r\n\x1b[31m${event.reason || "연결이 종료되었습니다."}\x1b[0m`);
        }
      });

      cleanups.push(unsubscribeClose);

      const syncTerminalSize = () => {
        if (isTerminalDisposed || isHiddenRef.current) {
          return;
        }

        fitAddon.fit();
        window.kanvibeDesktop!.resizeTerminal(taskId, tabId, terminal.cols, terminal.rows);
        const screen = pasteEventTarget.querySelector<HTMLElement>(".xterm-screen");
        if (screen) {
          const bounds = screen.getBoundingClientRect();
          const containerBounds = pasteEventTarget.getBoundingClientRect();
          const next = { cols: terminal.cols, rows: terminal.rows, left: bounds.left - containerBounds.left, top: bounds.top - containerBounds.top, width: bounds.width, height: bounds.height };
          setChatGrid((current) => current && Object.keys(next).every((key) => current[key as keyof TerminalChatGrid] === next[key as keyof TerminalChatGrid]) ? current : next);
        }
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

      await new Promise<void>((resolve, reject) => {
        requestAnimationFrame(() => {
          try {
            if (!signal.aborted && !isHiddenRef.current) {
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
      cleanups.push(() => window.kanvibeDesktop!.closeTerminal(taskId, tabId));
      let terminalReady = await window.kanvibeDesktop!.openTerminal(taskId, tabId, terminal.cols, terminal.rows);
      if (signal.aborted) {
        return;
      }

      // 재시도 힌트는 일시적인 전송 실패에만 온다. 자동 재연결은 한 번으로 제한한다.
      const retryAfterMs = terminalReady.retryAfterMs;
      if (!terminalReady.ok && typeof retryAfterMs === "number" && Number.isFinite(retryAfterMs) && retryAfterMs > 0) {
        setConnectionStatus("reconnecting");
        const didWait = !isHiddenRef.current && await new Promise<boolean>((resolve) => {
          const finish = (didWait: boolean) => {
            window.clearTimeout(timer);
            signal.removeEventListener("abort", cancel);
            cancelRetryRef.current = null;
            resolve(didWait);
          };
          const cancel = () => finish(false);
          const timer = window.setTimeout(() => finish(true), retryAfterMs);
          cancelRetryRef.current = cancel;
          signal.addEventListener("abort", cancel, { once: true });
        });
        if (signal.aborted) {
          return;
        }
        if (!didWait) {
          setConnectionError(translationsRef.current("terminalReconnectPaused"));
          setConnectionStatus("error");
          return;
        }
        terminalReady = await window.kanvibeDesktop!.openTerminal(taskId, tabId, terminal.cols, terminal.rows);
        if (signal.aborted) {
          return;
        }
      }

      if (!terminalReady.ok) {
        const error = terminalReady.error || translationsRef.current("terminalConnectionFailed");
        terminal.writeln(`\r\n\x1b[31m${error}\x1b[0m`);
        setConnectionError(error);
        setConnectionStatus("error");
        return;
      }
      isTerminalOpen = true;
      setConnectionStatus("ready");

      terminal.onData((data) => {
        // 커서 조회 같은 프로토콜 응답도 여기로 오므로 사용자 입력은 키보드·붙여넣기 경계에서 차단한다.
        window.kanvibeDesktop!.writeTerminal(taskId, tabId, data);
      });

      terminal.onResize(({ cols, rows }) => {
        window.kanvibeDesktop!.resizeTerminal(taskId, tabId, cols, rows);
      });

      const focusCurrentTerminal = () => {
        if (isHiddenRef.current || chatFocusedRef.current || hasTerminalFocusBlocker()) {
          return;
        }

        terminal.focus();
      };

      focusCurrentTerminal();

      const resizeObserver = new ResizeObserver(() => {
        syncTerminalSize();
      });
      cleanups.push(() => resizeObserver.disconnect());
      resizeObserver.observe(pasteEventTarget);

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

      cleanups.push(() => {
        document.removeEventListener("visibilitychange", handleVisibilityChange);
        window.removeEventListener("focus", handleWindowFocus);
        window.removeEventListener(REQUEST_ACTIVE_TERMINAL_FOCUS_EVENT, handleRequestTerminalFocus);
      });
    } catch (error) {
      dispose();
      throw error;
    }
  }, [tabId, taskId]);

  useEffect(() => {
    const controller = new AbortController();
    let cleanup: (() => void) | undefined;

    void connect(controller.signal, (dispose) => { cleanup = dispose; })
      .catch((error) => {
        console.error("데스크톱 터미널 초기화 실패:", error);
        if (!controller.signal.aborted) {
          setConnectionError(error instanceof Error ? error.message : translationsRef.current("terminalConnectionFailed"));
          setConnectionStatus("error");
        }
      });

    return () => {
      controller.abort();
      cleanup?.();
    };
  }, [connect, connectionAttempt]);

  useEffect(() => {
    if (isHidden) {
      cancelRetryRef.current?.();
    }
  }, [isHidden]);

  const isConnecting = connectionStatus === "connecting" || connectionStatus === "reconnecting";

  return (
    <div className="relative w-full h-full overflow-hidden bg-terminal-bg" aria-busy={isConnecting}>
      <div ref={containerRef} className="w-full h-full" />
      {onHistory ? <TerminalPaneChatLayer key={`${taskId}:${tabId ?? "multiplexer"}`} taskId={taskId} terminalTabId={tabId ?? undefined} taskTitle={taskTitle} terminalRef={containerRef} metrics={chatGrid} isVisible={!isHidden && connectionStatus === "ready"} onHistory={onHistory} onChatFocusChange={onChatFocusChange} /> : null}
      {isConnecting && (
        <div className="absolute inset-0 flex items-center justify-center bg-terminal-bg text-text-muted">
          <p role="status" className="flex items-center gap-2 text-sm">
            <span aria-hidden="true" className="h-4 w-4 rounded-full border-2 border-current border-t-transparent motion-safe:animate-spin" />
            {t(connectionStatus === "reconnecting" ? "terminalReconnecting" : "terminalConnecting")}
          </p>
        </div>
      )}
      {connectionStatus === "error" && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-terminal-bg px-4 text-center">
          <p role="alert" className="text-sm text-status-error break-words max-w-full">{connectionError}</p>
          <button
            type="button"
            onClick={() => setConnectionAttempt((attempt) => attempt + 1)}
            className="rounded px-3 py-1.5 text-xs bg-button-neutral text-text-primary hover:border-brand-primary border border-border-default focus:outline-none focus:ring-1 focus:ring-brand-primary"
          >
            {t("terminalRetry")}
          </button>
        </div>
      )}
    </div>
  );
}
