import { useCallback, useEffect, useRef, useState } from "react";
import * as actions from "@/desktop/renderer/actions/paneChat";
import type { PaneChatActionResult, PaneChatKey, PaneChatSnapshot } from "@/desktop/shared/paneChat";
import { useIsWindowActive } from "./useIsWindowActive";

const POLL_INTERVAL_MS = 1_500;

export function usePaneChat(taskId: string, isVisible = true, paneId?: string) {
  const isActive = useIsWindowActive();
  const [result, setResult] = useState<{ taskId: string; paneId?: string; snapshot: PaneChatSnapshot } | null>(null);
  const [readError, setReadError] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [isReady, setIsReady] = useState(false);
  const current = useRef<PaneChatSnapshot | null>(null);
  const generation = useRef(0);
  const busy = useRef(false);
  const refreshRef = useRef<() => Promise<void>>(async () => {});

  useEffect(() => {
    ++generation.current;
    if (!isVisible) {
      current.current = null;
      setIsReady(false);
      refreshRef.current = async () => {};
      return;
    }
    let disposed = false;
    let reading: Promise<void> | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (current.current?.taskId !== taskId || (paneId !== undefined && current.current?.paneId !== paneId)) {
      current.current = null;
      setIsReady(false);
    }
    setReadError(false);
    const refresh = (): Promise<void> => {
      if (reading) return reading;
      if (timer) clearTimeout(timer);
      reading = (async () => {
        try {
          const snapshot = await actions.getPaneChatSnapshot(taskId, paneId);
          if (disposed) return;
          current.current = snapshot;
          setIsReady(paneId === undefined || snapshot.paneId === paneId);
          setResult({ taskId, paneId, snapshot });
          setReadError(false);
        } catch {
          if (!disposed) {
            current.current = null;
            setIsReady(false);
            setReadError(true);
          }
        } finally {
          reading = null;
          if (!disposed && isActive) timer = setTimeout(() => void refresh(), POLL_INTERVAL_MS);
        }
      })();
      return reading;
    };
    refreshRef.current = refresh;
    void refresh();
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
    };
  }, [taskId, paneId, isActive, isVisible]);

  const run = useCallback(async (action: (token: string) => Promise<PaneChatActionResult>): Promise<boolean> => {
    const target = current.current;
    if (!isVisible || busy.current || !target?.canSend || !target.targetToken || target.taskId !== taskId || (paneId !== undefined && target.paneId !== paneId)) return false;
    const epoch = generation.current;
    busy.current = true;
    setIsBusy(true);
    setActionError(null);
    try {
      const accepted = await action(target.targetToken);
      return accepted.success;
    } catch (error) {
      if (generation.current === epoch && current.current?.targetToken === target.targetToken) {
        setActionError(error instanceof Error ? error.message : "CLI action failed");
      }
      return false;
    } finally {
      busy.current = false;
      setIsBusy(false);
      if (generation.current === epoch) await refreshRef.current();
    }
  }, [taskId, paneId, isVisible]);

  return {
    snapshot: result?.taskId === taskId && result.paneId === paneId ? result.snapshot : null,
    readError, actionError, isBusy,
    isReady: isReady && result?.taskId === taskId && result.paneId === paneId,
    refresh: () => refreshRef.current(),
    send: (text: string, attachmentIds?: string[], requestId?: string) => run((token) => actions.sendPaneChatPrompt(taskId, token, text, attachmentIds, requestId)),
    interrupt: () => run((token) => actions.interruptPaneChat(taskId, token)),
    openModelPicker: () => run((token) => actions.openPaneChatModelPicker(taskId, token)),
    sendKey: (key: PaneChatKey) => run((token) => actions.sendPaneChatKey(taskId, token, key)),
  };
}
