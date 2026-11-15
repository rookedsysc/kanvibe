import { useCallback, useEffect, useRef, useState } from "react";
import * as actions from "@/desktop/renderer/actions/paneChat";
import type { PaneChatActionResult, PaneChatKey, PaneChatSnapshot } from "@/desktop/shared/paneChat";
import { useIsWindowActive } from "./useIsWindowActive";
import type { AggregatedAiMessage } from "@/lib/aiSessions/types";

const POLL_INTERVAL_MS = 1_500;
const MAX_UNAVAILABLE_READS = 3;

interface PendingMessage {
  id: string;
  conversation: string;
  message: AggregatedAiMessage;
  baseline: string[];
  state: "sending" | "sent" | "error";
}
const messageKey = (message: AggregatedAiMessage) => JSON.stringify([message.timestamp, message.fullText || message.text, message.attachments?.map((item) => item.id)]);
const conversationKey = (snapshot: PaneChatSnapshot) => JSON.stringify([snapshot.taskId, snapshot.paneId, snapshot.conversationKey ?? snapshot.targetToken]);

export function usePaneChat(taskId: string, isVisible = true, paneId?: string) {
  const isActive = useIsWindowActive();
  const [result, setResult] = useState<{ taskId: string; paneId?: string; snapshot: PaneChatSnapshot } | null>(null);
  const [readError, setReadError] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [isReady, setIsReady] = useState(false);
  const [pending, setPending] = useState<PendingMessage[]>([]);
  const confirmedOperations = useRef(new Set<string>());
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
    let unavailableReads = 0;
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
          const isWaitingForInput = snapshot.transcript.reason === "codex-thread-input-busy";
          unavailableReads = snapshot.transcript.status === "unavailable" && !isWaitingForInput ? unavailableReads + 1 : 0;
          current.current = snapshot;
          setIsReady(paneId === undefined || snapshot.paneId === paneId);
          setResult({ taskId, paneId, snapshot });
          setReadError(false);
        } catch {
          if (!disposed) {
            unavailableReads++;
            current.current = null;
            setIsReady(false);
            setReadError(true);
          }
        } finally {
          reading = null;
          if (!disposed && isActive && unavailableReads < MAX_UNAVAILABLE_READS) timer = setTimeout(() => void refresh(), POLL_INTERVAL_MS);
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

  const run = useCallback(async (action: (token: string) => Promise<PaneChatActionResult>, preserveSendReceipt = false): Promise<boolean> => {
    const target = current.current;
    if (!isVisible || busy.current || !target?.canSend || !target.targetToken || target.taskId !== taskId || (paneId !== undefined && target.paneId !== paneId)) return false;
    const epoch = generation.current;
    busy.current = true;
    setIsBusy(true);
    setActionError(null);
    try {
      const accepted = await action(target.targetToken);
      return accepted.success && (preserveSendReceipt || (generation.current === epoch && current.current?.targetToken === target.targetToken));
    } catch (error) {
      if (generation.current === epoch && current.current?.targetToken === target.targetToken) {
        setActionError(error instanceof Error ? error.message : "CLI action failed");
      }
      return false;
    } finally {
      busy.current = false;
      setIsBusy(false);
      if (generation.current === epoch) void refreshRef.current();
    }
  }, [taskId, paneId, isVisible]);

  async function send(text: string, attachmentIds?: string[], requestId?: string) {
    const target = current.current;
    if (!target?.canSend || !target.targetToken || busy.current || !isVisible) return false;
    const id = requestId ?? crypto.randomUUID();
    const conversation = conversationKey(target);
    const isCommand = /^\s*\//.test(text);
    if (!isCommand && !confirmedOperations.current.has(id)) setPending((rows) => [...rows.filter((row) => row.id !== id), {
      id, conversation, state: "sending", baseline: target.transcript.messages.map(messageKey),
      message: { role: "user", timestamp: new Date().toISOString(), text, fullText: text, isTruncated: false },
    }]);
    const accepted = await run((token) => actions.sendPaneChatPrompt(taskId, token, text, attachmentIds, id), true);
    if (!isCommand) setPending((rows) => rows.map((row) => row.id === id ? { ...row, state: accepted ? "sent" : "error" } : row));
    return accepted;
  }

  const snapshot = result?.taskId === taskId && result.paneId === paneId ? result.snapshot : null;
  const usedRows = new Set<string>();
  const acknowledged: string[] = [];
  const pendingMessages = snapshot ? pending.filter((row) => {
    if (row.conversation !== conversationKey(snapshot)) return false;
    const recorded = snapshot.transcript.messages.find((message) => {
      const key = messageKey(message);
      const text = message.fullText || message.text;
      return message.role === "user" && !usedRows.has(key) && !row.baseline.includes(key)
        && (row.message.text ? text === row.message.text || text.startsWith(`${row.message.text}\n\n`) : !!message.attachments?.length);
    });
    if (!recorded) return true;
    acknowledged.push(row.id);
    usedRows.add(messageKey(recorded));
    return false;
  }) : [];
  const acknowledgedKey = JSON.stringify(acknowledged);
  useEffect(() => {
    const ids = new Set<string>(JSON.parse(acknowledgedKey));
    if (!ids.size) return;
    for (const id of ids) confirmedOperations.current.add(id);
    setPending((rows) => rows.filter((row) => !ids.has(row.id)));
  }, [acknowledgedKey]);

  return {
    snapshot, pendingMessages,
    readError, actionError, isBusy,
    isReady: isReady && result?.taskId === taskId && result.paneId === paneId,
    refresh: () => refreshRef.current(),
    send,
    interrupt: () => run((token) => actions.interruptPaneChat(taskId, token)),
    openModelPicker: () => run((token) => actions.openPaneChatModelPicker(taskId, token)),
    sendKey: (key: PaneChatKey) => run((token) => actions.sendPaneChatKey(taskId, token, key)),
  };
}
