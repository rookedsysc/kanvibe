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
  /** 원격(SSH) 세션에서만 클립보드 이미지 붙여넣기를 가로챈다. 로컬 세션은 기존 텍스트 붙여넣기만 유지한다 */
  isRemote?: boolean;
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

export default function Terminal({ taskId, tabId = null, isHidden = false, isRemote = false }: TerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  /** connect는 taskId·tabId에만 묶여 있어야 하므로, 표시 여부·원격 여부는 ref로 읽는다 */
  const isHiddenRef = useRef(isHidden);
  const isRemoteRef = useRef(isRemote);

  isHiddenRef.current = isHidden;
  isRemoteRef.current = isRemote;

  const connect = useCallback(async () => {
    if (!containerRef.current) {
      return undefined;
    }

    const pasteEventTarget = containerRef.current;
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
        pasteEventTarget.removeEventListener("paste", handlePaste, true);
        pasteEventTarget.removeEventListener("auxclick", handleAuxClick);
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
      pasteEventTarget.removeEventListener("paste", handlePaste, true);
      pasteEventTarget.removeEventListener("auxclick", handleAuxClick);
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
