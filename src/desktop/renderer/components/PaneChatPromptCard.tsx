import { useState, type FormEvent } from "react";
import { useTranslations } from "next-intl";
import { isBlankClaudeTextEntry, type PaneChatPromptOption } from "@/lib/aiSessions/paneChatClaudePrompt";
import { MAX_PANE_CHAT_PROMPT_OPTION_NUMBER, MAX_PANE_CHAT_PROMPT_TEXT_LENGTH, type PaneChatPrompt, type PaneChatPromptAnswer } from "@/desktop/shared/paneChat";

const KIND_LABELS: Record<PaneChatPrompt["kind"], string> = { question: "promptQuestion", permission: "promptPermission", confirmation: "promptConfirmation" };
const controlClass = "min-h-9 rounded-md border border-border-default bg-bg-surface px-3 text-xs text-text-primary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40";
const numberBadgeClass = "flex h-6 w-6 shrink-0 items-center justify-center rounded bg-bg-page text-xs tabular-nums text-text-secondary";

/** 고르면 글을 직접 입력받는 선택지. 카드에서 글을 적어 제출하면 CLI의 그 줄에 넣고 답한다 */
function PaneChatPromptTextEntry({ option, isBusy, onSubmit }: { option: PaneChatPromptOption; isBusy: boolean; onSubmit: (text: string) => void }) {
  const t = useTranslations("taskDetail.paneChat");
  const [text, setText] = useState("");
  function submit(event: FormEvent) {
    event.preventDefault();
    if (text.trim()) onSubmit(text.trim());
  }
  return (
    <form onSubmit={submit} className="flex items-center gap-2 rounded-md border border-border-default bg-bg-surface px-2 py-1.5">
      <span aria-hidden="true" className={numberBadgeClass}>{option.number}</span>
      <input type="text" aria-label={t("promptTextPlaceholder")} placeholder={t("promptTextPlaceholder")} maxLength={MAX_PANE_CHAT_PROMPT_TEXT_LENGTH} value={text} disabled={isBusy} onChange={(event) => setText(event.target.value)} className="min-w-0 flex-1 bg-transparent text-sm text-text-primary outline-none placeholder:text-text-secondary" />
      <button type="submit" disabled={isBusy || !text.trim()} className={controlClass}>{t("promptSubmitText")}</button>
    </form>
  );
}

/**
 * CLI가 답을 기다리며 띄운 선택 화면을 채팅에서 보여 주고 답하게 하는 카드.
 * AI의 질문, 도구 실행 허용 요청, CLI 스스로의 확인을 같은 모양으로 다루고, 고른 답은 CLI 화면에 키 입력으로 전달된다.
 */
export function PaneChatPromptCard({ prompt, isBusy, error, onAnswer, onTerminal }: {
  prompt: PaneChatPrompt;
  isBusy: boolean;
  error: string | null;
  onAnswer: (answer: PaneChatPromptAnswer) => void;
  onTerminal: () => void;
}) {
  const t = useTranslations("taskDetail.paneChat");
  const kindLabel = t(KIND_LABELS[prompt.kind]);

  return (
    <section data-testid="pane-chat-prompt" aria-label={kindLabel} className="mx-auto flex max-h-[70%] min-h-0 w-full max-w-3xl shrink-0 flex-col gap-2 overflow-y-auto px-4 pb-3 sm:px-8">
      <div className="flex flex-col gap-2 rounded-xl border border-border-default bg-bg-surface p-3">
        <div className="flex flex-wrap items-center gap-2 text-xs text-text-secondary">
          <span className="rounded bg-brand-subtle px-2 py-0.5 font-medium text-text-brand">{kindLabel}</span>
          {prompt.stepsLine ? <span className="font-mono">{prompt.stepsLine}</span> : null}
        </div>
        {prompt.detail ? <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded-md bg-bg-page px-3 py-2 font-mono text-xs leading-5 text-text-primary">{prompt.detail}</pre> : null}
        <p className="whitespace-pre-line text-sm font-medium text-text-primary">{prompt.title}</p>
        {prompt.hasHiddenOptions ? <p role="status" className="text-xs text-text-secondary">{t("promptHiddenOptions")}</p> : null}
        <ul className="flex flex-col gap-1.5">
          {prompt.options.map((option) => <li key={option.number}>
            {isBlankClaudeTextEntry(option) ? <PaneChatPromptTextEntry option={option} isBusy={isBusy} onSubmit={(text) => onAnswer({ type: "text", number: option.number, text })} />
              : option.isTextEntry ? <div className="flex items-center gap-2 rounded-md border border-border-default px-2 py-1.5 text-sm text-text-primary">
                <span aria-hidden="true" className={numberBadgeClass}>{option.number}</span>
                <span className="min-w-0 flex-1"><span className="block break-words">{option.label}</span><span className="block text-xs text-text-secondary">{t("promptTextEntered")}</span></span>
                {/* 다중 선택은 글을 넣은 순간 그 줄이 체크되어 이미 답이다. 단일 선택만 그 글로 답하는 동작이 남는다. */}
                {prompt.isMultiSelect ? null : <button type="button" disabled={isBusy} onClick={() => onAnswer({ type: "text", number: option.number, text: option.label })} className={controlClass}>{t("promptSubmitText")}</button>}
              </div>
              // 다중 선택은 답을 보내는 중에 누른 다른 선택지도 차례로 전달되므로 잠그지 않는다
              : <button type="button" disabled={(isBusy && !prompt.isMultiSelect) || option.number > MAX_PANE_CHAT_PROMPT_OPTION_NUMBER} aria-pressed={prompt.isMultiSelect ? option.isChecked : undefined} onClick={() => onAnswer({ type: "option", number: option.number })} className={`flex w-full items-start gap-2 rounded-md border px-2 py-1.5 text-left text-sm text-text-primary hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary disabled:opacity-40 ${option.isChecked ? "border-brand-primary bg-brand-subtle" : "border-border-default bg-bg-surface"}`}>
                <span aria-hidden="true" className={numberBadgeClass}>{option.number}</span>
                <span className="min-w-0 flex-1"><span className="block break-words">{option.label}</span>{option.description ? <span className="block break-words text-xs text-text-secondary">{option.description}</span> : null}</span>
                {option.isChecked ? <span aria-hidden="true" className="text-text-brand">✔</span> : null}
              </button>}
          </li>)}
        </ul>
        {error ? <p role="alert" className="text-xs text-status-error">{error}</p> : null}
        <div className="flex flex-wrap items-center gap-2">
          {prompt.stepsLine ? <>
            <button type="button" disabled={isBusy} onClick={() => onAnswer({ type: "step", direction: "previous" })} className={controlClass}>{t("promptPrevious")}</button>
            <button type="button" disabled={isBusy} onClick={() => onAnswer({ type: "step", direction: "next" })} className={controlClass}>{t("promptNext")}</button>
          </> : null}
          <button type="button" disabled={isBusy} onClick={() => onAnswer({ type: "cancel" })} className={`${controlClass} ml-auto`}>{t("cancel")}</button>
          <button type="button" onClick={onTerminal} className={controlClass}>{t("terminal")}</button>
        </div>
      </div>
    </section>
  );
}
