import { useCallback, useRef, useState } from "react";

export interface TrackedAction<Args extends unknown[]> {
  isPending: boolean;
  /** 진행 중인 실행의 인자. 여러 대상 중 어느 것을 처리하는지 화면에 보일 때 쓴다 */
  pendingArgs: Args | null;
  /** 마지막 실패의 사유. 다시 시도하거나 clearError를 부르기 전까지 남는다 */
  error: string | null;
  run: (...args: Args) => Promise<void>;
  retry: () => Promise<void>;
  clearError: () => void;
}

/**
 * 버튼·단축키로 시작하는 비동기 작업의 진행·실패 상태를 들고 있는다.
 * 진행 중에 다시 부르면 무시해 같은 작업이 겹쳐 나가지 않게 하고, 실패는 사유로 남겨 화면이 재시도를 보이게 한다.
 */
export function useTrackedAction<Args extends unknown[]>(
  action: (...args: Args) => Promise<unknown>,
): TrackedAction<Args> {
  const [pendingArgs, setPendingArgs] = useState<Args | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** state는 다음 렌더에야 바뀌므로, 같은 틱에 연달아 들어온 실행은 ref로 막는다 */
  const isRunningRef = useRef(false);
  const lastArgsRef = useRef<Args | null>(null);
  const actionRef = useRef(action);
  actionRef.current = action;

  const run = useCallback(async (...args: Args) => {
    if (isRunningRef.current) {
      return;
    }

    isRunningRef.current = true;
    lastArgsRef.current = args;
    setPendingArgs(args);
    setError(null);
    try {
      await actionRef.current(...args);
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      isRunningRef.current = false;
      setPendingArgs(null);
    }
  }, []);

  const retry = useCallback(async () => {
    if (lastArgsRef.current) {
      await run(...lastArgsRef.current);
    }
  }, [run]);

  const clearError = useCallback(() => setError(null), []);

  return {
    isPending: pendingArgs !== null,
    pendingArgs,
    error,
    run,
    retry,
    clearError,
  };
}
