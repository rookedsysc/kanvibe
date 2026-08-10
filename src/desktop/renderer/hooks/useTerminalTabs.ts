import { useCallback, useEffect, useRef, useState } from "react";
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

export interface TerminalTabsController {
  tabs: TerminalTab[];
  activeTab: TerminalTab | null;
  /** 탭 목록을 읽지 못한 이유. 멀티플렉서가 없거나 세션이 죽은 경우다 */
  error: string | null;
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
  const tabsRef = useRef<TerminalTab[]>([]);
  const isWindowActive = useIsWindowActive();

  tabsRef.current = tabs;

  const refresh = useCallback(async () => {
    if (!sessionType) {
      return;
    }

    /** 조회가 실패해도 터미널 자체는 계속 써야 하므로, 탭 바만 접고 화면은 유지한다 */
    try {
      const result = await listTerminalTabs(taskId);
      if (result.ok) {
        setTabs(result.tabs);
        setError(null);
        return;
      }

      setTabs([]);
      setError(result.error ?? null);
    } catch (error) {
      setTabs([]);
      setError(error instanceof Error ? error.message : "터미널 탭 목록을 읽지 못했습니다.");
    }
  }, [sessionType, taskId]);

  useEffect(() => {
    if (!sessionType || !isVisible) {
      return;
    }

    /** 창이 돌아왔을 때 이 효과가 다시 돌면서 밀린 변화를 한 번에 따라잡는다 */
    void refresh();

    /** terminal 세션의 탭은 KanVibe만 바꾸므로 주기 조회가 필요 없다 */
    if (sessionType === SessionType.TERMINAL || !isWindowActive) {
      return;
    }

    const pollIntervalId = window.setInterval(() => {
      void refresh();
    }, isRemote ? REMOTE_POLL_INTERVAL_MS : LOCAL_POLL_INTERVAL_MS);

    return () => {
      window.clearInterval(pollIntervalId);
    };
  }, [isRemote, isVisible, isWindowActive, refresh, sessionType]);

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

  const selectTab = useCallback(async (tabId: string) => {
    /** 폴링을 기다리면 클릭이 굼떠 보이므로 먼저 화면을 바꾸고 실제 상태로 덮어쓴다 */
    setTabs((currentTabs) => currentTabs.map((tab) => ({ ...tab, isActive: tab.id === tabId })));
    await selectTerminalTab(taskId, tabId);
    await refresh();
  }, [refresh, taskId]);

  const createTab = useCallback(async () => {
    await createTerminalTab(taskId);
    await refresh();
  }, [refresh, taskId]);

  const closeTab = useCallback(async (tabId: string) => {
    const result = await closeTerminalTab(taskId, tabId);
    await refresh();
    return result.shouldCloseWindow ?? false;
  }, [refresh, taskId]);

  const renameTab = useCallback(async (tabId: string, name: string) => {
    await renameTerminalTab(taskId, tabId, name);
    await refresh();
  }, [refresh, taskId]);

  const moveTab = useCallback(async (tabId: string, targetIndex: number) => {
    await moveTerminalTab(taskId, tabId, targetIndex);
    await refresh();
  }, [refresh, taskId]);

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
