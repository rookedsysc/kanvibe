import { getChatTranscriptRows } from "@/desktop/renderer/utils/chatTranscript";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { AI_SESSION_PROVIDER_LABELS } from "@/lib/aiSessions/providers";
import { usePaneChat } from "@/desktop/renderer/hooks/usePaneChat";
import { usePaneChatTranscript } from "@/desktop/renderer/hooks/usePaneChatTranscript";
import { usePaneChatPreferences } from "@/desktop/renderer/hooks/usePaneChatPreferences";
import { isAiInstructionMessage } from "@/lib/aiSessions/messageInstructions";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";
import { getPaneChatPromptDialogKey, type PaneChatCodexThreadReveal } from "@/desktop/shared/paneChat";
import type { AiSessionProvider } from "@/lib/aiSessions/types";
import { PaneChatComposer } from "./PaneChatComposer";
import { AiChatTranscriptMessage } from "./AiChatTranscriptMessage";
import { AiChatFilePreview } from "./AiChatFilePreview";
import { PaneChatModelPicker } from "./PaneChatModelPicker";
import { PaneChatPromptCard } from "./PaneChatPromptCard";

const NEAR_BOTTOM_PX = 96;
const REVEAL_OUTCOME_MESSAGES: Record<PaneChatCodexThreadReveal["outcome"], string> = {
  revealed: "revealRevealed",
  "input-not-empty": "revealInputNotEmpty",
  "unsupported-version": "revealUnsupportedVersion",
  "thread-not-found": "revealThreadNotFound",
};

/** footer에서 대화 ID를 찾지 못한 Codex. 사용자가 "대화 ID 표시"로 연결할 수 있는 상태다 */
function isCodexFooterReason(reason: string | null): boolean {
  return reason === "codex-thread-unconfirmed" || reason === "codex-thread-input-busy";
}

/** 대화를 연결하지 못한 원인에 맞는 안내 문구 키 */
function describeUnavailableTranscript(provider: AiSessionProvider, reason: string | null): string {
  if (reason === "transcript-read-failed") return "transcriptReadFailed";
  if (reason === "codex-thread-record-missing") return "codexRecordMissing";
  if (provider === "claude") return "claudeIdentityMissing";
  if (provider === "codex") return "codexIdentityMissing";
  return "transcriptIdentityMissing";
}
/**
 * 보낸 메시지를 에이전트가 처리하는 동안 대화 끝에 붙는 표시. Claude 데스크톱 앱처럼 제공자 아이콘이 숨 쉬듯 깜빡이고 경과 초가 올라가
 * 기록이 아직 늘지 않아도 일이 진행 중임을 알 수 있다. 작업이 끝나면 사라지고 다음 작업에서 다시 0초부터 센다.
 */
function PaneChatWorkingIndicator({ provider }: { provider: AiSessionProvider | null }) {
  const t = useTranslations("taskDetail.paneChat");
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  useEffect(() => {
    const startedAt = Date.now();
    const timer = setInterval(() => setElapsedSeconds(Math.floor((Date.now() - startedAt) / 1_000)), 1_000);
    return () => clearInterval(timer);
  }, []);
  return (
    <div data-testid="pane-chat-working" role="status" className="flex items-center gap-2 py-2 text-xs text-text-secondary">
      {provider ? <span aria-hidden="true" className="motion-safe:animate-pulse"><AiProviderIcon provider={provider} size={16} /></span> : null}
      <span>{t("agentWorking")}</span>
      {elapsedSeconds > 0 ? <span className="tabular-nums">{t("agentWorkingElapsed", { seconds: elapsedSeconds })}</span> : null}
    </div>
  );
}

