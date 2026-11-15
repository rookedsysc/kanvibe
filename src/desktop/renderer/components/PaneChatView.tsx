import { getChatTranscriptRows } from "@/desktop/renderer/utils/chatTranscript";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { AI_SESSION_PROVIDER_LABELS } from "@/lib/aiSessions/providers";
import type { AggregatedAiMessage } from "@/lib/aiSessions/types";
import { usePaneChat } from "@/desktop/renderer/hooks/usePaneChat";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";
import { PaneChatComposer } from "./PaneChatComposer";
import { AiChatTranscriptMessage } from "./AiChatTranscriptMessage";
import { AiChatFilePreview } from "./AiChatFilePreview";

const NEAR_BOTTOM_PX = 96;
const EMPTY_MESSAGES: AggregatedAiMessage[] = [];
const controlClass = "min-h-10 rounded-md border border-border-default bg-bg-surface px-3 text-xs text-text-primary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40";

export function PaneChatView({ taskId, paneId, taskTitle, paneTabs = [], isVisible = true, onHistory, onTerminal }: {
  taskId: string;
  paneId?: string;
  taskTitle?: string;
  paneTabs?: Pick<TerminalTab, "id" | "name">[];
  isVisible?: boolean;
  onHistory: () => void;
  onTerminal: () => void;
}) {
  const t = useTranslations("taskDetail.paneChat");
  const chat = usePaneChat(taskId, isVisible, paneId);
  const snapshot = chat.snapshot;
  const targetKey = snapshot?.targetToken ?? `${taskId}:unavailable`;
  const [isDetached, setIsDetached] = useState(false);
  const [previewFilePath, setPreviewFilePath] = useState<string | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const followLatest = useRef(true);
  const messages = snapshot?.transcript.messages ?? EMPTY_MESSAGES;
  const paneName = paneTabs.find((tab) => tab.id === snapshot?.paneId)?.name;

  useEffect(() => {
    followLatest.current = true;
    setIsDetached(false);
    setPreviewFilePath(null);
  }, [targetKey]);

  useLayoutEffect(() => {
    if (followLatest.current && viewport.current) viewport.current.scrollTop = viewport.current.scrollHeight;
  }, [messages, targetKey, isVisible]);

  async function openModelPicker() {
    if (await chat.openModelPicker()) onTerminal();
  }

  return (
    <section data-testid="pane-chat" className="relative flex min-h-0 flex-1 flex-col bg-bg-page">
      <header className="flex min-h-14 flex-wrap items-center gap-2 border-b border-border-subtle px-4 py-2">
        {snapshot?.provider ? <AiProviderIcon provider={snapshot.provider} size={18} /> : null}
        <div className="min-w-0 flex-1 basis-28">
          <h2 title={taskTitle} className="truncate text-sm font-medium text-text-primary">{taskTitle || t("currentPane")}</h2>
          <p title={snapshot?.paneId ?? undefined} aria-label={snapshot?.paneId ?? undefined} className="truncate text-xs text-text-secondary">{[paneName, snapshot?.provider ? AI_SESSION_PROVIDER_LABELS[snapshot.provider] : null, snapshot?.paneId?.slice(-8)].filter(Boolean).join(" · ") || t("loading")}</p>
        </div>
        <div className="ml-auto flex shrink-0 gap-2">
          <button type="button" onClick={onHistory} className={controlClass}>{t("history")}</button>
          <button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button>
        </div>
      </header>
      {chat.readError ? <div role="alert" className="flex items-center justify-between gap-3 px-4 py-2 text-xs text-status-error">{t("readError")}<button type="button" onClick={() => void chat.refresh()} className={controlClass}>{t("retry")}</button></div> : null}
      <div ref={viewport} data-testid="pane-chat-transcript" className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable] px-4 py-6 sm:px-8" onScroll={() => {
        const node = viewport.current;
        if (!node) return;
        followLatest.current = node.scrollHeight - node.scrollTop - node.clientHeight < NEAR_BOTTOM_PX;
        setIsDetached(!followLatest.current);
      }}>
        <div className="mx-auto max-w-3xl space-y-2.5">
          {!snapshot && !chat.readError ? <p role="status" className="text-sm text-text-secondary">{t("loading")}</p> : null}
          {snapshot && !snapshot.canSend && !snapshot.provider ? <div className="space-y-4"><p className="text-sm text-text-secondary">{t("noAgent")}</p><button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button></div> : null}
          {snapshot?.canSend && snapshot.transcript.status === "available" ? <>
            {snapshot.transcript.nextCursor ? <button type="button" onClick={onHistory} className="text-xs text-text-brand">{t("olderHint")}</button> : null}
            {messages.length === 0 ? <p className="text-sm text-text-secondary">{t("empty")}</p> : null}
            {getChatTranscriptRows(messages, snapshot.transcript.sessionId).map(({ message, key }) => (
              <AiChatTranscriptMessage key={key} message={message} provider={snapshot.provider!} attachmentContext={snapshot.transcript.sessionId ? { taskId, provider: snapshot.provider!, sessionId: snapshot.transcript.sessionId } : undefined} onOpenFile={(file) => setPreviewFilePath(file.path)} />
            ))}
          </> : null}
          {snapshot?.provider && snapshot.transcript.status === "unavailable" ? <div role="status" className="space-y-3 rounded-lg bg-bg-surface p-4 text-sm text-text-secondary">
            <p>{t("transcriptUnavailable")}</p>
            <p>{t(snapshot.transcript.reason === "transcript-read-failed" ? "transcriptReadFailed" : "transcriptIdentityMissing")}</p>
            <div className="flex flex-wrap gap-2"><button type="button" onClick={() => void chat.refresh()} className={controlClass}>{t("retry")}</button><button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button></div>
          </div> : null}
          {snapshot?.screen && snapshot.transcript.status !== "available" ? <details className="rounded-lg border border-border-subtle bg-bg-surface">
            <summary className="cursor-pointer px-4 py-3 text-xs text-text-secondary">{t("screen")}</summary>
            <pre className="max-h-80 overflow-auto whitespace-pre px-4 pb-4 font-mono text-xs leading-5 text-text-primary">{snapshot.screen}</pre>
          </details> : null}
        </div>
      </div>
      {isDetached ? <button type="button" onClick={() => {
        followLatest.current = true;
        if (viewport.current) viewport.current.scrollTop = viewport.current.scrollHeight;
        setIsDetached(false);
      }} className="absolute bottom-44 left-1/2 -translate-x-1/2 rounded-full border border-border-default bg-bg-surface px-4 py-2 text-xs text-text-primary">{t("jumpToLatest")} ↓</button> : null}
      {previewFilePath ? <AiChatFilePreview taskId={taskId} filePath={previewFilePath} onClose={() => setPreviewFilePath(null)} /> : null}
      <PaneChatComposer taskId={taskId} chat={chat} isVisible={isVisible} onModel={() => void openModelPicker()} />
    </section>
  );
}
