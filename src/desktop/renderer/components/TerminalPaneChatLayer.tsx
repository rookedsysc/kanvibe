import { useCallback, useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { useTranslations } from "next-intl";
import { focusPaneChatTerminal, getPaneChatLayout } from "@/desktop/renderer/actions/paneChat";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";
import { REQUEST_TOGGLE_PANE_CHAT_EVENT, requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import type { PaneChatLayout } from "@/desktop/shared/paneChat";
import { PaneChatView } from "./PaneChatView";

const LAYOUT_POLL_INTERVAL_MS = 1_000;

export interface TerminalChatGrid {
  cols: number;
  rows: number;
  left: number;
  top: number;
  width: number;
  height: number;
}

export function TerminalPaneChatLayer({ taskId, terminalTabId, taskTitle, terminalRef, metrics, isVisible, onHistory, onChatFocusChange }: {
  taskId: string;
  terminalTabId?: string;
  taskTitle?: string;
  terminalRef: RefObject<HTMLDivElement | null>;
  metrics: TerminalChatGrid | null;
  isVisible: boolean;
  onHistory?: () => void;
  onChatFocusChange: (focused: boolean) => void;
}) {
  const t = useTranslations("taskDetail.paneChat");
  const isWindowActive = useIsWindowActive();
  const [layout, setLayout] = useState<PaneChatLayout | null>(null);
  const [openedPanes, setOpenedPanes] = useState<string[]>([]);
  const [chatPanes, setChatPanes] = useState<Set<string>>(() => new Set());
  const [layoutFailed, setLayoutFailed] = useState(false);
  const [returnFailed, setReturnFailed] = useState(false);
  const layerRef = useRef<HTMLDivElement>(null);
  const focusedPane = useRef<string | null>(null);
  const chatPanesRef = useRef(chatPanes);
  chatPanesRef.current = chatPanes;
  const visibleRef = useRef(isVisible);
  visibleRef.current = isVisible;
  const generation = useRef(0);
  const pendingToggle = useRef(false);
  const loadRef = useRef<(fresh?: boolean) => Promise<PaneChatLayout | null>>(async () => null);
  const toggleRef = useRef<() => Promise<void>>(async () => {});
  const hasOpenedPanes = openedPanes.length > 0;

  const focusPane = useCallback((paneId: string) => {
    focusedPane.current = paneId;
    onChatFocusChange(true);
    requestAnimationFrame(() => {
      const panel = Array.from(layerRef.current?.children ?? []).find((node) => node.getAttribute("data-pane-chat-id") === paneId) as HTMLElement | undefined;
      if (visibleRef.current && panel?.style.display !== "none") (panel?.querySelector<HTMLTextAreaElement>("textarea:not(:disabled)") ?? panel)?.focus();
    });
  }, [onChatFocusChange]);

  const closePane = useCallback(async (paneId: string) => {
    if (!visibleRef.current || pendingToggle.current) return;
    const epoch = generation.current;
    pendingToggle.current = true;
    setReturnFailed(false);
    try {
      const result = await focusPaneChatTerminal(taskId, paneId);
      if (!result.success) throw new Error("Pane focus failed");
      if (!visibleRef.current || generation.current !== epoch) return;
      setChatPanes((current) => {
        const next = new Set(current);
        next.delete(paneId);
        return next;
      });
      focusedPane.current = null;
      onChatFocusChange(false);
      requestActiveTerminalFocusAfterUiSettles();
    } catch {
      if (visibleRef.current && generation.current === epoch) {
        setReturnFailed(true);
        focusPane(paneId);
      }
    } finally {
      pendingToggle.current = false;
    }
  }, [taskId, onChatFocusChange, focusPane]);

  useEffect(() => {
    const epoch = ++generation.current;
    let disposed = false;
    let reading: Promise<PaneChatLayout | null> | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let restoreFocus = isVisible && isWindowActive;
    const load = (fresh = false): Promise<PaneChatLayout | null> => {
      if (reading) return fresh ? reading.then(() => disposed ? null : load()) : reading;
      if (timer) clearTimeout(timer);
      reading = getPaneChatLayout(taskId, terminalTabId).then((next) => {
        if (disposed || generation.current !== epoch || !visibleRef.current) return null;
        setLayout(next);
        setLayoutFailed(false);
        if (focusedPane.current && !next.panes.some((pane) => pane.id === focusedPane.current)) {
          focusedPane.current = null;
          onChatFocusChange(false);
        }
        if (restoreFocus) {
          restoreFocus = false;
          const paneId = focusedPane.current ?? next.activePaneId;
          if (paneId && chatPanesRef.current.has(paneId) && next.panes.some((pane) => pane.id === paneId)) focusPane(paneId);
        }
        return next;
      }).catch(() => {
        // 이전 좌표로 다른 pane을 덮지 않도록 조회 실패 시 원래 터미널을 보여 준다.
        if (!disposed) {
          setLayout(null);
          setLayoutFailed(true);
          onChatFocusChange(false);
        }
        return null;
      }).finally(() => {
        reading = null;
        if (!disposed && isVisible && isWindowActive && hasOpenedPanes) {
          timer = setTimeout(() => void load(), LAYOUT_POLL_INTERVAL_MS);
        }
      });
      return reading;
    };
    loadRef.current = load;
    if (isVisible && focusedPane.current && chatPanesRef.current.has(focusedPane.current)) onChatFocusChange(true);
    if (isVisible && hasOpenedPanes) void load();
    if (!isVisible) {
      setLayout(null);
      onChatFocusChange(false);
    }
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
    };
  }, [taskId, terminalTabId, isVisible, isWindowActive, hasOpenedPanes, onChatFocusChange, focusPane]);

  toggleRef.current = async () => {
    if (!visibleRef.current || pendingToggle.current) return;
    const focused = focusedPane.current;
    if (focused && chatPanes.has(focused) && layout?.panes.some((pane) => pane.id === focused)) {
      await closePane(focused);
      return;
    }
    const epoch = generation.current;
    pendingToggle.current = true;
    try {
      const next = await loadRef.current(true);
      if (!next?.activePaneId || !visibleRef.current || epoch !== generation.current) return;
      const paneId = next.activePaneId;
      setOpenedPanes((current) => current.includes(paneId) ? current : [...current, paneId]);
      setChatPanes((current) => new Set(current).add(paneId));
      focusPane(paneId);
    } finally {
      pendingToggle.current = false;
    }
  };

  useEffect(() => {
    const toggle = () => void toggleRef.current();
    const focusTerminal = () => {
      focusedPane.current = null;
      onChatFocusChange(false);
      setLayout(null);
      if (hasOpenedPanes) void loadRef.current();
    };
    const terminal = terminalRef.current;
    window.addEventListener(REQUEST_TOGGLE_PANE_CHAT_EVENT, toggle);
    terminal?.addEventListener("pointerdown", focusTerminal);
    return () => {
      window.removeEventListener(REQUEST_TOGGLE_PANE_CHAT_EVENT, toggle);
      terminal?.removeEventListener("pointerdown", focusTerminal);
    };
  }, [terminalRef, onChatFocusChange, hasOpenedPanes]);

  return <div ref={layerRef} className="pointer-events-none absolute inset-0 overflow-hidden">
    {openedPanes.map((paneId) => {
      const pane = layout?.panes.find((entry) => entry.id === paneId);
      let position: CSSProperties | null = pane?.bounds === null ? { inset: 0 } : null;
      if (pane?.bounds && metrics && metrics.cols > 0 && metrics.rows > 0) {
        const { left, top, width, height } = pane.bounds;
        if (left >= 0 && top >= 0 && width > 0 && height > 0 && left + width <= metrics.cols && top + height <= metrics.rows) {
          position = { left: metrics.left + left * metrics.width / metrics.cols, top: metrics.top + top * metrics.height / metrics.rows, width: width * metrics.width / metrics.cols, height: height * metrics.height / metrics.rows };
        }
      }
      const visible = isVisible && chatPanes.has(paneId) && position !== null;
      return <div key={paneId} data-pane-chat-id={paneId} tabIndex={-1} style={{ ...position, display: visible ? "flex" : "none" }} className="pointer-events-auto absolute min-h-0 min-w-0 flex-col overflow-hidden bg-bg-page outline-none" onFocusCapture={() => {
        focusedPane.current = paneId;
        onChatFocusChange(true);
      }} onPointerDownCapture={() => {
        focusedPane.current = paneId;
        onChatFocusChange(true);
      }}>
        <PaneChatView taskId={taskId} paneId={paneId} taskTitle={taskTitle} isVisible={visible} onHistory={onHistory ?? (() => {})} onTerminal={() => closePane(paneId)} />
      </div>;
    })}
    {isVisible && (layoutFailed || returnFailed) ? <div role="alert" className="pointer-events-auto absolute bottom-3 left-3 right-3 flex items-center justify-between gap-2 rounded-lg border border-border-default bg-bg-surface p-3 text-xs text-text-primary">
      {t(returnFailed ? "returnError" : "readError")}<button type="button" onClick={() => void toggleRef.current()} className="min-h-10 rounded px-3 text-text-brand focus-visible:outline-2 focus-visible:outline-brand-primary">{t("retry")}</button>
    </div> : null}
  </div>;
}
