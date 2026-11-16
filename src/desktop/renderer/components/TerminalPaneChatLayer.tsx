import { useCallback, useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { useTranslations } from "next-intl";
import { focusPaneChatTerminal, getPaneChatLayout, getPaneChatModes, setPaneChatMode } from "@/desktop/renderer/actions/paneChat";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";
import { usePaneChatPreferences } from "@/desktop/renderer/hooks/usePaneChatPreferences";
import { REQUEST_TOGGLE_PANE_CHAT_EVENT, requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import type { PaneChatLayout, PaneChatLayoutPane } from "@/desktop/shared/paneChat";
import { HugeiconsIcon } from "@hugeicons/react";
import { Chatting01Icon } from "@hugeicons/core-free-icons";
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

function panePosition(pane: PaneChatLayoutPane | undefined, metrics: TerminalChatGrid | null): CSSProperties | null {
  if (pane?.bounds === null) return { inset: 0 };
  if (!pane?.bounds || !metrics || metrics.cols <= 0 || metrics.rows <= 0) return null;
  const { left, top, width, height } = pane.bounds;
  if (left < 0 || top < 0 || width <= 0 || height <= 0 || left + width > metrics.cols || top + height > metrics.rows) return null;
  return { left: metrics.left + left * metrics.width / metrics.cols, top: metrics.top + top * metrics.height / metrics.rows, width: width * metrics.width / metrics.cols, height: height * metrics.height / metrics.rows };
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
  const preferences = usePaneChatPreferences();
  const [layout, setLayout] = useState<PaneChatLayout | null>(null);
  const [chatPanes, setChatPanes] = useState<Set<string>>(() => new Set());
  const [openedPanes, setOpenedPanes] = useState<string[]>([]);
  const [modesStatus, setModesStatus] = useState<"loading" | "loaded" | "failed">("loading");
  const [layoutFailed, setLayoutFailed] = useState(false);
  const [returnFailed, setReturnFailed] = useState(false);
  const layerRef = useRef<HTMLDivElement>(null);
  const focusedPane = useRef<string | null>(null);
  const chatPanesRef = useRef(chatPanes);
  chatPanesRef.current = chatPanes;
  const modesStatusRef = useRef(modesStatus);
  modesStatusRef.current = modesStatus;
  const visibleRef = useRef(isVisible);
  visibleRef.current = isVisible;
  const generation = useRef(0);
  const pendingToggle = useRef(false);
  const loadRef = useRef<(fresh?: boolean) => Promise<PaneChatLayout | null>>(async () => null);
  const toggleRef = useRef<(paneId?: string) => Promise<void>>(async () => {});

  /** 모드가 정해진 pane. 저장소에서 복원했거나, 이 화면에서 사용자나 기본 채팅 모드가 정한 pane만 들어 있다 */
  const decidedModes = useRef(new Map<string, boolean>());
  const saveMode = useCallback((paneId: string, isChat: boolean) => {
    decidedModes.current.set(paneId, isChat);
    // 저장 실패는 화면 동작을 막지 않는다. 지금 화면에는 반영되고 다음에 열 때만 복원되지 않는다.
    void setPaneChatMode(taskId, paneId, isChat).catch(() => {});
  }, [taskId]);
  useEffect(() => {
    let disposed = false;
    getPaneChatModes(taskId).then((savedModes) => {
      if (disposed) return;
      // 조회가 끝나기 전에 사용자가 먼저 바꾼 pane은 저장된 값으로 덮지 않는다.
      const restoredModes = Object.entries(savedModes).filter(([paneId]) => !decidedModes.current.has(paneId));
      for (const [paneId, isChat] of restoredModes) decidedModes.current.set(paneId, isChat);
      const restoredChatPaneIds = restoredModes.filter(([, isChat]) => isChat).map(([paneId]) => paneId);
      setChatPanes((current) => new Set([...current, ...restoredChatPaneIds]));
      setOpenedPanes((current) => [...current, ...restoredChatPaneIds.filter((paneId) => !current.includes(paneId))]);
      setModesStatus("loaded");
    }).catch(() => {
      if (!disposed) setModesStatus("failed");
    });
    return () => { disposed = true; };
  }, [taskId]);
  useEffect(() => {
    if (modesStatus === "loaded" && chatPanesRef.current.size > 0 && visibleRef.current) void loadRef.current();
  }, [modesStatus]);
  // 저장된 기록을 읽은 뒤에만 적용해야 직접 터미널로 둔 pane을 기본 채팅 모드가 다시 열지 않는다. 조회에 실패하면 그 기록을 알 수 없으므로 적용하지 않는다.
  useEffect(() => {
    if (!preferences.loaded || modesStatus !== "loaded" || !layout?.activePaneId || decidedModes.current.has(layout.activePaneId)) return;
    const paneId = layout.activePaneId;
    if (!preferences.defaultEnabled) {
      decidedModes.current.set(paneId, false);
      return;
    }
    saveMode(paneId, true);
    setChatPanes((current) => new Set(current).add(paneId));
    setOpenedPanes((current) => current.includes(paneId) ? current : [...current, paneId]);
  }, [preferences.loaded, preferences.defaultEnabled, modesStatus, layout, saveMode]);

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
      saveMode(paneId, false);
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
  }, [taskId, onChatFocusChange, focusPane, saveMode]);

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
        // 저장된 모드를 읽기 전에는 복원할 채팅 pane을 알 수 없으므로 포커스 복원을 소비하지 않고 모드 조회가 끝난 뒤의 읽기로 미룬다.
        if (restoreFocus && modesStatusRef.current !== "loading") {
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
        if (!disposed && isVisible && isWindowActive) {
          timer = setTimeout(() => void load(), LAYOUT_POLL_INTERVAL_MS);
        }
      });
      return reading;
    };
    loadRef.current = load;
    if (isVisible && focusedPane.current && chatPanesRef.current.has(focusedPane.current)) onChatFocusChange(true);
    if (isVisible) void load();
    if (!isVisible) {
      setLayout(null);
      onChatFocusChange(false);
    }
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
    };
  }, [taskId, terminalTabId, isVisible, isWindowActive, onChatFocusChange, focusPane]);

  toggleRef.current = async (requestedPaneId) => {
    if (!visibleRef.current || pendingToggle.current) return;
    const focused = focusedPane.current;
    if (!requestedPaneId && focused && chatPanes.has(focused) && layout?.panes.some((pane) => pane.id === focused)) {
      await closePane(focused);
      return;
    }
    const epoch = generation.current;
    pendingToggle.current = true;
    try {
      const next = await loadRef.current(true);
      const paneId = requestedPaneId ?? next?.activePaneId;
      if (!paneId || !next?.panes.some((pane) => pane.id === paneId) || !visibleRef.current || epoch !== generation.current) return;
      setOpenedPanes((current) => current.includes(paneId) ? current : [...current, paneId]);
      saveMode(paneId, true);
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
      void loadRef.current();
    };
    const terminal = terminalRef.current;
    window.addEventListener(REQUEST_TOGGLE_PANE_CHAT_EVENT, toggle);
    terminal?.addEventListener("pointerdown", focusTerminal);
    return () => {
      window.removeEventListener(REQUEST_TOGGLE_PANE_CHAT_EVENT, toggle);
      terminal?.removeEventListener("pointerdown", focusTerminal);
    };
  }, [terminalRef, onChatFocusChange]);

  const activePane = layout?.panes.find((pane) => pane.id === layout.activePaneId);
  const activePosition = panePosition(activePane, metrics);
  return <div ref={layerRef} className="pointer-events-none absolute inset-0 overflow-hidden">
    {isVisible && !layout && !layoutFailed && (!preferences.loaded || modesStatus === "loading" || preferences.defaultEnabled || chatPanes.size > 0) ? <section data-testid="pane-chat-loading" aria-busy="true" className="pointer-events-auto absolute inset-0 flex flex-col bg-bg-page">
      <header className="flex min-h-14 items-center gap-2 border-b border-border-default px-4 text-sm text-text-primary"><HugeiconsIcon icon={Chatting01Icon} size={18} aria-hidden="true" />{taskTitle ?? t("toggle")}</header>
      <div role="status" className="flex flex-1 items-center justify-center text-sm text-text-secondary">{t("loading")}</div>
    </section> : null}
    {isVisible && activePane && activePosition && !chatPanes.has(activePane.id) ? <div style={activePosition} className="pointer-events-none absolute">
      <button type="button" data-testid="pane-chat-toggle" aria-label={`${t("toggle")} · ${activePane.id}`} title={`${t("toggle")} · ${activePane.id}`} onClick={() => { void toggleRef.current(activePane.id); }} className="pointer-events-auto absolute right-2 top-1 z-10 flex h-9 items-center gap-1 rounded-md bg-bg-surface px-2 text-xs text-text-primary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary">
        <HugeiconsIcon icon={Chatting01Icon} size={16} aria-hidden="true" />{t("toggle")}
      </button>
    </div> : null}
    {openedPanes.map((paneId) => {
      const pane = layout?.panes.find((entry) => entry.id === paneId);
      const position = panePosition(pane, metrics);
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
