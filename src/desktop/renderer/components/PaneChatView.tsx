import { getChatTranscriptRows } from "@/desktop/renderer/utils/chatTranscript";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { AI_SESSION_PROVIDER_LABELS } from "@/lib/aiSessions/providers";
import type { AggregatedAiMessage } from "@/lib/aiSessions/types";
import { usePaneChat } from "@/desktop/renderer/hooks/usePaneChat";
import type { PaneChatKey } from "@/desktop/shared/paneChat";
import { AiChatTranscriptMessage } from "./AiChatTranscriptMessage";

const NEAR_BOTTOM_PX = 96;
const EMPTY_MESSAGES: AggregatedAiMessage[] = [];
const controlClass = "min-h-10 rounded-md border border-border-default bg-bg-surface px-3 text-xs text-text-primary hover:bg-button-neutral focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40";

export function PaneChatView({ taskId, isVisible = true, onHistory, onTerminal }: {
  taskId: string;
  isVisible?: boolean;
  onHistory: () => void;
  onTerminal: () => void;
}) {
  const t = useTranslations("taskDetail.paneChat");
  const chat = usePaneChat(taskId, isVisible);
  const snapshot = chat.snapshot;
  const targetKey = snapshot?.targetToken ?? `${taskId}:unavailable`;
  const currentTarget = useRef(targetKey);
  currentTarget.current = targetKey;
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [controlsTarget, setControlsTarget] = useState<string | null>(null);
  const [sendFailed, setSendFailed] = useState(false);
  const [isDetached, setIsDetached] = useState(false);
  const viewport = useRef<HTMLDivElement>(null);
  const followLatest = useRef(true);
  const draft = drafts[targetKey] ?? "";
  const controlsOpen = controlsTarget === targetKey;
  const disconnected = !snapshot?.canSend || chat.readError || !chat.isReady;
  const disabled = disconnected || chat.isBusy;
  const inputReady = snapshot?.inputState === "ready";
  const messages = snapshot?.transcript.messages ?? EMPTY_MESSAGES;

  useEffect(() => {
    followLatest.current = true;
    setIsDetached(false);
    setSendFailed(false);
  }, [targetKey]);

  useLayoutEffect(() => {
    if (followLatest.current && viewport.current) viewport.current.scrollTop = viewport.current.scrollHeight;
  }, [messages, targetKey, isVisible]);

  async function submit() {
    if (!draft.trim() || disabled || controlsOpen || !inputReady) return;
    const key = targetKey;
    const text = draft;
    setSendFailed(false);
    const accepted = await chat.send(text);
    if (accepted) {
      setDrafts((previous) => previous[key] === text ? { ...previous, [key]: "" } : previous);
    } else if (currentTarget.current === key) {
      setSendFailed(true);
    }
  }

  async function openModelPicker() {
    if (await chat.openModelPicker()) setControlsTarget(targetKey);
  }

  async function navigate(key: PaneChatKey) {
    const accepted = await chat.sendKey(key);
    if (accepted && key === "escape") setControlsTarget(null);
  }

  return (
    <section data-testid="pane-chat" className="relative flex min-h-0 flex-1 flex-col bg-bg-page">
      <header className="flex min-h-14 flex-wrap items-center gap-2 border-b border-border-subtle px-4 py-2">
        {snapshot?.provider ? <AiProviderIcon provider={snapshot.provider} size={18} /> : null}
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-medium text-text-primary">{snapshot?.provider ? AI_SESSION_PROVIDER_LABELS[snapshot.provider] : t("currentPane")}</h2>
          <p className="truncate text-xs text-text-muted">{snapshot?.paneId ?? t("loading")}</p>
        </div>
        <button type="button" onClick={onHistory} className={controlClass}>{t("history")}</button>
        <button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button>
      </header>
      {chat.readError ? <div role="alert" className="flex items-center justify-between gap-3 px-4 py-2 text-xs text-status-error">{t("readError")}<button type="button" onClick={() => void chat.refresh()} className={controlClass}>{t("retry")}</button></div> : null}
      {(sendFailed || chat.actionError) ? <div role="alert" className="px-4 py-2 text-xs text-status-error">{chat.actionError || t("sendError")}</div> : null}
      <div ref={viewport} data-testid="pane-chat-transcript" className="min-h-0 flex-1 overflow-y-auto px-4 py-6 sm:px-8" onScroll={() => {
        const node = viewport.current;
        if (!node) return;
        followLatest.current = node.scrollHeight - node.scrollTop - node.clientHeight < NEAR_BOTTOM_PX;
        setIsDetached(!followLatest.current);
      }}>
        <div className="mx-auto max-w-3xl space-y-6">
          {!snapshot && !chat.readError ? <p role="status" className="text-sm text-text-muted">{t("loading")}</p> : null}
          {snapshot && !snapshot.canSend ? <div className="space-y-4"><p className="text-sm text-text-secondary">{t("noAgent")}</p><button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button></div> : null}
          {snapshot?.canSend && snapshot.transcript.status === "available" ? <>
            {snapshot.transcript.nextCursor ? <button type="button" onClick={onHistory} className="text-xs text-text-brand">{t("olderHint")}</button> : null}
            {messages.length === 0 ? <p className="text-sm text-text-muted">{t("empty")}</p> : null}
            {getChatTranscriptRows(messages, snapshot.transcript.sessionId).map(({ message, key }) => (
              <AiChatTranscriptMessage key={key} message={message} provider={snapshot.provider!} />
            ))}
          </> : null}
          {snapshot?.canSend && snapshot.transcript.status === "unavailable" ? <p className="text-xs text-text-muted">{t("transcriptPending")}</p> : null}
          {snapshot?.screen && !controlsOpen ? <details open={snapshot.transcript.status === "unavailable"} className="rounded-lg border border-border-subtle bg-bg-surface">
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
      {controlsOpen ? <div data-testid="pane-chat-controls" role="region" aria-label={t("controls")} tabIndex={0} className="mx-4 mb-3 rounded-lg border border-border-default bg-bg-surface p-3 focus-visible:outline-2 focus-visible:outline-brand-primary" onKeyDown={(event) => {
        const keys: Record<string, PaneChatKey> = { ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right", Enter: "enter", Escape: "escape" };
        if (event.target !== event.currentTarget || !keys[event.key] || disabled) return;
        event.preventDefault();
        void navigate(keys[event.key]);
      }}>
        <div className="mb-2 flex items-center justify-between gap-2"><p className="text-xs font-medium text-text-primary">{t("screen")}</p><button type="button" onClick={() => setControlsTarget(null)} className="min-h-9 px-2 text-xs text-text-secondary">{t("closeControls")}</button></div>
        <pre aria-live="polite" className="max-h-[32vh] overflow-auto whitespace-pre font-mono text-xs leading-5 text-text-primary">{snapshot?.screen}</pre>
        <p className="my-2 text-xs text-text-muted">{t("modelHelp")}</p>
        <div className="flex flex-wrap gap-2">
          {(["up", "down", "left", "right"] as const).map((key) => <button key={key} type="button" disabled={disabled} aria-label={t(key)} onClick={() => void navigate(key)} className={controlClass}>{{ up: "↑", down: "↓", left: "←", right: "→" }[key]}</button>)}
          <button type="button" disabled={disabled} onClick={() => void navigate("enter")} className={controlClass}>{t("confirm")}</button>
          <button type="button" disabled={disabled} onClick={() => void navigate("escape")} className={controlClass}>{t("cancel")}</button>
        </div>
      </div> : null}
      <form onSubmit={(event) => { event.preventDefault(); void submit(); }} className="shrink-0 border-t border-border-subtle px-4 py-3 sm:px-8">
        <div className="mx-auto max-w-3xl">
          <div className="mb-2 flex flex-wrap gap-2">
            <button type="button" disabled={disabled || !snapshot?.modelPicker.supported || !inputReady || controlsOpen} onClick={() => void openModelPicker()} className={controlClass}>{t("model")}</button>
            <button type="button" disabled={disabled} onClick={() => setControlsTarget(controlsOpen ? null : targetKey)} className={controlClass}>{t("controls")}</button>
            <button type="button" disabled={disabled} onClick={() => void chat.interrupt()} className={controlClass}>{t("stop")}</button>
          </div>
          <div className="flex items-end gap-2 rounded-xl border border-border-default bg-bg-surface p-2 focus-within:border-brand-primary">
            <textarea aria-label={t("placeholder")} placeholder={t("placeholder")} rows={3} value={draft} disabled={!chat.isReady || !snapshot?.targetToken || chat.isBusy} onChange={(event) => { setDrafts((previous) => ({ ...previous, [targetKey]: event.target.value })); setSendFailed(false); }} onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void submit(); }
            }} className="max-h-48 min-h-16 min-w-0 flex-1 resize-y bg-transparent px-2 py-1 text-sm leading-6 text-text-primary outline-none placeholder:text-text-muted" />
            <button type="submit" disabled={disabled || !draft.trim() || controlsOpen || !inputReady} className="min-h-10 shrink-0 rounded-lg bg-brand-primary px-4 text-xs font-medium text-white hover:bg-brand-hover disabled:opacity-40">{chat.isBusy ? t("sending") : t("send")}</button>
          </div>
          <p className="mt-2 text-xs text-text-muted">{snapshot?.canSend && !inputReady ? t(snapshot.inputState === "occupied" ? "inputOccupied" : "inputWaiting") : t("draftHint")}</p>
        </div>
      </form>
    </section>
  );
}