/** 아직 주고받은 메시지가 없는 대화의 첫 화면. 바로 아래 입력창과 한 묶음으로 화면 가운데에 놓여, 지금 써도 되는 화면임을 알린다 */
function PaneChatWelcome({ provider }: { provider: AiSessionProvider }) {
  const t = useTranslations("taskDetail.paneChat");
  return (
    <div data-testid="pane-chat-welcome" className="flex flex-col items-center gap-2 pb-2 text-center">
      <div className="flex items-center gap-3">
        <AiProviderIcon provider={provider} size={28} />
        <h3 className="text-xl font-medium text-text-primary">{t("welcomeTitle", { provider: AI_SESSION_PROVIDER_LABELS[provider] })}</h3>
      </div>
      <p className="text-sm text-text-secondary">{t("welcomeHint")}</p>
    </div>
  );
}

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
  const transcript = usePaneChatTranscript(snapshot);
  const preferences = usePaneChatPreferences();
  const targetKey = snapshot?.targetToken ?? `${taskId}:unavailable`;
  const [isDetached, setIsDetached] = useState(false);
  const [previewFilePath, setPreviewFilePath] = useState<string | null>(null);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [revealOutcome, setRevealOutcome] = useState<PaneChatCodexThreadReveal["outcome"] | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const followLatest = useRef(true);
  const messages = transcript.messages;
  const visibleMessages = preferences.showInstructions ? messages : messages.filter((message) => !isAiInstructionMessage(message));
  const scrollAnchor = useRef<{ top: number; height: number } | null>(null);
  const paneName = paneTabs.find((tab) => tab.id === snapshot?.paneId)?.name;
  const isWaitingForInput = snapshot?.transcript.reason === "codex-thread-input-busy";
  const prompt = snapshot?.prompt;
  const promptDialogKey = prompt ? getPaneChatPromptDialogKey(prompt) : null;
  const isNewConversation = !!snapshot?.canSend && snapshot.transcript.status === "available" && visibleMessages.length === 0
    && !transcript.nextCursor && chat.pendingMessages.length === 0 && !chat.isAgentWorking;

  useEffect(() => {
    followLatest.current = true;
    setIsDetached(false);
    setPreviewFilePath(null);
    setModelPickerOpen(false);
    setRevealOutcome(null);
  }, [targetKey]);

  useLayoutEffect(() => {
    if (scrollAnchor.current && viewport.current) {
      viewport.current.scrollTop = scrollAnchor.current.top + viewport.current.scrollHeight - scrollAnchor.current.height;
      scrollAnchor.current = null;
    } else if (followLatest.current && viewport.current) viewport.current.scrollTop = viewport.current.scrollHeight;
  }, [messages, chat.pendingMessages.length, chat.isAgentWorking, targetKey, isVisible, promptDialogKey]);

  async function revealCodexThread() {
    setRevealOutcome(await chat.revealCodexThread());
  }

  async function openModelPicker() {
    if (snapshot?.modelSettings || snapshot?.modelPicker.supportsApi) { setModelPickerOpen((value) => !value); return; }
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
          <label className={`${controlClass} flex cursor-pointer items-center gap-2`}>
            <input type="checkbox" checked={preferences.showInstructions} disabled={!preferences.loaded || preferences.saving} onChange={(event) => void preferences.setShowInstructions(event.target.checked)} className="accent-brand-primary" />{t("showInstructions")}
          </label>
          <button type="button" onClick={onHistory} className={controlClass}>{t("history")}</button>
          <button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button>
        </div>
      </header>
      {chat.readError ? <div role="alert" className="flex items-center justify-between gap-3 px-4 py-2 text-xs text-status-error">{t("readError")}<button type="button" onClick={() => void chat.refresh()} className={controlClass}>{t("retry")}</button></div> : null}
      {preferences.error ? <p role="alert" className="px-4 py-2 text-xs text-status-error">{t("preferencesError")}</p> : null}
      <div className="relative flex min-h-0 flex-1 flex-col">
      <div ref={viewport} data-testid="pane-chat-transcript" className={`min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable] px-4 py-6 sm:px-8 ${isNewConversation ? "flex flex-col justify-end" : ""}`} onScroll={() => {
        const node = viewport.current;
        if (!node) return;
        followLatest.current = node.scrollHeight - node.scrollTop - node.clientHeight < NEAR_BOTTOM_PX;
        setIsDetached(!followLatest.current);
      }}>
        <div className="mx-auto w-full max-w-3xl space-y-2.5">
          {!snapshot && !chat.readError ? <p role="status" className="text-sm text-text-secondary">{t("loading")}</p> : null}
          {snapshot && !snapshot.canSend && !snapshot.provider ? <div className="space-y-4"><p className="text-sm text-text-secondary">{t("noAgent")}</p><button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button></div> : null}
          {snapshot?.canSend && snapshot.transcript.status === "available" ? <>
            {transcript.nextCursor ? <button type="button" disabled={transcript.loading} onClick={() => void transcript.loadOlder(() => {
              if (viewport.current) scrollAnchor.current = { top: viewport.current.scrollTop, height: viewport.current.scrollHeight };
              followLatest.current = false;
            })} className={controlClass}>{t(transcript.loading ? "loadingOlder" : "loadOlder")}</button> : null}
            {transcript.error ? <p role="alert" className="text-xs text-status-error">{t("olderError")}</p> : null}
            {isNewConversation ? <PaneChatWelcome provider={snapshot.provider!} /> : null}
            {getChatTranscriptRows(visibleMessages, snapshot.transcript.sessionId).map(({ message, key }) => (
              <AiChatTranscriptMessage key={key} message={message} provider={snapshot.provider!} attachmentContext={snapshot.transcript.sessionId ? { taskId, provider: snapshot.provider!, sessionId: snapshot.transcript.sessionId } : undefined} onOpenFile={(file) => setPreviewFilePath(file.path)} />
            ))}
          </> : null}
          {snapshot?.provider && snapshot.transcript.status === "unavailable" ? <div role="status" className="space-y-3 rounded-lg bg-bg-surface p-4 text-sm text-text-secondary">
            <p>{t(isWaitingForInput ? "transcriptWaiting" : "transcriptUnavailable")}</p>
            {isCodexFooterReason(snapshot.transcript.reason) ? <>
              {isWaitingForInput ? <p>{t("transcriptInputBusy")}</p> : null}
              <p>{t("codexDaemonLimit")}</p>
              <p>{t("codexRevealHint")}</p>
              <div className="flex flex-wrap items-center gap-2">
                <button type="button" disabled={isWaitingForInput || chat.isBusy} onClick={() => void revealCodexThread()} className="min-h-10 rounded-md bg-brand-primary px-3 text-xs text-text-inverse hover:bg-brand-hover focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40">{t(chat.isBusy ? "revealingThread" : "revealThread")}</button>
                {isWaitingForInput ? <span className="text-xs">{t("codexRevealBusy")}</span> : null}
              </div>
              {revealOutcome ? <p role={revealOutcome === "revealed" ? "status" : "alert"} className={revealOutcome === "revealed" ? "text-text-primary" : "text-status-warning"}>{t(REVEAL_OUTCOME_MESSAGES[revealOutcome])}</p> : null}
              <p>{t("codexNoDaemonHint")}</p>
            </> : <p>{t(describeUnavailableTranscript(snapshot.provider, snapshot.transcript.reason))}</p>}
            <div className="flex flex-wrap gap-2"><button type="button" onClick={() => void chat.refresh()} className={controlClass}>{t("retry")}</button><button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button></div>
          </div> : null}
          {snapshot?.screen && snapshot.transcript.status !== "available" ? <details className="rounded-lg border border-border-subtle bg-bg-surface">
            <summary className="cursor-pointer px-4 py-3 text-xs text-text-secondary">{t("screen")}</summary>
            <pre className="max-h-80 overflow-auto whitespace-pre px-4 pb-4 font-mono text-xs leading-5 text-text-primary">{snapshot.screen}</pre>
          </details> : null}
          {chat.pendingMessages?.map((pending) => <div key={pending.id} data-testid="pane-chat-pending-message">
            <AiChatTranscriptMessage message={pending.message} provider={snapshot?.provider ?? "codex"} />
            <p role="status" className="text-right text-xs text-text-secondary">{t(pending.state === "sending" ? "sending" : pending.state === "sent" ? "messageSent" : "sendError")}</p>
          </div>)}
          {chat.isAgentWorking && snapshot?.canSend ? <PaneChatWorkingIndicator provider={snapshot.provider} /> : null}
        </div>
      </div>
        {modelPickerOpen && snapshot?.targetToken ? <div className="pointer-events-none absolute inset-y-2 left-4 z-20 flex w-80 max-w-[calc(100%-2rem)] items-end sm:left-8"><PaneChatModelPicker key={snapshot.targetToken} taskId={taskId} targetToken={snapshot.targetToken} initialSettings={snapshot.modelSettings} onClose={(restoreFocus = true) => {
          setModelPickerOpen(false);
          if (restoreFocus) requestAnimationFrame(() => viewport.current?.closest("section")?.querySelector<HTMLButtonElement>("[data-pane-chat-model-trigger]")?.focus());
        }} onChanged={() => void chat.refresh()} /></div> : null}
      </div>
      {isDetached ? <button type="button" onClick={() => {
        followLatest.current = true;
        if (viewport.current) viewport.current.scrollTop = viewport.current.scrollHeight;
        setIsDetached(false);
      }} className="absolute bottom-44 left-1/2 -translate-x-1/2 rounded-full border border-border-default bg-bg-surface px-4 py-2 text-xs text-text-primary">{t("jumpToLatest")} ↓</button> : null}
      {previewFilePath ? <AiChatFilePreview taskId={taskId} filePath={previewFilePath} onClose={() => setPreviewFilePath(null)} /> : null}
      {/* 체크만 바뀐 같은 선택 화면에서는 카드를 다시 만들지 않아, 적던 글과 답을 보내지 못한 이유가 남는다. 다른 선택 화면이 뜨면 새 카드로 시작한다. */}
      {prompt ? <PaneChatPromptCard key={promptDialogKey} prompt={prompt} isBusy={chat.isBusy} error={chat.promptError?.dialogKey === promptDialogKey ? chat.promptError.message : null} onAnswer={(answer) => void chat.answerPrompt(prompt, answer)} onTerminal={onTerminal} /> : null}
      {/* 선택 화면이 떠 있는 동안 CLI는 메시지를 받지 않는다. 쓰던 글이 남도록 입력창은 지우지 않고 가려 둔다. */}
      <div className={`relative max-h-[70%] min-h-0 shrink-0 flex-col ${prompt ? "hidden" : "flex"}`}>
        <PaneChatComposer taskId={taskId} chat={chat} isVisible={isVisible} isModelPickerOpen={modelPickerOpen} onModel={() => void openModelPicker()} />
      </div>
      {/* 대화 영역과 같은 몫의 여백을 입력창 아래에 두어 첫 화면 안내와 입력창을 가운데로 모은다. */}
      {isNewConversation ? <div aria-hidden="true" className="flex-1" /> : null}
    </section>
  );
}
