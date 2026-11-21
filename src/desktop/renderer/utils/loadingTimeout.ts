import { useCallback, useState } from "react";

export const INITIAL_DESKTOP_LOAD_TIMEOUT_MS = 5000;

export function logDesktopInitialLoadTimeout(
  routeName: string,
  details: Record<string, unknown> = {},
) {
  window.kanvibeDesktop?.logRendererError?.("renderer:initial-load-timeout", {
    routeName,
    timeoutMs: INITIAL_DESKTOP_LOAD_TIMEOUT_MS,
    ...details,
  });
}

/** 라우트 조회 한 번의 수명. 성공이나 실패로 끝내고, effect를 정리할 때는 취소한다 */
export interface RouteLoadAttempt {
  succeed: () => void;
  fail: (error: unknown) => void;
  cancel: () => void;
}

/**
 * 라우트 조회가 늦어지거나 실패한 상태를 데이터와 따로 들고 있는다.
 * 늦어진 조회를 빈 보드나 기본 설정으로 바꾸면 사용자가 데이터가 사라졌다고 오해하므로,
 * 화면은 이 상태로 지연 안내나 오류·재시도를 보이고 데이터는 실제 응답으로만 채운다.
 */
export function useRouteLoadStatus() {
  const [isLoadDelayed, setIsLoadDelayed] = useState(false);
  const [loadFailureReason, setLoadFailureReason] = useState<string | null>(null);
  const [retrySignal, setRetrySignal] = useState(0);

  const beginLoad = useCallback((routeName: string, details: Record<string, unknown> = {}): RouteLoadAttempt => {
    let timeoutId: number | null = window.setTimeout(() => {
      timeoutId = null;
      logDesktopInitialLoadTimeout(routeName, details);
      setIsLoadDelayed(true);
    }, INITIAL_DESKTOP_LOAD_TIMEOUT_MS);

    const stopTimer = () => {
      if (timeoutId === null) {
        return;
      }

      window.clearTimeout(timeoutId);
      timeoutId = null;
    };

    return {
      succeed: () => {
        stopTimer();
        setIsLoadDelayed(false);
        setLoadFailureReason(null);
      },
      fail: (error: unknown) => {
        stopTimer();
        setIsLoadDelayed(false);
        setLoadFailureReason(error instanceof Error ? error.message : String(error));
      },
      cancel: stopTimer,
    };
  }, []);

  const retryLoad = useCallback(() => {
    setLoadFailureReason(null);
    setRetrySignal((count) => count + 1);
  }, []);

  return { isLoadDelayed, loadFailureReason, retrySignal, beginLoad, retryLoad };
}
