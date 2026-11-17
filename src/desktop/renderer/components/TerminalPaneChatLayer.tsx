import { useCallback, useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { useTranslations } from "next-intl";
import { focusPaneChatTerminal, getPaneChatAgentProvider, getPaneChatLayout, getPaneChatModes, getPaneChatSnapshot, setPaneChatMode } from "@/desktop/renderer/actions/paneChat";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";
import { usePaneChatPreferences } from "@/desktop/renderer/hooks/usePaneChatPreferences";
import { REQUEST_TOGGLE_PANE_CHAT_EVENT, requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import type { PaneChatLayout, PaneChatLayoutPane } from "@/desktop/shared/paneChat";
import { HugeiconsIcon } from "@hugeicons/react";
import { Chatting01Icon } from "@hugeicons/core-free-icons";
import { PaneChatView } from "./PaneChatView";

const LAYOUT_POLL_INTERVAL_MS = 1_000;

/**
 * 자동으로 채팅을 열 만한 pane인지 확인한다. CLI가 실행 중이어도 폴더 신뢰 확인 같은 첫 화면에서는 대화를 연결할 수 없어, 채팅이 그 화면을 가리기만 한다.
 * Codex가 "대화 ID 표시"를 기다리는 상태는 채팅에서 버튼을 눌러야 연결되므로 연다. CLI가 없는 pane이 대부분이라 값싼 프로세스 확인을 먼저 한다.
 */
async function isPaneChatReadyToOpen(taskId: string, paneId: string): Promise<boolean> {
  if (!await getPaneChatAgentProvider(taskId, paneId)) return false;
  const snapshot = await getPaneChatSnapshot(taskId, paneId);
  return snapshot.canSend || snapshot.transcript.reason === "codex-thread-unconfirmed";
}

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
  /** 저장된 모드는 채팅이지만 그 pane에서 CLI가 실행 중인지 아직 확인하지 못해 터미널로 보여 주는 pane */
  const savedChatPanesAwaitingAgent = useRef(new Set<string>());
  useEffect(() => {
    let disposed = false;
    getPaneChatModes(taskId).then((savedModes) => {
      if (disposed) return;
      // 조회가 끝나기 전에 사용자가 먼저 바꾼 pane은 저장된 값으로 덮지 않는다.
      const restoredModes = Object.entries(savedModes).filter(([paneId]) => !decidedModes.current.has(paneId));
      for (const [paneId, isChat] of restoredModes) {
        decidedModes.current.set(paneId, isChat);
        if (isChat) savedChatPanesAwaitingAgent.current.add(paneId);
      }
      setModesStatus("loaded");
    }).catch(() => {
      if (!disposed) setModesStatus("failed");
    });
    return () => { disposed = true; };
  }, [taskId]);

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

  const agentProbingPanes = useRef(new Set<string>());
  /**
   * 저장된 채팅 모드나 기본 채팅 모드로 자동으로 열 pane은, 그 pane의 CLI와 대화를 연결할 수 있는 것이 확인된 뒤에만 채팅으로 연다.
   * 연결할 CLI가 없는 pane에 채팅 화면을 띄우지 않기 위해서다. pane 배치를 읽을 때마다 다시 확인하므로 나중에 CLI를 실행한 pane도 채팅으로 바뀐다.
   * 기본 채팅 모드는 저장된 기록을 읽은 뒤에만 적용해야 직접 터미널로 둔 pane을 다시 열지 않는다. 조회에 실패하면 그 기록을 알 수 없으므로 적용하지 않는다.
   */
  useEffect(() => {
    if (!isVisible || !layout) return;
    const activePaneId = layout.activePaneId;
    const isActivePaneLeftToDefaultMode = preferences.loaded && modesStatus === "loaded" && !!activePaneId && !decidedModes.current.has(activePaneId);
    if (isActivePaneLeftToDefaultMode && !preferences.defaultEnabled) decidedModes.current.set(activePaneId, false);
    const defaultChatPaneId = isActivePaneLeftToDefaultMode && preferences.defaultEnabled ? activePaneId : null;
    const epoch = generation.current;
    for (const { id: paneId } of layout.panes) {
      const isAwaitingAgent = paneId === defaultChatPaneId || savedChatPanesAwaitingAgent.current.has(paneId);
      if (!isAwaitingAgent || agentProbingPanes.current.has(paneId)) continue;
      agentProbingPanes.current.add(paneId);
      void isPaneChatReadyToOpen(taskId, paneId).then((isReadyToOpen) => {
        if (!isReadyToOpen || !visibleRef.current || generation.current !== epoch) return;
        const isSavedChatPane = savedChatPanesAwaitingAgent.current.delete(paneId);
        // 확인을 기다리는 사이 사용자가 직접 전환했거나 기본 채팅 모드를 껐으면 그 선택을 따른다.
        if (!isSavedChatPane && decidedModes.current.has(paneId)) return;
        if (!isSavedChatPane) saveMode(paneId, true);
        setOpenedPanes((current) => current.includes(paneId) ? current : [...current, paneId]);
        setChatPanes((current) => new Set(current).add(paneId));
        // 채팅이 덮는 pane이 지금 쓰는 pane이면 입력도 채팅으로 옮겨야, 가려진 터미널에 글자가 들어가지 않는다.
        if (paneId === activePaneId && !focusedPane.current) focusPane(paneId);
      }).catch(() => {
        // 확인하지 못한 pane은 터미널로 두고 다음 조회에서 다시 확인한다.
      }).finally(() => { agentProbingPanes.current.delete(paneId); });
    }
  }, [taskId, isVisible, layout, modesStatus, preferences.loaded, preferences.defaultEnabled, saveMode, focusPane]);

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
      savedChatPanesAwaitingAgent.current.delete(paneId);
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
    {isVisible && !layout && !layoutFailed && chatPanes.size > 0 ? <section data-testid="pane-chat-loading" aria-busy="true" className="pointer-events-auto absolute inset-0 flex flex-col bg-bg-page">
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
