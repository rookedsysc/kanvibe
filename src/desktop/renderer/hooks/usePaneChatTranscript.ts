import { useEffect, useMemo, useRef, useState } from "react";
import { getPaneChatTranscriptPage } from "@/desktop/renderer/actions/paneChat";
import type { PaneChatSnapshot } from "@/desktop/shared/paneChat";
import type { AggregatedAiMessage } from "@/lib/aiSessions/types";

/** 최신 폴링 결과와 사용자가 펼친 이전 페이지를 같은 대화에만 합친다. */
export function usePaneChatTranscript(snapshot: PaneChatSnapshot | null) {
  const key = JSON.stringify([snapshot?.taskId, snapshot?.paneId, snapshot?.conversationKey ?? snapshot?.transcript.sessionId]);
  const currentKey = useRef(key);
  currentKey.current = key;
  const [page, setPage] = useState<{ key: string; messages: AggregatedAiMessage[]; nextCursor: string | null } | null>(null);
  const [loading, setLoading] = useState<string | null>(null);
  const loadingRef = useRef<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ownPage = page?.key === key ? page : null;
  useEffect(() => {
    setPage((previous) => {
      if (previous?.key !== key) return previous;
      const known = new Set(previous.messages.map((message) => JSON.stringify([message.role, message.timestamp, message.fullText || message.text, message.attachments?.map((item) => item.id)])));
      const added = (snapshot?.transcript.messages ?? []).filter((message) => !known.has(JSON.stringify([message.role, message.timestamp, message.fullText || message.text, message.attachments?.map((item) => item.id)])));
      return added.length ? { ...previous, messages: [...added, ...previous.messages] } : previous;
    });
  }, [key, snapshot?.transcript.messages]);
  const messages = useMemo(() => {
    const rows = [...snapshot?.transcript.messages ?? [], ...ownPage?.messages ?? []];
    const seen = new Set<string>();
    return rows.filter((message) => {
      const id = JSON.stringify([message.role, message.timestamp, message.fullText || message.text, message.attachments?.map((attachment) => attachment.id)]);
      if (seen.has(id)) return false;
      seen.add(id);
      return true;
    });
  }, [snapshot?.transcript.messages, ownPage]);
  const nextCursor = ownPage ? ownPage.nextCursor : snapshot?.transcript.nextCursor;

  async function loadOlder(beforePrepend: () => void) {
    if (!snapshot?.targetToken || !nextCursor || loadingRef.current === key) return;
    loadingRef.current = key;
    setLoading(key);
    setError(null);
    try {
      const result = await getPaneChatTranscriptPage(snapshot.taskId, snapshot.targetToken, nextCursor);
      if (currentKey.current !== key || result.sessionId !== snapshot.transcript.sessionId) return;
      beforePrepend();
      setPage((previous) => ({ key, messages: [...(previous?.key === key ? previous.messages : []), ...snapshot.transcript.messages, ...result.messages], nextCursor: result.nextCursor }));
    } catch { if (currentKey.current === key) setError(key); }
    finally {
      if (loadingRef.current === key) { loadingRef.current = null; setLoading(null); }
    }
  }
  return { messages, nextCursor, loadOlder, loading: loading === key, error: error === key };
}
