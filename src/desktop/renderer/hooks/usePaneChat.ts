import { useCallback, useEffect, useRef, useState } from "react";
import * as actions from "@/desktop/renderer/actions/paneChat";
import { getPaneChatPromptDialogKey, type PaneChatActionResult, type PaneChatCodexThreadReveal, type PaneChatKey, type PaneChatPrompt, type PaneChatPromptAnswer, type PaneChatSnapshot } from "@/desktop/shared/paneChat";
import { useIsWindowActive } from "./useIsWindowActive";
import type { AggregatedAiMessage } from "@/lib/aiSessions/types";

const POLL_INTERVAL_MS = 1_500;
const UNAVAILABLE_BACKOFF_THRESHOLD = 3;
const UNAVAILABLE_POLL_INTERVAL_MS = 5_000;
/** Electron이 IPC 오류 앞에 붙이는 호출 경로와 오류 이름. 떼어 내면 서비스가 쓴 안내문만 남는다 */
const IPC_ERROR_PREFIX = /^Error invoking remote method '[^']*': (?:\w+: )?/;
const describeActionError = (error: unknown) => error instanceof Error ? error.message.replace(IPC_ERROR_PREFIX, "") : "CLI action failed";

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
  /** 선택 화면에 답을 보내지 못한 이유. 그 선택 화면의 카드에만 보이도록 어느 화면의 일인지 함께 둔다 */
  const [promptError, setPromptError] = useState<{ dialogKey: string; message: string } | null>(null);
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
          if (!disposed && isActive) timer = setTimeout(() => void refresh(), unavailableReads >= UNAVAILABLE_BACKOFF_THRESHOLD ? UNAVAILABLE_POLL_INTERVAL_MS : POLL_INTERVAL_MS);
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

  const run = useCallback(async (action: (token: string) => Promise<PaneChatActionResult>, preserveSendReceipt = false, reportError: (message: string | null) => void = setActionError): Promise<boolean> => {
    const target = current.current;
    if (!isVisible || busy.current || !target?.canSend || !target.targetToken || target.taskId !== taskId || (paneId !== undefined && target.paneId !== paneId)) return false;
    const epoch = generation.current;
    busy.current = true;
    setIsBusy(true);
    reportError(null);
    try {
      const accepted = await action(target.targetToken);
      return accepted.success && (preserveSendReceipt || (generation.current === epoch && current.current?.targetToken === target.targetToken));
    } catch (error) {
      if (generation.current === epoch && current.current?.targetToken === target.targetToken) {
        reportError(describeActionError(error));
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

  /** 대화를 연결하지 못한 Codex에서 사용자가 누른 "대화 ID 표시"를 실행한다. 아직 대상 토큰이 없으므로 pane 기준으로 서비스가 다시 확인한다 */
  async function revealCodexThread(): Promise<PaneChatCodexThreadReveal["outcome"] | null> {
    if (!isVisible || busy.current) return null;
    const epoch = generation.current;
    busy.current = true;
    setIsBusy(true);
    setActionError(null);
    try {
      return (await actions.revealPaneChatCodexThread(taskId, paneId)).outcome;
    } catch (error) {
      if (generation.current === epoch) setActionError(describeActionError(error));
      return null;
    } finally {
      busy.current = false;
      setIsBusy(false);
      if (generation.current === epoch) void refreshRef.current();
    }
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
  // CLI 화면으로 작업 중인지 모르는 경우에도, 보낸 메시지가 기록되기 전까지는 에이전트가 받아 처리하는 중으로 본다
  const hasUnrecordedSend = pendingMessages.some((row) => row.state !== "error");
  const isAgentWorking = snapshot?.agentActivity === "working" || hasUnrecordedSend;
  const acknowledgedKey = JSON.stringify(acknowledged);
  useEffect(() => {
    const ids = new Set<string>(JSON.parse(acknowledgedKey));
    if (!ids.size) return;
    for (const id of ids) confirmedOperations.current.add(id);
    setPending((rows) => rows.filter((row) => !ids.has(row.id)));
  }, [acknowledgedKey]);

  return {
    snapshot, pendingMessages, isAgentWorking,
    readError, actionError, promptError, isBusy,
    isReady: isReady && result?.taskId === taskId && result.paneId === paneId,
    refresh: () => refreshRef.current(),
    send,
    revealCodexThread,
    interrupt: () => run((token) => actions.interruptPaneChat(taskId, token)),
    openModelPicker: () => run((token) => actions.openPaneChatModelPicker(taskId, token)),
    sendKey: (key: PaneChatKey) => run((token) => actions.sendPaneChatKey(taskId, token, key)),
    answerPrompt: (prompt: PaneChatPrompt, answer: PaneChatPromptAnswer) => run(async (token) => {
      const accepted = await actions.answerPaneChatPrompt(taskId, token, prompt.id, answer);
      // 바뀐 화면을 받아 온 뒤에야 카드를 다시 누를 수 있게 해, 다음 답이 낡은 카드에서 나가지 않게 한다
      await refreshRef.current();
      return accepted;
    }, false, (message) => setPromptError(message ? { dialogKey: getPaneChatPromptDialogKey(prompt), message } : null)),
  };
}
