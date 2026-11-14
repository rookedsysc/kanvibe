import { useState } from "react";
import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import { AI_SESSION_PROVIDER_LABELS } from "@/lib/aiSessions/providers";
import type { AggregatedAiMessage, AiMessageFile, AiSessionProvider } from "@/lib/aiSessions/types";
import { AiSessionMessageContent } from "./AiSessionMessageContent";
import { AiChatAttachment, type AiChatAttachmentContext } from "./AiChatAttachment";

export function AiChatTranscriptMessage({ message, provider, attachmentContext, onOpenFile }: {
  message: AggregatedAiMessage;
  provider: AiSessionProvider;
  attachmentContext?: AiChatAttachmentContext;
  /** AI가 만들거나 고친 파일의 카드를 눌렀을 때 미리보기를 연다. 없으면 카드를 그리지 않는다 */
  onOpenFile?: (file: AiMessageFile) => void;
}) {
  const t = useTranslations("taskDetail");
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const text = message.fullText || message.text;
  const isUser = message.role === "user";
  const isDisclosure = message.role === "tool" || message.role === "reasoning";
  const timestamp = message.timestamp ? new Date(message.timestamp) : null;
  const copyLabel = t(`aiSessions.${copyState === "idle" ? "copyMessage" : copyState === "copied" ? "copiedMessage" : "copyFailed"}`);

  async function copyMessage() {
    try {
      await navigator.clipboard.writeText(text);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
  }

  const meta = (
    <div className="mt-1 flex min-h-7 w-fit max-w-full flex-wrap items-center gap-x-2 text-xs text-text-secondary">
      {!isUser && <AiProviderIcon provider={provider} size={16} />}
      {!isUser && <span className="font-medium">{message.role === "assistant" ? AI_SESSION_PROVIDER_LABELS[provider] : t(`aiSessions.roles.${message.role}`)}</span>}
      {timestamp && Number.isFinite(timestamp.getTime()) ? (
        <time dateTime={message.timestamp!} title={timestamp.toLocaleString()} className="text-text-secondary tabular-nums">
          {timestamp.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
        </time>
      ) : null}
      {text ? <button
        type="button"
        onClick={() => void copyMessage()}
        className="min-h-7 rounded px-2 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-surface hover:text-text-primary focus-visible:outline-2 focus-visible:outline-brand-primary"
        aria-label={copyLabel}
      >
        <span aria-live="polite">{copyLabel}</span>
      </button> : null}
    </div>
  );
  const fileCards = onOpenFile && message.files?.length ? (
    <ul className="mt-2 flex flex-wrap gap-2" aria-label={t("aiSessions.changedFiles")}>
      {message.files.map((file, index) => (
        <li key={`${file.path}:${index}`} className="min-w-0 max-w-full">
          <button
            type="button"
            onClick={() => onOpenFile(file)}
            title={file.path}
            className="flex min-h-11 max-w-full items-center gap-3 rounded-lg border border-border-default bg-bg-surface px-3 py-2 text-left hover:bg-bg-page focus-visible:outline-2 focus-visible:outline-brand-primary"
          >
            <span className="min-w-0">
              <span className="block truncate text-sm font-medium text-text-primary">{file.path.split("/").at(-1)}</span>
              <span className="block text-xs text-text-secondary">{t(`aiSessions.fileChange.${file.change}`)}</span>
            </span>
          </button>
        </li>
      ))}
    </ul>
  ) : null;
  const content = !text ? null : isUser ? <p className="whitespace-pre-wrap break-words">{text}</p> : <AiSessionMessageContent text={text} />;
  const body = <>{content}{message.attachments?.map((attachment, index) => <AiChatAttachment key={`${attachment.id}:${index}`} attachment={attachment} context={attachmentContext} />)}</>;

  if (isDisclosure) {
    return (
      <div data-message-role={message.role}>
        <details className="group rounded-lg border border-border-subtle bg-bg-surface px-4 py-2">
          <summary className="cursor-pointer py-1 text-xs font-medium text-text-secondary focus-visible:outline-2 focus-visible:outline-brand-primary">
            {message.toolName ?? t(`aiSessions.roles.${message.role}`)}
            <span className="ml-2 inline-block max-w-[65%] truncate align-bottom font-normal text-text-secondary">{text.split("\n")[0]}</span>
          </summary>
          <div className="mt-2 min-w-0 pb-2 text-sm leading-6 text-text-primary">{body}{meta}</div>
        </details>
        {fileCards}
      </div>
    );
  }

  return (
    <article className={`flex min-w-0 ${isUser ? "justify-end" : "justify-start"}`} data-message-role={message.role} aria-label={isUser ? t("aiSessions.roles.user") : undefined}>
      <div className={isUser
        ? "min-w-0 max-w-[80%] rounded-2xl rounded-br-md bg-bg-surface px-4 py-2 text-sm leading-6 text-text-primary"
        : "min-w-0 w-full py-1 text-sm leading-7 text-text-primary"}>
        {body}
        {fileCards}
        {meta}
      </div>
    </article>
  );
}
