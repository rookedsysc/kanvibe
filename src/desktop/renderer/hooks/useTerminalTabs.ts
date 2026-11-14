import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SessionType } from "@/entities/KanbanTask";
import {
  closeTerminalTab,
  createTerminalTab,
  listTerminalTabs,
  moveTerminalTab,
  renameTerminalTab,
  selectTerminalTab,
} from "@/desktop/renderer/actions/terminalTabs";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";
import { useTrackedAction } from "@/desktop/renderer/hooks/useTrackedAction";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

/**
 * 로컬 멀티플렉서 조회는 프로세스 실행 한 번이라 저렴하다.
 * 원격은 SSH 제어 소켓을 재사용해도 왕복이 남으므로 주기를 늘린다.
 */
const LOCAL_POLL_INTERVAL_MS = 1_000;
const REMOTE_POLL_INTERVAL_MS = 3_000;

interface UseTerminalTabsOptions {
  taskId: string;
  sessionType: SessionType | null;
  isRemote: boolean;
  /** 터미널 화면이 실제로 보이는 동안에만 폴링한다 */
  isVisible: boolean;
}

/** 탭 바가 어느 탭에서 무엇을 처리 중인지 보일 수 있게 진행 중인 명령을 알린다 */
export interface TerminalTabPendingAction {
  kind: "create" | "select" | "close" | "rename" | "move";
  /** 추가처럼 아직 대상 탭이 없는 명령은 null이다 */
  tabId: string | null;
}

type TerminalTabCommand =
  | { kind: "create"; tabId: null }
  | { kind: "select"; tabId: string }
  | { kind: "close"; tabId: string; onClosed: (shouldCloseWindow: boolean) => void }
  | { kind: "rename"; tabId: string; name: string }
  | { kind: "move"; tabId: string; targetIndex: number };

export interface TerminalTabsController {
  tabs: TerminalTab[];
  activeTab: TerminalTab | null;
  /** 탭 목록을 읽지 못한 이유. 멀티플렉서가 없거나 세션이 죽은 경우다 */
  error: string | null;
  /** 첫 조회가 끝났는지. 아직 기다리는 중인 빈 목록과 실제로 탭이 없는 목록을 가른다 */
  isTabListLoaded: boolean;
  pendingAction: TerminalTabPendingAction | null;
  /** 마지막 탭 명령이 실패한 이유. 다시 시도하거나 다음 명령이 성공하면 지워진다 */
  actionError: string | null;
  retryAction: () => Promise<void>;
  refresh: () => Promise<void>;
  createTab: () => Promise<void>;
  selectTab: (tabId: string) => Promise<void>;
  /** 창까지 닫아야 하는지를 반환한다 */
  closeTab: (tabId: string) => Promise<boolean>;
  renameTab: (tabId: string, name: string) => Promise<void>;
  moveTab: (tabId: string, targetIndex: number) => Promise<void>;
  selectRelativeTab: (offset: number) => Promise<void>;
  selectTabByPosition: (position: number) => Promise<void>;
}

/**
 * tmux와 zellij는 KanVibe 밖에서도 탭이 바뀌는데 이벤트 스트림이 없어 폴링으로 따라간다.
 * terminal 세션은 KanVibe가 탭의 소유자라 폴링 없이 조작 직후에만 다시 읽는다.
 */
