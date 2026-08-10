import { useCallback, useEffect, useState } from "react";
import { getRunningAgentPanes, getTaskLiveAiSessions } from "@/desktop/renderer/actions/project";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";
import type { LiveAiSession, RunningAgentPane } from "@/lib/aiSessions/types";

/** 세션 패널은 서브태스크가 뜨고 지는 것을 눈으로 따라갈 수 있어야 해서 짧게 돈다 */
const LIVE_SESSION_POLL_INTERVAL_MS = 2_000;

/** 보드 배지는 provider가 붙었는지만 보여주므로 더 느리게 돌아도 된다 */
const RUNNING_PANE_POLL_INTERVAL_MS = 5_000;

/**
 * 조회 결과를 주기적으로 새로 읽는다.
 * 앞선 조회가 아직 안 끝났으면 건너뛰어, 원격처럼 느린 호출에서 요청이 쌓이지 않게 한다.
 *
 * 창이 가려져 있으면 주기 조회는 멈추되 첫 조회는 한 번 한다.
 * 그래야 창이 다시 앞으로 나왔을 때 빈 화면부터 보여주지 않는다.
 */
function usePolledValue<T>(
  read: () => Promise<T>,
  emptyValue: T,
  intervalMs: number,
  isEnabled: boolean,
): T {
  const [value, setValue] = useState<T>(emptyValue);
  const isWindowActive = useIsWindowActive();

  useEffect(() => {
    if (!isEnabled) {
      return;
    }

    let isCancelled = false;
    let isReading = false;

    const readOnce = async () => {
      if (isReading) {
        return;
      }

      isReading = true;
      try {
        const nextValue = await read();
        if (!isCancelled) {
          setValue(nextValue);
        }
      } catch {
        // 폴링 실패는 다음 주기에 다시 시도한다. 화면을 오류로 덮지 않는다.
      } finally {
        isReading = false;
      }
    };

    void readOnce();

    if (!isWindowActive) {
      return () => {
        isCancelled = true;
      };
    }

    const pollIntervalId = window.setInterval(() => {
      void readOnce();
    }, intervalMs);

    return () => {
      isCancelled = true;
      window.clearInterval(pollIntervalId);
    };
  }, [intervalMs, isEnabled, isWindowActive, read]);

  return value;
}

const EMPTY_SESSIONS: LiveAiSession[] = [];
const EMPTY_PANES: RunningAgentPane[] = [];

/** 태스크 하나의 실행중 세션과 서브태스크. 패널이 열려 있는 동안에만 폴링한다 */
export function useTaskLiveAiSessions(taskId: string | null, isEnabled: boolean): LiveAiSession[] {
  const read = useCallback(
    async () => (taskId ? (await getTaskLiveAiSessions(taskId)).sessions : EMPTY_SESSIONS),
    [taskId],
  );

  return usePolledValue(read, EMPTY_SESSIONS, LIVE_SESSION_POLL_INTERVAL_MS, isEnabled && Boolean(taskId));
}

/** 보드 전체가 공유하는 실행중 에이전트 목록. 카드마다 조회하지 않도록 한 번만 읽는다 */
export function useRunningAgentPanes(isEnabled: boolean): RunningAgentPane[] {
  const read = useCallback(() => getRunningAgentPanes(), []);

  return usePolledValue(read, EMPTY_PANES, RUNNING_PANE_POLL_INTERVAL_MS, isEnabled);
}
