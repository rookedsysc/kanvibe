import { useEffect, useState } from "react";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";

/**
 * 조회 결과를 주기적으로 새로 읽는다.
 * 앞선 조회가 아직 안 끝났으면 건너뛰어, 원격처럼 느린 호출에서 요청이 쌓이지 않게 한다.
 *
 * 창이 가려져 있으면 주기 조회는 멈추되 첫 조회는 한 번 한다.
 * 그래야 창이 다시 앞으로 나왔을 때 빈 화면부터 보여주지 않는다.
 */
export function usePolledValue<T>(
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