export function useTerminalTabs({
  taskId,
  sessionType,
  isRemote,
  isVisible,
}: UseTerminalTabsOptions): TerminalTabsController {
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [isTabListLoaded, setIsTabListLoaded] = useState(false);
  const tabsRef = useRef<TerminalTab[]>([]);
  const isWindowActive = useIsWindowActive();

  tabsRef.current = tabs;

  const refreshState = useMemo(() => ({
    active: true,
    pending: null as Promise<void> | null,
    queued: false,
    hasStartedRefresh: false,
  }), [taskId, sessionType]);
  const currentRefreshState = useRef(refreshState);
  currentRefreshState.current = refreshState;

  useEffect(() => {
    refreshState.active = true;
    setTabs([]);
    setError(null);
    setIsTabListLoaded(false);
    return () => {
      refreshState.active = false;
    };
  }, [refreshState]);

  const refresh = useCallback((queueIfBusy = true): Promise<void> => {
    const isCurrent = () => refreshState.active && currentRefreshState.current === refreshState;
    if (!sessionType || !isCurrent()) {
      return Promise.resolve();
    }
    if (refreshState.pending) {
      refreshState.queued ||= queueIfBusy;
      return refreshState.pending;
    }

    refreshState.hasStartedRefresh = true;
    refreshState.pending = (async () => {
      do {
        refreshState.queued = false;
        /** 조회가 실패해도 터미널 자체는 계속 써야 하므로, 탭 바만 접고 화면은 유지한다 */
        try {
          const result = await listTerminalTabs(taskId);
          if (!isCurrent()) {
            return;
          }
          setTabs(result.ok ? result.tabs : []);
          setError(result.ok ? null : result.error ?? null);
          setIsTabListLoaded(true);
        } catch (error) {
          if (!isCurrent()) {
            return;
          }
          setTabs([]);
          setError(error instanceof Error ? error.message : "터미널 탭 목록을 읽지 못했습니다.");
          setIsTabListLoaded(true);
        }
      } while (refreshState.queued && isCurrent());
    })().finally(() => {
      refreshState.pending = null;
    });
    return refreshState.pending;
  }, [refreshState, sessionType, taskId]);

  useEffect(() => {
    if (!sessionType || !isVisible) {
      return;
    }

    /**
     * 창이 돌아왔을 때 이 효과가 다시 돌면서 밀린 변화를 한 번에 따라잡는다.
     * 창이 가려질 때도 이 효과가 다시 돌지만, 이미 읽은 목록을 그때 또 읽으면 원격 task는 창을 떠날 때마다 SSH 왕복이 나간다.
     */
    if (isWindowActive || !refreshState.hasStartedRefresh) {
      void refresh(false);
    }

    /** terminal 세션의 탭은 KanVibe만 바꾸므로 주기 조회가 필요 없다 */
    if (sessionType === SessionType.TERMINAL || !isWindowActive) {
      return;
    }

    const pollIntervalId = window.setInterval(() => {
      void refresh(false);
    }, isRemote ? REMOTE_POLL_INTERVAL_MS : LOCAL_POLL_INTERVAL_MS);

    return () => {
      window.clearInterval(pollIntervalId);
    };
  }, [isRemote, isVisible, isWindowActive, refresh, refreshState, sessionType]);

  /**
   * terminal 세션은 폴링하지 않으므로, 셸이 스스로 끝나 main이 탭을 지운 사실을 알 방법이 없다.
   * 그대로 두면 되살릴 수 없는 탭이 탭 바에 남는다.
   */
  useEffect(() => {
    if (sessionType !== SessionType.TERMINAL) {
      return;
    }

    return window.kanvibeDesktop?.onTerminalClose?.((event: { taskId: string }) => {
      if (event.taskId === taskId) {
        void refresh();
      }
    });
  }, [refresh, sessionType, taskId]);

  /**
   * 탭 명령은 한 번에 하나만 보낸다. 응답 전에 같은 명령이 겹치면 탭이 두 개 생기거나 순서가 뒤엉킨다.
   * 성공이든 실패든 끝나면 다시 읽어, 실패한 선택이나 이름 변경이 화면에 남지 않고 실제 상태로 돌아온다.
   */
  const tabCommand = useTrackedAction(async (command: TerminalTabCommand) => {
    try {
      switch (command.kind) {
        case "create":
          await createTerminalTab(taskId);
          return;
        case "select":
          /** 폴링을 기다리면 클릭이 굼떠 보이므로 먼저 화면을 바꾸고 실제 상태로 덮어쓴다 */
          setTabs((currentTabs) => currentTabs.map((tab) => ({ ...tab, isActive: tab.id === command.tabId })));
          await selectTerminalTab(taskId, command.tabId);
          return;
        case "close": {
          const result = await closeTerminalTab(taskId, command.tabId);
          command.onClosed(result.shouldCloseWindow ?? false);
          return;
        }
        case "rename":
          await renameTerminalTab(taskId, command.tabId, command.name);
          return;
        case "move":
          await moveTerminalTab(taskId, command.tabId, command.targetIndex);
      }
    } finally {
      await refresh();
    }
  });
  const runTabCommand = tabCommand.run;

  const selectTab = useCallback(
    (tabId: string) => runTabCommand({ kind: "select", tabId }),
    [runTabCommand],
  );

  const createTab = useCallback(
    () => runTabCommand({ kind: "create", tabId: null }),
    [runTabCommand],
  );

  const closeTab = useCallback(async (tabId: string) => {
    let shouldCloseWindow = false;
    await runTabCommand({
      kind: "close",
      tabId,
      onClosed: (closesWindow) => {
        shouldCloseWindow = closesWindow;
      },
    });
    return shouldCloseWindow;
  }, [runTabCommand]);

  const renameTab = useCallback(
    (tabId: string, name: string) => runTabCommand({ kind: "rename", tabId, name }),
    [runTabCommand],
  );

  const moveTab = useCallback(
    (tabId: string, targetIndex: number) => runTabCommand({ kind: "move", tabId, targetIndex }),
    [runTabCommand],
  );

  /** 목록 끝에서 반대쪽으로 넘어가, 탭이 두 개일 때도 같은 키를 계속 눌러 오갈 수 있다 */
  const selectRelativeTab = useCallback(async (offset: number) => {
    const currentTabs = tabsRef.current;
    if (currentTabs.length < 2) {
      return;
    }

    const activeIndex = currentTabs.findIndex((tab) => tab.isActive);
    const baseIndex = activeIndex === -1 ? 0 : activeIndex;
    const nextIndex = (baseIndex + offset + currentTabs.length) % currentTabs.length;

    await selectTab(currentTabs[nextIndex].id);
  }, [selectTab]);

  const selectTabByPosition = useCallback(async (position: number) => {
    const targetTab = tabsRef.current[position];
    if (targetTab) {
      await selectTab(targetTab.id);
    }
  }, [selectTab]);

  return {
    tabs,
    activeTab: tabs.find((tab) => tab.isActive) ?? null,
    error,
    isTabListLoaded,
    pendingAction: tabCommand.pendingArgs
      ? { kind: tabCommand.pendingArgs[0].kind, tabId: tabCommand.pendingArgs[0].tabId }
      : null,
    actionError: tabCommand.error,
    retryAction: tabCommand.retry,
    refresh,
    createTab,
    selectTab,
    closeTab,
    renameTab,
    moveTab,
    selectRelativeTab,
    selectTabByPosition,
  };
}
