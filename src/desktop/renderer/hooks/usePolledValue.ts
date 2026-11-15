import { useEffect, useRef, useState } from "react";
import { useIsWindowActive } from "@/desktop/renderer/hooks/useIsWindowActive";

/** 같은 조회 함수에 대한 읽기 상태. 창 활성 여부가 바뀌어 효과가 다시 돌아도 이어서 쓴다 */
interface PolledReadState<T> {
  read: () => Promise<T>;
  isReading: boolean;
  hasStartedRead: boolean;
  isSubscribed: boolean;
}

/**
 * 조회 결과를 주기적으로 새로 읽는다.
 * 앞선 조회가 아직 안 끝났으면 건너뛰어, 원격처럼 느린 호출에서 요청이 쌓이지 않게 한다.
 *
 * 창이 가려져 있으면 주기 조회는 멈추되 첫 조회는 한 번 한다.
 * 그래야 창이 다시 앞으로 나왔을 때 빈 화면부터 보여주지 않는다.
 * 이미 읽은 창이 가려질 때는 다시 읽지 않는다. 가려지는 순간마다 읽으면 원격 조회가 창 전환 횟수만큼 더 나간다.
 */
export function usePolledValue<T>(
  read: () => Promise<T>,
  emptyValue: T,
  intervalMs: number,
  isEnabled: boolean,
): T {
  const [result, setResult] = useState<{ read: () => Promise<T>; value: T }>({ read, value: emptyValue });
  const isWindowActive = useIsWindowActive();
  const readStateRef = useRef<PolledReadState<T> | null>(null);

  useEffect(() => {
    if (!isEnabled) {
      return;
    }

    if (readStateRef.current?.read !== read) {
      readStateRef.current = { read, isReading: false, hasStartedRead: false, isSubscribed: false };
    }
    const readState = readStateRef.current;
    readState.isSubscribed = true;

    const readOnce = async () => {
      if (readState.isReading) {
        return;
      }

      readState.isReading = true;
      readState.hasStartedRead = true;
      try {
        const nextValue = await read();
        // 창 활성 여부만 바뀐 사이에 끝난 조회는 그대로 반영하고, 조회 대상이 바뀌었거나 화면이 사라졌으면 버린다.
        if (readState.isSubscribed) {
          setResult({ read, value: nextValue });
        }
      } catch {
        // 폴링 실패는 다음 주기에 다시 시도한다. 화면을 오류로 덮지 않는다.
      } finally {
        readState.isReading = false;
      }
    };

    if (isWindowActive || !readState.hasStartedRead) {
      void readOnce();
    }

    if (!isWindowActive) {
      return () => {
        readState.isSubscribed = false;
      };
    }

    const pollIntervalId = window.setInterval(() => {
      void readOnce();
    }, intervalMs);

    return () => {
      readState.isSubscribed = false;
      window.clearInterval(pollIntervalId);
    };
  }, [intervalMs, isEnabled, isWindowActive, read]);

  return result.read === read ? result.value : emptyValue;
}
